use super::*;

/// A shown tooltip hides when a key is pressed and when its window stops being the active one.
#[gpui_kit::test]
async fn keeps_dismissal_behavior_for_a_key_press_and_the_window_losing_the_focus(cx: &mut TestAppContext) {
    let cx = host(cx, "Project", true);
    cx.update(|window, _| window.activate_window());
    hover(cx);
    assert!(shown(cx));
    cx.simulate_keystrokes("escape");
    assert!(!shown(cx), "a key press hides the tooltip");

    cx.simulate_mouse_move(point(px(400.0), px(400.0)), None, Modifiers::default());
    hover(cx);
    assert!(shown(cx));
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(!shown(cx), "the window losing the focus hides the tooltip");
}
