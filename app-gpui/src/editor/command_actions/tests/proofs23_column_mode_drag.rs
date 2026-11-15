use super::*;
use gpui_kit::{Modifiers, MouseButton, Pixels, Point, point};

/// Where `offset` is drawn, a little below the top of its line.
fn at(f: &Fixture, cx: &mut VisualTestContext, offset: u32) -> Point<Pixels> {
    f.editor.read_with(cx, |e, _| e.position_of(offset).expect("painted")) + point(px(0.0), px(3.0))
}

fn drag(f: &Fixture, cx: &mut VisualTestContext, from: u32, to: u32) {
    let (from, to) = (at(f, cx, from), at(f, cx, to));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
}

/// With Column Selection Mode on, a plain drag selects a block of columns; turned off, the same drag
/// selects the text between the press and the release.
#[gpui_kit::test]
async fn column_selection_mode_makes_a_plain_drag_select_a_block(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    f.set(cx, "|first\nsecond\nthird");
    run(cx, "editor.toggleColumnSelection");
    drag(&f, cx, 1, 16);
    assert_eq!(f.editor.read_with(cx, |e, _| e.selections().to_vec()), [(1, 3), (7, 9), (14, 16)]);
    run(cx, "editor.toggleColumnSelection");
    f.editor.update(cx, |e, cx| e.select(0, 0, cx));
    drag(&f, cx, 1, 16);
    assert_eq!(f.editor.read_with(cx, |e, _| e.selections().to_vec()), [(1, 16)]);
    close(cx);
}
