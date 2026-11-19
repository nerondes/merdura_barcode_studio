use crate::models::{
    BarcodeFieldConfig, BarcodeKind, BarcodeRecord, LabelTemplate, Rotation, TextAlign,
};
use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use barcoders::sym::code128::Code128;
use barcoders::sym::ean13::EAN13;
use qrcode::{Color as QrColor, QrCode};
use std::fs;
use std::ops::Index;
use std::path::Path;

/// Harici `image` (ve 160 alt video/resim kodek) bağımlılığını ortadan kaldıran %100 saf Rust RGBA piksel yapısı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Index<usize> for Rgba {
    type Output = u8;
    #[inline]
    fn index(&self, idx: usize) -> &Self::Output {
        &self.0[idx]
    }
}

#[derive(Debug, Clone)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbaImage {
    pub fn from_pixel(width: u32, height: u32, pixel: Rgba) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let total_px = (w as usize).saturating_mul(h as usize);
        let mut data = Vec::with_capacity(total_px * 4);
        for _ in 0..total_px {
            data.extend_from_slice(&pixel.0);
        }
        Self {
            width: w,
            height: h,
            data,
        }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    pub fn as_raw(&self) -> &[u8] {
        &self.data
    }

    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Rgba {
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Rgba([
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ])
    }

    #[inline]
    pub fn put_pixel(&mut self, x: u32, y: u32, pixel: Rgba) {
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.data[idx..idx + 4].copy_from_slice(&pixel.0);
    }

    pub fn pixels(&self) -> impl Iterator<Item = Rgba> + '_ {
        self.data
            .chunks_exact(4)
            .map(|c| Rgba([c[0], c[1], c[2], c[3]]))
    }

    /// Saat yönünde 90° tam sayı matris rotasyonu.
    pub fn rotate90(&self) -> Self {
        let mut out = Self::from_pixel(self.height, self.width, WHITE);
        for y in 0..self.height {
            for x in 0..self.width {
                out.put_pixel(self.height - 1 - y, x, self.get_pixel(x, y));
            }
        }
        out
    }

    /// 180° tam sayı matris rotasyonu.
    pub fn rotate180(&self) -> Self {
        let mut out = Self::from_pixel(self.width, self.height, WHITE);
        for y in 0..self.height {
            for x in 0..self.width {
                out.put_pixel(
                    self.width - 1 - x,
                    self.height - 1 - y,
                    self.get_pixel(x, y),
                );
            }
        }
        out
    }

    /// Saat yönünde 270° (saat yönü tersi 90°) tam sayı matris rotasyonu.
    pub fn rotate270(&self) -> Self {
        let mut out = Self::from_pixel(self.height, self.width, WHITE);
        for y in 0..self.height {
            for x in 0..self.width {
                out.put_pixel(y, self.width - 1 - x, self.get_pixel(x, y));
            }
        }
        out
    }

    /// Windows dışı geliştirme testleri için sıfır kütüphaneli standart 32-bit BMP çıktısı yazar.
    #[cfg(not(windows))]
    pub fn save_bmp(&self, path: &Path) -> std::io::Result<()> {
        let pixel_bytes = (self.width * self.height * 4) as u32;
        let file_size = 54 + pixel_bytes;
        let mut out = Vec::with_capacity(file_size as usize);

        // BITMAPFILEHEADER (14 bytes)
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&file_size.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes());

        // BITMAPINFOHEADER (40 bytes, negatif yükseklik = top-down)
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(self.width as i32).to_le_bytes());
        out.extend_from_slice(&(-(self.height as i32)).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&pixel_bytes.to_le_bytes());
        out.extend_from_slice(&7992i32.to_le_bytes());
        out.extend_from_slice(&7992i32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        for px in self.pixels() {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        fs::write(path, out)
    }
}

pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
const TRANSPARENT: Rgba = Rgba([255, 255, 255, 0]);
const WARN_BORDER: Rgba = Rgba([210, 40, 40, 255]);
const WARN_BG: Rgba = Rgba([255, 240, 240, 255]);

#[derive(Debug, Clone)]
pub struct RenderedOutput {
    pub image: RgbaImage,
    pub warning: Option<String>,
}

/// Türkçe karakter destekli font motoru (Windows/macOS/Linux sistem fontu + gömülü fallback).
#[derive(Clone)]
pub struct FontEngine {
    font: FontArc,
    raw_bytes: Vec<u8>,
}

impl FontEngine {
    pub fn load() -> Self {
        let candidate_paths = [
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\tahoma.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Tahoma.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        ];

        for p in candidate_paths {
            let path = Path::new(p);
            if path.exists()
                && let Ok(bytes) = fs::read(path)
                && let Ok(font) = FontArc::try_from_vec(bytes.clone())
            {
                return Self {
                    font,
                    raw_bytes: bytes,
                };
            }
        }

        // Sistem fontu bulunamazsa (ör. izole test ortamı) egui içindeki gömülü Latin-Extended fontu kullan
        let default_defs = egui::FontDefinitions::default();
        for data in default_defs.font_data.values() {
            let bytes = data.font.to_vec();
            if let Ok(font) = FontArc::try_from_vec(bytes.clone()) {
                return Self {
                    font,
                    raw_bytes: bytes,
                };
            }
        }

        unreachable!("egui varsayılan gömülü fontu her zaman mevcuttur");
    }

    pub fn raw_font_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

/// Matematiksel olarak güvenli mm -> piksel boyut dönüşümü (asla 0, negatif veya NaN üretmez).
pub fn mm_to_px(mm: f32, dpi: u32) -> u32 {
    if !mm.is_finite() || mm <= 0.0 {
        return 1;
    }
    let safe_dpi = dpi.clamp(36, 2400) as f64;
    (((mm as f64) * safe_dpi) / 25.4).round().max(1.0) as u32
}

/// X/Y koordinatları için güvenli mm -> piksel dönüşümü (0 mm = 0 px, negatif/NaN = 0 px).
pub fn mm_to_coord_px(mm: f32, dpi: u32) -> u32 {
    if !mm.is_finite() || mm <= 0.0 {
        return 0;
    }
    let safe_dpi = dpi.clamp(36, 2400) as f64;
    (((mm as f64) * safe_dpi) / 25.4).round().max(0.0) as u32
}

/// Tipografik punto (pt) -> piksel boyutu dönüşümü (1 inç = 72 pt).
pub fn pt_to_px(pt: f32, dpi: u32) -> f32 {
    let safe_pt = if pt.is_finite() {
        pt.clamp(3.0, 144.0)
    } else {
        8.0
    };
    let safe_dpi = dpi.clamp(36, 2400) as f32;
    ((safe_pt * safe_dpi) / 72.0).max(6.0)
}

/// Tanım alanlarındaki `+++` dizilimini ve satır sonlarını alt alta satırlara böler.
pub fn split_multiline_plus(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for segment in text.split("+++") {
        for sub in segment.lines() {
            let trimmed = sub.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
    }
    lines
}

/// Bellekteki piksel tamponuna tam sayı matris rotasyonu uygular (0°/90°/180°/270°).
pub fn apply_integer_rotation(img: RgbaImage, rotation: Rotation) -> RgbaImage {
    match rotation {
        Rotation::Deg0 => img,
        Rotation::Deg90 => img.rotate90(),
        Rotation::Deg180 => img.rotate180(),
        Rotation::Deg270 => img.rotate270(),
    }
}

/// EAN-13 için 12 haneli verinin GS1 kontrol hanesini (13. hane) hesaplar.
pub fn compute_ean13_check_digit(digits12: &[u8]) -> u8 {
    let sum: u32 = digits12
        .iter()
        .take(12)
        .enumerate()
        .map(|(idx, &b)| {
            let d = u32::from(b.saturating_sub(b'0'));
            if idx % 2 == 0 { d } else { d * 3 }
        })
        .sum();
    ((10 - (sum % 10)) % 10) as u8
}

/// EAN-13 girdisini doğrular; 12 hane ise 13. kontrol hanesini otomatik tamamlar.
/// Dönüş: Ok((ilk_12_hane, tam_13_hane))
pub fn normalize_ean13(raw: &str) -> Result<(String, String), String> {
    let trimmed = raw.trim();
    if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Err("EAN-13 yalnızca rakamlardan (0-9) oluşmalıdır.".to_string());
    }
    match trimmed.len() {
        12 => {
            let check = compute_ean13_check_digit(trimmed.as_bytes());
            let full13 = format!("{}{}", trimmed, check);
            Ok((trimmed.to_string(), full13))
        }
        13 => {
            let prefix12 = &trimmed[..12];
            let expected = compute_ean13_check_digit(prefix12.as_bytes());
            let actual = trimmed.as_bytes()[12] - b'0';
            if actual != expected {
                return Err(format!(
                    "Geçersiz EAN-13 kontrol hanesi (girilen: {}, beklenen: {}).",
                    actual, expected
                ));
            }
            Ok((prefix12.to_string(), trimmed.to_string()))
        }
        other => Err(format!(
            "EAN-13 için 12 veya 13 rakam girilmelidir (mevcut: {} karakter).",
            other
        )),
    }
}

/// Alfa karışımlı güvenli piksel birleştirme (sınır aşımı imkansız).
fn composite_rgba(canvas: &mut RgbaImage, sprite: &RgbaImage, dst_x: u32, dst_y: u32) {
    let cw = canvas.width();
    let ch = canvas.height();
    for sy in 0..sprite.height() {
        let cy = dst_y.saturating_add(sy);
        if cy >= ch {
            break;
        }
        for sx in 0..sprite.width() {
            let cx = dst_x.saturating_add(sx);
            if cx >= cw {
                break;
            }
            let sp = sprite.get_pixel(sx, sy);
            let alpha = sp[3];
            if alpha == 0 {
                continue;
            }
            if alpha == 255 {
                canvas.put_pixel(cx, cy, sp);
            } else {
                let bg = canvas.get_pixel(cx, cy);
                let a = u32::from(alpha);
                let inv_a = 255 - a;
                let r = ((u32::from(sp[0]) * a + u32::from(bg[0]) * inv_a) / 255) as u8;
                let g = ((u32::from(sp[1]) * a + u32::from(bg[1]) * inv_a) / 255) as u8;
                let b = ((u32::from(sp[2]) * a + u32::from(bg[2]) * inv_a) / 255) as u8;
                canvas.put_pixel(cx, cy, Rgba([r, g, b, 255]));
            }
        }
    }
}

fn measure_line_width_px(line: &str, px_size: f32, font_engine: &FontEngine) -> f32 {
    let scale = PxScale::from(px_size.max(6.0));
    let scaled = font_engine.font.as_scaled(scale);
    let mut cursor_x = 0.0f32;
    let mut prev_glyph = None;
    for ch in line.chars() {
        let gid = scaled.glyph_id(ch);
        if let Some(prev) = prev_glyph {
            cursor_x += scaled.kern(prev, gid);
        }
        cursor_x += scaled.h_advance(gid);
        prev_glyph = Some(gid);
    }
    cursor_x
}

/// Tek bir çok uzun kelime bile kutu genişliğini aşıyorsa karakter bazında güvenli böler.
fn break_oversized_word(
    word: &str,
    usable_px: f32,
    px_size: f32,
    font_engine: &FontEngine,
    out: &mut Vec<String>,
) -> String {
    let mut chunk = String::new();
    for ch in word.chars() {
        let mut candidate = chunk.clone();
        candidate.push(ch);
        if !chunk.is_empty() && measure_line_width_px(&candidate, px_size, font_engine) > usable_px {
            out.push(chunk);
            chunk = ch.to_string();
        } else {
            chunk = candidate;
        }
    }
    chunk
}

/// Hem `+++` manuel satır sonlarını hem de `max_width_px` genişlik sınırına göre otomatik kelime kaydırmayı (word-wrap) uygular.
pub fn wrap_text_lines(
    text: &str,
    max_width_px: u32,
    px_size: f32,
    font_engine: &FontEngine,
) -> Vec<String> {
    let base_paragraphs = split_multiline_plus(text);
    if base_paragraphs.is_empty() {
        return Vec::new();
    }

    let usable_px = (max_width_px.saturating_sub(2)).max(8) as f32;
    let mut wrapped = Vec::new();

    for para in base_paragraphs {
        if measure_line_width_px(&para, px_size, font_engine) <= usable_px {
            wrapped.push(para);
            continue;
        }

        let mut current = String::new();
        for word in para.split_whitespace() {
            if current.is_empty() {
                if measure_line_width_px(word, px_size, font_engine) <= usable_px {
                    current = word.to_string();
                } else {
                    current = break_oversized_word(word, usable_px, px_size, font_engine, &mut wrapped);
                }
            } else {
                let candidate = format!("{current} {word}");
                if measure_line_width_px(&candidate, px_size, font_engine) <= usable_px {
                    current = candidate;
                } else {
                    wrapped.push(current);
                    if measure_line_width_px(word, px_size, font_engine) <= usable_px {
                        current = word.to_string();
                    } else {
                        current =
                            break_oversized_word(word, usable_px, px_size, font_engine, &mut wrapped);
                    }
                }
            }
        }
        if !current.is_empty() {
            wrapped.push(current);
        }
    }

    wrapped
}

/// Tek veya çok satırlı metni yatay (0°) tampona çizer (kutu genişliği ve Sol/Orta/Sağ hizalama destekli).
fn render_horizontal_text_block(
    lines: &[String],
    px_size: f32,
    font_engine: &FontEngine,
    color: [u8; 3],
    opaque_white_bg: bool,
    box_width_px: Option<u32>,
    align: TextAlign,
) -> Option<RgbaImage> {
    if lines.is_empty() {
        return None;
    }

    let scale = PxScale::from(px_size.max(6.0));
    let scaled = font_engine.font.as_scaled(scale);
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let line_height = ((ascent - descent) * 1.08).ceil().max(6.0) as u32;

    // Her satırın piksel genişliğini ölç
    let mut line_widths = Vec::with_capacity(lines.len());
    let mut natural_max_w = 1u32;
    for line in lines {
        let lw = measure_line_width_px(line, px_size, font_engine).ceil().max(1.0) as u32;
        line_widths.push(lw);
        natural_max_w = natural_max_w.max(lw + 2);
    }

    let canvas_w = box_width_px
        .map(|bw| bw.max(natural_max_w))
        .unwrap_or(natural_max_w)
        .max(1);

    let total_height = (line_height.saturating_mul(lines.len() as u32))
        .saturating_add(2)
        .max(1);
    let bg = if opaque_white_bg { WHITE } else { TRANSPARENT };
    let mut img = RgbaImage::from_pixel(canvas_w, total_height, bg);

    for (line_idx, line) in lines.iter().enumerate() {
        let baseline_y = (line_idx as f32 * line_height as f32) + ascent;
        let lw = line_widths[line_idx];
        let mut cursor_x = match align {
            TextAlign::Left => 1.0f32,
            TextAlign::Center => ((canvas_w.saturating_sub(lw)) as f32 / 2.0).max(1.0),
            TextAlign::Right => (canvas_w.saturating_sub(lw).saturating_sub(1)) as f32,
        };
        let mut prev_glyph = None;

        for ch in line.chars() {
            let gid = scaled.glyph_id(ch);
            if let Some(prev) = prev_glyph {
                cursor_x += scaled.kern(prev, gid);
            }
            let glyph = gid.with_scale_and_position(scale, point(cursor_x, baseline_y));
            if let Some(outlined) = font_engine.font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height()
                    {
                        let alpha = (cov.clamp(0.0, 1.0) * 255.0).round() as u8;
                        if alpha > 0 {
                            if opaque_white_bg {
                                let inv = 255 - u32::from(alpha);
                                let r = ((u32::from(color[0]) * u32::from(alpha) + 255 * inv) / 255)
                                    as u8;
                                let g = ((u32::from(color[1]) * u32::from(alpha) + 255 * inv) / 255)
                                    as u8;
                                let b = ((u32::from(color[2]) * u32::from(alpha) + 255 * inv) / 255)
                                    as u8;
                                img.put_pixel(px as u32, py as u32, Rgba([r, g, b, 255]));
                            } else {
                                let existing = img.get_pixel(px as u32, py as u32)[3];
                                let merged_a = existing.max(alpha);
                                img.put_pixel(
                                    px as u32,
                                    py as u32,
                                    Rgba([color[0], color[1], color[2], merged_a]),
                                );
                            }
                        }
                    }
                });
            }
            cursor_x += scaled.h_advance(gid);
            prev_glyph = Some(gid);
        }
    }

    Some(img)
}

/// 1D barkod modül dizisini (0/1) ve opsiyonel alt metni önce 0° yatay tamponda çizer.
fn build_1d_barcode_horizontal(
    modules: &[u8],
    display_text: &str,
    cfg: &BarcodeFieldConfig,
    dpi: u32,
    font_engine: &FontEngine,
) -> RgbaImage {
    let target_w_px = mm_to_px(cfg.width_mm, dpi);
    let target_bar_h_px = mm_to_px(cfg.height_mm, dpi);
    let module_count = modules.len().max(1) as u32;

    // Tam sayı bar genişliği: termal yazıcıda bulanıklık olmaması için her modül tam sayı piksel genişliğinde çizilir.
    let auto_scale = (target_w_px / module_count).max(1);
    let clamped_scale = cfg.module_scale.clamp(1, 12);
    let effective_bar_px = if clamped_scale <= 1 {
        auto_scale
    } else if module_count * clamped_scale <= target_w_px.saturating_mul(5) / 4 {
        auto_scale.max(clamped_scale)
    } else {
        auto_scale
    };

    let bars_w = module_count.saturating_mul(effective_bar_px).max(1);

    let text_img = if cfg.show_text && !display_text.is_empty() {
        let px_size = pt_to_px(cfg.text_font_pt, dpi);
        render_horizontal_text_block(
            &[display_text.to_string()],
            px_size,
            font_engine,
            [0, 0, 0],
            true,
            None,
            TextAlign::Center,
        )
    } else {
        None
    };

    let text_h = text_img.as_ref().map(|t| t.height() + 2).unwrap_or(0);
    let text_w = text_img.as_ref().map(|t| t.width()).unwrap_or(0);
    let total_w = bars_w.max(text_w).max(1);
    let total_h = target_bar_h_px.saturating_add(text_h).max(1);

    let mut buf = RgbaImage::from_pixel(total_w, total_h, WHITE);
    let bar_offset_x = (total_w.saturating_sub(bars_w)) / 2;

    for (idx, &bit) in modules.iter().enumerate() {
        if bit == 1 {
            let x_start = bar_offset_x + (idx as u32) * effective_bar_px;
            for dx in 0..effective_bar_px {
                let x = x_start + dx;
                if x < total_w {
                    for y in 0..target_bar_h_px {
                        buf.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }
    }

    if let Some(t_img) = text_img {
        let tx = (total_w.saturating_sub(t_img.width())) / 2;
        let ty = target_bar_h_px.saturating_add(2);
        composite_rgba(&mut buf, &t_img, tx, ty);
    }

    buf
}

/// QR Kodu ve opsiyonel alt metni önce 0° yatay tamponda çizer.
fn build_qr_horizontal(
    qr: &QrCode,
    display_text: &str,
    cfg: &BarcodeFieldConfig,
    dpi: u32,
    font_engine: &FontEngine,
) -> RgbaImage {
    let side_mm = cfg.width_mm.min(cfg.height_mm).max(4.0);
    let target_side_px = mm_to_px(side_mm, dpi);
    let qr_modules = qr.width().max(1) as u32;
    let quiet_zone_modules = 2u32;
    let total_modules = qr_modules + quiet_zone_modules * 2;
    let mod_px = (target_side_px / total_modules).max(cfg.module_scale.clamp(1, 8)).max(1);
    let effective_mod_px = if total_modules * mod_px > target_side_px.saturating_mul(3) / 2 {
        (target_side_px / total_modules).max(1)
    } else {
        mod_px
    };

    let qr_px_size = total_modules * effective_mod_px;
    let colors = qr.to_colors();

    let text_img = if cfg.show_text && !display_text.is_empty() {
        let px_size = pt_to_px(cfg.text_font_pt, dpi);
        render_horizontal_text_block(
            &[display_text.to_string()],
            px_size,
            font_engine,
            [0, 0, 0],
            true,
            None,
            TextAlign::Center,
        )
    } else {
        None
    };

    let text_h = text_img.as_ref().map(|t| t.height() + 2).unwrap_or(0);
    let text_w = text_img.as_ref().map(|t| t.width()).unwrap_or(0);
    let canvas_w = qr_px_size.max(text_w).max(1);
    let canvas_h = qr_px_size.saturating_add(text_h).max(1);

    let mut buf = RgbaImage::from_pixel(canvas_w, canvas_h, WHITE);
    let qr_offset_x = (canvas_w.saturating_sub(qr_px_size)) / 2;

    for my in 0..qr_modules {
        for mx in 0..qr_modules {
            if colors[(my * qr_modules + mx) as usize] == QrColor::Dark {
                let start_x = qr_offset_x + (mx + quiet_zone_modules) * effective_mod_px;
                let start_y = (my + quiet_zone_modules) * effective_mod_px;
                for dy in 0..effective_mod_px {
                    for dx in 0..effective_mod_px {
                        buf.put_pixel(start_x + dx, start_y + dy, Rgba([0, 0, 0, 255]));
                    }
                }
            }
        }
    }

    if let Some(t_img) = text_img {
        let tx = (canvas_w.saturating_sub(t_img.width())) / 2;
        composite_rgba(&mut buf, &t_img, tx, qr_px_size + 2);
    }

    buf
}

/// Geçersiz barkod verisinde çökmeden önizlemede anlaşılır uyarı kutusu çizer.
fn build_warning_box(
    label: &str,
    cfg: &BarcodeFieldConfig,
    dpi: u32,
    font_engine: &FontEngine,
) -> RgbaImage {
    let w = mm_to_px(cfg.width_mm, dpi).max(40);
    let h = mm_to_px(cfg.height_mm, dpi).max(20);
    let mut box_img = RgbaImage::from_pixel(w, h, WARN_BG);

    for x in 0..w {
        box_img.put_pixel(x, 0, WARN_BORDER);
        box_img.put_pixel(x, h - 1, WARN_BORDER);
    }
    for y in 0..h {
        box_img.put_pixel(0, y, WARN_BORDER);
        box_img.put_pixel(w - 1, y, WARN_BORDER);
    }

    if let Some(txt) = render_horizontal_text_block(
        &[label.to_string()],
        pt_to_px(7.5, dpi),
        font_engine,
        [190, 20, 20],
        false,
        None,
        TextAlign::Center,
    ) {
        let tx = (w.saturating_sub(txt.width())) / 2;
        let ty = (h.saturating_sub(txt.height())) / 2;
        composite_rgba(&mut box_img, &txt, tx, ty);
    }

    box_img
}

fn encode_barcode_sprite(
    raw_data: &str,
    kind: BarcodeKind,
    cfg: &BarcodeFieldConfig,
    dpi: u32,
    font_engine: &FontEngine,
) -> (RgbaImage, Option<String>) {
    let data = raw_data.trim();
    if data.is_empty() {
        let warn_box = build_warning_box("BOŞ BARKOD", cfg, dpi, font_engine);
        return (
            apply_integer_rotation(warn_box, cfg.rotation),
            Some("Barkod alanı boş.".to_string()),
        );
    }

    match kind {
        BarcodeKind::Code128 => {
            if !data.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
                let warn_box = build_warning_box("CODE128: SADECE ASCII", cfg, dpi, font_engine);
                return (
                    apply_integer_rotation(warn_box, cfg.rotation),
                    Some("Code128 yalnızca standart ASCII karakterleri destekler (Türkçe harf kullanılamaz).".to_string()),
                );
            }

            // Çift haneli saf rakamlarda Set C (\u{0106}) ile daha kompakt barkod, diğerlerinde Set B (\u{0181})
            let encoded_res = if data.len() >= 4
                && data.len().is_multiple_of(2)
                && data.bytes().all(|b| b.is_ascii_digit())
            {
                Code128::new(format!("\u{0106}{data}"))
                    .or_else(|_| Code128::new(format!("\u{0181}{data}")))
            } else {
                Code128::new(format!("\u{0181}{data}"))
            };

            match encoded_res {
                Ok(c128) => {
                    let modules = c128.encode();
                    let horiz = build_1d_barcode_horizontal(&modules, data, cfg, dpi, font_engine);
                    (apply_integer_rotation(horiz, cfg.rotation), None)
                }
                Err(e) => {
                    let warn_box = build_warning_box("GEÇERSİZ CODE128", cfg, dpi, font_engine);
                    (
                        apply_integer_rotation(warn_box, cfg.rotation),
                        Some(format!("Code128 kodlama hatası: {e}")),
                    )
                }
            }
        }
        BarcodeKind::Ean13 => match normalize_ean13(data) {
            Ok((prefix12, full13)) => {
                let encoded_res = EAN13::new(&prefix12).or_else(|_| EAN13::new(&full13));
                match encoded_res {
                    Ok(ean) => {
                        let modules = ean.encode();
                        let horiz =
                            build_1d_barcode_horizontal(&modules, &full13, cfg, dpi, font_engine);
                        (apply_integer_rotation(horiz, cfg.rotation), None)
                    }
                    Err(e) => {
                        let warn_box = build_warning_box("GEÇERSİZ EAN-13", cfg, dpi, font_engine);
                        (
                            apply_integer_rotation(warn_box, cfg.rotation),
                            Some(format!("EAN-13 kodlama hatası: {e}")),
                        )
                    }
                }
            }
            Err(msg) => {
                let warn_box = build_warning_box("GEÇERSİZ EAN-13", cfg, dpi, font_engine);
                (apply_integer_rotation(warn_box, cfg.rotation), Some(msg))
            }
        },
        BarcodeKind::QrCode => match QrCode::new(data.as_bytes()) {
            Ok(qr) => {
                let horiz = build_qr_horizontal(&qr, data, cfg, dpi, font_engine);
                (apply_integer_rotation(horiz, cfg.rotation), None)
            }
            Err(e) => {
                let warn_box = build_warning_box("GEÇERSİZ QR KOD", cfg, dpi, font_engine);
                (
                    apply_integer_rotation(warn_box, cfg.rotation),
                    Some(format!("QR Kod hatası: {e}")),
                )
            }
        },
    }
}

/// Tek bir etiketi (label_width_mm x label_height_mm) belirtilen DPI değerinde bellekte çizer.
pub fn render_single_label(
    record: &BarcodeRecord,
    template: &LabelTemplate,
    dpi: u32,
    font_engine: &FontEngine,
) -> RenderedOutput {
    let mut tpl = template.clone();
    tpl.sanitize();

    let safe_dpi = dpi.clamp(36, 2400);
    let label_w_px = mm_to_px(tpl.label_width_mm, safe_dpi);
    let label_h_px = mm_to_px(tpl.label_height_mm, safe_dpi);
    let mut canvas = RgbaImage::from_pixel(label_w_px, label_h_px, WHITE);

    // 1. Barkod Çizimi (0° çiz -> tam sayı matris rotasyonu -> tuvale yapıştır)
    let (barcode_sprite, warning) = encode_barcode_sprite(
        &record.barcode,
        tpl.barcode_kind,
        &tpl.barcode,
        safe_dpi,
        font_engine,
    );
    let bx = mm_to_coord_px(tpl.barcode.x_mm, safe_dpi);
    let by = mm_to_coord_px(tpl.barcode.y_mm, safe_dpi);
    composite_rgba(&mut canvas, &barcode_sprite, bx, by);

    // 2. Tanım 1..4 Alanları (+++ manuel + Genişlik (mm) otomatik kelime kaydırma + bağımsız döndürme/hizalama)
    let descs = [
        &record.desc1,
        &record.desc2,
        &record.desc3,
        &record.desc4,
    ];
    for (idx, field_cfg) in tpl.desc_fields.iter().enumerate() {
        if !field_cfg.enabled || descs[idx].trim().is_empty() {
            continue;
        }

        // Genişlik 0 ise metnin okuma eksenindeki kalan etiket boyunu otomatik baz al
        let effective_width_mm = if field_cfg.width_mm > 0.0 {
            field_cfg.width_mm
        } else {
            match field_cfg.rotation {
                Rotation::Deg0 | Rotation::Deg180 => {
                    (tpl.label_width_mm - field_cfg.x_mm - 1.0).max(4.0)
                }
                Rotation::Deg90 | Rotation::Deg270 => {
                    (tpl.label_height_mm - field_cfg.y_mm - 1.0).max(4.0)
                }
            }
        };

        let max_width_px = mm_to_px(effective_width_mm, safe_dpi);
        let px_size = pt_to_px(field_cfg.font_size_pt, safe_dpi);
        let lines = wrap_text_lines(descs[idx], max_width_px, px_size, font_engine);
        if lines.is_empty() {
            continue;
        }

        if let Some(horiz_txt) = render_horizontal_text_block(
            &lines,
            px_size,
            font_engine,
            [0, 0, 0],
            false,
            Some(max_width_px),
            field_cfg.align,
        ) {
            let rotated_txt = apply_integer_rotation(horiz_txt, field_cfg.rotation);
            let tx = mm_to_coord_px(field_cfg.x_mm, safe_dpi);
            let ty = mm_to_coord_px(field_cfg.y_mm, safe_dpi);
            composite_rgba(&mut canvas, &rotated_txt, tx, ty);
        }
    }

    RenderedOutput {
        image: canvas,
        warning,
    }
}

/// Kayıt listesini her kaydın `print_count` adedi kadar çoğaltır.
pub fn expand_records_by_count(records: &[BarcodeRecord]) -> Vec<BarcodeRecord> {
    let mut expanded = Vec::new();
    for rec in records {
        let copies = rec.print_count.clamp(1, 10_000);
        for _ in 0..copies {
            expanded.push(rec.clone());
        }
    }
    expanded
}

/// Çoklu sütunlu termal rulo kuralına göre yatay satırları (1 satır = 1 yazıcı sayfası) üretir.
/// `draw_preview_guides` true ise canlı önizleme için etiket sınır çizgilerini açık gri olarak belirtir.
pub fn render_roll_rows(
    records: &[BarcodeRecord],
    template: &LabelTemplate,
    dpi: u32,
    font_engine: &FontEngine,
    draw_preview_guides: bool,
) -> (Vec<RgbaImage>, Option<String>) {
    let mut tpl = template.clone();
    tpl.sanitize();

    let safe_dpi = dpi.clamp(36, 2400);
    let cols = tpl.columns.clamp(1, 8) as usize;
    let expanded = expand_records_by_count(records);

    if expanded.is_empty() {
        return (Vec::new(), None);
    }

    let row_w_px = mm_to_px(tpl.total_row_width_mm(), safe_dpi);
    let row_h_px = mm_to_px(tpl.total_row_height_mm(), safe_dpi);
    let margin_left_px = mm_to_coord_px(tpl.margin_left_mm, safe_dpi);
    let margin_top_px = mm_to_coord_px(tpl.margin_top_mm, safe_dpi);
    let label_w_px = mm_to_px(tpl.label_width_mm, safe_dpi);
    let label_h_px = mm_to_px(tpl.label_height_mm, safe_dpi);
    let h_gap_px = mm_to_coord_px(tpl.horizontal_gap_mm, safe_dpi);

    let bg_color = if draw_preview_guides && cols > 1 {
        Rgba([244, 246, 249, 255])
    } else {
        WHITE
    };

    let mut pages = Vec::new();
    let mut first_warning: Option<String> = None;

    for chunk in expanded.chunks(cols) {
        let mut row_img = RgbaImage::from_pixel(row_w_px, row_h_px, bg_color);

        for col_idx in 0..cols {
            let slot_x = margin_left_px + (col_idx as u32) * (label_w_px + h_gap_px);
            let slot_y = margin_top_px;

            if let Some(rec) = chunk.get(col_idx) {
                let single = render_single_label(rec, &tpl, safe_dpi, font_engine);
                if first_warning.is_none() && single.warning.is_some() {
                    first_warning = single.warning;
                }
                composite_rgba(&mut row_img, &single.image, slot_x, slot_y);

                if draw_preview_guides {
                    draw_guide_rect(&mut row_img, slot_x, slot_y, label_w_px, label_h_px);
                }
            } else if draw_preview_guides {
                // Son satırdaki boş rulo yuvalarını önizlemede açık kesikli çerçeve olarak göster
                draw_guide_rect(&mut row_img, slot_x, slot_y, label_w_px, label_h_px);
            }
        }

        pages.push(row_img);
    }

    (pages, first_warning)
}

fn draw_guide_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32) {
    let border = Rgba([195, 202, 212, 255]);
    let max_x = (x + w).min(img.width());
    let max_y = (y + h).min(img.height());
    if x >= max_x || y >= max_y {
        return;
    }
    for px in x..max_x {
        img.put_pixel(px, y, border);
        img.put_pixel(px, max_y - 1, border);
    }
    for py in y..max_y {
        img.put_pixel(x, py, border);
        img.put_pixel(max_x - 1, py, border);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mm_to_px_zero_negative_nan_safety() {
        assert_eq!(mm_to_px(0.0, 203), 1);
        assert_eq!(mm_to_px(-25.0, 203), 1);
        assert_eq!(mm_to_px(f32::NAN, 203), 1);
        assert_eq!(mm_to_px(f32::INFINITY, 203), 1);
        // 25.4 mm (1 inç) @ 203 DPI == tam 203 piksel
        assert_eq!(mm_to_px(25.4, 203), 203);
        // 0 DPI girilse bile clamp(36, 2400) ile çökmez
        assert!(mm_to_px(40.0, 0) >= 1);
    }

    #[test]
    fn test_split_multiline_plus() {
        let lines = split_multiline_plus("Üst Satır +++ Orta Satır +++ ğüşıöçĞÜŞİÖÇ");
        assert_eq!(lines, vec!["Üst Satır", "Orta Satır", "ğüşıöçĞÜŞİÖÇ"]);
        assert!(split_multiline_plus("   ").is_empty());
    }

    #[test]
    fn test_ean13_validation_and_checksum_completion() {
        // 12 hane girildiğinde 13. kontrol hanesini otomatik tamamlar
        let (p12, full13) = normalize_ean13("869123456789").unwrap();
        assert_eq!(p12, "869123456789");
        assert_eq!(full13.len(), 13);

        // Tamamlanan 13 haneli barkod tekrar doğrulandığında geçerli olmalıdır
        let (_, verified13) = normalize_ean13(&full13).unwrap();
        assert_eq!(verified13, full13);

        // Yanlış kontrol haneli 13 hane reddedilmelidir (çökmeden Err döner)
        let wrong_last = if full13.ends_with('0') { '1' } else { '0' };
        let corrupted = format!("{}{}", &full13[..12], wrong_last);
        assert!(normalize_ean13(&corrupted).is_err());
    }

    #[test]
    fn test_vertical_rotations_0_90_180_270_and_zero_mm_extremes() {
        let font_engine = FontEngine::load();
        let rec = BarcodeRecord::sample_preview();

        for rot in Rotation::ALL {
            let mut tpl = LabelTemplate::default_1_col_vertical();
            tpl.barcode.rotation = *rot;
            tpl.desc_fields[0].rotation = *rot;

            let out_c128 = render_single_label(&rec, &tpl, 203, &font_engine);
            assert!(out_c128.warning.is_none());
            assert!(out_c128.image.width() > 10 && out_c128.image.height() > 10);

            tpl.barcode_kind = BarcodeKind::Ean13;
            let mut ean_rec = rec.clone();
            ean_rec.barcode = "869123456789".to_string(); // 12 hane otomatik tamamlama
            let out_ean = render_single_label(&ean_rec, &tpl, 203, &font_engine);
            assert!(out_ean.warning.is_none());

            tpl.barcode_kind = BarcodeKind::QrCode;
            let out_qr = render_single_label(&rec, &tpl, 203, &font_engine);
            assert!(out_qr.warning.is_none());
        }

        // Uç değer testi: 0 mm etiket boyutu / NaN koordinat girilse dahi FPU hatası vermez
        let mut extreme_tpl = LabelTemplate::default_1_col_vertical();
        extreme_tpl.label_width_mm = 0.0;
        extreme_tpl.label_height_mm = 0.0;
        extreme_tpl.barcode.width_mm = 0.0;
        extreme_tpl.barcode.height_mm = -10.0;
        extreme_tpl.barcode.rotation = Rotation::Deg270;
        let safe_out = render_single_label(&rec, &extreme_tpl, 203, &font_engine);
        assert!(safe_out.image.width() >= 1 && safe_out.image.height() >= 1);
    }

    #[test]
    fn test_word_wrap_long_turkish_description_fits_box() {
        let font_engine = FontEngine::load();
        let px_size = pt_to_px(8.0, 203);
        let max_w_px = mm_to_px(28.0, 203); // 28 mm dar dikey etiket yüksekliği
        let wrapped = wrap_text_lines(
            "KESİK AĞIZ KAÇEROLA TABANSIZ 430",
            max_w_px,
            px_size,
            &font_engine,
        );
        assert!(wrapped.len() >= 2);
        for line in &wrapped {
            assert!(measure_line_width_px(line, px_size, &font_engine) <= max_w_px as f32);
        }
    }

    #[test]
    fn test_multi_column_51_records_into_13_rows() {
        let font_engine = FontEngine::load();
        let tpl = LabelTemplate::default_4_col_40x20();
        let rec = BarcodeRecord {
            id: Some(1),
            group_id: 1,
            barcode: "MRD-001".to_string(),
            desc1: "Test".to_string(),
            desc2: String::new(),
            desc3: String::new(),
            desc4: String::new(),
            print_count: 51,
        };
        let (rows, warn) = render_roll_rows(&[rec], &tpl, 203, &font_engine, false);
        assert!(warn.is_none());
        // PDF kuralı: 51 kayıt 4 sütunlu şablonda 13 yatay rulo satırı üretir
        assert_eq!(rows.len(), 13);
    }
}
