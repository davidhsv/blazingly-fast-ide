use super::*;

// src/features/settings/__tests__/keymap.test.ts › applyKeymapSettings › ships the VS Code preset for every IntelliJ
// action id
/// The VS Code macOS preset lists every action IDEA's macOS preset binds, and each of its bindings is valid.
#[test]
fn ships_the_vs_code_preset_for_every_intellij_action_id() {
    let idea = preset("intellij-macos").expect("IDEA's preset");
    let vscode = preset("vscode-macos").expect("VS Code's preset");
    let missing: Vec<&String> = idea.keys().filter(|id| !vscode.contains_key(*id)).collect();
    assert!(missing.is_empty(), "VS Code's preset lacks {missing:?}");
    for (id, keys) in vscode {
        assert!(keys.iter().all(|key| is_valid_binding(key)), "{id}: {keys:?}");
    }
}
