use super::proofs13_agents_failed_send::{fail_sends, wait_for_composer_focus};
use super::proofs13_agents_fixtures::*;
use super::*;

/// After an edited first message was rolled back and its send failed, the message sent again from the composer
/// shows as a sent message: no message is being edited and none is drawn as an editor.
#[gpui_kit::test]
async fn shows_a_message_sent_again_after_a_failed_edit_as_sent_not_in_an_editor(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let task = start_with(&workbench, &store, "Original message", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    click_on(cx, "session-user-0-edit");
    cx.simulate_input(" edited");
    fail_sends(cx, Some("Send failed"));
    click_on(cx, "session-edit-send");
    start_fresh(&workbench, cx);
    wait_long(cx, "the edit back in the composer", |cx| {
        store.read_with(cx, |store, _| store.composer(&task).text == "Original message edited")
    });
    wait_for_composer_focus(&workbench, cx);
    fail_sends(cx, None);
    click_on(cx, "composer-send");
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let (session, _) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    assert_eq!(session.read_with(cx, |session, _| session.editing()), None, "no message is being edited");
    assert!(cx.debug_bounds("session-edit-0").is_none(), "the new message is not drawn as an editor");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
