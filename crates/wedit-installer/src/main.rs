#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Wedit Setup: a self-contained installer / uninstaller written in Rust.
//!
//! * `wedit-setup.exe`                    – interactive install
//! * `wedit-setup.exe --quiet`            – silent install with default options
//! * `uninstall.exe --uninstall [--quiet]` – remove Wedit

mod system;

use egui::{Align, Color32, RichText};
use std::path::PathBuf;
use std::sync::Arc;
use system::*;

const ICON_64: &[u8] = include_bytes!("../../../assets/wedit_64.rgba");
const LOGO_128: &[u8] = include_bytes!("../../../assets/wedit_128.rgba");
const GOLD: Color32 = Color32::from_rgb(232, 178, 40);

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uninstall_mode = args.iter().any(|a| a == "--uninstall")
        || std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.eq_ignore_ascii_case(UNINSTALLER_NAME))).unwrap_or(false);
    let quiet = args.iter().any(|a| a == "--quiet" || a == "/S");

    if quiet {
        let code = if uninstall_mode { quiet_uninstall() } else { quiet_install() };
        std::process::exit(code);
    }

    let title = if uninstall_mode { "Uninstall Wedit" } else { "Wedit Setup" };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([600.0, 430.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(Arc::new(egui::IconData { rgba: ICON_64.to_vec(), width: 64, height: 64 })),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(move |cc| Ok(Box::new(Setup::new(cc, uninstall_mode)))))
}

fn quiet_install() -> i32 {
    let o = Options {
        install_dir: existing_install().map(|e| e.0).unwrap_or_else(default_install_dir),
        start_menu: true,
        desktop: false,
        context_menu: true,
        associate: true,
        add_to_path: false,
    };
    for (_, step) in install_steps() {
        if let Err(e) = step(&o) {
            eprintln!("{e}");
            return 1;
        }
    }
    0
}

fn quiet_uninstall() -> i32 {
    let dir = uninstall_dir();
    match uninstall(&dir) {
        Ok(()) => {
            schedule_folder_removal(&dir);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn uninstall_dir() -> PathBuf {
    existing_install()
        .map(|e| e.0)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)))
        .unwrap_or_else(default_install_dir)
}

enum Page {
    Welcome,
    Installing { step: usize },
    Done,
    Failed(String),
    ConfirmUninstall,
    Uninstalled,
}

struct Setup {
    page: Page,
    opts: Options,
    path_text: String,
    existing: Option<(PathBuf, String)>,
    launch_after: bool,
    logo: egui::TextureHandle,
    log: Vec<String>,
}

impl Setup {
    fn new(cc: &eframe::CreationContext<'_>, uninstall_mode: bool) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = Color32::from_rgb(30, 30, 34);
        v.selection.bg_fill = GOLD.gamma_multiply(0.6);
        v.hyperlink_color = GOLD;
        v.widgets.hovered.bg_stroke.color = GOLD.gamma_multiply(0.6);
        cc.egui_ctx.set_visuals(v);
        cc.egui_ctx.style_mut(|s| {
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.5));
            s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
            s.spacing.button_padding = egui::vec2(14.0, 6.0);
        });

        let existing = existing_install();
        let install_dir = existing.as_ref().map(|e| e.0.clone()).unwrap_or_else(default_install_dir);
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([128, 128], LOGO_128),
            egui::TextureOptions::LINEAR,
        );
        Self {
            page: if uninstall_mode { Page::ConfirmUninstall } else { Page::Welcome },
            path_text: install_dir.display().to_string(),
            opts: Options { install_dir, start_menu: true, desktop: true, context_menu: true, associate: true, add_to_path: false },
            existing,
            launch_after: true,
            logo,
            log: Vec::new(),
        }
    }

    fn header(&self, ui: &mut egui::Ui, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(64.0, 64.0)));
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("Wedit").size(30.0).strong().color(GOLD));
                ui.label(RichText::new(subtitle).color(Color32::from_gray(170)));
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let sub = format!("Version {VERSION} — a fast code editor for Rust and C#");
        self.header(ui, &sub);
        if let Some((dir, ver)) = &self.existing {
            ui.label(RichText::new(format!("Wedit {ver} is already installed in {}. It will be updated.", dir.display())).color(GOLD));
            ui.add_space(4.0);
        }
        ui.label("Install location:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.path_text).desired_width(440.0));
            if ui.button("Browse…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_directory(&self.path_text).pick_folder() {
                    let p = if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(APP_NAME)) { p } else { p.join(APP_NAME) };
                    self.path_text = p.display().to_string();
                }
            }
        });
        ui.add_space(8.0);
        ui.checkbox(&mut self.opts.start_menu, "Create a Start Menu shortcut");
        ui.checkbox(&mut self.opts.desktop, "Create a Desktop shortcut");
        ui.checkbox(&mut self.opts.context_menu, "Add \"Edit with Wedit\" to the Explorer right-click menu");
        ui.checkbox(&mut self.opts.associate, "Register Wedit as an editor for .rs and .cs files");
        ui.checkbox(&mut self.opts.add_to_path, "Add Wedit to PATH (run `wedit` from a terminal)");
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Space required: {:.1} MB  ·  No administrator rights needed", PAYLOAD.len() as f64 * 2.0 / 1_048_576.0)).small().weak());

        ui.with_layout(egui::Layout::bottom_up(Align::Max), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let label = if self.existing.is_some() { "Update" } else { "Install" };
                    if ui.add(egui::Button::new(RichText::new(label).strong().color(Color32::BLACK)).fill(GOLD)).clicked() {
                        self.opts.install_dir = PathBuf::from(self.path_text.trim());
                        if self.opts.install_dir.as_os_str().is_empty() || !self.opts.install_dir.is_absolute() {
                            self.page = Page::Failed("Please choose a full install path, e.g. C:\\Tools\\Wedit".into());
                        } else {
                            self.page = Page::Installing { step: 0 };
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }

    fn installing(&mut self, ui: &mut egui::Ui, step: usize) {
        self.header(ui, "Installing…");
        let steps = install_steps();
        // Run one step per frame so the progress bar visibly advances.
        if step < steps.len() {
            let (name, f) = steps[step];
            match f(&self.opts) {
                Ok(()) => {
                    self.log.push(format!("✔ {name}"));
                    self.page = Page::Installing { step: step + 1 };
                }
                Err(e) => {
                    self.page = Page::Failed(format!("{name} failed:\n{e}"));
                    return;
                }
            }
            ui.ctx().request_repaint();
        } else {
            self.page = Page::Done;
        }
        ui.add(egui::ProgressBar::new((step + 1) as f32 / steps.len() as f32).fill(GOLD).show_percentage());
        ui.add_space(8.0);
        for l in &self.log {
            ui.label(l);
        }
    }

    fn done(&mut self, ui: &mut egui::Ui) {
        self.header(ui, "Installation complete");
        ui.label(RichText::new("Wedit has been installed successfully.").size(16.0));
        ui.add_space(6.0);
        ui.label(format!("Location: {}", self.opts.install_dir.display()));
        if self.opts.start_menu {
            ui.label("You can start Wedit from the Start Menu.");
        }
        ui.add_space(10.0);
        ui.checkbox(&mut self.launch_after, "Launch Wedit now");
        ui.with_layout(egui::Layout::bottom_up(Align::Max), |ui| {
            if ui.add(egui::Button::new(RichText::new("Finish").strong().color(Color32::BLACK)).fill(GOLD)).clicked() {
                if self.launch_after {
                    launch(&self.opts.install_dir);
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn failed(&mut self, ui: &mut egui::Ui, msg: &str) {
        self.header(ui, "Something went wrong");
        ui.label(RichText::new(msg).color(Color32::from_rgb(240, 110, 100)));
        ui.with_layout(egui::Layout::bottom_up(Align::Max), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Back").clicked() {
                    self.log.clear();
                    self.page = Page::Welcome;
                }
            });
        });
    }

    fn confirm_uninstall(&mut self, ui: &mut egui::Ui) {
        self.header(ui, "Uninstall");
        let dir = uninstall_dir();
        ui.label("This will remove Wedit, its shortcuts and Explorer integration from:");
        ui.label(RichText::new(dir.display().to_string()).monospace().color(GOLD));
        ui.add_space(6.0);
        ui.label(RichText::new("Your own files and settings are not touched.").weak());
        ui.with_layout(egui::Layout::bottom_up(Align::Max), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Uninstall").strong().color(Color32::BLACK)).fill(GOLD)).clicked() {
                        match uninstall(&dir) {
                            Ok(()) => {
                                self.opts.install_dir = dir.clone();
                                self.page = Page::Uninstalled;
                            }
                            Err(e) => self.page = Page::Failed(e.to_string()),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }

    fn uninstalled(&mut self, ui: &mut egui::Ui) {
        self.header(ui, "Uninstall complete");
        ui.label("Wedit has been removed from your computer.");
        ui.with_layout(egui::Layout::bottom_up(Align::Max), |ui| {
            if ui.button("Close").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_rgb(30, 30, 34)).inner_margin(egui::Margin::same(20)))
            .show(ctx, |ui| match std::mem::replace(&mut self.page, Page::Done) {
                Page::Welcome => {
                    self.page = Page::Welcome;
                    self.welcome(ui);
                }
                Page::Installing { step } => {
                    self.page = Page::Installing { step };
                    self.installing(ui, step);
                }
                Page::Done => self.done(ui),
                Page::Failed(msg) => {
                    self.page = Page::Failed(msg.clone());
                    self.failed(ui, &msg);
                }
                Page::ConfirmUninstall => {
                    self.page = Page::ConfirmUninstall;
                    self.confirm_uninstall(ui);
                }
                Page::Uninstalled => {
                    self.page = Page::Uninstalled;
                    self.uninstalled(ui);
                }
            });
        // Closing the window after uninstalling still removes the folder.
        if matches!(self.page, Page::Uninstalled) && ctx.input(|i| i.viewport().close_requested()) {
            schedule_folder_removal(&self.opts.install_dir);
        }
    }
}
