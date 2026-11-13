//! The UI side of plugin contributions as plain models: what the plugin host lists and announces,
//! turned into what windows show (translated declarations, icon themes, gutter markers, decorations,
//! run targets, web frame terminals …). Nothing here calls the host or draws: each model takes the
//! host's answers and returns what to ask or do next.

pub mod active_file;
pub mod consent;
pub mod contribution;
pub mod decorations;
pub mod icon_theme_resolver;
pub mod icon_themes;
pub mod indexed_markers;
pub mod language_visuals;
pub mod line_markers;
pub mod marker_icons;
pub mod marker_store;
pub mod navigation;
pub mod plugin_icon;
pub mod plugin_text;
pub mod plugins_page;
pub mod pty_bridge;
pub mod run_targets;
pub mod runtime_ui;
pub mod startup_theme;
pub mod store;
pub mod view_helpers;
pub mod web_frames;
pub mod workspace_contains;

#[cfg(test)]
mod test_support;
