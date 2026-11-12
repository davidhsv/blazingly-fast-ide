//! The models behind the Settings dialog: the page tree and its search, staged edits and Apply,
//! and what each page computes from the settings (the keymap editor, inspections, file types,
//! deployment, run targets, SSH, the MCP server, appearance), without any window attached. The
//! settings themselves are the backend's `ide_core::settings::Settings`.

pub mod appearance_runtime;
pub mod assign_shortcut;
pub mod code_style_scheme;
pub mod contrast;
pub mod deployment_model;
pub mod file_types;
pub mod inspections_model;
pub mod keymap_groups;
pub mod keymap_model;
pub mod keymap_page;
pub mod language_pages;
pub mod mcp_server;
pub mod page_spec;
pub mod pages;
pub mod run_targets;
pub mod runtime;
pub mod schema;
pub mod settings_window;
pub mod ssh_connection;
pub mod staging;
pub mod window_bridge;
