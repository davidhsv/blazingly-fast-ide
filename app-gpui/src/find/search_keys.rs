//! The keys Search Everywhere's query field handles itself: while it has focus the keymap leaves them alone,
//! in every modifier combination. With an editor behind the popup the keymap would otherwise run editor
//! actions on them (⌥↵ Show Context Actions, ⌥↑ Extend Selection, …).

use crate::platform::dispatcher::KeyPress;

/// The keys of the query field, as the dispatcher names them.
pub const SEARCH_KEYS: [&str; 6] = ["Enter", "Tab", "Up", "Down", "PageUp", "PageDown"];

/// Whether the query field keeps `press` for itself; what a dispatch environment answers from its `claimed`
/// while the popup is open.
pub fn claims_search_key(press: &KeyPress, query_field_focused: bool) -> bool {
    query_field_focused && SEARCH_KEYS.contains(&press.key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::dispatcher::{DispatchEnv, Dispatcher, KeyTarget, Mods, Outcome};
    use crate::platform::keymap::normalize_binding;

    /// A keymap and whether the query field has the focus.
    struct Env {
        bindings: Vec<(String, String)>,
        query_field_focused: bool,
    }

    impl DispatchEnv for Env {
        fn bindings(&self) -> &[(String, String)] {
            &self.bindings
        }

        fn key_enabled(&self, _action_id: &str) -> bool {
            true
        }

        fn claimed(&self, press: &KeyPress, _stroke: &str) -> bool {
            claims_search_key(press, self.query_field_focused)
        }
    }

    // src/features/search/__tests__/searchKeys.test.ts › Search Everywhere's query field › keeps ⌥↵ and the
    // navigation keys from the editor's actions behind the popup
    #[test]
    fn keeps_alt_enter_and_the_navigation_keys_from_the_editors_actions_behind_the_popup() {
        let bound = |binding: &str, id: &str| (normalize_binding(binding), id.to_owned());
        let mut env = Env {
            bindings: vec![bound("Alt+Enter", "t.intentions"), bound("Alt+Up", "t.extend")],
            query_field_focused: true,
        };
        let alt = Mods { alt: true, ..Mods::NONE };
        let press = |key: &str| KeyPress::new(key, alt).on(KeyTarget::TextField);
        let mut dispatcher = Dispatcher::new(true);
        assert_eq!(dispatcher.key_down(&press("Enter"), 0.0, &env), Outcome::Pass);
        assert_eq!(dispatcher.key_down(&press("Up"), 1.0, &env), Outcome::Pass);
        // In any other field the keymap answers.
        env.query_field_focused = false;
        assert_eq!(dispatcher.key_down(&press("Enter"), 2.0, &env), Outcome::Run("t.intentions".to_owned()));
        assert_eq!(dispatcher.key_down(&press("Up"), 3.0, &env), Outcome::Run("t.extend".to_owned()));
    }

    // Not from a spec: only the query field's own keys are claimed, whatever the modifiers.
    #[test]
    fn claims_only_the_query_fields_keys() {
        for key in SEARCH_KEYS {
            assert!(claims_search_key(&KeyPress::new(key, Mods::NONE), true), "{key}");
            assert!(claims_search_key(&KeyPress::new(key, Mods { shift: true, meta: true, ..Mods::NONE }), true));
            assert!(!claims_search_key(&KeyPress::new(key, Mods::NONE), false), "{key}");
        }
        assert!(!claims_search_key(&KeyPress::new("A", Mods { meta: true, ..Mods::NONE }), true));
        assert!(!claims_search_key(&KeyPress::new("Escape", Mods::NONE), true));
    }
}
