//! The debugger: breakpoints of the open project, debug sessions fed by the backend's debug
//! events, the execution point with its inline values, and what each debugger action does.
//!
//! The models are state plus transitions that return the [`effects::Effect`]s to carry out; the
//! Debug tool window's views (`frames_view`, `variables_view`, `window_view`) draw them. The backend
//! glue lives with the workbench.

pub mod actions;
pub mod attach;
pub mod breakpoints;
pub mod breakpoints_view;
pub mod driver;
pub mod editor;
pub mod effects;
pub mod focused_view;
pub mod frames;
pub mod frames_view;
pub mod hotswap;
pub mod java;
pub mod launch;
pub mod missing_adapter;
pub mod overlays;
pub mod remote;
pub mod start_failure;
pub mod state;
pub mod taskbar_flash;
pub mod value_changes;
pub mod value_text;
pub mod values;
pub mod variables_view;
pub mod window_view;
