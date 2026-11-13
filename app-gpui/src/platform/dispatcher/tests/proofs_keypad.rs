use super::*;
use crate::platform::keymap::KeymapState;

/// The effective keymap of a built-in preset with only `enabled` registered, as when no other view is there to
/// take a shared key.
struct PresetEnv {
    keymap: KeymapState,
    enabled: &'static [&'static str],
}

impl DispatchEnv for PresetEnv {
    fn bindings(&self) -> &[(String, String)] {
        self.keymap.effective_bindings()
    }
    fn key_enabled(&self, action_id: &str) -> bool {
        self.enabled.contains(&action_id)
    }
}

fn press(env: &PresetEnv, mac: bool, key: &str, mods: Mods) -> Outcome {
    Dispatcher::new(mac).key_down(&KeyPress::new(key, mods), 0.0, env)
}

// src/platform/keymaps/eclipse.test.ts › %s › distinguishes keypad folding from font size and block comments during
// dispatch
/// In the Eclipse presets the keypad's minus folds and the main minus shrinks the font; the keypad's slash with
/// Shift folds everything and the main slash with Shift comments a block.
#[test]
fn the_eclipse_presets_tell_keypad_folding_from_font_size_and_block_comments() {
    const ENABLED: &[&str] = &["code.fold", "editor.decreaseFontSize", "code.foldAll", "code.commentBlock"];
    for (id, mac) in [("eclipse-windows", false), ("eclipse-macos", true)] {
        let env = PresetEnv { keymap: KeymapState::new(id), enabled: ENABLED };
        let primary = if mac { Mods { meta: true, ..Mods::NONE } } else { Mods { ctrl: true, ..Mods::NONE } };
        let shifted = Mods { shift: true, ..primary };
        for (key, mods, action) in [
            ("NumpadSubtract", primary, "code.fold"),
            ("-", primary, "editor.decreaseFontSize"),
            ("NumpadDivide", shifted, "code.foldAll"),
            ("/", shifted, "code.commentBlock"),
        ] {
            assert_eq!(press(&env, mac, key, mods), Outcome::Run(action.to_owned()), "{id}: {key}");
        }
    }
}

// src/platform/keymaps/eclipse.test.ts › keeps numeric-keypad aliases working for existing presets
/// In a preset that binds only the main minus to Collapse, Ctrl and the keypad's minus collapse too.
#[test]
fn keypad_aliases_keep_working_for_existing_presets() {
    let env = PresetEnv { keymap: KeymapState::new("intellij-windows"), enabled: &["code.fold"] };
    let ctrl = Mods { ctrl: true, ..Mods::NONE };
    assert_eq!(press(&env, false, "NumpadSubtract", ctrl), Outcome::Run("code.fold".to_owned()));
}
