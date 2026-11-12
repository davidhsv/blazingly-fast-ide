//! Vim mode, modelled on IdeaVim: the on/off setting and what it switches across the app, the
//! shortcut conflicts the keymap dispatcher respects, the `~/.ideavimrc` subset, the IDE's Ex
//! commands and rc mappings on top of a Vim engine, and Normal-mode IME compositions.

pub mod conflicts;
pub mod engine;
pub mod ime;
pub mod mode;
pub mod rc;
pub mod settings;
