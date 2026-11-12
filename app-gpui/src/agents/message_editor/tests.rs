use super::*;
use crate::agents::fixtures::{agent, connected_store, local_task};
use crate::agents::session::{UserButton, user_buttons, user_files};
use crate::agents::store::{AgentCall, AgentEffect, CallAnswer};
use ide_agent::{AgentEntry, AgentPrompt};

fn image(data: &str, name: &str) -> AgentImage {
    AgentImage { mime_type: "image/png".to_owned(), data: data.to_owned(), name: Some(name.to_owned()) }
}

const COMMAND: KeyState = KeyState { shift: false, command: true, composing: false };

fn editable(status: AgentStatus) -> AgentTask {
    let mut task = local_task("A");
    task.status = status;
    task.capabilities.rollback = true;
    task
}

/// The editor of "Original message", edited to "Edited message".
fn edited() -> MessageEditor {
    let mut editor = MessageEditor::new("Original message", &[]);
    editor.set_text("Edited message");
    editor
}

#[test]
fn shows_the_images_and_lists_only_the_files_as_chips() {
    let attachments = ["a.ts".to_owned(), "shot.png".to_owned()];
    let images = [image("AAA", "shot.png")];
    assert_eq!(image_src(&images[0]), "data:image/png;base64,AAA");
    assert_eq!(user_files(&attachments, images.len()), ["a.ts".to_owned()]);
}

#[test]
fn opens_an_image_in_a_dialog_and_keeps_the_images_when_editing_minus_the_removed_ones() {
    let i18n = I18n::default();
    let (a, b) = (image("AAA", "a.png"), image("BBB", "b.png"));
    let (mut store, _) = connected_store(vec![editable(AgentStatus::Ready)]);
    let message = AgentEntry::User {
        text: "Look".to_owned(),
        attachments: vec!["a.png".to_owned(), "b.png".to_owned()],
        images: vec![a.clone(), b.clone()],
        sent_ms: Some(1.0),
        done_ms: Some(2.0),
        steered: false,
    };
    store.entries_loaded("A", None, vec![message, agent("Original answer")]);
    let mut editor = MessageEditor::new("Look", &[a, b.clone()]);
    assert_eq!(editor.images().len(), 2);
    editor.remove_image(0);
    assert_eq!(editor.images(), std::slice::from_ref(&b));
    let Some(EditAction::Send(text, images)) = editor.send() else { panic!("a send") };
    let (_, effects) = store.rerun_from("A", 0, &text, &images, 0.0, &i18n);
    let Some(AgentEffect::Confirm { ticket, .. }) = effects.first() else { panic!("a confirmation") };
    let effects = store.confirmed(*ticket, true, 0.0);
    let rollback = effects
        .iter()
        .find_map(|e| match e {
            AgentEffect::Call { ticket: Some(t), call: AgentCall::Rollback { .. } } => Some(*t),
            _ => None,
        })
        .expect("a rollback");
    let effects = store.call_answered(rollback, Ok(CallAnswer::RolledBack(None)));
    let prompt = effects.iter().find_map(|e| match e {
        AgentEffect::Call { call: AgentCall::Prompt { prompt, .. }, .. } => Some(prompt.clone()),
        _ => None,
    });
    assert_eq!(prompt, Some(AgentPrompt { text: "Look".to_owned(), files: Vec::new(), images: vec![b] }));
}

#[test]
fn names_the_icon_only_edit_and_resend_action_and_disables_empty_edits() {
    let task = editable(AgentStatus::Ready);
    assert!(user_buttons(&task, "Original message", 0, Some(0), 1).contains(&UserButton::Edit));
    let mut editor = edited();
    assert!(editor.can_send());
    editor.set_text("");
    assert!(!editor.can_send());
    assert_eq!(editor.send(), None);
}

#[test]
fn keeps_enter_shift_enter_and_ime_confirmation_for_editing() {
    let mut editor = edited();
    let presses =
        [KeyState::default(), KeyState { shift: true, ..KeyState::default() }, KeyState { composing: true, ..COMMAND }];
    for state in presses {
        assert_eq!(editor.key(EditKey::Enter, state), None);
    }
    assert_eq!(editor.text(), "Edited message");
    assert!(!editor.sending());
}

#[test]
fn still_sends_with_modified_enter() {
    // ⌘ and Ctrl both arrive as the command modifier.
    let mut editor = edited();
    assert_eq!(editor.key(EditKey::Enter, COMMAND), Some(EditAction::Send("Edited message".to_owned(), Vec::new())));
    assert!(editor.sending());
}

#[test]
fn method_restores_the_message_and_discards_edits_without_touching_the_conversation() {
    // The Cancel button and Escape both cancel.
    let mut editor = edited();
    assert_eq!(editor.key(EditKey::Escape, KeyState::default()), Some(EditAction::Cancel));
    // Editing again starts from the message as sent.
    assert_eq!(MessageEditor::new("Original message", &[]).text(), "Original message");
}

#[test]
fn keeps_queue_named_and_shows_icon_only_stop_actions_while_the_agent_works() {
    let i18n = I18n::default();
    let task = editable(AgentStatus::Running);
    let buttons = submit_buttons(&task, &ComposerDraft::default(), false, &i18n);
    assert_eq!(buttons.label, "Queue");
    assert!(buttons.stop && !buttons.enabled && !buttons.primary);
    let idle = submit_buttons(&editable(AgentStatus::Ready), &ComposerDraft::default(), false, &i18n);
    assert_eq!((idle.label.as_str(), idle.stop), ("Send", false));
}
