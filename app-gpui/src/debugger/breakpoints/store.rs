//! Line breakpoints of the open project: stored per project in the UI storage and mirrored to the
//! backend whenever a file's list changes. The backend keeps them across sessions and returns
//! verification states.
//!
//! [`BreakpointStore`] does no I/O. What a change asks for (the storage write, the backend call)
//! collects in the store and is handed out by [`BreakpointStore::take_effects`]. Edits reach the
//! backend after [`SYNC_DELAY_MS`] without another edit to the same file: the caller wakes the
//! store at [`BreakpointStore::next_deadline`] with [`BreakpointStore::sync_due`].

use super::model::{
    BreakpointEntry, BreakpointProps, BreakpointStatus, make_breakpoint, new_breakpoint_id, sort_by_line, to_backend,
    toggle_line,
};
use crate::debugger::effects::{BackendCall, Effect};
use ide_core::IdeError;
use ide_dap::BreakpointState;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Storage key prefix of a project's breakpoints; the project root follows.
pub const STORAGE_PREFIX: &str = "openidea.debugger.breakpoints:";
/// How long a file's breakpoints stay unsent after an edit, in milliseconds.
pub const SYNC_DELAY_MS: f64 = 120.0;

/// The key a project's breakpoints are stored under.
pub fn storage_key(root: &str) -> String {
    format!("{STORAGE_PREFIX}{root}")
}

/// Whether a stored JSON value counts as set, the way the webview read it.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// A stored line number: a whole, non-negative number.
fn stored_line(value: Option<&Value>) -> Option<u32> {
    let number = value?.as_f64()?;
    (number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&number)).then_some(number as u32)
}

/// The breakpoints of a stored list, ordered by line; entries that are not breakpoints are dropped.
pub fn sanitize(raw: &Value) -> Vec<BreakpointEntry> {
    let Some(items) = raw.as_array() else { return Vec::new() };
    let entries = items.iter().filter_map(|item| {
        let b = item.as_object()?;
        let line = stored_line(b.get("line"))?;
        let log_message = text(b.get("logMessage"));
        // Before "Suspend" was its own option, a log message meant "log instead of suspending".
        let logs_only = !b.contains_key("suspend") && log_message.as_deref().is_some_and(|m| !m.trim().is_empty());
        Some(BreakpointEntry {
            id: text(b.get("id")).unwrap_or_else(new_breakpoint_id),
            line,
            enabled: b.get("enabled") != Some(&Value::Bool(false)),
            condition: text(b.get("condition")),
            log_message,
            hit_condition: text(b.get("hitCondition")),
            temporary: b.get("temporary") == Some(&Value::Bool(true)),
            suspend: !(b.get("suspend") == Some(&Value::Bool(false)) || logs_only),
            log_hit: b.get("logHit") == Some(&Value::Bool(true)),
        })
    });
    sort_by_line(entries.collect())
}

/// A breakpoint as it is stored: the optional flags only when they are set.
fn stored_entry(b: &BreakpointEntry) -> Value {
    let mut entry = Map::new();
    entry.insert("id".to_owned(), json!(b.id));
    entry.insert("line".to_owned(), json!(b.line));
    entry.insert("enabled".to_owned(), json!(b.enabled));
    entry.insert("condition".to_owned(), json!(b.condition));
    entry.insert("logMessage".to_owned(), json!(b.log_message));
    entry.insert("hitCondition".to_owned(), json!(b.hit_condition));
    if b.temporary {
        entry.insert("temporary".to_owned(), json!(true));
    }
    if !b.suspend {
        entry.insert("suspend".to_owned(), json!(false));
    }
    if b.log_hit {
        entry.insert("logHit".to_owned(), json!(true));
    }
    Value::Object(entry)
}

/// What starting a flush came to.
#[derive(Debug)]
pub enum Flush {
    /// Everything is with the backend already.
    Done,
    /// Sends are on their way: the [`BreakpointStore::sync_finished`] that settles the last one
    /// reports how the flush ended.
    Pending,
}

#[derive(Debug, Clone, Default)]
pub struct BreakpointStore {
    /// Project root the breakpoints belong to (storage key).
    root: Option<String>,
    by_file: BTreeMap<String, Vec<BreakpointEntry>>,
    /// Verification state by breakpoint id.
    status: HashMap<String, BreakpointStatus>,
    muted: bool,
    /// When each edited file is sent, in milliseconds since the epoch.
    timers: BTreeMap<String, f64>,
    /// Ids in the order last sent per file (backend states are index-aligned).
    sent: HashMap<String, Vec<String>>,
    /// Files with a send under way, and how many more sends wait behind it.
    in_flight: BTreeMap<String, u32>,
    /// The running flush: per file, the sends it still waits for.
    flush: Option<BTreeMap<String, u32>>,
    effects: Vec<Effect>,
}

impl BreakpointStore {
    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn by_file(&self) -> &BTreeMap<String, Vec<BreakpointEntry>> {
        &self.by_file
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn status(&self, id: &str) -> Option<&BreakpointStatus> {
        self.status.get(id)
    }

    /// The breakpoints of a file, ordered by line.
    pub fn breakpoints_of(&self, path: &str) -> &[BreakpointEntry] {
        self.by_file.get(path).map_or(&[], Vec::as_slice)
    }

    /// The breakpoint on `line` of `path`.
    pub fn at_line(&self, path: &str, line: u32) -> Option<&BreakpointEntry> {
        self.breakpoints_of(path).iter().find(|b| b.line == line)
    }

    /// What the changes since the last call ask for, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// The JSON stored for the project: the mute flag and the files that have breakpoints.
    pub fn serialized(&self) -> String {
        let files: Map<String, Value> = self
            .by_file
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(path, list)| (path.clone(), Value::Array(list.iter().map(stored_entry).collect())))
            .collect();
        json!({ "muted": self.muted, "files": files }).to_string()
    }

    fn persist(&mut self) {
        if let Some(root) = &self.root {
            self.effects.push(Effect::StorageSet { key: storage_key(root), value: self.serialized() });
        }
    }

    /// Loads the breakpoints of the project at `root` from `stored` (the value under
    /// [`storage_key`]) and schedules all of them, and the files that just lost theirs, for the backend.
    pub fn load(&mut self, root: Option<&str>, stored: Option<&str>, now_ms: f64) {
        let mut by_file = BTreeMap::new();
        let mut muted = false;
        let data = root.and(stored).and_then(|stored| serde_json::from_str::<Value>(stored).ok());
        if let Some(Value::Object(data)) = data {
            muted = data.get("muted").is_some_and(truthy);
            if let Some(Value::Object(files)) = data.get("files") {
                for (path, list) in files {
                    let clean = sanitize(list);
                    if !clean.is_empty() {
                        by_file.insert(path.clone(), clean);
                    }
                }
            }
        }
        let touched: BTreeSet<String> = self.by_file.keys().chain(by_file.keys()).cloned().collect();
        self.root = root.map(str::to_owned);
        self.by_file = by_file;
        self.muted = muted;
        self.status.clear();
        for path in touched {
            self.schedule_sync(&path, now_ms);
        }
    }

    // --- backend sync ---------------------------------------------------------

    /// Sends `path` once [`SYNC_DELAY_MS`] pass without another edit to it.
    pub fn schedule_sync(&mut self, path: &str, now_ms: f64) {
        self.timers.insert(path.to_owned(), now_ms + SYNC_DELAY_MS);
    }

    /// When [`BreakpointStore::sync_due`] has something to send next.
    pub fn next_deadline(&self) -> Option<f64> {
        self.timers.values().copied().reduce(f64::min)
    }

    /// Sends the files whose delay has passed. `can_sync`: a backend is attached and the project is
    /// trusted; without that nothing is sent.
    pub fn sync_due(&mut self, now_ms: f64, can_sync: bool) {
        let due: Vec<String> =
            self.timers.iter().filter(|(_, deadline)| **deadline <= now_ms).map(|(path, _)| path.clone()).collect();
        for path in due {
            self.timers.remove(&path);
            self.sync_file(&path, can_sync);
        }
    }

    fn send(&mut self, path: &str) {
        let list = self.by_file.get(path).map_or(&[][..], Vec::as_slice);
        self.sent.insert(path.to_owned(), list.iter().map(|b| b.id.clone()).collect());
        let breakpoints = to_backend(list, self.muted, path);
        self.effects.push(Effect::Call(BackendCall::DebugSetBreakpoints { path: path.to_owned(), breakpoints }));
    }

    /// Sends the file's list now, or right after the send of it that is under way.
    pub fn sync_file(&mut self, path: &str, can_sync: bool) {
        if let Some(queued) = self.in_flight.get_mut(path) {
            *queued += 1;
        } else if can_sync {
            self.in_flight.insert(path.to_owned(), 0);
            self.send(path);
        }
    }

    /// How many answers for `path` are outstanding.
    fn outstanding(&self, path: &str) -> u32 {
        self.in_flight.get(path).map_or(0, |queued| queued + 1)
    }

    /// Drains the edits not yet with the backend, before it takes its startup snapshot of the
    /// breakpoints: every edited file is sent now. A flush already running is joined.
    pub fn flush(&mut self, can_sync: bool) -> Flush {
        if self.flush.is_some() {
            return Flush::Pending;
        }
        let paths: BTreeSet<String> =
            self.timers.keys().chain(self.in_flight.keys()).chain(self.by_file.keys()).cloned().collect();
        self.flush_round(paths, can_sync)
    }

    fn flush_round(&mut self, paths: BTreeSet<String>, can_sync: bool) -> Flush {
        self.timers.clear();
        let mut waiting = BTreeMap::new();
        for path in paths {
            self.sync_file(&path, can_sync);
            let outstanding = self.outstanding(&path);
            if outstanding > 0 {
                waiting.insert(path, outstanding);
            }
        }
        if waiting.is_empty() {
            self.flush = None;
            Flush::Done
        } else {
            self.flush = Some(waiting);
            Flush::Pending
        }
    }

    /// The backend answered a send of `path`. Returns how the running flush ended, when this
    /// answer settles it: `Err` with the failure of a send the flush made.
    pub fn sync_finished(
        &mut self,
        path: &str,
        result: Result<Vec<BreakpointState>, IdeError>,
        can_sync: bool,
    ) -> Option<Result<(), IdeError>> {
        let failure = match result {
            Ok(states) => {
                self.apply_states(path, &states);
                None
            }
            Err(error) => Some(error),
        };
        match self.in_flight.get_mut(path) {
            Some(queued) if *queued > 0 => {
                *queued -= 1;
                self.send(path);
            }
            _ => {
                self.in_flight.remove(path);
            }
        }
        let waiting = self.flush.as_mut()?;
        let remaining = waiting.get_mut(path)?;
        *remaining -= 1;
        if *remaining > 0 {
            return None;
        }
        waiting.remove(path);
        // The send that just answered is the one the flush made for this file.
        if let Some(error) = failure {
            self.flush = None;
            return Some(Err(error));
        }
        if !waiting.is_empty() {
            return None;
        }
        // Edits made meanwhile, and sends still under way, belong to the same drain.
        let paths: BTreeSet<String> = self.timers.keys().chain(self.in_flight.keys()).cloned().collect();
        match self.flush_round(paths, can_sync) {
            Flush::Done => Some(Ok(())),
            Flush::Pending => None,
        }
    }

    /// Asks the backend for the verification states of `path` again (after it reported a change).
    pub fn refresh_states(&mut self, path: &str) {
        self.effects.push(Effect::Call(BackendCall::DebugBreakpointStates { path: path.to_owned() }));
    }

    /// Takes the backend's states of `path`, aligned with the list last sent.
    pub fn apply_states(&mut self, path: &str, states: &[BreakpointState]) {
        let current = || self.by_file.get(path).into_iter().flatten().map(|b| b.id.clone()).collect();
        let ids: Vec<String> = self.sent.get(path).cloned().unwrap_or_else(current);
        for (id, state) in ids.into_iter().zip(states) {
            let status =
                BreakpointStatus { verified: state.verified, message: state.message.clone(), line: state.line };
            self.status.insert(id, status);
        }
    }

    /// Marks every breakpoint unverified (no session is running).
    pub fn reset_statuses(&mut self) {
        self.status.clear();
    }

    // --- mutations ------------------------------------------------------------

    fn set_file(&mut self, path: &str, list: Vec<BreakpointEntry>, now_ms: f64) {
        if self.breakpoints_of(path) == list.as_slice() {
            return;
        }
        if list.is_empty() {
            self.by_file.remove(path);
        } else {
            self.by_file.insert(path.to_owned(), list);
        }
        self.persist();
        self.schedule_sync(path, now_ms);
    }

    pub fn toggle_breakpoint(&mut self, path: &str, line: u32, now_ms: f64) {
        let list = toggle_line(self.breakpoints_of(path), line);
        self.set_file(path, list, now_ms);
    }

    /// Puts a breakpoint with `props` on `line`, replacing the one there.
    pub fn add_breakpoint(&mut self, path: &str, line: u32, props: BreakpointProps, now_ms: f64) -> BreakpointEntry {
        let mut list: Vec<BreakpointEntry> =
            self.breakpoints_of(path).iter().filter(|b| b.line != line).cloned().collect();
        let bp = make_breakpoint(line, props);
        list.push(bp.clone());
        self.set_file(path, sort_by_line(list), now_ms);
        bp
    }

    /// Changes the breakpoint `id` of `path` with `change`.
    pub fn update_breakpoint(&mut self, path: &str, id: &str, now_ms: f64, change: impl FnOnce(&mut BreakpointEntry)) {
        let mut list = self.breakpoints_of(path).to_vec();
        if let Some(b) = list.iter_mut().find(|b| b.id == id) {
            change(b);
        }
        self.set_file(path, sort_by_line(list), now_ms);
    }

    pub fn remove_breakpoint(&mut self, path: &str, id: &str, now_ms: f64) {
        let list = self.breakpoints_of(path).iter().filter(|b| b.id != id).cloned().collect();
        self.set_file(path, list, now_ms);
    }

    pub fn remove_all_breakpoints(&mut self, now_ms: f64) {
        let paths: Vec<String> = self.by_file.keys().cloned().collect();
        for path in paths {
            self.set_file(&path, Vec::new(), now_ms);
        }
    }

    /// Replaces a file's list after its lines moved with an edit (from the editor gutter).
    pub fn set_file_breakpoints(&mut self, path: &str, list: Vec<BreakpointEntry>, now_ms: f64) {
        self.set_file(path, list, now_ms);
    }

    /// A suspend at `path:line` hit the temporary breakpoint there: remove it, like IDEA.
    /// Returns whether one was removed.
    pub fn consume_temporary(&mut self, path: &str, line: u32, now_ms: f64) -> bool {
        let hit = self.breakpoints_of(path).iter().find(|b| b.temporary && b.line == line && b.enabled);
        let Some(id) = hit.map(|b| b.id.clone()) else { return false };
        self.remove_breakpoint(path, &id, now_ms);
        true
    }

    /// Toggles a temporary breakpoint on `line`; an existing one there is removed.
    pub fn toggle_temporary_breakpoint(&mut self, path: &str, line: u32, now_ms: f64) {
        if self.at_line(path, line).is_some() {
            let list = self.breakpoints_of(path).iter().filter(|b| b.line != line).cloned().collect();
            self.set_file(path, list, now_ms);
        } else {
            self.add_breakpoint(path, line, BreakpointProps { temporary: true, ..Default::default() }, now_ms);
        }
    }

    /// Mutes or unmutes every breakpoint; the stored flags of each stay as they are.
    pub fn set_muted(&mut self, muted: bool, now_ms: f64) {
        if self.muted == muted {
            return;
        }
        self.muted = muted;
        self.persist();
        let paths: Vec<String> = self.by_file.keys().cloned().collect();
        for path in paths {
            self.schedule_sync(&path, now_ms);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// The UI storage of a test: what [`Effect::StorageSet`] wrote.
    #[derive(Debug, Default)]
    pub struct Storage(pub HashMap<String, String>);

    impl Storage {
        /// Carries out the storage writes among `effects`.
        pub fn apply(&mut self, effects: &[Effect]) {
            for effect in effects {
                if let Effect::StorageSet { key, value } = effect {
                    self.0.insert(key.clone(), value.clone());
                }
            }
        }

        pub fn get(&self, key: &str) -> Option<&str> {
            self.0.get(key).map(String::as_str)
        }
    }

    /// The paths of the breakpoint lists `effects` send to the backend, with each list's lines.
    pub fn sent(effects: &[Effect]) -> Vec<(String, Vec<u32>)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Call(BackendCall::DebugSetBreakpoints { path, breakpoints }) => {
                    Some((path.clone(), breakpoints.iter().map(|b| b.line).collect()))
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Storage, sent};
    use super::*;

    fn lines(store: &BreakpointStore, path: &str) -> Vec<u32> {
        store.breakpoints_of(path).iter().map(|b| b.line).collect()
    }

    /// A store of project `root`, loaded from `storage`.
    fn loaded(root: &str, storage: &Storage) -> BreakpointStore {
        let mut store = BreakpointStore::default();
        store.load(Some(root), storage.get(&storage_key(root)), 0.0);
        store
    }

    fn save(store: &mut BreakpointStore, storage: &mut Storage) {
        storage.apply(&store.take_effects());
    }

    #[test]
    fn migrates_stored_log_messages_to_non_suspending_breakpoints() {
        let mut storage = Storage::default();
        let list = json!([{ "id": "a", "line": 1, "logMessage": "v={v}" }, { "id": "b", "line": 2 }]);
        storage.0.insert(storage_key("/old"), json!({ "files": { "/x": list } }).to_string());
        let store = loaded("/old", &storage);
        let list = store.breakpoints_of("/x");
        assert!(!list[0].suspend);
        assert_eq!(list[0].log_message.as_deref(), Some("v={v}"));
        assert!(list[1].suspend);
    }

    #[test]
    fn toggles_edits_and_removes_per_file() {
        let mut store = loaded("/proj", &Storage::default());
        store.toggle_breakpoint("/proj/a.py", 4, 0.0);
        store.toggle_breakpoint("/proj/a.py", 1, 0.0);
        store.toggle_breakpoint("/proj/b.py", 0, 0.0);
        assert_eq!(lines(&store, "/proj/a.py"), [1, 4]);
        let id = store.breakpoints_of("/proj/a.py")[1].id.clone();
        store.update_breakpoint("/proj/a.py", &id, 0.0, |b| {
            b.condition = Some("i == 3".to_owned());
            b.enabled = false;
        });
        let edited = &store.breakpoints_of("/proj/a.py")[1];
        assert_eq!((edited.condition.as_deref(), edited.enabled, edited.line), (Some("i == 3"), false, 4));
        store.remove_breakpoint("/proj/a.py", &id, 0.0);
        assert_eq!(lines(&store, "/proj/a.py"), [1]);
        store.toggle_breakpoint("/proj/a.py", 1, 0.0);
        assert!(!store.by_file().contains_key("/proj/a.py"));
    }

    #[test]
    fn adds_a_conditional_breakpoint_replacing_one_on_the_same_line() {
        let mut store = loaded("/proj", &Storage::default());
        store.toggle_breakpoint("/proj/a.py", 2, 0.0);
        let props = BreakpointProps { log_message: Some("hit {i}".to_owned()), ..Default::default() };
        store.add_breakpoint("/proj/a.py", 2, props, 0.0);
        let list = store.breakpoints_of("/proj/a.py");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].log_message.as_deref(), Some("hit {i}"));
    }

    #[test]
    fn persists_per_project_including_mute() {
        let mut storage = Storage::default();
        let mut store = loaded("/proj", &storage);
        store.toggle_breakpoint("/proj/a.py", 7, 0.0);
        store.set_muted(true, 0.0);
        save(&mut store, &mut storage);
        store.load(Some("/other"), storage.get(&storage_key("/other")), 0.0);
        assert!(store.by_file().is_empty());
        store.load(Some("/proj"), storage.get(&storage_key("/proj")), 0.0);
        assert_eq!(lines(&store, "/proj/a.py"), [7]);
        assert!(store.muted());
    }

    #[test]
    fn ignores_corrupt_storage() {
        let mut storage = Storage::default();
        storage.0.insert(storage_key("/bad"), "{nope".to_owned());
        assert!(loaded("/bad", &storage).by_file().is_empty());
        let stored = json!({ "files": { "/x": [{ "line": "a" }, { "line": 3 }] } });
        storage.0.insert(storage_key("/bad2"), stored.to_string());
        assert_eq!(lines(&loaded("/bad2", &storage), "/x"), [3]);
    }

    // src/features/debugger/__tests__/special.test.ts › temporary line breakpoints
    #[test]
    fn temporary_breakpoints_are_removed_after_the_first_hit() {
        let mut store = loaded("/p", &Storage::default());
        store.toggle_temporary_breakpoint("/p/a.py", 7, 0.0);
        let bp = &store.breakpoints_of("/p/a.py")[0];
        assert!(bp.line == 7 && bp.temporary);
        assert!(!store.consume_temporary("/p/a.py", 6, 0.0));
        assert!(store.consume_temporary("/p/a.py", 7, 0.0));
        assert!(store.breakpoints_of("/p/a.py").is_empty());
    }

    // src/features/debugger/__tests__/special.test.ts › temporary line breakpoints
    #[test]
    fn temporary_breakpoints_survive_a_reload_and_toggle_off() {
        let mut storage = Storage::default();
        let mut store = loaded("/p", &storage);
        store.toggle_temporary_breakpoint("/p/a.py", 2, 0.0);
        save(&mut store, &mut storage);
        let mut store = loaded("/p", &storage);
        assert!(store.breakpoints_of("/p/a.py")[0].temporary);
        store.toggle_temporary_breakpoint("/p/a.py", 2, 0.0);
        assert!(store.breakpoints_of("/p/a.py").is_empty());
    }

    // src/features/debugger/__tests__/startupBreakpoints.test.ts
    #[test]
    fn does_not_resend_unchanged_breakpoints_for_concurrent_flushes() {
        let mut store = loaded("/p", &Storage::default());
        store.add_breakpoint("/p/Main.java", 4, BreakpointProps::default(), 0.0);
        assert!(matches!(store.flush(true), Flush::Pending));
        assert!(matches!(store.flush(true), Flush::Pending));
        assert!(matches!(store.flush(true), Flush::Pending));
        assert_eq!(sent(&store.take_effects()), [("/p/Main.java".to_owned(), vec![4])]);
        assert!(matches!(store.sync_finished("/p/Main.java", Ok(Vec::new()), true), Some(Ok(()))));
        assert!(sent(&store.take_effects()).is_empty());
        assert_eq!(store.next_deadline(), None);
    }

    // Not from a spec: an edit reaches the backend once its delay has passed, and the states that
    // come back belong to the breakpoints in the order they were sent.
    #[test]
    fn sends_an_edited_file_after_the_delay_and_keeps_its_states() {
        let mut store = loaded("/p", &Storage::default());
        store.toggle_breakpoint("/p/a.py", 3, 1000.0);
        store.toggle_breakpoint("/p/a.py", 5, 1050.0);
        assert_eq!(store.next_deadline(), Some(1050.0 + SYNC_DELAY_MS));
        store.sync_due(1100.0, true);
        assert!(sent(&store.take_effects()).is_empty());
        store.sync_due(1170.0, true);
        assert_eq!(sent(&store.take_effects()), [("/p/a.py".to_owned(), vec![3, 5])]);
        let states = vec![
            BreakpointState { line: 3, verified: true, message: None },
            BreakpointState { line: 6, verified: false, message: Some("moved".to_owned()) },
        ];
        assert!(store.sync_finished("/p/a.py", Ok(states), true).is_none());
        let second = store.breakpoints_of("/p/a.py")[1].id.clone();
        let status = store.status(&second).expect("the state of the second breakpoint");
        assert_eq!((status.verified, status.line, status.message.as_deref()), (false, 6, Some("moved")));
        store.reset_statuses();
        assert!(store.status(&second).is_none());
    }

    // Not from a spec: an untrusted project, or a window without a backend, sends nothing.
    #[test]
    fn sends_nothing_when_the_backend_takes_no_breakpoints() {
        let mut store = loaded("/p", &Storage::default());
        store.toggle_breakpoint("/p/a.py", 3, 0.0);
        store.sync_due(500.0, false);
        assert!(matches!(store.flush(false), Flush::Done));
        assert!(sent(&store.take_effects()).is_empty());
    }

    // Not from a spec: the stored JSON keeps its shape, with the optional flags only when set.
    #[test]
    fn stores_the_optional_flags_only_when_set() {
        let mut store = loaded("/p", &Storage::default());
        let props = BreakpointProps { suspend: Some(false), log_hit: true, temporary: true, ..Default::default() };
        let special = store.add_breakpoint("/p/a.py", 2, props, 0.0);
        let plain = store.add_breakpoint("/p/a.py", 1, BreakpointProps::default(), 0.0);
        let stored: Value = serde_json::from_str(&store.serialized()).expect("valid JSON");
        assert_eq!(
            stored,
            json!({ "muted": false, "files": { "/p/a.py": [
                { "id": plain.id, "line": 1, "enabled": true, "condition": null, "logMessage": null,
                  "hitCondition": null },
                { "id": special.id, "line": 2, "enabled": true, "condition": null, "logMessage": null,
                  "hitCondition": null, "temporary": true, "suspend": false, "logHit": true },
            ] } }),
        );
    }
}
