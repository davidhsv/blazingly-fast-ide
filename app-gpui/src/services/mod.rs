//! The Services tool window: a tree of the services that providers contribute (run configurations,
//! Docker connections and registries, plugin views), the selected node's detail pane beside it and
//! "Add Service" on its toolbar.
//!
//! [`model`] is the tree host: the providers' roots merged in provider order, selection, expansion,
//! reordering, the batched toolbar and context menu of a selection, and the detail pane's lifetime.
//! [`docker`] is the Docker provider's model: the nodes of a connection's snapshot, their actions
//! and menus, and the refresh / operation flows as requests with tickets. [`run_types`] holds the
//! Docker run configuration types and the run targets of Dockerfiles and Compose files,
//! [`dashboard`] the parsers behind a Docker object's dashboard, and [`view`] the tool window's view.
//!
//! The models do no I/O: backend answers arrive as arguments and what the backend must do is
//! returned as data.

pub mod dashboard;
pub mod docker;
pub mod model;
pub mod run_types;
pub mod view;
