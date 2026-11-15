//! The visual design: tokens, themes and icons, all read from the webview UI's sources.

pub mod appearance;
pub mod bundled_fonts;
pub mod color_scheme;
pub mod fonts;
pub mod icls;
pub mod icons;
pub mod installed_fonts;
pub mod themes;
pub mod tokens;
pub mod zoom;

use gpui_kit::{AnyElement, App, BoxShadow, Global, Hsla, Pixels, Rgba, img, prelude::*, px, svg};
use tokens::{Theme, ThemeAttributes, Tokens};

/// The theme every window draws with.
pub struct ActiveTheme {
    pub theme: Theme,
}

impl Global for ActiveTheme {}

impl Default for ActiveTheme {
    /// Islands Light, the default theme.
    fn default() -> Self {
        Self { theme: Tokens::builtin().theme(&ThemeAttributes::light()) }
    }
}

impl ActiveTheme {
    pub fn is_dark(&self) -> bool {
        !self.theme.attributes().is_light()
    }
}

/// Switches every window to the theme with `attributes`.
pub fn set_theme(attributes: &ThemeAttributes, cx: &mut App) {
    cx.set_global(ActiveTheme { theme: Tokens::builtin().theme(attributes) });
    cx.refresh_windows();
}

/// The colour of token `name` (without `--`). A token the stylesheet does not define is a bug
/// in the caller: it is logged and drawn in a colour nobody would pick.
pub fn color(cx: &App, name: &str) -> Hsla {
    let theme = cx.try_global::<ActiveTheme>().map(|t| &t.theme);
    match theme.and_then(|t| t.color(name)) {
        Some(c) => Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into(),
        None => {
            tracing::error!("design token --{name} is not a colour");
            Rgba { r: 1.0, g: 0.0, b: 1.0, a: 1.0 }.into()
        }
    }
}

/// The length of token `name` in pixels; 0 (and a log line) when it is not defined.
pub fn length(cx: &App, name: &str) -> Pixels {
    match cx.try_global::<ActiveTheme>().and_then(|t| t.theme.px(name)) {
        Some(value) => px(value),
        None => {
            tracing::error!("design token --{name} is not a length");
            px(0.0)
        }
    }
}

/// The shadow of token `name` (`shadow-dialog`, `shadow-popup`, `shadow-tooltip`, `shadow-balloon`,
/// …) in the active theme, as [`parse_shadow`] reads it; none (and a log line when the value cannot
/// be read) when the theme gives none.
pub fn shadow(cx: &App, name: &str) -> Vec<BoxShadow> {
    let Some(value) = cx.try_global::<ActiveTheme>().and_then(|t| t.theme.resolved(name)) else { return Vec::new() };
    parse_shadow(&value).unwrap_or_else(|| {
        tracing::error!("design token --{name} is not a shadow: {value}");
        Vec::new()
    })
}

/// A CSS `box-shadow` value: one layer per comma-separated part, each
/// `[inset] <x> <y> [<blur> [<spread>]] <colour>` with lengths in pixels. `none` is no shadow;
/// `None` when a layer cannot be read.
pub fn parse_shadow(value: &str) -> Option<Vec<BoxShadow>> {
    let value = value.trim();
    if value.is_empty() || value == "none" {
        return Some(Vec::new());
    }
    tokens::split_top_level(value, ',')
        .into_iter()
        .map(|layer| {
            let mut words = layer.split_whitespace().peekable();
            let inset = words.next_if_eq(&"inset").is_some();
            let mut lengths: Vec<f32> = Vec::new();
            while lengths.len() < 4
                && let Some(length) =
                    words.peek().and_then(|word| word.strip_suffix("px").unwrap_or(word).parse::<f32>().ok())
            {
                lengths.push(length);
                words.next();
            }
            let color = tokens::parse_color(&words.collect::<Vec<_>>().join(" "))?;
            let (x, y) = (*lengths.first()?, *lengths.get(1)?);
            let (blur, spread) = (lengths.get(2).copied().unwrap_or(0.0), lengths.get(3).copied().unwrap_or(0.0));
            let color = Rgba { r: color.r, g: color.g, b: color.b, a: color.a };
            let shadow = BoxShadow::new(px(x), px(y), color.into()).blur_radius(px(blur)).spread_radius(px(spread));
            Some(if inset { shadow.inset() } else { shadow })
        })
        .collect()
}

/// Icon `name` at `size` pixels. A monochrome icon is drawn in `color` (the caller's text
/// colour, so selected and disabled states apply); a coloured one keeps its palette, in the
/// variant of the active theme.
pub fn icon(cx: &App, name: &str, size: f32, color: Hsla) -> AnyElement {
    let dark = cx.try_global::<ActiveTheme>().is_some_and(ActiveTheme::is_dark);
    let asset = icons::icon_asset(name, size, dark);
    if asset.mono {
        // A mask is painted only in an explicit colour.
        svg().path(asset.asset).size(px(size)).flex_none().text_color(color).into_any_element()
    } else {
        img(asset.asset).size(px(size)).flex_none().into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokens::Tokens;

    // A shadow token's layers, in order, with their spread, blur and colour; `none` and an empty
    // value are no shadow; a value that is not a shadow is refused.
    #[test]
    fn reads_the_layers_of_a_shadow_token() {
        let dialog = parse_shadow(
            "0 0 0 0.5px rgba(0, 0, 0, 0.6), 0 10px 24px rgba(0, 0, 0, 0.35), 0 24px 72px rgba(0, 0, 0, 0.45)",
        )
        .expect("the dark dialog shadow");
        let layers: Vec<_> = dialog
            .iter()
            .map(|layer| (f32::from(layer.offset.y), f32::from(layer.blur_radius), f32::from(layer.spread_radius)))
            .collect();
        assert_eq!(layers, [(0.0, 0.0, 0.5), (10.0, 24.0, 0.0), (24.0, 72.0, 0.0)]);
        assert!((dialog[2].color.a - 0.45).abs() < 0.001, "{:?}", dialog[2].color);
        let hex = parse_shadow("0 4px 16px #00000024").expect("a hex colour");
        assert_eq!((f32::from(hex[0].offset.y), f32::from(hex[0].blur_radius)), (4.0, 16.0));
        assert!(!hex[0].inset && parse_shadow("inset 0 0 0 1px #000").expect("an inset shadow")[0].inset);
        assert_eq!(parse_shadow("none"), Some(Vec::new()));
        assert_eq!(parse_shadow(""), Some(Vec::new()));
        for broken in ["12px", "0 rgba(0, 0, 0, 0.4)", "0 4px 12px nothing", "red"] {
            assert_eq!(parse_shadow(broken), None, "{broken}");
        }
    }

    // Every overlay shadow of every built-in theme can be drawn.
    #[test]
    fn every_built_in_theme_has_overlay_shadows_that_can_be_drawn() {
        use tokens::ThemeAttributes;
        let attributes = [
            ThemeAttributes::light(),
            ThemeAttributes::dark(),
            ThemeAttributes::dark_high_contrast(),
            ThemeAttributes::dark().with("data-color-theme", "one-dark"),
            ThemeAttributes::light().with("data-color-theme", "solarized-light"),
        ];
        for attributes in attributes {
            let theme = Tokens::builtin().theme(&attributes);
            for name in ["shadow-dialog", "shadow-popup", "shadow-tooltip", "shadow-stripe-tooltip", "shadow-balloon"] {
                let value = theme.resolved(name).unwrap_or_else(|| panic!("--{name} in {attributes:?}"));
                let layers = parse_shadow(&value).unwrap_or_else(|| panic!("--{name} = {value} in {attributes:?}"));
                assert!(!layers.is_empty(), "--{name} in {attributes:?}");
            }
        }
    }
}
