//! Sidebar folder tree and the file index used by Quick Open (Ctrl+P).

use crate::theme::{Palette, GOLD};
use egui::{Color32, RichText};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[".git", ".vs", ".idea", "target", "bin", "obj", "node_modules", ".svn"];

pub struct Explorer {
    pub root: Option<PathBuf>,
    listings: HashMap<PathBuf, Vec<Entry>>,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

impl Explorer {
    pub fn new() -> Self {
        Self { root: None, listings: HashMap::new() }
    }

    pub fn set_root(&mut self, root: Option<PathBuf>) {
        self.root = root;
        self.listings.clear();
    }

    pub fn refresh(&mut self) {
        self.listings.clear();
    }

    fn list(&mut self, dir: &Path) -> &[Entry] {
        self.listings.entry(dir.to_path_buf()).or_insert_with(|| {
            let mut v: Vec<Entry> = std::fs::read_dir(dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| {
                            let path = e.path();
                            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            Entry { name: e.file_name().to_string_lossy().into_owned(), path, is_dir }
                        })
                        .filter(|e| e.name != ".git")
                        .collect()
                })
                .unwrap_or_default();
            v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            v
        })
    }

    /// Draws the tree; returns a file path the user clicked.
    pub fn ui(&mut self, ui: &mut egui::Ui, pal: &Palette, active: Option<&Path>) -> Option<PathBuf> {
        let Some(root) = self.root.clone() else {
            ui.add_space(8.0);
            ui.label(RichText::new("No folder opened.").color(pal.dim));
            ui.add_space(4.0);
            return None;
        };
        let mut clicked = None;
        self.dir_ui(ui, &root, pal, active, &mut clicked, true);
        clicked
    }

    fn dir_ui(&mut self, ui: &mut egui::Ui, dir: &Path, pal: &Palette, active: Option<&Path>, clicked: &mut Option<PathBuf>, is_root: bool) {
        let entries = self.list(dir).to_vec();
        if entries.is_empty() && is_root {
            ui.label(RichText::new("(empty folder)").color(pal.dim));
        }
        for e in entries {
            if e.is_dir {
                let dimmed = SKIP_DIRS.contains(&e.name.as_str());
                let label = RichText::new(&e.name).color(if dimmed { pal.dim } else { pal.text });
                egui::CollapsingHeader::new(label)
                    .id_salt(&e.path)
                    .default_open(false)
                    .show(ui, |ui| self.dir_ui(ui, &e.path, pal, active, clicked, false));
            } else {
                let (badge, color) = file_badge(&e.name);
                let selected = active == Some(e.path.as_path());
                let resp = ui
                    .horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(RichText::new(badge).monospace().size(10.0).color(color));
                        ui.selectable_label(selected, RichText::new(&e.name).color(pal.text))
                    })
                    .inner;
                if resp.clicked() {
                    *clicked = Some(e.path.clone());
                }
                resp.on_hover_text(e.path.display().to_string());
            }
        }
    }
}

pub fn file_badge(name: &str) -> (&'static str, Color32) {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "rs" => ("RS", Color32::from_rgb(222, 120, 60)),
        "cs" | "csx" => ("C#", Color32::from_rgb(120, 190, 90)),
        "toml" => ("TM", Color32::from_rgb(160, 160, 160)),
        "csproj" | "sln" => ("PR", Color32::from_rgb(160, 120, 220)),
        "md" => ("MD", Color32::from_rgb(90, 160, 230)),
        "json" => ("{}", GOLD),
        _ => ("··", Color32::from_rgb(130, 130, 140)),
    }
}

/// Recursively list files below `root` (skipping build/VCS folders), capped at `limit`.
pub fn index_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                let name = e.file_name();
                if !SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                    stack.push(path);
                }
            } else {
                out.push(path);
                if out.len() >= limit {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Simple fuzzy score: all query chars must appear in order. Higher is better.
pub fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.to_lowercase().chars().collect();
    let mut score = 0;
    let mut ci = 0;
    let mut last_match: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        if q == ' ' {
            continue;
        }
        let mut found = false;
        while ci < cand.len() {
            if cand[ci] == q {
                score += 10;
                if last_match.is_some_and(|l| l + 1 == ci) {
                    score += 15;
                }
                if ci == 0 || matches!(cand[ci - 1], '/' | '\\' | '_' | '-' | '.') {
                    score += 10;
                }
                last_match = Some(ci);
                ci += 1;
                found = true;
                break;
            }
            ci += 1;
        }
        if !found {
            return None;
        }
    }
    Some(score - cand.len() as i32 / 4)
}
