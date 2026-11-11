//! OpenIDEA's desktop shell on GPUI.

pub mod backend;
pub mod editor;
pub mod platform;
pub mod project_tree;
pub mod theme;
pub mod ui;
pub mod workbench;

use gpui_kit::App;

/// Installs what every window needs: the theme, the keymap and shortcut dispatch, and the
/// backend with the app's directories.
pub fn init(cx: &mut App) -> anyhow::Result<()> {
    cx.set_global(theme::ActiveTheme::default());
    platform::input::init(cx);
    editor::actions::register(cx);
    cx.default_global::<platform::i18n::Localization>();
    let dirs = backend::AppDirs::resolve()?;
    backend::Backend::init(dirs.clone(), cx)?;
    cx.set_global(platform::storage::UiStorage::load(&dirs.config));
    workbench::register_actions(cx);
    Ok(())
}
