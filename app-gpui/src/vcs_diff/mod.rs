//! The diff viewer and what surrounds it in version control: the comparison of two texts and the
//! chunks the viewer draws, moving between changes and files, scrolling two panes together, the
//! change markers and inline blame of the text editor, conflict marker diagnostics, file trees and
//! decorations, and the compare actions.

pub mod cm;
pub mod compare;
pub mod comparison;
pub mod conflict_markers;
pub mod diff_controls;
pub mod diff_input;
pub mod diff_navigation;
pub mod diff_options;
pub mod diff_scroll;
pub mod doc;
pub mod file_tree;
pub mod gutter;
pub mod hunks;
pub mod inline_blame;
pub mod marks;
pub mod merge_gutters;
pub mod model;
