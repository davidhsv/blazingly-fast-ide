//! Code intelligence in the editor: what the editor does with the answers of the language servers.
//!
//! The backend talks to the servers; these models turn their answers into completion lists,
//! edits, hints, lenses and navigation targets. They call nothing: requests go out as values the
//! caller carries out, and answers come back as arguments, tagged so that an answer to an older
//! request is dropped.

pub mod availability;
pub mod code_lens;
pub mod completion;
pub mod completion_items;
pub mod completion_view;
pub mod configuration_navigation;
pub mod definition_view;
pub mod diagnostics;
pub mod file_references;
pub mod files;
pub mod format;
pub mod format_guard;
pub mod generate;
pub mod hint;
pub mod hover;
pub mod hover_view;
pub mod imports_on_paste;
pub mod inlay;
pub mod inline_completion;
pub mod inplace_rename;
pub mod intentions;
pub mod intentions_view;
pub mod links;
pub mod lsp;
pub mod markdown;
pub mod navigation;
pub mod outside_project;
pub mod positions;
pub mod reformat_dialog;
pub mod rename;
pub mod semantic;
pub mod signature;
pub mod signature_view;
pub mod snippet;
pub mod spelling;
pub mod syntax_errors;
pub mod targets_view;
pub mod type_info;
pub mod workspace_edit;
