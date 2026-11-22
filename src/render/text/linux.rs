//! Unicode shaping with cosmic-text and grayscale glyph rasterization with Swash.
use super::*;
use cosmic_text::{Attrs, Buffer, Color, Cursor, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap, fontdb};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use tiny_skia::{Mask, PixmapMut};

/// The installed fonts, scanned once and shared by each engine's font system.
pub(crate) fn database() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let sans = preferred(&db, &["DejaVu Sans", "Liberation Sans", "Noto Sans", "Adwaita Sans", "Cantarell", "Open Sans"], false);
        let serif = preferred(&db, &["DejaVu Serif", "Liberation Serif", "Noto Serif", "STIX Two Text"], false);
        let mono = preferred(&db, &["DejaVu Sans Mono", "Liberation Mono", "Noto Sans Mono", "Adwaita Mono", "JetBrains Mono"], true);
        db.set_sans_serif_family(sans);
        db.set_serif_family(serif);
        db.set_monospace_family(mono);
        db
    })
}

fn installed(db: &fontdb::Database, name: &str) -> bool {
    db.faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}

fn preferred(db: &fontdb::Database, names: &[&str], mono: bool) -> String {
    names.iter().find(|n| installed(db, n)).map_or_else(
        || db.faces().find(|f| (!mono || f.monospaced) && !f.post_script_name.contains("Emoji")).and_then(|f| f.families.first()).map_or_else(|| names[0].to_string(), |(n, _)| n.clone()),
        |n| (*n).to_string(),
    )
}

pub struct Fonts {
    system: FontSystem,
    swash: SwashCache,
    lines: HashMap<(FontKey, String), Rc<Line>>,
    body: Vec<String>,
    code: Vec<String>,
    scale: f32,
}

struct Line {
    buffer: Buffer,
    width: f32,
    ascent: f32,
    descent: f32,
}

impl Fonts {
    pub fn new() -> Fonts {
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"].iter().find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty())).unwrap_or_else(|| "en-US".into());
        Fonts { system: FontSystem::new_with_locale_and_db(locale, database().clone()), swash: SwashCache::new(), lines: HashMap::new(), body: vec![], code: vec![], scale: 1.0 }
    }

    pub fn set_families(&mut self, body: Vec<String>, code: Vec<String>) {
        if (&body, &code) != (&self.body, &self.code) {
            (self.body, self.code) = (body, code);
            self.lines.clear();
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        if scale != self.scale {
            self.scale = scale;
            self.lines.clear();
            self.swash.image_cache.clear();
        }
    }

    fn family(&self, k: FontKey) -> Family<'_> {
        if k.style & MATH != 0 {
            for name in ["STIX Two Math", "STIXGeneral", "STIX", "Latin Modern Math"] {
                if installed(self.system.db(), name) {
                    return Family::Name(name);
                }
            }
            return Family::Serif;
        }
        let names = if k.style & MONO != 0 { &self.code } else { &self.body };
        for name in names {
            match name.to_ascii_lowercase().as_str() {
                n if system_ui(n) => return Family::SansSerif,
                "ui-monospace" | "monospace" => return Family::Monospace,
                "ui-serif" | "serif" => return Family::Serif,
                _ if installed(self.system.db(), name) => return Family::Name(name),
                _ => {}
            }
        }
        if k.style & MONO != 0 { Family::Monospace } else { Family::SansSerif }
    }

    fn line(&mut self, k: FontKey, s: &str) -> Rc<Line> {
        let key = (k, s.to_string());
        if let Some(line) = self.lines.get(&key) {
            return line.clone();
        }
        if self.lines.len() > 20_000 {
            self.lines.clear();
        }
        let size = k.px().max(0.0625);
        let mut features = FontFeatures::new();
        if k.style & TABULAR != 0 {
            features.enable(FeatureTag::new(b"tnum"));
        }
        let css_weight = k.style >> WEIGHT_SHIFT;
        let weight = if css_weight != 0 {
            css_weight
        } else if k.style & BOLD != 0 {
            700
        } else {
            400
        };
        let attrs = Attrs::new()
            .family(self.family(k))
            .weight(Weight(weight))
            .style(if k.style & ITALIC != 0 { Style::Italic } else { Style::Normal })
            .letter_spacing(f32::from(k.track) / 1000.0)
            .font_features(features);
        let mut buffer = Buffer::new_empty(Metrics::new(size, size * 1.5));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        buffer.set_text(s, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.system, false);
        let (mut ascent, mut descent, mut width) = (0.0f32, 0.0f32, 0.0f32);
        for line in buffer.lines.iter().filter_map(|l| l.layout_opt()).flatten() {
            ascent = ascent.max(line.max_ascent);
            descent = descent.max(line.max_descent);
            width = width.max(line.w);
        }
        if ascent + descent == 0.0 {
            (ascent, descent) = (size * 0.8, size * 0.2);
        }
        let line = Rc::new(Line { buffer, width, ascent, descent });
        self.lines.insert(key, line.clone());
        line
    }

    pub fn measure(&mut self, k: FontKey, s: &str) -> f32 {
        self.line(k, s).width
    }

    pub fn ascent_descent(&mut self, k: FontKey) -> (f32, f32) {
        let line = self.line(FontKey { track: 0, ..k }, "Mg");
        (line.ascent, line.descent)
    }

    /// The shaped caret position at a character index, including ligatures and Unicode clusters.
    pub fn char_x(&mut self, k: FontKey, s: &str, idx: usize) -> f32 {
        if idx == 0 {
            return 0.0;
        }
        let Some((byte, _)) = s.char_indices().nth(idx) else { return self.measure(k, s) };
        self.line(k, s).buffer.layout_runs().next().and_then(|r| r.cursor_position(&Cursor::new(0, byte))).unwrap_or(0.0)
    }

    /// Draws at a baseline in layout pixels, with `z` output pixels per layout pixel.
    #[expect(clippy::too_many_arguments, reason = "the glyph run, baseline, output scale, color and clip")]
    pub fn draw(&mut self, dst: &mut PixmapMut<'_>, k: FontKey, s: &str, x: f32, y: f32, z: f32, color: u32, mask: Option<&Mask>) {
        let line = self.line(k, s);
        let baseline = line.buffer.layout_runs().next().map_or(0.0, |r| r.line_y);
        let (w, h) = (dst.width() as i32, dst.height() as i32);
        let base = Color::rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        let pixels = dst.data_mut();
        if self.swash.image_cache.len() > 20_000 {
            self.swash.image_cache.clear();
        }
        for run in line.buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((x * z, (y + run.line_y - baseline) * z), z);
                self.swash.with_pixels(&mut self.system, physical.cache_key, base, |gx, gy, ink| {
                    let (px, py) = (physical.x + gx, physical.y + gy);
                    if px < 0 || py < 0 || px >= w || py >= h {
                        return;
                    }
                    let i = py as usize * w as usize + px as usize;
                    let coverage = mask.map_or(255, |m| m.data()[i]);
                    blend(&mut pixels[i * 4..i * 4 + 4], ink, coverage);
                });
            }
        }
    }
}

/// Source-over into premultiplied RGBA; Swash supplies straight RGBA and clip coverage.
fn blend(dst: &mut [u8], color: Color, coverage: u8) {
    let alpha = (u32::from(color.a()) * u32::from(coverage) + 127) / 255;
    let inv = 255 - alpha;
    for (dst, src) in dst[..3].iter_mut().zip([color.r(), color.g(), color.b()]) {
        *dst = ((u32::from(src) * alpha + u32::from(*dst) * inv + 127) / 255) as u8;
    }
    dst[3] = (alpha + (u32::from(dst[3]) * inv + 127) / 255) as u8;
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaping_measuring_and_scaling_agree() {
        let mut fonts = Fonts::new();
        let k = FontKey::new(18.0, 0);
        let w = fonts.measure(k, "office 中文 é");
        assert!(w > 0.0 && w.is_finite());
        assert!(fonts.measure(k.tracked(0.1), "office 中文 é") > w);
        assert!((fonts.measure(FontKey::new(36.0, 0), "office 中文 é") - 2.0 * w).abs() < 0.1);
        assert_eq!(fonts.char_x(k, "é🙂a", 3), fonts.measure(k, "é🙂a"));
        assert!(fonts.char_x(k, "office", 2) > fonts.char_x(k, "office", 1));
        let (a, d) = fonts.ascent_descent(k);
        assert!(a > 0.0 && d >= 0.0);
    }

    #[test]
    fn generic_and_missing_families_have_installed_fallbacks() {
        let mut fonts = Fonts::new();
        fonts.set_families(vec!["not-a-real-font".into(), "system-ui".into()], vec!["monospace".into()]);
        assert_eq!(fonts.family(FontKey::new(16.0, 0)), Family::SansSerif);
        assert_eq!(fonts.family(FontKey::new(16.0, MONO)), Family::Monospace);
        let k = FontKey::new(16.0, MONO);
        assert!((fonts.measure(k, "iii") - fonts.measure(k, "WWW")).abs() < 0.1);
    }

    #[test]
    fn glyph_compositing_keeps_premultiplied_alpha_and_clip() {
        let mut px = [0, 0, 0, 0];
        blend(&mut px, Color::rgba(200, 100, 50, 128), 255);
        assert_eq!(px, [100, 50, 25, 128]);
        let before = px;
        blend(&mut px, Color::rgb(255, 255, 255), 0);
        assert_eq!(px, before);
    }
}
