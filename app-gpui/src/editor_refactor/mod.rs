//! The editor's refactorings, navigation and structure logic: Safe Delete, Refactor This and the
//! Refactor menu actions, the server-side Introduce interaction, Go to Test, Next / Previous and
//! Super Method, contributed navigation, and the language-server-first fallback to the syntax
//! tree.
//!
//! Nothing here calls the backend or knows about views: flows return requests tagged with a
//! ticket for the caller to carry out, take the answers back as arguments, and drop an answer
//! whose ticket they no longer wait for. Offsets are UTF-16 code units and lines 0-based.

pub mod catalog;
pub mod goto_test;
pub mod names;
pub mod navigation_providers;
pub mod occurrences;
pub mod preview;
pub mod project_level;
pub mod refactor_this;
pub mod run_kind;
pub mod safe_delete;
pub mod server_introduce;
pub mod structure_fallback;
pub mod structure_nav;
pub mod templates;
pub mod test_naming;
pub mod text;

#[cfg(test)]
pub(crate) mod testing;
