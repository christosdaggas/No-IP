mod app;
mod config;
mod keyring;
mod service;
mod tasks;
mod theme;
mod widgets;

use std::sync::Arc;

use eframe::egui;

use config::AppConfig;

const LOGO_PNG: &[u8] = include_bytes!("../../../logo.png");

fn main() -> eframe::Result<()> {
    env_logger::init();
    let cfg = AppConfig::load();

    let icon = image::load_from_memory(LOGO_PNG)
        .map(|img| {
            let rgba = img.to_rgba8();
            egui::IconData {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            }
        })
        .ok();

    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([660.0, 820.0])
        .with_min_inner_size([560.0, 560.0])
        .with_app_id("com.noip.DUC");
    if let Some(icon) = icon {
        vp = vp.with_icon(Arc::new(icon));
    }

    eframe::run_native(
        "No-IP DUC",
        eframe::NativeOptions {
            viewport: vp,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::DucApp::new(cc, cfg)))),
    )
}
