//! The debugger: breakpoints of the open project, debug sessions fed by the backend's debug
//! events, the execution point with its inline values, and what each debugger action does.
//!
//! Everything here is a model: state plus transitions that return the [`effects::Effect`]s to carry
//! out. The views and the backend glue live with the workbench.

pub mod actions;
pub mod attach;
pub mod breakpoints;
pub mod editor;
pub mod effects;
pub mod focused_view;
pub mod frames;
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
