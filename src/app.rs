use crate::db::Database;
use crate::i18n::I18n;
use crate::importer::{ColumnMapping, ImportWorkbook, extract_records, load_spreadsheet};
use crate::models::{
    BarcodeKind, BarcodeRecord, Group, LabelTemplate, Rotation, TextAlign,
    ensure_default_templates, list_templates, load_template, parse_flexible_f32, save_template,
};
use crate::printer::{list_system_printers, print_records};
use crate::renderer::{FontEngine, RgbaImage, WHITE, render_roll_rows, render_single_label};
use eframe::egui::{self, Color32, ColorImage, RichText, Sense, TextureHandle, TextureOptions};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Records,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusLevel {
    Info,
    Warning,
    Error,
}

struct NumeratorDialog {
    open: bool,
    prefix: String,
    start_num: u64,
    count: u32,
    step: u64,
    pad_digits: usize,
    suffix: String,
    desc1: String,
    desc2: String,
    desc3: String,
    desc4: String,
    copies: u32,
}

impl Default for NumeratorDialog {
    fn default() -> Self {
        Self {
            open: false,
            prefix: String::new(),
            start_num: 1,
            count: 20,
            step: 1,
            pad_digits: 6,
            suffix: String::new(),
            desc1: String::new(),
            desc2: String::new(),
            desc3: String::new(),
            desc4: String::new(),
            copies: 1,
        }
    }
}

#[derive(Default)]
struct ImportDialog {
    open: bool,
    workbook: Option<ImportWorkbook>,
    selected_sheet: usize,
    mapping: Option<ColumnMapping>,
}

struct PrintPreviewModal {
    open: bool,
    pages: Vec<RgbaImage>,
    current_page: usize,
    zoom: f32,
    texture: Option<TextureHandle>,
    texture_page_idx: Option<usize>,
    target_records: Vec<BarcodeRecord>,
}

impl Default for PrintPreviewModal {
    fn default() -> Self {
        Self {
            open: false,
            pages: Vec::new(),
            current_page: 0,
            zoom: 1.0,
            texture: None,
            texture_page_idx: None,
            target_records: Vec::new(),
        }
    }
}

pub struct BarkodStudyoApp {
    db: Database,
    i18n: I18n,
    font_engine: FontEngine,
    templates_dir: PathBuf,

    // Sekme ve durum
    active_tab: ActiveTab,
    status_msg: String,
    status_level: StatusLevel,

    // Gruplar ve kayıtlar
    groups: Vec<Group>,
    active_group_id: i64,
    records: Vec<BarcodeRecord>,
    filtered_indices: Vec<usize>,
    search_query: String,
    range_start: usize,
    range_end: usize,
    selected_record_id: Option<i64>,
    editor_record: BarcodeRecord,
    focus_barcode_input: bool,

    // Şablonlar ve yazıcılar
    template_files: Vec<String>,
    active_template_file: String,
    template: LabelTemplate,
    printers: Vec<String>,
    selected_printer: String,

    // Esnek ondalık metin giriş tamponları (virgül ve nokta desteği için)
    float_buffers: BTreeMap<String, String>,

    // Canlı Önizleme önbelleği
    preview_dirty: bool,
    preview_show_full_row: bool,
    preview_zoom: f32,
    live_preview_tex: Option<TextureHandle>,
    live_preview_size_px: [u32; 2],

    // Diyalog pencereleri
    new_group_open: bool,
    new_group_name: String,
    new_tpl_open: bool,
    new_tpl_name: String,
    numerator_dlg: NumeratorDialog,
    import_dlg: ImportDialog,
    preview_modal: PrintPreviewModal,
}

impl BarkodStudyoApp {
    pub fn new(
        db: Database,
        i18n: I18n,
        font_engine: FontEngine,
        templates_dir: PathBuf,
    ) -> Self {
        let _ = ensure_default_templates(&templates_dir);
        let template_files = list_templates(&templates_dir);
        let active_template_file = template_files
            .iter()
            .find(|n| n.as_str() == "1_Sutun_Yatay.json")
            .cloned()
            .or_else(|| template_files.first().cloned())
            .unwrap_or_else(|| "1_Sutun_Yatay.json".to_string());

        let template = load_template(&templates_dir, &active_template_file)
            .unwrap_or_else(|_| LabelTemplate::default_1_col_horizontal());

        let groups = db.list_groups().unwrap_or_default();
        let active_group_id = groups.first().map(|g| g.id).unwrap_or(1);

        let printers = list_system_printers();
        let selected_printer = printers.first().cloned().unwrap_or_default();

        let status_ready = i18n.t("status_ready").to_string();

        let mut app = Self {
            db,
            i18n,
            font_engine,
            templates_dir,
            active_tab: ActiveTab::Records,
            status_msg: status_ready,
            status_level: StatusLevel::Info,
            groups,
            active_group_id,
            records: Vec::new(),
            filtered_indices: Vec::new(),
            search_query: String::new(),
            range_start: 1,
            range_end: 10,
            selected_record_id: None,
            editor_record: BarcodeRecord::new_empty(active_group_id),
            focus_barcode_input: false,
            template_files,
            active_template_file,
            template,
            printers,
            selected_printer,
            float_buffers: BTreeMap::new(),
            preview_dirty: true,
            preview_show_full_row: false,
            preview_zoom: 1.0,
            live_preview_tex: None,
            live_preview_size_px: [1, 1],
            new_group_open: false,
            new_group_name: String::new(),
            new_tpl_open: false,
            new_tpl_name: String::new(),
            numerator_dlg: NumeratorDialog::default(),
            import_dlg: ImportDialog::default(),
            preview_modal: PrintPreviewModal::default(),
        };

        app.reload_records();
        app
    }

    fn set_status(&mut self, level: StatusLevel, msg: impl Into<String>) {
        self.status_level = level;
        self.status_msg = msg.into();
    }

    fn reload_groups(&mut self) {
        if let Ok(list) = self.db.list_groups() {
            self.groups = list;
            if !self.groups.iter().any(|g| g.id == self.active_group_id)
                && let Some(first) = self.groups.first()
            {
                self.active_group_id = first.id;
            }
        }
    }

    fn reload_records(&mut self) {
        if let Ok(list) = self.db.list_records(self.active_group_id) {
            self.records = list;
        }
        self.rebuild_filter();

        if let Some(sel_id) = self.selected_record_id {
            if let Some(found) = self.records.iter().find(|r| r.id == Some(sel_id)) {
                self.editor_record = found.clone();
            } else {
                self.selected_record_id = None;
                self.editor_record = BarcodeRecord::new_empty(self.active_group_id);
            }
        } else if let Some(first) = self.records.first() {
            self.selected_record_id = first.id;
            self.editor_record = first.clone();
        } else {
            self.editor_record = BarcodeRecord::new_empty(self.active_group_id);
        }

        self.preview_dirty = true;
    }

    fn rebuild_filter(&mut self) {
        let q = turkish_lowercase(self.search_query.trim());
        if q.is_empty() {
            self.filtered_indices = (0..self.records.len()).collect();
        } else {
            self.filtered_indices = self
                .records
                .iter()
                .enumerate()
                .filter(|(_, rec)| {
                    turkish_lowercase(&rec.barcode).contains(&q)
                        || turkish_lowercase(&rec.desc1).contains(&q)
                        || turkish_lowercase(&rec.desc2).contains(&q)
                        || turkish_lowercase(&rec.desc3).contains(&q)
                        || turkish_lowercase(&rec.desc4).contains(&q)
                })
                .map(|(idx, _)| idx)
                .collect();
        }

        if !self.filtered_indices.is_empty() {
            let max_len = self.filtered_indices.len();
            self.range_start = self.range_start.clamp(1, max_len);
            if self.range_end == 0 || self.range_end > max_len {
                self.range_end = max_len.min(10).max(self.range_start);
            }
        }
    }

    fn records_in_selected_range(&self) -> Vec<BarcodeRecord> {
        let selected_indices = slice_1based_inclusive(
            &self.filtered_indices,
            self.range_start,
            self.range_end,
        );
        selected_indices
            .into_iter()
            .filter_map(|idx| self.records.get(idx).cloned())
            .collect()
    }

    fn active_record_for_preview(&self) -> BarcodeRecord {
        if !self.editor_record.barcode.trim().is_empty() {
            return self.editor_record.clone();
        }
        if let Some(sel_id) = self.selected_record_id
            && let Some(rec) = self.records.iter().find(|r| r.id == Some(sel_id))
        {
            return rec.clone();
        }
        if let Some(first) = self.records.first() {
            return first.clone();
        }
        let mut sample = BarcodeRecord::sample_preview();
        if self.template.barcode_kind == BarcodeKind::Ean13 {
            sample.barcode = "869123456789".to_string();
        }
        sample
    }

    fn refresh_live_preview_if_needed(&mut self, ctx: &egui::Context) {
        if !self.preview_dirty && self.live_preview_tex.is_some() {
            return;
        }
        self.preview_dirty = false;

        let preview_dpi = self.template.dpi_override.unwrap_or(203).clamp(72, 600);
        let (rgba, warn) = if self.preview_show_full_row && self.template.columns > 1 {
            let source_records: Vec<BarcodeRecord> = if !self.records.is_empty() {
                self.records
                    .iter()
                    .take(self.template.columns as usize)
                    .cloned()
                    .collect()
            } else {
                let mut sample = self.active_record_for_preview();
                sample.print_count = self.template.columns;
                vec![sample]
            };
            let (mut rows, w) =
                render_roll_rows(&source_records, &self.template, preview_dpi, &self.font_engine, true);
            let img = rows
                .pop()
                .unwrap_or_else(|| RgbaImage::from_pixel(100, 50, WHITE));
            (img, w)
        } else {
            let rec = self.active_record_for_preview();
            let out = render_single_label(&rec, &self.template, preview_dpi, &self.font_engine);
            (out.image, out.warning)
        };

        if let Some(w_msg) = warn {
            self.set_status(StatusLevel::Warning, w_msg);
        }

        self.live_preview_size_px = [rgba.width(), rgba.height()];
        let color_img = ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        self.live_preview_tex =
            Some(ctx.load_texture("live_label_preview", color_img, TextureOptions::LINEAR));
    }

    fn open_print_preview(&mut self, records_to_preview: Vec<BarcodeRecord>) {
        if records_to_preview.is_empty() {
            let msg = self.i18n.t("err_no_records").to_string();
            self.set_status(StatusLevel::Warning, msg);
            return;
        }
        let preview_dpi = self.template.dpi_override.unwrap_or(203).clamp(72, 600);
        let (pages, warn) = render_roll_rows(
            &records_to_preview,
            &self.template,
            preview_dpi,
            &self.font_engine,
            true,
        );
        if let Some(w) = warn {
            self.set_status(StatusLevel::Warning, w);
        }
        if pages.is_empty() {
            let msg = self.i18n.t("err_no_records").to_string();
            self.set_status(StatusLevel::Warning, msg);
            return;
        }
        self.preview_modal.pages = pages;
        self.preview_modal.current_page = 0;
        self.preview_modal.texture = None;
        self.preview_modal.texture_page_idx = None;
        self.preview_modal.target_records = records_to_preview;
        self.preview_modal.open = true;
    }

    fn execute_print(&mut self, target_records: &[BarcodeRecord]) {
        if target_records.is_empty() {
            let msg = self.i18n.t("err_no_records").to_string();
            self.set_status(StatusLevel::Warning, msg);
            return;
        }
        if self.selected_printer.trim().is_empty() {
            let msg = self.i18n.t("err_no_printer").to_string();
            self.set_status(StatusLevel::Error, msg);
            return;
        }

        match print_records(
            &self.selected_printer,
            target_records,
            &self.template,
            &self.font_engine,
        ) {
            Ok(summary) => {
                let suffix = self.i18n.t("msg_printed");
                let info = format!(
                    "{} sayfa ({} DPI - {}) {}",
                    summary.pages_sent, summary.dpi_used, self.selected_printer, suffix
                );
                if let Some(w) = summary.warning {
                    self.set_status(StatusLevel::Warning, format!("{info} | Uyarı: {w}"));
                } else {
                    self.set_status(StatusLevel::Info, info);
                }
            }
            Err(err) => {
                self.set_status(StatusLevel::Error, format!("Yazdırma hatası: {err}"));
            }
        }
    }

    /// Hem virgül (,) hem nokta (.) kabul eden sayısal metin kutusu yardımcısı.
    fn flexible_f32_edit(
        float_buffers: &mut BTreeMap<String, String>,
        ui: &mut egui::Ui,
        id_key: &str,
        value: &mut f32,
        min: f32,
        max: f32,
    ) -> bool {
        let mut changed = false;
        let entry = float_buffers
            .entry(id_key.to_string())
            .or_insert_with(|| format_clean_f32(*value));

        let resp = ui.add_sized([76.0, 22.0], egui::TextEdit::singleline(entry));
        if resp.changed() {
            if let Some(parsed) = parse_flexible_f32(entry) {
                let clamped = parsed.clamp(min, max);
                if (*value - clamped).abs() > f32::EPSILON {
                    *value = clamped;
                    changed = true;
                }
            }
        } else if !resp.has_focus() {
            // Odak dışında şablon değiştiyse tamponu senkronize et
            let expected = format_clean_f32(*value);
            if *entry != expected {
                *entry = expected;
            }
        }

        if resp.lost_focus() {
            *entry = format_clean_f32(*value);
        }

        changed
    }

    fn draw_top_toolbar(&mut self, ui: &mut egui::Ui) {
        // 1. Satır: Tümünü / Aktif Kaydı / Belirli Sıra Aralığını Önizle & Yazdır + Dil Seçici
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            if ui.button(format!("🔍 {}", self.i18n.t("btn_preview_all"))).clicked() {
                let all: Vec<BarcodeRecord> = self
                    .filtered_indices
                    .iter()
                    .filter_map(|&idx| self.records.get(idx).cloned())
                    .collect();
                self.open_print_preview(all);
            }
            if ui.button(format!("🖨 {}", self.i18n.t("btn_print_all"))).clicked() {
                let all: Vec<BarcodeRecord> = self
                    .filtered_indices
                    .iter()
                    .filter_map(|&idx| self.records.get(idx).cloned())
                    .collect();
                self.execute_print(&all);
            }

            ui.separator();

            if ui
                .button(format!("👁 {}", self.i18n.t("btn_preview_active")))
                .clicked()
            {
                if self.editor_record.barcode.trim().is_empty() && self.selected_record_id.is_none()
                {
                    let msg = self.i18n.t("err_no_active_record").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                } else {
                    let rec = self.active_record_for_preview();
                    self.open_print_preview(vec![rec]);
                }
            }
            if ui
                .button(format!("🖨 {}", self.i18n.t("btn_print_active")))
                .clicked()
            {
                if self.editor_record.barcode.trim().is_empty() && self.selected_record_id.is_none()
                {
                    let msg = self.i18n.t("err_no_active_record").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                } else {
                    let rec = self.active_record_for_preview();
                    self.execute_print(&[rec]);
                }
            }

            ui.separator();

            // Belirli sıra aralığı (ör. 1 - 10) Önizle ve Yazdır
            let max_rows = self.filtered_indices.len().max(1);
            ui.label(RichText::new(self.i18n.t("lbl_range")).strong());
            ui.add(
                egui::DragValue::new(&mut self.range_start)
                    .range(1..=max_rows)
                    .speed(0.2),
            );
            ui.label("-");
            ui.add(
                egui::DragValue::new(&mut self.range_end)
                    .range(1..=max_rows)
                    .speed(0.2),
            );

            if ui
                .button(format!("🔍 {}", self.i18n.t("btn_preview_range")))
                .clicked()
            {
                let sub = self.records_in_selected_range();
                if sub.is_empty() {
                    let msg = self.i18n.t("err_empty_range").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                } else {
                    self.open_print_preview(sub);
                }
            }

            if ui
                .button(format!("🖨 {}", self.i18n.t("btn_print_range")))
                .clicked()
            {
                let sub = self.records_in_selected_range();
                if sub.is_empty() {
                    let msg = self.i18n.t("err_empty_range").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                } else {
                    self.execute_print(&sub);
                }
            }

            // Sağ köşede dinamik Dil Seçici
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current_code = self.i18n.active_lang().to_uppercase();
                let resp = egui::ComboBox::from_id_salt("top_lang_combo")
                    .selected_text(current_code)
                    .width(115.0)
                    .show_ui(ui, |ui| {
                        for (code, label) in self.i18n.available_languages() {
                            if ui
                                .selectable_label(self.i18n.active_lang() == code, label)
                                .clicked()
                            {
                                self.i18n.set_active_lang(&code);
                            }
                        }
                    });
                if resp.response.clicked() {
                    // Kullanıcı klasöre yeni bir .json dil dosyası attıysa anında algıla
                    self.i18n.reload_external_locales();
                }
                ui.label(self.i18n.t("lbl_language"));
            });
        });

        ui.separator();

        // 2. Satır: Sekme Seçiciler + Barkod Türü + Şablon Profili Yönetimi
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            ui.selectable_value(
                &mut self.active_tab,
                ActiveTab::Records,
                RichText::new(format!("📋 {}", self.i18n.t("tab_records"))).strong(),
            );
            ui.selectable_value(
                &mut self.active_tab,
                ActiveTab::Settings,
                RichText::new(format!("⚙ {}", self.i18n.t("tab_settings"))).strong(),
            );

            ui.separator();

            // Barkod Türü Seçici
            ui.label(self.i18n.t("lbl_barcode_kind"));
            egui::ComboBox::from_id_salt("top_barcode_kind")
                .selected_text(self.template.barcode_kind.display_name())
                .width(105.0)
                .show_ui(ui, |ui| {
                    for kind in BarcodeKind::ALL {
                        if ui
                            .selectable_value(
                                &mut self.template.barcode_kind,
                                *kind,
                                kind.display_name(),
                            )
                            .changed()
                        {
                            self.preview_dirty = true;
                        }
                    }
                });

            ui.separator();

            // Şablon Seçici ve Kaydet / Yeni butonları
            ui.label(self.i18n.t("lbl_template"));
            let current_tpl_name = self.active_template_file.clone();
            egui::ComboBox::from_id_salt("top_template_combo")
                .selected_text(&current_tpl_name)
                .width(175.0)
                .show_ui(ui, |ui| {
                    for fname in self.template_files.clone() {
                        if ui
                            .selectable_label(self.active_template_file == fname, &fname)
                            .clicked()
                        {
                            match load_template(&self.templates_dir, &fname) {
                                Ok(loaded) => {
                                    self.template = loaded;
                                    self.active_template_file = fname;
                                    self.float_buffers.clear();
                                    self.preview_dirty = true;
                                }
                                Err(e) => {
                                    self.set_status(
                                        StatusLevel::Error,
                                        format!("Şablon yüklenemedi: {e}"),
                                    );
                                }
                            }
                        }
                    }
                });

            if ui
                .button(format!("💾 {}", self.i18n.t("btn_save_template")))
                .clicked()
            {
                match save_template(
                    &self.templates_dir,
                    &self.active_template_file,
                    &self.template,
                ) {
                    Ok(()) => {
                        let msg = format!(
                            "{} {}",
                            self.i18n.t("msg_saved_template"),
                            self.active_template_file
                        );
                        self.set_status(StatusLevel::Info, msg);
                    }
                    Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                }
            }

            if ui
                .button(format!("➕ {}", self.i18n.t("btn_new_template")))
                .clicked()
            {
                self.new_tpl_name = "Yeni_Sablon.json".to_string();
                self.new_tpl_open = true;
            }
        });
    }

    fn draw_records_tab(&mut self, ui: &mut egui::Ui) {
        // 1. Üst arama/filtreleme çubuğu
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.i18n.t("lbl_search")).strong());
            let search_resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text(self.i18n.t("hint_search"))
                    .desired_width(340.0),
            );
            if search_resp.changed() {
                self.rebuild_filter();
            }
            if !self.search_query.is_empty() && ui.button(self.i18n.t("btn_clear")).clicked() {
                self.search_query.clear();
                self.rebuild_filter();
            }
        });

        ui.add_space(4.0);

        // Alt paneller için yer bırakarak sanal tablo yüksekliğini hesapla
        let available_h = (ui.available_height() - 165.0).max(140.0);

        // 2. Sanal Tablo (egui_extras::TableBuilder — binlerce satırda sıfır kasma)
        let mut clicked_record_idx: Option<usize> = None;
        let mut double_clicked = false;

        ui.push_id("records_virtual_table", |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .sense(Sense::click())
                .min_scrolled_height(100.0)
                .max_scroll_height(available_h)
                .column(Column::initial(44.0).at_least(36.0))
                .column(Column::initial(165.0).at_least(100.0))
                .column(Column::initial(180.0).at_least(90.0))
                .column(Column::initial(180.0).at_least(90.0))
                .column(Column::initial(150.0).at_least(80.0))
                .column(Column::remainder().at_least(90.0))
                .column(Column::initial(64.0).at_least(50.0))
                .header(24.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("#");
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_barcode"));
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_desc1"));
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_desc2"));
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_desc3"));
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_desc4"));
                    });
                    header.col(|ui| {
                        ui.strong(self.i18n.t("col_count"));
                    });
                })
                .body(|body| {
                    let row_count = self.filtered_indices.len();
                    body.rows(22.0, row_count, |mut row| {
                        let filtered_pos = row.index();
                        let rec_idx = self.filtered_indices[filtered_pos];
                        let rec = &self.records[rec_idx];
                        let is_selected = rec.id.is_some() && rec.id == self.selected_record_id;
                        row.set_selected(is_selected);

                        let mut any_click = false;
                        let mut any_dbl = false;

                        let (_, r0) = row.col(|ui| {
                            ui.label(format!("{}", filtered_pos + 1));
                        });
                        let (_, r1) = row.col(|ui| {
                            ui.label(&rec.barcode);
                        });
                        let (_, r2) = row.col(|ui| {
                            ui.label(&rec.desc1);
                        });
                        let (_, r3) = row.col(|ui| {
                            ui.label(&rec.desc2);
                        });
                        let (_, r4) = row.col(|ui| {
                            ui.label(&rec.desc3);
                        });
                        let (_, r5) = row.col(|ui| {
                            ui.label(&rec.desc4);
                        });
                        let (_, r6) = row.col(|ui| {
                            ui.label(format!("{}", rec.print_count));
                        });

                        for r in [&r0, &r1, &r2, &r3, &r4, &r5, &r6] {
                            if r.clicked() {
                                any_click = true;
                            }
                            if r.double_clicked() {
                                any_dbl = true;
                            }
                        }
                        if row.response().clicked() {
                            any_click = true;
                        }
                        if row.response().double_clicked() {
                            any_dbl = true;
                        }

                        if any_click || any_dbl {
                            clicked_record_idx = Some(rec_idx);
                            if any_dbl {
                                double_clicked = true;
                            }
                        }
                    });
                });
        });

        if let Some(idx) = clicked_record_idx
            && let Some(rec) = self.records.get(idx)
        {
            self.selected_record_id = rec.id;
            self.editor_record = rec.clone();
            self.preview_dirty = true;
            if double_clicked {
                self.focus_barcode_input = true;
            }
        }

        ui.separator();

        // 3. Alt Düzenleme Paneli
        egui::Grid::new("record_editor_grid")
            .num_columns(6)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new(self.i18n.t("col_barcode")).strong());
                let bc_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.editor_record.barcode)
                        .desired_width(180.0),
                );
                if self.focus_barcode_input {
                    bc_edit.request_focus();
                    self.focus_barcode_input = false;
                }
                if bc_edit.changed() {
                    self.preview_dirty = true;
                }

                ui.label(self.i18n.t("col_desc1"));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.editor_record.desc1)
                            .desired_width(210.0),
                    )
                    .changed()
                {
                    self.preview_dirty = true;
                }

                ui.label(self.i18n.t("col_desc2"));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.editor_record.desc2)
                            .desired_width(210.0),
                    )
                    .changed()
                {
                    self.preview_dirty = true;
                }
                ui.end_row();

                ui.label(self.i18n.t("col_count"));
                if ui
                    .add(
                        egui::DragValue::new(&mut self.editor_record.print_count)
                            .range(1..=10_000),
                    )
                    .changed()
                {
                    self.preview_dirty = true;
                }

                ui.label(self.i18n.t("col_desc3"));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.editor_record.desc3)
                            .desired_width(210.0),
                    )
                    .changed()
                {
                    self.preview_dirty = true;
                }

                ui.label(self.i18n.t("col_desc4"));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.editor_record.desc4)
                            .desired_width(210.0),
                    )
                    .changed()
                {
                    self.preview_dirty = true;
                }
                ui.end_row();
            });

        ui.add_space(6.0);

        // 4. İşlem Çubuğu (Sol: Kayıt & Aktarım & Numaratör | Sağ: Grup Yönetimi)
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("📄 {}", self.i18n.t("btn_new"))).clicked() {
                self.selected_record_id = None;
                self.editor_record = BarcodeRecord::new_empty(self.active_group_id);
                self.focus_barcode_input = true;
                self.preview_dirty = true;
            }

            if ui.button(format!("💾 {}", self.i18n.t("btn_save"))).clicked() {
                self.editor_record.group_id = self.active_group_id;
                if self.editor_record.barcode.trim().is_empty() {
                    let msg = self.i18n.t("err_empty_barcode").to_string();
                    self.set_status(StatusLevel::Error, msg);
                } else {
                    match self.db.save_record(&self.editor_record) {
                        Ok(saved_id) => {
                            self.selected_record_id = Some(saved_id);
                            self.reload_records();
                            let msg = self.i18n.t("msg_saved_record").to_string();
                            self.set_status(StatusLevel::Info, msg);
                        }
                        Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                    }
                }
            }

            if ui.button(format!("🗑 {}", self.i18n.t("btn_delete"))).clicked()
                && let Some(sel_id) = self.selected_record_id
            {
                match self.db.delete_record(sel_id) {
                    Ok(()) => {
                        self.selected_record_id = None;
                        self.reload_records();
                        let msg = self.i18n.t("msg_deleted_record").to_string();
                        self.set_status(StatusLevel::Info, msg);
                    }
                    Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                }
            }

            if ui.button(self.i18n.t("btn_delete_all")).clicked()
                && self.db.clear_group_records(self.active_group_id).is_ok()
            {
                self.selected_record_id = None;
                self.reload_records();
                let msg = self.i18n.t("msg_cleared_group").to_string();
                self.set_status(StatusLevel::Info, msg);
            }

            ui.separator();

            if ui
                .button(format!("📊 {}", self.i18n.t("btn_import_excel")))
                .clicked()
            {
                self.pick_and_open_import_file();
            }

            if ui
                .button(format!("🔢 {}", self.i18n.t("btn_numerator")))
                .clicked()
            {
                self.numerator_dlg.open = true;
            }

            // Sağ taraf: Grup Yönetimi
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(self.i18n.t("btn_delete_group")).clicked() {
                    if self.groups.len() <= 1 {
                        let msg = self.i18n.t("err_cannot_delete_last_group").to_string();
                        self.set_status(StatusLevel::Warning, msg);
                    } else if let Err(e) = self.db.delete_group(self.active_group_id) {
                        self.set_status(StatusLevel::Error, format!("{e}"));
                    } else {
                        self.reload_groups();
                        self.selected_record_id = None;
                        self.reload_records();
                    }
                }

                if ui.button(self.i18n.t("btn_new_group")).clicked() {
                    self.new_group_name.clear();
                    self.new_group_open = true;
                }

                let active_group_name = self
                    .groups
                    .iter()
                    .find(|g| g.id == self.active_group_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();

                egui::ComboBox::from_id_salt("group_selector_combo")
                    .selected_text(active_group_name)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for grp in self.groups.clone() {
                            if ui
                                .selectable_label(self.active_group_id == grp.id, &grp.name)
                                .clicked()
                                && self.active_group_id != grp.id
                            {
                                self.active_group_id = grp.id;
                                self.selected_record_id = None;
                                self.reload_records();
                            }
                        }
                    });

                ui.label(RichText::new(self.i18n.t("lbl_group")).strong());
            });
        });

        ui.add_space(4.0);
        ui.label(
            RichText::new(self.i18n.t("info_multiline"))
                .small()
                .color(Color32::from_rgb(95, 110, 130)),
        );
    }

    fn draw_settings_tab(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let total_h = ui.available_height();

        ui.columns(2, |cols| {
            // SOL PANEL: Sayısal ve Seçmeli Şablon & Yazıcı Ayarları
            egui::ScrollArea::vertical()
                .id_salt("settings_left_scroll")
                .max_height(total_h)
                .show(&mut cols[0], |ui| {
                    let mut dirty = false;

                    ui.heading(self.i18n.t("sec_printer_page"));
                    ui.add_space(4.0);

                    egui::Grid::new("printer_page_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(self.i18n.t("lbl_printer"));
                            ui.horizontal(|ui| {
                                let p_label = if self.selected_printer.is_empty() {
                                    "--"
                                } else {
                                    &self.selected_printer
                                };
                                egui::ComboBox::from_id_salt("printer_select_combo")
                                    .selected_text(p_label)
                                    .width(210.0)
                                    .show_ui(ui, |ui| {
                                        for p in &self.printers {
                                            ui.selectable_value(
                                                &mut self.selected_printer,
                                                p.clone(),
                                                p,
                                            );
                                        }
                                    });
                                if ui.button(self.i18n.t("btn_refresh_printers")).clicked() {
                                    self.printers = list_system_printers();
                                    if !self.printers.contains(&self.selected_printer) {
                                        self.selected_printer =
                                            self.printers.first().cloned().unwrap_or_default();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_columns"));
                            if ui
                                .add(egui::Slider::new(&mut self.template.columns, 1..=8))
                                .changed()
                            {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_label_width"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "tpl_lw",
                                &mut self.template.label_width_mm,
                                5.0,
                                500.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_label_height"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "tpl_lh",
                                &mut self.template.label_height_mm,
                                5.0,
                                500.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_h_gap"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "tpl_hgap",
                                &mut self.template.horizontal_gap_mm,
                                0.0,
                                100.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_margin_left"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "tpl_ml",
                                &mut self.template.margin_left_mm,
                                0.0,
                                100.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_margin_top"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "tpl_mt",
                                &mut self.template.margin_top_mm,
                                0.0,
                                100.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_dpi_override"));
                            let mut custom_dpi = self.template.dpi_override.unwrap_or(0);
                            if ui
                                .add(egui::DragValue::new(&mut custom_dpi).range(0..=1200))
                                .changed()
                            {
                                self.template.dpi_override = if custom_dpi < 72 {
                                    None
                                } else {
                                    Some(custom_dpi)
                                };
                                dirty = true;
                            }
                            ui.end_row();
                        });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.heading(self.i18n.t("sec_barcode_cfg"));
                    ui.add_space(4.0);

                    egui::Grid::new("barcode_cfg_grid")
                        .num_columns(4)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(self.i18n.t("lbl_barcode_x"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "bc_x",
                                &mut self.template.barcode.x_mm,
                                0.0,
                                500.0,
                            ) {
                                dirty = true;
                            }

                            ui.label(self.i18n.t("lbl_barcode_y"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "bc_y",
                                &mut self.template.barcode.y_mm,
                                0.0,
                                500.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_barcode_w"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "bc_w",
                                &mut self.template.barcode.width_mm,
                                4.0,
                                300.0,
                            ) {
                                dirty = true;
                            }

                            ui.label(self.i18n.t("lbl_barcode_h"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "bc_h",
                                &mut self.template.barcode.height_mm,
                                3.0,
                                300.0,
                            ) {
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_module_scale"));
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.template.barcode.module_scale)
                                        .range(1..=10),
                                )
                                .changed()
                            {
                                dirty = true;
                            }

                            ui.label(self.i18n.t("lbl_rotation"));
                            let rot_label = self
                                .i18n
                                .t(self.template.barcode.rotation.i18n_key())
                                .to_string();
                            egui::ComboBox::from_id_salt("bc_rot_combo")
                                .selected_text(rot_label)
                                .width(115.0)
                                .show_ui(ui, |ui| {
                                    for r in Rotation::ALL {
                                        let txt = self.i18n.t(r.i18n_key());
                                        if ui
                                            .selectable_value(
                                                &mut self.template.barcode.rotation,
                                                *r,
                                                txt,
                                            )
                                            .changed()
                                        {
                                            dirty = true;
                                        }
                                    }
                                });
                            ui.end_row();

                            if ui
                                .checkbox(
                                    &mut self.template.barcode.show_text,
                                    self.i18n.t("lbl_show_hr_text"),
                                )
                                .changed()
                            {
                                dirty = true;
                            }
                            ui.label(self.i18n.t("lbl_hr_font_pt"));
                            if Self::flexible_f32_edit(
                                &mut self.float_buffers,
                                ui,
                                "bc_font",
                                &mut self.template.barcode.text_font_pt,
                                3.0,
                                72.0,
                            ) {
                                dirty = true;
                            }
                            ui.label("");
                            ui.end_row();
                        });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.heading(self.i18n.t("sec_desc_fields"));
                    ui.add_space(4.0);

                    let desc_keys = ["col_desc1", "col_desc2", "col_desc3", "col_desc4"];
                    for (idx, title_key) in desc_keys.iter().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                let title = self.i18n.t(title_key).to_string();
                                if ui
                                    .checkbox(
                                        &mut self.template.desc_fields[idx].enabled,
                                        RichText::new(title).strong(),
                                    )
                                    .changed()
                                {
                                    dirty = true;
                                }

                                ui.label(self.i18n.t("lbl_pos_x"));
                                if Self::flexible_f32_edit(
                                    &mut self.float_buffers,
                                    ui,
                                    &format!("d{idx}_x"),
                                    &mut self.template.desc_fields[idx].x_mm,
                                    0.0,
                                    500.0,
                                ) {
                                    dirty = true;
                                }

                                ui.label(self.i18n.t("lbl_pos_y"));
                                if Self::flexible_f32_edit(
                                    &mut self.float_buffers,
                                    ui,
                                    &format!("d{idx}_y"),
                                    &mut self.template.desc_fields[idx].y_mm,
                                    0.0,
                                    500.0,
                                ) {
                                    dirty = true;
                                }

                                ui.label(self.i18n.t("lbl_text_width"));
                                if Self::flexible_f32_edit(
                                    &mut self.float_buffers,
                                    ui,
                                    &format!("d{idx}_w"),
                                    &mut self.template.desc_fields[idx].width_mm,
                                    0.0,
                                    500.0,
                                ) {
                                    dirty = true;
                                }
                            });

                            ui.horizontal_wrapped(|ui| {
                                ui.label(self.i18n.t("lbl_font_pt"));
                                if Self::flexible_f32_edit(
                                    &mut self.float_buffers,
                                    ui,
                                    &format!("d{idx}_pt"),
                                    &mut self.template.desc_fields[idx].font_size_pt,
                                    3.0,
                                    120.0,
                                ) {
                                    dirty = true;
                                }

                                ui.label(self.i18n.t("lbl_rotation"));
                                let r_txt = self
                                    .i18n
                                    .t(self.template.desc_fields[idx].rotation.i18n_key())
                                    .to_string();
                                egui::ComboBox::from_id_salt(("desc_rot", idx))
                                    .selected_text(r_txt)
                                    .width(105.0)
                                    .show_ui(ui, |ui| {
                                        for r in Rotation::ALL {
                                            let label = self.i18n.t(r.i18n_key());
                                            if ui
                                                .selectable_value(
                                                    &mut self.template.desc_fields[idx].rotation,
                                                    *r,
                                                    label,
                                                )
                                                .changed()
                                            {
                                                dirty = true;
                                            }
                                        }
                                    });

                                ui.label(self.i18n.t("lbl_align"));
                                let a_txt = self
                                    .i18n
                                    .t(self.template.desc_fields[idx].align.i18n_key())
                                    .to_string();
                                egui::ComboBox::from_id_salt(("desc_align", idx))
                                    .selected_text(a_txt)
                                    .width(80.0)
                                    .show_ui(ui, |ui| {
                                        for a in TextAlign::ALL {
                                            let label = self.i18n.t(a.i18n_key());
                                            if ui
                                                .selectable_value(
                                                    &mut self.template.desc_fields[idx].align,
                                                    *a,
                                                    label,
                                                )
                                                .changed()
                                            {
                                                dirty = true;
                                            }
                                        }
                                    });
                            });
                        });
                        ui.add_space(3.0);
                    }

                    if dirty {
                        self.preview_dirty = true;
                    }
                });

            // SAĞ PANEL: Canlı Önizleme Tuvali
            let right_ui = &mut cols[1];
            right_ui.heading(self.i18n.t("sec_live_preview"));
            right_ui.horizontal_wrapped(|ui| {
                if self.template.columns > 1
                    && ui
                        .checkbox(
                            &mut self.preview_show_full_row,
                            self.i18n.t("lbl_preview_mode_row"),
                        )
                        .changed()
                {
                    self.preview_dirty = true;
                }
                ui.label(self.i18n.t("lbl_zoom"));
                ui.add(egui::Slider::new(&mut self.preview_zoom, 0.35..=2.5).text("x"));
                if ui.button("1:1").clicked() {
                    self.preview_zoom = 1.0;
                }
            });

            self.refresh_live_preview_if_needed(ctx);

            let [pw, ph] = self.live_preview_size_px;
            right_ui.label(
                RichText::new(format!(
                    "Rulo Genişliği: {:.1} mm × {:.1} mm ({} Sütun) | Önizleme: {}×{} px",
                    self.template.total_row_width_mm(),
                    self.template.total_row_height_mm(),
                    self.template.columns,
                    pw,
                    ph
                ))
                .small()
                .color(Color32::from_rgb(100, 115, 135)),
            );

            egui::Frame::canvas(right_ui.style())
                .fill(Color32::from_rgb(220, 225, 232))
                .inner_margin(14.0)
                .show(right_ui, |ui| {
                    egui::ScrollArea::both()
                        .id_salt("live_preview_canvas_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(tex) = &self.live_preview_tex {
                                // 203 DPI pikselini ekran (96 DPI) gerçek oranına yaklaştırıp zoom ile çarp
                                let screen_scale = (96.0 / 203.0) * self.preview_zoom * 1.45;
                                let disp_size = egui::vec2(
                                    (pw as f32 * screen_scale).max(20.0),
                                    (ph as f32 * screen_scale).max(20.0),
                                );
                                ui.centered_and_justified(|ui| {
                                    ui.add(
                                        egui::Image::from_texture((tex.id(), disp_size))
                                            .maintain_aspect_ratio(true),
                                    );
                                });
                            }
                        });
                });
        });
    }

    fn pick_and_open_import_file(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("Excel & CSV", &["xlsx", "xls", "ods", "csv", "txt"])
            .pick_file();

        if let Some(path) = picked {
            match load_spreadsheet(&path) {
                Ok(wb) => {
                    let mapping = wb.sheets.first().map(ColumnMapping::guess_for_sheet);
                    self.import_dlg = ImportDialog {
                        open: true,
                        workbook: Some(wb),
                        selected_sheet: 0,
                        mapping,
                    };
                }
                Err(e) => {
                    self.set_status(StatusLevel::Error, format!("Dosya okunamadı: {e}"));
                }
            }
        }
    }

    fn draw_modals(&mut self, ctx: &egui::Context) {
        // 1. Yeni Grup Modalı
        if self.new_group_open {
            let mut open = self.new_group_open;
            egui::Window::new(self.i18n.t("dlg_new_group_title"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(self.i18n.t("lbl_group_name"));
                        ui.text_edit_singleline(&mut self.new_group_name);
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(self.i18n.t("btn_save")).clicked() {
                            match self.db.create_group(&self.new_group_name) {
                                Ok(new_id) => {
                                    self.reload_groups();
                                    self.active_group_id = new_id;
                                    self.selected_record_id = None;
                                    self.reload_records();
                                    self.new_group_open = false;
                                }
                                Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                            }
                        }
                        if ui.button(self.i18n.t("btn_cancel")).clicked() {
                            self.new_group_open = false;
                        }
                    });
                });
            if !open {
                self.new_group_open = false;
            }
        }

        // 2. Yeni Şablon Modalı
        if self.new_tpl_open {
            let mut open = self.new_tpl_open;
            egui::Window::new(self.i18n.t("dlg_new_tpl_title"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(self.i18n.t("lbl_tpl_name"));
                        ui.text_edit_singleline(&mut self.new_tpl_name);
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(self.i18n.t("btn_save")).clicked() {
                            let clean_name = self.new_tpl_name.trim().to_string();
                            if !clean_name.is_empty() {
                                let final_name = if clean_name.to_lowercase().ends_with(".json") {
                                    clean_name
                                } else {
                                    format!("{clean_name}.json")
                                };
                                match save_template(&self.templates_dir, &final_name, &self.template)
                                {
                                    Ok(()) => {
                                        self.template_files = list_templates(&self.templates_dir);
                                        self.active_template_file = final_name.clone();
                                        let msg = format!(
                                            "{} {}",
                                            self.i18n.t("msg_saved_template"),
                                            final_name
                                        );
                                        self.set_status(StatusLevel::Info, msg);
                                        self.new_tpl_open = false;
                                    }
                                    Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                                }
                            }
                        }
                        if ui.button(self.i18n.t("btn_cancel")).clicked() {
                            self.new_tpl_open = false;
                        }
                    });
                });
            if !open {
                self.new_tpl_open = false;
            }
        }

        // 3. Seri Barkod Üretici (Numaratör) Modalı
        if self.numerator_dlg.open {
            let mut open = self.numerator_dlg.open;
            let mut do_generate = false;
            egui::Window::new(self.i18n.t("dlg_numerator_title"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("numerator_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(self.i18n.t("lbl_num_prefix"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.prefix);
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_start"));
                            ui.add(
                                egui::DragValue::new(&mut self.numerator_dlg.start_num)
                                    .range(0..=999_999_999_999u64),
                            );
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_count"));
                            ui.add(
                                egui::DragValue::new(&mut self.numerator_dlg.count)
                                    .range(1..=50_000),
                            );
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_step"));
                            ui.add(
                                egui::DragValue::new(&mut self.numerator_dlg.step)
                                    .range(1..=10_000),
                            );
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_pad"));
                            ui.add(
                                egui::DragValue::new(&mut self.numerator_dlg.pad_digits)
                                    .range(1..=18),
                            );
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_suffix"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.suffix);
                            ui.end_row();

                            ui.label(self.i18n.t("col_desc1"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.desc1);
                            ui.end_row();

                            ui.label(self.i18n.t("col_desc2"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.desc2);
                            ui.end_row();

                            ui.label(self.i18n.t("col_desc3"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.desc3);
                            ui.end_row();

                            ui.label(self.i18n.t("col_desc4"));
                            ui.text_edit_singleline(&mut self.numerator_dlg.desc4);
                            ui.end_row();

                            ui.label(self.i18n.t("lbl_num_copies"));
                            ui.add(
                                egui::DragValue::new(&mut self.numerator_dlg.copies)
                                    .range(1..=1_000),
                            );
                            ui.end_row();
                        });

                    let pad = self.numerator_dlg.pad_digits;
                    let sample_first = format!(
                        "{}{:0width$}{}",
                        self.numerator_dlg.prefix,
                        self.numerator_dlg.start_num,
                        self.numerator_dlg.suffix,
                        width = pad
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Örnek İlk Barkod: {sample_first}")).italics());
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        if ui.button(self.i18n.t("btn_generate")).clicked() {
                            do_generate = true;
                        }
                        if ui.button(self.i18n.t("btn_cancel")).clicked() {
                            self.numerator_dlg.open = false;
                        }
                    });
                });

            if !open {
                self.numerator_dlg.open = false;
            }
            if do_generate {
                let mut batch = Vec::with_capacity(self.numerator_dlg.count as usize);
                let pad = self.numerator_dlg.pad_digits;
                for i in 0..u64::from(self.numerator_dlg.count) {
                    let n = self
                        .numerator_dlg
                        .start_num
                        .saturating_add(i.saturating_mul(self.numerator_dlg.step));
                    let code = format!(
                        "{}{:0width$}{}",
                        self.numerator_dlg.prefix,
                        n,
                        self.numerator_dlg.suffix,
                        width = pad
                    );
                    batch.push(BarcodeRecord {
                        id: None,
                        group_id: self.active_group_id,
                        barcode: code,
                        desc1: self.numerator_dlg.desc1.clone(),
                        desc2: self.numerator_dlg.desc2.clone(),
                        desc3: self.numerator_dlg.desc3.clone(),
                        desc4: self.numerator_dlg.desc4.clone(),
                        print_count: self.numerator_dlg.copies.max(1),
                    });
                }
                match self.db.bulk_insert_records(self.active_group_id, &batch) {
                    Ok(inserted) => {
                        self.reload_records();
                        let msg = format!("{inserted} {}", self.i18n.t("msg_generated"));
                        self.set_status(StatusLevel::Info, msg);
                        self.numerator_dlg.open = false;
                    }
                    Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                }
            }
        }

        // 4. Excel / CSV Sütun Eşleştirme ve İçe Aktarım Modalı
        if self.import_dlg.open {
            let mut open = self.import_dlg.open;
            let mut trigger_import = false;
            let mut trigger_rechoose = false;

            egui::Window::new(self.i18n.t("dlg_import_title"))
                .collapsible(false)
                .resizable(true)
                .default_width(680.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(self.i18n.t("btn_choose_file")).clicked() {
                            trigger_rechoose = true;
                        }
                        if let Some(wb) = &self.import_dlg.workbook {
                            ui.strong(&wb.file_name);
                        }
                    });

                    if let Some(wb) = &self.import_dlg.workbook {
                        ui.separator();
                        let mut sheet_changed = false;
                        ui.horizontal(|ui| {
                            ui.label(self.i18n.t("lbl_sheet"));
                            let curr_name = wb
                                .sheets
                                .get(self.import_dlg.selected_sheet)
                                .map(|s| s.name.as_str())
                                .unwrap_or("");
                            egui::ComboBox::from_id_salt("import_sheet_combo")
                                .selected_text(curr_name)
                                .show_ui(ui, |ui| {
                                    for (idx, sh) in wb.sheets.iter().enumerate() {
                                        if ui
                                            .selectable_value(
                                                &mut self.import_dlg.selected_sheet,
                                                idx,
                                                &sh.name,
                                            )
                                            .changed()
                                        {
                                            sheet_changed = true;
                                        }
                                    }
                                });
                        });

                        if sheet_changed
                            && let Some(sh) = wb.sheets.get(self.import_dlg.selected_sheet)
                        {
                            self.import_dlg.mapping = Some(ColumnMapping::guess_for_sheet(sh));
                        }

                        if let (Some(sheet), Some(mapping)) = (
                            wb.sheets.get(self.import_dlg.selected_sheet),
                            self.import_dlg.mapping.as_mut(),
                        ) {
                            ui.checkbox(&mut mapping.skip_header, self.i18n.t("lbl_skip_header"));
                            let col_labels = sheet.column_labels(mapping.skip_header);
                            let none_txt = self.i18n.t("opt_none");

                            ui.add_space(4.0);
                            ui.strong(self.i18n.t("lbl_col_map"));

                            egui::Grid::new("col_map_grid")
                                .num_columns(4)
                                .spacing([10.0, 6.0])
                                .show(ui, |ui| {
                                    col_ComboBox(
                                        ui,
                                        "map_bc",
                                        self.i18n.t("col_barcode"),
                                        &mut mapping.barcode_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_ComboBox(
                                        ui,
                                        "map_cnt",
                                        self.i18n.t("col_count"),
                                        &mut mapping.count_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    ui.end_row();

                                    col_ComboBox(
                                        ui,
                                        "map_d1",
                                        self.i18n.t("col_desc1"),
                                        &mut mapping.desc1_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_ComboBox(
                                        ui,
                                        "map_d2",
                                        self.i18n.t("col_desc2"),
                                        &mut mapping.desc2_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    ui.end_row();

                                    col_ComboBox(
                                        ui,
                                        "map_d3",
                                        self.i18n.t("col_desc3"),
                                        &mut mapping.desc3_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_ComboBox(
                                        ui,
                                        "map_d4",
                                        self.i18n.t("col_desc4"),
                                        &mut mapping.desc4_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    ui.end_row();
                                });

                            ui.separator();
                            let preview_recs =
                                extract_records(sheet, mapping, self.active_group_id);
                            ui.label(format!(
                                "{} (Toplam aktarılacak geçerli satır: {})",
                                self.i18n.t("lbl_preview_rows"),
                                preview_recs.len()
                            ));

                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    egui::Grid::new("import_preview_grid")
                                        .striped(true)
                                        .num_columns(6)
                                        .spacing([12.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.strong(self.i18n.t("col_barcode"));
                                            ui.strong(self.i18n.t("col_desc1"));
                                            ui.strong(self.i18n.t("col_desc2"));
                                            ui.strong(self.i18n.t("col_desc3"));
                                            ui.strong(self.i18n.t("col_desc4"));
                                            ui.strong(self.i18n.t("col_count"));
                                            ui.end_row();

                                            for r in preview_recs.iter().take(10) {
                                                ui.label(&r.barcode);
                                                ui.label(&r.desc1);
                                                ui.label(&r.desc2);
                                                ui.label(&r.desc3);
                                                ui.label(&r.desc4);
                                                ui.label(format!("{}", r.print_count));
                                                ui.end_row();
                                            }
                                        });
                                });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !preview_recs.is_empty(),
                                        egui::Button::new(format!(
                                            "✅ {}",
                                            self.i18n.t("btn_import_confirm")
                                        )),
                                    )
                                    .clicked()
                                {
                                    trigger_import = true;
                                }
                                if ui.button(self.i18n.t("btn_cancel")).clicked() {
                                    self.import_dlg.open = false;
                                }
                            });
                        }
                    }
                });

            if !open {
                self.import_dlg.open = false;
            }
            if trigger_rechoose {
                self.pick_and_open_import_file();
            }
            if trigger_import
                && let (Some(wb), Some(mapping)) =
                    (&self.import_dlg.workbook, &self.import_dlg.mapping)
                && let Some(sheet) = wb.sheets.get(self.import_dlg.selected_sheet)
            {
                let extracted = extract_records(sheet, mapping, self.active_group_id);
                match self
                    .db
                    .bulk_insert_records(self.active_group_id, &extracted)
                {
                    Ok(count) => {
                        self.reload_records();
                        let msg = format!("{count} {}", self.i18n.t("msg_imported"));
                        self.set_status(StatusLevel::Info, msg);
                        self.import_dlg.open = false;
                    }
                    Err(e) => self.set_status(StatusLevel::Error, format!("{e}")),
                }
            }
        }

        // 5. Çok Sayfalı Baskı Önizleme Modalı
        if self.preview_modal.open {
            let mut open = self.preview_modal.open;
            let mut print_from_modal = false;

            egui::Window::new(self.i18n.t("dlg_preview_title"))
                .collapsible(false)
                .resizable(true)
                .default_size([760.0, 520.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    let total_pages = self.preview_modal.pages.len().max(1);
                    if self.preview_modal.current_page >= total_pages {
                        self.preview_modal.current_page = total_pages - 1;
                    }

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.preview_modal.current_page > 0,
                                egui::Button::new("◀"),
                            )
                            .clicked()
                        {
                            self.preview_modal.current_page -= 1;
                        }
                        ui.strong(format!(
                            "{} {} / {}",
                            self.i18n.t("lbl_page"),
                            self.preview_modal.current_page + 1,
                            total_pages
                        ));
                        if ui
                            .add_enabled(
                                self.preview_modal.current_page + 1 < total_pages,
                                egui::Button::new("▶"),
                            )
                            .clicked()
                        {
                            self.preview_modal.current_page += 1;
                        }

                        ui.separator();
                        ui.label(self.i18n.t("lbl_zoom"));
                        ui.add(
                            egui::Slider::new(&mut self.preview_modal.zoom, 0.35..=2.5).text("x"),
                        );

                        ui.separator();
                        if ui
                            .button(format!("🖨 {}", self.i18n.t("btn_print_all")))
                            .clicked()
                        {
                            print_from_modal = true;
                        }
                        if ui.button(self.i18n.t("btn_close")).clicked() {
                            self.preview_modal.open = false;
                        }
                    });

                    ui.separator();

                    if self.preview_modal.texture_page_idx != Some(self.preview_modal.current_page)
                        && let Some(img) =
                            self.preview_modal.pages.get(self.preview_modal.current_page)
                    {
                        let cimg = ColorImage::from_rgba_unmultiplied(
                            [img.width() as usize, img.height() as usize],
                            img.as_raw(),
                        );
                        self.preview_modal.texture =
                            Some(ctx.load_texture("modal_page_tex", cimg, TextureOptions::LINEAR));
                        self.preview_modal.texture_page_idx = Some(self.preview_modal.current_page);
                    }

                    egui::Frame::canvas(ui.style())
                        .fill(Color32::from_rgb(218, 223, 230))
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let (Some(tex), Some(img)) = (
                                        &self.preview_modal.texture,
                                        self.preview_modal.pages.get(self.preview_modal.current_page),
                                    ) {
                                        let scale =
                                            (96.0 / 203.0) * self.preview_modal.zoom * 1.45;
                                        let sz = egui::vec2(
                                            (img.width() as f32 * scale).max(20.0),
                                            (img.height() as f32 * scale).max(20.0),
                                        );
                                        ui.centered_and_justified(|ui| {
                                            ui.add(egui::Image::from_texture((tex.id(), sz)));
                                        });
                                    }
                                });
                        });
                });

            if !open {
                self.preview_modal.open = false;
            }
            if print_from_modal {
                let recs = self.preview_modal.target_records.clone();
                self.execute_print(&recs);
                self.preview_modal.open = false;
            }
        }
    }
}

#[allow(non_snake_case)]
fn col_ComboBox(
    ui: &mut egui::Ui,
    id_salt: &str,
    label: &str,
    selected: &mut Option<usize>,
    col_labels: &[String],
    none_txt: &str,
) {
    ui.label(label);
    let current_txt = selected
        .and_then(|i| col_labels.get(i))
        .map(|s| s.as_str())
        .unwrap_or(none_txt);
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(current_txt)
        .width(175.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, none_txt);
            for (idx, col_name) in col_labels.iter().enumerate() {
                ui.selectable_value(selected, Some(idx), col_name);
            }
        });
}

fn format_clean_f32(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if (rounded - rounded.round()).abs() < 0.001 {
        format!("{:.0}", rounded)
    } else {
        format!("{rounded}")
    }
}

/// Türkçe İ/ı ve I/i dönüşüm kurallarına tam uyumlu küçük harfe çevirme.
fn turkish_lowercase(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'İ' => out.push('i'),
            'I' => out.push('ı'),
            _ => {
                for lc in ch.to_lowercase() {
                    out.push(lc);
                }
            }
        }
    }
    out
}

/// 1-tabanlı kapsayıcı sıra aralığını (ör. 1..=10) güvenli şekilde dilimler.
fn slice_1based_inclusive<T: Clone>(items: &[T], start_1based: usize, end_1based: usize) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    let low = start_1based.min(end_1based).max(1);
    let high = start_1based.max(end_1based).max(1);
    if low > items.len() {
        return Vec::new();
    }
    let start_idx = low - 1;
    let end_exclusive = high.min(items.len());
    items[start_idx..end_exclusive].to_vec()
}

impl eframe::App for BarkodStudyoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Pencere başlığını aktif dile göre senkron tut
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            self.i18n.t("app_title").to_string(),
        ));

        // 1. Üst Araç Çubuğu + Sekme Başlıkları
        egui::TopBottomPanel::top("top_main_bar").show(ctx, |ui| {
            ui.add_space(3.0);
            self.draw_top_toolbar(ui);
            ui.add_space(2.0);
        });

        // 2. En Alt Durum Çubuğu
        egui::TopBottomPanel::bottom("bottom_status_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let grp_name = self
                    .groups
                    .iter()
                    .find(|g| g.id == self.active_group_id)
                    .map(|g| g.name.as_str())
                    .unwrap_or("-");

                ui.label(format!(
                    "{}: {}",
                    self.i18n.t("status_group"),
                    grp_name
                ));
                ui.separator();
                ui.label(format!(
                    "{}: {}",
                    self.i18n.t("status_template"),
                    self.active_template_file
                ));
                ui.separator();
                ui.label(format!(
                    "{}: {}",
                    self.i18n.t("status_records"),
                    self.records.len()
                ));
                ui.separator();

                let msg_color = match self.status_level {
                    StatusLevel::Info => Color32::from_rgb(35, 115, 65),
                    StatusLevel::Warning => Color32::from_rgb(185, 110, 10),
                    StatusLevel::Error => Color32::from_rgb(195, 35, 35),
                };
                ui.label(RichText::new(&self.status_msg).color(msg_color).strong());
            });
        });

        // 3. Ana İçerik Alanı
        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            ActiveTab::Records => self.draw_records_tab(ui),
            ActiveTab::Settings => self.draw_settings_tab(ctx, ui),
        });

        // 4. Açık Diyalog Pencereleri
        self.draw_modals(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_1based_inclusive_ranges() {
        let items: Vec<usize> = (101..=120).collect(); // 20 eleman (Sıra 1..=20)
        // 1-10 arası tam 10 kayıt döndürmeli (101..=110)
        let r1_10 = slice_1based_inclusive(&items, 1, 10);
        assert_eq!(r1_10.len(), 10);
        assert_eq!(r1_10.first(), Some(&101));
        assert_eq!(r1_10.last(), Some(&110));

        // Ters girilse bile (10-5) güvenli şekilde 5..=10 döndürmeli
        let reversed = slice_1based_inclusive(&items, 10, 5);
        assert_eq!(reversed, vec![105, 106, 107, 108, 109, 110]);

        // Üst sınır aşılırsa (18-50) mevcut son kayda kadar kırpmalı
        let clamped = slice_1based_inclusive(&items, 18, 50);
        assert_eq!(clamped, vec![118, 119, 120]);
    }
}
