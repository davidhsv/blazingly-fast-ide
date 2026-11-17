//! Live templates, postfix completion, Surround With, Emmet and file templates: the models behind
//! Tab expansion, the template lookups and New ▸ File, without any editor or window attached.

pub mod chooser_view;
pub mod editor;
pub mod editor_context;
pub mod emmet_model;
pub mod expand;
pub mod files_builtins;
pub mod files_create;
pub mod files_model;
pub mod files_store;
pub mod live_builtins;
pub mod live_model;
pub mod live_snippets;
pub mod live_store;
pub mod live_surrounders;
pub mod new_file_dialog;
pub mod popup_store;
pub mod postfix_action;
pub mod postfix_model;
pub mod postfix_store;
pub mod snippet;
pub mod storage;
pub mod update_tag_dialog;

#[cfg(test)]
mod proofs33_chooser_view;
#[cfg(test)]
mod proofs33_expand;
#[cfg(test)]
mod proofs34_new_file_dialog;
#[cfg(test)]
mod proofs34_update_tag_dialog;
#[cfg(test)]
mod proofs39_completion_enter;
#[cfg(test)]
mod proofs39_django_emmet;
#[cfg(test)]
mod proofs39_java_var;
#[cfg(test)]
mod proofs39_zig_live;
#[cfg(test)]
mod proofs39_zig_postfix;
#[cfg(test)]
mod proofs39_zig_surrounders;
