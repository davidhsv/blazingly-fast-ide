//! Live templates, postfix completion, Surround With, Emmet and file templates: the models behind
//! Tab expansion, the template lookups and New ▸ File, without any editor or window attached.

pub mod editor;
pub mod editor_context;
pub mod emmet_model;
pub mod files_builtins;
pub mod files_create;
pub mod files_model;
pub mod live_builtins;
pub mod live_model;
pub mod live_snippets;
pub mod live_store;
pub mod live_surrounders;
pub mod popup_store;
pub mod postfix_model;
pub mod postfix_store;
pub mod snippet;
pub mod storage;
