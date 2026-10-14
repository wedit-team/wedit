use crate::document::{byte_to_char, char_to_byte, line_start_char, Document, LineEnding};
use crate::editing::{self, Sel};
use crate::explorer::{self, Explorer};
use crate::syntax::{self, Lang};
use crate::theme::{Palette, ThemeKind, GOLD};
use egui::text::{CCursor, CCursorRange, LayoutJob, LayoutSection};
use egui::{Align, Align2, FontFamily, FontId, Key, KeyboardShortcut, Modifiers, RichText, TextFormat};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SETTINGS_KEY: &str = "wedit_settings";
const MAX_RECENT: usize = 12;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    theme: ThemeKind,
    font_size: f32,
    show_sidebar: bool,
    word_wrap: bool,
    last_folder: Option<PathBuf>,
    open_files: Vec<PathBuf>,
    recent_files: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Dark,
            font_size: 15.0,
            show_sidebar: true,
            word_wrap: false,
            last_folder: None,
            open_files: Vec::new(),
            recent_files: Vec::new(),
        }
    }
}

#[derive(Default)]
struct FindState {
    open: bool,
    replace: bool,
    query: String,
    replacement: String,
    match_case: bool,
    focus: bool,
}

struct QuickOpen {
    query: String,
    files: Vec<PathBuf>,
    selected: usize,
    focus: bool,
}

/// Cached syntax-highlighted layout for one document.
#[derive(Default)]
struct HighlightCache {
    key: u64,
    job: LayoutJob,
}

pub struct WeditApp {
    docs: Vec<Document>,
    active: usize,
    next_id: u64,
    untitled_counter: usize,
    explorer: Explorer,
    settings: Settings,
    find: FindState,
    goto_line: Option<String>,
    quick_open: Option<QuickOpen>,
    highlight: std::collections::HashMap<u64, HighlightCache>,
    /// (doc id, selection) to apply to the editor on the next frame.
    pending_select: Option<(u64, Sel)>,
    pending_scroll: bool,
    status: Option<(String, f64)>,
    last_title: String,
    allow_close: bool,
    show_about: bool,
    logo: Option<egui::TextureHandle>,
}

impl WeditApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Vec<PathBuf>) -> Self {
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, SETTINGS_KEY)).unwrap_or_default();
        settings.theme.apply(&cc.egui_ctx);

        let mut app = Self {
            docs: Vec::new(),
            active: 0,
            next_id: 1,
            untitled_counter: 0,
            explorer: Explorer::new(),
            settings,
            find: FindState::default(),
            goto_line: None,
            quick_open: None,
            highlight: Default::default(),
            pending_select: None,
            pending_scroll: false,
            status: None,
            last_title: String::new(),
            allow_close: false,
            show_about: false,
            logo: None,
        };

        let logo = egui::ColorImage::from_rgba_unmultiplied([128, 128], crate::LOGO_128);
        app.logo = Some(cc.egui_ctx.load_texture("wedit-logo", logo, egui::TextureOptions::LINEAR));

        if args.is_empty() {
            // Restore the previous session.
            if let Some(folder) = app.settings.last_folder.clone().filter(|f| f.is_dir()) {
                app.explorer.set_root(Some(folder));
            }
            for f in std::mem::take(&mut app.settings.open_files) {
                if f.is_file() {
                    app.open_path(&f);
                }
            }
        } else {
            for a in &args {
                let p = std::fs::canonicalize(a).unwrap_or_else(|_| a.clone());
                if p.is_dir() {
                    app.open_folder(p);
                } else {
                    app.open_path(&p);
                }
            }
        }
        app
    }

    // ------------------------------------------------------------------ documents

    fn active_doc(&self) -> Option<&Document> {
        self.docs.get(self.active)
    }

    fn new_file(&mut self) {
        self.untitled_counter += 1;
        let doc = Document::new_untitled(self.next_id, self.untitled_counter);
        self.next_id += 1;
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.focus_editor_soon();
    }

    fn open_path(&mut self, path: &Path) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path)) {
            self.active = i;
            return;
        }
        match Document::open(self.next_id, path) {
            Ok(doc) => {
                self.next_id += 1;
                if doc.lossy {
                    self.set_status(format!("{} is not valid UTF-8; invalid bytes were replaced", doc.title()));
                }
                // Replace a lone, empty, untouched untitled tab.
                if self.docs.len() == 1 && self.docs[0].path.is_none() && !self.docs[0].is_dirty() && self.docs[0].text.is_empty() {
                    self.docs.clear();
                }
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
                self.push_recent(path.to_path_buf());
                self.focus_editor_soon();
            }
            Err(e) => {
                rfd::MessageDialog::new()
                    .set_title("Wedit")
                    .set_level(rfd::MessageLevel::Error)
                    .set_description(format!("Could not open {}:\n{e}", path.display()))
                    .show();
            }
        }
    }

    fn open_file_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Source files", &["rs", "cs", "csx"])
            .add_filter("Rust", &["rs"])
            .add_filter("C#", &["cs", "csx"])
            .add_filter("All files", &["*"]);
        if let Some(dir) = self.explorer.root.as_ref() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(files) = dlg.pick_files() {
            for f in files {
                self.open_path(&f);
            }
        }
    }

    fn open_folder(&mut self, folder: PathBuf) {
        self.settings.last_folder = Some(folder.clone());
        self.settings.show_sidebar = true;
        self.explorer.set_root(Some(folder));
    }

    fn open_folder_dialog(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(folder);
        }
    }

    fn push_recent(&mut self, p: PathBuf) {
        self.settings.recent_files.retain(|r| r != &p);
        self.settings.recent_files.insert(0, p);
        self.settings.recent_files.truncate(MAX_RECENT);
    }

    /// Saves document `idx`. Returns false if the user cancelled or saving failed.
    fn save(&mut self, idx: usize, save_as: bool) -> bool {
        let Some(doc) = self.docs.get(idx) else { return false };
        let path = match (&doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let default_name = match &doc.path {
                    Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    None => match doc.lang {
                        Lang::Rust => format!("{}.rs", doc.title()),
                        Lang::CSharp => format!("{}.cs", doc.title()),
                        Lang::Plain => format!("{}.txt", doc.title()),
                    },
                };
                let mut dlg = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("Rust", &["rs"])
                    .add_filter("C#", &["cs"])
                    .add_filter("All files", &["*"]);
                if let Some(dir) = doc.path.as_ref().and_then(|p| p.parent()).or(self.explorer.root.as_deref()) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        let doc = &mut self.docs[idx];
        match doc.save_to(&path) {
            Ok(()) => {
                let msg = format!("Saved {}", doc.title());
                self.push_recent(path);
                self.explorer.refresh();
                self.set_status(msg);
                true
            }
            Err(e) => {
                rfd::MessageDialog::new()
                    .set_title("Wedit")
                    .set_level(rfd::MessageLevel::Error)
                    .set_description(format!("Could not save {}:\n{e}", path.display()))
                    .show();
                false
            }
        }
    }

    fn save_all(&mut self) -> bool {
        for i in 0..self.docs.len() {
            if self.docs[i].is_dirty() && !self.save(i, false) {
                return false;
            }
        }
        true
    }

    fn close_tab(&mut self, idx: usize) {
        if idx >= self.docs.len() {
            return;
        }
        if self.docs[idx].is_dirty() {
            let res = rfd::MessageDialog::new()
                .set_title("Wedit")
                .set_level(rfd::MessageLevel::Warning)
                .set_description(format!("Do you want to save the changes you made to {}?", self.docs[idx].title()))
                .set_buttons(rfd::MessageButtons::YesNoCancel)
                .show();
            match res {
                rfd::MessageDialogResult::Yes => {
                    if !self.save(idx, false) {
                        return;
                    }
                }
                rfd::MessageDialogResult::No => {}
                _ => return,
            }
        }
        let doc = self.docs.remove(idx);
        self.highlight.remove(&doc.id);
        if self.active >= idx && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.docs.len().saturating_sub(1));
    }

    fn set_status(&mut self, msg: String) {
        self.status = Some((msg, f64::NAN));
    }

    fn focus_editor_soon(&mut self) {
        if let Some(doc) = self.docs.get(self.active) {
            self.pending_select = Some((doc.id, (doc.cursor, doc.cursor)));
        }
    }

    fn select_in_active(&mut self, sel: Sel) {
        if let Some(doc) = self.docs.get(self.active) {
            self.pending_select = Some((doc.id, sel));
            self.pending_scroll = true;
        }
    }

    // ------------------------------------------------------------------ find

    fn find_matches(&self, text: &str) -> Vec<Range<usize>> {
        find_all(text, &self.find.query, self.find.match_case)
    }

    fn find_step(&mut self, forward: bool) {
        let Some(doc) = self.active_doc() else { return };
        let matches = self.find_matches(&doc.text);
        if matches.is_empty() {
            self.set_status(format!("No results for \"{}\"", self.find.query));
            return;
        }
        let sel_start = char_to_byte(&doc.text, doc.cursor - doc.selection_len.min(doc.cursor));
        let sel_end = char_to_byte(&doc.text, doc.cursor);
        let m = if forward {
            matches.iter().find(|m| m.start >= sel_end).or(matches.first())
        } else {
            matches.iter().rev().find(|m| m.start < sel_start).or(matches.last())
        }
        .cloned()
        .unwrap();
        let sel = (byte_to_char(&doc.text, m.start), byte_to_char(&doc.text, m.end));
        let n = matches.iter().position(|x| *x == m).unwrap_or(0) + 1;
        self.set_status(format!("Match {n} of {}", matches.len()));
        self.select_in_active(sel);
    }

    fn replace_one(&mut self) {
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let case = self.find.match_case;
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let start = doc.cursor - doc.selection_len.min(doc.cursor);
        let (a, b) = (char_to_byte(&doc.text, start), char_to_byte(&doc.text, doc.cursor));
        let selected_is_match = find_all(&doc.text[a..b], &query, case).first().is_some_and(|m| *m == (0..b - a));
        if selected_is_match {
            doc.text.replace_range(a..b, &replacement);
            let caret = start + replacement.chars().count();
            doc.cursor = caret;
            doc.selection_len = 0;
        }
        self.find_step(true);
    }

    fn replace_all(&mut self) {
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let case = self.find.match_case;
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let matches = find_all(&doc.text, &query, case);
        let n = matches.len();
        for m in matches.into_iter().rev() {
            doc.text.replace_range(m, &replacement);
        }
        let cur = doc.cursor.min(doc.text.chars().count());
        self.set_status(format!("Replaced {n} occurrence{}", if n == 1 { "" } else { "s" }));
        self.select_in_active((cur, cur));
    }

    // ------------------------------------------------------------------ shortcuts

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let sc = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        // Specific (shift) combos first: consume_shortcut ignores extra shift.
        if sc(cmd_shift, Key::S) {
            self.save(self.active, true);
        }
        if sc(cmd_shift, Key::O) {
            self.open_folder_dialog();
        }
        if sc(cmd_shift, Key::Tab) && !self.docs.is_empty() {
            self.active = (self.active + self.docs.len() - 1) % self.docs.len();
        }
        if sc(Modifiers::COMMAND, Key::Tab) && !self.docs.is_empty() {
            self.active = (self.active + 1) % self.docs.len();
        }
        if sc(Modifiers::COMMAND, Key::N) {
            self.new_file();
        }
        if sc(Modifiers::COMMAND, Key::O) {
            self.open_file_dialog();
        }
        if sc(Modifiers::COMMAND, Key::S) {
            self.save(self.active, false);
        }
        if sc(Modifiers::COMMAND, Key::W) {
            self.close_tab(self.active);
        }
        if sc(Modifiers::COMMAND, Key::H) {
            self.find.open = true;
            self.find.replace = true;
            self.find.focus = true;
            self.prefill_find();
        }
        if sc(Modifiers::COMMAND, Key::F) {
            self.find.open = true;
            self.find.focus = true;
            self.prefill_find();
        }
        if sc(Modifiers::COMMAND, Key::G) && !self.docs.is_empty() {
            self.goto_line = Some(String::new());
        }
        if sc(Modifiers::COMMAND, Key::P) {
            self.open_quick_open();
        }
        if sc(Modifiers::COMMAND, Key::B) {
            self.settings.show_sidebar = !self.settings.show_sidebar;
        }
        if sc(Modifiers::ALT, Key::Z) {
            self.settings.word_wrap = !self.settings.word_wrap;
        }
        if sc(Modifiers::COMMAND, Key::Plus) || sc(Modifiers::COMMAND, Key::Equals) {
            self.zoom(1.0);
        }
        if sc(Modifiers::COMMAND, Key::Minus) {
            self.zoom(-1.0);
        }
        if sc(Modifiers::COMMAND, Key::Num0) {
            self.settings.font_size = 15.0;
        }
        if sc(Modifiers::NONE, Key::Escape) {
            if self.quick_open.is_some() {
                self.quick_open = None;
            } else if self.goto_line.is_some() {
                self.goto_line = None;
            } else if self.find.open {
                self.find.open = false;
                self.focus_editor_soon();
            }
        }
        if sc(Modifiers::SHIFT, Key::F3) {
            self.find_step(false);
        }
        if sc(Modifiers::NONE, Key::F3) {
            self.find_step(true);
        }
    }

    fn zoom(&mut self, delta: f32) {
        self.settings.font_size = (self.settings.font_size + delta).clamp(8.0, 40.0);
    }

    fn prefill_find(&mut self) {
        if let Some(doc) = self.active_doc() {
            if doc.selection_len > 0 {
                let a = char_to_byte(&doc.text, doc.cursor - doc.selection_len);
                let b = char_to_byte(&doc.text, doc.cursor);
                let s = &doc.text[a..b];
                if !s.contains('\n') {
                    self.find.query = s.to_string();
                }
            }
        }
    }

    fn open_quick_open(&mut self) {
        let files = match &self.explorer.root {
            Some(root) => explorer::index_files(root, 20_000),
            None => self.settings.recent_files.clone(),
        };
        self.quick_open = Some(QuickOpen { query: String::new(), files, selected: 0, focus: true });
    }

    /// Smart editing keys and typed text, handled (in order) before the TextEdit sees the input.
    fn handle_editor_keys(&mut self, ctx: &egui::Context, editor_id: egui::Id) {
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let Some(mut state) = egui::TextEdit::load_state(ctx, editor_id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        let (p, s) = (range.primary.index, range.secondary.index);
        let mut sel: Sel = (p.min(s), p.max(s));
        let comment = doc.lang.line_comment().unwrap_or("//");

        let events = ctx.input(|i| i.events.clone());
        let mut keep = Vec::with_capacity(events.len());
        let mut changed = false;
        for event in events {
            let handled = match &event {
                egui::Event::Key { key, pressed: true, modifiers: m, .. } => {
                    let none = !m.command && !m.ctrl && !m.alt && !m.shift;
                    let shift_only = m.shift && !m.command && !m.ctrl && !m.alt;
                    let cmd = m.command && !m.alt;
                    let alt_only = m.alt && !m.command && !m.ctrl && !m.shift;
                    let result = match key {
                        Key::Tab if shift_only => Some(editing::outdent(&mut doc.text, sel)),
                        Key::Tab if none => Some(editing::tab(&mut doc.text, sel)),
                        Key::Enter if none => Some(editing::newline(&mut doc.text, sel)),
                        Key::Slash if cmd && !m.shift => Some(editing::toggle_comment(&mut doc.text, sel, comment)),
                        Key::D if cmd && m.shift => Some(editing::duplicate_lines(&mut doc.text, sel)),
                        Key::ArrowUp if alt_only => Some(editing::move_lines(&mut doc.text, sel, true)),
                        Key::ArrowDown if alt_only => Some(editing::move_lines(&mut doc.text, sel, false)),
                        _ => None,
                    };
                    result.map(|ns| sel = ns).is_some()
                }
                egui::Event::Text(t) => {
                    for c in t.chars() {
                        sel = editing::type_char(&mut doc.text, sel, c)
                            .unwrap_or_else(|| editing::replace_selection(&mut doc.text, sel, c.encode_utf8(&mut [0; 4])));
                    }
                    true
                }
                _ => false,
            };
            if handled {
                changed = true;
            } else {
                keep.push(event);
            }
        }
        if changed {
            ctx.input_mut(|i| i.events = keep);
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
            state.store(ctx, editor_id);
            self.pending_scroll = true;
        }
    }

    // ------------------------------------------------------------------ UI

    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.add(egui::Image::new(self.logo.as_ref().unwrap()).fit_to_exact_size(egui::vec2(16.0, 16.0)));
                ui.menu_button("File", |ui| {
                    if item(ui, "New File", "Ctrl+N") {
                        self.new_file();
                    }
                    if item(ui, "Open File…", "Ctrl+O") {
                        self.open_file_dialog();
                    }
                    if item(ui, "Open Folder…", "Ctrl+Shift+O") {
                        self.open_folder_dialog();
                    }
                    ui.menu_button("Open Recent", |ui| {
                        if self.settings.recent_files.is_empty() {
                            ui.label(RichText::new("No recent files").weak());
                        }
                        for p in self.settings.recent_files.clone() {
                            if ui.button(p.display().to_string()).clicked() {
                                self.open_path(&p);
                                ui.close_menu();
                            }
                        }
                        if !self.settings.recent_files.is_empty() {
                            ui.separator();
                            if ui.button("Clear Recent").clicked() {
                                self.settings.recent_files.clear();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    if item(ui, "Save", "Ctrl+S") {
                        self.save(self.active, false);
                    }
                    if item(ui, "Save As…", "Ctrl+Shift+S") {
                        self.save(self.active, true);
                    }
                    if item(ui, "Save All", "") {
                        self.save_all();
                    }
                    ui.separator();
                    if item(ui, "Close Tab", "Ctrl+W") {
                        self.close_tab(self.active);
                    }
                    if item(ui, "Close Folder", "") {
                        self.explorer.set_root(None);
                        self.settings.last_folder = None;
                    }
                    ui.separator();
                    if item(ui, "Exit", "Alt+F4") {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if item(ui, "Find", "Ctrl+F") {
                        self.find.open = true;
                        self.find.focus = true;
                        self.prefill_find();
                    }
                    if item(ui, "Replace", "Ctrl+H") {
                        self.find.open = true;
                        self.find.replace = true;
                        self.find.focus = true;
                        self.prefill_find();
                    }
                    if item(ui, "Find Next", "F3") {
                        self.find_step(true);
                    }
                    if item(ui, "Find Previous", "Shift+F3") {
                        self.find_step(false);
                    }
                    ui.separator();
                    ui.label(RichText::new("In the editor:").weak());
                    ui.label(RichText::new("  Toggle Comment    Ctrl+/").weak());
                    ui.label(RichText::new("  Duplicate Line    Ctrl+Shift+D").weak());
                    ui.label(RichText::new("  Move Line         Alt+Up / Alt+Down").weak());
                    ui.label(RichText::new("  Indent / Outdent  Tab / Shift+Tab").weak());
                    ui.label(RichText::new("  Undo / Redo       Ctrl+Z / Ctrl+Y").weak());
                });
                ui.menu_button("Go", |ui| {
                    if item(ui, "Go to File…", "Ctrl+P") {
                        self.open_quick_open();
                    }
                    if item(ui, "Go to Line…", "Ctrl+G") && !self.docs.is_empty() {
                        self.goto_line = Some(String::new());
                    }
                    ui.separator();
                    if item(ui, "Next Tab", "Ctrl+Tab") && !self.docs.is_empty() {
                        self.active = (self.active + 1) % self.docs.len();
                    }
                    if item(ui, "Previous Tab", "Ctrl+Shift+Tab") && !self.docs.is_empty() {
                        self.active = (self.active + self.docs.len() - 1) % self.docs.len();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.checkbox(&mut self.settings.show_sidebar, "Explorer (Ctrl+B)").changed() {
                        ui.close_menu();
                    }
                    if ui.checkbox(&mut self.settings.word_wrap, "Word Wrap (Alt+Z)").changed() {
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label("Theme");
                    let before = self.settings.theme;
                    ui.radio_value(&mut self.settings.theme, ThemeKind::Dark, "Wedit Dark");
                    ui.radio_value(&mut self.settings.theme, ThemeKind::Light, "Wedit Light");
                    if before != self.settings.theme {
                        self.settings.theme.apply(ctx);
                        self.highlight.clear();
                    }
                    ui.separator();
                    if item(ui, "Zoom In", "Ctrl+=") {
                        self.zoom(1.0);
                    }
                    if item(ui, "Zoom Out", "Ctrl+-") {
                        self.zoom(-1.0);
                    }
                    if item(ui, "Reset Zoom", "Ctrl+0") {
                        self.settings.font_size = 15.0;
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About Wedit").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    fn tab_bar(&mut self, ctx: &egui::Context, pal: &Palette) {
        if self.docs.is_empty() {
            return;
        }
        let mut close = None;
        egui::TopBottomPanel::top("tabs")
            .frame(egui::Frame::default().fill(pal.tab_inactive))
            .show(ctx, |ui| {
                egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (i, doc) in self.docs.iter().enumerate() {
                            let active = i == self.active;
                            let fill = if active { pal.tab_active } else { pal.tab_inactive };
                            let resp = egui::Frame::default()
                                .fill(fill)
                                .inner_margin(egui::Margin::symmetric(12, 7))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let (badge, color) = explorer::file_badge(&doc.title());
                                    ui.label(RichText::new(badge).monospace().size(10.0).color(color));
                                    let title_color = if active { pal.text } else { pal.dim };
                                    let title = ui.add(egui::Label::new(RichText::new(doc.title()).color(title_color)).sense(egui::Sense::click()));
                                    let close_txt = if doc.is_dirty() { "●" } else { "×" };
                                    let x = ui.add(egui::Label::new(RichText::new(close_txt).color(pal.dim)).sense(egui::Sense::click()));
                                    if x.clicked() {
                                        close = Some(i);
                                    }
                                    let x = x.on_hover_text("Close (Ctrl+W)");
                                    if x.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    title
                                });
                            let full = resp.response.rect;
                            if active {
                                ui.painter().line_segment([full.left_top(), full.right_top()], egui::Stroke::new(2.0_f32, GOLD));
                            }
                            let title = resp.inner;
                            if title.clicked() || ui.interact(full, ui.id().with(("tab", doc.id)), egui::Sense::click()).clicked() {
                                self.active = i;
                            }
                            if title.middle_clicked() {
                                close = Some(i);
                            }
                            if let Some(p) = &doc.path {
                                title.on_hover_text(p.display().to_string());
                            }
                        }
                    });
                });
            });
        if let Some(i) = close {
            self.close_tab(i);
        }
    }

    fn find_bar(&mut self, ctx: &egui::Context) {
        if !self.find.open || self.docs.is_empty() {
            return;
        }
        let count = self.active_doc().map(|d| self.find_matches(&d.text).len()).unwrap_or(0);
        let mut action: Option<&str> = None;
        egui::TopBottomPanel::top("find").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                let toggle = if self.find.replace { "⏷" } else { "⏵" };
                if ui.small_button(toggle).on_hover_text("Toggle Replace").clicked() {
                    self.find.replace = !self.find.replace;
                }
                let r = ui.add(egui::TextEdit::singleline(&mut self.find.query).hint_text("Find").desired_width(260.0));
                if self.find.focus {
                    r.request_focus();
                    self.find.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    action = Some(if ui.input(|i| i.modifiers.shift) { "prev" } else { "next" });
                    r.request_focus();
                }
                ui.toggle_value(&mut self.find.match_case, "Aa").on_hover_text("Match Case");
                let label = if self.find.query.is_empty() {
                    String::new()
                } else if count == 0 {
                    "No results".into()
                } else {
                    format!("{count} found")
                };
                ui.label(RichText::new(label).weak());
                if ui.button("↑").on_hover_text("Previous (Shift+F3)").clicked() {
                    action = Some("prev");
                }
                if ui.button("↓").on_hover_text("Next (F3)").clicked() {
                    action = Some("next");
                }
                if ui.button("✕").on_hover_text("Close (Esc)").clicked() {
                    action = Some("close");
                }
            });
            if self.find.replace {
                ui.horizontal(|ui| {
                    ui.add_space(ui.spacing().interact_size.y + 4.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut self.find.replacement).hint_text("Replace").desired_width(260.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        action = Some("replace");
                        r.request_focus();
                    }
                    if ui.button("Replace").clicked() {
                        action = Some("replace");
                    }
                    if ui.button("Replace All").clicked() {
                        action = Some("replace_all");
                    }
                });
            }
            ui.add_space(3.0);
        });
        match action {
            Some("next") => self.find_step(true),
            Some("prev") => self.find_step(false),
            Some("replace") => self.replace_one(),
            Some("replace_all") => self.replace_all(),
            Some("close") => {
                self.find.open = false;
                self.focus_editor_soon();
            }
            _ => {}
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context, pal: &Palette) {
        if !self.settings.show_sidebar {
            return;
        }
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(250.0)
            .width_range(150.0..=600.0)
            .frame(egui::Frame::default().fill(pal.sidebar).inner_margin(egui::Margin::symmetric(8, 6)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("EXPLORER").small().strong().color(pal.dim));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if self.explorer.root.is_some() && ui.small_button("⟳").on_hover_text("Refresh").clicked() {
                            self.explorer.refresh();
                        }
                    });
                });
                if let Some(root) = &self.explorer.root {
                    let name = root.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
                    ui.label(RichText::new(name).strong().color(GOLD)).on_hover_text(root.display().to_string());
                }
                ui.separator();
                let active_path = self.docs.get(self.active).and_then(|d| d.path.clone());
                let mut clicked = None;
                egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    clicked = self.explorer.ui(ui, pal, active_path.as_deref());
                    if self.explorer.root.is_none() && ui.button("Open Folder…").clicked() {
                        self.open_folder_dialog();
                    }
                });
                if let Some(p) = clicked {
                    self.open_path(&p);
                }
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context, pal: &Palette) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(egui::Frame::default().fill(pal.status_bg).inner_margin(egui::Margin::symmetric(10, 2)))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(pal.status_fg);
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("W").strong().color(GOLD));
                    if let Some(root) = &self.explorer.root {
                        ui.label(root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    }
                    let now = ctx.input(|i| i.time);
                    if let Some((msg, t)) = &mut self.status {
                        if t.is_nan() {
                            *t = now;
                        }
                        if now - *t < 4.0 {
                            ui.label(format!("  {msg}"));
                            ctx.request_repaint_after(std::time::Duration::from_millis(500));
                        }
                    }

                    let Some(doc) = self.docs.get_mut(self.active) else { return };
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button(doc.lang.name(), |ui| {
                            for l in [Lang::Rust, Lang::CSharp, Lang::Plain] {
                                if ui.selectable_label(doc.lang == l, l.name()).clicked() {
                                    doc.lang = l;
                                    ui.close_menu();
                                }
                            }
                        });
                        let le = doc.line_ending;
                        if ui.add(egui::Label::new(le.label()).sense(egui::Sense::click())).on_hover_text("Click to toggle line endings").clicked() {
                            doc.line_ending = if le == LineEnding::Lf { LineEnding::CrLf } else { LineEnding::Lf };
                        }
                        ui.label("UTF-8");
                        ui.label("Spaces: 4");
                        let (l, c) = doc.line_col();
                        let sel = if doc.selection_len > 0 { format!(" ({} selected)", doc.selection_len) } else { String::new() };
                        ui.label(format!("Ln {l}, Col {c}{sel}"));
                    });
                });
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.15).max(20.0));
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(112.0, 112.0)));
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Wedit").size(40.0).strong().color(GOLD));
            ui.label(RichText::new("A fast code editor for Rust and C#").size(15.0).color(pal.dim));
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                let w = 3.0 * 130.0 + 2.0 * ui.spacing().item_spacing.x;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                if ui.add_sized([130.0, 32.0], egui::Button::new("New File")).clicked() {
                    self.new_file();
                }
                if ui.add_sized([130.0, 32.0], egui::Button::new("Open File…")).clicked() {
                    self.open_file_dialog();
                }
                if ui.add_sized([130.0, 32.0], egui::Button::new("Open Folder…")).clicked() {
                    self.open_folder_dialog();
                }
            });
            ui.add_space(24.0);
            let shortcuts = [
                ("Go to File", "Ctrl+P"),
                ("Find / Replace", "Ctrl+F / Ctrl+H"),
                ("Go to Line", "Ctrl+G"),
                ("Toggle Explorer", "Ctrl+B"),
                ("Toggle Comment", "Ctrl+/"),
            ];
            egui::Grid::new("shortcuts").spacing([24.0, 6.0]).show(ui, |ui| {
                for (a, b) in shortcuts {
                    ui.label(RichText::new(a).color(pal.dim));
                    ui.label(RichText::new(b).monospace().color(pal.text));
                    ui.end_row();
                }
            });
            if !self.settings.recent_files.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new("Recent").strong().color(pal.dim));
                for p in self.settings.recent_files.clone().into_iter().take(6) {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if ui.link(name).on_hover_text(p.display().to_string()).clicked() {
                        self.open_path(&p);
                    }
                }
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let theme = self.settings.theme;
        let font_id = FontId::new(self.settings.font_size, FontFamily::Monospace);
        let doc_id = self.docs[self.active].id;
        let editor_id = egui::Id::new(("wedit-editor", doc_id));

        if ui.ctx().memory(|m| m.has_focus(editor_id)) {
            self.handle_editor_keys(ui.ctx(), editor_id);
        }

        // Ctrl + mouse wheel zooms the editor font.
        if ui.rect_contains_pointer(ui.max_rect()) {
            let z = ui.input(|i| i.zoom_delta());
            if z != 1.0 {
                self.settings.font_size = (self.settings.font_size * z).clamp(8.0, 40.0);
            }
        }

        if let Some((id, (a, b))) = self.pending_select {
            if id == doc_id {
                let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
                state.store(ui.ctx(), editor_id);
                ui.ctx().memory_mut(|m| m.request_focus(editor_id));
                self.pending_select = None;
            }
        }

        let find_query = if self.find.open { self.find.query.clone() } else { String::new() };
        let match_case = self.find.match_case;
        let word_wrap = self.settings.word_wrap;
        let scroll_to_cursor = std::mem::take(&mut self.pending_scroll);

        let doc = &mut self.docs[self.active];
        let cache = self.highlight.entry(doc_id).or_default();
        let lang = doc.lang;
        let font_size = self.settings.font_size;

        let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
            let mut h = DefaultHasher::new();
            text.hash(&mut h);
            lang.hash(&mut h);
            theme.hash(&mut h);
            font_size.to_bits().hash(&mut h);
            let key = h.finish();
            if cache.key != key || cache.job.text.len() != text.len() {
                cache.job = highlight_job(text, lang, theme, FontId::new(font_size, FontFamily::Monospace));
                cache.key = key;
            }
            let mut job = cache.job.clone();
            job.wrap.max_width = if word_wrap { wrap_width } else { f32::INFINITY };
            ui.fonts(|f| f.layout_job(job))
        };

        let row_height = ui.fonts(|f| f.row_height(&font_id));
        let digits = doc.line_count().to_string().len().max(3);
        let char_w = ui.fonts(|f| f.glyph_width(&font_id, '0'));
        let gutter_w = digits as f32 * char_w + 28.0;

        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0.0, pal.bg);
        let gutter_rect = egui::Rect::from_min_size(full.min, egui::vec2(gutter_w, full.height()));
        let text_rect = egui::Rect::from_min_max(egui::pos2(full.min.x + gutter_w, full.min.y), full.max);

        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(egui::Layout::top_down(Align::Min)));
        let mut out = None;
        egui::ScrollArea::both()
            .id_salt(("editor-scroll", doc_id))
            .auto_shrink(false)
            .show(&mut text_ui, |ui| {
                let desired_rows = ((ui.available_height() / row_height) as usize).max(1);
                let output = egui::TextEdit::multiline(&mut doc.text)
                    .id(editor_id)
                    .font(font_id.clone())
                    .code_editor()
                    .frame(false)
                    .margin(egui::Margin::symmetric(6, 0))
                    .desired_width(if word_wrap { ui.available_width() } else { f32::INFINITY })
                    .desired_rows(desired_rows)
                    .layouter(&mut layouter)
                    .show(ui);

                let galley_pos = output.galley_pos;
                let painter = ui.painter();
                let clip = ui.clip_rect();

                // Current line highlight.
                if let Some(range) = output.cursor_range {
                    let r = output.galley.pos_from_cursor(&range.primary).translate(galley_pos.to_vec2());
                    let line_rect = egui::Rect::from_min_max(egui::pos2(clip.left(), r.top()), egui::pos2(clip.right(), r.bottom()));
                    painter.rect_filled(line_rect, 0.0, pal.current_line);
                    if scroll_to_cursor {
                        ui.scroll_to_rect(r.expand(row_height * 2.0), Some(Align::Center));
                    }
                }

                // Find-match highlights (visible area only).
                if !find_query.is_empty() {
                    let text = output.galley.job.text.as_str();
                    let sel = output.cursor_range.map(|r| {
                        let (a, b) = (r.primary.ccursor.index, r.secondary.ccursor.index);
                        (a.min(b), a.max(b))
                    });
                    let mut char_pos = 0;
                    let mut byte_pos = 0;
                    for m in find_all(text, &find_query, match_case).into_iter().take(5000) {
                        char_pos += text[byte_pos..m.start].chars().count();
                        let start_c = char_pos;
                        let end_c = start_c + text[m.clone()].chars().count();
                        byte_pos = m.start;
                        let a = output.galley.pos_from_ccursor(CCursor::new(start_c)).translate(galley_pos.to_vec2());
                        if a.top() > clip.bottom() {
                            break;
                        }
                        if a.bottom() < clip.top() {
                            continue;
                        }
                        let b = output.galley.pos_from_ccursor(CCursor::new(end_c)).translate(galley_pos.to_vec2());
                        if (a.top() - b.top()).abs() > 1.0 {
                            continue;
                        }
                        let rect = egui::Rect::from_min_max(a.min, egui::pos2(b.max.x.max(a.max.x + 2.0), a.max.y));
                        let current = sel == Some((start_c, end_c));
                        let fill = if current { GOLD.gamma_multiply(0.55) } else { GOLD.gamma_multiply(0.22) };
                        painter.rect_filled(rect, 2.0, fill);
                        if current {
                            painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, GOLD), egui::StrokeKind::Outside);
                        }
                    }
                }
                out = Some(output);
            });

        let Some(output) = out else { return };

        if let Some(range) = output.cursor_range {
            let (a, b) = (range.primary.ccursor.index, range.secondary.ccursor.index);
            doc.cursor = a.max(b);
            doc.selection_len = a.max(b) - a.min(b);
        }

        // Line-number gutter, painted after layout so it tracks scrolling and wrapping.
        let painter = ui.painter_at(gutter_rect);
        painter.rect_filled(gutter_rect, 0.0, pal.bg);
        let active_line = doc.line_col().0;
        let mut line = 1;
        let mut new_line = true;
        for row in &output.galley.rows {
            let y = output.galley_pos.y + row.rect.min.y;
            if new_line && y + row_height >= gutter_rect.top() && y <= gutter_rect.bottom() {
                let color = if line == active_line { pal.gutter_active } else { pal.gutter_fg };
                painter.text(egui::pos2(gutter_rect.right() - 14.0, y), Align2::RIGHT_TOP, line.to_string(), font_id.clone(), color);
            }
            if y > gutter_rect.bottom() {
                break;
            }
            new_line = row.ends_with_newline;
            if row.ends_with_newline {
                line += 1;
            }
        }

        // Context menu.
        output.response.context_menu(|ui| {
            if ui.button("Find…").clicked() {
                self.find.open = true;
                self.find.focus = true;
                ui.close_menu();
            }
            if ui.button("Go to Line…").clicked() {
                self.goto_line = Some(String::new());
                ui.close_menu();
            }
            ui.separator();
            ui.label(RichText::new("Toggle Comment   Ctrl+/").weak());
            ui.label(RichText::new("Duplicate Line   Ctrl+Shift+D").weak());
        });
    }

    fn goto_dialog(&mut self, ctx: &egui::Context) {
        let Some(input) = &mut self.goto_line else { return };
        let total = self.docs.get(self.active).map(|d| d.line_count()).unwrap_or(1);
        let mut submit = None;
        let mut close = false;
        egui::Window::new("Go to Line")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 70.0))
            .show(ctx, |ui| {
                ui.label(format!("Go to line (1 - {total}), optionally line:column"));
                let r = ui.add(egui::TextEdit::singleline(input).desired_width(320.0).hint_text("e.g. 42 or 42:7"));
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = Some(input.clone());
                }
                if r.lost_focus() && !ui.input(|i| i.key_pressed(Key::Enter)) {
                    close = true;
                }
            });
        if let Some(s) = submit {
            self.goto_line = None;
            let mut parts = s.split(':');
            let line: usize = parts.next().and_then(|p| p.trim().parse().ok()).unwrap_or(1);
            let col: usize = parts.next().and_then(|p| p.trim().parse().ok()).unwrap_or(1);
            if let Some(doc) = self.docs.get(self.active) {
                let start = line_start_char(&doc.text, line.clamp(1, total));
                let line_len = doc.text.chars().skip(start).take_while(|&c| c != '\n').count();
                let pos = start + (col.max(1) - 1).min(line_len);
                self.select_in_active((pos, pos));
            }
        } else if close {
            self.goto_line = None;
        }
    }

    fn quick_open_dialog(&mut self, ctx: &egui::Context) {
        let Some(qo) = &mut self.quick_open else { return };
        let root = self.explorer.root.clone();
        let rel = |p: &Path| -> String {
            root.as_ref().and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).display().to_string()
        };
        let mut results: Vec<(i32, &PathBuf, String)> = qo
            .files
            .iter()
            .filter_map(|p| {
                let r = rel(p);
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let s = explorer::fuzzy_score(&r, &qo.query)?;
                // Prefer hits in the file name itself.
                let bonus = explorer::fuzzy_score(&name, &qo.query).map(|_| 40).unwrap_or(0);
                Some((s + bonus, p, r))
            })
            .collect();
        results.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.len().cmp(&b.2.len())));
        results.truncate(60);

        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            qo.selected = (qo.selected + 1).min(results.len().saturating_sub(1));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            qo.selected = qo.selected.saturating_sub(1);
        }
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));

        let mut open = None;
        let mut close = false;
        egui::Window::new("Go to File")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .fixed_size([560.0, 380.0])
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut qo.query)
                        .desired_width(f32::INFINITY)
                        .hint_text(if root.is_some() { "Search files by name" } else { "Open a folder to search it — showing recent files" }),
                );
                if r.changed() {
                    qo.selected = 0;
                }
                if qo.focus {
                    r.request_focus();
                    qo.focus = false;
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                    if results.is_empty() {
                        ui.label(RichText::new("No matching files").weak());
                    }
                    for (i, (_, p, r)) in results.iter().enumerate() {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let (badge, color) = explorer::file_badge(&name);
                        let selected = i == qo.selected;
                        let resp = ui
                            .horizontal(|ui| {
                                ui.label(RichText::new(badge).monospace().size(10.0).color(color));
                                let resp = ui.selectable_label(selected, RichText::new(&name).strong());
                                ui.label(RichText::new(r).weak().small());
                                resp
                            })
                            .inner;
                        if selected {
                            resp.scroll_to_me(None);
                        }
                        if resp.clicked() {
                            open = Some((*p).clone());
                        }
                    }
                });
                if enter {
                    open = results.get(qo.selected).map(|(_, p, _)| (*p).clone());
                    if open.is_none() {
                        close = true;
                    }
                }
            });
        if let Some(p) = open {
            self.quick_open = None;
            self.open_path(&p);
        } else if close {
            self.quick_open = None;
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new("About Wedit")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(72.0, 72.0)));
                    }
                    ui.label(RichText::new("Wedit").size(26.0).strong().color(GOLD));
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(6.0);
                    ui.label("A lightweight code editor written in Rust.");
                    ui.label("Syntax highlighting for Rust (.rs) and C# (.cs).");
                });
            });
        self.show_about = open;
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.active_doc() {
            Some(doc) => format!("Wedit - Editing {}", doc.title()),
            None => "Wedit".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        let dirty = self.docs.iter().filter(|d| d.is_dirty()).count();
        if dirty == 0 {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        let res = rfd::MessageDialog::new()
            .set_title("Wedit")
            .set_level(rfd::MessageLevel::Warning)
            .set_description(format!(
                "You have {dirty} file{} with unsaved changes.\n\nSave before closing?",
                if dirty == 1 { "" } else { "s" }
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        let close = match res {
            rfd::MessageDialogResult::Yes => self.save_all(),
            rfd::MessageDialogResult::No => true,
            _ => false,
        };
        if close {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in dropped {
            if p.is_dir() {
                self.open_folder(p);
            } else {
                self.open_path(&p);
            }
        }
    }
}

impl eframe::App for WeditApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let pal = self.settings.theme.palette();
        self.handle_close_request(ctx);
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);

        self.menu_bar(ctx);
        self.tab_bar(ctx, &pal);
        self.find_bar(ctx);
        self.status_bar(ctx, &pal);
        self.sidebar(ctx, &pal);

        egui::CentralPanel::default().frame(egui::Frame::default().fill(pal.bg)).show(ctx, |ui| {
            if self.docs.is_empty() {
                self.welcome(ui, &pal);
            } else {
                self.active = self.active.min(self.docs.len() - 1);
                self.editor(ui, &pal);
            }
        });

        self.goto_dialog(ctx);
        self.quick_open_dialog(ctx);
        self.about_window(ctx);
        self.update_title(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.settings.open_files = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        eframe::set_value(storage, SETTINGS_KEY, &self.settings);
    }
}

fn item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
    let clicked = ui.add(egui::Button::new(label).shortcut_text(shortcut)).clicked();
    if clicked {
        ui.close_menu();
    }
    clicked
}

/// All non-overlapping matches of `query` in `text` as byte ranges.
fn find_all(text: &str, query: &str, match_case: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    // ASCII-only case folding keeps byte offsets identical between haystack and original.
    let (hay, needle) = if match_case {
        (std::borrow::Cow::Borrowed(text), std::borrow::Cow::Borrowed(query))
    } else {
        (std::borrow::Cow::Owned(text.to_ascii_lowercase()), std::borrow::Cow::Owned(query.to_ascii_lowercase()))
    };
    hay.match_indices(needle.as_ref()).map(|(i, m)| i..i + m.len()).collect()
}

fn highlight_job(text: &str, lang: Lang, theme: ThemeKind, font_id: FontId) -> LayoutJob {
    let mut job = LayoutJob { text: text.to_owned(), ..Default::default() };
    for (range, tok) in syntax::tokenize(lang, text) {
        let mut format = TextFormat::simple(font_id.clone(), theme.token_color(tok));
        if matches!(tok, syntax::Tok::Comment | syntax::Tok::DocComment) {
            format.italics = true;
        }
        job.sections.push(LayoutSection { leading_space: 0.0, byte_range: range, format });
    }
    if job.sections.is_empty() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: 0..0,
            format: TextFormat::simple(font_id, theme.token_color(syntax::Tok::Plain)),
        });
    }
    job
}

/// Prefer the native Windows fonts (Consolas / Segoe UI) when available.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let try_load = |file: &str| std::fs::read(Path::new(&windir).join("Fonts").join(file)).ok();
    if let Some(bytes) = try_load("consola.ttf") {
        fonts.font_data.insert("consolas".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "consolas".into());
    }
    if let Some(bytes) = try_load("segoeui.ttf") {
        fonts.font_data.insert("segoeui".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoeui".into());
    }
    if let Some(bytes) = try_load("seguisym.ttf") {
        fonts.font_data.insert("segoe-symbol".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().push("segoe-symbol".into());
    }
    ctx.set_fonts(fonts);
}
