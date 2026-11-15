//! Settings › Code Folding › Fold by default, applied when a file opens, and the open chevrons
//! that show only while the pointer is over the gutter.

use super::*;
use ide_workspace::settings::SettingsScope;

/// A file header comment, two imports, a documentation comment and a function.
const HEADED: &str =
    "// Header one\n// Header two\nuse std::fmt;\nuse std::io;\n\n/// Doc one\n/// Doc two\nfn a() {\n    b();\n}\n";

/// The editor of `file`, opened from the project, once its folds are current.
fn show(f: &Fixture, file: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = f.root.join(file);
    let wanted = path.clone();
    f.workbench.update_in(cx, |w, window, cx| w.open_project_file(path, window, cx));
    settle(cx, |cx| f.editor(cx).is_some_and(|e| e.read_with(cx, |e, _| e.path() == wanted)));
    f.wait_folds(cx);
    f.active(cx)
}

/// With the defaults (file header and imports on, documentation comments off) a file opens with
/// its header comment and its imports folded and its documentation comment open; with
/// documentation comments on and imports off the next file opens with the header and the
/// documentation comment folded and the imports open.
#[gpui_kit::test]
async fn a_file_opens_with_the_regions_fold_by_default_names_folded(cx: &mut TestAppContext) {
    let files = [("src/a.rs", HEADED), ("src/b.rs", HEADED)];
    let (f, cx) = open(cx, &files, "src/a.rs", None);
    f.wait_folds(cx);
    settle(cx, |cx| f.shown_lines(cx) == [0, 2, 4, 5, 6, 7, 8, 9, 10]);
    let editor = f.active(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.fold_marker(0)), Some(FoldMarker::Closed));
    assert_eq!(editor.read_with(cx, |e, _| e.fold_marker(2)), Some(FoldMarker::Closed));
    assert_eq!(editor.read_with(cx, |e, _| e.fold_marker(5)), Some(FoldMarker::Open));

    cx.update(|_, cx| {
        for (key, value) in [("editor.folding.docComments", true), ("editor.folding.imports", false)] {
            settings::set("w1", SettingsScope::Global, key, &json!(value), cx).expect("the setting is stored");
        }
    });
    show(&f, "src/b.rs", cx);
    settle(cx, |cx| f.shown_lines(cx) == [0, 2, 3, 4, 5, 7, 8, 9, 10]);
    f.close(cx);
}

/// A line that starts an open fold shows its chevron only while the pointer is over the gutter; a
/// folded header shows its chevron all the time.
#[gpui_kit::test]
async fn open_chevrons_show_only_while_the_pointer_is_over_the_gutter(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
    f.wait_folds(cx);
    let editor = f.active(cx);
    let painted = |cx: &mut VisualTestContext| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        editor.read_with(cx, |e, _| e.painted_fold_markers())
    };
    let (chevron, _) = gutter_points(&editor, 1, cx);
    let text = editor.read_with(cx, |e, _| e.position_of(e.offset_of(3, 8)).expect("the line is painted"));
    cx.simulate_mouse_move(text, None, Modifiers::default());
    assert!(painted(cx).is_empty(), "no chevron while the pointer is over the text");
    cx.simulate_mouse_move(chevron, None, Modifiers::default());
    assert_eq!(painted(cx), [(0, FoldMarker::Open), (1, FoldMarker::Open), (2, FoldMarker::Open)]);
    cx.simulate_click(chevron, Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(text, None, Modifiers::default());
    assert_eq!(painted(cx), [(1, FoldMarker::Closed)], "the folded header keeps its chevron");
    f.close(cx);
}
