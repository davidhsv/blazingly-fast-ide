//! OpenIDEA's desktop shell on GPUI.

pub mod agents;
pub mod app_models;
pub mod backend;
pub mod build_window;
pub mod code_insight;
pub mod database;
pub mod debugger;
pub mod editor;
pub mod editor_commands;
pub mod editor_lsp;
pub mod editor_model;
pub mod editor_refactor;
pub mod everywhere;
pub mod features_a;
pub mod features_b;
pub mod find;
pub mod github;
pub mod platform;
pub mod platform_more;
pub mod plugin_models;
pub mod plugins;
pub mod project_tree;
pub mod project_tree_model;
pub mod project_tree_paths;
pub mod project_view;
pub mod run;
pub mod run_producers;
pub mod run_testing;
pub mod self_check;
pub mod services;
pub mod settings_model;
pub mod settings_view;
pub mod templates;
pub mod terminal;
pub mod theme;
pub mod ui;
pub mod vcs;
pub mod vcs_diff;
pub mod vcs_more;
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
    workbench::apply_keymap_settings(cx);
    plugins::Plugins::init(cx)?;
    workbench::register_actions(cx);
    workbench::menu::init(cx);
    Ok(())
}
