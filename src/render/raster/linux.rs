//! Software rasterization of the display list with tiny-skia.
use super::Ctx;
use crate::render::icons;
use crate::render::page::*;
use crate::render::text::{FontKey, Fonts};
use std::collections::HashMap;
use tiny_skia::{Color, FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, PixmapMut, PixmapPaint, PixmapRef, Rect, Stroke, Transform};

type BoxRect = [f32; 4];

struct Canvas<'a> {
    pixels: PixmapMut<'a>,
    z: f32,
    top: f32,
}

fn paint(color: u32, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8((color >> 16) as u8, (color >> 8) as u8, color as u8, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    p
}

fn rounded([x, y, w, h]: BoxRect, radius: f32) -> Option<Path> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let rect = Rect::from_xywh(x, y, w, h)?;
    if r == 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let (x2, y2, k) = (x + w, y + h, r * 0.552_284_8);
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x2 - r, y);
    p.cubic_to(x2 - r + k, y, x2, y + r - k, x2, y + r);
    p.line_to(x2, y2 - r);
    p.cubic_to(x2, y2 - r + k, x2 - r + k, y2, x2 - r, y2);
    p.line_to(x + r, y2);
    p.cubic_to(x + r - k, y2, x, y2 - r + k, x, y2 - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.close();
    p.finish()
}

impl Canvas<'_> {
    fn transform(&self) -> Transform {
        Transform::from_row(self.z, 0.0, 0.0, self.z, 0.0, -self.top * self.z)
    }

    fn mask(&self, rect: BoxRect) -> Option<Mask> {
        let mut mask = Mask::new(self.pixels.width(), self.pixels.height())?;
        if let Some(path) = rounded(rect, 0.0) {
            mask.fill_path(&path, FillRule::Winding, false, self.transform());
        }
        Some(mask)
    }

    fn fill(&mut self, rect: BoxRect, radius: f32, color: u32, alpha: f32, mask: Option<&Mask>) {
        if let Some(path) = rounded(rect, radius) {
            self.pixels.fill_path(&path, &paint(color, alpha), FillRule::Winding, self.transform(), mask);
        }
    }

    fn stroke(&mut self, path: &Path, width: f32, color: u32, alpha: f32, mask: Option<&Mask>) {
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        self.pixels.stroke_path(path, &paint(color, alpha), &stroke, self.transform(), mask);
    }

    fn image(&mut self, b: &Bitmap, [x, y, w, h]: BoxRect, alpha: f32, mask: Option<&Mask>) {
        if let Some(src) = PixmapRef::from_bytes(&b.px, b.w, b.h) {
            let tf = Transform::from_row(self.z * w / b.w as f32, 0.0, 0.0, self.z * h / b.h as f32, x * self.z, (y - self.top) * self.z);
            self.pixels.draw_pixmap(0, 0, src, &PixmapPaint { opacity: alpha, quality: FilterQuality::Bilinear, ..PixmapPaint::default() }, tf, mask);
        }
    }

    #[expect(clippy::too_many_arguments, reason = "the font run, baseline, color and clip")]
    fn text(&mut self, fonts: &mut Fonts, f: FontKey, text: &str, x: f32, y: f32, color: u32, mask: Option<&Mask>) {
        fonts.draw(&mut self.pixels, f, text, x, y - self.top, self.z, color, mask);
    }
}

fn hs_rect(hs: &HScroll) -> BoxRect {
    [hs.x, hs.y, hs.w, hs.h]
}

fn draw_text(canvas: &mut Canvas<'_>, fonts: &mut Fonts, it: &Item, x: f32, color: u32, mask: Option<&Mask>) {
    let Kind::Text { s, f, under, strike, asc, .. } = &it.k else { return };
    canvas.text(fonts, *f, s, x, it.y + asc, color, mask);
    let (u, st, thickness) = f.lines();
    for (on, dy) in [(*under, u), (*strike, st)] {
        if on {
            canvas.fill([x, it.y + asc + dy, it.w, thickness], 0.0, color, 1.0, mask);
        }
    }
}

/// Draws page rows into the caller's `w` by `h` premultiplied RGBA tile.
#[expect(clippy::too_many_arguments, reason = "the tile's place, size and scale, each a plain number")]
pub fn render_into(fonts: &mut Fonts, cx: &Ctx, top: f32, h: usize, w: usize, items: &[u32], z: f64, out: &mut [u8]) {
    let Some(pixels) = PixmapMut::from_bytes(out, w as u32, h as u32) else { return };
    let mut canvas = Canvas { pixels, top, z: z as f32 };
    let bg = cx.th.bg;
    canvas.pixels.fill(Color::from_rgba8((bg >> 16) as u8, (bg >> 8) as u8, bg as u8, 255));
    let (page, scale) = (cx.page, cx.scale);
    let bottom = top + h as f32 / canvas.z;
    let mut clips = HashMap::new();
    for &i in items {
        let it = &page.items[i as usize];
        let (dx, mask) = if it.clip > 0 {
            let hs = &page.hscroll[it.clip as usize - 1];
            let off = cx.ov.hoff.get(it.clip as usize - 1).copied().unwrap_or(0.0);
            let mask = clips.entry(it.clip).or_insert_with(|| canvas.mask(hs_rect(hs)));
            (-off, mask.as_ref())
        } else {
            (0.0, None)
        };
        let (x, y, iw, ih) = (it.x + dx, it.y, it.w, it.h);
        match &it.k {
            Kind::Rect { fill, stroke, radius } => {
                canvas.fill([x, y, iw, ih], *radius, *fill, (fill >> 24) as f32 / 255.0, mask);
                if let Some((sw, color)) = stroke {
                    let half = sw / 2.0;
                    if let Some(path) = rounded([x + half, y + half, iw - sw, ih - sw], (*radius - half).max(0.0)) {
                        canvas.stroke(&path, *sw, *color, (color >> 24) as f32 / 255.0, mask);
                    }
                }
            }
            Kind::Text { color, .. } => draw_text(&mut canvas, fonts, it, x, *color, mask),
            Kind::Img(b) => canvas.image(b, [x, y, iw, ih], 1.0, mask),
            Kind::Poly { pts, color, width, fill } => {
                let mut p = PathBuilder::new();
                for (i, &(px, py)) in pts.iter().enumerate() {
                    if i == 0 { p.move_to(x + px, y + py) } else { p.line_to(x + px, y + py) }
                }
                if *fill {
                    p.close();
                }
                if let Some(path) = p.finish() {
                    let alpha = (color >> 24) as f32 / 255.0;
                    if *fill {
                        canvas.pixels.fill_path(&path, &paint(*color, alpha), FillRule::Winding, canvas.transform(), mask);
                    } else {
                        canvas.stroke(&path, *width, *color, alpha, mask);
                    }
                }
            }
        }
    }
    // A search mark replaces the text color within its clipped box, before controls draw over it.
    for &(x, y, mw, mh, current) in &cx.ov.marks {
        if y > bottom || y + mh < top {
            continue;
        }
        let Some(mut mask) = canvas.mask([x, y, mw, mh]) else { continue };
        let mid = y + mh / 2.0;
        if let Some(hs) = page.hscroll.iter().find(|hs| hs.y <= mid && mid < hs.y + hs.h)
            && let Some(path) = rounded(hs_rect(hs), 0.0)
        {
            mask.intersect_path(&path, FillRule::Winding, false, canvas.transform());
        }
        canvas.fill([x, y, mw, mh], 0.0, if current { cx.th.hl_cur_bg } else { cx.th.hl_bg }, 1.0, Some(&mask));
        for &i in items {
            let it = &page.items[i as usize];
            let dx = if it.clip > 0 { -cx.ov.hoff.get(it.clip as usize - 1).copied().unwrap_or(0.0) } else { 0.0 };
            let ix = it.x + dx;
            if !matches!(it.k, Kind::Text { .. }) || it.y > y + mh || it.y + it.h < y || ix > x + mw || ix + it.w < x {
                continue;
            }
            draw_text(&mut canvas, fonts, it, ix, if current { cx.th.hl_cur_fg } else { cx.th.hl_fg }, Some(&mask));
        }
    }
    for (i, hs) in page.hscroll.iter().enumerate() {
        if hs.max <= 0.0 || hs.y > bottom || hs.y + hs.h < top {
            continue;
        }
        let off = cx.ov.hoff.get(i).copied().unwrap_or(0.0);
        let (tx, tw) = hs.thumb(off, scale);
        canvas.fill([tx, hs.bar_y, tw, HScroll::BAR * scale], HScroll::BAR / 2.0 * scale, cx.th.fg_muted, 0.55, None);
    }
    for (i, b) in page.buttons.iter().enumerate() {
        if b.w <= 0.0 || b.y > bottom || b.y + b.h < top {
            continue;
        }
        let (hover, copied) = (cx.ov.hover_btn == Some(i), cx.ov.copied_btn == Some(i));
        draw_button(&mut canvas, cx, b, hover, copied);
        let label = if copied {
            Some("Copied!")
        } else if hover && b.kind == BtnKind::CopyCode {
            Some("Copy code to clipboard")
        } else {
            None
        };
        if let Some(text) = label {
            let f = FontKey::new(12.0 * scale, 0);
            let tw = fonts.measure(f, text);
            let (a, d) = fonts.ascent_descent(f);
            let (px, py) = (4.0 * scale, 3.0 * scale);
            let bx = b.x - 6.0 * scale - tw - 2.0 * px;
            let by = b.y + (b.h - (a + d + 2.0 * py)) / 2.0;
            canvas.fill([bx, by, tw + 2.0 * px, a + d + 2.0 * py], 6.0 * scale, cx.th.tooltip_bg, 1.0, None);
            canvas.text(fonts, f, text, bx + px, by + py + a, 0xffffff, None);
        }
    }
    for &(x, y, sw, sh) in &cx.ov.sel {
        if y <= bottom && y + sh >= top {
            canvas.fill([x, y, sw, sh], 0.0, cx.th.sel_bg, 0.6, None);
        }
    }
    if let Some(e) = &cx.ov.expanded
        && e.y <= bottom
        && e.y + e.h >= top
    {
        let rect = [0.0, e.y, page.w, e.h];
        canvas.fill(rect, 0.0, cx.th.bg, 1.0, None);
        let mask = canvas.mask(rect);
        canvas.image(&e.bmp, [e.x, e.top, e.bmp.w as f32, e.bmp.h as f32], 1.0, mask.as_ref());
    }
}

fn draw_button(canvas: &mut Canvas<'_>, cx: &Ctx, b: &Button, hover: bool, copied: bool) {
    let (s, rect) = (cx.scale, [b.x, b.y, b.w, b.h]);
    let color = match b.kind {
        BtnKind::CopyCode => {
            canvas.fill(rect, 6.0 * s, cx.th.pre_bg, 1.0, None);
            if hover {
                canvas.fill(rect, 6.0 * s, cx.th.rule, 0.6, None);
            }
            if copied { cx.th.copied } else { cx.th.fg_muted }
        }
        BtnKind::Expand | BtnKind::CopyDiagram => {
            canvas.fill(rect, 6.0 * s, cx.th.button_bg, 1.0, None);
            if let Some(path) = rounded([b.x + s / 2.0, b.y + s / 2.0, b.w - s, b.h - s], 5.5 * s) {
                canvas.stroke(&path, s, cx.th.border, 1.0, None);
            }
            if copied { cx.th.copied } else { cx.th.fg_muted }
        }
        _ => cx.th.text,
    };
    let size = 16.0 * s;
    if let Some(img) = icons::bitmap(icons::Icon::of(b.kind, copied), color, (size * canvas.z).round() as u32) {
        canvas.image(&img, [b.x + ((b.w - size) / 2.0).round(), b.y + ((b.h - size) / 2.0).round(), size, size], if b.off { 0.35 } else { 1.0 }, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::raster::Overlay;
    use crate::render::theme::Theme;
    use std::rc::Rc;

    #[test]
    fn rects_images_and_clips_use_y_down_premultiplied_pixels() {
        let mut page = Page { w: 20.0, ..Page::default() };
        page.hscroll.push(HScroll { x: 2.0, y: 2.0, w: 4.0, h: 4.0, max: 0.0, bar_y: 6.0 });
        page.items.push(Item { x: 0.0, y: 0.0, w: 10.0, h: 10.0, clip: 1, k: Kind::Rect { fill: 0xffff0000, stroke: None, radius: 0.0 } });
        page.items.push(Item { x: 12.0, y: 2.0, w: 2.0, h: 2.0, clip: 0, k: Kind::Img(Rc::new(Bitmap { w: 1, h: 2, px: vec![0, 255, 0, 255, 0, 0, 255, 255] })) });
        let th = Theme::new(crate::theme::builtin("github-light").unwrap());
        let cx = Ctx { page: &page, th: &th, scale: 1.0, ov: &Overlay::default(), smooth: true, display_p3: false };
        let mut out = vec![0; 20 * 12 * 4];
        render_into(&mut Fonts::new(), &cx, 0.0, 12, 20, &[0, 1], 1.0, &mut out);
        let pixel = |x: usize, y: usize| &out[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
        assert_eq!(pixel(3, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(12, 2), [0, 255, 0, 255]);
        assert_eq!(pixel(12, 3), [0, 0, 255, 255]);
    }

    #[test]
    fn adjacent_tiles_match_a_whole_render_at_two_output_scales() {
        let mut page = Page { w: 30.0, ..Page::default() };
        page.items.push(Item { x: 4.0, y: 3.0, w: 20.0, h: 20.0, clip: 0, k: Kind::Rect { fill: 0x808000ff, stroke: Some((2.0, 0xff00ff00)), radius: 4.0 } });
        let th = Theme::new(crate::theme::builtin("github-light").unwrap());
        let cx = Ctx { page: &page, th: &th, scale: 1.0, ov: &Overlay::default(), smooth: true, display_p3: false };
        let mut fonts = Fonts::new();
        for z in [1.0, 0.5] {
            let w = (30.0 * z) as usize;
            let h = (12.0 * z) as usize;
            let mut whole = vec![0; w * h * 2 * 4];
            let mut tiles = vec![0; whole.len()];
            render_into(&mut fonts, &cx, 0.0, h * 2, w, &[0], z, &mut whole);
            let (a, b) = tiles.split_at_mut(w * h * 4);
            render_into(&mut fonts, &cx, 0.0, h, w, &[0], z, a);
            render_into(&mut fonts, &cx, 12.0, h, w, &[0], z, b);
            assert_eq!(whole, tiles);
        }
    }
}
