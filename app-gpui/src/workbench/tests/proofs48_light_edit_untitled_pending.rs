//! File › New › Untitled File in a window that shows nothing: the window turns into a LightEdit
//! window without a file, rooted at the home folder (the test's own), with the new buffer as its tab
//! holding the keyboard; Save As opens at the home folder there, and the saved file stays a tab.

use super::proofs48_untitled::{active_input, all_inputs, close_backend};
use super::*;
use crate::editor_model::untitled::untitled_input;

/// Whether the window edits loose files in tabs (LightEdit).
fn light_edit(workbench: &Workbench) -> bool {
    matches!(workbench.content, Content::Editor { .. })
}

#[gpui_kit::test]
async fn new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    assert!(workbench.read_with(cx, |w, _| w.welcome().is_some()));

    run_action(cx, "file.newUntitled", None);
    settle(cx, |cx| {
        active_input(&workbench, cx) == Some(untitled_input("untitled:Untitled-1"))
            && workbench.read_with(cx, |w, _| w.editor().is_some())
    });
    assert!(workbench.read_with(cx, |w, _| light_edit(w) && w.welcome().is_none()));
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let project = host.project("w1").expect("the window has a LightEdit project");
    assert!(project.light);
    assert_eq!(project.root.canonicalize().ok(), home.path().canonicalize().ok(), "rooted at the test's home");
    assert_eq!(workbench.read_with(cx, |w, cx| w.title(cx)), "Untitled-1");

    // The new buffer has the keyboard.
    cx.simulate_input("draft");
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    settle(cx, |cx| editor.read_with(cx, |e, _| e.text() == "draft" && e.is_dirty()));

    run_action(cx, "file.save", None);
    let saved = home.path().join("notes/new.txt");
    let target = saved.clone();
    cx.simulate_new_path_selection(move |_| Some(target));
    wait_long(cx, "the buffer to be written", |cx| {
        std::fs::read_to_string(&saved).is_ok_and(|text| text == "draft\n")
            && !editor.read_with(cx, |e, _| e.is_dirty())
    });
    let inputs = all_inputs(&workbench, cx);
    assert_eq!(inputs.len(), 1);
    assert_eq!((inputs[0].kind.as_str(), inputs[0].path.as_deref()), ("file", saved.to_str()));
    assert!(workbench.read_with(cx, |w, _| light_edit(w)), "the window still edits loose files");
    close_backend(cx);
}
