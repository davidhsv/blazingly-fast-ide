//! OpenIDEA's desktop shell on GPUI.

pub mod backend;
pub mod editor;
pub mod platform;
pub mod theme;
pub mod workbench;

use gpui_kit::App;

/// Installs what every window needs: the theme, the keymap and shortcut dispatch, and the
/// backend with the app's directories.
pub fn init(cx: &mut App) -> anyhow::Result<()> {
    cx.set_global(theme::ActiveTheme::default());
    platform::input::init(cx);
    cx.default_global::<platform::i18n::Localization>();
    backend::Backend::init(backend::AppDirs::resolve()?, cx)
}
