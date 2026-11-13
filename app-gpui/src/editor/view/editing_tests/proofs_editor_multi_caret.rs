use super::*;
use crate::platform::input::Keymap;
use crate::platform::keymap::{KeymapState, PRESET_TITLES};
use gpui_kit::{Modifiers, MouseButton};

const DOC: &str = "first\nsecond\nthird";

fn alt() -> Modifiers {
    Modifiers { alt: true, ..Modifiers::default() }
}

fn alt_shift() -> Modifiers {
    Modifiers { shift: true, ..alt() }
}

/// Makes `preset` with `overrides` (action id → bindings; an empty list removes the action's) the keymap.
fn use_keymap(cx: &mut VisualTestContext, preset: &str, overrides: &[(&str, &[&str])]) {
    let mut state = KeymapState::new(preset);
    state.set_overrides(
        overrides
            .iter()
            .map(|(id, keys)| ((*id).to_owned(), keys.iter().map(|key| (*key).to_owned()).collect()))
            .collect(),
    );
    cx.update(|_, cx| cx.set_global(Keymap(state)));
}

/// The editor on [`DOC`] with the app's input handling, the caret at 0.
async fn caret_editor(cx: &mut TestAppContext) -> (Opened, &mut VisualTestContext) {
    let (o, cx) = open(cx, DOC).await;
    cx.update(|_, cx| crate::platform::input::init(cx));
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    (o, cx)
}

/// Where `offset` is drawn, a little below the top of its line.
fn at(o: &Opened, cx: &mut VisualTestContext, offset: u32) -> Point<Pixels> {
    o.editor.read_with(cx, |e, _| e.position_of(offset).expect("painted")) + point(px(0.0), px(3.0))
}

fn drag(o: &Opened, cx: &mut VisualTestContext, from: u32, to: u32, modifiers: Modifiers) {
    let (from, to) = (at(o, cx, from), at(o, cx, to));
    cx.simulate_mouse_down(from, MouseButton::Left, modifiers);
    cx.simulate_mouse_move(to, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(to, MouseButton::Left, modifiers);
}

/// Every keymap preset binds Add or Remove Caret to ⌥⇧-click, and that click adds a caret.
#[gpui_kit::test]
async fn each_preset_keeps_the_default_gesture(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    for (preset, _) in PRESET_TITLES {
        use_keymap(cx, preset, &[]);
        let bindings = cx.update(|_, cx| cx.global::<Keymap>().0.bindings_for(ADD_CARET_ACTION).to_vec());
        assert_eq!(bindings, ["Alt+Shift+Button1"], "{preset}");
        o.editor.update(cx, |e, cx| e.select(0, 0, cx));
        let two = at(&o, cx, 2);
        cx.simulate_click(two, alt_shift());
        assert_eq!(selections(&o, cx), [(0, 0), (2, 2)], "{preset}");
    }
    o.host.close_project("w1");
}

/// Only the whole stroke with the left button adds a caret: ⌥ alone, ⌥⇧ with ⌃ or ⌘, and the middle button
/// do not.
#[gpui_kit::test]
async fn matches_the_complete_stroke_and_only_the_left_button(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    use_keymap(cx, "intellij-macos", &[]);
    let two = at(&o, cx, 2);
    cx.simulate_click(two, alt_shift());
    assert_eq!(selections(&o, cx), [(0, 0), (2, 2)]);
    let others = [alt(), Modifiers { control: true, ..alt_shift() }, Modifiers { platform: true, ..alt_shift() }];
    for modifiers in others {
        o.editor.update(cx, |e, cx| e.select(0, 0, cx));
        cx.simulate_click(two, modifiers);
        assert_eq!(selections(&o, cx).len(), 1, "{modifiers:?}: {:?}", selections(&o, cx));
    }
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.simulate_mouse_down(two, MouseButton::Middle, alt());
    cx.simulate_mouse_up(two, MouseButton::Middle, alt());
    assert_eq!(selections(&o, cx).len(), 1, "{:?}", selections(&o, cx));
    o.host.close_project("w1");
}

/// A rebinding takes effect at once: ⌥⇧-click no longer adds a caret, ⌥-click does, ⌥-drag adds a range
/// from the press, and the gesture on a caret removes it.
#[gpui_kit::test]
async fn uses_rebinding_immediately_and_yields_column_selection_to_the_caret_gesture(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    use_keymap(cx, "intellij-macos", &[(ADD_CARET_ACTION, &["Alt+Button1"])]);
    let two = at(&o, cx, 2);
    cx.simulate_click(two, alt_shift());
    assert_eq!(selections(&o, cx).len(), 1, "{:?}", selections(&o, cx));
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.simulate_click(two, alt());
    assert_eq!(selections(&o, cx), [(0, 0), (2, 2)]);
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    drag(&o, cx, 2, 8, alt());
    assert_eq!(selections(&o, cx), [(0, 0), (2, 8)]);
    o.editor.update(cx, |e, cx| e.set_selections(vec![(0, 0), (2, 2)], 1, cx));
    cx.simulate_click(two, alt());
    assert_eq!(selections(&o, cx), [(0, 0)]);
    o.host.close_project("w1");
}

/// With the binding removed neither ⌥⇧-click nor ⌥-click adds a caret; the default comes back with the
/// overrides cleared.
#[gpui_kit::test]
async fn honours_removal_of_the_caret_gesture_and_restores_it(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    use_keymap(cx, "intellij-macos", &[(ADD_CARET_ACTION, &[])]);
    let two = at(&o, cx, 2);
    for modifiers in [alt_shift(), alt()] {
        o.editor.update(cx, |e, cx| e.select(0, 0, cx));
        cx.simulate_click(two, modifiers);
        assert_eq!(selections(&o, cx).len(), 1, "{modifiers:?}: {:?}", selections(&o, cx));
    }
    use_keymap(cx, "intellij-macos", &[]);
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.simulate_click(two, alt_shift());
    assert_eq!(selections(&o, cx), [(0, 0), (2, 2)]);
    o.host.close_project("w1");
}

/// With the caret gesture removed from ⌥-click, a ⌥-drag from line 1 column 1 to line 3 column 3 selects
/// columns 1–3 of each line.
#[gpui_kit::test]
async fn restores_alt_drag_column_selection_once_the_caret_gesture_is_removed(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    use_keymap(cx, "intellij-macos", &[(ADD_CARET_ACTION, &[])]);
    drag(&o, cx, 1, 16, alt());
    assert_eq!(selections(&o, cx), [(1, 3), (7, 9), (14, 16)]);
    o.host.close_project("w1");
}

/// A binding written `alt+ctrl+button1` matches the ⌃⌥ press it normalizes to (right Alt on Windows), and
/// not ⌥ alone.
#[gpui_kit::test]
async fn matches_normalized_ctrl_alt_strokes_as_reported_for_right_alt_on_windows(cx: &mut TestAppContext) {
    let (o, cx) = caret_editor(cx).await;
    use_keymap(cx, "intellij-macos", &[(ADD_CARET_ACTION, &["alt+ctrl+button1"])]);
    let two = at(&o, cx, 2);
    cx.simulate_click(two, Modifiers { control: true, ..alt() });
    assert_eq!(selections(&o, cx), [(0, 0), (2, 2)]);
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.simulate_click(two, alt());
    assert_eq!(selections(&o, cx).len(), 1, "{:?}", selections(&o, cx));
    o.host.close_project("w1");
}
