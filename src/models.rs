use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Desteklenen barkod türleri (yeni türler kolayca eklenebilecek şekilde modüler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BarcodeKind {
    Code128,
    Ean13,
    QrCode,
}

impl BarcodeKind {
    pub const ALL: &'static [BarcodeKind] = &[
        BarcodeKind::Code128,
        BarcodeKind::Ean13,
        BarcodeKind::QrCode,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            BarcodeKind::Code128 => "Code 128",
            BarcodeKind::Ean13 => "EAN-13",
            BarcodeKind::QrCode => "QR Kod",
        }
    }
}

/// Tam sayı matris rotasyonu açıları (sürücü bağımsız bellekte döndürme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rotation {
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    pub const ALL: &'static [Rotation] = &[
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ];

    pub fn i18n_key(&self) -> &'static str {
        match self {
            Rotation::Deg0 => "rot_0",
            Rotation::Deg90 => "rot_90",
            Rotation::Deg180 => "rot_180",
            Rotation::Deg270 => "rot_270",
        }
    }
}

/// Metin kutusu içi yatay hizalama seçeneği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: &'static [TextAlign] = &[TextAlign::Left, TextAlign::Center, TextAlign::Right];

    pub fn i18n_key(&self) -> &'static str {
        match self {
            TextAlign::Left => "align_left",
            TextAlign::Center => "align_center",
            TextAlign::Right => "align_right",
        }
    }
}

/// Tanım 1..4 metin alanlarının her biri için yerleşim, genişlik (otomatik satır kaydırma) ve hizalama.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextFieldConfig {
    pub enabled: bool,
    pub x_mm: f32,
    pub y_mm: f32,
    /// Metin kutusu genişliği (mm). `0.0` ise etiket sınırına kadar otomatik sarar; `> 0.0` ise verilen genişlikte alt satıra böler.
    #[serde(default)]
    pub width_mm: f32,
    pub font_size_pt: f32,
    pub rotation: Rotation,
    #[serde(default)]
    pub align: TextAlign,
}

impl TextFieldConfig {
    pub fn sanitize(&mut self) {
        if !self.x_mm.is_finite() || self.x_mm < 0.0 {
            self.x_mm = 0.0;
        }
        if !self.y_mm.is_finite() || self.y_mm < 0.0 {
            self.y_mm = 0.0;
        }
        if !self.width_mm.is_finite() || self.width_mm < 0.0 {
            self.width_mm = 0.0;
        }
        self.width_mm = self.width_mm.clamp(0.0, 500.0);
        if !self.font_size_pt.is_finite() {
            self.font_size_pt = 8.0;
        }
        self.font_size_pt = self.font_size_pt.clamp(3.0, 120.0);
    }
}

/// Barkod bileşeninin konum, boyut ve döndürme ayarları.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarcodeFieldConfig {
    pub x_mm: f32,
    pub y_mm: f32,
    pub width_mm: f32,
    pub height_mm: f32,
    pub module_scale: u32,
    pub rotation: Rotation,
    pub show_text: bool,
    pub text_font_pt: f32,
}

impl BarcodeFieldConfig {
    pub fn sanitize(&mut self) {
        if !self.x_mm.is_finite() || self.x_mm < 0.0 {
            self.x_mm = 0.0;
        }
        if !self.y_mm.is_finite() || self.y_mm < 0.0 {
            self.y_mm = 0.0;
        }
        if !self.width_mm.is_finite() {
            self.width_mm = 30.0;
        }
        self.width_mm = self.width_mm.clamp(4.0, 300.0);

        if !self.height_mm.is_finite() {
            self.height_mm = 12.0;
        }
        self.height_mm = self.height_mm.clamp(3.0, 300.0);

        self.module_scale = self.module_scale.clamp(1, 12);

        if !self.text_font_pt.is_finite() {
            self.text_font_pt = 8.0;
        }
        self.text_font_pt = self.text_font_pt.clamp(3.0, 72.0);
    }
}

/// Çoklu sütun ve termal rulo destekli etiket şablon profili.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelTemplate {
    pub columns: u32,
    pub label_width_mm: f32,
    pub label_height_mm: f32,
    pub horizontal_gap_mm: f32,
    pub margin_left_mm: f32,
    pub margin_top_mm: f32,
    pub dpi_override: Option<u32>,
    pub barcode_kind: BarcodeKind,
    pub barcode: BarcodeFieldConfig,
    pub desc_fields: [TextFieldConfig; 4],
}

impl LabelTemplate {
    /// Geçersiz, sıfır veya NaN değerleri güvenli aralıklara çekerek FPU çökmesini engeller.
    pub fn sanitize(&mut self) {
        self.columns = self.columns.clamp(1, 8);

        if !self.label_width_mm.is_finite() {
            self.label_width_mm = 60.0;
        }
        self.label_width_mm = self.label_width_mm.clamp(5.0, 500.0);

        if !self.label_height_mm.is_finite() {
            self.label_height_mm = 40.0;
        }
        self.label_height_mm = self.label_height_mm.clamp(5.0, 500.0);

        if !self.horizontal_gap_mm.is_finite() || self.horizontal_gap_mm < 0.0 {
            self.horizontal_gap_mm = 0.0;
        }
        self.horizontal_gap_mm = self.horizontal_gap_mm.clamp(0.0, 100.0);

        if !self.margin_left_mm.is_finite() || self.margin_left_mm < 0.0 {
            self.margin_left_mm = 0.0;
        }
        self.margin_left_mm = self.margin_left_mm.clamp(0.0, 100.0);

        if !self.margin_top_mm.is_finite() || self.margin_top_mm < 0.0 {
            self.margin_top_mm = 0.0;
        }
        self.margin_top_mm = self.margin_top_mm.clamp(0.0, 100.0);

        if let Some(dpi) = self.dpi_override {
            if dpi == 0 {
                self.dpi_override = None;
            } else {
                self.dpi_override = Some(dpi.clamp(72, 2400));
            }
        }

        self.barcode.sanitize();
        for field in &mut self.desc_fields {
            field.sanitize();
        }
    }

    /// Tek bir yatay rulo satırının toplam milimetrik genişliği.
    pub fn total_row_width_mm(&self) -> f32 {
        let cols = self.columns.clamp(1, 8);
        let gaps = cols.saturating_sub(1);
        self.margin_left_mm
            + (cols as f32 * self.label_width_mm)
            + (gaps as f32 * self.horizontal_gap_mm)
    }

    /// Tek bir yatay rulo satırının toplam milimetrik yüksekliği.
    pub fn total_row_height_mm(&self) -> f32 {
        self.margin_top_mm + self.label_height_mm
    }

    /// Varsayılan 1 Sütun Yatay (60x40 mm) şablonu.
    pub fn default_1_col_horizontal() -> Self {
        Self {
            columns: 1,
            label_width_mm: 60.0,
            label_height_mm: 40.0,
            horizontal_gap_mm: 0.0,
            margin_left_mm: 1.0,
            margin_top_mm: 1.0,
            dpi_override: None,
            barcode_kind: BarcodeKind::Code128,
            barcode: BarcodeFieldConfig {
                x_mm: 4.0,
                y_mm: 21.0,
                width_mm: 52.0,
                height_mm: 13.0,
                module_scale: 2,
                rotation: Rotation::Deg0,
                show_text: true,
                text_font_pt: 8.5,
            },
            desc_fields: [
                TextFieldConfig {
                    enabled: true,
                    x_mm: 4.0,
                    y_mm: 3.0,
                    width_mm: 52.0,
                    font_size_pt: 10.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 4.0,
                    y_mm: 9.0,
                    width_mm: 52.0,
                    font_size_pt: 8.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 4.0,
                    y_mm: 14.5,
                    width_mm: 52.0,
                    font_size_pt: 8.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: false,
                    x_mm: 35.0,
                    y_mm: 14.5,
                    width_mm: 22.0,
                    font_size_pt: 8.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
            ],
        }
    }

    /// Varsayılan 1 Sütun Dikey Barkodlu (60x40 mm, 90° dikey barkod) şablonu.
    pub fn default_1_col_vertical() -> Self {
        Self {
            columns: 1,
            label_width_mm: 60.0,
            label_height_mm: 40.0,
            horizontal_gap_mm: 0.0,
            margin_left_mm: 1.0,
            margin_top_mm: 1.0,
            dpi_override: None,
            barcode_kind: BarcodeKind::Code128,
            barcode: BarcodeFieldConfig {
                x_mm: 3.0,
                y_mm: 2.0,
                width_mm: 36.0,
                height_mm: 14.0,
                module_scale: 2,
                rotation: Rotation::Deg90,
                show_text: true,
                text_font_pt: 8.0,
            },
            desc_fields: [
                TextFieldConfig {
                    enabled: true,
                    x_mm: 23.0,
                    y_mm: 4.0,
                    width_mm: 34.0,
                    font_size_pt: 10.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 23.0,
                    y_mm: 13.0,
                    width_mm: 34.0,
                    font_size_pt: 8.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 23.0,
                    y_mm: 22.0,
                    width_mm: 34.0,
                    font_size_pt: 8.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 23.0,
                    y_mm: 30.0,
                    width_mm: 34.0,
                    font_size_pt: 8.0,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
            ],
        }
    }

    /// Varsayılan 4 Sütunlu (40x20 mm, 2 mm sütun boşluklu) termal rulo şablonu.
    pub fn default_4_col_40x20() -> Self {
        Self {
            columns: 4,
            label_width_mm: 40.0,
            label_height_mm: 20.0,
            horizontal_gap_mm: 2.0,
            margin_left_mm: 1.0,
            margin_top_mm: 0.5,
            dpi_override: None,
            barcode_kind: BarcodeKind::Code128,
            barcode: BarcodeFieldConfig {
                x_mm: 2.0,
                y_mm: 8.5,
                width_mm: 36.0,
                height_mm: 7.5,
                module_scale: 1,
                rotation: Rotation::Deg0,
                show_text: true,
                text_font_pt: 7.0,
            },
            desc_fields: [
                TextFieldConfig {
                    enabled: true,
                    x_mm: 2.0,
                    y_mm: 1.5,
                    width_mm: 36.0,
                    font_size_pt: 7.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: true,
                    x_mm: 2.0,
                    y_mm: 5.0,
                    width_mm: 36.0,
                    font_size_pt: 6.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: false,
                    x_mm: 2.0,
                    y_mm: 15.0,
                    width_mm: 36.0,
                    font_size_pt: 6.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
                TextFieldConfig {
                    enabled: false,
                    x_mm: 20.0,
                    y_mm: 15.0,
                    width_mm: 18.0,
                    font_size_pt: 6.5,
                    rotation: Rotation::Deg0,
                    align: TextAlign::Left,
                },
            ],
        }
    }
}

/// Kayıt grubu modeli.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub created_at: String,
}

/// Barkod ve Tanım 1..4 alanlarını içeren kayıt modeli.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarcodeRecord {
    pub id: Option<i64>,
    pub group_id: i64,
    pub barcode: String,
    pub desc1: String,
    pub desc2: String,
    pub desc3: String,
    pub desc4: String,
    pub print_count: u32,
}

impl BarcodeRecord {
    pub fn new_empty(group_id: i64) -> Self {
        Self {
            id: None,
            group_id,
            barcode: String::new(),
            desc1: String::new(),
            desc2: String::new(),
            desc3: String::new(),
            desc4: String::new(),
            print_count: 1,
        }
    }

    pub fn sample_preview() -> Self {
        Self {
            id: None,
            group_id: 1,
            barcode: "8691234567890".to_string(),
            desc1: "MRD-3102".to_string(),
            desc2: "PASLANMAZ ÇELİK KAÇEROLA\n18x8 cm • 1,8 Lt".to_string(),
            desc3: "AISI 304 KALİTE".to_string(),
            desc4: "MERDURA".to_string(),
            print_count: 1,
        }
    }
}

/// Hem virgül (,) hem nokta (.) kabul eden bölgesel ayar dostu f32 ayrıştırıcı.
pub fn parse_flexible_f32(input: &str) -> Option<f32> {
    let cleaned = input.trim().replace(',', ".");
    if cleaned.is_empty() {
        return None;
    }
    let val = cleaned.parse::<f32>().ok()?;
    if val.is_finite() {
        Some(val)
    } else {
        None
    }
}

/// İlk çalıştırmada ./templates klasörünü ve 3 hazır stüdyo şablonunu oluşturur.
pub fn ensure_default_templates(templates_dir: &Path) -> Result<()> {
    if !templates_dir.exists() {
        fs::create_dir_all(templates_dir)
            .with_context(|| format!("Şablon klasörü oluşturulamadı: {:?}", templates_dir))?;
    }

    // Klasörde halihazırda kullanıcı şablonu varsa ekstra dosya dayatma; yalnızca boşsa varsayılanları üret
    if !list_templates(templates_dir).is_empty() {
        return Ok(());
    }

    let defaults = [
        ("Standart_Yatay_60x40.json", LabelTemplate::default_1_col_horizontal()),
        ("Dikey_Etiket_60x40.json", LabelTemplate::default_1_col_vertical()),
        ("Coklu_Rulo_4x_40x20.json", LabelTemplate::default_4_col_40x20()),
    ];

    for (file_name, tpl) in defaults {
        let path = templates_dir.join(file_name);
        if !path.exists() {
            save_template(templates_dir, file_name, &tpl)?;
        }
    }

    Ok(())
}

/// Klasördeki tüm .json şablon dosya adlarını alfabetik sıralı döner.
pub fn list_templates(templates_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(templates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(fname) = path.file_name().and_then(|f| f.to_str())
            {
                names.push(fname.to_string());
            }
        }
    }
    names.sort();
    names
}

/// JSON şablon dosyasını yükler ve tüm sayısal alanları sanitize eder.
pub fn load_template(templates_dir: &Path, file_name: &str) -> Result<LabelTemplate> {
    let path = templates_dir.join(file_name);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Şablon dosyası okunamadı: {:?}", path))?;
    let mut tpl: LabelTemplate = serde_json::from_str(&content)
        .with_context(|| format!("Şablon JSON formatı hatalı: {:?}", path))?;
    tpl.sanitize();
    Ok(tpl)
}

/// Şablonu JSON standardında (noktalı ondalık) kaydeder.
pub fn save_template(templates_dir: &Path, file_name: &str, tpl: &LabelTemplate) -> Result<()> {
    if !templates_dir.exists() {
        fs::create_dir_all(templates_dir)?;
    }
    let mut sanitized = tpl.clone();
    sanitized.sanitize();

    let normalized_name = if file_name.to_lowercase().ends_with(".json") {
        file_name.to_string()
    } else {
        format!("{file_name}.json")
    };

    let path = templates_dir.join(normalized_name);
    let json = serde_json::to_string_pretty(&sanitized)?;
    fs::write(&path, json).with_context(|| format!("Şablon kaydedilemedi: {:?}", path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_flexible_f32_comma_and_dot() {
        assert_eq!(parse_flexible_f32("40,5"), Some(40.5));
        assert_eq!(parse_flexible_f32("40.5"), Some(40.5));
        assert_eq!(parse_flexible_f32("  1,25  "), Some(1.25));
        assert_eq!(parse_flexible_f32(""), None);
        assert_eq!(parse_flexible_f32("NaN"), None);
        assert_eq!(parse_flexible_f32("abc"), None);
    }

    #[test]
    fn test_template_sanitize_prevents_zero_or_negative() {
        let mut tpl = LabelTemplate::default_1_col_horizontal();
        tpl.columns = 0;
        tpl.label_width_mm = 0.0;
        tpl.label_height_mm = -15.0;
        tpl.horizontal_gap_mm = -3.0;
        tpl.barcode.width_mm = 0.0;
        tpl.barcode.height_mm = f32::NAN;
        tpl.sanitize();

        assert_eq!(tpl.columns, 1);
        assert!(tpl.label_width_mm >= 5.0);
        assert!(tpl.label_height_mm >= 5.0);
        assert_eq!(tpl.horizontal_gap_mm, 0.0);
        assert!(tpl.barcode.width_mm >= 4.0);
        assert!(tpl.barcode.height_mm >= 3.0);
    }
}
