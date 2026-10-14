use crate::syntax::Tok;
use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ThemeKind {
    Dark,
    Light,
}

pub const GOLD: Color32 = Color32::from_rgb(232, 178, 40);

pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub sidebar: Color32,
    pub gutter_fg: Color32,
    pub gutter_active: Color32,
    pub current_line: Color32,
    pub status_bg: Color32,
    pub status_fg: Color32,
    pub tab_active: Color32,
    pub tab_inactive: Color32,
    pub text: Color32,
    pub dim: Color32,
}

impl ThemeKind {
    pub fn palette(self) -> Palette {
        match self {
            ThemeKind::Dark => Palette {
                bg: Color32::from_rgb(30, 30, 34),
                panel: Color32::from_rgb(37, 37, 42),
                sidebar: Color32::from_rgb(26, 26, 30),
                gutter_fg: Color32::from_rgb(98, 98, 108),
                gutter_active: Color32::from_rgb(214, 214, 220),
                current_line: Color32::from_rgba_premultiplied(9, 9, 10, 9),
                status_bg: Color32::from_rgb(58, 44, 12),
                status_fg: Color32::from_rgb(248, 226, 170),
                tab_active: Color32::from_rgb(30, 30, 34),
                tab_inactive: Color32::from_rgb(44, 44, 50),
                text: Color32::from_rgb(212, 212, 212),
                dim: Color32::from_rgb(140, 140, 150),
            },
            ThemeKind::Light => Palette {
                bg: Color32::from_rgb(255, 255, 255),
                panel: Color32::from_rgb(243, 243, 243),
                sidebar: Color32::from_rgb(237, 237, 240),
                gutter_fg: Color32::from_rgb(160, 160, 168),
                gutter_active: Color32::from_rgb(40, 40, 40),
                current_line: Color32::from_black_alpha(9),
                status_bg: Color32::from_rgb(196, 140, 20),
                status_fg: Color32::from_rgb(255, 255, 255),
                tab_active: Color32::from_rgb(255, 255, 255),
                tab_inactive: Color32::from_rgb(232, 232, 236),
                text: Color32::from_rgb(30, 30, 30),
                dim: Color32::from_rgb(110, 110, 120),
            },
        }
    }

    pub fn token_color(self, tok: Tok) -> Color32 {
        use Tok::*;
        let rgb = Color32::from_rgb;
        match self {
            ThemeKind::Dark => match tok {
                Plain => rgb(212, 212, 212),
                Keyword => rgb(86, 156, 214),
                Control => rgb(197, 134, 192),
                Type => rgb(78, 201, 176),
                Function => rgb(220, 220, 170),
                Macro => rgb(232, 178, 40),
                String => rgb(206, 145, 120),
                Escape => rgb(215, 186, 125),
                Number => rgb(181, 206, 168),
                Comment => rgb(106, 153, 85),
                DocComment => rgb(96, 139, 78),
                Attribute => rgb(156, 156, 210),
                Lifetime => rgb(86, 156, 214),
                Constant => rgb(79, 193, 255),
                Punct => rgb(180, 180, 180),
                Preprocessor => rgb(155, 155, 155),
            },
            ThemeKind::Light => match tok {
                Plain => rgb(30, 30, 30),
                Keyword => rgb(0, 0, 255),
                Control => rgb(175, 0, 219),
                Type => rgb(38, 127, 153),
                Function => rgb(121, 94, 38),
                Macro => rgb(170, 110, 0),
                String => rgb(163, 21, 21),
                Escape => rgb(238, 0, 0),
                Number => rgb(9, 134, 88),
                Comment => rgb(0, 128, 0),
                DocComment => rgb(0, 110, 0),
                Attribute => rgb(80, 80, 180),
                Lifetime => rgb(0, 0, 255),
                Constant => rgb(0, 112, 193),
                Punct => rgb(60, 60, 60),
                Preprocessor => rgb(128, 128, 128),
            },
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let p = self.palette();
        let mut v = match self {
            ThemeKind::Dark => egui::Visuals::dark(),
            ThemeKind::Light => egui::Visuals::light(),
        };
        v.panel_fill = p.panel;
        v.window_fill = p.panel;
        v.extreme_bg_color = p.bg;
        v.code_bg_color = p.bg;
        v.override_text_color = None;
        v.selection.bg_fill = match self {
            ThemeKind::Dark => Color32::from_rgba_unmultiplied(232, 178, 40, 70),
            ThemeKind::Light => Color32::from_rgba_unmultiplied(232, 178, 40, 90),
        };
        v.selection.stroke.color = GOLD;
        v.hyperlink_color = GOLD;
        v.text_cursor.stroke.color = GOLD;
        v.text_cursor.stroke.width = 2.0;
        v.widgets.hovered.bg_stroke.color = GOLD.gamma_multiply(0.6);
        v.window_corner_radius = 6.0.into();
        ctx.set_visuals(v);
    }
}
