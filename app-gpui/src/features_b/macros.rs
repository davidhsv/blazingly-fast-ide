//! Edit › Macros (IDEA's ActionMacroManager): a macro is the list of actions the user ran plus
//! what they typed and the editing / caret keys they pressed in the editor. The last recording
//! and the named macros persist in the UI storage under [`STORAGE_KEY`] (per user, like live
//! templates), as the same JSON the other shell writes.
//!
//! Playback is a step machine: [`MacroStore::start_playback`] hands out a [`Playback`] whose
//! steps the caller runs one at a time (letting the UI settle in between), then
//! [`MacroStore::finish_playback`] ends it. Nothing is recorded while a macro plays.

use crate::database::model::js_number_string;
use crate::platform::storage::UiStorage;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use serde_json::{Map, Value, json};

/// The storage key of the last macro and the saved macros.
pub const STORAGE_KEY: &str = "openidea.macros.v1";

/// One recorded step. An action's `arg` is plain data (see [`storable_arg`]); a key is replayed
/// as the editor command [`EDITOR_KEYS`] names.
#[derive(Debug, Clone, PartialEq)]
pub enum MacroStep {
    Action { id: String, arg: Option<Value> },
    Text { text: String },
    Key { key: String },
}

impl MacroStep {
    fn to_json(&self) -> Value {
        match self {
            Self::Action { id, arg: None } => json!({ "kind": "action", "id": id }),
            Self::Action { id, arg: Some(arg) } => json!({ "kind": "action", "id": id, "arg": arg }),
            Self::Text { text } => json!({ "kind": "text", "text": text }),
            Self::Key { key } => json!({ "kind": "key", "key": key }),
        }
    }

    /// A stored step; `None` for anything malformed.
    fn from_json(value: &Value) -> Option<Self> {
        let step = value.as_object()?;
        let text = |field: &str| step.get(field).and_then(Value::as_str).map(str::to_owned);
        match step.get("kind").and_then(Value::as_str)? {
            "action" => {
                let id = text("id")?;
                match step.get("arg") {
                    None => Some(Self::Action { id, arg: None }),
                    Some(arg) if is_macro_arg(arg) => Some(Self::Action { id, arg: Some(arg.clone()) }),
                    Some(_) => None,
                }
            }
            "text" => Some(Self::Text { text: text("text")? }),
            "key" => Some(Self::Key { key: text("key")? }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Macro {
    pub name: String,
    pub steps: Vec<MacroStep>,
}

/// A string, a boolean, a finite number or a list of strings.
fn is_macro_value(value: &Value) -> bool {
    match value {
        Value::String(_) | Value::Bool(_) => true,
        Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
        Value::Array(items) => items.iter().all(Value::is_string),
        Value::Null | Value::Object(_) => false,
    }
}

/// A macro value, or a flat object of macro values.
fn is_macro_arg(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.values().all(is_macro_value),
        other => is_macro_value(other),
    }
}

/// What storing an action's argument comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum StorableArg {
    /// The action ran without an argument.
    None,
    /// Plain data, stored with the step.
    Plain(Value),
    /// Not plain data (a menu's context): the step is left out of the recording.
    Unstorable,
}

/// An action's argument when it can be stored (plain data only).
pub fn storable_arg(arg: Option<&Value>) -> StorableArg {
    match arg {
        None => StorableArg::None,
        Some(arg) if is_macro_arg(arg) => StorableArg::Plain(arg.clone()),
        Some(_) => StorableArg::Unstorable,
    }
}

fn steps_from(value: Option<&Value>) -> Option<Vec<MacroStep>> {
    value.and_then(Value::as_array).map(|items| items.iter().filter_map(MacroStep::from_json).collect())
}

fn steps_json(steps: &[MacroStep]) -> Value {
    Value::Array(steps.iter().map(MacroStep::to_json).collect())
}

/// Recording, playback and the persisted macros.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacroStore {
    /// Steps recorded so far, while recording.
    pub recording: Option<Vec<MacroStep>>,
    /// A macro is being played back.
    pub playing: bool,
    /// The last recorded macro (Play Back Last Macro).
    pub last: Option<Vec<MacroStep>>,
    pub saved: Vec<Macro>,
}

impl MacroStore {
    /// Reads the persisted macros; a missing or malformed value is no macros, and malformed
    /// steps and unnamed macros are dropped.
    pub fn load(storage: &UiStorage) -> Self {
        let raw = storage.get(STORAGE_KEY).and_then(|text| serde_json::from_str::<Value>(text).ok());
        let raw = raw.as_ref().and_then(Value::as_object);
        let saved = raw
            .and_then(|raw| raw.get("saved"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let name = item.get("name").and_then(Value::as_str).filter(|name| !name.trim().is_empty())?;
                        Some(Macro { name: name.to_owned(), steps: steps_from(item.get("steps")).unwrap_or_default() })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { recording: None, playing: false, last: steps_from(raw.and_then(|raw| raw.get("last"))), saved }
    }

    fn persist(&self, storage: &mut UiStorage) {
        let saved: Vec<Value> =
            self.saved.iter().map(|m| json!({ "name": m.name, "steps": steps_json(&m.steps) })).collect();
        let mut value = Map::new();
        value.insert("last".to_owned(), self.last.as_deref().map_or(Value::Null, steps_json));
        value.insert("saved".to_owned(), Value::Array(saved));
        storage.set(STORAGE_KEY, Value::Object(value).to_string());
    }

    /// The context keys the macro actions' `when` clauses read.
    pub fn context(&self) -> [(&'static str, bool); 3] {
        [
            ("macroRecording", self.recording.is_some()),
            ("macroLastAvailable", self.last.as_ref().is_some_and(|last| !last.is_empty())),
            ("macrosSaved", !self.saved.is_empty()),
        ]
    }

    pub fn start_recording(&mut self) {
        self.recording = Some(Vec::new());
    }

    /// Appends a step to the recording; consecutive typing is merged into one text step.
    /// Ignored when not recording or while a macro plays.
    pub fn record_step(&mut self, step: MacroStep) {
        if self.playing {
            return;
        }
        let Some(recording) = &mut self.recording else { return };
        if let (MacroStep::Text { text }, Some(MacroStep::Text { text: previous })) = (&step, recording.last_mut()) {
            previous.push_str(text);
            return;
        }
        recording.push(step);
    }

    /// Records an action the user ran, unless it is a macro action itself or its argument
    /// cannot be stored.
    pub fn record_action(&mut self, id: &str, arg: Option<&Value>) {
        if self.recording.is_none() || self.playing || id.starts_with("macro.") || id.starts_with("macros.") {
            return;
        }
        let arg = match storable_arg(arg) {
            StorableArg::None => None,
            StorableArg::Plain(arg) => Some(arg),
            StorableArg::Unstorable => return,
        };
        self.record_step(MacroStep::Action { id: id.to_owned(), arg });
    }

    /// Ends the recording; returns its steps (also the new "last macro" when not empty).
    pub fn stop_recording(&mut self, storage: &mut UiStorage) -> Vec<MacroStep> {
        let recording = self.recording.take().unwrap_or_default();
        if !recording.is_empty() {
            self.last = Some(recording.clone());
        }
        self.persist(storage);
        recording
    }

    /// Saves `steps` as `name` (trimmed), replacing a macro of that name. A blank name saves nothing.
    pub fn save_macro(&mut self, name: &str, steps: Vec<MacroStep>, storage: &mut UiStorage) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let entry = Macro { name: name.to_owned(), steps };
        match self.saved.iter_mut().find(|m| m.name == name) {
            Some(existing) => *existing = entry,
            None => self.saved.push(entry),
        }
        self.persist(storage);
    }

    /// Why `name` cannot be the new name of the macro `current` (Rename Macro's validation).
    pub fn rename_error(&self, current: &str, name: &str) -> Option<&'static str> {
        let name = name.trim();
        if name.is_empty() {
            Some("Enter a macro name")
        } else if name != current && self.saved.iter().any(|m| m.name == name) {
            Some("A macro with this name exists")
        } else {
            None
        }
    }

    /// Renames `from` to `to` (trimmed); false when `to` is blank or another macro's name.
    pub fn rename_macro(&mut self, from: &str, to: &str, storage: &mut UiStorage) -> bool {
        if self.rename_error(from, to).is_some() {
            return false;
        }
        let to = to.trim();
        for m in self.saved.iter_mut().filter(|m| m.name == from) {
            to.clone_into(&mut m.name);
        }
        self.persist(storage);
        true
    }

    pub fn delete_macro(&mut self, name: &str, storage: &mut UiStorage) {
        self.saved.retain(|m| m.name != name);
        self.persist(storage);
    }

    pub fn saved_macro(&self, name: &str) -> Option<&Macro> {
        self.saved.iter().find(|m| m.name == name)
    }

    /// Starts playing `steps` (a saved macro's `name`, if any); `None` while another plays.
    pub fn start_playback(&mut self, steps: &[MacroStep], name: Option<&str>) -> Option<Playback> {
        if self.playing {
            return None;
        }
        self.playing = true;
        Some(Playback { steps: steps.to_vec(), next: 0, name: name.map(str::to_owned) })
    }

    /// Ends `playback`; `completed` is whether every step ran. Returns the warning to show
    /// when a step could not run.
    pub fn finish_playback(&mut self, playback: Playback, completed: bool) -> Option<NotificationRequest> {
        self.playing = false;
        if completed {
            return None;
        }
        let title = match &playback.name {
            Some(name) => format!("Macro '{name}' stopped"),
            None => "Macro playback stopped".to_owned(),
        };
        Some(NotificationRequest {
            body: Some("A recorded step could not run here.".to_owned()),
            key: Some("macro.playback".to_owned()),
            ..NotificationRequest::new(NotificationLevel::Warning, title)
        })
    }
}

/// What Stop Macro Recording shows when nothing was recorded.
pub fn nothing_recorded() -> NotificationRequest {
    NotificationRequest {
        key: Some("macro.recording".to_owned()),
        ..NotificationRequest::new(NotificationLevel::Info, "Nothing was recorded")
    }
}

/// A macro being played: the caller runs [`Playback::next_step`] until it is `None` or a step
/// fails, then calls [`MacroStore::finish_playback`].
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    steps: Vec<MacroStep>,
    next: usize,
    name: Option<String>,
}

impl Playback {
    /// The next step to run; `None` when every step ran.
    pub fn next_step(&mut self) -> Option<&MacroStep> {
        let step = self.steps.get(self.next)?;
        self.next += 1;
        Some(step)
    }
}

/// The id of the action that plays the saved macro `name` (so it can get a shortcut).
pub fn play_action_id(name: &str) -> String {
    format!("macros.play.{name}")
}

fn describe_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), js_number_string),
        other => other.to_string(),
    }
}

fn describe_arg(arg: &Value) -> String {
    match arg {
        Value::Array(items) => items.iter().map(describe_value).collect::<Vec<_>>().join(", "),
        Value::Object(fields) => match fields.get("path") {
            Some(Value::String(path)) => path.clone(),
            _ => arg.to_string(),
        },
        other => describe_value(other),
    }
}

/// One line per step, for Edit Macros. `title` names an action by its id.
pub fn describe_step(step: &MacroStep, title: impl Fn(&str) -> String) -> String {
    match step {
        MacroStep::Action { id, arg: None } => title(id),
        MacroStep::Action { id, arg: Some(arg) } => format!("{} ({})", title(id), describe_arg(arg)),
        MacroStep::Text { text } => format!("Type \"{}\"", text.replace('\n', "↵")),
        MacroStep::Key { key } => format!("Press {key}"),
    }
}

/// The editing and caret keys a macro records in the editor (keys the IDE keymap does not bind
/// run editor commands directly), by recorded name, with the editor command each replays as.
pub const EDITOR_KEYS: &[(&str, &str)] = &[
    ("Enter", "insertNewlineAndIndent"),
    ("Backspace", "deleteCharBackward"),
    ("Delete", "deleteCharForward"),
    ("Alt+Backspace", "deleteGroupBackward"),
    ("Alt+Delete", "deleteGroupForward"),
    ("Tab", "insertTab"),
    ("Shift+Tab", "indentLess"),
    ("ArrowLeft", "cursorCharLeft"),
    ("ArrowRight", "cursorCharRight"),
    ("ArrowUp", "cursorLineUp"),
    ("ArrowDown", "cursorLineDown"),
    ("Shift+ArrowLeft", "selectCharLeft"),
    ("Shift+ArrowRight", "selectCharRight"),
    ("Shift+ArrowUp", "selectLineUp"),
    ("Shift+ArrowDown", "selectLineDown"),
    ("Alt+ArrowLeft", "cursorGroupLeft"),
    ("Alt+ArrowRight", "cursorGroupRight"),
    ("Alt+Shift+ArrowLeft", "selectGroupLeft"),
    ("Alt+Shift+ArrowRight", "selectGroupRight"),
    ("Home", "cursorLineBoundaryBackward"),
    ("End", "cursorLineBoundaryForward"),
    ("Shift+Home", "selectLineBoundaryBackward"),
    ("Shift+End", "selectLineBoundaryForward"),
    ("PageUp", "cursorPageUp"),
    ("PageDown", "cursorPageDown"),
    ("Shift+PageUp", "selectPageUp"),
    ("Shift+PageDown", "selectPageDown"),
    ("Meta+ArrowLeft", "cursorLineBoundaryBackward"),
    ("Meta+ArrowRight", "cursorLineBoundaryForward"),
    ("Meta+Shift+ArrowLeft", "selectLineBoundaryBackward"),
    ("Meta+Shift+ArrowRight", "selectLineBoundaryForward"),
    ("Meta+ArrowUp", "cursorDocStart"),
    ("Meta+ArrowDown", "cursorDocEnd"),
    ("Ctrl+Home", "cursorDocStart"),
    ("Ctrl+End", "cursorDocEnd"),
    ("Ctrl+Shift+Home", "selectDocStart"),
    ("Ctrl+Shift+End", "selectDocEnd"),
    ("Ctrl+ArrowLeft", "cursorGroupLeft"),
    ("Ctrl+ArrowRight", "cursorGroupRight"),
];

/// A key press in the editor: the key as GPUI names it (`left`, `enter`) and the modifiers held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyPress<'a> {
    pub key: &'a str,
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
}

/// The name a macro records an editor key under (`ArrowLeft` for GPUI's `left`): the names saved
/// macros are stored with.
fn recorded_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "enter" => "Enter",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "tab" => "Tab",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => return None,
    })
}

/// The editor command a recorded key replays as.
pub fn editor_key_command(name: &str) -> Option<&'static str> {
    EDITOR_KEYS.iter().find(|(key, _)| *key == name).map(|(_, command)| *command)
}

/// The recorded name of a key press (`Alt+Shift+ArrowLeft`), or `None` when it is not an editor
/// key a macro records.
pub fn editor_key_name(press: KeyPress<'_>) -> Option<String> {
    let modifiers = [(press.meta, "Meta"), (press.ctrl, "Ctrl"), (press.alt, "Alt"), (press.shift, "Shift")];
    let mut parts: Vec<&str> = modifiers.iter().filter(|(held, _)| *held).map(|(_, name)| *name).collect();
    parts.push(recorded_key(press.key)?);
    let name = parts.join("+");
    editor_key_command(&name).is_some().then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> MacroStep {
        MacroStep::Text { text: text.to_owned() }
    }
    fn key(key: &str) -> MacroStep {
        MacroStep::Key { key: key.to_owned() }
    }
    fn action(id: &str, arg: Option<Value>) -> MacroStep {
        MacroStep::Action { id: id.to_owned(), arg }
    }
    fn names(store: &MacroStore) -> Vec<&str> {
        store.saved.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn records_actions_merged_typing_and_keys_only_while_recording() {
        let mut storage = UiStorage::default();
        let mut store = MacroStore::load(&storage);
        store.record_step(text("ignored"));
        store.start_recording();
        store.record_step(text("he"));
        store.record_step(text("llo"));
        store.record_step(key("Enter"));
        store.record_step(action("editor.duplicateLine", None));
        let steps = store.stop_recording(&mut storage);
        assert_eq!(steps, vec![text("hello"), key("Enter"), action("editor.duplicateLine", None)]);
        assert_eq!(store.last.as_ref(), Some(&steps));
        // An empty recording keeps the last macro.
        store.start_recording();
        store.stop_recording(&mut storage);
        assert_eq!(store.last.as_ref(), Some(&steps));
    }

    #[test]
    fn persists_saved_macros_renames_and_deletes_them() {
        let mut storage = UiStorage::default();
        let mut store = MacroStore::load(&storage);
        store.save_macro("Dup", vec![action("editor.duplicateLine", None)], &mut storage);
        store.save_macro("Two", Vec::new(), &mut storage);
        assert!(!store.rename_macro("Dup", "Two", &mut storage));
        assert!(store.rename_macro("Dup", "Duplicate", &mut storage));
        let mut store = MacroStore::load(&storage);
        assert_eq!(names(&store), ["Duplicate", "Two"]);
        store.delete_macro("Two", &mut storage);
        let store = MacroStore::load(&storage);
        assert_eq!(names(&store), ["Duplicate"]);
    }

    #[test]
    fn drops_malformed_stored_steps() {
        let mut storage = UiStorage::default();
        let stored = json!({
            "last": [
                { "kind": "text", "text": "a" },
                { "kind": "action" },
                { "kind": "action", "id": "x", "arg": { "a": { "b": 1 } } }
            ],
            "saved": [{ "name": "" }]
        });
        storage.set(STORAGE_KEY, stored.to_string());
        let store = MacroStore::load(&storage);
        assert_eq!(store.last, Some(vec![text("a")]));
        assert_eq!(store.saved, Vec::new());
    }

    #[test]
    fn plays_steps_in_order_and_stops_at_the_first_that_cannot_run_nothing_is_recorded_meanwhile() {
        let mut store = MacroStore::default();
        let mut log: Vec<String> = Vec::new();
        store.start_recording();
        let steps = [action("a", Some(json!(1))), text("t"), key("Tab"), action("b", None)];
        let mut playback = store.start_playback(&steps, None).expect("nothing else plays");
        assert_eq!(store.start_playback(&steps, None), None);
        let mut completed = true;
        while let Some(step) = playback.next_step().cloned() {
            let ok = match &step {
                MacroStep::Action { id, arg } => {
                    store.record_step(action(id, None));
                    store.record_action(id, arg.as_ref());
                    log.push(format!("{id}{}", arg.as_ref().map(describe_arg).unwrap_or_default()));
                    id != "b"
                }
                MacroStep::Text { text } => {
                    log.push(text.clone());
                    true
                }
                MacroStep::Key { key } => {
                    log.push(key.clone());
                    true
                }
            };
            if !ok {
                completed = false;
                break;
            }
        }
        let notice = store.finish_playback(playback, completed);
        assert!(!completed);
        assert_eq!(log, ["a1", "t", "Tab", "b"]);
        assert_eq!(store.recording, Some(Vec::new()));
        assert!(!store.playing);
        let notice = notice.expect("a stopped playback warns");
        assert_eq!(notice.title, "Macro playback stopped");
        assert_eq!(notice.level, NotificationLevel::Warning);
    }

    #[test]
    fn stores_plain_action_arguments_only_and_describes_steps() {
        assert_eq!(storable_arg(Some(&json!("x"))), StorableArg::Plain(json!("x")));
        assert_eq!(storable_arg(Some(&json!(2))), StorableArg::Plain(json!(2)));
        assert_eq!(storable_arg(None), StorableArg::None);
        // A JSON value cannot hold NaN; `null` is what the argument would be closest to.
        assert_eq!(storable_arg(Some(&Value::Null)), StorableArg::Unstorable);
        let title = |id: &str| if id == "a" { "Action A".to_owned() } else { id.to_owned() };
        assert_eq!(describe_step(&action("a", Some(json!("x"))), title), "Action A (x)");
        assert_eq!(describe_step(&text("a\nb"), title), "Type \"a↵b\"");
        assert_eq!(describe_step(&key("Enter"), title), "Press Enter");
    }

    #[test]
    fn stores_a_selection_payload_and_rejects_anything_that_is_not_plain_data() {
        let selection = json!({ "path": "/a", "paths": ["/a", "/b"] });
        assert_eq!(storable_arg(Some(&selection)), StorableArg::Plain(selection.clone()));
        assert_eq!(storable_arg(Some(&json!(["/a"]))), StorableArg::Plain(json!(["/a"])));
        assert_eq!(storable_arg(Some(&json!(true))), StorableArg::Plain(json!(true)));
        assert_eq!(storable_arg(Some(&json!({ "a": { "b": 1 } }))), StorableArg::Unstorable);
        assert_eq!(storable_arg(Some(&json!([1]))), StorableArg::Unstorable);
        assert_eq!(storable_arg(Some(&json!({ "n": null }))), StorableArg::Unstorable);
        assert_eq!(storable_arg(Some(&json!({}))), StorableArg::Plain(json!({})));
    }

    #[test]
    fn round_trips_an_object_argument_through_storage_and_describes_it() {
        let mut storage = UiStorage::default();
        let mut store = MacroStore::default();
        let arg = json!({ "path": "/a", "paths": ["/a", "/b"] });
        store.save_macro("Sel", vec![action("p.run", Some(arg.clone()))], &mut storage);
        let store = MacroStore::load(&storage);
        assert_eq!(store.saved[0].steps, vec![action("p.run", Some(arg.clone()))]);
        let id = |id: &str| id.to_owned();
        assert_eq!(describe_step(&action("p.run", Some(arg)), id), "p.run (/a)");
        assert_eq!(describe_step(&action("p.run", Some(json!(["/a", "/b"]))), id), "p.run (/a, /b)");
        assert_eq!(describe_step(&action("p.run", Some(json!({ "n": 1 }))), id), "p.run ({\"n\":1})");
    }

    #[test]
    fn names_the_editor_keys_a_macro_records() {
        let k = |key, press: KeyPress<'static>| editor_key_name(KeyPress { key, ..press });
        assert_eq!(k("enter", KeyPress::default()).as_deref(), Some("Enter"));
        let alt_shift = KeyPress { alt: true, shift: true, ..KeyPress::default() };
        assert_eq!(k("left", alt_shift).as_deref(), Some("Alt+Shift+ArrowLeft"));
        assert_eq!(k("a", KeyPress::default()), None);
        assert_eq!(k("s", KeyPress { meta: true, ..KeyPress::default() }), None);
    }
}
