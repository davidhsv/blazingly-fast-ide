use super::*;

const ANSWERS: &str =
    r#"{ "textDocument/hover": { "contents": { "kind": "markdown", "value": "**bar** is a name" } } }"#;

/// Runs the clock past the documentation delay, so that whatever would show has shown.
fn past_the_hover_delay(cx: &mut VisualTestContext) {
    for _ in 0..30 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
}

/// The hover's lines as shown.
fn hover_lines(f: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    let hover = f.intelligence(cx).read_with(cx, |this, _| this.hover().clone());
    hover.read_with(cx, |hover, _| hover.lines())
}

/// Where the pointer rests over the character at `offset`.
fn over(f: &Fixture, cx: &mut VisualTestContext, offset: u32) -> gpui_kit::Point<gpui_kit::Pixels> {
    let editor = f.editor(cx).expect("an editor");
    let (at, line_height) =
        editor.read_with(cx, |editor, cx| (editor.position_of(offset).expect("painted"), editor.line_height(cx)));
    gpui_kit::point(at.x + px(1.0), at.y + line_height / 2.0)
}

/// The project with `a.mock` open, its server attached and answering the documentation of `bar`.
fn documented(cx: &mut TestAppContext) -> (Fixture, &mut VisualTestContext) {
    let answers: Value = serde_json::from_str(ANSWERS).expect("the answers are JSON");
    let (f, cx) = open(cx, &[("a.mock", "foo bar baz\n")], "a.mock", answers);
    settle(cx, |_| !f.requests("textDocument/didOpen").is_empty());
    (f, cx)
}

/// The navigation modifier (⌘ on macOS, Ctrl elsewhere) held alone owns the pointer: resting with it on a
/// name shows no documentation; resting there without it does.
#[gpui_kit::test]
async fn the_plain_hover_yields_while_the_navigation_modifier_is_held(cx: &mut TestAppContext) {
    let (f, cx) = documented(cx);
    let navigation = cx.update(|_, cx| Modifiers::secondary_key(cx));
    let at = over(&f, cx, 5);
    cx.simulate_mouse_move(at, None, navigation);
    past_the_hover_delay(cx);
    assert!(hover_lines(&f, cx).is_empty(), "{:?}", hover_lines(&f, cx));

    let at = over(&f, cx, 6);
    cx.simulate_mouse_move(at, None, Modifiers::default());
    settle(cx, |cx| !hover_lines(&f, cx).is_empty());
    assert_eq!(hover_lines(&f, cx), ["bar is a name"]);
    f.close(cx);
}

/// Pressing the navigation modifier while the documentation shows closes it at once, the pointer still.
#[gpui_kit::test]
async fn pressing_the_navigation_modifier_closes_an_open_hover(cx: &mut TestAppContext) {
    let (f, cx) = documented(cx);
    let at = over(&f, cx, 5);
    cx.simulate_mouse_move(at, None, Modifiers::default());
    settle(cx, |cx| !hover_lines(&f, cx).is_empty());
    let navigation = cx.update(|_, cx| Modifiers::secondary_key(cx));
    cx.simulate_modifiers_change(navigation);
    cx.run_until_parked();
    assert!(hover_lines(&f, cx).is_empty(), "the hover stays open while ⌘ is held: {:?}", hover_lines(&f, cx));
    f.close(cx);
}
