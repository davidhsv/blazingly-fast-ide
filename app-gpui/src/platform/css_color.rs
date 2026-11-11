//! Design tokens as concrete colours. Themes may write tokens as `hsl()`, `color-mix()` or
//! `var()`; consumers outside the style system (the native window, Mermaid, the terminal) need
//! RGB, whatever syntax the theme used.

use crate::theme::tokens::{self, Color, Theme};

/// A component of `color(srgb …)`: digits and dots only, clamped to the `0..=1` a [`Color`] holds.
fn unit_component(text: &str) -> Option<f32> {
    let plain = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    if plain { text.parse::<f32>().ok().map(|n| n.clamp(0.0, 1.0)) } else { None }
}

/// `color(srgb r g b)` or `color(srgb r g b / a)`, already in lower case.
fn parse_srgb(value: &str) -> Option<Color> {
    let args = value.strip_prefix("color(srgb")?.strip_suffix(')')?;
    let (channels, alpha) = match args.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha.trim())),
        None => (args, None),
    };
    if !channels.starts_with(char::is_whitespace) {
        return None;
    }
    let mut components = channels.split_whitespace().map(unit_component);
    let (r, g, b) = (components.next()??, components.next()??, components.next()??);
    if components.next().is_some() {
        return None;
    }
    let a = match alpha {
        Some(alpha) => unit_component(alpha)?,
        None => 1.0,
    };
    Some(Color { r, g, b, a })
}

/// Parses `#rgb[a]`, `#rrggbb[aa]`, `rgb[a](…)` and `color(srgb …)`, in any letter case; `None`
/// for every other syntax (`hsl()`, `color-mix()`, names), which only a theme can resolve: see
/// [`resolve_token_color`].
pub fn parse_color(input: &str) -> Option<Color> {
    let value = input.trim().to_ascii_lowercase();
    if value.starts_with('#') || value.starts_with("rgb") {
        return tokens::parse_color(&value);
    }
    parse_srgb(&value)
}

/// The colour of token `name` (`--bg-app`) in `theme`, whatever syntax the theme wrote it in.
pub fn resolve_token_color(name: &str, theme: &Theme) -> Option<Color> {
    let value = theme.resolved(name.strip_prefix("--").unwrap_or(name))?;
    parse_color(&value).or_else(|| tokens::parse_color(&value))
}

/// `rgb(r, g, b)`, or `rgba(r, g, b, a)` when translucent, the alpha with at most three decimals.
pub fn to_rgb(color: Color) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = (byte(color.r), byte(color.g), byte(color.b));
    if color.a < 1.0 {
        let alpha = (f64::from(color.a) * 1000.0).round() / 1000.0;
        format!("rgba({r}, {g}, {b}, {alpha})")
    } else {
        format!("rgb({r}, {g}, {b})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tokens::{ThemeAttributes, Tokens};

    // src/platform/cssColor.test.ts › parses hex, rgb() and color(srgb) and formats them back
    #[test]
    fn parses_hex_rgb_and_color_srgb_and_formats_them_back() {
        assert_eq!(parse_color("#eee8d5"), Some(Color::rgb(238, 232, 213)));
        assert_eq!(parse_color("#fff8"), Some(Color { a: 136.0 / 255.0, ..Color::rgb(255, 255, 255) }));
        assert_eq!(parse_color("rgb(33, 37, 43)"), Some(Color::rgb(33, 37, 43)));
        assert_eq!(parse_color("rgba(0 0 0 / 50%)"), Some(Color { a: 0.5, ..Color::rgb(0, 0, 0) }));
        assert_eq!(parse_color("color(srgb 0.5 0.25 0.75 / 0.8)"), Some(Color { r: 0.5, g: 0.25, b: 0.75, a: 0.8 }));
        assert_eq!(parse_color("hsl(220, 13%, 15%)"), None);
        assert_eq!(Color::rgb(33, 37, 43).to_hex(), "#21252b");
        assert_eq!(Color { r: 0.5, g: 0.25, b: 0.75, a: 0.8 }.to_hex(), "#8040bfcc");
        assert_eq!(to_rgb(Color::rgb(33, 37, 43)), "rgb(33, 37, 43)");
        assert_eq!(to_rgb(Color { a: 0.5, ..Color::rgb(0, 0, 0) }), "rgba(0, 0, 0, 0.5)");
    }

    // src/platform/cssColor.test.ts › resolves tokens written as hsl(), color-mix() or var() through the browser
    #[test]
    fn resolves_tokens_written_as_hsl() {
        let stylesheet = Tokens::parse(":root { --hex: #073642; --hsl: hsl(220, 13%, 15%); }");
        let theme = stylesheet.theme(&ThemeAttributes::dark());
        assert_eq!(resolve_token_color("--hex", &theme), Some(Color::rgb(7, 54, 66)));
        assert_eq!(resolve_token_color("--hsl", &theme).map(Color::to_hex).as_deref(), Some("#21252b"));
        assert_eq!(resolve_token_color("--none", &theme), None);
    }
}
