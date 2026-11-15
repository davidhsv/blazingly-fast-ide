use super::*;
use gpui_kit::{Modifiers, MouseButton};

const DOC: &str = "first\nsecond\nthird";

/// Where `offset` is drawn, a little below the top of its line.
fn at(o: &Opened, cx: &mut VisualTestContext, offset: u32) -> Point<Pixels> {
    o.editor.read_with(cx, |e, _| e.position_of(offset).expect("painted")) + point(px(0.0), px(3.0))
}

fn middle_drag(o: &Opened, cx: &mut VisualTestContext, from: u32, to: u32, modifiers: Modifiers) {
    let (from, to) = (at(o, cx, from), at(o, cx, to));
    cx.simulate_mouse_down(from, MouseButton::Middle, modifiers);
    cx.simulate_mouse_move(to, MouseButton::Middle, modifiers);
    cx.simulate_mouse_up(to, MouseButton::Middle, modifiers);
}

/// A middle-button drag from line 1 column 1 to line 3 column 3 selects columns 1–3 of each line, except
/// on Linux, where the middle button is left to the system; with ⌥ held it selects nothing.
#[gpui_kit::test]
async fn preserves_middle_button_column_selection_except_on_linux(cx: &mut TestAppContext) {
    let (o, cx) = open(cx, DOC).await;
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    middle_drag(&o, cx, 1, 16, Modifiers::default());
    if cfg!(target_os = "linux") {
        assert_eq!(selections(&o, cx), [(0, 0)]);
    } else {
        assert_eq!(selections(&o, cx), [(1, 3), (7, 9), (14, 16)]);
    }
    // Once the button is up, moving the pointer leaves the block alone.
    let before = selections(&o, cx);
    let elsewhere = at(&o, cx, 4);
    cx.simulate_mouse_move(elsewhere, None, Modifiers::default());
    assert_eq!(selections(&o, cx), before);
    o.editor.update(cx, |e, cx| e.select(0, 0, cx));
    middle_drag(&o, cx, 1, 16, Modifiers { alt: true, ..Modifiers::default() });
    assert_eq!(selections(&o, cx), [(0, 0)]);
    o.host.close_project("w1");
}

/// The middle button pressed over the gutter neither moves the caret nor selects a block.
#[gpui_kit::test]
async fn a_middle_press_on_the_gutter_selects_nothing(cx: &mut TestAppContext) {
    let (o, cx) = open(cx, DOC).await;
    o.editor.update(cx, |e, cx| e.select(2, 2, cx));
    let bounds = o.editor.read_with(cx, |e, _| e.painted_bounds().expect("painted"));
    let (second_line, end) = (at(&o, cx, 7), at(&o, cx, 16));
    let gutter = point(bounds.left() + px(2.0), second_line.y);
    cx.simulate_mouse_down(gutter, MouseButton::Middle, Modifiers::default());
    cx.simulate_mouse_move(end, MouseButton::Middle, Modifiers::default());
    cx.simulate_mouse_up(end, MouseButton::Middle, Modifiers::default());
    assert_eq!(selections(&o, cx), [(2, 2)]);
    o.host.close_project("w1");
}
