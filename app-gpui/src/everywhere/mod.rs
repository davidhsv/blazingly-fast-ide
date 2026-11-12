//! Search Everywhere, Recent Files and Locations, Bookmarks and the Structure outline: the models behind
//! the popups and tool windows.
//!
//! Nothing here touches the backend, storage or a window. What is remembered between sessions is parsed
//! from the stored text the caller passes in and comes back as writes to save; a model that needs a
//! backend answer says what to ask and is handed the answer.

pub mod bookmarks_store;
pub mod bookmarks_view;
pub mod filters;
pub mod inherited;
pub mod locations;
pub mod model;
pub mod open;
pub mod outline;
pub mod rank;
pub mod recent_model;
pub mod store;
