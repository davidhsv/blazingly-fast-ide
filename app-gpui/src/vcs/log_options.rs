//! The Git log's options (Graph Options, View Options, the columns) and what they make of a
//! commit's references.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Where the options are stored.
pub const STORAGE_KEY: &str = "openidea.vcs.logOptions";

/// The order of the log (Graph Options › Sort).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSort {
    #[default]
    Date,
    Topo,
    Standard,
}

/// Where the diff of a selected change shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLocation {
    #[default]
    Editor,
    Right,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogOptions {
    pub sort: LogSort,
    pub first_parent: bool,
    pub no_merges: bool,
    pub collapse_linear: bool,
    pub compact_refs: bool,
    pub tag_names: bool,
    pub long_edges: bool,
    pub commit_timestamp: bool,
    pub refs_on_left: bool,
    pub show_author: bool,
    pub show_date: bool,
    pub show_hash: bool,
    pub show_signature: bool,
    pub show_git_hub_checks: bool,
    pub show_column_headers: bool,
    pub highlight_mine: bool,
    pub highlight_merges: bool,
    pub highlight_current_branch: bool,
    pub show_branches: bool,
    pub group_by_directory: bool,
    pub diff_preview: bool,
    pub show_line_stats: bool,
    pub show_details: bool,
    pub show_repository: bool,
    pub show_only_affected: bool,
    pub changes_from_parents: bool,
    pub diff_location: DiffLocation,
    pub column_order: Vec<String>,
    pub column_widths: BTreeMap<String, f64>,
    pub linearize_merges: bool,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            sort: LogSort::Date,
            first_parent: false,
            no_merges: false,
            collapse_linear: false,
            compact_refs: true,
            tag_names: false,
            long_edges: false,
            commit_timestamp: false,
            refs_on_left: false,
            show_author: true,
            show_date: true,
            show_hash: false,
            show_signature: false,
            show_git_hub_checks: false,
            show_column_headers: false,
            highlight_mine: true,
            highlight_merges: true,
            highlight_current_branch: true,
            show_branches: true,
            group_by_directory: true,
            diff_preview: true,
            show_line_stats: false,
            show_details: true,
            show_repository: true,
            show_only_affected: false,
            changes_from_parents: false,
            diff_location: DiffLocation::Editor,
            column_order: ["author", "date", "hash"].map(str::to_owned).to_vec(),
            column_widths: BTreeMap::new(),
            linearize_merges: false,
        }
    }
}

impl LogOptions {
    /// Whether changing from `before` to these options changes what the log asks the backend.
    pub fn changes_query(&self, before: &LogOptions) -> bool {
        (self.sort, self.first_parent, self.no_merges) != (before.sort, before.first_parent, before.no_merges)
    }
}

/// The stored options, each field taking its default when it is missing or of the wrong type.
/// Column widths outside 40 to 800 pixels are dropped.
pub fn parse_log_options(raw: Option<&Value>) -> LogOptions {
    let mut out = LogOptions::default();
    let Some(raw) = raw.and_then(Value::as_object) else { return out };
    let flag = |name: &str, slot: &mut bool| {
        if let Some(value) = raw.get(name).and_then(Value::as_bool) {
            *slot = value;
        }
    };
    out.sort = match raw.get("sort").and_then(Value::as_str) {
        Some("standard") => LogSort::Standard,
        Some("topo") => LogSort::Topo,
        _ => LogSort::Date,
    };
    out.diff_location = match raw.get("diffLocation").and_then(Value::as_str) {
        Some("right") => DiffLocation::Right,
        Some("bottom") => DiffLocation::Bottom,
        _ => DiffLocation::Editor,
    };
    if let Some(order) = raw.get("columnOrder").and_then(Value::as_array) {
        let mut seen = BTreeSet::new();
        out.column_order =
            order.iter().filter_map(Value::as_str).filter(|id| seen.insert(*id)).map(str::to_owned).collect();
    }
    if let Some(widths) = raw.get("columnWidths").and_then(Value::as_object) {
        out.column_widths = widths
            .iter()
            .filter_map(|(id, width)| Some((id.clone(), width.as_f64()?)))
            .filter(|(_, width)| width.is_finite() && (40.0..=800.0).contains(width))
            .collect();
    }
    flag("firstParent", &mut out.first_parent);
    flag("noMerges", &mut out.no_merges);
    flag("collapseLinear", &mut out.collapse_linear);
    flag("compactRefs", &mut out.compact_refs);
    flag("tagNames", &mut out.tag_names);
    flag("longEdges", &mut out.long_edges);
    flag("commitTimestamp", &mut out.commit_timestamp);
    flag("refsOnLeft", &mut out.refs_on_left);
    flag("showAuthor", &mut out.show_author);
    flag("showDate", &mut out.show_date);
    flag("showHash", &mut out.show_hash);
    flag("showSignature", &mut out.show_signature);
    flag("showGitHubChecks", &mut out.show_git_hub_checks);
    flag("showColumnHeaders", &mut out.show_column_headers);
    flag("highlightMine", &mut out.highlight_mine);
    flag("highlightMerges", &mut out.highlight_merges);
    flag("highlightCurrentBranch", &mut out.highlight_current_branch);
    flag("showBranches", &mut out.show_branches);
    flag("groupByDirectory", &mut out.group_by_directory);
    flag("diffPreview", &mut out.diff_preview);
    flag("showLineStats", &mut out.show_line_stats);
    flag("showDetails", &mut out.show_details);
    flag("showRepository", &mut out.show_repository);
    flag("showOnlyAffected", &mut out.show_only_affected);
    flag("changesFromParents", &mut out.changes_from_parents);
    flag("linearizeMerges", &mut out.linearize_merges);
    out
}

/// What kind of reference a label stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    Local,
    Remote,
    Tag,
}

/// A reference label on a commit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub kind: RefKind,
    /// What the label says; empty for the one icon that stands for all tags.
    pub text: String,
    /// Every reference the label stands for.
    pub title: String,
}

/// The reference labels of a commit row. `refs` are decorations (`HEAD`, `main`, `origin/main`,
/// `tag: v1`); `remote_names` the remote-tracking branch names, and when there are none known a
/// name with a `/` counts as remote. Compact: a local branch absorbs its remote-tracking
/// branches (`origin & main`) and HEAD is left out next to other references. Without tag names,
/// the tags are one label without text.
pub fn ref_labels<S: AsRef<str>>(
    refs: &[S],
    compact: bool,
    tag_names: bool,
    remote_names: &BTreeSet<String>,
) -> Vec<RefLabel> {
    let refs: Vec<&str> = refs.iter().map(AsRef::as_ref).collect();
    let is_remote = |name: &str| if remote_names.is_empty() { name.contains('/') } else { remote_names.contains(name) };
    let tags: Vec<&str> = refs.iter().filter_map(|name| name.strip_prefix("tag: ")).collect();
    let branches: Vec<&str> =
        refs.iter().copied().filter(|name| *name != "HEAD" && !name.starts_with("tag: ")).collect();
    let label = |kind, text: String, title: String| RefLabel { kind, text, title };
    let mut out = Vec::new();
    if refs.contains(&"HEAD") && (!compact || branches.is_empty()) {
        out.push(label(RefKind::Head, "HEAD".to_owned(), "HEAD".to_owned()));
    }
    let remotes: Vec<&str> = branches.iter().copied().filter(|name| is_remote(name)).collect();
    let mut absorbed = BTreeSet::new();
    for local in branches.iter().copied().filter(|name| !is_remote(name)) {
        let tracking: Vec<&str> = if compact {
            remotes
                .iter()
                .copied()
                .filter(|remote| remote.strip_suffix(local).is_some_and(|rest| rest.ends_with('/')))
                .collect()
        } else {
            Vec::new()
        };
        absorbed.extend(tracking.iter().copied());
        let prefixes: Vec<&str> = tracking.iter().map(|remote| &remote[..remote.len() - local.len() - 1]).collect();
        let text = if prefixes.is_empty() { local.to_owned() } else { format!("{} & {local}", prefixes.join(", ")) };
        let title = std::iter::once(local).chain(tracking.iter().copied()).collect::<Vec<_>>().join(", ");
        out.push(label(RefKind::Local, text, title));
    }
    out.extend(
        remotes
            .iter()
            .filter(|remote| !absorbed.contains(**remote))
            .map(|remote| label(RefKind::Remote, (*remote).to_owned(), (*remote).to_owned())),
    );
    if !tags.is_empty() {
        if tag_names {
            out.extend(tags.iter().map(|tag| label(RefKind::Tag, (*tag).to_owned(), (*tag).to_owned())));
        } else {
            out.push(label(RefKind::Tag, String::new(), tags.join(", ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REFS: [&str; 6] = ["HEAD", "main", "origin/main", "feat/x", "tag: v1", "tag: v2"];

    fn remotes() -> BTreeSet<String> {
        BTreeSet::from(["origin/main".to_owned(), "origin/feat/x".to_owned()])
    }

    #[test]
    fn merges_a_branch_with_its_remote_tracking_branches_in_the_compact_view() {
        let labels = ref_labels(&REFS, true, false, &remotes());
        let shown: Vec<(RefKind, &str)> = labels.iter().map(|label| (label.kind, label.text.as_str())).collect();
        assert_eq!(shown, [(RefKind::Local, "origin & main"), (RefKind::Local, "feat/x"), (RefKind::Tag, "")]);
        assert_eq!(labels[2].title, "v1, v2");
    }

    #[test]
    fn lists_every_reference_otherwise() {
        let labels = ref_labels(&REFS, false, true, &remotes());
        let texts: Vec<&str> = labels.iter().map(|label| label.text.as_str()).collect();
        assert_eq!(texts, ["HEAD", "main", "feat/x", "origin/main", "v1", "v2"]);
    }

    #[test]
    fn parses_stored_options_with_per_field_defaults() {
        assert_eq!(parse_log_options(None), LogOptions::default());
        let parsed = parse_log_options(Some(&json!({ "sort": "topo", "longEdges": true, "showHash": "yes" })));
        assert_eq!(parsed, LogOptions { sort: LogSort::Topo, long_edges: true, ..LogOptions::default() });
    }

    #[test]
    fn previews_the_selected_files_diff_by_default_and_remembers_turning_it_off() {
        assert!(LogOptions::default().diff_preview);
        let off = parse_log_options(Some(&json!({ "diffPreview": false })));
        assert!(!off.diff_preview);
        assert!(!off.changes_query(&LogOptions::default()));
    }

    #[test]
    fn reloads_only_for_options_that_change_the_query() {
        let defaults = LogOptions::default();
        assert!(!LogOptions { compact_refs: false, ..defaults.clone() }.changes_query(&defaults));
        assert!(LogOptions { no_merges: true, ..defaults.clone() }.changes_query(&defaults));
        // What is stored reads back the same.
        let stored = serde_json::to_value(LogOptions { no_merges: true, ..defaults }).expect("serializes");
        assert_eq!(stored["noMerges"], true);
        assert_eq!(stored["sort"], "date");
        assert!(parse_log_options(Some(&stored)).no_merges);
    }

    // Not from a spec: without known remote names a slash marks a remote, and bad stored values go.
    #[test]
    fn guesses_remotes_by_their_slash_and_drops_bad_column_widths() {
        let labels = ref_labels(&["main", "origin/main"], true, false, &BTreeSet::new());
        assert_eq!(labels.len(), 1);
        assert_eq!((labels[0].text.as_str(), labels[0].title.as_str()), ("origin & main", "main, origin/main"));
        let parsed = parse_log_options(Some(&json!({
            "columnOrder": ["date", "date", 3, "author"],
            "columnWidths": { "author": 20, "date": 200, "hash": "wide" },
            "diffLocation": "bottom",
        })));
        assert_eq!(parsed.column_order, ["date", "author"]);
        assert_eq!(parsed.column_widths, BTreeMap::from([("date".to_owned(), 200.0)]));
        assert_eq!(parsed.diff_location, DiffLocation::Bottom);
    }
}
