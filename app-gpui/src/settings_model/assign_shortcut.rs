//! IDEA's "Assign Shortcut" from Search Everywhere: the Keymap page's keystroke dialog for one
//! action, saved straight to the active keymap (the overrides of a bundled keymap, or the user
//! keymap itself).

use super::keymap_model::{
    Conflict, CustomKeymaps, KeymapDraft, active_draft, add_binding, remove_conflicts, with_custom_overrides,
};
use crate::platform::keymap::{Keymap, preset};
use crate::platform::settings::SettingsWrite;
use serde_json::Value;

/// Settings key of the overrides of the bundled keymaps.
pub const KEYMAP_OVERRIDES_KEY: &str = "keymapOverrides";
/// Settings key of the user keymaps.
pub const CUSTOM_KEYMAPS_KEY: &str = "customKeymaps";

/// The keymap part of the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeymapSettings {
    /// The selected keymap: a bundled preset's id or a user keymap's.
    pub keymap: String,
    pub keymap_overrides: Keymap,
    pub custom_keymaps: CustomKeymaps,
}

/// The keymap the settings select, as a draft to edit.
pub fn draft_of(settings: &KeymapSettings) -> KeymapDraft {
    active_draft(&settings.keymap, &settings.keymap_overrides, &settings.custom_keymaps)
}

fn json<T: serde::Serialize>(value: &T) -> Value {
    // Keymaps hold only strings and lists of strings.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The settings write that adds `binding` to `action_id` in the active keymap. With
/// `remove_others` the `conflicts` (the other actions the binding is assigned to) lose it.
/// `draft` is [`draft_of`] the settings, with the plugins' default shortcuts filled in.
pub fn assigned_setting(
    settings: &KeymapSettings,
    draft: &KeymapDraft,
    action_id: &str,
    binding: &str,
    conflicts: &[Conflict],
    remove_others: bool,
) -> SettingsWrite {
    let mut base = draft.clone();
    if remove_others {
        base.overrides = remove_conflicts(draft, conflicts);
    }
    let overrides = add_binding(&base, action_id, binding);
    let custom = preset(&settings.keymap).is_none() && settings.custom_keymaps.contains_key(&settings.keymap);
    if custom {
        let customs = with_custom_overrides(&settings.custom_keymaps, &settings.keymap, overrides);
        SettingsWrite { key: CUSTOM_KEYMAPS_KEY, value: json(&customs) }
    } else {
        SettingsWrite { key: KEYMAP_OVERRIDES_KEY, value: json(&overrides) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::keymap_model::{KeymapAction, find_conflicts};
    use ide_core::settings::CustomKeymap;
    use serde_json::json;

    fn actions() -> [KeymapAction; 2] {
        [KeymapAction::new("file.save", "Save All"), KeymapAction::new("my.action", "My Action")]
    }

    fn bundled() -> KeymapSettings {
        KeymapSettings { keymap: "intellij-macos".to_owned(), ..KeymapSettings::default() }
    }

    // Assign Shortcut (Search Everywhere ⌥↵) › adds the binding as an override of a bundled keymap
    #[test]
    fn adds_the_binding_as_an_override_of_a_bundled_keymap() {
        let s = bundled();
        let write = assigned_setting(&s, &draft_of(&s), "my.action", "Alt+Meta+J", &[], false);
        let expected = SettingsWrite { key: "keymapOverrides", value: json!({ "my.action": ["Alt+Meta+J"] }) };
        assert_eq!(write, expected);
    }

    // Assign Shortcut (Search Everywhere ⌥↵) › removes conflicting assignments when asked, like the Keymap page
    #[test]
    fn removes_conflicting_assignments_when_asked_like_the_keymap_page() {
        let s = bundled();
        let draft = draft_of(&s);
        let conflicts = find_conflicts(&draft, "Meta+S", "my.action", &actions());
        assert_eq!(conflicts.iter().map(|c| c.action_id.as_str()).collect::<Vec<_>>(), ["file.save"]);
        let removed = assigned_setting(&s, &draft, "my.action", "Meta+S", &conflicts, true).value;
        assert_eq!(removed["my.action"], json!(["Meta+S"]));
        assert_eq!(removed["file.save"], json!([]));
        let kept = assigned_setting(&s, &draft, "my.action", "Meta+S", &conflicts, false).value;
        assert_eq!(kept.get("file.save"), None);
    }

    // Assign Shortcut (Search Everywhere ⌥↵) › writes into the selected user keymap
    #[test]
    fn writes_into_the_selected_user_keymap() {
        let (name, parent) = ("Mine".to_owned(), "intellij-macos".to_owned());
        let mine = CustomKeymap { name, parent, ..CustomKeymap::default() };
        let s = KeymapSettings {
            keymap: "custom.mine".to_owned(),
            custom_keymaps: [("custom.mine".to_owned(), mine)].into(),
            ..KeymapSettings::default()
        };
        let out = assigned_setting(&s, &draft_of(&s), "my.action", "Alt+Meta+J", &[], false);
        assert_eq!(out.key, "customKeymaps");
        let expected = json!({
            "custom.mine": { "name": "Mine", "parent": "intellij-macos", "overrides": { "my.action": ["Alt+Meta+J"] } },
        });
        assert_eq!(out.value, expected);
    }
}
