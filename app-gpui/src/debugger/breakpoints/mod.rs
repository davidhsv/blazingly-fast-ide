//! Breakpoints of the open project: line breakpoints per file, method breakpoints and field
//! watchpoints, and the exception filters of each debug adapter.

pub mod code_lines;
pub mod exceptions;
pub mod languages;
pub mod model;
pub mod special;
pub mod store;
pub mod symbols;
pub mod toggle;

use crate::debugger::effects::Effect;

/// Every kind of breakpoint of the open project.
#[derive(Debug, Clone, Default)]
pub struct Breakpoints {
    pub lines: store::BreakpointStore,
    pub special: special::SpecialBreakpoints,
    pub exceptions: exceptions::ExceptionBreakpoints,
    pub languages: languages::DebuggableLanguages,
}

impl Breakpoints {
    /// What the changes since the last call ask for: line breakpoints first, then method
    /// breakpoints and watchpoints, then exception filters.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        let mut effects = self.lines.take_effects();
        effects.extend(self.special.take_effects());
        effects.extend(self.exceptions.take_effects());
        effects
    }

    /// Loads the breakpoints of the project at `root`: `stored` reads a key of the UI storage.
    /// `can_sync`: a backend is attached and the project is trusted.
    pub fn load(&mut self, root: Option<&str>, stored: impl Fn(&str) -> Option<String>, can_sync: bool, now_ms: f64) {
        let read = |key: fn(&str) -> String| root.and_then(|root| stored(&key(root)));
        self.lines.load(root, read(store::storage_key).as_deref(), now_ms);
        self.exceptions.load(root, read(exceptions::choices_key).as_deref(), can_sync);
        self.special.load(root, read(special::storage_key).as_deref(), now_ms);
    }

    /// When the stores have something to send next ([`Breakpoints::sync_due`]).
    pub fn next_deadline(&self) -> Option<f64> {
        [self.lines.next_deadline(), self.special.next_deadline()].into_iter().flatten().reduce(f64::min)
    }

    /// Sends what has waited out its delay.
    pub fn sync_due(&mut self, now_ms: f64, can_sync: bool) {
        self.lines.sync_due(now_ms, can_sync);
        self.special.sync_due(now_ms, can_sync, self.lines.muted());
    }

    /// Mute Breakpoints: applies to line breakpoints, method breakpoints and watchpoints alike.
    pub fn set_muted(&mut self, muted: bool, now_ms: f64) {
        if self.lines.muted() != muted {
            self.lines.set_muted(muted, now_ms);
            self.special.schedule_sync(now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::effects::BackendCall;

    // Not from a spec: muting reaches the backend for every kind of breakpoint, and loading a
    // project reads each store's own key.
    #[test]
    fn loads_and_mutes_every_kind_of_breakpoint_together() {
        let mut breakpoints = Breakpoints::default();
        let stored = |key: &str| {
            (key == "openidea.debugger.breakpoints:/p").then(|| r#"{"files":{"/p/a.py":[{"line":3}]}}"#.to_owned())
        };
        breakpoints.load(Some("/p"), stored, true, 0.0);
        assert_eq!(breakpoints.lines.breakpoints_of("/p/a.py").len(), 1);
        breakpoints.special.add_method_breakpoint("run", None, 0.0);
        breakpoints.sync_due(200.0, true);
        breakpoints.lines.sync_finished("/p/a.py", Ok(Vec::new()), true);
        breakpoints.take_effects();
        breakpoints.set_muted(true, 1000.0);
        breakpoints.set_muted(true, 1050.0);
        assert_eq!(breakpoints.next_deadline(), Some(1120.0));
        breakpoints.sync_due(1120.0, true);
        let effects = breakpoints.take_effects();
        let calls: Vec<&BackendCall> = effects.iter().filter_map(Effect::call).collect();
        assert!(matches!(
            calls.as_slice(),
            [
                BackendCall::DebugSetBreakpoints { breakpoints, .. },
                BackendCall::DebugSetFunctionBreakpoints(functions),
                BackendCall::DebugSetDataBreakpoints(_),
            ] if !breakpoints[0].enabled && !functions[0].enabled
        ));
    }
}
