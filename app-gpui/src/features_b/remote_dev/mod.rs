//! Remote development: a window whose project lives on an SSH host, in a WSL distribution or in a
//! dev container. The backend (`openidea-server`) runs there and the window routes its commands
//! to it (`crate::platform_more::remote`).
//!
//! Nothing here calls the backend or touches a window: each model says what to ask and takes the
//! answers, and UI state that outlives a session is read from and written to
//! [`crate::platform::storage::UiStorage`].

pub mod connect_dialog;
pub mod container;
pub mod environment;
pub mod model;
pub mod ports;
pub mod recents;

/// File › Remote Development…: opens the connect dialog.
pub const ACTION_CONNECT: &str = "remote.connect";
/// Disconnect from Remote Host (when a project is open).
pub const ACTION_DISCONNECT: &str = "remote.disconnect";
/// Reopen in Container (when a local project is open).
pub const ACTION_REOPEN_IN_CONTAINER: &str = "remote.reopenInContainer";
/// Reload Environment (when a project is open).
pub const ACTION_RELOAD_ENVIRONMENT: &str = "remote.reloadEnvironment";

/// The id of the Ports tool window, shown while the window is connected to an SSH host or a dev
/// container.
pub const PORTS_TOOL_WINDOW: &str = "remote.ports";
/// The id of the status bar item that names where the project runs.
pub const STATUS_ITEM: &str = "remote.location";
/// The id of the connect dialog's overlay.
pub const CONNECT_OVERLAY: &str = "remote.connect";
