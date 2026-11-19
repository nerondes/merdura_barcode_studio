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
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, ViewportBuilder};
use i18n::I18n;
use renderer::FontEngine;
use std::path::PathBuf;
use std::sync::Arc;

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

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(initial_title)
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([920.0, 580.0]),
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
