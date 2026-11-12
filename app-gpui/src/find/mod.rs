//! Find in Files and Replace in Files: the models behind the popup and the Find tool window.
//!
//! Nothing here touches the backend, storage or a window. A model that needs a backend answer returns the
//! request as an effect and is handed the answer through a method, so a view can drive it.

pub mod backend;
pub mod find_tool_window;
pub mod ipc;
pub mod popup_view;
pub mod query;
pub mod replace_flow;
pub mod row_selection;
pub mod scopes_dialog;
pub mod search_keys;
pub mod session;
pub mod store;
pub mod utf16;
pub mod view;
pub mod window_store;
