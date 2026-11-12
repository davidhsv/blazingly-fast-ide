//! Shortcut conflicts between Vim and the IDE keymap (the Settings › Editor › Vim table). Each Vim
//! key has a handler: Vim gets the keystroke in a Vim editor even when an IDE action is bound to
//! it; IDE lets the keymap run first (Vim still gets the key when no IDE action is enabled).

use super::settings::{Handler, VimSettings};
use crate::platform::keymap::normalize_binding;
use std::collections::BTreeMap;

/// A key Vim uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VimKey {
    /// Canonical stroke (`Ctrl+R`).
    pub stroke: &'static str,
    /// What Vim does with it.
    pub vim: &'static str,
    /// The default handler.
    pub handler: Handler,
}

const fn key(stroke: &'static str, vim: &'static str, handler: Handler) -> VimKey {
    VimKey { stroke, vim, handler }
}

pub const VIM_KEYS: [VimKey; 20] = [
    key("Escape", "Exit to Normal mode", Handler::Ide),
    key("Ctrl+[", "Exit to Normal mode", Handler::Vim),
    key("Ctrl+A", "Increment number", Handler::Vim),
    key("Ctrl+B", "Page up", Handler::Vim),
    key("Ctrl+C", "Exit to Normal mode", Handler::Ide),
    key("Ctrl+D", "Scroll half page down / unindent", Handler::Vim),
    key("Ctrl+E", "Scroll line down", Handler::Vim),
    key("Ctrl+F", "Page down", Handler::Vim),
    key("Ctrl+I", "Jump forward", Handler::Vim),
    key("Ctrl+N", "Next line", Handler::Ide),
    key("Ctrl+O", "Jump back / one Normal command", Handler::Vim),
    key("Ctrl+P", "Previous line", Handler::Ide),
    key("Ctrl+Q", "Visual block", Handler::Ide),
    key("Ctrl+R", "Redo / insert register", Handler::Vim),
    key("Ctrl+T", "Indent", Handler::Ide),
    key("Ctrl+U", "Scroll half page up / delete to line start", Handler::Vim),
    key("Ctrl+V", "Visual block", Handler::Vim),
    key("Ctrl+W", "Delete word before caret", Handler::Vim),
    key("Ctrl+X", "Decrement number", Handler::Vim),
    key("Ctrl+Y", "Scroll line up", Handler::Vim),
];

/// The handler of `stroke`, or `None` when Vim does not use it.
pub fn handler_for(stroke: &str, handlers: &BTreeMap<String, Handler>) -> Option<Handler> {
    let stroke = normalize_binding(stroke);
    let key = VIM_KEYS.iter().find(|k| normalize_binding(k.stroke) == stroke)?;
    Some(handlers.get(key.stroke).copied().unwrap_or(key.handler))
}

/// A row of the conflicts table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub key: VimKey,
    /// IDE actions bound to the stroke (first stroke of a binding included).
    pub actions: Vec<String>,
    /// Effective handler.
    pub current: Handler,
}

/// The conflicts table: every Vim key with the IDE actions bound to it. `bindings` are
/// `(binding, action id)` pairs of the effective keymap.
pub fn conflict_rows(bindings: &[(String, String)], handlers: &BTreeMap<String, Handler>) -> Vec<ConflictRow> {
    VIM_KEYS
        .iter()
        .map(|k| {
            let stroke = normalize_binding(k.stroke);
            let chord = format!("{stroke} ");
            let mut actions: Vec<String> = Vec::new();
            for (binding, id) in bindings {
                if (*binding == stroke || binding.starts_with(&chord)) && !actions.contains(id) {
                    actions.push(id.clone());
                }
            }
            ConflictRow { key: *k, actions, current: handlers.get(k.stroke).copied().unwrap_or(k.handler) }
        })
        .collect()
}

/// Whether the dispatcher leaves a first stroke to the focused editor: Vim mode is on, focus is
/// in a Vim editor and the stroke's handler is Vim.
pub fn vim_claims(settings: &VimSettings, in_vim_editor: bool, stroke: &str) -> bool {
    settings.enabled && in_vim_editor && handler_for(stroke, &settings.handlers) == Some(Handler::Vim)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(b, id)| ((*b).to_owned(), (*id).to_owned())).collect()
    }

    fn handlers(list: &[(&str, Handler)]) -> BTreeMap<String, Handler> {
        list.iter().map(|(k, h)| ((*k).to_owned(), *h)).collect()
    }

    fn on() -> VimSettings {
        VimSettings { enabled: true, handlers: BTreeMap::new() }
    }

    #[test]
    fn defaults_typical_vim_keys_to_vim_and_escape_to_the_ide_first() {
        for k in ["Ctrl+R", "Ctrl+V", "Ctrl+D", "Ctrl+W", "Ctrl+U", "Ctrl+["] {
            assert_eq!(handler_for(k, &BTreeMap::new()), Some(Handler::Vim), "{k}");
        }
        assert_eq!(handler_for("Escape", &BTreeMap::new()), Some(Handler::Ide));
        assert_eq!(handler_for("Ctrl+C", &BTreeMap::new()), Some(Handler::Ide));
        assert_eq!(handler_for("Meta+R", &BTreeMap::new()), None);
        assert_eq!(handler_for("Ctrl+R", &handlers(&[("Ctrl+R", Handler::Ide)])), Some(Handler::Ide));
    }

    #[test]
    fn lists_the_ide_actions_bound_to_each_vim_key() {
        let rows = conflict_rows(
            &pairs(&[
                ("Ctrl+R", "run.run"),
                ("Ctrl+D", "run.debug"),
                ("Ctrl+W Ctrl+X", "t.chord"),
                ("Meta+R", "other"),
            ]),
            &handlers(&[("Ctrl+D", Handler::Ide)]),
        );
        assert_eq!(rows.len(), VIM_KEYS.len());
        let row = |s: &str| rows.iter().find(|r| r.key.stroke == s).expect("row");
        assert_eq!(row("Ctrl+R").actions, ["run.run"]);
        assert_eq!(row("Ctrl+R").current, Handler::Vim);
        assert_eq!(row("Ctrl+D").actions, ["run.debug"]);
        assert_eq!(row("Ctrl+D").current, Handler::Ide);
        assert_eq!(row("Ctrl+W").actions, ["t.chord"]);
        assert!(row("Ctrl+V").actions.is_empty());
    }

    #[test]
    fn leaves_a_vim_handled_key_to_the_focused_vim_editor() {
        assert!(vim_claims(&on(), true, "Ctrl+R"));
    }

    #[test]
    fn runs_the_ide_action_when_the_handler_is_ide() {
        let settings = VimSettings { enabled: true, handlers: handlers(&[("Ctrl+R", Handler::Ide)]) };
        assert!(!vim_claims(&settings, true, "Ctrl+R"));
    }

    #[test]
    fn runs_the_ide_action_when_vim_mode_is_off_or_focus_is_outside_a_vim_editor() {
        assert!(!vim_claims(&on(), false, "Ctrl+R"));
        assert!(!vim_claims(&VimSettings::default(), true, "Ctrl+R"));
    }

    #[test]
    fn gives_escape_to_the_ide_first_by_default_to_vim_when_configured() {
        assert!(!vim_claims(&on(), true, "Escape"));
        let settings = VimSettings { enabled: true, handlers: handlers(&[("Escape", Handler::Vim)]) };
        assert!(vim_claims(&settings, true, "Escape"));
    }
}
