//! SSH: credential prompts of `ssh` / `git` child processes (askpass), shown one at a time with
//! prompts arriving meanwhile waiting in order, and Tools › Start SSH Session: pick an SSH
//! configuration and run `ssh` for it in a terminal tab. The configurations live in Settings ›
//! Tools › SSH Configurations.

use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::event::AskpassKind;
use ide_core::settings::SshConfiguration;
use serde::Deserialize;
use serde_json::{Value, json};

/// The settings page of the SSH configurations.
pub const SSH_SETTINGS_PAGE: &str = "tools.sshConfigurations";

/// A credential prompt (`IdeEvent::Askpass`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskpassPrompt {
    pub id: u64,
    pub prompt: String,
    pub kind: AskpassKind,
    /// The dialog offers "Remember" (git HTTPS passwords).
    pub remember: bool,
}

/// The reply to send: `ssh_askpass_answer(id, answer, remember)`. The backend ties `remember`
/// to this prompt; the git command saves it only on success and drops it again when
/// authentication fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskpassReply {
    pub id: u64,
    pub answer: Option<String>,
    pub remember: bool,
}

/// The pending prompts; the first one is the dialog shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskpassQueue {
    queue: Vec<AskpassPrompt>,
}

impl AskpassQueue {
    pub fn prompts(&self) -> &[AskpassPrompt] {
        &self.queue
    }

    /// The prompt the dialog shows.
    pub fn head(&self) -> Option<&AskpassPrompt> {
        self.queue.first()
    }

    /// Queues `prompt` unless it is queued already.
    pub fn enqueue(&mut self, prompt: AskpassPrompt) {
        if !self.queue.iter().any(|q| q.id == prompt.id) {
            self.queue.push(prompt);
        }
    }

    /// Drops prompt `id` (answered in another window, or the process gave up).
    pub fn drop_prompt(&mut self, id: u64) {
        self.queue.retain(|q| q.id != id);
    }

    /// Answers `prompt`: it leaves the queue (showing the next one) and the reply is to be sent.
    pub fn answer(&mut self, prompt: &AskpassPrompt, answer: Option<String>, remember: bool) -> AskpassReply {
        self.drop_prompt(prompt.id);
        AskpassReply { id: prompt.id, answer, remember }
    }

    /// Clears the queue (the feature is deactivated).
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

/// What the user did in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskpassChoice {
    /// OK with the typed text.
    Text(String),
    /// Yes or No of a confirmation.
    Confirm(bool),
}

/// The answer sent back: the typed text, `yes` / `no` for a confirmation, `None` when cancelled
/// (or when the choice does not fit the prompt).
pub fn askpass_answer(kind: AskpassKind, choice: Option<AskpassChoice>) -> Option<String> {
    match (kind, choice?) {
        (AskpassKind::Confirm, AskpassChoice::Confirm(yes)) => Some(if yes { "yes" } else { "no" }.to_owned()),
        (AskpassKind::Confirm, AskpassChoice::Text(_)) | (_, AskpassChoice::Confirm(_)) => None,
        (_, AskpassChoice::Text(text)) => Some(text),
    }
}

/// The askpass dialog of the head prompt: the typed text and the "Remember for this host" box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskpassDialog {
    pub text: String,
    pub remember: bool,
}

impl Default for AskpassDialog {
    fn default() -> Self {
        Self { text: String::new(), remember: true }
    }
}

impl AskpassDialog {
    /// The dialog's title: a password field for a secret, else "SSH".
    pub fn title(kind: AskpassKind, i18n: &I18n) -> String {
        i18n.tx(if kind == AskpassKind::Secret { "SSH Password" } else { "SSH" })
    }

    /// Closes the dialog with `choice` (`None`: cancelled) and answers the prompt.
    pub fn done(
        &self,
        queue: &mut AskpassQueue,
        prompt: &AskpassPrompt,
        choice: Option<AskpassChoice>,
    ) -> AskpassReply {
        queue.answer(prompt, askpass_answer(prompt.kind, choice), self.remember)
    }
}

/// What a row of the Start SSH Session chooser does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRun {
    /// Start a session of the configuration with this id.
    Start(String),
    /// Open Settings › Tools › SSH Configurations.
    Configure,
}

/// A row of the Start SSH Session chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionItem {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub icon: &'static str,
    /// Why the row cannot be chosen.
    pub disabled: Option<String>,
    pub run: SessionRun,
}

/// Rows of the chooser: one per configuration, then a way to edit them.
pub fn session_items(configs: &[SshConfiguration], i18n: &I18n) -> Vec<SessionItem> {
    let mut items: Vec<SessionItem> = configs
        .iter()
        .map(|c| {
            let label = c.label();
            let name = c.name.trim();
            let shown = [name, label.as_str(), c.id.as_str()].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
            SessionItem {
                id: c.id.clone(),
                label: shown.to_owned(),
                detail: (!name.is_empty() && label != c.name).then_some(label),
                icon: "terminal",
                disabled: c.host.trim().is_empty().then(|| i18n.tx("No host")),
                run: SessionRun::Start(c.id.clone()),
            }
        })
        .collect();
    let configure = if configs.is_empty() { "No SSH configurations. Add one…" } else { "Edit SSH Configurations…" };
    items.push(SessionItem {
        id: "<configure>".to_owned(),
        label: i18n.tx(configure),
        detail: None,
        icon: "settings",
        disabled: None,
        run: SessionRun::Configure,
    });
    items
}

/// The chooser popup's title.
pub fn chooser_title(i18n: &I18n) -> String {
    i18n.tx("Start SSH Session")
}

/// What `ssh.startSession` does with its argument: a configuration id starts it right away;
/// anything else opens the chooser (`None`).
pub fn session_to_start(arg: Option<&Value>) -> Option<&str> {
    arg.and_then(Value::as_str).filter(|id| !id.is_empty())
}

/// The backend's `ssh_terminal_command` answer (defined by the desktop shell's commands, not a
/// dependency of this crate, so mirrored here): the system `ssh` command line for a configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SshTerminalCommand {
    pub shell: String,
    pub args: Vec<String>,
    pub title: String,
}

/// What starting a session comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionStart {
    /// Run this action with this argument (`terminal.runCommand`).
    RunAction {
        id: &'static str,
        arg: Value,
    },
    Notify(NotificationRequest),
}

/// The answer of `ssh_terminal_command(id)`: a terminal tab running it, or the error shown.
pub fn session_started(result: Result<SshTerminalCommand, String>, i18n: &I18n) -> SessionStart {
    match result {
        Ok(command) => SessionStart::RunAction {
            id: "terminal.runCommand",
            arg: json!({ "program": command.shell, "args": command.args, "name": command.title }),
        },
        Err(message) => SessionStart::Notify(NotificationRequest {
            body: Some(message),
            group: Some("tools".to_owned()),
            ..NotificationRequest::new(NotificationLevel::Error, i18n.tx("Cannot start SSH session"))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(id: u64, text: &str, kind: AskpassKind, remember: bool) -> AskpassPrompt {
        AskpassPrompt { id, prompt: text.to_owned(), kind, remember }
    }

    fn ids(queue: &AskpassQueue) -> Vec<u64> {
        queue.prompts().iter().map(|p| p.id).collect()
    }

    fn config(id: &str, name: &str, host: &str, user: &str, port: Option<u16>) -> SshConfiguration {
        SshConfiguration {
            id: id.to_owned(),
            name: name.to_owned(),
            host: host.to_owned(),
            user: user.to_owned(),
            port,
            ..SshConfiguration::default()
        }
    }

    #[test]
    fn queues_prompts_in_arrival_order_once_each() {
        let mut queue = AskpassQueue::default();
        queue.enqueue(prompt(1, "Password:", AskpassKind::Secret, false));
        queue.enqueue(prompt(2, "Continue connecting (yes/no)?", AskpassKind::Confirm, false));
        queue.enqueue(prompt(1, "Password:", AskpassKind::Secret, false));
        assert_eq!(ids(&queue), [1, 2]);
    }

    #[test]
    fn drops_a_prompt_answered_elsewhere() {
        let mut queue = AskpassQueue::default();
        queue.enqueue(prompt(1, "a", AskpassKind::Text, false));
        queue.enqueue(prompt(2, "b", AskpassKind::Text, false));
        queue.drop_prompt(1);
        queue.drop_prompt(9);
        assert_eq!(ids(&queue), [2]);
    }

    #[test]
    fn answers_the_backend_and_shows_the_next_prompt() {
        let mut queue = AskpassQueue::default();
        let first = prompt(1, "a", AskpassKind::Secret, true);
        queue.enqueue(first.clone());
        queue.enqueue(prompt(2, "b", AskpassKind::Secret, false));
        let reply = queue.answer(&first, Some("hunter2".to_owned()), true);
        assert_eq!(reply, AskpassReply { id: 1, answer: Some("hunter2".to_owned()), remember: true });
        assert_eq!(ids(&queue), [2]);
        assert_eq!(queue.head().map(|p| p.id), Some(2));
    }

    #[test]
    fn maps_choices_to_answers() {
        let text = |t: &str| Some(AskpassChoice::Text(t.to_owned()));
        assert_eq!(askpass_answer(AskpassKind::Secret, text("pw")).as_deref(), Some("pw"));
        assert_eq!(askpass_answer(AskpassKind::Text, text("")).as_deref(), Some(""));
        assert_eq!(askpass_answer(AskpassKind::Confirm, Some(AskpassChoice::Confirm(true))).as_deref(), Some("yes"));
        assert_eq!(askpass_answer(AskpassKind::Confirm, Some(AskpassChoice::Confirm(false))).as_deref(), Some("no"));
        assert_eq!(askpass_answer(AskpassKind::Secret, None), None);
        assert_eq!(askpass_answer(AskpassKind::Confirm, None), None);
    }

    #[test]
    fn lists_configurations_then_a_way_to_edit_them() {
        let i18n = I18n::default();
        let items = session_items(
            &[
                config("a", "Dev box", "box", "dev", Some(2222)),
                config("b", "", "bastion", "", None),
                config("c", "Empty", "", "", None),
            ],
            &i18n,
        );
        let rows: Vec<(&str, Option<&str>)> = items.iter().map(|i| (i.label.as_str(), i.detail.as_deref())).collect();
        assert_eq!(
            rows,
            [("Dev box", Some("dev@box:2222")), ("bastion", None), ("Empty", None), ("Edit SSH Configurations…", None),]
        );
        assert!(items[2].disabled.is_some());
        assert_eq!(items[0].run, SessionRun::Start("a".to_owned()));
        assert_eq!(items[3].run, SessionRun::Configure);
    }

    #[test]
    fn offers_to_add_one_when_there_are_none() {
        let items = session_items(&[], &I18n::default());
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["No SSH configurations. Add one…"]);
    }
}
