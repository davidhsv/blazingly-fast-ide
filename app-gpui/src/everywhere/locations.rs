//! Recent caret locations (⇧⌘E): the places recorded while the user moves and edits, merged with the
//! editor's navigation history.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentLocation {
    pub path: String,
    /// 0-based.
    pub line: u32,
    /// In UTF-16 code units.
    pub col: u32,
    /// A few lines around `line`, starting at `snippet_start`.
    pub snippet: Vec<String>,
    pub snippet_start: u32,
    /// Recorded because of an edit (IDEA "Show changed only").
    pub changed: bool,
    /// When the place was visited, as Unix milliseconds; 0 when unknown.
    pub time: u64,
}

pub const MAX_LOCATIONS: usize = 60;
/// Caret moves within this many lines merge into one entry.
pub const MERGE_DISTANCE: u32 = 5;

fn is_near(path: &str, line: u32, other_path: &str, other_line: u32) -> bool {
    path == other_path && line.abs_diff(other_line) <= MERGE_DISTANCE
}

/// Puts `entry` in front of `list`, replacing the entries near it; it stays "changed" when the nearest one
/// was.
pub fn record_location(list: &[RecentLocation], entry: RecentLocation) -> Vec<RecentLocation> {
    let near = |location: &&RecentLocation| is_near(&location.path, location.line, &entry.path, entry.line);
    let changed = entry.changed || list.iter().find(near).is_some_and(|merged| merged.changed);
    let mut out: Vec<RecentLocation> = list.iter().filter(|location| !near(location)).cloned().collect();
    out.insert(0, RecentLocation { changed, ..entry });
    out.truncate(MAX_LOCATIONS);
    out
}

/// Combines the editor's history and the locations recorded here, most recent first. An entry without a
/// snippet takes the one of the place it merges with.
pub fn merge_locations(lists: &[&[RecentLocation]]) -> Vec<RecentLocation> {
    let mut all: Vec<RecentLocation> = lists.concat();
    // Stable: places visited at the same time keep the order of the lists.
    all.sort_by_key(|location| location.time);
    let mut out: Vec<RecentLocation> = Vec::new();
    for mut location in all {
        let prev = out.iter().find(|known| is_near(&known.path, known.line, &location.path, location.line));
        if let Some(prev) = prev.filter(|_| location.snippet.is_empty()) {
            location.snippet = prev.snippet.clone();
            location.snippet_start = prev.snippet_start;
        }
        out = record_location(&out, location);
    }
    out
}

/// A place removed from Recent Locations (Delete); visits after `time` show it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DismissedLocation {
    pub path: String,
    pub line: u32,
    pub time: u64,
}

pub fn is_dismissed(location: &RecentLocation, dismissed: &[DismissedLocation]) -> bool {
    dismissed
        .iter()
        .any(|gone| is_near(&gone.path, gone.line, &location.path, location.line) && location.time <= gone.time)
}

/// `path` after `from` became `to`: `None` when it is not `from` or below it, `Some(None)` when it went
/// away with `from`, else its new path.
pub fn renamed_path(path: &str, from: &str, to: Option<&str>) -> Option<Option<String>> {
    let rest = path.strip_prefix(from).filter(|rest| rest.is_empty() || rest.starts_with('/'))?;
    Some(to.map(|to| format!("{to}{rest}")))
}

/// Drops the entries pointing at removed files (`to` is `None`) and moves those of renamed ones.
pub fn rename_locations(list: &[RecentLocation], from: &str, to: Option<&str>) -> Vec<RecentLocation> {
    list.iter()
        .filter_map(|location| match renamed_path(&location.path, from, to) {
            None => Some(location.clone()),
            Some(moved) => moved.map(|path| RecentLocation { path, ..location.clone() }),
        })
        .collect()
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

/// A stored number as a line, a column or a count: negative ones are 0, fractions are dropped.
fn count(n: f64) -> u32 {
    n.max(0.0) as u32
}

/// A snippet line as text, whatever it was stored as.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Normalises the editor's navigation history into [`RecentLocation`]s: either its
/// `{ entries (oldest first), index }` snapshot or a plain list of entries. The entries are read
/// leniently: the position may sit on the entry or under `selection`, `pos` or `position`, and entries
/// without a path are skipped. `None` when `raw` is neither shape.
pub fn from_navigation_history(raw: &Value) -> Option<Vec<RecentLocation>> {
    let entries = match raw.get("entries") {
        Some(Value::Array(entries)) => entries,
        _ => raw.as_array()?,
    };
    let mut out = Vec::new();
    for entry in entries {
        let Some(path) = entry.get("path").and_then(Value::as_str).filter(|path| !path.is_empty()) else { continue };
        let nested = |key: &str| entry.get(key).filter(|value| !value.is_null());
        let pos = nested("selection").or_else(|| nested("pos")).or_else(|| nested("position")).unwrap_or(entry);
        let line = number(pos.get("line")).map_or(0, count);
        let col = number(pos.get("col")).or_else(|| number(pos.get("column"))).map_or(0, count);
        let snippet: Vec<String> = match entry.get("snippet") {
            Some(Value::Array(lines)) => lines.iter().map(text_of).collect(),
            Some(Value::String(line)) => vec![line.clone()],
            _ => entry.get("text").and_then(Value::as_str).map(|text| vec![text.to_owned()]).unwrap_or_default(),
        };
        let flag = |name: &str| entry.get(name) == Some(&Value::Bool(true));
        out.push(RecentLocation {
            path: path.to_owned(),
            line,
            col,
            snippet,
            snippet_start: number(entry.get("snippetStart")).map_or(line, count),
            changed: flag("changed") || flag("edited"),
            time: number(entry.get("time")).map_or(0, |time| time.max(0.0) as u64),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn loc(path: &str, line: u32) -> RecentLocation {
        RecentLocation {
            path: path.to_owned(),
            line,
            col: 0,
            snippet: vec![format!("line {line}")],
            snippet_start: line,
            changed: false,
            time: u64::from(line),
        }
    }

    fn places(list: &[RecentLocation]) -> Vec<String> {
        list.iter().map(|location| format!("{}:{}", location.path, location.line)).collect()
    }

    #[test]
    fn merges_nearby_caret_moves_and_keeps_the_most_recent_first() {
        let mut list = Vec::new();
        list = record_location(&list, loc("/a", 10));
        list = record_location(&list, loc("/b", 1));
        list = record_location(&list, RecentLocation { changed: true, ..loc("/a", 12) });
        list = record_location(&list, loc("/a", 13));
        assert_eq!(places(&list), ["/a:13", "/b:1"]);
        assert!(list[0].changed);
    }

    #[test]
    fn drops_removed_files() {
        let moved = rename_locations(&[loc("/d/a", 1), loc("/e", 1)], "/d", None);
        assert_eq!(moved.iter().map(|location| location.path.as_str()).collect::<Vec<_>>(), ["/e"]);
    }

    #[test]
    fn normalises_editor_navigation_history_entries() {
        let raw = json!([
            { "path": "/a", "selection": { "line": 3, "col": 2 } },
            { "path": "/b", "line": 1, "column": 4, "text": "x" },
            null,
            { "nope": 1 },
        ]);
        let out = from_navigation_history(&raw).unwrap();
        let read: Vec<(&str, u32, u32, &[String])> = out
            .iter()
            .map(|location| (location.path.as_str(), location.line, location.col, location.snippet.as_slice()))
            .collect();
        assert_eq!(read, [("/a", 3, 2, &[][..]), ("/b", 1, 4, &["x".to_owned()][..])]);
        assert_eq!(from_navigation_history(&json!("bad")), None);
    }

    #[test]
    fn accepts_the_editors_entries_and_index_snapshot_and_merges_with_recorded_locations() {
        let snapshot = json!({
            "entries": [
                { "path": "/a", "line": 1, "col": 0, "time": 1 },
                { "path": "/b", "line": 50, "col": 0, "time": 3 },
            ],
            "index": 1,
        });
        let history = from_navigation_history(&snapshot).unwrap();
        let recorded = [RecentLocation { time: 2, snippet: vec!["code".to_owned()], changed: true, ..loc("/a", 2) }];
        let merged = merge_locations(&[history.as_slice(), recorded.as_slice()]);
        assert_eq!(places(&merged), ["/b:50", "/a:2"]);
        assert!(merged[1].changed);
        assert_eq!(merged[1].snippet, ["code"]);
    }

    #[test]
    fn hide_the_place_until_it_is_visited_again() {
        let dismissed = [DismissedLocation { path: "/a".to_owned(), line: 10, time: 100 }];
        let visited = |path: &str, line: u32, time: u64| RecentLocation { time, ..loc(path, line) };
        assert!(is_dismissed(&visited("/a", 12, 90), &dismissed));
        assert!(!is_dismissed(&visited("/a", 30, 90), &dismissed));
        assert!(!is_dismissed(&visited("/b", 10, 90), &dismissed));
        assert!(!is_dismissed(&visited("/a", 10, 101), &dismissed));
    }

    // Not from a spec: renamed folders move their locations, siblings sharing the prefix stay, and the list
    // is capped.
    #[test]
    fn renames_follow_folders_and_the_list_is_capped() {
        let list = [loc("/d/a", 1), loc("/d", 2), loc("/dx/a", 3)];
        let moved = rename_locations(&list, "/d", Some("/n"));
        assert_eq!(places(&moved), ["/n/a:1", "/n:2", "/dx/a:3"]);
        let mut many = Vec::new();
        let visits = MAX_LOCATIONS as u32 + 5;
        for line in 0..visits {
            many = record_location(&many, loc("/a", line * 10));
        }
        assert_eq!(many.len(), MAX_LOCATIONS);
        assert_eq!(many[0].line, (visits - 1) * 10);
    }

    // Not from a spec: every optional field of a history entry and the defaults of the missing ones.
    #[test]
    fn history_entries_are_read_leniently() {
        let raw = json!([
            { "path": "/a", "pos": { "line": 7, "column": 3 }, "snippet": ["x", 1], "snippetStart": 6, "edited": 1 },
            { "path": "/a2", "edited": true },
            { "path": "/b", "position": { "line": -2 }, "snippet": "one", "changed": "yes", "time": 12.9 },
            { "path": "", "line": 1 },
            { "path": 5 },
        ]);
        let out = from_navigation_history(&raw).unwrap();
        assert_eq!(out.len(), 3);
        let first = RecentLocation {
            path: "/a".to_owned(),
            line: 7,
            col: 3,
            snippet: vec!["x".to_owned(), "1".to_owned()],
            snippet_start: 6,
            changed: false,
            time: 0,
        };
        assert_eq!(out[0], first);
        assert!(out[1].changed);
        assert!(out[1].snippet.is_empty());
        assert_eq!((out[2].line, out[2].snippet_start, out[2].changed, out[2].time), (0, 0, false, 12));
        assert_eq!(out[2].snippet, ["one"]);
        assert_eq!(from_navigation_history(&json!({ "entries": "bad" })), None);
    }
}
