use super::*;

/// The tone the hint at the caret of the active editor is drawn with, while one shows.
fn drawn_tone(f: &Fixture, cx: &mut VisualTestContext) -> Option<Tone> {
    let editor = f.active(cx);
    let intelligence = cx.update(|_, cx| EditorIntelligence::of(&editor, cx)).expect("the editor's intelligence");
    let view = intelligence.read_with(cx, |this, _| this.hint_view().clone());
    view.read_with(cx, |view, _| view.tone())
}

/// Shows `text` as the hint with `tone` (the default without) and waits until it shows.
fn show(f: &Fixture, cx: &mut VisualTestContext, text: &str, tone: Option<&str>) {
    let arg = match tone {
        Some(tone) => json!({ "text": text, "tone": tone }),
        None => json!({ "text": text }),
    };
    f.run_with(cx, "editor.showHint", Some(arg));
    f.wait_hint(cx, text);
}

/// Show Editor Hint draws its hint in the tone it is given: info without one, warning and error when asked.
#[gpui_kit::test]
async fn show_editor_hint_draws_the_hint_in_the_tone_it_is_given(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "alpha\n")], "a.txt", None, &[]);
    show(&f, cx, "Plain", None);
    assert_eq!(drawn_tone(&f, cx), Some(Tone::Info));
    show(&f, cx, "Careful", Some("warning"));
    assert_eq!(drawn_tone(&f, cx), Some(Tone::Warning));
    show(&f, cx, "Broken", Some("error"));
    assert_eq!(drawn_tone(&f, cx), Some(Tone::Error));
    f.close(cx);
}

/// A hint at the caret goes away by itself after four seconds, and not before.
#[gpui_kit::test]
async fn a_hint_at_the_caret_goes_away_after_four_seconds(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "alpha\n")], "a.txt", None, &[]);
    show(&f, cx, "A hint", None);
    cx.executor().advance_clock(Duration::from_millis(3_000));
    cx.run_until_parked();
    assert_eq!(f.hint(cx).as_deref(), Some("A hint"), "three seconds on it still shows");
    cx.executor().advance_clock(Duration::from_millis(1_100));
    cx.run_until_parked();
    assert_eq!(f.hint(cx), None, "after four seconds it is gone");
    assert_eq!(drawn_tone(&f, cx), None, "and no longer drawn");
    f.close(cx);
}

/// The hint at the caret goes away with the next edit, with the next caret move, and with Escape.
#[gpui_kit::test]
async fn an_edit_a_caret_move_or_escape_dismisses_the_hint_at_the_caret(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "alpha\n")], "a.txt", None, &[]);
    show(&f, cx, "Before an edit", None);
    cx.simulate_input("x");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "xalpha\n");
    assert_eq!(f.hint(cx), None, "an edit dismisses the hint");

    show(&f, cx, "Before a caret move", None);
    cx.simulate_keystrokes("right");
    cx.run_until_parked();
    assert_eq!(f.caret(cx), 2);
    assert_eq!(f.hint(cx), None, "a caret move dismisses the hint");

    show(&f, cx, "Before Escape", None);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(f.hint(cx), None, "Escape dismisses the hint");
    assert_eq!(f.text(cx), "xalpha\n", "Escape changes nothing else");
    f.close(cx);
}
