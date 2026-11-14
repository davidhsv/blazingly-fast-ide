use super::*;

/// The primary modifier of the host's shortcuts: ⌘ on macOS, Ctrl elsewhere.
#[cfg(target_os = "macos")]
const PRIMARY: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const PRIMARY: &str = "ctrl";

/// The editor's own keys leave the shortcuts IDEA gives to other actions alone (⌘/Ctrl+Enter is Split
/// Line, Ctrl+M and Ctrl+L scroll to the caret, ⌘/Ctrl+[ ] go to the code block's ends, ⌥⌘/Ctrl+Alt+↑↓
/// are Previous / Next Occurrence): pressed with no keymap they neither insert a blank line, select
/// the line, nor add a caret. ⇧ with the primary modifier and Z redoes, Ctrl+Shift+Z on Windows too.
#[gpui_kit::test]
async fn drops_codemirror_defaults_that_idea_uses_for_other_actions(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("notes.txt", "alpha\nbeta\n")]).await;
    f.editor.update(cx, |e, cx| e.select(2, 2, cx));
    let state = |cx: &mut VisualTestContext| f.editor.read_with(cx, |e, _| (e.text(), e.selections().to_vec()));
    let before = state(cx);
    assert_eq!(before, ("alpha\nbeta\n".to_owned(), vec![(2, 2)]));
    for keys in [
        format!("{PRIMARY}-enter"),
        "ctrl-m".to_owned(),
        "ctrl-l".to_owned(),
        format!("{PRIMARY}-alt-up"),
        format!("{PRIMARY}-alt-down"),
        format!("{PRIMARY}-["),
        format!("{PRIMARY}-]"),
    ] {
        cx.simulate_keystrokes(&keys);
        assert_eq!(state(cx), before, "{keys} is left to the keymap");
    }
    assert_eq!(f.backend_text(), "alpha\nbeta\n");

    cx.simulate_input("x");
    assert_eq!(f.backend_text(), "alxpha\nbeta\n");
    cx.simulate_keystrokes(&format!("{PRIMARY}-z"));
    assert_eq!(f.backend_text(), "alpha\nbeta\n");
    cx.simulate_keystrokes(&format!("{PRIMARY}-shift-z"));
    assert_eq!(f.backend_text(), "alxpha\nbeta\n");
    assert_eq!(f.editor.read_with(cx, |e, _| e.caret()), 3);
    f.host.close_project("w1");
}
