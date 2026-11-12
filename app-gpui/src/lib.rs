//! OpenIDEA's desktop shell on GPUI.

pub mod backend;
pub mod debugger;
pub mod editor;
pub mod everywhere;
pub mod find;
pub mod platform;
pub mod plugins;
pub mod project_tree;
pub mod project_tree_model;
pub mod project_tree_paths;
pub mod project_view;
pub mod run;
pub mod self_check;
pub mod settings_model;
pub mod templates;
pub mod terminal;
pub mod theme;
pub mod ui;
pub mod vcs;
pub mod vcs_diff;
pub mod vcs_ops;
pub mod web_view;
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
    theme::appearance::init(cx);
    plugins::Plugins::init(cx)?;
    workbench::register_actions(cx);
    workbench::menu::init(cx);
    Ok(())
}
