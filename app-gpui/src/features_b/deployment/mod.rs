//! Deployment: upload, download and synchronise project files with the servers of Settings ›
//! Build, Execution, Deployment › Deployment, the Sync with Deployed comparison (its own window on
//! desktop) and the Remote Host tool window.
//!
//! Nothing here calls the backend: every flow returns the request to send and takes the answer,
//! with the request id (or a ticket) that tells a current answer from a stale one. Transfers,
//! automatic upload and remote deletes run in the backend (`ide-remote`).

pub mod actions;
pub mod comparison;
pub mod model;
pub mod preview;
pub mod remote_host;
pub mod store;
pub mod sync_view;
pub mod types;
pub mod window;

/// The Remote Host tool window's id.
pub const REMOTE_HOST_WINDOW: &str = "remoteHost";
