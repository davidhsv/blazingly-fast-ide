//! Platform, workbench and widget models that have no view of their own: settings shaping,
//! remote routing, start-up and focus rules, contribution registries and drag geometry.
//!
//! Nothing here touches the backend, storage or a window. A model that needs something done
//! returns it as an effect; time, ids and what the backend answered are passed in.

pub mod boot;
pub mod build_tool_recovery;
pub mod client_settings;
pub mod distraction_free;
pub mod dotnet;
pub mod dotnet_process;
pub mod endpoint_providers;
pub mod file_drop;
pub mod file_editors;
pub mod focus;
pub mod inspection_controls;
pub mod inspections;
pub mod occurrences;
pub mod page_swipe;
pub mod pointer_drag;
pub mod project;
pub mod provider_acquisition;
pub mod proxy_settings;
pub mod registry;
pub mod remote;
pub mod remote_settings;
pub mod running_processes;
pub mod selectable_text;
pub mod workflow_settings;
