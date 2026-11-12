//! Method breakpoints (DAP function breakpoints) and field watchpoints (DAP data breakpoints).
//! Both are project-wide lists, stored like line breakpoints and mirrored to the backend, which
//! sends them to every session whose adapter supports them. Entries remember the source line they
//! were created on for the gutter icon.
//!
//! Like the line breakpoint store, [`SpecialBreakpoints`] collects what its changes ask for
//! ([`SpecialBreakpoints::take_effects`]) and sends its lists after [`SYNC_DELAY_MS`] without a
//! further change.

use super::model::{BreakpointStatus, new_breakpoint_id};
use crate::debugger::effects::{BackendCall, Effect};
use ide_core::IdeError;
use ide_dap::{BreakpointState, DataBreakpoint, FunctionBreakpoint};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// Storage key prefix of a project's method breakpoints and watchpoints; the project root follows.
pub const STORAGE_PREFIX: &str = "openidea.debugger.specialBreakpoints:";
/// How long the lists stay unsent after a change, in milliseconds.
pub const SYNC_DELAY_MS: f64 = 120.0;

/// The key a project's method breakpoints and watchpoints are stored under.
pub fn storage_key(root: &str) -> String {
    format!("{STORAGE_PREFIX}{root}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodBreakpoint {
    pub id: String,
    /// Function name in the adapter's syntax (`pkg.Class#method`, `main.run`, `parse`).
    pub name: String,
    /// Source location (gutter icon, Go to Source).
    pub path: Option<String>,
    pub line: Option<u32>,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
}

/// Which accesses of a field suspend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Write,
    Read,
    ReadWrite,
}

impl AccessType {
    /// The DAP name, which is also what is stored.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Read => "read",
            Self::ReadWrite => "readWrite",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Self::Write, Self::Read, Self::ReadWrite].into_iter().find(|access| access.as_str() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldWatchpoint {
    pub id: String,
    pub data_id: String,
    /// What the user sees (`Order.total`).
    pub label: String,
    pub access_type: Option<AccessType>,
    /// Source location; `None` for watchpoints made from a value.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub enabled: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKind {
    Method,
    Field,
}

/// A method breakpoint or a field watchpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecialBreakpoint {
    Method(MethodBreakpoint),
    Field(FieldWatchpoint),
}

impl SpecialBreakpoint {
    pub fn kind(&self) -> SpecialKind {
        match self {
            Self::Method(_) => SpecialKind::Method,
            Self::Field(_) => SpecialKind::Field,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Method(m) => &m.id,
            Self::Field(f) => &f.id,
        }
    }

    pub fn line(&self) -> Option<u32> {
        match self {
            Self::Method(m) => m.line,
            Self::Field(f) => f.line,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Method(m) => m.enabled,
            Self::Field(f) => f.enabled,
        }
    }
}

/// Changes to a method breakpoint or watchpoint; `None` leaves a property as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecialPatch {
    pub enabled: Option<bool>,
    pub condition: Option<Option<String>>,
    pub hit_condition: Option<Option<String>>,
    /// Method breakpoints only.
    pub name: Option<String>,
    /// Watchpoints only.
    pub access_type: Option<Option<AccessType>>,
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// The properties both kinds store, read leniently.
struct Common {
    id: String,
    path: Option<String>,
    line: Option<u32>,
    enabled: bool,
    condition: Option<String>,
    hit_condition: Option<String>,
}

fn common(raw: &Map<String, Value>) -> Common {
    let whole = |n: f64| n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n);
    Common {
        id: text(raw.get("id")).unwrap_or_else(new_breakpoint_id),
        path: text(raw.get("path")),
        line: raw.get("line").and_then(Value::as_f64).filter(|n| whole(*n)).map(|n| n as u32),
        enabled: raw.get("enabled") != Some(&Value::Bool(false)),
        condition: text(raw.get("condition")),
        hit_condition: text(raw.get("hitCondition")),
    }
}

fn records(value: Option<&Value>) -> impl Iterator<Item = &Map<String, Value>> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

/// The method breakpoints and watchpoints of stored data; entries that are neither are dropped.
pub fn sanitize_special(raw: &Value) -> (Vec<MethodBreakpoint>, Vec<FieldWatchpoint>) {
    let methods = records(raw.get("methods"))
        .filter_map(|m| {
            let name = text(m.get("name")).filter(|name| !name.trim().is_empty())?;
            let c = common(m);
            Some(MethodBreakpoint {
                id: c.id,
                name,
                path: c.path,
                line: c.line,
                enabled: c.enabled,
                condition: c.condition,
                hit_condition: c.hit_condition,
            })
        })
        .collect();
    let fields = records(raw.get("fields"))
        .filter_map(|f| {
            let data_id = text(f.get("dataId")).filter(|id| !id.is_empty())?;
            let c = common(f);
            Some(FieldWatchpoint {
                id: c.id,
                label: text(f.get("label")).unwrap_or_else(|| data_id.clone()),
                data_id,
                access_type: f.get("accessType").and_then(Value::as_str).and_then(AccessType::parse),
                path: c.path,
                line: c.line,
                enabled: c.enabled,
                condition: c.condition,
                hit_condition: c.hit_condition,
            })
        })
        .collect();
    (methods, fields)
}

fn trimmed(text: &Option<String>) -> Option<String> {
    text.as_deref().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// The backend list; muting disables everything without touching the stored flags.
pub fn to_function_breakpoints(list: &[MethodBreakpoint], muted: bool) -> Vec<FunctionBreakpoint> {
    list.iter()
        .map(|m| FunctionBreakpoint {
            name: m.name.trim().to_owned(),
            condition: trimmed(&m.condition),
            hit_condition: trimmed(&m.hit_condition),
            enabled: m.enabled && !muted,
        })
        .collect()
}

pub fn to_data_breakpoints(list: &[FieldWatchpoint], muted: bool) -> Vec<DataBreakpoint> {
    list.iter()
        .map(|f| DataBreakpoint {
            data_id: f.data_id.clone(),
            access_type: f.access_type.map(|access| access.as_str().to_owned()),
            condition: trimmed(&f.condition),
            hit_condition: trimmed(&f.hit_condition),
            enabled: f.enabled && !muted,
        })
        .collect()
}

/// The ids and gutter lines of the lists a sync sent, to match the states that come back.
#[derive(Debug, Clone, Default)]
struct Sent {
    methods: Vec<(String, u32)>,
    fields: Vec<(String, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct SpecialBreakpoints {
    root: Option<String>,
    methods: Vec<MethodBreakpoint>,
    fields: Vec<FieldWatchpoint>,
    status: HashMap<String, BreakpointStatus>,
    /// When the lists are sent, in milliseconds since the epoch.
    sync_at: Option<f64>,
    sent: Sent,
    effects: Vec<Effect>,
}

impl SpecialBreakpoints {
    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn methods(&self) -> &[MethodBreakpoint] {
        &self.methods
    }

    pub fn fields(&self) -> &[FieldWatchpoint] {
        &self.fields
    }

    pub fn status(&self, id: &str) -> Option<&BreakpointStatus> {
        self.status.get(id)
    }

    /// Marks every method breakpoint and watchpoint unverified (no session is running).
    pub fn reset_statuses(&mut self) {
        self.status.clear();
    }

    /// What the changes since the last call ask for, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// The JSON stored for the project.
    pub fn serialized(&self) -> String {
        let methods: Vec<Value> = self
            .methods
            .iter()
            .map(|m| {
                json!({
                    "id": m.id, "kind": "method", "name": m.name, "path": m.path, "line": m.line,
                    "enabled": m.enabled, "condition": m.condition, "hitCondition": m.hit_condition,
                })
            })
            .collect();
        let fields: Vec<Value> = self
            .fields
            .iter()
            .map(|f| {
                json!({
                    "id": f.id, "kind": "field", "dataId": f.data_id, "label": f.label,
                    "accessType": f.access_type.map(AccessType::as_str), "path": f.path, "line": f.line,
                    "enabled": f.enabled, "condition": f.condition, "hitCondition": f.hit_condition,
                })
            })
            .collect();
        json!({ "methods": methods, "fields": fields }).to_string()
    }

    /// Loads the project's lists from `stored` (the value under [`storage_key`]) and schedules
    /// them for the backend.
    pub fn load(&mut self, root: Option<&str>, stored: Option<&str>, now_ms: f64) {
        let data = root.and(stored).and_then(|stored| serde_json::from_str::<Value>(stored).ok());
        let (methods, fields) = data.as_ref().map(sanitize_special).unwrap_or_default();
        self.root = root.map(str::to_owned);
        self.methods = methods;
        self.fields = fields;
        self.status.clear();
        self.schedule_sync(now_ms);
    }

    /// Sends the lists once [`SYNC_DELAY_MS`] pass without another change (also after muting,
    /// which applies to them too).
    pub fn schedule_sync(&mut self, now_ms: f64) {
        self.sync_at = Some(now_ms + SYNC_DELAY_MS);
    }

    /// When [`SpecialBreakpoints::sync_due`] has something to send.
    pub fn next_deadline(&self) -> Option<f64> {
        self.sync_at
    }

    /// Sends the lists when their delay has passed.
    pub fn sync_due(&mut self, now_ms: f64, can_sync: bool, muted: bool) {
        if self.sync_at.is_some_and(|deadline| deadline <= now_ms) {
            self.sync_at = None;
            self.sync(can_sync, muted);
        }
    }

    /// Sends both lists now. `can_sync`: a backend is attached and the project is trusted; `muted`:
    /// Mute Breakpoints is on. The answers come back through [`SpecialBreakpoints::synced`].
    pub fn sync(&mut self, can_sync: bool, muted: bool) {
        if !can_sync || self.root.is_none() {
            return;
        }
        let line = |line: Option<u32>| line.unwrap_or(0);
        self.sent = Sent {
            methods: self.methods.iter().map(|m| (m.id.clone(), line(m.line))).collect(),
            fields: self.fields.iter().map(|f| (f.id.clone(), line(f.line))).collect(),
        };
        let functions = to_function_breakpoints(&self.methods, muted);
        let data = to_data_breakpoints(&self.fields, muted);
        self.effects.push(Effect::Call(BackendCall::DebugSetFunctionBreakpoints(functions)));
        self.effects.push(Effect::Call(BackendCall::DebugSetDataBreakpoints(data)));
    }

    /// The backend answered both calls of a sync; a failure of either leaves the states as they were.
    pub fn synced(
        &mut self,
        methods: Result<Vec<BreakpointState>, IdeError>,
        fields: Result<Vec<BreakpointState>, IdeError>,
    ) {
        let (Ok(methods), Ok(fields)) = (methods, fields) else { return };
        let sent = std::mem::take(&mut self.sent);
        for ((id, line), state) in sent.methods.into_iter().zip(methods).chain(sent.fields.into_iter().zip(fields)) {
            self.status.insert(id, BreakpointStatus { verified: state.verified, message: state.message, line });
        }
    }

    fn changed(&mut self, now_ms: f64) {
        if let Some(root) = &self.root {
            self.effects.push(Effect::StorageSet { key: storage_key(root), value: self.serialized() });
        }
        self.schedule_sync(now_ms);
    }

    /// Adds a breakpoint on the method `name`; `at`: the declaration's file and line.
    pub fn add_method_breakpoint(&mut self, name: &str, at: Option<(&str, u32)>, now_ms: f64) -> MethodBreakpoint {
        let bp = MethodBreakpoint {
            id: new_breakpoint_id(),
            name: name.trim().to_owned(),
            path: at.map(|(path, _)| path.to_owned()),
            line: at.map(|(_, line)| line),
            enabled: true,
            condition: None,
            hit_condition: None,
        };
        self.methods.push(bp.clone());
        self.changed(now_ms);
        bp
    }

    /// Adds a watchpoint on the field `data_id`, or returns the one there is.
    pub fn add_field_watchpoint(
        &mut self,
        data_id: &str,
        label: &str,
        access_type: Option<AccessType>,
        at: Option<(&str, u32)>,
        now_ms: f64,
    ) -> FieldWatchpoint {
        if let Some(existing) = self.fields.iter().find(|f| f.data_id == data_id) {
            return existing.clone();
        }
        let bp = FieldWatchpoint {
            id: new_breakpoint_id(),
            data_id: data_id.to_owned(),
            label: label.to_owned(),
            access_type,
            path: at.map(|(path, _)| path.to_owned()),
            line: at.map(|(_, line)| line),
            enabled: true,
            condition: None,
            hit_condition: None,
        };
        self.fields.push(bp.clone());
        self.changed(now_ms);
        bp
    }

    /// Changes the method breakpoint or watchpoint `id`.
    pub fn update_special(&mut self, id: &str, patch: SpecialPatch, now_ms: f64) {
        if let Some(m) = self.methods.iter_mut().find(|m| m.id == id) {
            m.enabled = patch.enabled.unwrap_or(m.enabled);
            if let Some(condition) = patch.condition.clone() {
                m.condition = condition;
            }
            if let Some(hit_condition) = patch.hit_condition.clone() {
                m.hit_condition = hit_condition;
            }
            if let Some(name) = patch.name.clone() {
                m.name = name;
            }
        }
        if let Some(f) = self.fields.iter_mut().find(|f| f.id == id) {
            f.enabled = patch.enabled.unwrap_or(f.enabled);
            if let Some(condition) = patch.condition {
                f.condition = condition;
            }
            if let Some(hit_condition) = patch.hit_condition {
                f.hit_condition = hit_condition;
            }
            if let Some(access_type) = patch.access_type {
                f.access_type = access_type;
            }
        }
        self.changed(now_ms);
    }

    pub fn remove_special(&mut self, id: &str, now_ms: f64) {
        self.methods.retain(|m| m.id != id);
        self.fields.retain(|f| f.id != id);
        self.changed(now_ms);
    }

    pub fn remove_all_special(&mut self, now_ms: f64) {
        self.methods.clear();
        self.fields.clear();
        self.changed(now_ms);
    }

    /// Method breakpoints / watchpoints whose gutter icon is in `path`: methods first.
    pub fn special_at(&self, path: &str) -> Vec<SpecialBreakpoint> {
        let here = |at: &Option<String>, line: Option<u32>| at.as_deref() == Some(path) && line.is_some();
        let methods = self.methods.iter().filter(|m| here(&m.path, m.line)).cloned().map(SpecialBreakpoint::Method);
        let fields = self.fields.iter().filter(|f| here(&f.path, f.line)).cloned().map(SpecialBreakpoint::Field);
        methods.chain(fields).collect()
    }

    /// The method breakpoint or watchpoint whose gutter icon is on `line` of `path`.
    pub fn special_on_line(&self, path: &str, line: u32) -> Option<SpecialBreakpoint> {
        self.special_at(path).into_iter().find(|b| b.line() == Some(line))
    }

    /// Toggles the method breakpoint on `path:line` (adds `name` when there is none).
    pub fn toggle_method_at(&mut self, path: &str, line: u32, name: &str, now_ms: f64) -> Option<MethodBreakpoint> {
        let existing = self.methods.iter().find(|m| m.path.as_deref() == Some(path) && m.line == Some(line));
        if let Some(id) = existing.map(|m| m.id.clone()) {
            self.remove_special(&id, now_ms);
            return None;
        }
        Some(self.add_method_breakpoint(name, Some((path, line)), now_ms))
    }

    /// Toggles the watchpoint on `path:line` (adds one on writes of `data_id` when there is none).
    pub fn toggle_field_at(
        &mut self,
        path: &str,
        line: u32,
        data_id: &str,
        label: &str,
        now_ms: f64,
    ) -> Option<FieldWatchpoint> {
        let existing = self.fields.iter().find(|f| f.path.as_deref() == Some(path) && f.line == Some(line));
        if let Some(id) = existing.map(|f| f.id.clone()) {
            self.remove_special(&id, now_ms);
            return None;
        }
        Some(self.add_field_watchpoint(data_id, label, Some(AccessType::Write), Some((path, line)), now_ms))
    }

    /// Moves gutter lines after an edit in `path`: `moved` are the ids that still have a line,
    /// with it. One that is missing was merged away by the edit and loses its place in the file.
    pub fn set_special_lines(&mut self, path: &str, moved: &[(String, u32)], now_ms: f64) {
        let new_line = |id: &str| moved.iter().find(|(moved_id, _)| moved_id == id).map(|(_, line)| *line);
        let mut changed = false;
        let mut touch = |id: &str, at: &mut Option<String>, line: &mut Option<u32>| {
            if at.as_deref() != Some(path) {
                return;
            }
            let next = new_line(id);
            if next.is_none() {
                *at = None;
            }
            changed |= next.is_none() || *line != next;
            *line = next;
        };
        for m in &mut self.methods {
            touch(&m.id, &mut m.path, &mut m.line);
        }
        for f in &mut self.fields {
            touch(&f.id, &mut f.path, &mut f.line);
        }
        if changed {
            self.changed(now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::breakpoints::model::map_line_numbers;
    use crate::debugger::breakpoints::model::testing::edit;
    use crate::debugger::breakpoints::store::testing::Storage;
    use ropey::Rope;

    fn loaded(storage: &Storage) -> SpecialBreakpoints {
        let mut special = SpecialBreakpoints::default();
        special.load(Some("/p"), storage.get(&storage_key("/p")), 0.0);
        special
    }

    #[test]
    fn toggles_a_method_breakpoint_on_its_declaration_line_and_persists_it() {
        let mut storage = Storage::default();
        let mut special = loaded(&storage);
        let bp = special.toggle_method_at("/p/A.java", 4, "a.A#run", 0.0);
        assert_eq!(bp.map(|bp| bp.name).as_deref(), Some("a.A#run"));
        let lines: Vec<Option<u32>> = special.special_at("/p/A.java").iter().map(SpecialBreakpoint::line).collect();
        assert_eq!(lines, [Some(4)]);
        storage.apply(&special.take_effects());
        let mut special = loaded(&storage);
        assert_eq!(special.methods().iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["a.A#run"]);
        assert_eq!(special.toggle_method_at("/p/A.java", 4, "a.A#run", 0.0), None);
        assert!(special.special_at("/p/A.java").is_empty());
    }

    #[test]
    fn sends_enabled_state_conditions_and_muting_to_the_backend_lists() {
        let mut special = loaded(&Storage::default());
        let m = special.add_method_breakpoint("main.run", None, 0.0);
        let condition = Some(Some(" n > 1 ".to_owned()));
        special.update_special(&m.id, SpecialPatch { condition, enabled: Some(false), ..Default::default() }, 0.0);
        let f = special.add_field_watchpoint("a.A#count", "A.count", Some(AccessType::Write), None, 0.0);
        assert_eq!(special.add_field_watchpoint("a.A#count", "dup", None, None, 0.0).id, f.id);
        assert_eq!(
            to_function_breakpoints(special.methods(), false),
            [FunctionBreakpoint {
                name: "main.run".to_owned(),
                condition: Some("n > 1".to_owned()),
                hit_condition: None,
                enabled: false
            }],
        );
        assert_eq!(
            to_data_breakpoints(special.fields(), false),
            [DataBreakpoint {
                data_id: "a.A#count".to_owned(),
                access_type: Some("write".to_owned()),
                condition: None,
                hit_condition: None,
                enabled: true
            }],
        );
        assert!(!to_data_breakpoints(special.fields(), true)[0].enabled);
    }

    #[test]
    fn follows_edits_and_drops_sanitized_junk() {
        let mut special = loaded(&Storage::default());
        let m = special.toggle_method_at("/p/a.py", 3, "run", 0.0).expect("a new method breakpoint");
        let doc = Rope::from_str("a\nb\nc\ndef run():\n  pass");
        let (changes, next) = edit(&doc, 0, 0, "x\ny\n");
        let moved: Vec<(String, u32)> =
            map_line_numbers(&[3], &doc, &changes, &next).into_iter().map(|(_, line)| (m.id.clone(), line)).collect();
        special.set_special_lines("/p/a.py", &moved, 0.0);
        assert_eq!(special.special_at("/p/a.py")[0].line(), Some(5));
        let methods = json!([{ "name": "" }, { "name": "ok", "line": 2 }, 3]);
        let junk = json!({ "methods": methods, "fields": [{ "label": "no id" }] });
        let (methods, fields) = sanitize_special(&junk);
        assert!(fields.is_empty());
        assert_eq!(methods.len(), 1);
        assert_eq!((methods[0].name.as_str(), methods[0].line, methods[0].enabled), ("ok", Some(2), true));
    }

    // Not from a spec: a sync sends both lists once the delay has passed, and the states that come
    // back belong to the entries in the order sent, at their gutter lines.
    #[test]
    fn sends_both_lists_after_the_delay_and_keeps_their_states() {
        let mut special = loaded(&Storage::default());
        let m = special.add_method_breakpoint("main.run", Some(("/p/main.go", 7)), 1000.0);
        special.take_effects();
        special.sync_due(1100.0, true, false);
        assert!(special.take_effects().is_empty());
        special.sync_due(1120.0, true, false);
        let effects = special.take_effects();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Call(BackendCall::DebugSetFunctionBreakpoints(functions)),
                Effect::Call(BackendCall::DebugSetDataBreakpoints(data)),
            ] if functions.len() == 1 && data.is_empty()
        ));
        assert_eq!(special.next_deadline(), None);
        special.synced(Ok(vec![BreakpointState { line: 0, verified: true, message: None }]), Ok(Vec::new()));
        assert_eq!(special.status(&m.id), Some(&BreakpointStatus { verified: true, message: None, line: 7 }),);
        special.sync(false, false);
        assert!(special.take_effects().is_empty());
    }

    // Not from a spec: an entry whose line an edit merged away keeps existing without a gutter icon.
    #[test]
    fn an_entry_merged_away_by_an_edit_loses_its_place() {
        let mut special = loaded(&Storage::default());
        let m = special.toggle_method_at("/p/a.py", 3, "run", 0.0).expect("a new method breakpoint");
        special.set_special_lines("/p/a.py", &[], 0.0);
        assert!(special.special_at("/p/a.py").is_empty());
        assert_eq!(special.methods()[0].id, m.id);
        assert_eq!((special.methods()[0].path.as_deref(), special.methods()[0].line), (None, None));
    }
}
