//! Toggle Full Screen on the window it runs in.

use super::*;

/// Toggle Full Screen puts the window in full screen and a second run takes it out; on macOS ⌃⌘F
/// does the same.
#[gpui_kit::test]
async fn toggle_full_screen_puts_the_window_in_full_screen_and_takes_it_out(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let full = |cx: &mut VisualTestContext| cx.update(|window, _| window.is_fullscreen());
    assert!(!full(cx));
    run_action(cx, "window.toggleFullScreen", None);
    assert!(full(cx), "the window is in full screen");
    run_action(cx, "window.toggleFullScreen", None);
    assert!(!full(cx), "the window is out of full screen");
    if crate::platform::keymap::is_mac_host() {
        cx.simulate_keystrokes("cmd-ctrl-f");
        assert!(full(cx), "⌃⌘F puts it in full screen");
        cx.simulate_keystrokes("cmd-ctrl-f");
        assert!(!full(cx));
    }
}
