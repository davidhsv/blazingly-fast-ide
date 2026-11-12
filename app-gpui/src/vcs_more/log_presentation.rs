//! What the Git log draws: the loaded commits after Collapse Linear Branches and the linearized
//! merges, with the graph rows of what is left. The same displayed adjacency drives painting,
//! navigation and the fragment actions.

use super::multi_root::graph_commit;
use crate::vcs::graph::{
    CollapsedGraph, GraphCommit, GraphRow, GraphState, LONG_EDGE, LinearizedGraph, collapse_linear, compact_lanes,
    compute_graph, hide_long_edges, linearize_merges, straighten_lane_changes,
};
use crate::vcs::log_options::LogOptions;
use crate::vcs_ops::log_store::RepoCommit;
use std::collections::{HashMap, HashSet};

/// Entry of [`LogFragments::merges`] that turns the set around: every merge is expanded but the
/// listed ones.
pub const ALL_MERGES: &str = "*";

/// The parts of the graph the user folded and unfolded one by one, by commit key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogFragments {
    /// Commits kept visible while linear branches are collapsed.
    pub expanded: HashSet<String>,
    /// Commits hidden while linear branches are not collapsed as a whole.
    pub collapsed: HashSet<String>,
    /// Merges whose incoming commits are not drawn in a line; with [`ALL_MERGES`], those that are.
    pub merges: HashSet<String>,
}

/// The log as displayed.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentedLog {
    /// Per displayed row, the index of its commit among the loaded ones.
    pub commits: Vec<usize>,
    /// The displayed commits linked to each other, by commit key.
    pub graph: Vec<GraphCommit>,
    pub rows: Vec<GraphRow>,
    pub linear: Option<LinearizedGraph>,
    pub collapsed: Option<CollapsedGraph>,
}

/// Lays out `all` as the options and the folded fragments say; the commits in `keep` stay visible
/// when linear branches are collapsed.
pub fn present_log_graph(
    all: &[RepoCommit],
    options: &LogOptions,
    fragments: &LogFragments,
    keep: &HashSet<String>,
) -> PresentedLog {
    let source: Vec<GraphCommit> = all.iter().map(graph_commit).collect();
    let collapsed = (options.collapse_linear || !fragments.collapsed.is_empty()).then(|| {
        let kept: HashSet<String> = keep.union(&fragments.expanded).cloned().collect();
        collapse_linear(&source, &kept, (!options.collapse_linear).then_some(&fragments.collapsed))
    });
    let shown = collapsed.as_ref().map_or(source.as_slice(), |collapsed| collapsed.graph.as_slice());
    let linear = options.linearize_merges.then(|| {
        if fragments.merges.contains(ALL_MERGES) {
            let expanded: HashSet<String> =
                source.iter().filter(|c| !fragments.merges.contains(&c.id)).map(|c| c.id.clone()).collect();
            linearize_merges(shown, &expanded)
        } else {
            linearize_merges(shown, &fragments.merges)
        }
    });
    let graph = linear.as_ref().map_or(shown, |linear| linear.graph.as_slice()).to_vec();
    let index_of: HashMap<String, usize> = all.iter().enumerate().map(|(index, c)| (c.key(), index)).collect();
    let commits = graph.iter().filter_map(|c| index_of.get(&c.id).copied()).collect();
    let laid = compute_graph(&graph, GraphState::default()).rows;
    let edges = if options.long_edges { laid } else { hide_long_edges(&laid, LONG_EDGE) };
    let rows = straighten_lane_changes(&compact_lanes(&edges));
    PresentedLog { commits, graph, rows, linear, collapsed }
}
