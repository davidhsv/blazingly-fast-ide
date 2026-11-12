//! GitHub pull requests (after IntelliJ's GitHub plugin): the list and its search, a pull request's
//! details and timeline, review in the diff, and New Pull Request, as models without GPUI types.
//! GitHub is reached through gh: the backend's `gh_api` / `gh_availability` / `gh_pr_*` commands
//! (`ide-github`), whose requests [`api`] builds and whose answers [`types`] reads.

pub mod api;
pub mod batches;
pub mod clone;
pub mod create;
pub mod data_model;
pub mod details;
pub mod details_panel;
pub mod details_view;
pub mod diff;
pub mod graphql;
pub mod gutter;
pub mod list;
pub mod list_panel;
pub mod new_comments;
pub mod repository;
pub mod repository_data;
pub mod review_diff;
pub mod review_editor;
pub mod review_mode;
pub mod search_history;
pub mod time;
pub mod timeline;
pub mod toolwindow;
pub mod types;
pub mod ui_model;
pub mod window_view;
