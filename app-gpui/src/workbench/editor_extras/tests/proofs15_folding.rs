//! The fold column of the gutter under Settings › Code Folding › Show code folding outline.

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
    let gutter = cx.update(|_, cx| theme::length(cx, "w-gutter"));
    let fold = px(crate::editor::view::FOLD_GUTTER);
    assert_eq!(editor.read_with(cx, |e, cx| e.fold_column_width(cx)), fold);
    assert_eq!(text_left(&editor, 0, cx), gutter + fold);

    let (chevron, _) = gutter_points(&editor, 1, cx);
    set_global("editor.folding.showOutline", json!(false), cx);
    settle(cx, |cx| editor.read_with(cx, |e, cx| e.fold_column_width(cx)) == px(0.0));
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
    settle(cx, |cx| editor.read_with(cx, |e, cx| e.fold_column_width(cx)) == fold);
    assert_eq!(text_left(&editor, 0, cx), gutter + fold);
    let (chevron, _) = gutter_points(&editor, 1, cx);
    cx.simulate_click(chevron, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(f.shown_lines(cx), [0, 1, 6, 7], "the chevron folds its region again");
    f.close(cx);
}
