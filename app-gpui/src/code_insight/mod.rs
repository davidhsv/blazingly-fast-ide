//! Code insight: the Problems tool window, Find / Show Usages, the call and type hierarchies, and
//! the language servers' status.
//!
//! The models call nothing: what needs the backend goes out as a request value the caller carries
//! out, and the answer comes back as an argument tagged with the ticket of its request, so that an
//! answer to a request that is no longer current is dropped. The views read the models and report
//! what the user asked for through events.

pub mod hierarchy;
pub mod hierarchy_view;
pub mod problems;
pub mod problems_report;
pub mod problems_view;
pub mod server_status;
pub mod servers;
pub mod syntax_usages;
pub mod usage_search;
pub mod usages;
pub mod usages_popup;
pub mod workspace_undo;

/// The Problems tool window's id.
pub const PROBLEMS_TOOL_WINDOW: &str = "problems";
/// The Find Usages tool window's id.
pub const USAGES_TOOL_WINDOW: &str = "usages";
/// The Hierarchy tool window's id.
pub const HIERARCHY_TOOL_WINDOW: &str = "hierarchy";
/// The status bar item listing the language servers.
pub const SERVERS_STATUS_ITEM: &str = "code-insight.servers";
