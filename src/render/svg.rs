//! SVG: parsing with the system's fonts, and drawing into bitmaps. Images, diagrams and icons all draw through it.
use std::sync::{Arc, OnceLock};

use resvg::usvg;

use crate::render::page::Bitmap;

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        #[cfg(target_os = "macos")]
        {
            for f in [
                "/System/Library/Fonts/SFNS.ttf",
                "/System/Library/Fonts/Helvetica.ttc",
                "/System/Library/Fonts/Supplemental/Arial.ttf",
                "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
                "/System/Library/Fonts/Menlo.ttc",
            ] {
                if let Ok(d) = std::fs::read(f) {
                    db.load_font_data(d);
                }
            }
            db.set_sans_serif_family("Helvetica");
            db.set_serif_family("Helvetica");
            db.set_monospace_family("Menlo");
        }
        #[cfg(target_os = "linux")]
        {
            use cosmic_text::fontdb::{Family, Source};
            let fonts = crate::render::text::database();
            let mut paths = std::collections::HashSet::new();
            for face in fonts.faces() {
                match &face.source {
                    Source::File(path) | Source::SharedFile(path, _) if paths.insert(path) => {
                        let _ = db.load_font_file(path);
                    }
                    Source::Binary(data) => db.load_font_data(data.as_ref().as_ref().to_vec()),
                    _ => {}
                }
            }
            db.set_sans_serif_family(fonts.family_name(&Family::SansSerif));
            db.set_serif_family(fonts.family_name(&Family::Serif));
            db.set_monospace_family(fonts.family_name(&Family::Monospace));
        }
        Arc::new(db)
    })
    .clone()
}

/// The SF family's name in `fontdb`: macOS's system font file calls itself this.
#[cfg(target_os = "macos")]
pub const SYSTEM_FONT: &str = "System Font";
/// Linux's generic family, resolved from the installed font database.
#[cfg(target_os = "linux")]
pub const SYSTEM_FONT: &str = "sans-serif";

/// A theme's font list for SVG text, with generic system names resolved on each platform.
pub fn svg_families(fonts: &[String]) -> String {
    let mut out = Vec::new();
    for f in fonts {
        match f.to_ascii_lowercase().as_str() {
            n if crate::render::text::system_ui(n) || matches!(n, "sf pro" | "sf pro text" | "sf pro display") => {
                out.push(SYSTEM_FONT.to_string());
                break;
            }
            "ui-monospace" | "monospace" => {
                out.push("monospace".into());
                break;
            }
            "ui-serif" | "serif" => {
                out.push("serif".into());
                break;
            }
            _ => out.push(f.clone()),
        }
    }
    out.join(", ")
}

pub fn svg_tree(data: &[u8]) -> Option<usvg::Tree> {
    let db = fontdb();
    let family = db.family_name(&usvg::fontdb::Family::SansSerif).to_string();
    let o = usvg::Options { fontdb: db, font_family: family, ..usvg::Options::default() };
    usvg::Tree::from_data(data, &o).ok()
}

/// The widest or tallest an SVG draws, in pixels. An SVG may declare any size, and a bigger draw panics in
/// tiny-skia or takes gigabytes.
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels an SVG draws: 128 MiB of pixmap. Two sides at `MAX_SIDE` would take 1 GiB.
pub const MAX_PIXELS: u64 = 1 << 25;

fn too_big(w: u32, h: u32) -> bool {
    w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS
}

/// A `w` by `h` draw size scaled down, keeping its aspect, to whole pixels that `render_svg` accepts.
pub fn fit(w: f32, h: f32) -> (f32, f32) {
    // Rounded first: a size that fits only before rounding, such as 2399.5 by 13983.5, would not draw.
    let (w, h) = (f64::from(w.round().max(1.0)), f64::from(h.round().max(1.0)));
    // In f64: f32 overflows the area of a declared size near its limit, and rounds the scale for one a pixel
    // over the cap, such as 2761 by 12153, up to 1.
    let side = f64::from(MAX_SIDE);
    let k = (side / w).min(side / h).min((MAX_PIXELS as f64 / (w * h)).sqrt());
    if k >= 1.0 {
        return (w as f32, h as f32);
    }
    // Nearest keeps a size cut to the side cap at 16384, and the floor is the fallback that always fits.
    let near = ((w * k).round().max(1.0), (h * k).round().max(1.0));
    let (fw, fh) = if too_big(near.0 as u32, near.1 as u32) { ((w * k).floor().max(1.0), (h * k).floor().max(1.0)) } else { near };
    (fw as f32, fh as f32)
}

pub fn render_svg(t: &usvg::Tree, w: u32, h: u32) -> Option<Bitmap> {
    if too_big(w, h) {
        return None;
    }
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let s = t.size();
    let tf = resvg::tiny_skia::Transform::from_scale(w as f32 / s.width(), h as f32 / s.height());
    resvg::render(t, tf, &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

/// A `w` by `h` window onto `t` drawn at `z` pixels per SVG unit, with SVG point (`x`, `y`) at its top left.
pub fn render_view(t: &usvg::Tree, w: u32, h: u32, z: f32, x: f32, y: f32) -> Option<Bitmap> {
    if too_big(w, h) {
        return None;
    }
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(t, resvg::tiny_skia::Transform::from_row(z, 0.0, 0.0, z, -x * z, -y * z), &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_huge_draw_fits_the_limits() {
        for (w, h) in [(20.0, 4e9), (4e9, 20.0), (16_384.0, 16_384.0), (3000.0, 16_000.0), (1.0, 1e30), (2399.5, 13_983.5), (2761.0, 12_153.0)] {
            let (fw, fh) = super::fit(w, h);
            assert!(!super::too_big(fw.round() as u32, fh.round() as u32), "{w}x{h} fit to {fw}x{fh}");
            assert!(fw >= 1.0 && fh >= 1.0);
        }
        assert_eq!(super::fit(800.0, 600.0), (800.0, 600.0));
        assert_eq!(super::fit(20.0, 1e38).1, 16_384.0);
        assert!(super::too_big(16_384, 16_384));
    }

    #[test]
    fn generic_names_reach_installed_svg_fonts() {
        let fonts = ["system-ui", "-apple-system", "Avenir Next"].map(String::from);
        assert_eq!(super::svg_families(&fonts), super::SYSTEM_FONT);
        let fonts = ["Avenir Next", "ui-sans-serif", "Inter"].map(String::from);
        assert_eq!(super::svg_families(&fonts), format!("Avenir Next, {}", super::SYSTEM_FONT));
        let db = super::fontdb();
        #[cfg(target_os = "macos")]
        let family = super::SYSTEM_FONT;
        #[cfg(target_os = "linux")]
        let family = db.family_name(&super::usvg::fontdb::Family::SansSerif);
        assert!(db.faces().any(|f| f.families.iter().any(|(n, _)| n == family)));
    }
}
