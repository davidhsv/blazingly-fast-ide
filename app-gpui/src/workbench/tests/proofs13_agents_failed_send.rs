use super::proofs13_agents_fixtures::*;
use super::*;
use crate::workbench::agents::spawn_guard::FailingPrompts;

/// From now on every message sent to an agent fails with `error` before it reaches the backend (`None`: they go
/// through again).
pub(super) fn fail_sends(cx: &mut VisualTestContext, error: Option<&str>) {
    let failing = FailingPrompts(error.map(str::to_owned));
    cx.update(|_, cx| cx.set_global(failing));
}

/// The methods the agent has read, in order.
pub(super) fn agent_calls(folder: &tempfile::TempDir) -> Vec<String> {
    log_lines(folder, "agent-calls.log")
}

pub(super) fn count(calls: &[String], method: &str) -> usize {
    calls.iter().filter(|call| *call == method).count()
}

/// Waits for the docked composer to have the keyboard.
pub(super) fn wait_for_composer_focus(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    wait_long(cx, "the composer to take the keyboard", |cx| {
        cx.update(|window, cx| composer.focus_handle(cx).is_focused(window))
    });
}

/// Regenerate Response on the first reply of a message sent with an image, once a fresh session is confirmed, rolls
/// the conversation back and sends the message again; when that send fails, the message and its image go back to
/// the composer above what was typed there meanwhile (`draft`: a text, a file and an image), which takes the
/// keyboard. Send from the composer then sends all of it, without rolling back again.
fn recovers_images_after_a_failed_regeneration(cx: &mut TestAppContext, draft: bool) {
    let home = start(cx);
    let (folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let image = png("shot.png", "AAA");
    let task = start_with(&workbench, &store, "Original message", Vec::new(), vec![image.clone()], cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let added = png("added.png", "BBB");
    let files = if draft { vec![readme(&folder)] } else { Vec::new() };
    if draft {
        let (files, added) = (files.clone(), added.clone());
        store.update(cx, |store, cx| {
            store.update_composer(&task, |composer| {
                composer.text = "Another draft @README.md".to_owned();
                composer.files = files;
                composer.images = vec![added];
            });
            cx.notify();
        });
    }

    fail_sends(cx, Some("Send failed"));
    click_on(cx, "session-reply-1-regenerate");
    start_fresh(&workbench, cx);
    let text = if draft { "Original message\nAnother draft @README.md" } else { "Original message" };
    wait_long(cx, "the message back in the composer", |cx| {
        store.read_with(cx, |store, _| store.composer(&task).text == text)
    });
    let restored = store.read_with(cx, |store, _| store.composer(&task));
    let images = if draft { vec![image, added] } else { vec![image] };
    assert_eq!(restored.images, images);
    assert_eq!(restored.files, files);
    wait_for_composer_focus(&workbench, cx);

    fail_sends(cx, None);
    click_on(cx, "composer-send");
    wait_long(cx, "the message to reach the agent", |_| agent_prompts(&folder).len() == 2);
    let sent = agent_prompts(&folder).pop().expect("the second prompt");
    let data: Vec<&str> = images.iter().map(|image| image.data.as_str()).collect();
    assert_eq!(sent["text"], serde_json::json!(text), "{sent}");
    assert_eq!(sent["images"], serde_json::json!(data), "{sent}");
    assert_eq!(sent["files"], serde_json::json!(if draft { vec!["README.md"] } else { Vec::new() }), "{sent}");
    let calls = agent_calls(&folder);
    assert_eq!(count(&calls, "session/new"), 2, "rolled back once, by a fresh session: {calls:?}");
    assert_eq!(count(&calls, "_openidea/rollback"), 0, "{calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

#[gpui_kit::test]
async fn recovers_images_after_failed_regeneration_without_a_new_draft(cx: &mut TestAppContext) {
    recovers_images_after_a_failed_regeneration(cx, false);
}

#[gpui_kit::test]
async fn recovers_images_after_failed_regeneration_with_a_new_draft(cx: &mut TestAppContext) {
    recovers_images_after_a_failed_regeneration(cx, true);
}

/// Edit on the first message, a change and Send: once a fresh session is confirmed the conversation is rolled back
/// (the editor leaves with the message); when the send then fails, the edited text goes back to the composer above
/// `draft`, and the composer takes the keyboard. Send from the composer sends it without rolling back again.
fn recovers_the_edit_after_rollback_if_sending_fails(cx: &mut TestAppContext, draft: &str) {
    let home = start(cx);
    let (folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let task = start_with(&workbench, &store, "Original message", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    store.update(cx, |store, cx| {
        store.update_composer(&task, |composer| composer.text = draft.to_owned());
        cx.notify();
    });
    click_on(cx, "session-user-0-edit");
    cx.simulate_input(" edited");
    fail_sends(cx, Some("Send failed"));
    click_on(cx, "session-edit-send");
    start_fresh(&workbench, cx);
    let expected = if draft.is_empty() {
        "Original message edited".to_owned()
    } else {
        format!("Original message edited\n{draft}")
    };
    wait_long(cx, "the edit back in the composer", |cx| {
        store.read_with(cx, |store, _| store.composer(&task).text == expected)
    });
    wait_long(cx, "the conversation to empty", |cx| !shows(&workbench, "you: ", cx));
    assert!(cx.debug_bounds("session-edit-0").is_none(), "the editor left with the message");
    wait_for_composer_focus(&workbench, cx);

    fail_sends(cx, None);
    click_on(cx, "composer-send");
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    assert!(shows(&workbench, "you: Original message edited", cx), "{:?}", agent_conversation(&workbench, cx));
    let calls = agent_calls(&folder);
    assert_eq!(count(&calls, "session/new"), 2, "rolled back once, by a fresh session: {calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 2, "{calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

#[gpui_kit::test]
async fn recovers_the_edit_after_rollback_if_sending_fails_without_a_draft(cx: &mut TestAppContext) {
    recovers_the_edit_after_rollback_if_sending_fails(cx, "");
}

#[gpui_kit::test]
async fn recovers_the_edit_after_rollback_if_sending_fails_with_another_draft(cx: &mut TestAppContext) {
    recovers_the_edit_after_rollback_if_sending_fails(cx, "Another draft");
}
