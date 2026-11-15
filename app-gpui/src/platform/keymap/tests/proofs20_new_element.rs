use super::*;

// src/platform/keymaps/newElement.test.ts › binds New in This Directory like IDEA
/// New in This Directory is IDEA's ⌃⌥N on macOS and Ctrl+Alt+Insert on Windows; the VS Code keymap leaves it
/// unbound.
#[test]
fn binds_new_in_this_directory_like_idea() {
    let keys = |name: &str| {
        let preset = preset(name).unwrap_or_else(|| panic!("{name} is built in"));
        preset.get("project.newInThisDirectory").cloned()
    };
    assert_eq!(keys("intellij-macos"), Some(vec!["Ctrl+Alt+N".to_owned()]));
    assert_eq!(keys("intellij-windows"), Some(vec!["Ctrl+Alt+Insert".to_owned()]));
    assert_eq!(keys("vscode-macos"), Some(Vec::new()));
}
