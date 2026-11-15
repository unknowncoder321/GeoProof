#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod analysis;
mod app;
mod detect;
mod media;
mod theme;
mod timeline;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GeoProof")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1100.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "GeoProof",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::GeoProofApp::default()))
        }),
    )
}
