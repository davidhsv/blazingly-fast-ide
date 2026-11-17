//! The prompt of Update Tag with Emmet on its own: what it asks, when OK can be pressed, and what it
//! reports.

use super::update_tag_dialog::{UpdateTagDialog, UpdateTagEvent};
use crate::platform::input;
use crate::theme;
use gpui_kit::{Entity, EntityInputHandler, Focusable, Modifiers, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<UpdateTagEvent>>>;

fn open<'a>(
    cx: &'a mut TestAppContext,
    tag: &'static str,
) -> (Entity<UpdateTagDialog>, Events, &'a mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let (dialog, cx) = cx.add_window_view(move |_, cx| UpdateTagDialog::new(tag, cx));
    let events: Events = Rc::default();
    let log = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&dialog, move |_, event: &UpdateTagEvent, _| log.borrow_mut().push(event.clone())).detach();
        window.activate_window();
        let focus = dialog.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (dialog, events, cx)
}

// Not from a spec: the prompt names the tag and shows the example; OK is off until something
// besides blanks is typed, then Enter reports the abbreviation as typed.
#[gpui_kit::test]
async fn the_prompt_names_the_tag_and_ok_waits_for_an_abbreviation(cx: &mut TestAppContext) {
    let (dialog, events, cx) = open(cx, "div");
    assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "Update Tag with Emmet");
    assert_eq!(
        dialog.update(cx, |dialog, cx| dialog.label(cx)),
        "Abbreviation for <div>, e.g. section.card#main[data-x=1]"
    );
    assert_eq!(
        dialog.update(cx, |dialog, cx| dialog.buttons(cx)),
        [("OK".to_owned(), false), ("Cancel".to_owned(), true)]
    );
    cx.simulate_keystrokes("enter");
    cx.simulate_input("  ");
    cx.simulate_keystrokes("enter");
    assert!(events.borrow().is_empty(), "nothing to apply yet");
    cx.simulate_input("section.b");
    assert!(dialog.read_with(cx, |dialog, cx| dialog.can_apply(cx)));
    cx.simulate_keystrokes("enter");
    assert_eq!(*events.borrow(), [UpdateTagEvent::Apply("  section.b".to_owned())]);
}

// Not from a spec: Escape and Cancel close the prompt without an abbreviation; Enter while an input
// method composes confirms the composed text only.
#[gpui_kit::test]
async fn escape_and_cancel_close_the_prompt_and_composing_text_is_not_applied(cx: &mut TestAppContext) {
    let (dialog, events, cx) = open(cx, "p");
    let field = dialog.read_with(cx, |dialog, _| dialog.field().clone());
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.replace_and_mark_text_in_range(None, "文本", None, window, cx));
    });
    cx.simulate_keystrokes("enter");
    assert!(events.borrow().is_empty());
    cx.update(|window, cx| field.update(cx, |field, cx| field.unmark_text(window, cx)));
    cx.simulate_keystrokes("escape");
    assert_eq!(*events.borrow(), [UpdateTagEvent::Cancelled]);
    cx.run_until_parked();
    let bounds = cx.debug_bounds("update-tag-cancel").expect("Cancel is drawn");
    cx.simulate_click(bounds.center(), Modifiers::default());
    assert_eq!(*events.borrow(), [UpdateTagEvent::Cancelled, UpdateTagEvent::Cancelled]);
}
