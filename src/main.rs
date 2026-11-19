#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod db;
mod i18n;
mod importer;
mod models;
mod printer;
mod renderer;

use app::BarkodStudyoApp;
use db::Database;
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, IconData, ViewportBuilder};
use i18n::I18n;
use renderer::FontEngine;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

const EMBEDDED_WINDOW_ICON: &[u8] = include_bytes!("../assets/icon_64.png");

/// Gömülü PNG baytlarını RGBA8 piksel tamponuna çözer (ek kütüphane şişkinliği olmadan).
pub fn decode_png_rgba(png_bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let buf_size = reader.output_buffer_size()?;
    let mut buf = vec![0u8; buf_size];
    let info = reader.next_frame(&mut buf).ok()?;
    let raw = &buf[..info.buffer_size()];

    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for chunk in raw.chunks_exact(3) {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            out
        }
        _ => return None,
    };

    Some((rgba, info.width, info.height))
}

/// Windows'ta taşınabilir çalışma dizinini (`.`), macOS'ta ise App Store Sandbox uyumlu
/// `~/Library/Application Support/com.merdura.barkodstudyo` dizinini döner.
fn resolve_data_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let dir = PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.merdura.barkodstudyo");
            let _ = std::fs::create_dir_all(&dir);
            return dir;
        }
    }
    PathBuf::from(".")
}

fn configure_egui_fonts(ctx: &egui::Context, font_engine: &FontEngine) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "turkish_sys_font".to_owned(),
        Arc::new(FontData::from_owned(font_engine.raw_font_bytes().to_vec())),
    );

    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "turkish_sys_font".to_owned());
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.push("turkish_sys_font".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let root = resolve_data_root();
    let db = Database::open(&root.join("data.json")).expect("data.json veri deposu başlatılamadı");
    let i18n = I18n::init(&root.join("locales"));
    let font_engine = FontEngine::load();
    let templates_dir = root.join("templates");

    let initial_title = i18n.t("app_title").to_string();

    let mut viewport = ViewportBuilder::default()
        .with_title(initial_title)
        .with_inner_size([1180.0, 740.0])
        .with_min_inner_size([920.0, 580.0]);

    if let Some((rgba, width, height)) = decode_png_rgba(EMBEDDED_WINDOW_ICON) {
        viewport = viewport.with_icon(Arc::new(IconData {
            rgba,
            width,
            height,
        }));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "barkod_studyo",
        native_options,
        Box::new(move |cc| {
            configure_egui_fonts(&cc.egui_ctx, &font_engine);
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(BarkodStudyoApp::new(
                db,
                i18n,
                font_engine,
                templates_dir,
            )))
        }),
    )
}
