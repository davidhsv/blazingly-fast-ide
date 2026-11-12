//! Find in Files, Replace in Files and Search Everywhere: the models behind the popups and the Find tool
//! window.
//!
//! Nothing here touches the backend, storage or a window. A model that needs a backend answer returns the
//! request as an effect and is handed the answer through a method, so a view can drive it.

pub mod find_tool_window;
pub mod ipc;
pub mod open;
pub mod query;
pub mod replace_flow;
pub mod row_selection;
pub mod scopes_dialog;
pub mod search_keys;
pub mod session;
pub mod store;
pub mod utf16;
pub mod window_store;
