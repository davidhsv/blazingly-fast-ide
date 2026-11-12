//! The visual design: tokens, themes and icons, all read from the webview UI's sources.

pub mod color_scheme;
pub mod fonts;
pub mod icls;
pub mod icons;
pub mod themes;
pub mod tokens;

use gpui_kit::{AnyElement, App, Global, Hsla, Pixels, Rgba, img, prelude::*, px, svg};
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
