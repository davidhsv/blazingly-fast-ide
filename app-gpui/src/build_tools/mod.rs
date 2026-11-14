//! The Maven and Gradle tool windows: the tree of a project's builds as the build files describe
//! them (and the last sync refines them), the run configurations a phase, goal or task starts, and
//! the Execute Maven Goal / Execute Gradle Task popup.

pub mod execute;
pub mod model;
pub mod view;
