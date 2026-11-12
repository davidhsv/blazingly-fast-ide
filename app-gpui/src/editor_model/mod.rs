//! The editor's document, tab and file-synchronization logic, free of any view: who may open
//! and close tabs and when, the lifecycle of documents owned by custom editors, and the
//! decisions the workbench carries out.

pub mod controller;
pub mod detach;
pub mod disk_sync;
pub mod document;
pub mod documents;
pub mod editor_prefs;
pub mod file_attributes;
pub mod guard;
pub mod nav_bar;
pub mod python_members;
pub mod removal;
pub mod semantic_php;
pub mod semantic_tokens;
pub mod virtual_input;
pub mod workspace_history;
