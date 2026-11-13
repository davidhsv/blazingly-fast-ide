//! Usages as Find Usages (the tool window) and Show Usages (the popup) list them: grouped by file
//! with each usage's line, the semantic category and read / write access a server may add, the
//! filters, and the hand-over of found usages to the tool window.

use crate::find::store::StorageWrite;
use crate::find::utf16;
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::basename;
use ide_core::ipc_path::to_ipc;
use ide_core::{IpcPath, Location, Pos, Range};
use ide_lsp::types::{ClassifiedReference, ReferenceAccess, ReferenceKind};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// The semantic category a language extension gave a usage (Java's accessor calls).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageKind {
    Getter,
    Setter,
    Field,
    Unknown,
}

impl UsageKind {
    pub fn from_backend(kind: &ReferenceKind) -> Self {
        match kind {
            ReferenceKind::Getter => Self::Getter,
            ReferenceKind::Setter => Self::Setter,
            ReferenceKind::Field => Self::Field,
            ReferenceKind::Unknown => Self::Unknown,
        }
    }

    /// How the backend spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Getter => "getter",
            Self::Setter => "setter",
            Self::Field => "field",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "getter" => Self::Getter,
            "setter" => Self::Setter,
            "field" => Self::Field,
            _ => Self::Unknown,
        }
    }
}

/// Whether a usage reads or writes what it refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageAccess {
    Read,
    Write,
    ReadWrite,
    Unknown,
}

impl UsageAccess {
    pub fn from_backend(access: &ReferenceAccess) -> Self {
        match access {
            ReferenceAccess::Read => Self::Read,
            ReferenceAccess::Write => Self::Write,
            ReferenceAccess::ReadWrite => Self::ReadWrite,
            ReferenceAccess::Unknown => Self::Unknown,
        }
    }

    /// How the backend spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "readWrite",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "read" => Self::Read,
            "write" => Self::Write,
            "readWrite" => Self::ReadWrite,
            _ => Self::Unknown,
        }
    }
}

/// A server's classification of one reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub location: Location,
    pub kind: UsageKind,
    /// `None`: unclassified (older extensions).
    pub access: Option<UsageAccess>,
    /// The reference's line, when the server has it at hand (a library class's).
    pub text: Option<String>,
}

impl Classified {
    pub fn from_backend(reference: &ClassifiedReference) -> Self {
        Self {
            location: reference.location.clone(),
            kind: UsageKind::from_backend(&reference.kind),
            access: reference.access.as_ref().map(UsageAccess::from_backend),
            text: reference.text.clone(),
        }
    }
}

/// One usage with its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub kind: Option<UsageKind>,
    pub access: Option<UsageAccess>,
    /// The exact range the server gave, kept when the line preview is clamped to one line.
    pub source_location: Option<Location>,
    pub path: String,
    pub line: u32,
    pub col: u32,
    /// End column on `line` (clamped to the line for a range over several lines).
    pub end_col: u32,
    /// The usage's line (empty when the file could not be read).
    pub text: String,
}

impl Usage {
    /// A plain usage at `line`, columns `col..end_col` of `text`.
    pub fn at(path: &str, line: u32, col: u32, end_col: u32, text: &str) -> Self {
        Self {
            kind: None,
            access: None,
            source_location: None,
            path: path.to_owned(),
            line,
            col,
            end_col,
            text: text.to_owned(),
        }
    }

    /// Where the usage is, as the backend gave it or as its line shows it.
    pub fn location(&self) -> Location {
        self.source_location.clone().unwrap_or_else(|| Location {
            path: IpcPath::from(PathBuf::from(&self.path)),
            range: Range::new(Pos::new(self.line, self.col), Pos::new(self.line, self.end_col)),
            uri: None,
        })
    }
}

/// The usages of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageGroup {
    pub path: String,
    pub usages: Vec<Usage>,
}

type LocationKey = (String, u32, u32, u32, u32);

fn location_key(location: &Location) -> LocationKey {
    let Range { start, end } = location.range;
    (to_ipc(&location.path), start.line, start.col, end.line, end.col)
}

/// The lines of the files that could be read, by path (`None`: unreadable).
pub type FileLines = HashMap<String, Option<Vec<String>>>;

/// Groups reference locations by file (by path, then position), each with its line: from `files`,
/// else the line the server gave along. Repeated locations count once.
pub fn group_usages(locations: &[Location], files: &FileLines, classified: &[Classified]) -> Vec<UsageGroup> {
    let kinds: HashMap<LocationKey, &Classified> =
        classified.iter().map(|reference| (location_key(&reference.location), reference)).collect();
    let mut groups: Vec<UsageGroup> = Vec::new();
    let mut seen = HashSet::new();
    for location in locations {
        let key = location_key(location);
        if !seen.insert(key.clone()) {
            continue;
        }
        let path = key.0.clone();
        let Range { start, end } = location.range;
        let class = kinds.get(&key);
        let text = files
            .get(&path)
            .and_then(Option::as_ref)
            .and_then(|lines| lines.get(start.line as usize))
            .cloned()
            .or_else(|| class.and_then(|class| class.text.clone()))
            .unwrap_or_default();
        let end_col = if end.line == start.line { end.col } else { utf16::len(&text).max(start.col) };
        let usage = Usage {
            kind: class.map(|class| class.kind),
            access: class.and_then(|class| class.access),
            source_location: class.map(|_| location.clone()),
            path: path.clone(),
            line: start.line,
            col: start.col,
            end_col,
            text,
        };
        match groups.iter_mut().find(|group| group.path == path) {
            Some(group) => group.usages.push(usage),
            None => groups.push(UsageGroup { path, usages: vec![usage] }),
        }
    }
    groups.sort_by(|a, b| collate::compare(&a.path, &b.path));
    for group in &mut groups {
        group.usages.sort_by_key(|usage| (usage.line, usage.col));
    }
    groups
}

/// The paths whose lines are read to show these usages: not those the server gave along.
pub fn paths_to_read(locations: &[Location], classified: &[Classified]) -> Vec<String> {
    let given: HashSet<LocationKey> =
        classified.iter().filter(|reference| reference.text.is_some()).map(|r| location_key(&r.location)).collect();
    let mut paths: Vec<String> = Vec::new();
    for location in locations {
        let path = to_ipc(&location.path);
        if !given.contains(&location_key(location)) && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

pub fn usage_count(groups: &[UsageGroup]) -> usize {
    groups.iter().map(|group| group.usages.len()).sum()
}

/// A usage's line as a row shows it: leading and trailing blanks trimmed, the usage's range moved
/// along. Columns are UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrimmedPreview {
    pub text: String,
    /// The column of the line where `text` starts.
    pub text_col: u32,
    pub from: u32,
    pub to: u32,
}

pub fn trim_preview(usage: &Usage) -> TrimmedPreview {
    let start = usage.text.trim_start();
    let lead = utf16::len(&usage.text) - utf16::len(start);
    let text = start.trim_end().to_owned();
    let from = usage.col.saturating_sub(lead);
    let to = i64::from(usage.end_col) - i64::from(lead);
    let to = to.min(i64::from(utf16::len(&text))).max(i64::from(from));
    TrimmedPreview { text, text_col: lead, from, to: u32::try_from(to).unwrap_or(from) }
}

/// Usages whose file name or line contains what is typed, ignoring case.
pub fn filter_usages<'a>(items: &[&'a Usage], filter: &str) -> Vec<&'a Usage> {
    let query = filter.trim().to_lowercase();
    if query.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .copied()
        .filter(|usage| {
            usage.text.to_lowercase().contains(&query) || basename(&usage.path).to_lowercase().contains(&query)
        })
        .collect()
}

/// The groups' usages as locations for the Find Usages tool window, exact ranges kept.
pub fn usage_locations(groups: &[UsageGroup]) -> Vec<Location> {
    groups.iter().flat_map(|group| group.usages.iter().map(Usage::location)).collect()
}

/// The groups' usages with their categories, unclassified ones as `unknown`.
pub fn usage_classified(groups: &[UsageGroup]) -> Vec<Classified> {
    groups
        .iter()
        .flat_map(|group| group.usages.iter())
        .map(|usage| Classified {
            location: usage.location(),
            kind: usage.kind.unwrap_or(UsageKind::Unknown),
            access: usage.access,
            text: None,
        })
        .collect()
}

/// Where a search looks: Project Files, or Project and Libraries (Java's dependency jars and JDK).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageScope {
    Project,
    Libraries,
}

impl UsageScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project Files",
            Self::Libraries => "Project and Libraries",
        }
    }
}

/// The scope a search ran in, when there was a choice (`None` when the server has none).
/// `widenable`: an earlier run of the same search skipped the libraries.
pub fn searched_scope(libraries: bool, libraries_skipped: bool, widenable: bool) -> Option<UsageScope> {
    if libraries_skipped {
        Some(UsageScope::Project)
    } else {
        (libraries && widenable).then_some(UsageScope::Libraries)
    }
}

/// The tool window tab's title, e.g. "Usages of 'foo' in Project Files".
pub fn tab_title(symbol: Option<&str>, scope: Option<UsageScope>, i18n: &I18n) -> String {
    let Some(symbol) = symbol else { return i18n.tx("Usages") };
    let message = match scope {
        Some(UsageScope::Project) => "Usages of '{symbol}' in Project Files",
        Some(UsageScope::Libraries) => "Usages of '{symbol}' in Project and Libraries",
        None => "Usages of '{symbol}'",
    };
    crate::platform::i18n::format_message(&i18n.tx(message), &[("symbol", Param::from(symbol))], i18n.locale())
}

// ---------------------------------------------------------------------------
// Read / write access filter.

/// The key the access filter is kept under.
pub const ACCESS_KEY: &str = "openidea.usages.accessFilter.v1";

/// Which accesses are shown; at least one always is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageAccessOptions {
    pub read: bool,
    pub write: bool,
}

impl Default for UsageAccessOptions {
    fn default() -> Self {
        Self { read: true, write: true }
    }
}

/// One of the two access buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessButton {
    Read,
    Write,
}

impl UsageAccessOptions {
    /// The options stored under [`ACCESS_KEY`]; missing, malformed or hiding everything: both on.
    pub fn restored(stored: Option<&str>) -> Self {
        let value = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok()).unwrap_or(Value::Null);
        match (value.get("read").and_then(Value::as_bool), value.get("write").and_then(Value::as_bool)) {
            (Some(read), Some(write)) if read || write => Self { read, write },
            _ => Self::default(),
        }
    }

    /// Turns `which` on or off; turning off the last one shown turns the other one on.
    pub fn toggle(&mut self, which: AccessButton) -> StorageWrite {
        match which {
            AccessButton::Read => self.read = !self.read,
            AccessButton::Write => self.write = !self.write,
        }
        if !self.read && !self.write {
            match which {
                AccessButton::Read => self.write = true,
                AccessButton::Write => self.read = true,
            }
        }
        StorageWrite { key: ACCESS_KEY, value: json!({ "read": self.read, "write": self.write }).to_string() }
    }

    pub fn shows(self, which: AccessButton) -> bool {
        match which {
            AccessButton::Read => self.read,
            AccessButton::Write => self.write,
        }
    }
}

/// The usages the access filter lets through. Unclassified usages, unknown and read-write ones
/// always show: access is never guessed from an accessor's name.
pub fn filter_usage_access<'a>(items: &[&'a Usage], options: UsageAccessOptions) -> Vec<&'a Usage> {
    items
        .iter()
        .copied()
        .filter(|usage| match usage.access {
            None | Some(UsageAccess::Unknown | UsageAccess::ReadWrite) => true,
            Some(UsageAccess::Read) => options.read,
            Some(UsageAccess::Write) => options.write,
        })
        .collect()
}

/// Whether the server told accesses apart for any of `items` (the filter shows then).
pub fn supports_usage_access(items: &[&Usage]) -> bool {
    items.iter().any(|usage| usage.access.is_some_and(|access| access != UsageAccess::Unknown))
}

/// The count of the tool window's summary row: the total, or "shown / total" while the access
/// filter hides some, then "result(s)".
pub fn results_label(shown: usize, total: usize, filtering: bool, i18n: &I18n) -> String {
    let unit = i18n.tx(if total == 1 { "result" } else { "results" });
    if filtering { format!("{shown} / {total} {unit}") } else { format!("{total} {unit}") }
}

// ---------------------------------------------------------------------------
// Excluded usages (⌫ in the tool window).

/// A usage's identity among the results: its file and exact range.
pub fn usage_key(usage: &Usage) -> String {
    let (end_line, end_col) = usage
        .source_location
        .as_ref()
        .map_or((usage.line, usage.end_col), |location| (location.range.end.line, location.range.end.col));
    format!("{}:{}:{}:{end_line}:{end_col}", usage.path, usage.line, usage.col)
}

/// Toggles exclusion of `usages`: all excluded already → included again, else all excluded.
pub fn toggle_excluded_usages(excluded: &HashSet<String>, usages: &[&Usage]) -> HashSet<String> {
    let keys: Vec<String> = usages.iter().map(|usage| usage_key(usage)).collect();
    let mut next = excluded.clone();
    if keys.iter().all(|key| excluded.contains(key)) {
        for key in &keys {
            next.remove(key);
        }
    } else {
        next.extend(keys);
    }
    next
}

// ---------------------------------------------------------------------------
// Usages found elsewhere, shown in the tool window (`find.showUsageResults`).

/// The argument of `find.showUsageResults`: `{ symbol, locations, classified?, note? }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowUsageResults {
    pub symbol: Option<String>,
    pub locations: Vec<Location>,
    pub classified: Vec<Classified>,
    pub note: Option<String>,
}

impl ShowUsageResults {
    /// The usages `arg` describes; `None` without a list of locations. Malformed entries are skipped.
    pub fn parse(arg: Option<&Value>) -> Option<Self> {
        let arg = arg?;
        let locations = arg
            .get("locations")?
            .as_array()?
            .iter()
            .filter_map(|location| serde_json::from_value::<Location>(location.clone()).ok())
            .collect();
        let classified = arg
            .get("classified")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(parse_classified).collect())
            .unwrap_or_default();
        Some(Self {
            symbol: arg.get("symbol").and_then(Value::as_str).map(str::to_owned),
            locations,
            classified,
            note: arg.get("note").and_then(Value::as_str).map(str::to_owned),
        })
    }

    /// The popup's usages as the argument of `find.showUsageResults`, categories kept.
    pub fn from_groups(symbol: Option<&str>, note: Option<&str>, groups: &[UsageGroup]) -> Self {
        Self {
            symbol: symbol.map(str::to_owned),
            locations: usage_locations(groups),
            classified: usage_classified(groups),
            note: note.map(str::to_owned),
        }
    }

    pub fn to_value(&self) -> Value {
        let classified: Vec<Value> = self
            .classified
            .iter()
            .map(|reference| {
                let mut entry = json!({ "location": reference.location, "kind": reference.kind.as_str() });
                if let Some(access) = reference.access {
                    entry["access"] = json!(access.as_str());
                }
                if let Some(text) = &reference.text {
                    entry["text"] = json!(text);
                }
                entry
            })
            .collect();
        json!({
            "symbol": self.symbol,
            "note": self.note,
            "locations": self.locations,
            "classified": classified,
        })
    }
}

fn parse_classified(value: &Value) -> Option<Classified> {
    Some(Classified {
        location: serde_json::from_value(value.get("location")?.clone()).ok()?,
        kind: UsageKind::parse(value.get("kind").and_then(Value::as_str).unwrap_or("unknown")),
        access: value.get("access").and_then(Value::as_str).map(UsageAccess::parse),
        text: value.get("text").and_then(Value::as_str).map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(path: &str, line: u32, col: u32, end_col: u32) -> Location {
        Location {
            path: IpcPath::from(PathBuf::from(path)),
            range: Range::new(Pos::new(line, col), Pos::new(line, end_col)),
            uri: None,
        }
    }

    fn lines(entries: &[(&str, Option<&[&str]>)]) -> FileLines {
        entries
            .iter()
            .map(|(path, lines)| {
                ((*path).to_owned(), lines.map(|lines| lines.iter().map(|line| (*line).to_owned()).collect()))
            })
            .collect()
    }

    fn classified(location: &Location, kind: UsageKind) -> Classified {
        Classified { location: location.clone(), kind, access: None, text: None }
    }

    #[test]
    fn groups_by_file_with_previews_sorted_and_de_duplicated() {
        let files = lines(&[("/p/b.ts", Some(&["", "  const x = foo();"])), ("/p/a.ts", None)]);
        let groups = group_usages(
            &[loc("/p/b.ts", 1, 12, 15), loc("/p/a.ts", 4, 0, 3), loc("/p/b.ts", 1, 12, 15), loc("/p/b.ts", 0, 0, 0)],
            &files,
            &[],
        );
        let shape: Vec<(&str, Vec<u32>)> = groups
            .iter()
            .map(|group| (group.path.as_str(), group.usages.iter().map(|usage| usage.line).collect()))
            .collect();
        assert_eq!(shape, [("/p/a.ts", vec![4]), ("/p/b.ts", vec![0, 1])]);
        assert_eq!(usage_count(&groups), 3);
        assert_eq!(groups[0].usages[0].text, "");
        assert_eq!(
            trim_preview(&groups[1].usages[1]),
            TrimmedPreview { text: "const x = foo();".to_owned(), text_col: 2, from: 10, to: 13 }
        );
    }

    #[test]
    fn preserves_semantic_categories_for_separate_ranges_on_the_same_source_line() {
        let getter = loc("/Bean.java", 0, 12, 19);
        let setter = loc("/Bean.java", 0, 2, 9);
        let files = lines(&[("/Bean.java", Some(&["b.setName(b.getName())"]))]);
        let groups = group_usages(
            &[getter.clone(), setter.clone()],
            &files,
            &[classified(&getter, UsageKind::Getter), classified(&setter, UsageKind::Setter)],
        );
        let shape: Vec<(u32, Option<UsageKind>)> = groups[0].usages.iter().map(|u| (u.col, u.kind)).collect();
        assert_eq!(shape, [(2, Some(UsageKind::Setter)), (12, Some(UsageKind::Getter))]);
    }

    #[test]
    fn retains_nested_references_sharing_a_start_and_their_exact_multiline_range() {
        let inner = loc("/Bean.java", 0, 0, 9);
        let outer = Location { range: Range::new(Pos::new(0, 0), Pos::new(1, 10)), ..loc("/Bean.java", 0, 0, 0) };
        let files = lines(&[("/Bean.java", Some(&["b.value()", ".value(x);"]))]);
        let groups = group_usages(
            &[inner.clone(), outer.clone()],
            &files,
            &[classified(&inner, UsageKind::Getter), classified(&outer, UsageKind::Setter)],
        );
        assert_eq!(groups[0].usages.len(), 2);
        assert_eq!(groups[0].usages[1].source_location.as_ref(), Some(&outer));
    }

    #[test]
    fn shows_the_lines_the_server_gave_along_for_files_it_does_not_read() {
        let lib = loc("jdt://contents/lib/Lib.class", 3, 4, 8);
        let given = Classified { text: Some("    used();".to_owned()), ..classified(&lib, UsageKind::Unknown) };
        assert!(paths_to_read(std::slice::from_ref(&lib), std::slice::from_ref(&given)).is_empty());
        let groups = group_usages(std::slice::from_ref(&lib), &FileLines::new(), &[given]);
        assert_eq!(groups[0].usages[0].text, "    used();");
    }

    #[test]
    fn ignores_malformed_arguments() {
        assert_eq!(ShowUsageResults::parse(Some(&json!({ "symbol": "x" }))), None);
        assert_eq!(ShowUsageResults::parse(None), None);
        let range = json!({ "start": { "line": 1, "col": 4 }, "end": { "line": 1, "col": 8 } });
        let args = ShowUsageResults::parse(Some(&json!({
            "symbol": "used",
            "locations": [{ "path": "/p/b.rs", "range": range }, 7],
        })))
        .expect("locations are given");
        assert_eq!((args.symbol.as_deref(), args.locations.len()), (Some("used"), 1));
    }

    fn popup_groups() -> Vec<UsageGroup> {
        vec![
            UsageGroup {
                path: "/p/a.ts".to_owned(),
                usages: vec![Usage::at("/p/a.ts", 2, 0, 3, "foo(order)"), Usage::at("/p/a.ts", 5, 0, 3, "foo(cart)")],
            },
            UsageGroup { path: "/p/b.ts".to_owned(), usages: vec![Usage::at("/p/b.ts", 1, 0, 3, "foo(order, 2)")] },
        ]
    }

    #[test]
    fn matches_file_names_and_line_text() {
        let groups = popup_groups();
        let all: Vec<&Usage> = groups.iter().flat_map(|group| group.usages.iter()).collect();
        assert_eq!(filter_usages(&all, "b.ts").len(), 1);
        assert_eq!(filter_usages(&all, "ORDER").len(), 2);
        assert_eq!(filter_usages(&all, ""), all);
    }

    #[test]
    fn preserves_an_exact_multiline_semantic_range_when_transferring_to_find() {
        let source = Location { range: Range::new(Pos::new(1, 0), Pos::new(2, 10)), ..loc("/Bean.java", 1, 0, 0) };
        let usage = Usage { source_location: Some(source.clone()), ..Usage::at("/Bean.java", 1, 0, 3, "b.value()") };
        let groups = [UsageGroup { path: "/Bean.java".to_owned(), usages: vec![usage] }];
        assert_eq!(usage_locations(&groups), [source]);
    }

    fn access_rows() -> Vec<Usage> {
        let accesses = [UsageAccess::Read, UsageAccess::Write, UsageAccess::ReadWrite, UsageAccess::Unknown];
        accesses
            .into_iter()
            .map(Some)
            .chain([None])
            .enumerate()
            .map(|(line, access)| Usage { access, ..Usage::at("/Bean.java", line as u32, 0, 1, "x") })
            .collect()
    }

    fn shown_lines(rows: &[Usage], options: UsageAccessOptions) -> Vec<u32> {
        let refs: Vec<&Usage> = rows.iter().collect();
        filter_usage_access(&refs, options).into_iter().map(|usage| usage.line).collect()
    }

    #[test]
    fn matches_all_three_reachable_states_and_retains_unknown_legacy_rows() {
        let rows = access_rows();
        assert_eq!(shown_lines(&rows, UsageAccessOptions { read: true, write: true }).len(), 5);
        assert_eq!(shown_lines(&rows, UsageAccessOptions { read: true, write: false }), [0, 2, 3, 4]);
        assert_eq!(shown_lines(&rows, UsageAccessOptions { read: false, write: true }), [1, 2, 3, 4]);
        let getter = [Usage { kind: Some(UsageKind::Getter), ..rows[4].clone() }];
        assert_eq!(shown_lines(&getter, UsageAccessOptions { read: false, write: true }).len(), 1);
    }

    #[test]
    fn turns_the_other_button_on_when_the_last_visible_access_is_turned_off() {
        let mut options = UsageAccessOptions::default();
        let _ = options.toggle(AccessButton::Write);
        assert_eq!(options, UsageAccessOptions { read: true, write: false });
        let _ = options.toggle(AccessButton::Read);
        assert_eq!(options, UsageAccessOptions { read: false, write: true });
        let stored = options.toggle(AccessButton::Write);
        assert_eq!(options, UsageAccessOptions { read: true, write: false });
        assert_eq!(UsageAccessOptions::restored(Some(&stored.value)), UsageAccessOptions { read: true, write: false });
    }

    #[test]
    fn starts_with_both_on_and_repairs_invalid_persisted_state() {
        assert_eq!(UsageAccessOptions::restored(None), UsageAccessOptions::default());
        assert_eq!(
            UsageAccessOptions::restored(Some(r#"{"read":false,"write":false}"#)),
            UsageAccessOptions::default()
        );
    }

    #[test]
    fn excludes_a_group_unless_all_of_it_is_excluded_already() {
        let (one, two) = (Usage::at("/a", 1, 0, 1, ""), Usage::at("/a", 2, 0, 1, ""));
        let first = toggle_excluded_usages(&HashSet::new(), &[&one]);
        assert_eq!(first, HashSet::from(["/a:1:0:1:1".to_owned()]));
        let both = toggle_excluded_usages(&first, &[&one, &two]);
        assert_eq!(both, HashSet::from(["/a:1:0:1:1".to_owned(), "/a:2:0:2:1".to_owned()]));
        assert!(toggle_excluded_usages(&both, &[&one, &two]).is_empty());
    }

    #[test]
    fn shows_a_plain_count_for_unclassified_searches_after_a_java_access_mode_was_selected() {
        let rows = [Usage::at("/p/a.ts", 0, 0, 3, "foo()"), Usage::at("/p/a.ts", 1, 0, 3, "foo()")];
        let refs: Vec<&Usage> = rows.iter().collect();
        let options = UsageAccessOptions { read: true, write: false };
        let filtering = supports_usage_access(&refs) && !(options.read && options.write);
        let shown = filter_usage_access(&refs, options).len();
        assert!(!filtering);
        assert_eq!(results_label(shown, rows.len(), filtering, &I18n::default()), "2 results");
    }

    // Not from a spec: Open in Find Window keeps every usage's category, unclassified ones as unknown.
    #[test]
    fn hands_found_usages_over_with_their_categories() {
        let text = "b.setName(b.getName())";
        let usages = vec![
            Usage {
                kind: Some(UsageKind::Setter),
                access: Some(UsageAccess::Write),
                ..Usage::at("/p/Bean.java", 2, 2, 9, text)
            },
            Usage::at("/p/Bean.java", 4, 0, 4, "other"),
        ];
        let groups = [UsageGroup { path: "/p/Bean.java".to_owned(), usages }];
        let args = ShowUsageResults::from_groups(Some("name"), None, &groups);
        let kinds: Vec<(UsageKind, Option<UsageAccess>)> =
            args.classified.iter().map(|reference| (reference.kind, reference.access)).collect();
        assert_eq!(kinds, [(UsageKind::Setter, Some(UsageAccess::Write)), (UsageKind::Unknown, None)]);
        assert_eq!(ShowUsageResults::parse(Some(&args.to_value())), Some(args));
        let i18n = I18n::default();
        assert_eq!(tab_title(Some("foo"), Some(UsageScope::Project), &i18n), "Usages of 'foo' in Project Files");
        assert_eq!(tab_title(None, None, &i18n), "Usages");
        assert_eq!(searched_scope(true, false, true), Some(UsageScope::Libraries));
        assert_eq!(searched_scope(false, false, false), None);
    }
}
