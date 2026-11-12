//! Editing a message already sent, where it stands: its text and images (minus those removed), the keys
//! that send or cancel, and the session composer's submit button while the agent works. Sending runs the
//! conversation again from the message ([`crate::agents::store::AgentsStore::rerun_from`]).

use crate::agents::composer::{ComposerDraft, can_send};
use crate::agents::model::is_busy;
use crate::platform::i18n::I18n;
use ide_agent::{AgentImage, AgentStatus, AgentTask};

/// A key in the message being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    Enter,
    Escape,
}

/// The modifiers of a key, and whether an input method is composing (its keys are its own).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyState {
    pub shift: bool,
    /// ⌘ or Ctrl.
    pub command: bool,
    pub composing: bool,
}

/// What a key asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum EditAction {
    /// Send the edited text (trimmed) and images.
    Send(String, Vec<AgentImage>),
    /// Leave the editor; the message shows as it was sent.
    Cancel,
}

/// A sent message being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageEditor {
    text: String,
    images: Vec<AgentImage>,
    sending: bool,
    error: Option<String>,
}

impl MessageEditor {
    /// Editing starts from the message as sent, with its images.
    pub fn new(text: &str, images: &[AgentImage]) -> Self {
        Self { text: text.to_owned(), images: images.to_vec(), sending: false, error: None }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn images(&self) -> &[AgentImage] {
        &self.images
    }

    /// The text is read-only while it is being sent.
    pub fn set_text(&mut self, text: &str) {
        if !self.sending {
            self.text = text.to_owned();
        }
    }

    /// Removes an image (each has a Remove Image button, none while sending).
    pub fn remove_image(&mut self, index: usize) {
        if !self.sending && index < self.images.len() {
            self.images.remove(index);
        }
    }

    /// Send is enabled: there is text or an image, and nothing is being sent.
    pub fn can_send(&self) -> bool {
        (!self.text.trim().is_empty() || !self.images.is_empty()) && !self.sending
    }

    pub fn sending(&self) -> bool {
        self.sending
    }

    /// Why the last send failed, shown beside the editor.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Send: the text and images to run the conversation again with, unless there is nothing to send.
    pub fn send(&mut self) -> Option<EditAction> {
        if !self.can_send() {
            return None;
        }
        self.sending = true;
        self.error = None;
        Some(EditAction::Send(self.text.trim().to_owned(), self.images.clone()))
    }

    /// The send ended; a failure keeps the edit, with its message beside it.
    pub fn sent(&mut self, result: Result<(), String>) {
        self.sending = false;
        self.error = result.err();
    }

    /// A key: ⌘/Ctrl+Enter sends, Escape cancels (not while sending). Plain and ⇧Enter type a line
    /// break, and keys an input method is composing are left to it.
    pub fn key(&mut self, key: EditKey, state: KeyState) -> Option<EditAction> {
        if state.composing {
            return None;
        }
        match key {
            EditKey::Escape => (!self.sending).then_some(EditAction::Cancel),
            EditKey::Enter if state.command => self.send(),
            EditKey::Enter => None,
        }
    }
}

/// The `data:` URL an image thumbnail shows.
pub fn image_src(image: &AgentImage) -> String {
    format!("data:{};base64,{}", image.mime_type, image.data)
}

/// The note under an edited message that later turns follow: they are replaced.
pub fn replaces_later(turn: usize, turn_count: usize, i18n: &I18n) -> Option<String> {
    (turn + 1 < turn_count)
        .then(|| i18n.tx("The messages after this one are replaced. Files the agent changed stay as they are."))
}

/// The session composer's buttons: Stop while the agent works, and the submit button, which queues the
/// message for when the agent is done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitButtons {
    pub stop: bool,
    /// "Send", or "Queue" while the agent works; also the accessible name of the icon-only button.
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
    pub placeholder: String,
}

/// `sending`: a message is on its way.
pub fn submit_buttons(task: &AgentTask, composer: &ComposerDraft, sending: bool, i18n: &I18n) -> SubmitButtons {
    let busy = is_busy(task.status);
    let placeholder =
        if busy { "Queue a message for when the agent is done (⌘↵ sends it now)" } else { "Reply to the agent" };
    SubmitButtons {
        stop: busy,
        label: i18n.tx(if busy { "Queue" } else { "Send" }),
        primary: !busy,
        enabled: can_send(composer) && !sending && task.status != AgentStatus::Archived,
        placeholder: i18n.tx(placeholder),
    }
}

#[cfg(test)]
mod tests;
