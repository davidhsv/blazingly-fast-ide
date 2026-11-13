//! The Project view's operations as plain data: its state beyond what the tree shows (panes,
//! selection, inline editing, clipboard, contributed roots), the file operations behind its
//! actions, and its menus.

pub mod actions;
pub mod clipboard;
pub mod context_menu;
pub mod copy_path_popup;
pub mod file_dialogs;
pub mod file_path;
pub mod file_refactor;
pub mod fs;
pub mod index;
pub mod mark_directory;
pub mod module_groups;
pub mod new_popup;
pub mod new_popup_model;
pub mod operations;
pub mod store;
