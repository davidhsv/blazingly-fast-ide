//! Exception breakpoints: the filters each adapter offers (`exceptionBreakpointFilters`, cached per
//! adapter so the Breakpoints dialog can show them without a session) and the user's choices
//! (filter id → enabled), stored per project and mirrored to the backend, which applies them to
//! every session (adapter defaults for filters without a choice).

use crate::debugger::effects::{BackendCall, Effect};
use ide_dap::{ExceptionFilter, SessionExceptionFilters};
use serde_json::Value;
use std::collections::BTreeMap;

/// Storage key prefix of a project's choices; the project root follows.
pub const CHOICES_PREFIX: &str = "openidea.debugger.exceptions:";
/// Storage key of the filters adapters offered, for every project.
pub const KNOWN_KEY: &str = "openidea.debugger.exceptionFilters";

/// Explicit choices: choice key → enabled.
pub type Choices = BTreeMap<String, bool>;
/// Adapter name → the filters it offered last time.
pub type KnownFilters = BTreeMap<String, Vec<ExceptionFilter>>;

/// The key a project's choices are stored under.
pub fn choices_key(root: &str) -> String {
    format!("{CHOICES_PREFIX}{root}")
}

/// The choices of stored data: the entries that are booleans.
pub fn sanitize_choices(raw: &Value) -> Choices {
    let Some(entries) = raw.as_object() else { return Choices::new() };
    entries.iter().filter_map(|(key, value)| Some((key.clone(), value.as_bool()?))).collect()
}

/// The filter cache of stored data: per adapter, the entries that are filters.
pub fn sanitize_known(raw: &Value) -> KnownFilters {
    let Some(adapters) = raw.as_object() else { return KnownFilters::new() };
    let filter = |f: &Value| {
        Some(ExceptionFilter {
            filter: f.get("filter")?.as_str()?.to_owned(),
            label: f.get("label")?.as_str()?.to_owned(),
            description: f.get("description").and_then(Value::as_str).map(str::to_owned),
            default: f.get("default").and_then(Value::as_bool).unwrap_or(false),
            supports_condition: f.get("supportsCondition").and_then(Value::as_bool).unwrap_or(false),
        })
    };
    adapters
        .iter()
        .filter_map(|(adapter, list)| {
            let filters: Vec<ExceptionFilter> = list.as_array()?.iter().filter_map(&filter).collect();
            (!filters.is_empty()).then(|| (adapter.clone(), filters))
        })
        .collect()
}

fn seed(filter: &str, label: &str, default: bool) -> ExceptionFilter {
    ExceptionFilter {
        filter: filter.to_owned(),
        label: label.to_owned(),
        description: None,
        default,
        supports_condition: false,
    }
}

/// Filters of the supported adapters before any session reported its own (the dialog shows them
/// right away, like IDEA's per-language exception breakpoints).
pub fn builtin_filters() -> KnownFilters {
    let filters = |list: &[(&str, &str, bool)]| -> Vec<ExceptionFilter> {
        list.iter().map(|(id, label, on)| seed(id, label, *on)).collect()
    };
    BTreeMap::from([
        (
            "debugpy".to_owned(),
            filters(&[
                ("raised", "Raised Exceptions", false),
                ("uncaught", "Uncaught Exceptions", true),
                ("userUnhandled", "User Uncaught Exceptions", false),
            ]),
        ),
        (
            "java-debug".to_owned(),
            filters(&[("caught", "Caught Exceptions", false), ("uncaught", "Uncaught Exceptions", false)]),
        ),
        (
            "js-debug".to_owned(),
            filters(&[("all", "Caught Exceptions", false), ("uncaught", "Uncaught Exceptions", false)]),
        ),
        (
            "codelldb".to_owned(),
            filters(&[
                ("rust_panic", "Rust: on panic", true),
                ("cpp_throw", "C++: on throw", false),
                ("cpp_catch", "C++: on catch", false),
            ]),
        ),
    ])
}

/// Choice key of an adapter's filter (adapters reuse ids such as `uncaught`).
pub fn choice_key(adapter: &str, filter: &str) -> String {
    format!("{adapter}:{filter}")
}

/// Effective state: the adapter-scoped choice, an unscoped one of older data, else the default.
pub fn is_filter_enabled(filter: &ExceptionFilter, choices: &Choices, adapter: &str) -> bool {
    let scoped = choices.get(&choice_key(adapter, &filter.filter));
    scoped.or_else(|| choices.get(&filter.filter)).copied().unwrap_or(filter.default)
}

/// A filter with its effective state.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFilter {
    pub filter: ExceptionFilter,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionGroup {
    pub adapter: String,
    pub filters: Vec<GroupFilter>,
}

/// Filters grouped by adapter (sorted), with their effective state.
pub fn exception_groups(known: &KnownFilters, choices: &Choices) -> Vec<ExceptionGroup> {
    known
        .iter()
        .map(|(adapter, filters)| ExceptionGroup {
            adapter: adapter.clone(),
            filters: filters
                .iter()
                .map(|f| GroupFilter { filter: f.clone(), enabled: is_filter_enabled(f, choices, adapter) })
                .collect(),
        })
        .collect()
}

/// Merges filters reported by live sessions (adapter, filters) into the per-adapter cache;
/// `None` when they change nothing.
pub fn merge_known(known: &KnownFilters, sessions: &[(&str, &[ExceptionFilter])]) -> Option<KnownFilters> {
    let mut next: Option<KnownFilters> = None;
    for (adapter, filters) in sessions {
        let current = next.as_ref().unwrap_or(known);
        if filters.is_empty() || current.get(*adapter).map(Vec::as_slice) == Some(*filters) {
            continue;
        }
        next.get_or_insert_with(|| known.clone()).insert((*adapter).to_owned(), filters.to_vec());
    }
    next
}

/// The exception filters and the project's choices among them. What a change asks for collects
/// here until [`ExceptionBreakpoints::take_effects`].
#[derive(Debug, Clone)]
pub struct ExceptionBreakpoints {
    root: Option<String>,
    /// Explicit choices; filters without one use the adapter's default.
    choices: Choices,
    known: KnownFilters,
    effects: Vec<Effect>,
}

impl ExceptionBreakpoints {
    /// `stored_known`: the value under [`KNOWN_KEY`].
    pub fn new(stored_known: Option<&str>) -> Self {
        let stored = stored_known.and_then(|stored| serde_json::from_str::<Value>(stored).ok());
        let mut known = builtin_filters();
        known.extend(stored.as_ref().map(sanitize_known).unwrap_or_default());
        Self { root: None, choices: Choices::new(), known, effects: Vec::new() }
    }

    pub fn choices(&self) -> &Choices {
        &self.choices
    }

    pub fn known(&self) -> &KnownFilters {
        &self.known
    }

    /// What the changes since the last call ask for, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Sends the choices to the backend. `can_sync`: a backend is attached and the project is trusted.
    fn push(&mut self, can_sync: bool) {
        if can_sync && self.root.is_some() {
            self.effects.push(Effect::Call(BackendCall::DebugSetExceptionBreakpoints(self.choices.clone())));
        }
    }

    /// Loads the project's choices from `stored` (the value under [`choices_key`]) and sends them
    /// to the backend.
    pub fn load(&mut self, root: Option<&str>, stored: Option<&str>, can_sync: bool) {
        let stored = root.and(stored).and_then(|stored| serde_json::from_str::<Value>(stored).ok());
        self.root = root.map(str::to_owned);
        self.choices = stored.as_ref().map(sanitize_choices).unwrap_or_default();
        self.push(can_sync);
    }

    pub fn set_exception_filter(&mut self, adapter: &str, filter: &str, enabled: bool, can_sync: bool) {
        let key = choice_key(adapter, filter);
        if self.choices.get(&key) == Some(&enabled) {
            return;
        }
        self.choices.insert(key, enabled);
        if let Some(root) = &self.root {
            let value = serde_json::to_string(&self.choices).unwrap_or_default();
            self.effects.push(Effect::StorageSet { key: choices_key(root), value });
        }
        self.push(can_sync);
    }

    /// Asks for the filters of the live sessions (after a session starts, when the dialog opens).
    pub fn refresh(&mut self, desktop: bool) {
        if desktop {
            self.effects.push(Effect::Call(BackendCall::DebugExceptionFilters));
        }
    }

    /// The live sessions' filters arrived: what is new goes into the cache and the storage.
    pub fn filters_loaded(&mut self, sessions: &[SessionExceptionFilters]) {
        let reported: Vec<(&str, &[ExceptionFilter])> =
            sessions.iter().map(|s| (s.adapter.as_str(), s.filters.as_slice())).collect();
        if let Some(next) = merge_known(&self.known, &reported) {
            let value = serde_json::to_string(&next).unwrap_or_default();
            self.known = next;
            self.effects.push(Effect::StorageSet { key: KNOWN_KEY.to_owned(), value });
        }
    }
}

impl Default for ExceptionBreakpoints {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::DebugSessionId;
    use serde_json::json;

    fn f(filter: &str, default: bool) -> ExceptionFilter {
        let mut label = filter.to_owned();
        label[..1].make_ascii_uppercase();
        seed(filter, &label, default)
    }

    fn choices(entries: &[(&str, bool)]) -> Choices {
        entries.iter().map(|(key, enabled)| ((*key).to_owned(), *enabled)).collect()
    }

    #[test]
    fn explicit_choices_win_over_adapter_defaults() {
        assert!(is_filter_enabled(&f("uncaught", true), &choices(&[]), ""));
        assert!(!is_filter_enabled(&f("uncaught", true), &choices(&[("uncaught", false)]), ""));
        assert!(is_filter_enabled(&f("raised", false), &choices(&[("raised", true)]), ""));
    }

    #[test]
    fn groups_filters_by_adapter_with_effective_state() {
        let known = KnownFilters::from([
            ("debugpy".to_owned(), vec![f("raised", false), f("uncaught", true)]),
            ("codelldb".to_owned(), vec![f("rust_panic", false)]),
        ]);
        let groups = exception_groups(&known, &choices(&[("raised", true)]));
        assert_eq!(groups.iter().map(|g| g.adapter.as_str()).collect::<Vec<_>>(), ["codelldb", "debugpy"]);
        let states = groups[1].filters.iter().map(|x| (x.filter.filter.as_str(), x.enabled));
        assert_eq!(states.collect::<Vec<_>>(), [("raised", true), ("uncaught", true)]);
    }

    #[test]
    fn merges_session_filters_into_the_cache_without_churn() {
        let known = KnownFilters::from([("debugpy".to_owned(), vec![f("raised", false)])]);
        assert_eq!(merge_known(&known, &[("debugpy", &[f("raised", false)])]), None);
        assert_eq!(merge_known(&known, &[("debugpy", &[])]), None);
        let mut merged = known.clone();
        merged.insert("delve".to_owned(), vec![f("panic", false)]);
        assert_eq!(merge_known(&known, &[("delve", &[f("panic", false)])]), Some(merged));
    }

    #[test]
    fn sanitizes_persisted_data() {
        let stored = json!({ "a": true, "b": "x", "c": false });
        assert_eq!(sanitize_choices(&stored), choices(&[("a", true), ("c", false)]));
        assert_eq!(sanitize_choices(&json!([1])), Choices::new());
        let stored = json!({ "x": [serde_json::to_value(f("a", false)).expect("a filter"), { "nope": 1 }], "y": "z" });
        assert_eq!(sanitize_known(&stored), KnownFilters::from([("x".to_owned(), vec![f("a", false)])]));
    }

    // Not from a spec: a choice is stored under the project and sent to the backend; filters that
    // sessions report join the built-in ones in the cache.
    #[test]
    fn stores_choices_per_project_and_caches_reported_filters() {
        let mut exceptions = ExceptionBreakpoints::default();
        exceptions.load(Some("/p"), None, true);
        exceptions.set_exception_filter("debugpy", "raised", true, true);
        exceptions.set_exception_filter("debugpy", "raised", true, true);
        let effects = exceptions.take_effects();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::Call(BackendCall::DebugSetExceptionBreakpoints(loaded)),
                Effect::StorageSet { key, value },
                Effect::Call(BackendCall::DebugSetExceptionBreakpoints(sent)),
            ] if loaded.is_empty() && key == "openidea.debugger.exceptions:/p"
                && value == r#"{"debugpy:raised":true}"# && sent.get("debugpy:raised") == Some(&true)
        ));
        let session = SessionExceptionFilters {
            session: DebugSessionId(1),
            adapter: "delve".to_owned(),
            filters: vec![f("panic", false)],
        };
        exceptions.filters_loaded(std::slice::from_ref(&session));
        exceptions.filters_loaded(&[session]);
        let effects = exceptions.take_effects();
        assert!(matches!(effects.as_slice(), [Effect::StorageSet { key, .. }] if key == KNOWN_KEY));
        assert!(exceptions.known().contains_key("delve") && exceptions.known().contains_key("debugpy"));
        let mut reloaded = ExceptionBreakpoints::new(Some(r#"{"delve":[{"filter":"panic","label":"Panic"}]}"#));
        reloaded.load(None, Some(r#"{"debugpy:raised":true}"#), true);
        assert!(reloaded.choices().is_empty() && reloaded.take_effects().is_empty());
        assert_eq!(reloaded.known().get("delve"), Some(&vec![f("panic", false)]));
    }
}
