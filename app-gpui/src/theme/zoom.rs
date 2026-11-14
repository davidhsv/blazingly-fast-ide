//! The IDE zoom (Settings › Appearance › Zoom, View › Appearance › Zoom IDE): every length of
//! the theme, text sizes, row heights, paddings and radii, scaled by the zoom percentage.
//!
//! A length is a token whose value is one number of pixels (`13px`); colours, font stacks,
//! shadows, unitless numbers (weights, line heights) and lists of lengths stay as they are.

use super::tokens::Theme;
use crate::settings_model::appearance_runtime::{ZOOM_MAX, ZOOM_MIN};

/// The scale factor of a zoom of `percent`, kept within the zoom's range.
pub fn zoom_factor(percent: u32) -> f32 {
    percent.clamp(ZOOM_MIN, ZOOM_MAX) as f32 / 100.0
}

/// The pixels of `value` when it is a single length in pixels.
fn pixels(value: &str) -> Option<f32> {
    let number = value.trim().strip_suffix("px")?.trim();
    number.parse::<f32>().ok().filter(|pixels| pixels.is_finite())
}

/// Scales every length of `theme` by a zoom of `percent`; 100 leaves the theme as it is.
pub fn apply_zoom(theme: &mut Theme, percent: u32) {
    if percent.clamp(ZOOM_MIN, ZOOM_MAX) == 100 {
        return;
    }
    let factor = zoom_factor(percent);
    let lengths: Vec<(String, f32)> = theme
        .names()
        .filter_map(|name| theme.resolved(name).and_then(|value| pixels(&value)).map(|px| (name.to_owned(), px)))
        .collect();
    for (name, value) in lengths {
        theme.set(&name, &format!("{}px", value * factor));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tokens::{ThemeAttributes, Tokens};

    fn theme() -> Theme {
        Tokens::parse(
            ":root { --fs-ui: 13px; --h-row: var(--row, 24px); --fg: #102030; --fw-code: 400; \
             --shadow: 0 1px 2px #000; --font-ui: system-ui; }",
        )
        .theme(&ThemeAttributes::light())
    }

    // Not from a spec: a zoom scales the lengths, also those given through var(), and leaves
    // colours, numbers, shadows and fonts as they are; the default zoom changes nothing.
    #[test]
    fn a_zoom_scales_every_length_of_the_theme_and_nothing_else() {
        let mut zoomed = theme();
        apply_zoom(&mut zoomed, 110);
        assert!((zoomed.px("fs-ui").expect("a length") - 14.3).abs() < 1e-4);
        assert!((zoomed.px("h-row").expect("a length") - 26.4).abs() < 1e-4);
        assert_eq!(zoomed.resolved("fg").as_deref(), Some("#102030"));
        assert_eq!(zoomed.number("fw-code"), Some(400.0));
        assert_eq!(zoomed.resolved("shadow").as_deref(), Some("0 1px 2px #000"));
        assert_eq!(zoomed.resolved("font-ui").as_deref(), Some("system-ui"));

        let mut unchanged = theme();
        apply_zoom(&mut unchanged, 100);
        assert_eq!(unchanged.px("fs-ui"), Some(13.0));
        let mut clamped = theme();
        apply_zoom(&mut clamped, 1000);
        assert_eq!(clamped.px("fs-ui"), Some(39.0), "a zoom beyond the range is the largest one");
    }
}
