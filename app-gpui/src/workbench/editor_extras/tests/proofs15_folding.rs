//! The fold column of the gutter under Settings › Code Folding › Show code folding outline, the
//! open chevrons that show only while the pointer is over the gutter, and Fold by default applied
//! when a file opens.

use super::*;
use ide_workspace::settings::SettingsScope;

/// Sets the global setting `key` to `value` as Settings does.
fn set_global(key: &str, value: Value, cx: &mut VisualTestContext) {
    cx.update(|_, cx| settings::set("w1", SettingsScope::Global, key, &value, cx)).expect("the setting is stored");
    cx.run_until_parked();
}

/// Where the editor last painted the start of the text of line `line`, from its left edge.
fn text_left(editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) -> Pixels {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    editor.read_with(cx, |e, _| {
        let at = e.position_of(e.offset_of(line, 0)).expect("the line is painted");
        at.x - e.painted_bounds().expect("the editor is painted").left()
    })
}

/// With Show code folding outline off the gutter has no fold column: the text starts right after
/// the line numbers and the icons, and a click where a chevron was lands in the text.
/// The folding commands still fold. Turned on again, the column and its chevrons are back.
#[gpui_kit::test]
async fn the_fold_column_is_drawn_only_while_show_code_folding_outline_is_on(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("src/main.rs", NESTED)], "src/main.rs", None);
    f.wait_folds(cx);
    let editor = f.active(cx);
    // The numbers' column (32, its padding included), the bookmarks' 15, the run marks' 14 of a Rust
    // source, the change bars' 4 and the gutter's 1 px hairline.
    let gutter = px(32.0 + 15.0 + 14.0 + 4.0 + 1.0);
    let fold = px(crate::editor::view::FOLD_GUTTER);
    assert_eq!(editor.read_with(cx, |e, cx| e.fold_column_width(cx)), fold);
    assert_eq!(text_left(&editor, 0, cx), gutter + fold);

    let (chevron, _) = gutter_points(&editor, 1, cx);
    set_global("editor.folding.showOutline", json!(false), cx);
    settle(cx, "no fold column", |cx| editor.read_with(cx, |e, cx| e.fold_column_width(cx)) == px(0.0));
    assert_eq!(text_left(&editor, 0, cx), gutter, "the text starts where the fold column was");
    // Where the chevron of line 1 was drawn is now text: a click there puts the caret on the line
    // and folds nothing.
    let presses = count_gutter_presses(&editor, cx);
    cx.simulate_click(chevron, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7], "no chevron folds the region");
    assert_eq!(presses.get(), 0, "the click is not the gutter's");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position().0), 1);
    // Folding still works from the commands.
    f.caret_to(cx, 3, 12);
    f.run(cx, "code.fold");
    assert_eq!(f.shown_lines(cx), [0, 1, 2, 5, 6, 7]);
    f.run(cx, "code.unfoldAll");
    assert_eq!(f.shown_lines(cx), [0, 1, 2, 3, 4, 5, 6, 7]);

    set_global("editor.folding.showOutline", json!(true), cx);
    settle(cx, "the fold column", |cx| editor.read_with(cx, |e, cx| e.fold_column_width(cx)) == fold);
    assert_eq!(text_left(&editor, 0, cx), gutter + fold);
    let (chevron, _) = gutter_points(&editor, 1, cx);
    cx.simulate_click(chevron, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(f.shown_lines(cx), [0, 1, 6, 7], "the chevron folds its region again");
    f.close(cx);
}

/// A file header comment, two imports, a documentation comment and a function. The documentation
/// comment is a block: the backend offers no fold for a run of Rust `///` lines.
const HEADED: &str =
    "// Header one\n// Header two\nuse std::fmt;\nuse std::io;\n\n/**\n * Doc\n */\nfn a() {\n    b();\n}\n";

/// The editor of `file`, opened from the project, once its folds are current.
fn show(f: &Fixture, file: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = f.root.join(file);
    let wanted = path.clone();
    f.workbench.update_in(cx, |w, window, cx| w.open_project_file(path, window, cx));
    settle(cx, "the editor of the file", |cx| f.editor(cx).is_some_and(|e| e.read_with(cx, |e, _| e.path() == wanted)));
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
    settle(cx, "the shown lines", |cx| f.shown_lines(cx) == [0, 2, 4, 5, 6, 7, 8, 9, 10, 11]);
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
    settle(cx, "the shown lines", |cx| f.shown_lines(cx) == [0, 2, 3, 4, 5, 8, 9, 10, 11]);
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

/// A file opened at a line below its folded header and imports keeps that line where the opening
/// showed it: the rows folded away above it do not push it out of view.
#[gpui_kit::test]
async fn folding_by_default_keeps_the_line_a_file_is_opened_at_in_view(cx: &mut TestAppContext) {
    let mut text = String::from("// Header one\n// Header two\n");
    text.extend((0..20).map(|n| format!("use m{n};\n")));
    text.extend((0..200).map(|n| format!("fn f{n}() {{}}\n")));
    let (f, cx) = open(cx, &[("src/a.rs", "fn a() {}\n"), ("src/b.rs", &text)], "src/a.rs", None);
    let path = f.root.join("src/b.rs");
    let wanted = path.clone();
    f.workbench.update_in(cx, |w, window, cx| w.open_project_file_at(path, 150, 0, window, cx));
    settle(cx, "the editor of the file", |cx| f.editor(cx).is_some_and(|e| e.read_with(cx, |e, _| e.path() == wanted)));
    f.wait_folds(cx);
    let editor = f.active(cx);
    settle(cx, "the shown lines", |cx| editor.read_with(cx, |e, _| e.shown_lines().len()) == 223 - 1 - 19);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let (caret, bounds) = editor.read_with(cx, |e, _| (e.position_of(e.offset_of(150, 0)), e.painted_bounds()));
    let caret = caret.expect("the line opened at is painted");
    let bounds = bounds.expect("the editor is painted");
    assert!(bounds.top() <= caret.y && caret.y < bounds.bottom(), "{caret:?} in {bounds:?}");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position()), (150, 0));
    f.close(cx);
}
