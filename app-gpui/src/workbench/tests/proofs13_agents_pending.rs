use super::proofs13_agents_fixtures::*;
use super::*;

/// A click on an image of a sent message opens it in a dialog, which Close closes.
#[gpui_kit::test]
async fn opens_an_image_of_a_sent_message_in_a_dialog(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let images = vec![png("a.png", "AAA"), png("b.png", "BBB")];
    let task = start_with(&workbench, &store, "Look", Vec::new(), images, cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    click_on(cx, "session-user-0-image-0");
    assert!(cx.debug_bounds("session-image-dialog").is_some(), "the image opens in a dialog");
    click_on(cx, "session-image-dialog-close");
    assert!(cx.debug_bounds("session-image-dialog").is_none());
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// The docked conversation shows the task's changed files only on request: Show changes replaces the conversation
/// and its composer by the changes, the task stays unread meanwhile, and Back to conversation brings back the same
/// conversation and composer with the draft typed before.
#[gpui_kit::test]
async fn keeps_docked_changes_opt_in_preserves_the_draft_and_returns_to_the_same_conversation(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let task = start_with(&workbench, &store, "Change a file", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let (session, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    assert!(cx.debug_bounds("agents-changes").is_none(), "the changes are not shown unasked");
    store.update(cx, |store, cx| {
        store.update_composer(&task, |draft| draft.text = "Draft to keep".to_owned());
        cx.notify();
    });
    click_on(cx, "agents-show-changes");
    assert!(cx.debug_bounds("agents-changes").is_some());
    assert!(cx.debug_bounds("session-scroll").is_none() && cx.debug_bounds("composer-send").is_none());
    click_on(cx, "agents-back-to-conversation");
    let (again, composer_again) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation");
    assert_eq!(again.entity_id(), session.entity_id(), "the same conversation");
    assert_eq!(composer_again.entity_id(), composer.entity_id(), "the same composer");
    assert_eq!(store.read_with(cx, |store, _| store.composer(&task).text), "Draft to keep");
    assert!(shows(&workbench, "agent: Answer 1", cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
