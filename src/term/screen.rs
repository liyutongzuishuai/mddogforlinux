//! The screen the terminal is on.

/// The first page's device pixels per CSS pixel before display hints have been read.
#[cfg(target_os = "macos")]
pub const SCALE_GUESS: f64 = 2.0;
#[cfg(not(target_os = "macos"))]
pub const SCALE_GUESS: f64 = 1.0;

static SCALE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

fn positive_scale(value: Option<&str>) -> Option<f64> {
    value?.trim().parse::<f64>().ok().filter(|s| s.is_finite() && *s > 0.0)
}

/// Linux terminals don't expose their focused output's scale to a child process. Environment hints are
/// startup-only: MDDOG_SCALE, then GTK's integer GDK_SCALE, then WINIT_X11_SCALE_FACTOR, else 1.
#[cfg(any(test, not(target_os = "macos")))]
fn scale_from_hints(mddog: Option<&str>, gdk: Option<&str>, winit: Option<&str>) -> f64 {
    positive_scale(mddog).or_else(|| gdk.and_then(|s| s.trim().parse::<u32>().ok()).filter(|&s| s > 0).map(f64::from)).or_else(|| positive_scale(winit)).unwrap_or(1.0)
}

/// The display scale at startup. MDDOG_SCALE overrides both platforms. macOS reads CoreGraphics;
/// Linux uses environment hints without opening a display connection or asking a compositor.
pub fn display_scale() -> f64 {
    *SCALE.get_or_init(|| {
        let mddog = std::env::var("MDDOG_SCALE").ok();
        #[cfg(target_os = "macos")]
        {
            positive_scale(mddog.as_deref()).unwrap_or_else(system_scale)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let gdk = std::env::var("GDK_SCALE").ok();
            let winit = std::env::var("WINIT_X11_SCALE_FACTOR").ok();
            scale_from_hints(mddog.as_deref(), gdk.as_deref(), winit.as_deref())
        }
    })
}

#[cfg(target_os = "macos")]
fn system_scale() -> f64 {
    use core_graphics::display::CGDisplay;
    let scale = |id| CGDisplay::new(id).display_mode().filter(|m| m.width() > 0).map(|m| m.pixel_width() as f64 / m.width() as f64);
    let scales: Vec<f64> = CGDisplay::active_displays().unwrap_or_default().into_iter().filter_map(scale).collect();
    if let Some(&first) = scales.first()
        && scales.iter().all(|&s| s == first)
    {
        return first;
    }
    // Screens of mixed scales: the front window is the terminal mddog was just typed into.
    front_window_center()
        .and_then(|p| CGDisplay::displays_with_point(p, 1).ok())
        .and_then(|(ids, n)| if n > 0 { ids.first().copied() } else { None })
        .or(Some(CGDisplay::main().id))
        .and_then(scale)
        .unwrap_or(SCALE_GUESS)
}

/// The centre of the frontmost ordinary window, in global display coordinates. Bounds and layer need no
/// screen-recording permission.
#[cfg(target_os = "macos")]
fn front_window_center() -> Option<core_graphics::geometry::CGPoint> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly};
    type Dict = CFDictionary<CFString, CFType>;
    let dict = |v: &CFType| -> Option<Dict> {
        // SAFETY: the type ID check makes it a CFDictionary; the get rule retains it for the wrapper.
        (v.type_of() == Dict::type_id()).then(|| unsafe { Dict::wrap_under_get_rule(v.as_CFTypeRef().cast()) })
    };
    let num = |d: &Dict, k: &str| d.find(CFString::new(k)).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_f64());
    let list = copy_window_info(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID)?;
    list.iter().find_map(|item| {
        // SAFETY: the window list holds CFDictionaries, alive while `list` is.
        let w = unsafe { Dict::wrap_under_get_rule((*item).cast()) };
        if num(&w, "kCGWindowLayer") != Some(0.0) {
            return None;
        }
        let b = dict(&*w.find(CFString::new("kCGWindowBounds"))?)?;
        Some(core_graphics::geometry::CGPoint::new(num(&b, "X")? + num(&b, "Width")? / 2.0, num(&b, "Y")? + num(&b, "Height")? / 2.0))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_override_must_be_positive_and_finite() {
        assert_eq!(positive_scale(Some(" 1.25 ")), Some(1.25));
        for value in [None, Some(""), Some("0"), Some("-2"), Some("NaN"), Some("inf"), Some("1e999"), Some("auto")] {
            assert_eq!(positive_scale(value), None, "{value:?}");
        }
    }

    #[test]
    fn linux_scale_hints_have_a_fixed_precedence() {
        assert_eq!(scale_from_hints(Some("1.25"), Some("2"), Some("3")), 1.25);
        assert_eq!(scale_from_hints(Some("inf"), Some("2"), Some("1.5")), 2.0);
        assert_eq!(scale_from_hints(None, Some("1.5"), Some("1.75")), 1.75);
        assert_eq!(scale_from_hints(None, Some("0"), Some("2")), 2.0);
        assert_eq!(scale_from_hints(None, None, Some("auto")), 1.0);
        assert_eq!(scale_from_hints(Some("NaN"), Some("-1"), Some("inf")), 1.0);
        assert_eq!(scale_from_hints(None, None, None), 1.0);
    }
}
