//! Image viewer zoom: IDEA-like steps that always pass through 100%, fit to window, and zooming
//! about the point under the cursor. Mermaid drawings in the Markdown preview pan and zoom with
//! the same [`View`].

pub const MIN_ZOOM: f64 = 1.0 / 32.0;
pub const MAX_ZOOM: f64 = 64.0;
const STEP: f64 = 1.25;

pub fn clamp_zoom(zoom: f64) -> f64 {
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// The next zoom step in direction `dir` (`1` in, `-1` out); snaps to 100% when crossing it, so
/// the actual size is always reachable.
pub fn step_zoom(zoom: f64, dir: i8) -> f64 {
    let next = if dir > 0 { zoom * STEP } else { zoom / STEP };
    if (zoom < 1.0 && next > 1.0) || (zoom > 1.0 && next < 1.0) {
        return 1.0;
    }
    clamp_zoom(next)
}

/// The largest zoom showing the whole `width × height` image inside `avail_w × avail_h`.
pub fn fit_zoom(width: f64, height: f64, avail_w: f64, avail_h: f64) -> f64 {
    if width <= 0.0 || height <= 0.0 || avail_w <= 0.0 || avail_h <= 0.0 {
        return 1.0;
    }
    clamp_zoom((avail_w / width).min(avail_h / height))
}

/// The zoom an image opens at: actual size, unless it does not fit.
pub fn initial_zoom(width: f64, height: f64, avail_w: f64, avail_h: f64) -> f64 {
    fit_zoom(width, height, avail_w, avail_h).min(1.0)
}

pub fn zoom_label(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0 + 0.5).floor())
}

/// Pan and zoom: the content's top-left offset from the canvas (px) and its scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub z: f64,
    pub x: f64,
    pub y: f64,
}

impl View {
    pub const IDENTITY: View = View { z: 1.0, x: 0.0, y: 0.0 };
}

/// Zooms to `z` keeping the content point under canvas position (`px`, `py`) where it is.
pub fn zoom_at(view: View, z: f64, px: f64, py: f64) -> View {
    let k = z / view.z;
    View { z, x: px - (px - view.x) * k, y: py - (py - view.y) * k }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_through_100() {
        assert_eq!(step_zoom(1.0, 1), 1.25);
        assert_eq!(step_zoom(0.9, 1), 1.0);
        assert_eq!(step_zoom(1.1, -1), 1.0);
        assert_eq!(step_zoom(64.0, 1), 64.0);
    }

    #[test]
    fn fits_and_starts_at_actual_size_when_possible() {
        assert_eq!(fit_zoom(200.0, 100.0, 100.0, 100.0), 0.5);
        assert_eq!(initial_zoom(50.0, 50.0, 100.0, 100.0), 1.0);
        assert_eq!(initial_zoom(400.0, 100.0, 100.0, 100.0), 0.25);
        assert_eq!(zoom_label(0.333), "33%");
    }

    #[test]
    fn keeps_the_point_under_the_cursor_fixed_when_zooming() {
        let next = zoom_at(View { z: 1.0, x: 10.0, y: 20.0 }, 2.0, 110.0, 70.0);
        // The content point under (110, 70) before: ((110 - 10) / 1, (70 - 20) / 1) = (100, 50).
        assert_eq!(next, View { z: 2.0, x: -90.0, y: -30.0 });
        assert_eq!(next.x + 100.0 * next.z, 110.0);
        assert_eq!(next.y + 50.0 * next.z, 70.0);
    }
}
