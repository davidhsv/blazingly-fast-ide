use super::*;
use crate::debugger::state::SessionStatus;
use crate::workbench::context_menus::EditorMenuCommand;
use crate::workbench::debug_extras::tests::PROGRAM;

const EVALUATE: &str = "Evaluate Expression…";

/// Opens the editor's menu by a right click on line `line`.
fn open_editor_menu(fixture: &Fixture, line: u32, cx: &mut VisualTestContext) -> Entity<PopupMenu<EditorMenuCommand>> {
    let editor = fixture.editor(cx);
    let at = editor.read_with(cx, |editor, cx| {
        let at = editor.position_of(editor.offset_of(line, 0)).expect("the line is painted");
        point(at.x + px(2.0), at.y + editor.line_height(cx) / 2.0)
    });
    right_click(cx, at);
    fixture.workbench.read_with(cx, |w, _| w.open_menu::<EditorMenuCommand>()).expect("the menu")
}

/// The rows of the editor's menu opened on line `line`; the menu is closed again.
fn editor_menu_rows(fixture: &Fixture, line: u32, cx: &mut VisualTestContext) -> Vec<String> {
    let menu = open_editor_menu(fixture, line, cx);
    let rows = shown(&menu, cx).remove(0);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    rows
}

/// The editor's menu offers Evaluate Expression… while the debug session is paused, and choosing it opens the
/// Evaluate dialog; before the session pauses and once it has ended, the menu does not offer it.
#[gpui_kit::test]
async fn the_editors_menu_offers_evaluate_expression_while_the_session_is_paused(cx: &mut TestAppContext) {
    let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.fake", PROGRAM)], Some("main.fake"));
    let rows = editor_menu_rows(&fixture, 1, cx);
    assert!(!rows.iter().any(|row| row == EVALUATE), "no session: {rows:?}");

    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint", None);
    fixture.select_main(cx);
    run(cx, "run.debug", None);
    long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
    long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:2"]);
    let menu = open_editor_menu(&fixture, 1, cx);
    assert!(shown(&menu, cx)[0].iter().any(|row| row == EVALUATE), "{:?}", shown(&menu, cx));
    choose(&menu, &[EVALUATE], cx);
    assert!(fixture.workbench.read_with(cx, |w, _| w.evaluate_dialog().is_some()), "the Evaluate dialog opens");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    run(cx, "debug.stop", None);
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    let rows = editor_menu_rows(&fixture, 1, cx);
    assert!(!rows.iter().any(|row| row == EVALUATE), "the session ended: {rows:?}");
    fixture.close_project(cx);
}
