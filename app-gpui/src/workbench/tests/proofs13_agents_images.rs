use super::proofs13_agents_fixtures::*;
use super::*;

/// A message sent with a file and an image shows the image under its text and lists only the file among its file
/// chips; the agent was sent the file and the image's data.
#[gpui_kit::test]
async fn shows_the_images_and_lists_only_the_files_as_chips(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let images = vec![png("shot.png", "AAA")];
    let task = start_with(&workbench, &store, "Look @README.md", vec![readme(&folder)], images, cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let prompts = agent_prompts(&folder);
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert_eq!(prompts[0]["images"], serde_json::json!(["AAA"]), "{prompts:?}");
    assert_eq!(prompts[0]["files"], serde_json::json!(["README.md"]), "{prompts:?}");
    let lines = agent_conversation(&workbench, cx);
    let message = lines.iter().find(|line| line.starts_with("you: ")).expect("the message shows");
    assert!(message.starts_with("you: Look @README.md [files: README.md] [images: 1]"), "{message}");
    assert!(cx.debug_bounds("session-user-0-image-0").is_some(), "the image is drawn");
    assert!(cx.debug_bounds("session-user-0-image-1").is_none(), "the file is not drawn as an image");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Edit on a message sent with two images keeps both images, each with its Remove Image button; removing the first
/// and sending runs the conversation again with the text and the second image only.
#[gpui_kit::test]
async fn keeps_the_images_when_editing_minus_the_removed_ones(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = image_project(cx, home.path(), None);
    let images = vec![png("a.png", "AAA"), png("b.png", "BBB")];
    let task = start_with(&workbench, &store, "Look", Vec::new(), images, cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    assert!(cx.debug_bounds("session-user-0-image-1").is_some(), "both images are drawn");

    click_on(cx, "session-user-0-edit");
    assert!(cx.debug_bounds("session-edit-image-0-remove").is_some());
    assert!(cx.debug_bounds("session-edit-image-1-remove").is_some());
    assert!(cx.debug_bounds("session-edit-image-2-remove").is_none());
    click_on(cx, "session-edit-image-0-remove");
    assert!(cx.debug_bounds("session-edit-image-0-remove").is_some(), "one image is left");
    assert!(cx.debug_bounds("session-edit-image-1-remove").is_none(), "the first image is gone");

    click_on(cx, "session-edit-send");
    start_fresh(&workbench, cx);
    wait_long(cx, "the edited message to reach the agent", |_| agent_prompts(&folder).len() == 2);
    let prompts = agent_prompts(&folder);
    assert_eq!(prompts[1]["text"], serde_json::json!("Look"), "{prompts:?}");
    assert_eq!(prompts[1]["images"], serde_json::json!(["BBB"]), "{prompts:?}");
    wait_long(cx, "the new turn", |cx| {
        !agent_working(&store, &task, cx) && shows(&workbench, "you: Look [images: 1]", cx)
    });
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
