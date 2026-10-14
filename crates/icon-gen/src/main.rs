//! Generates the Wedit "Golden W" icon.
//!
//! Outputs (into the `assets/` folder at the workspace root):
//!   * `wedit.ico`        – multi-resolution Windows icon (16..256 px)
//!   * `wedit.png`        – 512 px preview
//!   * `wedit_64.rgba`    – raw 64x64 RGBA used for the runtime window icon
//!   * `wedit_128.rgba`   – raw 128x128 RGBA used for the welcome screen logo
//!
//! Run with: `cargo run -p icon-gen --release`

use std::path::PathBuf;
use tiny_skia::*;

fn main() {
    let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    std::fs::create_dir_all(&assets).expect("create assets dir");

    // Multi-size ICO. Each size is rendered natively for crisp small icons.
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16u32, 20, 24, 32, 40, 48, 64, 128, 256] {
        let pm = render(size);
        let img = ico::IconImage::from_rgba_data(size, size, unpremultiplied(&pm));
        dir.add_entry(ico::IconDirEntry::encode(&img).expect("encode icon entry"));
    }
    let ico_path = assets.join("wedit.ico");
    let file = std::fs::File::create(&ico_path).expect("create ico");
    dir.write(file).expect("write ico");

    render(512).save_png(assets.join("wedit.png")).expect("write png");
    std::fs::write(assets.join("wedit_64.rgba"), unpremultiplied(&render(64))).expect("write rgba");
    std::fs::write(assets.join("wedit_128.rgba"), unpremultiplied(&render(128))).expect("write rgba");

    println!("Icon written to {}", ico_path.canonicalize().unwrap_or(ico_path).display());
}

fn unpremultiplied(pm: &Pixmap) -> Vec<u8> {
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn render(size: u32) -> Pixmap {
    let mut pm = Pixmap::new(size, size).unwrap();
    let s = size as f32;
    let small = size <= 24;

    // --- Background: rounded square, deep charcoal with a soft vertical gradient.
    let margin = if small { 0.0 } else { s * 0.04 };
    let radius = s * 0.22;
    let bg = rounded_rect(margin, margin, s - 2.0 * margin, s - 2.0 * margin, radius);
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, s),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(44, 40, 52, 255)),
            GradientStop::new(1.0, Color::from_rgba8(18, 16, 22, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&bg, &paint, FillRule::Winding, Transform::identity(), None);

    // Thin golden rim around the tile.
    if !small {
        let mut rim = Paint::default();
        rim.anti_alias = true;
        rim.set_color_rgba8(212, 160, 23, 110);
        let stroke = Stroke { width: (s * 0.012).max(1.0), ..Default::default() };
        pm.stroke_path(&bg, &rim, &stroke, Transform::identity(), None);
    }

    // --- The "W": four slanted strokes with flat (horizontal) ends.
    // Each stroke is (top_x0, top_x1, top_y, bottom_x0, bottom_x1, bottom_y) in unit space.
    let strokes: [(f32, f32, f32, f32, f32, f32); 4] = [
        (0.12, 0.28, 0.24, 0.29, 0.43, 0.78),
        (0.42, 0.58, 0.40, 0.29, 0.43, 0.78),
        (0.42, 0.58, 0.40, 0.57, 0.71, 0.78),
        (0.72, 0.88, 0.24, 0.57, 0.71, 0.78),
    ];
    let pieces: Vec<Path> = strokes
        .iter()
        .map(|&(tx0, tx1, ty, bx0, bx1, by)| {
            let mut pb = PathBuilder::new();
            pb.move_to(tx0 * s, ty * s);
            pb.line_to(tx1 * s, ty * s);
            pb.line_to(bx1 * s, by * s);
            pb.line_to(bx0 * s, by * s);
            pb.close();
            pb.finish().unwrap()
        })
        .collect();

    if !small {
        // Soft drop shadow.
        let mut shadow = Paint::default();
        shadow.anti_alias = true;
        shadow.set_color_rgba8(0, 0, 0, 110);
        for p in &pieces {
            pm.fill_path(p, &shadow, FillRule::Winding, Transform::from_translate(s * 0.012, s * 0.025), None);
        }
        // Bronze outline: stroke every piece first, then the gold fill covers the inner seams.
        let mut outline = Paint::default();
        outline.anti_alias = true;
        outline.set_color_rgba8(105, 68, 6, 255);
        let st = Stroke { width: s * 0.03, line_join: LineJoin::Round, ..Default::default() };
        for p in &pieces {
            pm.stroke_path(p, &outline, &st, Transform::identity(), None);
        }
    }

    // Gold body: bright highlight at the top fading to deep amber.
    let mut gold = Paint::default();
    gold.anti_alias = true;
    gold.shader = LinearGradient::new(
        Point::from_xy(0.0, s * 0.22),
        Point::from_xy(0.0, s * 0.80),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(255, 242, 170, 255)),
            GradientStop::new(0.3, Color::from_rgba8(255, 208, 64, 255)),
            GradientStop::new(0.7, Color::from_rgba8(226, 162, 22, 255)),
            GradientStop::new(1.0, Color::from_rgba8(178, 114, 10, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    for p in &pieces {
        pm.fill_path(p, &gold, FillRule::Winding, Transform::identity(), None);
    }

    pm
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    let k = 0.5523 * r; // cubic bezier circle approximation
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}
