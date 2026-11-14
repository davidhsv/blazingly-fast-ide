//! Expand to Level 2–5 and Expand All to Level 3–5 through their keys, the Unwrap/Remove chooser
//! with several statements around the caret, Next / Previous Occurrence in the Find Usages window,
//! and Reload All from Disk re-reading every folder the Project view has read.

use super::*;

/// Seven regions, each inside the one before: the one opened on line `d - 1` closes on line
/// `15 - d`.
const SEVEN_DEEP: &str = concat!(
    "fn a() {\n",
    "    if b {\n",
    "        if c {\n",
    "            if d {\n",
    "                if e {\n",
    "                    if f {\n",
    "                        if g {\n",
    "                            z();\n",
    "                        }\n",
    "                    }\n",
    "                }\n",
    "            }\n",
    "        }\n",
    "    }\n",
    "}\n",
);

/// Lines `0..=first_end` and `last_start..=15`: what shows with the regions between folded.
fn around(first_end: u32, last_start: u32) -> Vec<u32> {
    (0..=first_end).chain(last_start..=15).collect()
}

/// ⌘⇧* 2…5 folds the regions of the file nested deeper than that level; ⌘* 2…5 does the same
/// inside the region around the caret, which counts as level 1. While the Project view has the
/// keyboard, the keys fold nothing.
#[gpui_kit::test]
async fn expand_to_levels_two_to_five_fold_the_regions_nested_deeper(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("src/main.rs", SEVEN_DEEP)], "src/main.rs", None);
    f.wait_folds(cx);
    assert_eq!(f.text(cx).lines().count(), 15, "seven regions on fifteen lines");
    assert_eq!(f.shown_lines(cx), around(15, 16), "nothing folded");
    for level in 2..=5u32 {
        f.caret_to(cx, 0, 0);
        cx.simulate_keystrokes(&format!("cmd-shift-* {level}"));
        assert_eq!(f.shown_lines(cx), around(level, 15 - level), "Expand All to Level {level}");
    }
    f.run(cx, "code.unfoldAll");
    for level in 2..=5u32 {
        // In the body of `if b`, the region of level 1 for the key.
        f.caret_to(cx, 2, 4);
        cx.simulate_keystrokes(&format!("cmd-* {level}"));
        assert_eq!(f.shown_lines(cx), around(level + 1, 14 - level), "Expand to Level {level}");
    }
    f.run(cx, "code.unfoldAll");
    f.focus_project_tree(cx);
    for level in 2..=5u32 {
        cx.simulate_keystrokes(&format!("cmd-shift-* {level}"));
        cx.simulate_keystrokes(&format!("cmd-* {level}"));
        assert_eq!(f.shown_lines(cx), around(15, 16), "the editor does not have the keyboard");
        assert!(f.unavailable(cx, &format!("code.expandAllToLevel{level}")));
        assert!(f.unavailable(cx, &format!("code.expandToLevel{level}")));
    }
    f.close(cx);
}

/// With two statements around the caret, ⇧⌘⌦ lists them innermost first, each named "Unwrap
/// '<keyword>'" with its first line; choosing the outer one unwraps it and keeps the inner one. With
/// one statement around the caret there is no chooser: it is unwrapped at once.
#[gpui_kit::test]
async fn unwrap_with_several_statements_around_the_caret_lists_them_innermost_first(cx: &mut TestAppContext) {
    let text = "fn f() {\n    if a {\n        while b {\n            foo();\n        }\n    }\n}\n";
    let (f, cx) = open(cx, &[("src/main.rs", text)], "src/main.rs", None);
    f.caret_to(cx, 3, 12);
    cx.simulate_keystrokes("cmd-shift-delete");
    let list = f.popup_list(cx);
    let lines = list.read_with(cx, |list, _| list.lines());
    assert_eq!(lines, ["Unwrap 'while' · while b {", "Unwrap 'if' · if a {"]);
    assert_eq!(f.text(cx), text, "nothing changes while choosing");
    cx.simulate_keystrokes("down enter");
    settle(cx, |cx| f.text(cx) == "fn f() {\n    while b {\n        foo();\n    }\n}\n");
    assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()), "the chooser closed");

    // Only the `while` is around the caret now.
    f.caret_to(cx, 2, 8);
    cx.simulate_keystrokes("cmd-shift-delete");
    settle(cx, |cx| f.text(cx) == "fn f() {\n    foo();\n}\n");
    assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()), "no chooser");
    f.close(cx);
}

/// With the Find Usages window filled, ⌥⌘↓ selects the next usage, puts the caret on it and the
/// status bar says which of how many; ⌥⌘↑ steps back. Before any search there is nothing to step:
/// the caret stays and the status bar says nothing.
#[gpui_kit::test]
async fn next_and_previous_occurrence_step_the_usages_and_say_which_of_how_many(cx: &mut TestAppContext) {
    let text = "let shared = 1\nshared + shared\n";
    let capabilities = json!({ "referencesProvider": true });
    let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "capabilities": capabilities.clone() })));
    let a = f.root.join("a.mock");
    let at = |line: u32, col: u32| {
        let at = json!({ "line": line, "character": col });
        json!({ "uri": format!("file://{}", a.display()), "range": { "start": at, "end": at } })
    };
    f.answer(json!({ "capabilities": capabilities, "textDocument/references": [at(1, 0), at(1, 9)] }));
    let bar = f.workbench.read_with(cx, |w, _| w.status_bar().clone());
    let says = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| bar.read(cx).texts(window, cx)).into_iter().find(|shown| shown.contains(" of "))
    };
    f.caret_to(cx, 0, 5);
    cx.simulate_keystrokes("cmd-alt-down");
    idle(cx);
    assert_eq!(f.at(cx), Some((a.clone(), (0, 5))), "no usages to step through");
    assert_eq!(says(cx), None);

    f.run(cx, "find.usages");
    let view = f.workbench.read_with(cx, |w, _| w.usages_view()).expect("the Find Usages window is shown");
    settle(cx, |cx| view.read_with(cx, |view, _| view.search().is_some_and(|search| !search.loading)));
    f.focus_editor(cx);
    cx.simulate_keystrokes("cmd-alt-down");
    settle(cx, |cx| f.at(cx) == Some((a.clone(), (1, 0))));
    assert_eq!(says(cx).as_deref(), Some("1 of 2"));
    f.focus_editor(cx);
    cx.simulate_keystrokes("cmd-alt-down");
    settle(cx, |cx| f.at(cx) == Some((a.clone(), (1, 9))));
    assert_eq!(says(cx).as_deref(), Some("2 of 2"));
    f.focus_editor(cx);
    cx.simulate_keystrokes("cmd-alt-up");
    settle(cx, |cx| f.at(cx) == Some((a.clone(), (1, 0))));
    assert_eq!(says(cx).as_deref(), Some("1 of 2"));
    f.close(cx);
}

/// Reload All from Disk asks the backend again for the listing of every folder the Project view
/// has read (the project's folder and the three opened below it), all at once: a change the file
/// watcher reports re-reads only its own folder. Without the action, nothing is read again.
#[gpui_kit::test]
async fn reload_all_from_disk_re_reads_every_folder_the_project_view_has_read(cx: &mut TestAppContext) {
    let files = [("a.txt", "a\n"), ("src/lib/b.txt", "b\n"), ("docs/c.txt", "c\n")];
    let (f, cx) = open(cx, &files, "a.txt", None);
    let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
    let labels = |cx: &mut VisualTestContext| {
        tree.read_with(cx, |tree, _| tree.rows().iter().map(|row| row.node.label.to_string()).collect::<Vec<_>>())
    };
    let key = |path: &str| f.root.join(path).to_string_lossy().into_owned();
    settle(cx, |cx| labels(cx).contains(&"src".to_owned()));
    for folder in ["src", "src/lib", "docs"] {
        let folder = key(folder);
        tree.update(cx, |tree, cx| tree.toggle(&folder, cx));
    }
    settle(cx, |cx| ["b.txt", "c.txt"].iter().all(|label| labels(cx).contains(&(*label).to_owned())));
    let reads = |cx: &mut VisualTestContext| tree.read_with(cx, |tree, _| tree.folder_reads());
    // The reads stop once every opened folder is listed.
    let mut before = reads(cx);
    loop {
        idle(cx);
        let now = reads(cx);
        if now == before {
            break;
        }
        before = now;
    }
    idle(cx);
    assert_eq!(reads(cx), before, "nothing is read again without the action");

    f.run(cx, "file.reloadAllFromDisk");
    settle(cx, |cx| reads(cx) == before + 4);
    idle(cx);
    assert_eq!(reads(cx), before + 4, "one listing for each folder read before, and no more");
    f.close(cx);
}
