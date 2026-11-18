//! The test tree of the Run tool window: the results of a session's test run ([`model`]), the tree
//! as shown under its filters and expansion ([`rows`]), and the view beside the console ([`view`]).

pub mod model;
pub mod rows;
pub mod view;
#[cfg(test)]
mod view_tests;

pub use model::{RunTests, can_rerun_failed, rerun_failed_starts, rerun_failed_target};

/// The context key that is on while the test tree has the keyboard: Expand All and Collapse All
/// take their keys then.
pub const TEST_TREE_FOCUS: &str = "testTreeFocus";
pub use view::{TestTreeCommand, TestTreeEvent, TestTreeView, TreeSession};
