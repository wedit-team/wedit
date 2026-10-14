#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod document;
mod editing;
mod explorer;
mod syntax;
mod theme;

use std::sync::Arc;

pub const ICON_64: &[u8] = include_bytes!("../../../assets/wedit_64.rgba");
pub const LOGO_128: &[u8] = include_bytes!("../../../assets/wedit_128.rgba");

fn main() -> eframe::Result {
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Wedit")
            .with_app_id("wedit")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([520.0, 320.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(egui::IconData { rgba: ICON_64.to_vec(), width: 64, height: 64 })),
        ..Default::default()
    };

    eframe::run_native("Wedit", options, Box::new(move |cc| Ok(Box::new(app::WeditApp::new(cc, args)))))
}
