//! Version control operations and commit logic: what the Git actions ask, run and report.
//!
//! These modules hold no view and run no git command. An operation is a function from the state it
//! reads to what to do next: the git command to run, the question to ask ([`dialogs`]), the
//! notification to raise ([`ops`]) or the menu to show ([`menu`]). Where an operation continues
//! after a command's result or an answer, it is split into steps that take that result.

pub mod actions;
pub mod branch_menu;
pub mod branch_ops;
pub mod branch_widget;
pub mod branch_widget_extensions;
pub mod change_ops;
pub mod changelists;
pub mod changelists_ui;
pub mod clone;
pub mod commit_checks;
pub mod commit_selection;
pub mod commit_store;
pub mod commit_ui;
pub mod commit_view;
pub mod compare_paths;
pub mod confirmations;
pub mod dialogs;
pub mod git_log;
pub mod git_tabs;
pub mod ignore;
pub mod log_pane_actions;
pub mod log_store;
pub mod menu;
pub mod operations;
pub mod ops;
pub mod patches;
pub mod rebase_plan;
pub mod remote_ui;
pub mod store;
#[cfg(test)]
mod test_support;
pub mod unity_metadata;
