//! Bookmarks, kept per project: line bookmarks (F3) and file / folder bookmarks, in named bookmark lists
//! (one is the default that new bookmarks go to). A line bookmark may carry a mnemonic (0-9 / A-Z, unique:
//! ⌥F3, ⌃⇧digit) and a description.
//!
//! Nothing here reads or writes storage. A project's bookmarks are stored as JSON under two keys
//! ([`bookmarks_key`], [`lists_key`]): the caller passes the stored text in and saves the
//! [`StorageWrite`]s that come back.

use super::locations::renamed_path;
use super::open::{StorageWrite, storage_write, stored_json};
use crate::platform::collate;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

/// A bookmarked line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bookmark {
    pub path: String,
    /// 0-based.
    pub line: u32,
    /// Line text when the bookmark was set or last moved.
    pub text: String,
    /// When it was set, as Unix milliseconds.
    pub created: u64,
    /// `0`-`9` or `A`-`Z`; at most one bookmark has each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<char>,
    /// User text shown instead of the line (F3 on a selection, Rename).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Id of its bookmark list; unset until stored (then the default list).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
}

/// A bookmarked file or folder (project view, editor tab).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathBookmark {
    pub path: String,
    pub dir: bool,
    pub list: String,
    pub created: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BookmarkList {
    pub id: String,
    pub name: String,
}

/// The mnemonics in the order the chooser offers them.
pub const MNEMONICS: [char; 36] = [
    '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

pub fn is_mnemonic(c: char) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase()
}

/// Prefix of the key a project's line bookmarks are stored under: a plain list, as versions without
/// bookmark lists wrote and read it.
pub const BOOKMARKS_KEY_PREFIX: &str = "openidea.bookmarks.v1:";
/// Prefix of the key a project's lists and file / folder bookmarks are stored under.
pub const LISTS_KEY_PREFIX: &str = "openidea.bookmarkLists.v1:";
pub const DEFAULT_LIST_ID: &str = "default";

/// Key of the line bookmarks of the project at `scope`.
pub fn bookmarks_key(scope: &str) -> String {
    format!("{BOOKMARKS_KEY_PREFIX}{scope}")
}

/// Key of the lists and file / folder bookmarks of the project at `scope`.
pub fn lists_key(scope: &str) -> String {
    format!("{LISTS_KEY_PREFIX}{scope}")
}

/// Bookmarks by path, then by line.
pub fn sort_bookmarks(mut list: Vec<Bookmark>) -> Vec<Bookmark> {
    list.sort_by(|a, b| collate::compare(&a.path, &b.path).then(a.line.cmp(&b.line)));
    list
}

fn is_at(bookmark: &Bookmark, path: &str, line: u32) -> bool {
    bookmark.path == path && bookmark.line == line
}

pub fn has_bookmark(list: &[Bookmark], path: &str, line: u32) -> bool {
    bookmark_at(list, path, line).is_some()
}

pub fn bookmark_at<'a>(list: &'a [Bookmark], path: &str, line: u32) -> Option<&'a Bookmark> {
    list.iter().find(|bookmark| is_at(bookmark, path, line))
}

/// `text` trimmed and cut to 200 UTF-16 code units, never inside a character.
fn clip(text: &str) -> String {
    let mut units = 0;
    text.trim()
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 200
        })
        .collect()
}

/// A description as it is kept: `None` when blank.
fn described(description: &str) -> Option<String> {
    (!description.trim().is_empty()).then(|| clip(description))
}

fn without(list: &[Bookmark], path: &str, line: u32) -> Vec<Bookmark> {
    list.iter().filter(|bookmark| !is_at(bookmark, path, line)).cloned().collect()
}

fn new_bookmark(path: &str, line: u32, text: &str, now: u64) -> Bookmark {
    Bookmark {
        path: path.to_owned(),
        line,
        text: clip(text),
        created: now,
        mnemonic: None,
        description: None,
        list: None,
    }
}

/// F3: removes the line's bookmark or adds one, created at `now`. `description` is the selected text,
/// as in IDEA.
pub fn toggle_bookmark(
    list: &[Bookmark],
    path: &str,
    line: u32,
    text: &str,
    now: u64,
    description: Option<&str>,
) -> Vec<Bookmark> {
    let mut out = without(list, path, line);
    if out.len() < list.len() {
        return out;
    }
    out.push(Bookmark { description: description.and_then(described), ..new_bookmark(path, line, text, now) });
    sort_bookmarks(out)
}

/// Gives the line's bookmark (created at `now` if needed) `mnemonic`, taking it from any other bookmark;
/// `None` turns it into a plain bookmark. `description` replaces the description when given (blank clears
/// it).
pub fn assign_mnemonic(
    list: &[Bookmark],
    path: &str,
    line: u32,
    text: &str,
    mnemonic: Option<char>,
    description: Option<&str>,
    now: u64,
) -> Vec<Bookmark> {
    let mut out = without(list, path, line);
    if mnemonic.is_some() {
        for other in out.iter_mut().filter(|other| other.mnemonic == mnemonic) {
            other.mnemonic = None;
        }
    }
    let mut bookmark = bookmark_at(list, path, line).cloned().unwrap_or_else(|| new_bookmark(path, line, text, now));
    bookmark.mnemonic = mnemonic;
    if let Some(description) = description {
        bookmark.description = described(description);
    }
    out.push(bookmark);
    sort_bookmarks(out)
}

/// ⌃⇧digit: removes the bookmark when the line already has `mnemonic`; otherwise assigns it.
pub fn toggle_mnemonic(
    list: &[Bookmark],
    path: &str,
    line: u32,
    text: &str,
    mnemonic: char,
    now: u64,
) -> Vec<Bookmark> {
    if bookmark_at(list, path, line).is_some_and(|bookmark| bookmark.mnemonic == Some(mnemonic)) {
        return without(list, path, line);
    }
    assign_mnemonic(list, path, line, text, Some(mnemonic), None, now)
}

pub fn bookmark_by_mnemonic(list: &[Bookmark], mnemonic: char) -> Option<&Bookmark> {
    list.iter().find(|bookmark| bookmark.mnemonic == Some(mnemonic))
}

/// Rename (F2 in the Bookmarks view): a blank description shows the line again.
pub fn describe_bookmark(list: &[Bookmark], path: &str, line: u32, description: &str) -> Vec<Bookmark> {
    let mut out = list.to_vec();
    for bookmark in out.iter_mut().filter(|bookmark| is_at(bookmark, path, line)) {
        bookmark.description = described(description);
    }
    out
}

/// What the Bookmarks view and popup show for a bookmark.
pub fn bookmark_label(bookmark: &Bookmark) -> &str {
    let description = bookmark.description.as_deref().map(str::trim).filter(|text| !text.is_empty());
    description.unwrap_or_else(|| bookmark.text.trim())
}

pub fn remove_bookmarks(list: &[Bookmark], remove: impl Fn(&Bookmark) -> bool) -> Vec<Bookmark> {
    list.iter().filter(|&bookmark| !remove(bookmark)).cloned().collect()
}

/// Replaces the lines of `path`'s bookmarks after an edit moved them. `lines` are the new line and its
/// text of each of the file's current bookmarks, in their order; bookmarks landing on the same line
/// collapse into the first. A line without a bookmark before is created at `now`.
pub fn set_file_lines(list: &[Bookmark], path: &str, lines: &[(u32, &str)], now: u64) -> Vec<Bookmark> {
    let old: Vec<&Bookmark> = list.iter().filter(|bookmark| bookmark.path == path).collect();
    let mut out: Vec<Bookmark> = list.iter().filter(|bookmark| bookmark.path != path).cloned().collect();
    let mut seen = HashSet::new();
    for (index, &(line, text)) in lines.iter().enumerate() {
        if !seen.insert(line) {
            continue;
        }
        let prev = old.get(index);
        out.push(Bookmark {
            path: path.to_owned(),
            line,
            text: clip(text),
            created: prev.map_or(now, |prev| prev.created),
            mnemonic: prev.and_then(|prev| prev.mnemonic),
            description: prev.and_then(|prev| prev.description.clone()),
            list: prev.and_then(|prev| prev.list.clone()),
        });
    }
    sort_bookmarks(out)
}

/// Moves the bookmarks of `from` and of what lies below it to `to`; drops them when `to` is `None`
/// (deleted).
pub fn rename_bookmarks(list: &[Bookmark], from: &str, to: Option<&str>) -> Vec<Bookmark> {
    let moved = list.iter().filter_map(|bookmark| match renamed_path(&bookmark.path, from, to) {
        None => Some(bookmark.clone()),
        Some(path) => path.map(|path| Bookmark { path, ..bookmark.clone() }),
    });
    sort_bookmarks(moved.collect())
}

/// The bookmarked lines of `path`, ascending.
pub fn lines_of(list: &[Bookmark], path: &str) -> Vec<u32> {
    let in_file = list.iter().filter(|bookmark| bookmark.path == path);
    let mut lines: Vec<u32> = in_file.map(|bookmark| bookmark.line).collect();
    lines.sort_unstable();
    lines
}

/// A bookmarked line with its mnemonic, as the gutter shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub line: u32,
    pub mnemonic: Option<char>,
}

/// Lines of `path`'s bookmarks with their mnemonics, sorted by line.
pub fn marks_of(list: &[Bookmark], path: &str) -> Vec<Mark> {
    let in_file = list.iter().filter(|bookmark| bookmark.path == path);
    let mut marks: Vec<Mark> =
        in_file.map(|bookmark| Mark { line: bookmark.line, mnemonic: bookmark.mnemonic }).collect();
    marks.sort_by_key(|mark| mark.line);
    marks
}

/// A stored time or count: a number, or text holding one; anything else is 0.
fn stored_number(value: Option<&Value>) -> u64 {
    let number = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    };
    number.filter(|number| number.is_finite()).map_or(0, |number| number.max(0.0) as u64)
}

fn stored_text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

fn stored_bookmark(raw: &Value) -> Option<Bookmark> {
    let path = stored_text(raw.get("path"))?;
    let line = raw.get("line")?.as_f64().filter(|line| line.fract() == 0.0)?;
    let text = match raw.get("text") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    let mnemonic = stored_text(raw.get("mnemonic")).and_then(|text| {
        let mut chars = text.chars();
        chars.next().filter(|&c| chars.next().is_none() && is_mnemonic(c))
    });
    Some(Bookmark {
        path: path.to_owned(),
        line: line.max(0.0) as u32,
        text,
        created: stored_number(raw.get("created")),
        mnemonic,
        description: stored_text(raw.get("description")).filter(|text| !text.trim().is_empty()).map(str::to_owned),
        list: stored_text(raw.get("list")).filter(|list| !list.is_empty()).map(str::to_owned),
    })
}

/// Line bookmarks from the value stored under [`bookmarks_key`], sorted. Anything that is not a list
/// reads as no bookmarks; entries without a path and a whole line number are dropped.
pub fn parse_bookmarks(stored: Option<&str>) -> Vec<Bookmark> {
    let Some(Value::Array(items)) = stored_json(stored) else { return Vec::new() };
    sort_bookmarks(items.iter().filter_map(stored_bookmark).collect())
}

/// Everything kept for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkData {
    pub items: Vec<Bookmark>,
    pub paths: Vec<PathBookmark>,
    pub lists: Vec<BookmarkList>,
    /// Id of the list new bookmarks go to.
    pub default_list: String,
}

impl BookmarkData {
    /// No bookmarks, in one default list called `list_name`.
    pub fn empty(list_name: &str) -> Self {
        Self {
            items: Vec::new(),
            paths: Vec::new(),
            lists: vec![BookmarkList { id: DEFAULT_LIST_ID.to_owned(), name: list_name.to_owned() }],
            default_list: DEFAULT_LIST_ID.to_owned(),
        }
    }

    fn has_list(&self, id: &str) -> bool {
        has_list(&self.lists, id)
    }
}

pub fn path_bookmark_at<'a>(paths: &'a [PathBookmark], path: &str) -> Option<&'a PathBookmark> {
    paths.iter().find(|bookmark| bookmark.path == path)
}

fn sort_paths(mut list: Vec<PathBookmark>) -> Vec<PathBookmark> {
    list.sort_by(|a, b| collate::compare(&a.path, &b.path));
    list
}

/// Adds `targets` (a path and whether it is a folder) to the default list at `now`, or removes them all
/// when every one is bookmarked already.
pub fn toggle_path_bookmarks(data: &BookmarkData, targets: &[(&str, bool)], now: u64) -> BookmarkData {
    let mut next = data.clone();
    if targets.is_empty() {
        return next;
    }
    if targets.iter().all(|&(path, _)| path_bookmark_at(&data.paths, path).is_some()) {
        next.paths.retain(|bookmark| !targets.iter().any(|&(path, _)| path == bookmark.path));
        return next;
    }
    for &(path, dir) in targets {
        if path_bookmark_at(&next.paths, path).is_none() {
            let list = data.default_list.clone();
            next.paths.push(PathBookmark { path: path.to_owned(), dir, list, created: now });
        }
    }
    next.paths = sort_paths(next.paths);
    next
}

fn new_list_id(lists: &[BookmarkList]) -> String {
    let mut number = lists.len() + 1;
    loop {
        let id = format!("list{number}");
        if !lists.iter().any(|list| list.id == id) {
            return id;
        }
        number += 1;
    }
}

/// Why a proposed list name cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListNameError {
    Empty,
    /// Another list has this (trimmed) name.
    Duplicate(String),
}

impl ListNameError {
    /// The English source text of the error.
    pub fn text(&self) -> String {
        match self {
            Self::Empty => "Name cannot be empty".to_owned(),
            Self::Duplicate(name) => format!("A list named '{name}' already exists"),
        }
    }
}

/// The error of a proposed list name, or `None` when it is valid. `own_id` is the list being renamed.
pub fn validate_list_name(lists: &[BookmarkList], name: &str, own_id: Option<&str>) -> Option<ListNameError> {
    let name = name.trim();
    if name.is_empty() {
        return Some(ListNameError::Empty);
    }
    let taken = lists.iter().any(|list| Some(list.id.as_str()) != own_id && list.name == name);
    taken.then(|| ListNameError::Duplicate(name.to_owned()))
}

/// Adds a list called `name`; returns the data with it and its id.
pub fn create_list(data: &BookmarkData, name: &str) -> (BookmarkData, String) {
    let id = new_list_id(&data.lists);
    let mut next = data.clone();
    next.lists.push(BookmarkList { id: id.clone(), name: name.trim().to_owned() });
    (next, id)
}

pub fn rename_list(data: &BookmarkData, id: &str, name: &str) -> BookmarkData {
    let mut next = data.clone();
    for list in next.lists.iter_mut().filter(|list| list.id == id) {
        list.name = name.trim().to_owned();
    }
    next
}

/// Makes the list `id` the default; unknown lists are ignored.
pub fn set_default_list(data: &BookmarkData, id: &str) -> BookmarkData {
    let mut next = data.clone();
    if next.has_list(id) {
        next.default_list = id.to_owned();
    }
    next
}

/// Deletes a list with its bookmarks. The default moves to the first remaining list; when none remains,
/// a fresh default list called `fallback_name` takes its place.
pub fn delete_list(data: &BookmarkData, id: &str, fallback_name: &str) -> BookmarkData {
    let mut next = data.clone();
    next.items.retain(|bookmark| bookmark.list.as_deref() != Some(id));
    next.paths.retain(|bookmark| bookmark.list != id);
    next.lists.retain(|list| list.id != id);
    if next.lists.is_empty() {
        next.lists.push(BookmarkList { id: DEFAULT_LIST_ID.to_owned(), name: fallback_name.to_owned() });
    }
    if !next.has_list(&next.default_list) {
        next.default_list = next.lists[0].id.clone();
    }
    next
}

/// Identifies a bookmark: a line (`path` and `line`) or a file / folder (`path` alone).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BookmarkRef {
    pub path: String,
    pub line: Option<u32>,
}

/// Moves the bookmarks `refs` name to the list `to`; an unknown list is ignored.
pub fn move_bookmarks(data: &BookmarkData, refs: &[BookmarkRef], to: &str) -> BookmarkData {
    fn names(refs: &[BookmarkRef], path: &str, line: Option<u32>) -> bool {
        refs.iter().any(|wanted| wanted.path == path && wanted.line == line)
    }
    let mut next = data.clone();
    if !next.has_list(to) {
        return next;
    }
    for bookmark in next.items.iter_mut().filter(|bookmark| names(refs, &bookmark.path, Some(bookmark.line))) {
        bookmark.list = Some(to.to_owned());
    }
    for bookmark in next.paths.iter_mut().filter(|bookmark| names(refs, &bookmark.path, None)) {
        bookmark.list = to.to_owned();
    }
    next
}

/// Moves the file / folder bookmarks of `from` and of what lies below it to `to`; drops them when `to`
/// is `None` (deleted).
pub fn rename_path_bookmarks(list: &[PathBookmark], from: &str, to: Option<&str>) -> Vec<PathBookmark> {
    let moved = list.iter().filter_map(|bookmark| match renamed_path(&bookmark.path, from, to) {
        None => Some(bookmark.clone()),
        Some(path) => path.map(|path| PathBookmark { path, ..bookmark.clone() }),
    });
    sort_paths(moved.collect())
}

/// The lists and file / folder bookmarks from their stored value, around the line bookmarks `items`.
/// Whatever is missing or malformed is dropped; there is always a default list, called `project_name`
/// when none is stored.
fn sanitize_data(raw: Option<&Value>, mut items: Vec<Bookmark>, project_name: &str) -> BookmarkData {
    let mut lists: Vec<BookmarkList> = Vec::new();
    for entry in stored_entries(raw, "lists") {
        let id = stored_text(entry.get("id")).filter(|id| !id.is_empty());
        if let (Some(id), Some(name)) = (id, stored_text(entry.get("name")))
            && !has_list(&lists, id)
        {
            let name = if name.trim().is_empty() { id } else { name.trim() };
            lists.push(BookmarkList { id: id.to_owned(), name: name.to_owned() });
        }
    }
    if lists.is_empty() {
        lists.push(BookmarkList { id: DEFAULT_LIST_ID.to_owned(), name: project_name.to_owned() });
    }
    let stored_default = stored_text(raw.and_then(|raw| raw.get("defaultList"))).filter(|id| has_list(&lists, id));
    let default_list = stored_default.unwrap_or(lists[0].id.as_str()).to_owned();
    let mut paths = Vec::new();
    for entry in stored_entries(raw, "paths") {
        let Some(path) = stored_text(entry.get("path")) else { continue };
        let list = stored_text(entry.get("list")).filter(|id| has_list(&lists, id)).unwrap_or(default_list.as_str());
        paths.push(PathBookmark {
            path: path.to_owned(),
            dir: entry.get("dir") == Some(&Value::Bool(true)),
            list: list.to_owned(),
            created: stored_number(entry.get("created")),
        });
    }
    // Bookmarks saved before lists existed belong to the default list.
    for bookmark in &mut items {
        if bookmark.list.as_deref().is_none_or(|list| !has_list(&lists, list)) {
            bookmark.list = Some(default_list.clone());
        }
    }
    BookmarkData { items, paths: sort_paths(paths), lists, default_list }
}

fn has_list(lists: &[BookmarkList], id: &str) -> bool {
    lists.iter().any(|list| list.id == id)
}

/// The entries of the list stored as `name` in `raw`; none when it is missing or not a list.
fn stored_entries<'a>(raw: Option<&'a Value>, name: &str) -> impl Iterator<Item = &'a Value> + use<'a> {
    raw.and_then(|raw| raw.get(name)).and_then(Value::as_array).into_iter().flatten()
}

/// The lists and file / folder bookmarks as they are stored under [`lists_key`].
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredLists<'a> {
    lists: &'a [BookmarkList],
    default_list: &'a str,
    paths: &'a [PathBookmark],
}

/// The bookmarks of the open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkStore {
    scope: Option<String>,
    data: BookmarkData,
}

impl Default for BookmarkStore {
    fn default() -> Self {
        Self { scope: None, data: BookmarkData::empty("Bookmarks") }
    }
}

impl BookmarkStore {
    /// The project root the bookmarks belong to; `None` without a project.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub fn data(&self) -> &BookmarkData {
        &self.data
    }

    /// Switches to the project at `scope` with what is stored for it under [`bookmarks_key`] and
    /// [`lists_key`]; without a project there are no bookmarks.
    pub fn load(&mut self, scope: Option<&str>, stored_items: Option<&str>, stored_lists: Option<&str>) {
        let Some(scope) = scope.filter(|scope| !scope.is_empty()) else {
            *self = Self { scope: scope.map(str::to_owned), ..Self::default() };
            return;
        };
        let name = scope.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(scope);
        let lists = stored_json(stored_lists);
        self.data = sanitize_data(lists.as_ref(), parse_bookmarks(stored_items), name);
        self.scope = Some(scope.to_owned());
    }

    /// Changes lists and file bookmarks (and line bookmarks); line bookmarks without a list join the
    /// default one. Returns what to save: nothing when nothing changed or no project is open.
    #[must_use]
    pub fn update_data(&mut self, change: impl FnOnce(&BookmarkData) -> BookmarkData) -> Vec<StorageWrite> {
        let mut next = change(&self.data);
        if next == self.data {
            return Vec::new();
        }
        for bookmark in &mut next.items {
            if bookmark.list.as_deref().is_none_or(str::is_empty) {
                bookmark.list = Some(next.default_list.clone());
            }
        }
        let mut writes = Vec::new();
        if let Some(scope) = &self.scope {
            if next.items != self.data.items {
                writes.push(storage_write(&bookmarks_key(scope), &next.items));
            }
            let (old, new) = (&self.data, &next);
            if new.paths != old.paths || new.lists != old.lists || new.default_list != old.default_list {
                let stored = StoredLists { lists: &new.lists, default_list: &new.default_list, paths: &new.paths };
                writes.push(storage_write(&lists_key(scope), &stored));
            }
        }
        self.data = next;
        writes
    }

    /// Changes the line bookmarks; see [`BookmarkStore::update_data`].
    #[must_use]
    pub fn update(&mut self, change: impl FnOnce(&[Bookmark]) -> Vec<Bookmark>) -> Vec<StorageWrite> {
        self.update_data(|data| BookmarkData { items: change(data.items.as_slice()), ..data.clone() })
    }
}

/// The line the mnemonic chooser (⌥F3) is open for.
#[derive(Debug, Clone, PartialEq)]
pub struct MnemonicTarget {
    pub path: String,
    pub line: u32,
    /// Line text, for a bookmark the chooser creates.
    pub text: String,
    /// Where to show the chooser (the caret), in window pixels; centred without.
    pub at: Option<(f32, f32)>,
}

/// The bookmark list being named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListNaming {
    /// The list being renamed; `None` names a new one.
    pub list_id: Option<String>,
    /// Bookmarks to move into the new list once it is created ("Move to › New List…").
    pub move_here: Vec<BookmarkRef>,
}

/// Which bookmark popups are open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookmarkPopups {
    /// ⌥F3: the mnemonic chooser for this line.
    pub chooser: Option<MnemonicTarget>,
    /// ⌘F3: the Bookmarks popup.
    pub list: bool,
    /// Edit Description dialog (F2 in the Bookmarks view and popup): the bookmark's path and line.
    pub editing: Option<(String, u32)>,
    /// New / rename bookmark list dialog.
    pub naming: Option<ListNaming>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    const NOW: u64 = 99;

    fn bookmark(path: &str, line: u32, text: &str, created: u64) -> Bookmark {
        new_bookmark(path, line, text, created)
    }

    fn places(list: &[Bookmark]) -> Vec<String> {
        list.iter().map(|bookmark| format!("{}:{}", bookmark.path, bookmark.line)).collect()
    }

    fn data() -> BookmarkData {
        let item = Bookmark { list: Some(DEFAULT_LIST_ID.to_owned()), ..bookmark("/p/a.ts", 1, "a", 1) };
        BookmarkData { items: vec![item], ..BookmarkData::empty("p") }
    }

    fn list(id: &str, name: &str) -> BookmarkList {
        BookmarkList { id: id.to_owned(), name: name.to_owned() }
    }

    fn line_ref(path: &str, line: u32) -> BookmarkRef {
        BookmarkRef { path: path.to_owned(), line: Some(line) }
    }

    fn path_ref(path: &str) -> BookmarkRef {
        BookmarkRef { path: path.to_owned(), line: None }
    }

    fn path_names(paths: &[PathBookmark]) -> Vec<&str> {
        paths.iter().map(|bookmark| bookmark.path.as_str()).collect()
    }

    /// A store with the storage its writes are saved to.
    #[derive(Default)]
    struct Harness {
        store: BookmarkStore,
        storage: BTreeMap<String, String>,
    }

    impl Harness {
        fn load(&mut self, scope: &str) {
            let items = self.storage.get(&bookmarks_key(scope)).map(String::as_str);
            let lists = self.storage.get(&lists_key(scope)).map(String::as_str);
            self.store.load(Some(scope), items, lists);
        }

        fn save(&mut self, writes: Vec<StorageWrite>) {
            for write in writes {
                self.storage.insert(write.key, write.value);
            }
        }
    }

    #[test]
    fn toggles_on_and_off_keeping_the_list_sorted() {
        let mut list = Vec::new();
        list = toggle_bookmark(&list, "/p/b.kt", 3, "  val x = 1  ", 1, None);
        list = toggle_bookmark(&list, "/p/a.kt", 9, "fun a()", 2, None);
        list = toggle_bookmark(&list, "/p/b.kt", 1, "import", 3, None);
        assert_eq!(places(&list), ["/p/a.kt:9", "/p/b.kt:1", "/p/b.kt:3"]);
        assert_eq!(list[2].text, "val x = 1");
        list = toggle_bookmark(&list, "/p/b.kt", 1, "import", NOW, None);
        assert_eq!(lines_of(&list, "/p/b.kt"), [3]);
    }

    #[test]
    fn moves_lines_after_edits_and_collapses_duplicates() {
        let list = [bookmark("/p/a", 1, "a", 1), bookmark("/p/a", 5, "b", 2), bookmark("/p/z", 0, "z", 3)];
        let moved = set_file_lines(&list, "/p/a", &[(3, "a"), (3, "b")], NOW);
        assert_eq!(places(&moved), ["/p/a:3", "/p/z:0"]);
        assert_eq!(moved[0].created, 1);
    }

    #[test]
    fn follows_renames_and_drops_deleted_files() {
        let list = [bookmark("/p/dir/a", 1, "", 1), bookmark("/p/other", 2, "", 1)];
        let renamed = rename_bookmarks(&list, "/p/dir", Some("/p/new"));
        assert_eq!(places(&renamed), ["/p/new/a:1", "/p/other:2"]);
        assert_eq!(places(&rename_bookmarks(&list, "/p/dir", None)), ["/p/other:2"]);
    }

    #[test]
    fn persists_per_project() {
        let mut bookmarks = Harness::default();
        bookmarks.load("/p1");
        let writes = bookmarks.store.update(|list| toggle_bookmark(list, "/p1/a", 0, "x", NOW, None));
        bookmarks.save(writes);
        bookmarks.load("/p2");
        assert!(bookmarks.store.data().items.is_empty());
        bookmarks.load("/p1");
        assert_eq!(lines_of(&bookmarks.store.data().items, "/p1/a"), [0]);
    }

    #[test]
    fn ignores_corrupt_storage() {
        let mut bookmarks = Harness::default();
        let stored = json!([{ "path": 1 }, { "path": "/bad/x", "line": 2 }, "nope"]);
        bookmarks.storage.insert(bookmarks_key("/bad"), stored.to_string());
        bookmarks.load("/bad");
        assert_eq!(bookmarks.store.data().items.iter().map(|bookmark| bookmark.line).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn assigns_a_mnemonic_moving_it_from_another_bookmark() {
        let mut list = Vec::new();
        list = assign_mnemonic(&list, "/p/a", 1, "one", Some('A'), None, 1);
        list = assign_mnemonic(&list, "/p/b", 2, "two", Some('A'), Some("second"), 2);
        let marked = bookmark_by_mnemonic(&list, 'A').unwrap();
        assert_eq!((marked.path.as_str(), marked.line, marked.description.as_deref()), ("/p/b", 2, Some("second")));
        assert_eq!(bookmark_at(&list, "/p/a", 1).unwrap().mnemonic, None);
        list = assign_mnemonic(&list, "/p/b", 2, "two", None, Some(""), NOW);
        assert_eq!(bookmark_at(&list, "/p/b", 2), Some(&bookmark("/p/b", 2, "two", 2)));
    }

    #[test]
    fn ctrl_shift_digit_toggles_a_numbered_bookmark() {
        let mark = |line: u32, mnemonic: Option<char>| Mark { line, mnemonic };
        let mut list = toggle_mnemonic(&[], "/p/a", 3, "x", '1', NOW);
        assert_eq!(marks_of(&list, "/p/a"), [mark(3, Some('1'))]);
        list = toggle_mnemonic(&list, "/p/a", 5, "y", '1', NOW);
        assert_eq!(marks_of(&list, "/p/a"), [mark(3, None), mark(5, Some('1'))]);
        list = toggle_mnemonic(&list, "/p/a", 5, "y", '1', NOW);
        assert_eq!(lines_of(&list, "/p/a"), [3]);
    }

    #[test]
    fn keeps_mnemonics_and_descriptions_when_lines_move_or_storage_reloads() {
        let plain = bookmark("/p/a", 1, "a", 1);
        let list = [Bookmark { mnemonic: Some('7'), description: Some("note".to_owned()), ..plain }];
        let moved = set_file_lines(&list, "/p/a", &[(4, "a")], NOW);
        assert_eq!((moved[0].line, moved[0].mnemonic, moved[0].description.as_deref()), (4, Some('7'), Some("note")));
        let mut stored = serde_json::to_value(list).unwrap();
        stored.as_array_mut().unwrap().push(json!({ "path": "/p/b", "line": 0, "mnemonic": "ab" }));
        let mut bookmarks = Harness::default();
        bookmarks.storage.insert(bookmarks_key("/m"), stored.to_string());
        bookmarks.load("/m");
        let items = &bookmarks.store.data().items;
        assert_eq!(items.iter().map(|bookmark| bookmark.mnemonic).collect::<Vec<_>>(), [Some('7'), None]);
    }

    #[test]
    fn f3_on_a_selection_uses_it_as_the_description_rename_edits_it() {
        let mut list = toggle_bookmark(&[], "/p/a", 0, "let x = 1;", 1, Some("  important  "));
        assert_eq!(bookmark_label(&list[0]), "important");
        list = describe_bookmark(&list, "/p/a", 0, "renamed");
        assert_eq!(bookmark_label(&list[0]), "renamed");
        list = describe_bookmark(&list, "/p/a", 0, " ");
        assert_eq!(bookmark_label(&list[0]), "let x = 1;");
    }

    #[test]
    fn creates_renames_and_validates_lists() {
        let (created, id) = create_list(&data(), " Review ");
        assert_eq!(created.lists, [list(DEFAULT_LIST_ID, "p"), list(&id, "Review")]);
        let duplicate = validate_list_name(&created.lists, "Review", None).unwrap();
        assert!(duplicate.text().contains("already exists"));
        assert_eq!(validate_list_name(&created.lists, "Review", Some(id.as_str())), None);
        assert!(validate_list_name(&created.lists, " ", None).unwrap().text().contains("empty"));
        assert_eq!(rename_list(&created, &id, "Later").lists[1].name, "Later");
    }

    #[test]
    fn moves_bookmarks_between_lists_and_changes_the_default() {
        let (created, id) = create_list(&data(), "Two");
        let mut moved = toggle_path_bookmarks(&created, &[("/p/src", true)], 5);
        moved = move_bookmarks(&moved, &[line_ref("/p/a.ts", 1), path_ref("/p/src")], &id);
        assert_eq!(moved.items[0].list.as_deref(), Some(id.as_str()));
        assert_eq!(moved.paths[0].list, id);
        // Unknown target lists are ignored.
        assert_eq!(move_bookmarks(&moved, &[path_ref("/p/src")], "nope"), moved);
        moved = set_default_list(&moved, &id);
        assert_eq!(moved.default_list, id);
        moved = toggle_path_bookmarks(&moved, &[("/p/b.ts", false)], 6);
        assert_eq!(path_bookmark_at(&moved.paths, "/p/b.ts").unwrap().list, id);
    }

    #[test]
    fn deletes_a_list_with_its_bookmarks_keeping_a_default() {
        let (created, id) = create_list(&data(), "Two");
        let in_two = set_default_list(&move_bookmarks(&created, &[line_ref("/p/a.ts", 1)], &id), &id);
        let deleted = delete_list(&in_two, &id, "p");
        assert!(deleted.items.is_empty());
        assert_eq!(deleted.lists, [list(DEFAULT_LIST_ID, "p")]);
        assert_eq!(deleted.default_list, DEFAULT_LIST_ID);
        let last = delete_list(&deleted, DEFAULT_LIST_ID, "fresh");
        assert_eq!(last.lists, [list(DEFAULT_LIST_ID, "fresh")]);
    }

    #[test]
    fn toggles_file_folder_bookmarks_together_and_follows_removals() {
        let both = [("/p/x.ts", false), ("/p/dir", true)];
        let mut toggled = toggle_path_bookmarks(&data(), &[("/p/x.ts", false)], 1);
        toggled = toggle_path_bookmarks(&toggled, &both, 2);
        assert_eq!(path_names(&toggled.paths), ["/p/dir", "/p/x.ts"]);
        toggled = toggle_path_bookmarks(&toggled, &both, NOW);
        assert!(toggled.paths.is_empty());
        let paths = toggle_path_bookmarks(&data(), &[("/p/dir/f.ts", false)], NOW).paths;
        assert_eq!(path_names(&rename_path_bookmarks(&paths, "/p/dir", Some("/p/new"))), ["/p/new/f.ts"]);
        assert!(rename_path_bookmarks(&paths, "/p/dir", None).is_empty());
    }

    #[test]
    fn reads_bookmarks_saved_before_lists_into_the_default_list() {
        let mut bookmarks = Harness::default();
        let stored = json!([{ "path": "/proj/a", "line": 2, "text": "", "created": 1 }]);
        bookmarks.storage.insert(bookmarks_key("/proj"), stored.to_string());
        bookmarks.load("/proj");
        let data = bookmarks.store.data();
        assert_eq!(data.lists, [list(DEFAULT_LIST_ID, "proj")]);
        assert_eq!(data.items[0].list.as_deref(), Some(DEFAULT_LIST_ID));
    }

    #[test]
    fn persists_lists_the_default_and_file_bookmarks_new_line_bookmarks_join_the_default_list() {
        let mut bookmarks = Harness::default();
        bookmarks.load("/q");
        let mut id = String::new();
        let writes = bookmarks.store.update_data(|data| {
            let (created, new_id) = create_list(data, "Two");
            id = new_id;
            toggle_path_bookmarks(&set_default_list(&created, &id), &[("/q/f", false)], NOW)
        });
        bookmarks.save(writes);
        let writes = bookmarks.store.update(|list| toggle_bookmark(list, "/q/a", 0, "x", NOW, None));
        bookmarks.save(writes);
        bookmarks.load("/other");
        bookmarks.load("/q");
        let data = bookmarks.store.data();
        assert_eq!(data.lists, [list(DEFAULT_LIST_ID, "q"), list(&id, "Two")]);
        assert_eq!(data.default_list, id);
        let paths: Vec<(&str, &str)> = data.paths.iter().map(|path| (path.path.as_str(), path.list.as_str())).collect();
        assert_eq!(paths, [("/q/f", id.as_str())]);
        let items: Vec<(&str, Option<&str>)> =
            data.items.iter().map(|item| (item.path.as_str(), item.list.as_deref())).collect();
        assert_eq!(items, [("/q/a", Some(id.as_str()))]);
        // Line bookmarks stay readable by versions without lists: the same key, a plain list.
        let stored: Value = serde_json::from_str(&bookmarks.storage[&bookmarks_key("/q")]).unwrap();
        assert_eq!(stored.as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn ignores_corrupt_list_storage() {
        let mut bookmarks = Harness::default();
        let stored = json!({ "lists": [{ "id": 3 }], "defaultList": "x", "paths": [1] });
        bookmarks.storage.insert(lists_key("/c"), stored.to_string());
        bookmarks.load("/c");
        let data = bookmarks.store.data();
        assert_eq!(data.lists, [list(DEFAULT_LIST_ID, "c")]);
        assert_eq!(data.default_list, DEFAULT_LIST_ID);
        assert!(data.paths.is_empty());
    }

    // Not from a spec: the stored shapes, key by key, and that a change that changes nothing, or one made
    // without a project, saves nothing.
    #[test]
    fn stored_shapes_and_what_is_saved() {
        let mut bookmarks = Harness::default();
        assert!(bookmarks.store.update(|list| toggle_bookmark(list, "/x/a", 0, "x", NOW, None)).is_empty());
        assert_eq!(places(&bookmarks.store.data().items), ["/x/a:0"]);
        bookmarks.load("/q");
        assert_eq!(bookmarks.store.scope(), Some("/q"));
        assert!(bookmarks.store.update_data(BookmarkData::clone).is_empty());
        let writes = bookmarks.store.update(|list| assign_mnemonic(list, "/q/a", 4, " x ", Some('B'), Some("why"), 7));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].key, "openidea.bookmarks.v1:/q");
        let items: Value = serde_json::from_str(&writes[0].value).unwrap();
        let expected = json!([{
            "path": "/q/a",
            "line": 4,
            "text": "x",
            "created": 7,
            "mnemonic": "B",
            "description": "why",
            "list": "default",
        }]);
        assert_eq!(items, expected);
        let writes = bookmarks.store.update_data(|data| toggle_path_bookmarks(data, &[("/q/src", true)], 8));
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].key, "openidea.bookmarkLists.v1:/q");
        let lists: Value = serde_json::from_str(&writes[0].value).unwrap();
        let expected = json!({
            "lists": [{ "id": "default", "name": "q" }],
            "defaultList": "default",
            "paths": [{ "path": "/q/src", "dir": true, "list": "default", "created": 8 }],
        });
        assert_eq!(lists, expected);
        bookmarks.store.load(None, None, None);
        assert_eq!(bookmarks.store, BookmarkStore::default());
    }

    // Not from a spec: descriptions and line text are cut to 200 UTF-16 code units, and only single
    // characters 0-9 / A-Z are mnemonics.
    #[test]
    fn long_text_is_clipped_and_mnemonics_are_single_characters() {
        let long = format!("  {}😀tail", "x".repeat(199));
        let list = toggle_bookmark(&[], "/p/a", 0, &long, NOW, Some(&long));
        assert_eq!(list[0].text, "x".repeat(199));
        assert_eq!(list[0].description.as_deref(), Some("x".repeat(199).as_str()));
        assert!(MNEMONICS.iter().all(|&c| is_mnemonic(c)));
        assert!(!is_mnemonic('a'));
        assert!(!is_mnemonic('-'));
        let stored = json!([
            { "path": "/p/a", "line": 1, "mnemonic": "a" },
            { "path": "/p/b", "line": 2.5 },
            { "path": "/p/c", "line": -3, "text": 5, "created": "12", "description": " ", "list": "" },
        ]);
        let parsed = parse_bookmarks(Some(&stored.to_string()));
        assert_eq!(parsed, [bookmark("/p/a", 1, "", 0), bookmark("/p/c", 0, "5", 12)]);
    }

    // Not from a spec: new lists get the first free id, and a name is only taken by another list.
    #[test]
    fn new_lists_get_the_first_free_id() {
        let (one, first) = create_list(&data(), "One");
        let (two, second) = create_list(&one, "Two");
        assert_eq!((first.as_str(), second.as_str()), ("list2", "list3"));
        let without_first = delete_list(&two, &first, "p");
        assert_eq!(create_list(&without_first, "Three").1, "list4");
        let taken = validate_list_name(&two.lists, " One ", Some(second.as_str()));
        assert_eq!(taken, Some(ListNameError::Duplicate("One".to_owned())));
        assert_eq!(set_default_list(&two, "nope"), two);
    }
}
