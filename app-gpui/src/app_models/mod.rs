//! The application shell's models: where projects and files open, what must be settled before a
//! window, a project or the app closes, update checks, the command-line launcher, New Project,
//! the workspace dialog, Invalidate Caches, importing JetBrains settings, About and the Keymap
//! Reference.
//!
//! Nothing here touches the backend or a window. A model that needs something done returns it as
//! a step or an effect; what the backend or the user answered, ids and time are passed in.

pub mod cli_command;
pub mod close_guard;
pub mod console_copy;
pub mod edit_command;
pub mod invalidate_caches;
pub mod jetbrains_settings;
pub mod project_actions;
pub mod shell;
pub mod updates;
pub mod windows;
pub mod workspace_dialog;
