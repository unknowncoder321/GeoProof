#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod analysis;
mod app;
mod detect;
mod media;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GeoProof")
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([980.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "GeoProof",
        options,
        Box::new(|cc| {
            app::apply_theme(&cc.egui_ctx);
            Ok(Box::new(app::GeoProofApp::default()))
        }),
    )
}
