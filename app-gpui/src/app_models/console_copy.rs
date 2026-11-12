//! Copying from output views (Run console, Build, database results) with the Windows and Linux
//! keymaps: Ctrl+C and Ctrl+Insert copy the view's own selection. No keymap binds Copy itself, so
//! the key reaches the focused view first ([`crate::platform_more::selectable_text::copies_selection`]).

#[cfg(test)]
mod tests {
    use crate::platform::dispatcher::Mods;
    use crate::platform::keymap::{PRESET_TITLES, preset};
    use crate::platform_more::selectable_text::{OutputKeyPress, copies_selection};

    const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };

    fn press(key: &'static str, mods: Mods) -> OutputKeyPress<'static> {
        OutputKeyPress { key, mods, selection_in_pane: true, ..OutputKeyPress::default() }
    }

    // copies selected Run console output with Ctrl+%s
    #[test]
    fn copies_selected_run_console_output_with_ctrl_c_or_ctrl_insert() {
        for key in ["C", "Insert"] {
            assert!(copies_selection(&press(key, CTRL), false), "{key}");
        }
    }

    // copies locally in %s
    #[test]
    fn copies_locally_in_every_windows_keymap() {
        for id in ["intellij-windows", "eclipse-windows", "vscode-windows"] {
            assert!(preset(id).is_some_and(|keymap| !keymap.contains_key("edit.copy")), "{id}");
            assert!(copies_selection(&press("C", CTRL), false));
        }
    }

    // leaves modified and composing keys alone (%j)
    #[test]
    fn leaves_modified_and_composing_keys_alone() {
        for blocked in [
            press("C", Mods { alt: true, ..CTRL }),
            press("C", Mods { shift: true, ..CTRL }),
            OutputKeyPress { composing: true, ..press("C", CTRL) },
        ] {
            assert!(!copies_selection(&blocked, false), "{blocked:?}");
        }
    }

    // %s does not pre-empt per-view Ctrl+C
    #[test]
    fn no_preset_pre_empts_per_view_ctrl_c() {
        for (id, _) in PRESET_TITLES {
            let keymap = preset(id).expect("a built-in preset");
            assert!(!keymap.contains_key("edit.copy"), "{id}");
        }
    }
}
