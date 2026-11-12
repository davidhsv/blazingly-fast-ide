//! The editor session of a project: its open tabs with their pinned and preview flags, the
//! active tab of every pane, the split layout, the active pane, and the caret and scroll
//! position per file. Tabs of editor kinds that ask to be restored are saved with their whole
//! input. A session is plain JSON stored per project folder; restoring one builds a fresh
//! [`TabsState`] with new ids.

use super::tabs::{EditorInput, LayoutNode, Pane, SplitDir, Tab, TabsState};
use crate::platform::paths::ipc_path;
use crate::platform::storage::UiStorage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

/// What the storage key of a project's session starts with; the project folder follows.
pub const STORAGE_PREFIX: &str = "openidea.editorSession.v1:";

/// A file tab (`path`), or a tab of a restorable editor kind (`input`, with its JSON data).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTab {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<EditorInput>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preview: bool,
}

impl SessionTab {
    /// What makes two saved tabs the same tab.
    fn key(&self) -> String {
        match (&self.path, &self.input) {
            (Some(path), _) => format!("file:{path}"),
            (None, Some(input)) => format!("input:{}", input.key),
            (None, None) => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionDir {
    Row,
    Column,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SessionLayout {
    Pane {
        tabs: Vec<SessionTab>,
        /// Index of the active tab.
        active: usize,
    },
    Split {
        dir: SessionDir,
        children: Vec<SessionLayout>,
    },
}

/// Where a file's editor was: the selection in UTF-16 offsets and the first visible line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewState {
    pub anchor: u32,
    pub head: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession {
    pub version: u32,
    pub layout: SessionLayout,
    /// Index of the active pane in visual (depth-first) order.
    pub active_pane: usize,
    pub files: BTreeMap<String, FileViewState>,
}

fn is_file_tab(tab: &Tab) -> bool {
    tab.input.kind == "file" && tab.input.path.as_deref().is_some_and(|path| !path.is_empty())
}

/// The session of `state`, with the view state of its files from `files`. None when no tab is
/// worth saving. `restorable` says whether a tab that is not a file is saved (its kind asked
/// for it).
pub fn serialize_session(
    state: &TabsState,
    files: &BTreeMap<String, FileViewState>,
    restorable: impl Fn(&EditorInput) -> bool,
) -> Option<EditorSession> {
    let saved = |tab: &Tab| is_file_tab(tab) || (tab.input.kind != "file" && restorable(&tab.input));
    fn walk(state: &TabsState, node: &LayoutNode, saved: &dyn Fn(&Tab) -> bool) -> Option<SessionLayout> {
        match node {
            LayoutNode::Pane { pane_id } => {
                let pane = state.pane(pane_id)?;
                let kept: Vec<&Tab> = pane.tabs.iter().filter(|tab| saved(tab)).collect();
                if kept.is_empty() {
                    return None;
                }
                let active = kept.iter().position(|tab| Some(&tab.id) == pane.active.as_ref()).unwrap_or(0);
                let tabs = kept
                    .into_iter()
                    .map(|tab| SessionTab {
                        path: is_file_tab(tab).then(|| tab.input.path.clone()).flatten(),
                        input: (!is_file_tab(tab)).then(|| tab.input.clone()),
                        pinned: tab.pinned,
                        preview: tab.preview,
                    })
                    .collect();
                Some(SessionLayout::Pane { tabs, active })
            }
            LayoutNode::Split { dir, children, .. } => {
                let mut children: Vec<SessionLayout> =
                    children.iter().filter_map(|child| walk(state, child, saved)).collect();
                match children.len() {
                    0 => None,
                    1 => children.pop(),
                    _ => {
                        let dir = match dir {
                            SplitDir::Row => SessionDir::Row,
                            SplitDir::Column => SessionDir::Column,
                        };
                        Some(SessionLayout::Split { dir, children })
                    }
                }
            }
        }
    }
    let layout = walk(state, &state.layout, &saved)?;
    // The active pane's index among the panes that are saved.
    let kept: Vec<&str> = state
        .pane_ids()
        .into_iter()
        .filter(|id| state.pane(id).is_some_and(|pane| pane.tabs.iter().any(&saved)))
        .collect();
    let active_pane = kept.iter().position(|id| *id == state.active_pane).unwrap_or(0);
    fn paths<'a>(layout: &'a SessionLayout, out: &mut BTreeSet<&'a str>) {
        match layout {
            SessionLayout::Pane { tabs, .. } => out.extend(tabs.iter().filter_map(|tab| tab.path.as_deref())),
            SessionLayout::Split { children, .. } => children.iter().for_each(|child| paths(child, out)),
        }
    }
    let mut open = BTreeSet::new();
    paths(&layout, &mut open);
    let files = open.into_iter().filter_map(|path| Some((path.to_owned(), *files.get(path)?))).collect();
    Some(EditorSession { version: 1, layout, active_pane, files })
}

fn session_tab(raw: &Value) -> Option<SessionTab> {
    let flag = |name: &str| raw.get(name).and_then(Value::as_bool).unwrap_or(false);
    if let Some(path) = raw.get("path").and_then(Value::as_str) {
        return (!path.is_empty()).then(|| SessionTab {
            path: Some(ipc_path(path)),
            input: None,
            pinned: flag("pinned"),
            preview: flag("preview"),
        });
    }
    let input: EditorInput = serde_json::from_value(raw.get("input")?.clone()).ok()?;
    (input.kind != "file" && !input.key.is_empty()).then(|| SessionTab {
        path: None,
        input: Some(input),
        pinned: flag("pinned"),
        preview: flag("preview"),
    })
}

/// How deep a stored layout may nest before it is taken for junk.
const MAX_DEPTH: usize = 16;

fn checked_layout(raw: &Value, depth: usize) -> Option<SessionLayout> {
    if depth > MAX_DEPTH {
        return None;
    }
    match raw.get("kind").and_then(Value::as_str)? {
        "pane" => {
            let tabs: Vec<SessionTab> =
                raw.get("tabs").and_then(Value::as_array).into_iter().flatten().filter_map(session_tab).collect();
            if tabs.is_empty() {
                return None;
            }
            let active = raw
                .get("active")
                .and_then(Value::as_f64)
                .filter(|active| *active >= 0.0 && *active < tabs.len() as f64)
                .map_or(0, |active| active.floor() as usize);
            Some(SessionLayout::Pane { tabs, active })
        }
        "split" => {
            let dir = match raw.get("dir").and_then(Value::as_str)? {
                "row" => SessionDir::Row,
                "column" => SessionDir::Column,
                _ => return None,
            };
            let mut children: Vec<SessionLayout> = raw
                .get("children")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(|child| checked_layout(child, depth + 1))
                .collect();
            match children.len() {
                0 => None,
                1 => children.pop(),
                _ => Some(SessionLayout::Split { dir, children }),
            }
        }
        _ => None,
    }
}

/// A session from stored JSON, which is not trusted: tabs and view states that make no sense
/// are dropped, Windows paths saved with backslashes are normalised. None when nothing usable
/// is left.
pub fn parse_session(raw: &Value) -> Option<EditorSession> {
    if raw.get("version").and_then(Value::as_u64) != Some(1) {
        return None;
    }
    let layout = checked_layout(raw.get("layout")?, 0)?;
    let offset =
        |state: &Value, name: &str| state.get(name).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    let files = raw
        .get("files")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(path, state)| {
            let (anchor, head) = (offset(state, "anchor")?, offset(state, "head")?);
            Some((ipc_path(path), FileViewState { anchor, head, top_line: offset(state, "topLine") }))
        })
        .collect();
    let active_pane = raw.get("activePane").and_then(Value::as_f64).map_or(0, |index| index.max(0.0).floor() as usize);
    Some(EditorSession { version: 1, layout, active_pane, files })
}

/// A fresh id for a restored pane, tab or split: never one a [`TabsState`] generates itself.
fn restored_id(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!("{prefix}r{}", SEQ.fetch_add(1, Ordering::Relaxed) + 1)
}

/// Rebuilds the tabs of `session`, with fresh ids. `file_key` gives the tab key of a file.
pub fn restore_tabs_state(session: &EditorSession, file_key: impl Fn(&str) -> String) -> TabsState {
    fn build(layout: &SessionLayout, file_key: &dyn Fn(&str) -> String, panes: &mut Vec<Pane>) -> LayoutNode {
        match layout {
            SessionLayout::Pane { tabs: saved, active } => {
                let pane_id = restored_id("pane");
                let mut seen = BTreeSet::new();
                let mut tabs: Vec<(String, Tab)> = Vec::new();
                let mut has_preview = false;
                for tab in saved {
                    let key = tab.key();
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    // At most one preview tab per pane, and never a pinned one.
                    let preview = tab.preview && !tab.pinned && !has_preview;
                    has_preview |= preview;
                    let input = match (&tab.path, &tab.input) {
                        (Some(path), _) => EditorInput {
                            kind: "file".to_owned(),
                            key: file_key(path),
                            path: Some(path.clone()),
                            data: None,
                        },
                        (None, Some(input)) => input.clone(),
                        (None, None) => continue,
                    };
                    tabs.push((key, Tab { id: restored_id("tab"), input, pinned: tab.pinned, preview }));
                }
                // Pinned tabs first, the rest in their saved order.
                tabs.sort_by_key(|(_, tab)| !tab.pinned);
                let active_key = saved.get(*active).map(SessionTab::key);
                let active = tabs
                    .iter()
                    .find(|(key, _)| Some(key) == active_key.as_ref())
                    .or(tabs.first())
                    .map(|(_, tab)| tab.id.clone());
                let tabs: Vec<Tab> = tabs.into_iter().map(|(_, tab)| tab).collect();
                let mru = active
                    .iter()
                    .cloned()
                    .chain(tabs.iter().filter(|tab| Some(&tab.id) != active.as_ref()).map(|tab| tab.id.clone()))
                    .collect();
                panes.push(Pane { id: pane_id.clone(), tabs, active, mru });
                LayoutNode::Pane { pane_id }
            }
            SessionLayout::Split { dir, children } => LayoutNode::Split {
                id: restored_id("split"),
                dir: match dir {
                    SessionDir::Row => SplitDir::Row,
                    SessionDir::Column => SplitDir::Column,
                },
                children: children.iter().map(|child| build(child, file_key, panes)).collect(),
            },
        }
    }
    let mut panes = Vec::new();
    let layout = build(&session.layout, &file_key, &mut panes);
    let Some(active) = panes.get(session.active_pane.min(panes.len().saturating_sub(1))) else {
        return TabsState::new();
    };
    let active_pane = active.id.clone();
    TabsState::from_parts(layout, panes, active_pane)
}

/// The session stored for the project at `root`. One saved under the backslash spelling of a
/// Windows folder is found too.
pub fn load_session(storage: &UiStorage, root: &str) -> Option<EditorSession> {
    let legacy = || {
        let bytes = root.as_bytes();
        let drive = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
        let native = drive.then(|| root.replace('/', "\\"))?;
        storage
            .get(&format!("{STORAGE_PREFIX}{native}"))
            .or_else(|| storage.get(&format!(r"{STORAGE_PREFIX}\\?\{native}")))
    };
    let raw = storage.get(&format!("{STORAGE_PREFIX}{root}")).or_else(legacy)?;
    parse_session(&serde_json::from_str(raw).ok()?)
}

/// Stores `session` for the project at `root`, or forgets what was stored when there is none.
pub fn store_session(storage: &mut UiStorage, root: &str, session: Option<&EditorSession>) {
    let key = format!("{STORAGE_PREFIX}{root}");
    match session.map(serde_json::to_string) {
        Some(Ok(json)) => storage.set(&key, json),
        Some(Err(error)) => tracing::warn!("cannot store the editor session: {error}"),
        None => storage.remove(&key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tabs::OpenTabOptions;
    use serde_json::json;

    fn file(path: &str) -> EditorInput {
        EditorInput { kind: "file".to_owned(), key: format!("file:{path}"), path: Some(path.to_owned()), data: None }
    }

    fn other(kind: &str, key: &str, data: Value) -> EditorInput {
        EditorInput { kind: kind.to_owned(), key: key.to_owned(), path: None, data: Some(data) }
    }

    fn file_key(path: &str) -> String {
        format!("file:{path}")
    }

    fn open(state: &mut TabsState, path: &str) {
        let pane = state.active_pane.clone();
        state.open_tab(&pane, file(path), OpenTabOptions::default());
    }

    fn paths(state: &TabsState, pane: &str) -> Vec<String> {
        state.pane(pane).expect("the pane").tabs.iter().filter_map(|tab| tab.input.path.clone()).collect()
    }

    fn reparsed(session: &EditorSession) -> EditorSession {
        let json = serde_json::to_string(session).expect("serializes");
        parse_session(&serde_json::from_str(&json).expect("JSON")).expect("parses back")
    }

    #[test]
    fn round_trips_tabs_flags_splits_active_pane_and_carets() {
        let mut s = TabsState::new();
        open(&mut s, "/p/a.ts");
        open(&mut s, "/p/b.ts");
        let first = s.active_pane.clone();
        let b = s.pane(&first).expect("the pane").tabs[1].id.clone();
        s.toggle_pin(&first, &b);
        s.open_tab(&first, other("diff", "diff:x", json!({ "big": true })), OpenTabOptions::default());
        // A regular tab opened later would close the preview: it opens in the background, last.
        s.open_tab(&first, file("/p/c.ts"), OpenTabOptions { preview: true, activate: false, ..Default::default() });
        s.split_pane(&first, SplitDir::Column).expect("a second pane");
        open(&mut s, "/p/d.ts");
        let views = BTreeMap::from([
            ("/p/a.ts".to_owned(), FileViewState { anchor: 3, head: 5, top_line: Some(10) }),
            ("/p/zzz.ts".to_owned(), FileViewState { anchor: 0, head: 0, top_line: None }),
        ]);
        let session = serialize_session(&s, &views, |_| false).expect("a session");
        assert!(matches!(session.layout, SessionLayout::Split { dir: SessionDir::Column, .. }));
        assert_eq!(session.active_pane, 1);
        assert_eq!(session.files.keys().collect::<Vec<_>>(), ["/p/a.ts"]);

        let r = restore_tabs_state(&reparsed(&session), file_key);
        let ids: Vec<String> = r.pane_ids().into_iter().map(str::to_owned).collect();
        let [p1, p2] = ids.as_slice() else { panic!("two panes, got {ids:?}") };
        assert_eq!(paths(&r, p1), paths(&s, &first));
        let flags: Vec<(Option<&str>, bool, bool)> =
            r.pane(p1).expect("the pane").tabs.iter().map(|t| (t.input.path.as_deref(), t.pinned, t.preview)).collect();
        assert_eq!(
            flags,
            [(Some("/p/b.ts"), true, false), (Some("/p/c.ts"), false, true), (Some("/p/a.ts"), false, false)]
        );
        // The diff tab is not saved; the second pane holds the file opened after the split.
        assert_eq!(paths(&r, p2), ["/p/d.ts"]);
        assert_eq!(&r.active_pane, p2);
        assert!(matches!(r.layout, LayoutNode::Split { dir: SplitDir::Column, .. }));
    }

    #[test]
    fn saves_tabs_of_restorable_editor_kinds_with_their_input() {
        let mut s = TabsState::new();
        open(&mut s, "/p/a.ts");
        let pane = s.active_pane.clone();
        let agent = other("agent-session", "agent:1", json!({ "task": "1" }));
        s.open_tab(&pane, agent.clone(), OpenTabOptions::default());
        s.open_tab(&pane, other("diff", "diff:x", json!({ "big": true })), OpenTabOptions::default());
        s.open_tab(&pane, agent.clone(), OpenTabOptions::default());
        let restorable = |input: &EditorInput| input.kind == "agent-session";
        let session = serialize_session(&s, &BTreeMap::new(), restorable).expect("a session");
        assert!(matches!(session.layout, SessionLayout::Pane { active: 1, .. }));
        let r = restore_tabs_state(&reparsed(&session), file_key);
        let restored = r.pane(r.pane_ids()[0]).expect("the pane");
        assert_eq!(restored.tabs.iter().map(|t| t.input.clone()).collect::<Vec<_>>(), [file("/p/a.ts"), agent]);
        assert_eq!(restored.active.as_ref(), Some(&restored.tabs[1].id));

        // Only restorable tabs is still a session; without a kind asking for it, it is none.
        let mut only = TabsState::new();
        let pane = only.active_pane.clone();
        only.open_tab(&pane, other("agent-session", "agent:2", json!({})), OpenTabOptions::default());
        assert!(serialize_session(&only, &BTreeMap::new(), restorable).is_some());
        assert!(serialize_session(&only, &BTreeMap::new(), |_| false).is_none());
        // Junk inputs (a file kind, no key) are dropped.
        let junk = parse_session(&json!({
            "version": 1,
            "layout": {
                "kind": "pane",
                "tabs": [{ "input": { "kind": "file", "key": "k" } }, { "input": { "kind": "x" } }, { "input": { "kind": "x", "key": "y" } }],
                "active": 0,
            },
        }))
        .expect("one tab is usable");
        let kept = EditorInput { kind: "x".to_owned(), key: "y".to_owned(), path: None, data: None };
        let tab = SessionTab { path: None, input: Some(kept), pinned: false, preview: false };
        assert_eq!(junk.layout, SessionLayout::Pane { tabs: vec![tab], active: 0 });
    }

    #[test]
    fn returns_none_for_sessions_without_file_tabs_and_rejects_junk() {
        assert!(serialize_session(&TabsState::new(), &BTreeMap::new(), |_| false).is_none());
        assert!(parse_session(&json!({ "version": 2 })).is_none());
        assert!(
            parse_session(&json!({ "version": 1, "layout": { "kind": "pane", "tabs": [{ "nope": 1 }], "active": 0 } }))
                .is_none()
        );
        let ok = parse_session(&json!({
            "version": 1,
            "activePane": 7,
            "layout": {
                "kind": "split",
                "dir": "row",
                "children": [{ "kind": "pane", "tabs": [{ "path": "/x" }], "active": 9 }, { "kind": "bogus" }],
            },
            "files": { "/x": { "anchor": 1, "head": -1 } },
        }))
        .expect("the pane is usable");
        let tab = SessionTab { path: Some("/x".to_owned()), input: None, pinned: false, preview: false };
        assert_eq!(ok.layout, SessionLayout::Pane { tabs: vec![tab], active: 0 });
        assert!(ok.files.is_empty());
        assert_eq!(restore_tabs_state(&ok, file_key).pane_ids().len(), 1);
    }

    #[test]
    fn loads_the_old_key_and_normalises_tab_and_caret_paths() {
        let old = json!({
            "version": 1,
            "layout": { "kind": "pane", "tabs": [{ "path": r"C:\p\a.ts" }, { "path": r"\\?\C:\p\b.ts" }], "active": 1 },
            "activePane": 0,
            "files": { r"C:\p\a.ts": { "anchor": 1, "head": 2 } },
        });
        let mut storage = UiStorage::default();
        storage.set(r"openidea.editorSession.v1:C:\p", old.to_string());
        let session = load_session(&storage, "C:/p").expect("found under the old key");
        let tab = |path: &str| SessionTab { path: Some(path.to_owned()), input: None, pinned: false, preview: false };
        assert_eq!(session.layout, SessionLayout::Pane { tabs: vec![tab("C:/p/a.ts"), tab("C:/p/b.ts")], active: 1 });
        let view = FileViewState { anchor: 1, head: 2, top_line: None };
        assert_eq!(session.files, BTreeMap::from([("C:/p/a.ts".to_owned(), view)]));
        assert!(load_session(&storage, "/p").is_none());
    }

    // Not from a spec: what is stored is what the webview stores, and storing none forgets it.
    #[test]
    fn a_stored_session_has_the_webviews_json_shape() {
        let mut s = TabsState::new();
        open(&mut s, "/p/a.ts");
        let pane = s.active_pane.clone();
        let a = s.pane(&pane).expect("the pane").tabs[0].id.clone();
        s.toggle_pin(&pane, &a);
        let views = BTreeMap::from([("/p/a.ts".to_owned(), FileViewState { anchor: 3, head: 5, top_line: Some(10) })]);
        let session = serialize_session(&s, &views, |_| false).expect("a session");
        let mut storage = UiStorage::default();
        store_session(&mut storage, "/p", Some(&session));
        let stored: Value =
            serde_json::from_str(storage.get("openidea.editorSession.v1:/p").expect("stored")).expect("JSON");
        assert_eq!(
            stored,
            json!({
                "version": 1,
                "layout": { "kind": "pane", "tabs": [{ "path": "/p/a.ts", "pinned": true }], "active": 0 },
                "activePane": 0,
                "files": { "/p/a.ts": { "anchor": 3, "head": 5, "topLine": 10 } },
            })
        );
        assert_eq!(load_session(&storage, "/p"), Some(session));
        store_session(&mut storage, "/p", None);
        assert!(storage.get("openidea.editorSession.v1:/p").is_none());
    }
}
