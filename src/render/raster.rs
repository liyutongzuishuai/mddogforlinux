//! Draws display-list items and overlays into premultiplied RGBA tiles.
use crate::render::page::{Bitmap, Page};
use crate::render::theme::Theme;
use std::rc::Rc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
pub use linux::render_into;
#[cfg(target_os = "macos")]
pub use macos::{cg_image, render_into};

/// Things drawn over the page that are not part of the display list.
#[derive(Default, Clone)]
pub struct Overlay {
    /// Highlight rects (page coordinates): (x, y, w, h, current).
    pub marks: Vec<(f32, f32, f32, f32, bool)>,
    pub sel: Vec<(f32, f32, f32, f32)>,
    /// Horizontal scroll offsets per hscroll region.
    pub hoff: Vec<f32>,
    /// Hovered button index and the one that shows "Copied!".
    pub hover_btn: Option<usize>,
    pub copied_btn: Option<usize>,
    pub expanded: Option<Expanded>,
}

/// An expanded diagram over the view, in device pixels.
#[derive(Clone)]
pub struct Expanded {
    pub y: f32,
    pub h: f32,
    pub bmp: Rc<Bitmap>,
    pub x: f32,
    pub top: f32,
}

pub struct Ctx<'a> {
    pub page: &'a Page,
    pub th: &'a Theme,
    pub scale: f32,
    pub ov: &'a Overlay,
    /// CoreGraphics font smoothing; Linux uses grayscale glyph antialiasing.
    pub smooth: bool,
    /// Convert sRGB to Display P3 for macOS terminals that require it.
    pub display_p3: bool,
}
