use crate::db::Database;
use crate::i18n::I18n;
use crate::importer::{ColumnMapping, ImportWorkbook, extract_records, load_spreadsheet};
use crate::models::{
    BarcodeKind, BarcodeRecord, Group, LabelTemplate, Rotation, TextAlign,
    ensure_default_templates, list_templates, load_template, parse_flexible_f32, save_template,
};
use crate::printer::{list_system_printers, print_records};
use crate::renderer::{FontEngine, RgbaImage, WHITE, render_roll_rows, render_single_label};
use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Margin, RichText, Sense, Stroke, TextureHandle,
    TextureOptions,
};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Merdura Kurumsal Renk Paleti (logo_barkod.png üzerinden örneklenmiştir)
const MERDURA_TEAL: Color32 = Color32::from_rgb(24, 158, 154);
const MERDURA_TURQUOISE: Color32 = Color32::from_rgb(44, 197, 192);
const MERDURA_ROSE: Color32 = Color32::from_rgb(235, 36, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrintScope {
    All,
    Selected,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    PageAndRoll,
    Barcode,
    TextLayers,
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

/// Merdura kurumsal Açık / Koyu stüdyo temasını egui bağlamına uygular.
pub fn apply_merdura_theme(ctx: &egui::Context, dark_mode: bool) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(7.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.window_margin = Margin::same(12);

    let radius = CornerRadius::same(7);
    let mut visuals = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);

    if dark_mode {
        let bg_panel = Color32::from_rgb(20, 24, 33);
        let bg_card = Color32::from_rgb(29, 35, 47);
        let bg_hover = Color32::from_rgb(39, 48, 64);
        let border = Color32::from_rgb(48, 58, 76);
        let fg_text = Color32::from_rgb(232, 238, 246);

        visuals.panel_fill = bg_panel;
        visuals.window_fill = bg_card;
        visuals.extreme_bg_color = Color32::from_rgb(15, 18, 25);
        visuals.faint_bg_color = Color32::from_rgb(25, 30, 41);

        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(44, 197, 192, 65);
        visuals.selection.stroke = Stroke::new(1.2_f32, MERDURA_TURQUOISE);

        visuals.widgets.noninteractive.bg_fill = bg_card;
        visuals.widgets.noninteractive.weak_bg_fill = bg_card;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg_text);
        visuals.widgets.noninteractive.corner_radius = radius;

        visuals.widgets.inactive.bg_fill = bg_card;
        visuals.widgets.inactive.weak_bg_fill = bg_card;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, fg_text);
        visuals.widgets.inactive.corner_radius = radius;

        visuals.widgets.hovered.bg_fill = bg_hover;
        visuals.widgets.hovered.weak_bg_fill = bg_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, MERDURA_TURQUOISE);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, Color32::WHITE);
        visuals.widgets.hovered.corner_radius = radius;

        visuals.widgets.active.bg_fill = MERDURA_TEAL;
        visuals.widgets.active.weak_bg_fill = MERDURA_TEAL;
        visuals.widgets.active.bg_stroke = Stroke::new(1.2_f32, MERDURA_TURQUOISE);
        visuals.widgets.active.fg_stroke = Stroke::new(1.2_f32, Color32::WHITE);
        visuals.widgets.active.corner_radius = radius;

        visuals.widgets.open.bg_fill = bg_hover;
        visuals.widgets.open.weak_bg_fill = bg_hover;
        visuals.widgets.open.bg_stroke = Stroke::new(1.2_f32, MERDURA_TURQUOISE);
        visuals.widgets.open.corner_radius = radius;
    } else {
        let bg_panel = Color32::from_rgb(243, 246, 250);
        let bg_card = Color32::WHITE;
        let bg_hover = Color32::from_rgb(232, 244, 244);
        let border = Color32::from_rgb(214, 222, 232);
        let fg_text = Color32::from_rgb(28, 37, 50);

        visuals.panel_fill = bg_panel;
        visuals.window_fill = bg_card;
        visuals.extreme_bg_color = Color32::WHITE;
        visuals.faint_bg_color = Color32::from_rgb(236, 240, 246);

        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(24, 158, 154, 42);
        visuals.selection.stroke = Stroke::new(1.2_f32, MERDURA_TEAL);

        visuals.widgets.noninteractive.bg_fill = bg_card;
        visuals.widgets.noninteractive.weak_bg_fill = bg_card;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg_text);
        visuals.widgets.noninteractive.corner_radius = radius;

        visuals.widgets.inactive.bg_fill = bg_card;
        visuals.widgets.inactive.weak_bg_fill = bg_card;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, fg_text);
        visuals.widgets.inactive.corner_radius = radius;

        visuals.widgets.hovered.bg_fill = bg_hover;
        visuals.widgets.hovered.weak_bg_fill = bg_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, MERDURA_TEAL);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, Color32::from_rgb(15, 23, 42));
        visuals.widgets.hovered.corner_radius = radius;

        visuals.widgets.active.bg_fill = MERDURA_TEAL;
        visuals.widgets.active.weak_bg_fill = MERDURA_TEAL;
        visuals.widgets.active.bg_stroke = Stroke::new(1.2_f32, MERDURA_TEAL);
        visuals.widgets.active.fg_stroke = Stroke::new(1.2_f32, Color32::WHITE);
        visuals.widgets.active.corner_radius = radius;

        visuals.widgets.open.bg_fill = bg_hover;
        visuals.widgets.open.weak_bg_fill = bg_hover;
        visuals.widgets.open.bg_stroke = Stroke::new(1.2_f32, MERDURA_TEAL);
        visuals.widgets.open.corner_radius = radius;
    }

    style.visuals = visuals;
    ctx.set_style(style);
}

pub struct BarkodStudyoApp {
    db: Database,
    i18n: I18n,
    font_engine: FontEngine,
    templates_dir: PathBuf,

    // Görünüm, Tema ve Durum
    dark_mode: bool,
    theme_dirty: bool,
    print_scope: PrintScope,
    inspector_tab: InspectorTab,
    status_msg: String,
    status_level: StatusLevel,

    // Koleksiyonlar ve ürünler
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

    // Esnek ondalık giriş tamponları
    float_buffers: BTreeMap<String, String>,

    // Canlı Önizleme ve Marka Logosu
    preview_dirty: bool,
    preview_show_full_row: bool,
    preview_zoom: f32,
    live_preview_tex: Option<TextureHandle>,
    live_preview_size_px: [u32; 2],
    brand_logo_tex: Option<TextureHandle>,

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
            .first()
            .cloned()
            .unwrap_or_else(|| "Standart_Yatay_60x40.json".to_string());

        let template = load_template(&templates_dir, &active_template_file)
            .unwrap_or_else(|_| LabelTemplate::default_1_col_horizontal());

        let groups = db.list_groups().unwrap_or_default();
        let active_group_id = groups.first().map(|g| g.id).unwrap_or(1);

        let printers = list_system_printers();
        let selected_printer = printers
            .iter()
            .find(|p| p.to_lowercase().contains("argox") || p.to_lowercase().contains("zebra"))
            .cloned()
            .or_else(|| printers.first().cloned())
            .unwrap_or_default();

        let status_ready = i18n.t("status_ready").to_string();

        let mut app = Self {
            db,
            i18n,
            font_engine,
            templates_dir,
            dark_mode: false,
            theme_dirty: true,
            print_scope: PrintScope::All,
            inspector_tab: InspectorTab::PageAndRoll,
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
            brand_logo_tex: None,
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
                self.editor_record = normalize_legacy_multiline(found);
            } else {
                self.selected_record_id = None;
                self.editor_record = BarcodeRecord::new_empty(self.active_group_id);
            }
        } else if let Some(first) = self.records.first() {
            self.selected_record_id = first.id;
            self.editor_record = normalize_legacy_multiline(first);
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

    fn collect_scope_records(&mut self) -> Option<Vec<BarcodeRecord>> {
        match self.print_scope {
            PrintScope::All => {
                let list: Vec<BarcodeRecord> = self
                    .filtered_indices
                    .iter()
                    .filter_map(|&idx| self.records.get(idx).cloned())
                    .collect();
                if list.is_empty() {
                    let msg = self.i18n.t("err_no_records").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                    None
                } else {
                    Some(list)
                }
            }
            PrintScope::Selected => {
                if self.editor_record.barcode.trim().is_empty() && self.selected_record_id.is_none()
                {
                    let msg = self.i18n.t("err_no_active_record").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                    None
                } else {
                    Some(vec![self.active_record_for_preview()])
                }
            }
            PrintScope::Range => {
                let selected_indices = slice_1based_inclusive(
                    &self.filtered_indices,
                    self.range_start,
                    self.range_end,
                );
                let list: Vec<BarcodeRecord> = selected_indices
                    .into_iter()
                    .filter_map(|idx| self.records.get(idx).cloned())
                    .collect();
                if list.is_empty() {
                    let msg = self.i18n.t("err_empty_range").to_string();
                    self.set_status(StatusLevel::Warning, msg);
                    None
                } else {
                    Some(list)
                }
            }
        }
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
            let (mut rows, w) = render_roll_rows(
                &source_records,
                &self.template,
                preview_dpi,
                &self.font_engine,
                true,
            );
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
                    "{} sayfa ({} DPI • {}) {}",
                    summary.pages_sent, summary.dpi_used, self.selected_printer, suffix
                );
                if let Some(w) = summary.warning {
                    self.set_status(StatusLevel::Warning, format!("{info} | {w}"));
                } else {
                    self.set_status(StatusLevel::Info, info);
                }
            }
            Err(err) => {
                self.set_status(StatusLevel::Error, format!("{err}"));
            }
        }
    }

    fn ensure_brand_logo(&mut self, ctx: &egui::Context) {
        if self.brand_logo_tex.is_some() {
            return;
        }
        const HEADER_LOGO_BYTES: &[u8] = include_bytes!("../assets/logo_header.png");
        if let Some((rgba, w, h)) = crate::decode_png_rgba(HEADER_LOGO_BYTES) {
            let cimg = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            self.brand_logo_tex =
                Some(ctx.load_texture("merdura_brand_logo", cimg, TextureOptions::LINEAR));
        }
    }

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

        let resp = ui.add_sized([68.0, 22.0], egui::TextEdit::singleline(entry));
        if resp.changed() {
            if let Some(parsed) = parse_flexible_f32(entry) {
                let clamped = parsed.clamp(min, max);
                if (*value - clamped).abs() > f32::EPSILON {
                    *value = clamped;
                    changed = true;
                }
            }
        } else if !resp.has_focus() {
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

    /// Merdura Kurumsal Üst Stüdyo Başlığı (Marka + Birleşik Baskı Merkezi + Tema/Dil)
    fn draw_studio_header(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        self.ensure_brand_logo(ctx);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Sol: Merdura Logo + Stüdyo Başlığı
            if let Some(logo_tex) = &self.brand_logo_tex {
                ui.add(
                    egui::Image::from_texture((logo_tex.id(), egui::vec2(50.0, 26.0)))
                        .maintain_aspect_ratio(true),
                );
            }
            ui.label(
                RichText::new("Merdura")
                    .size(16.5)
                    .strong()
                    .color(if self.dark_mode {
                        MERDURA_TURQUOISE
                    } else {
                        MERDURA_TEAL
                    }),
            );
            ui.label(RichText::new("Barcode Studio").size(15.5).strong());

            ui.separator();

            // Orta: Hedef Yazıcı Seçici + Birleşik Baskı Kapsamı + Önizle / Bas
            ui.label("🖨");
            let p_label = if self.selected_printer.is_empty() {
                "--"
            } else {
                &self.selected_printer
            };
            egui::ComboBox::from_id_salt("hdr_printer_combo")
                .selected_text(p_label)
                .width(185.0)
                .show_ui(ui, |ui| {
                    for p in &self.printers {
                        ui.selectable_value(&mut self.selected_printer, p.clone(), p);
                    }
                });
            if ui
                .button("⟳")
                .on_hover_text(self.i18n.t("btn_refresh"))
                .clicked()
            {
                self.printers = list_system_printers();
                if !self.printers.contains(&self.selected_printer) {
                    self.selected_printer = self.printers.first().cloned().unwrap_or_default();
                }
            }

            ui.separator();

            // Kapsam Seçici Hapları (Tüm Liste | Seçili Ürün | Sıra Aralığı)
            ui.selectable_value(
                &mut self.print_scope,
                PrintScope::All,
                self.i18n.t("scope_all"),
            );
            ui.selectable_value(
                &mut self.print_scope,
                PrintScope::Selected,
                self.i18n.t("scope_selected"),
            );
            ui.selectable_value(
                &mut self.print_scope,
                PrintScope::Range,
                self.i18n.t("scope_range"),
            );

            if self.print_scope == PrintScope::Range {
                let max_rows = self.filtered_indices.len().max(1);
                ui.add(
                    egui::DragValue::new(&mut self.range_start)
                        .range(1..=max_rows)
                        .speed(0.2),
                );
                ui.label("–");
                ui.add(
                    egui::DragValue::new(&mut self.range_end)
                        .range(1..=max_rows)
                        .speed(0.2),
                );
            }

            if ui
                .button(format!("🔍 {}", self.i18n.t("btn_preview")))
                .clicked()
                && let Some(recs) = self.collect_scope_records()
            {
                self.open_print_preview(recs);
            }

            let cta_btn = egui::Button::new(
                RichText::new(format!("🖨 {}", self.i18n.t("btn_print_now")))
                    .color(Color32::WHITE)
                    .strong(),
            )
            .fill(MERDURA_TEAL);

            if ui.add(cta_btn).clicked()
                && let Some(recs) = self.collect_scope_records()
            {
                self.execute_print(&recs);
            }

            // Sağ: Dil Seçici & Açık/Koyu Tema Anahtarı
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current_code = self.i18n.active_lang().to_uppercase();
                let resp = egui::ComboBox::from_id_salt("hdr_lang_combo")
                    .selected_text(current_code)
                    .width(95.0)
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
                    self.i18n.reload_external_locales();
                }

                let theme_label = if self.dark_mode {
                    format!("🌙 {}", self.i18n.t("theme_dark"))
                } else {
                    format!("☀ {}", self.i18n.t("theme_light"))
                };
                if ui.button(theme_label).clicked() {
                    self.dark_mode = !self.dark_mode;
                    self.theme_dirty = true;
                }
            });
        });
    }

    /// Sol Kenar Çubuğu: Koleksiyonlar, Etiket Şablonları ve Veri Araçları
    fn draw_left_sidebar(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("left_sidebar_scroll")
            .show(ui, |ui| {
                ui.add_space(4.0);

                // 1. KOLEKSİYONLAR BÖLÜMÜ
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.i18n.t("sec_collections"))
                            .size(11.5)
                            .strong()
                            .color(MERDURA_TEAL),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(format!("+ {}", self.i18n.t("btn_new_collection")))
                            .clicked()
                        {
                            self.new_group_name.clear();
                            self.new_group_open = true;
                        }
                    });
                });
                ui.add_space(3.0);

                for grp in self.groups.clone() {
                    let is_active = self.active_group_id == grp.id;
                    let resp = ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(format!("📁  {}", grp.name)).selected(is_active),
                    );
                    if resp.clicked() && !is_active {
                        self.active_group_id = grp.id;
                        self.selected_record_id = None;
                        self.reload_records();
                    }
                }

                if self.groups.len() > 1
                    && ui
                        .small_button(format!("🗑 {}", self.i18n.t("btn_delete_collection")))
                        .clicked()
                {
                    if let Err(e) = self.db.delete_group(self.active_group_id) {
                        self.set_status(StatusLevel::Error, format!("{e}"));
                    } else {
                        self.reload_groups();
                        self.selected_record_id = None;
                        self.reload_records();
                    }
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                // 2. ETİKET ŞABLONLARI BÖLÜMÜ
                ui.label(
                    RichText::new(self.i18n.t("sec_templates"))
                        .size(11.5)
                        .strong()
                        .color(MERDURA_TEAL),
                );
                ui.add_space(3.0);

                for fname in self.template_files.clone() {
                    let selected = self.active_template_file == fname;
                    let display = fname.trim_end_matches(".json");
                    let resp = ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(format!("🏷  {display}")).selected(selected),
                    );
                    if resp.clicked() && !selected {
                        match load_template(&self.templates_dir, &fname) {
                            Ok(loaded) => {
                                self.template = loaded;
                                self.active_template_file = fname;
                                self.float_buffers.clear();
                                self.preview_dirty = true;
                            }
                            Err(e) => {
                                self.set_status(StatusLevel::Error, format!("{e}"));
                            }
                        }
                    }
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("💾 {}", self.i18n.t("btn_save_tpl")))
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
                        .button(format!("+ {}", self.i18n.t("btn_save_as_tpl")))
                        .clicked()
                    {
                        self.new_tpl_name = "Ozel_Sablon".to_string();
                        self.new_tpl_open = true;
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);

                // 3. VERİ ARAÇLARI BÖLÜMÜ
                ui.label(
                    RichText::new(self.i18n.t("sec_tools"))
                        .size(11.5)
                        .strong()
                        .color(MERDURA_TEAL),
                );
                ui.add_space(4.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::new(format!("📊  {}", self.i18n.t("btn_import_table"))),
                    )
                    .clicked()
                {
                    self.pick_and_open_import_file();
                }

                ui.add_space(4.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::new(format!("🔢  {}", self.i18n.t("btn_serial_tool"))),
                    )
                    .clicked()
                {
                    self.numerator_dlg.open = true;
                }
            });
    }

    /// Sağ Stüdyo Denetçisi: Her An Görünür Canlı Etiket Sahnesi + 3 Sekmeli Tasarım Denetçisi
    fn draw_right_inspector(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // ÜST YARI: CANLI ETİKET SAHNESİ
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("👁 {}", self.i18n.t("sec_live_stage")))
                    .size(13.5)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("1:1").clicked() {
                    self.preview_zoom = 1.0;
                }
                ui.add(
                    egui::Slider::new(&mut self.preview_zoom, 0.35..=2.2)
                        .show_value(false)
                        .trailing_fill(true),
                );
                if self.template.columns > 1
                    && ui
                        .checkbox(&mut self.preview_show_full_row, self.i18n.t("lbl_roll_view"))
                        .changed()
                {
                    self.preview_dirty = true;
                }
            });
        });

        self.refresh_live_preview_if_needed(ctx);
        let [pw, ph] = self.live_preview_size_px;

        ui.label(
            RichText::new(format!(
                "{:.1} × {:.1} mm  •  {} Sütun  •  {}  •  {}×{} px",
                self.template.total_row_width_mm(),
                self.template.total_row_height_mm(),
                self.template.columns,
                self.template.barcode_kind.display_name(),
                pw,
                ph
            ))
            .size(11.0)
            .color(if self.dark_mode {
                Color32::from_rgb(148, 163, 184)
            } else {
                Color32::from_rgb(100, 116, 139)
            }),
        );

        let stage_height = (ui.available_height() * 0.42).clamp(165.0, 290.0);
        let stage_bg = if self.dark_mode {
            Color32::from_rgb(13, 16, 23)
        } else {
            Color32::from_rgb(222, 228, 236)
        };

        egui::Frame::canvas(ui.style())
            .fill(stage_bg)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_height(stage_height);
                egui::ScrollArea::both()
                    .id_salt("studio_live_stage_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(tex) = &self.live_preview_tex {
                            let screen_scale = (96.0 / 203.0) * self.preview_zoom * 1.25;
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

        ui.add_space(6.0);

        // ALT YARI: 3 SEKMELİ TASARIM DENETÇİSİ
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::PageAndRoll,
                format!("📐 {}", self.i18n.t("insp_tab_page")),
            );
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::Barcode,
                format!("▮▯ {}", self.i18n.t("insp_tab_barcode")),
            );
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::TextLayers,
                format!("Aa {}", self.i18n.t("insp_tab_text")),
            );
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("inspector_props_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut dirty = false;

                match self.inspector_tab {
                    InspectorTab::PageAndRoll => {
                        egui::Grid::new("insp_page_grid")
                            .num_columns(2)
                            .spacing([12.0, 7.0])
                            .show(ui, |ui| {
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
                    }
                    InspectorTab::Barcode => {
                        egui::Grid::new("insp_bc_grid")
                            .num_columns(2)
                            .spacing([12.0, 7.0])
                            .show(ui, |ui| {
                                ui.label(self.i18n.t("lbl_barcode_kind"));
                                egui::ComboBox::from_id_salt("insp_bc_kind")
                                    .selected_text(self.template.barcode_kind.display_name())
                                    .width(135.0)
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
                                                dirty = true;
                                            }
                                        }
                                    });
                                ui.end_row();

                                ui.label(self.i18n.t("lbl_rotation"));
                                let rot_label = self
                                    .i18n
                                    .t(self.template.barcode.rotation.i18n_key())
                                    .to_string();
                                egui::ComboBox::from_id_salt("insp_bc_rot")
                                    .selected_text(rot_label)
                                    .width(135.0)
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
                                ui.end_row();

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
                                ui.end_row();

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
                                        egui::Slider::new(
                                            &mut self.template.barcode.module_scale,
                                            1..=8,
                                        ),
                                    )
                                    .changed()
                                {
                                    dirty = true;
                                }
                                ui.end_row();

                                ui.label(self.i18n.t("lbl_show_hr_text"));
                                if ui.checkbox(&mut self.template.barcode.show_text, "").changed() {
                                    dirty = true;
                                }
                                ui.end_row();

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
                                ui.end_row();
                            });
                    }
                    InspectorTab::TextLayers => {
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

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let a_txt = self
                                                .i18n
                                                .t(self.template.desc_fields[idx].align.i18n_key())
                                                .to_string();
                                            egui::ComboBox::from_id_salt(("desc_align", idx))
                                                .selected_text(a_txt)
                                                .width(68.0)
                                                .show_ui(ui, |ui| {
                                                    for a in TextAlign::ALL {
                                                        let label = self.i18n.t(a.i18n_key());
                                                        if ui
                                                            .selectable_value(
                                                                &mut self.template.desc_fields[idx]
                                                                    .align,
                                                                *a,
                                                                label,
                                                            )
                                                            .changed()
                                                        {
                                                            dirty = true;
                                                        }
                                                    }
                                                });

                                            let r_txt = self
                                                .i18n
                                                .t(self.template.desc_fields[idx]
                                                    .rotation
                                                    .i18n_key())
                                                .to_string();
                                            egui::ComboBox::from_id_salt(("desc_rot", idx))
                                                .selected_text(r_txt)
                                                .width(96.0)
                                                .show_ui(ui, |ui| {
                                                    for r in Rotation::ALL {
                                                        let label = self.i18n.t(r.i18n_key());
                                                        if ui
                                                            .selectable_value(
                                                                &mut self.template.desc_fields[idx]
                                                                    .rotation,
                                                                *r,
                                                                label,
                                                            )
                                                            .changed()
                                                        {
                                                            dirty = true;
                                                        }
                                                    }
                                                });
                                        },
                                    );
                                });

                                if self.template.desc_fields[idx].enabled {
                                    ui.horizontal_wrapped(|ui| {
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
                                    });

                                    ui.horizontal_wrapped(|ui| {
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
                                    });
                                }
                            });
                            ui.add_space(4.0);
                        }
                    }
                }

                if dirty {
                    self.preview_dirty = true;
                }
            });
    }

    /// Orta Çalışma Alanı: Arama + Sanal Ürün Havuzu Tablosu + Çok Satırlı Etiket Kartı Editörü
    fn draw_center_workspace(&mut self, ui: &mut egui::Ui) {
        // 1. Arama Çubuğu ve Ürün Rozeti
        ui.horizontal(|ui| {
            ui.label("🔍");
            let search_resp = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .hint_text(self.i18n.t("lbl_search_placeholder"))
                    .desired_width(300.0),
            );
            if search_resp.changed() {
                self.rebuild_filter();
            }
            if !self.search_query.is_empty() && ui.button(self.i18n.t("btn_clear")).clicked() {
                self.search_query.clear();
                self.rebuild_filter();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        self.filtered_indices.len(),
                        self.i18n.t("lbl_items_badge")
                    ))
                    .strong()
                    .color(MERDURA_TEAL),
                );
            });
        });

        ui.add_space(4.0);

        let table_height = (ui.available_height() - 185.0).max(140.0);
        let mut clicked_record_idx: Option<usize> = None;
        let mut double_clicked = false;

        // 2. Sanal Ürün Tablosu
        ui.push_id("merdura_items_table", |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .sense(Sense::click())
                .min_scrolled_height(100.0)
                .max_scroll_height(table_height)
                .column(Column::initial(38.0).at_least(32.0))
                .column(Column::initial(135.0).at_least(90.0))
                .column(Column::initial(110.0).at_least(75.0))
                .column(Column::remainder().at_least(140.0))
                .column(Column::initial(110.0).at_least(70.0))
                .column(Column::initial(95.0).at_least(65.0))
                .column(Column::initial(56.0).at_least(46.0))
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
                    body.rows(24.0, row_count, |mut row| {
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
                            ui.label(RichText::new(&rec.barcode).monospace());
                        });
                        let (_, r2) = row.col(|ui| {
                            ui.label(compact_single_line(&rec.desc1));
                        });
                        let (_, r3) = row.col(|ui| {
                            ui.label(compact_single_line(&rec.desc2));
                        });
                        let (_, r4) = row.col(|ui| {
                            ui.label(compact_single_line(&rec.desc3));
                        });
                        let (_, r5) = row.col(|ui| {
                            ui.label(compact_single_line(&rec.desc4));
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
            self.editor_record = normalize_legacy_multiline(rec);
            self.preview_dirty = true;
            if double_clicked {
                self.focus_barcode_input = true;
            }
        }

        ui.add_space(6.0);

        // 3. Alt Seçili Ürün & Doğal Çok Satırlı Etiket Kartı
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("🏷 {}", self.i18n.t("sec_item_editor")))
                        .size(13.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(self.i18n.t("hint_multiline"))
                            .size(11.0)
                            .italics()
                            .color(Color32::from_rgb(120, 134, 155)),
                    );
                });
            });

            ui.add_space(2.0);

            ui.columns(2, |cols| {
                // Sol Sütun: Barkod, Stok Kodu, Ölçü/Özellik, Kopya
                egui::Grid::new("card_left_grid")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(&mut cols[0], |ui| {
                        ui.label(RichText::new(self.i18n.t("col_barcode")).strong());
                        let bc_edit = ui.add(
                            egui::TextEdit::singleline(&mut self.editor_record.barcode)
                                .desired_width(f32::INFINITY),
                        );
                        if self.focus_barcode_input {
                            bc_edit.request_focus();
                            self.focus_barcode_input = false;
                        }
                        if bc_edit.changed() {
                            self.preview_dirty = true;
                        }
                        ui.end_row();

                        ui.label(self.i18n.t("col_desc1"));
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.editor_record.desc1)
                                    .desired_width(f32::INFINITY),
                            )
                            .changed()
                        {
                            self.preview_dirty = true;
                        }
                        ui.end_row();

                        ui.label(self.i18n.t("col_desc3"));
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.editor_record.desc3)
                                    .desired_width(f32::INFINITY),
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
                        ui.end_row();
                    });

                // Sağ Sütun: Doğal Çok Satırlı Ürün Adı & Ek Not (Enter ile alt satır)
                egui::Grid::new("card_right_grid")
                    .num_columns(2)
                    .spacing([8.0, 5.0])
                    .show(&mut cols[1], |ui| {
                        ui.label(self.i18n.t("col_desc2"));
                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut self.editor_record.desc2)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            )
                            .changed()
                        {
                            self.preview_dirty = true;
                        }
                        ui.end_row();

                        ui.label(self.i18n.t("col_desc4"));
                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut self.editor_record.desc4)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            )
                            .changed()
                        {
                            self.preview_dirty = true;
                        }
                        ui.end_row();
                    });
            });

            ui.add_space(4.0);

            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("+ {}", self.i18n.t("btn_new_item")))
                    .clicked()
                {
                    self.selected_record_id = None;
                    self.editor_record = BarcodeRecord::new_empty(self.active_group_id);
                    self.focus_barcode_input = true;
                    self.preview_dirty = true;
                }

                let save_btn = egui::Button::new(
                    RichText::new(format!("✓ {}", self.i18n.t("btn_save_item")))
                        .color(Color32::WHITE)
                        .strong(),
                )
                .fill(MERDURA_TEAL);

                if ui.add(save_btn).clicked() {
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

                if ui
                    .button(format!("🗑 {}", self.i18n.t("btn_delete_item")))
                    .clicked()
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

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(self.i18n.t("btn_clear_list")).clicked()
                        && self.db.clear_group_records(self.active_group_id).is_ok()
                    {
                        self.selected_record_id = None;
                        self.reload_records();
                        let msg = self.i18n.t("msg_cleared_group").to_string();
                        self.set_status(StatusLevel::Info, msg);
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
                    self.set_status(StatusLevel::Error, format!("{e}"));
                }
            }
        }
    }

    fn draw_modals(&mut self, ctx: &egui::Context) {
        // 1. Yeni Koleksiyon Modalı
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
                        if ui.button(self.i18n.t("btn_save_item")).clicked() {
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

        // 2. Şablon Farklı Kaydet Modalı
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
                        if ui.button(self.i18n.t("btn_save_tpl")).clicked() {
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

        // 3. Seri Barkod Üretici Modalı
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
                    ui.label(RichText::new(format!("Örnek: {sample_first}")).italics());
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        let gen_btn = egui::Button::new(
                            RichText::new(self.i18n.t("btn_generate"))
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(MERDURA_TEAL);
                        if ui.add(gen_btn).clicked() {
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

        // 4. Tablo İçe Aktarma Sihirbazı Modalı
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
                                    col_combo(
                                        ui,
                                        "map_bc",
                                        self.i18n.t("col_barcode"),
                                        &mut mapping.barcode_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_combo(
                                        ui,
                                        "map_cnt",
                                        self.i18n.t("col_count"),
                                        &mut mapping.count_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    ui.end_row();

                                    col_combo(
                                        ui,
                                        "map_d1",
                                        self.i18n.t("col_desc1"),
                                        &mut mapping.desc1_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_combo(
                                        ui,
                                        "map_d2",
                                        self.i18n.t("col_desc2"),
                                        &mut mapping.desc2_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    ui.end_row();

                                    col_combo(
                                        ui,
                                        "map_d3",
                                        self.i18n.t("col_desc3"),
                                        &mut mapping.desc3_col,
                                        &col_labels,
                                        none_txt,
                                    );
                                    col_combo(
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
                                "{} ({})",
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
                                                ui.label(compact_single_line(&r.desc1));
                                                ui.label(compact_single_line(&r.desc2));
                                                ui.label(compact_single_line(&r.desc3));
                                                ui.label(compact_single_line(&r.desc4));
                                                ui.label(format!("{}", r.print_count));
                                                ui.end_row();
                                            }
                                        });
                                });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let imp_btn = egui::Button::new(
                                    RichText::new(format!(
                                        "✓ {}",
                                        self.i18n.t("btn_import_confirm")
                                    ))
                                    .color(Color32::WHITE)
                                    .strong(),
                                )
                                .fill(MERDURA_TEAL);

                                if ui.add_enabled(!preview_recs.is_empty(), imp_btn).clicked() {
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

        // 5. Çok Sayfalı Baskı Önizleme Merkezi
        if self.preview_modal.open {
            let mut open = self.preview_modal.open;
            let mut print_from_modal = false;

            egui::Window::new(self.i18n.t("dlg_preview_title"))
                .collapsible(false)
                .resizable(true)
                .default_size([780.0, 540.0])
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
                        let p_btn = egui::Button::new(
                            RichText::new(format!("🖨 {}", self.i18n.t("btn_print_now")))
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(MERDURA_TEAL);
                        if ui.add(p_btn).clicked() {
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

                    let modal_bg = if self.dark_mode {
                        Color32::from_rgb(13, 16, 23)
                    } else {
                        Color32::from_rgb(218, 224, 232)
                    };

                    egui::Frame::canvas(ui.style())
                        .fill(modal_bg)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let (Some(tex), Some(img)) = (
                                        &self.preview_modal.texture,
                                        self.preview_modal
                                            .pages
                                            .get(self.preview_modal.current_page),
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

fn col_combo(
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

/// Tablo hücrelerinde çok satırlı metinleri tek satırda temiz gösterir.
fn compact_single_line(s: &str) -> String {
    let normalized = s.replace("+++", "\n");
    let parts: Vec<&str> = normalized
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    parts.join(" ↵ ")
}

/// Eski kayıtlarda `+++` kalmışsa editöre yüklerken doğal satır sonuna (`\n`) dönüştürür.
fn normalize_legacy_multiline(rec: &BarcodeRecord) -> BarcodeRecord {
    let clean = |s: &str| {
        if s.contains("+++") {
            s.split("+++")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            s.to_string()
        }
    };
    BarcodeRecord {
        id: rec.id,
        group_id: rec.group_id,
        barcode: rec.barcode.clone(),
        desc1: clean(&rec.desc1),
        desc2: clean(&rec.desc2),
        desc3: clean(&rec.desc3),
        desc4: clean(&rec.desc4),
        print_count: rec.print_count,
    }
}

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
        if self.theme_dirty {
            apply_merdura_theme(ctx, self.dark_mode);
            self.theme_dirty = false;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            self.i18n.t("app_title").to_string(),
        ));

        // 1. ÜST MERDURA STÜDYO ÇUBUĞU
        egui::TopBottomPanel::top("merdura_studio_header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                self.draw_studio_header(ctx, ui);
            });

        // 2. ALT DURUM ŞERİDİ
        egui::TopBottomPanel::bottom("merdura_status_strip")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let grp_name = self
                        .groups
                        .iter()
                        .find(|g| g.id == self.active_group_id)
                        .map(|g| g.name.as_str())
                        .unwrap_or("-");

                    ui.label(format!("{}: {}", self.i18n.t("status_group"), grp_name));
                    ui.separator();
                    ui.label(format!(
                        "{}: {}",
                        self.i18n.t("status_template"),
                        self.active_template_file.trim_end_matches(".json")
                    ));
                    ui.separator();
                    ui.label(format!(
                        "{}: {}",
                        self.i18n.t("status_records"),
                        self.records.len()
                    ));
                    ui.separator();

                    let msg_color = match self.status_level {
                        StatusLevel::Info => {
                            if self.dark_mode {
                                MERDURA_TURQUOISE
                            } else {
                                MERDURA_TEAL
                            }
                        }
                        StatusLevel::Warning => Color32::from_rgb(217, 130, 15),
                        StatusLevel::Error => MERDURA_ROSE,
                    };
                    ui.label(RichText::new(&self.status_msg).color(msg_color).strong());
                });
            });

        // 3. SOL KENAR ÇUBUĞU (Koleksiyonlar, Şablonlar & Araçlar)
        egui::SidePanel::left("merdura_left_sidebar")
            .default_width(232.0)
            .width_range(195.0..=310.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(Margin::symmetric(10, 8)),
            )
            .show(ctx, |ui| {
                self.draw_left_sidebar(ui);
            });

        // 4. SAĞ STÜDYO DENETÇİSİ (Her An Açık Canlı Etiket Sahnesi + 3 Sekmeli Denetçi)
        egui::SidePanel::right("merdura_right_inspector")
            .default_width(375.0)
            .width_range(320.0..=520.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(Margin::symmetric(10, 8)),
            )
            .show(ctx, |ui| {
                self.draw_right_inspector(ctx, ui);
            });

        // 5. ORTA ÇALIŞMA ALANI (Sanal Ürün Tablosu & Çok Satırlı Etiket Kartı)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .inner_margin(Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                self.draw_center_workspace(ui);
            });

        // 6. Açık Diyalog Pencereleri
        self.draw_modals(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_1based_inclusive_ranges() {
        let items: Vec<usize> = (101..=120).collect();
        let r1_10 = slice_1based_inclusive(&items, 1, 10);
        assert_eq!(r1_10.len(), 10);
        assert_eq!(r1_10.first(), Some(&101));
        assert_eq!(r1_10.last(), Some(&110));

        let reversed = slice_1based_inclusive(&items, 10, 5);
        assert_eq!(reversed, vec![105, 106, 107, 108, 109, 110]);

        let clamped = slice_1based_inclusive(&items, 18, 50);
        assert_eq!(clamped, vec![118, 119, 120]);
    }
}
