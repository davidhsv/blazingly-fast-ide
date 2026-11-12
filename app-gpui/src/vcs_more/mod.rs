//! More of version control: the three-way merge and the merge editor's model, loading and
//! navigating the Git log of several repositories, what Show Diff opens and where, pushes that are
//! retargeted or rejected, and the smaller models around them.
//!
//! Like [`crate::vcs_ops`], these modules hold no view and run no git command: where an operation
//! needs the backend, the editor or a dialog, it is split into steps that say what to ask and take
//! the answer.

pub mod diff_blame;
pub mod issue_links;
pub mod log_branch_memory;
pub mod log_changes;
pub mod log_filters;
pub mod log_loading;
pub mod log_presentation;
pub mod log_search;
pub mod merge_actions;
pub mod merge_diff3;
pub mod merge_model;
pub mod merge_scroll;
pub mod multi_repo;
pub mod multi_root;
pub mod new_tag;
pub mod patch_file_manager;
pub mod project_switch;
pub mod push_rejected;
pub mod push_target;
pub mod recent_changes;
pub mod remote_results;
pub mod repository_paths;
pub mod rollback;
pub mod show_diff;
pub mod viewer_look;
pub mod workflow;
pub mod worktrees;
