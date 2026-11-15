use super::*;
use crate::platform::settings::{SettingsWrite, set_global};
use crate::vcs_more::viewer_look::CaretMotion;

fn appearance(cx: &mut VisualTestContext, smooth: bool, style: &str) {
    let write = [
        SettingsWrite { key: "editor.appearance.smoothCaretMovement", value: serde_json::json!(smooth) },
        SettingsWrite { key: "editor.appearance.caretMovementStyle", value: serde_json::json!(style) },
    ];
    cx.update(|_, cx| set_global("w1", &write, cx)).expect("the settings are stored");
}

fn glides(f: &Fixture, cx: &mut VisualTestContext) -> bool {
    f.editor.read_with(cx, |editor, _| editor.caret_glides())
}

/// Moves the clock on by `millis` and runs what became due.
fn wait(cx: &mut VisualTestContext, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
}

/// The chosen curve sets how long a move takes: Gliding travels for 180 ms, Snappy lands within 110 ms;
/// with Smooth caret movement off the caret jumps.
#[gpui_kit::test]
async fn a_move_follows_the_chosen_movement_curve(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    f.set(cx, "ab|c\nline two");
    appearance(cx, true, "gliding");
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().caret_motion == Some(CaretMotion::Gliding)));
    cx.simulate_keystrokes("left");
    assert!(glides(&f, cx));
    wait(cx, 90);
    // Half way through its time the frame draws the caret on its way to offset 1, not yet there.
    let drawn = f.editor.read_with(cx, |editor, _| editor.painted_caret_glide());
    assert!(drawn.is_some_and(|(head, progress)| head == 1 && progress > 0.0 && progress < 1.0), "{drawn:?}");
    wait(cx, 60);
    assert!(glides(&f, cx), "a gliding move is still on its way after 150 ms");
    wait(cx, 50);
    assert!(!glides(&f, cx));

    appearance(cx, true, "snappy");
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().caret_motion == Some(CaretMotion::Snappy)));
    cx.simulate_keystrokes("right");
    assert!(glides(&f, cx));
    wait(cx, 120);
    assert!(!glides(&f, cx), "a snappy move has landed after 120 ms");

    appearance(cx, false, "snappy");
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().caret_motion.is_none()));
    cx.simulate_keystrokes("left");
    assert!(!glides(&f, cx));
    close(cx);
}

/// A move of the selection alone glides; typing puts the caret in place at once, and so does a change
/// in the number of carets.
#[gpui_kit::test]
async fn glides_selection_only_moves_and_puts_the_caret_in_place_at_once_when_typing(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    f.set(cx, "ab|c");
    appearance(cx, true, "snappy");
    settle(cx, |cx| f.editor.read_with(cx, |e, _| e.facts().caret_motion.is_some()));
    f.set(cx, "ab|c");
    f.editor.update(cx, |editor, cx| editor.select(1, 1, cx));
    assert!(glides(&f, cx));
    cx.simulate_input("x");
    assert_eq!(f.shown(cx), "ax|bc");
    assert!(!glides(&f, cx));
    f.editor.update(cx, |editor, cx| editor.set_selections(vec![(0, 0), (3, 3)], 1, cx));
    assert!(!glides(&f, cx));
    close(cx);
}
