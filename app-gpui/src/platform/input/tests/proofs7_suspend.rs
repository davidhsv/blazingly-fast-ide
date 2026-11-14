use super::*;

// src/features/settings/__tests__/keymap.test.ts › applyKeymapSettings › suspends the dispatcher while recording
/// While a window's shortcut dispatch is suspended its key runs nothing; suspensions nest, so it runs again once
/// each is resumed, and a resume more than there were suspensions changes nothing.
#[gpui_kit::test]
fn a_suspended_window_runs_no_shortcut_until_it_is_resumed(cx: &mut TestAppContext) {
    let (cx, ran) = setup(cx, &[("t.suspend", "Meta+Y", None)]);
    cx.update(|window, cx| {
        let id = window.window_handle().window_id();
        suspend(id, cx);
        suspend(id, cx);
    });
    cx.simulate_keystrokes("cmd-y");
    assert!(ran.borrow().is_empty(), "the key runs nothing while dispatch is suspended");
    cx.update(|window, cx| resume(window.window_handle().window_id(), cx));
    cx.simulate_keystrokes("cmd-y");
    assert!(ran.borrow().is_empty(), "one suspension is still in place");
    cx.update(|window, cx| {
        let id = window.window_handle().window_id();
        resume(id, cx);
        resume(id, cx);
    });
    cx.simulate_keystrokes("cmd-y");
    assert_eq!(*ran.borrow(), ["t.suspend"], "resumed, the key runs its action once");
}
