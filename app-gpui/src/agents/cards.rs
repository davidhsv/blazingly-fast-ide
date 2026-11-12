//! The requests a conversation answers: a permission request as a line of the conversation (its heading,
//! the command, a button per option, and a compact record once answered), and an `AskUserQuestion`
//! request as a card above the composer that answers all its questions at once.
//!
//! An answer is an [`AgentCall::Respond`] with a ticket from the store; its result comes back to
//! [`Responding::answered`] (or the form's `answered`). One answer is sent at a time.

use crate::agents::permission::{
    ButtonVariant, command_prefix, permission_details, permission_option, permission_result,
};
use crate::agents::store::{AgentCall, AgentEffect, AgentsStore, Ticket};
use crate::platform::i18n::I18n;
use ide_agent::{AgentEntry, AgentQuestion, AgentTask};

/// Agent-written titles with the task's own folder dropped from paths.
pub fn short_title(task: &AgentTask, title: &str) -> String {
    title.replace(&format!("{}/", task.cwd.0.to_string_lossy()), "")
}

/// A button answering a permission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionAnswer {
    /// The short label, also the button's accessible name.
    pub label: String,
    /// The option's own name, as its tooltip.
    pub title: String,
    pub variant: ButtonVariant,
    /// The option id sent back unchanged; `None` cancels a request that offers no options.
    pub option: Option<String>,
}

/// An answered request, reduced to its record: "Allowed once: python3 …".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRecord {
    pub label: String,
    pub allowed: bool,
    /// The command's program, or the request's title.
    pub subject: String,
    /// The whole command or title, as the tooltip.
    pub detail: String,
}

/// A permission request in the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionLine {
    /// The heading and the line's accessible name: "Codex wants to run a command", "Allow tool call?".
    pub title: String,
    /// The command shown in a code block, while open.
    pub command: Option<String>,
    /// What an open request without a command shows.
    pub detail: String,
    /// The buttons of an open request; none for a question, which its card answers.
    pub answers: Vec<PermissionAnswer>,
    /// Set once answered: the line shows only this.
    pub record: Option<PermissionRecord>,
}

/// The line of a permission entry; `provider` is the agent's name.
pub fn permission_line(
    entry: &AgentEntry,
    task: &AgentTask,
    provider: &str,
    i18n: &mut I18n,
) -> Option<PermissionLine> {
    let AgentEntry::Permission { title, options, questions, chosen, .. } = entry else { return None };
    let details = permission_details(title, provider, i18n);
    let i18n = &*i18n;
    let record = chosen.as_deref().map(|chosen| {
        let result = permission_result(options, chosen, i18n);
        PermissionRecord {
            label: result.label,
            allowed: result.allowed,
            subject: details.command.as_deref().map_or_else(|| short_title(task, &details.detail), command_prefix),
            detail: details.detail.clone(),
        }
    });
    let answers = if record.is_some() || !questions.is_empty() {
        Vec::new()
    } else if options.is_empty() {
        // Some agents send none: the request can still be cancelled, so the agent doesn't wait for an
        // answer it can't get.
        vec![PermissionAnswer {
            label: i18n.tx("Cancel"),
            title: i18n.tx("Cancel"),
            variant: ButtonVariant::Ghost,
            option: None,
        }]
    } else {
        options
            .iter()
            .map(|o| {
                let choice = permission_option(o, details.command.is_some(), i18n);
                PermissionAnswer {
                    label: choice.label,
                    title: o.name.clone(),
                    variant: choice.variant,
                    option: Some(o.id.clone()),
                }
            })
            .collect()
    };
    Some(PermissionLine {
        title: details.title,
        command: if record.is_some() { None } else { details.command },
        detail: short_title(task, &details.detail),
        answers,
        record,
    })
}

/// Answering a request: one answer at a time, its failure shown beside the buttons.
#[derive(Clone, Debug, Default)]
pub struct Responding {
    pending: Option<Ticket>,
    error: Option<String>,
}

impl Responding {
    /// An answer is on its way: the buttons are disabled.
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Sends `option` for request `permission` of `task`; `None` while an answer is on its way.
    pub fn answer(
        &mut self,
        store: &mut AgentsStore,
        task: &str,
        permission: u32,
        option: Option<&str>,
    ) -> Option<AgentEffect> {
        if self.pending.is_some() {
            return None;
        }
        let ticket = store.new_ticket();
        self.pending = Some(ticket);
        self.error = None;
        let call = AgentCall::Respond { task: task.to_owned(), permission, option: option.map(str::to_owned) };
        Some(AgentEffect::Call { ticket: Some(ticket), call })
    }

    /// The answer's result; one for another request is dropped.
    pub fn answered(&mut self, ticket: Ticket, result: Result<(), String>) {
        if self.pending != Some(ticket) {
            return;
        }
        self.pending = None;
        self.error = result.err();
    }
}

/// The open `AskUserQuestion` request of the task's own conversation, if any: its id, title and questions.
pub fn question_card(entries: &[AgentEntry]) -> Option<(u32, &str, &[AgentQuestion])> {
    entries.iter().find_map(|e| match e {
        AgentEntry::Permission { id, title, questions, chosen: None, .. } if !questions.is_empty() => {
            Some((*id, title.as_str(), questions.as_slice()))
        }
        _ => None,
    })
}

const RECOMMENDED: &str = " (Recommended)";

/// What a question is answered with: picked options and the typed answer (`None` while "Other" isn't
/// chosen).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Answer {
    picked: Vec<String>,
    other: Option<String>,
}

impl Answer {
    fn items(&self) -> Vec<String> {
        let other = self.other.as_deref().map(str::trim).filter(|o| !o.is_empty());
        self.picked.iter().cloned().chain(other.map(str::to_owned)).collect()
    }

    /// Something is picked, and "Other", when chosen, is typed in.
    fn answered(&self) -> bool {
        !self.items().is_empty() && self.other.as_deref().is_none_or(|o| !o.trim().is_empty())
    }
}

/// A choice of a question: one of its options (by label) or "Other".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Option(String),
    Other,
}

/// A radio button (a checkbox for a multi-select question) of the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceView {
    pub choice: Choice,
    /// The label without its " (Recommended)" suffix, which shows as a tag instead.
    pub label: String,
    pub recommended: bool,
    pub description: String,
    pub checked: bool,
    /// The number key that picks it.
    pub key: usize,
}

/// The preview beside a question's options: the hovered choice, else the picked one, else the first
/// option with a preview. `body` is `None` for a choice without one ("No preview").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub caption: String,
    pub body: Option<String>,
}

/// The form of an open `AskUserQuestion` request ([`question_card`]); a new request gets a new form.
#[derive(Clone, Debug)]
pub struct QuestionForm {
    task: String,
    permission: u32,
    questions: Vec<AgentQuestion>,
    answers: Vec<Answer>,
    focused: usize,
    hovered: Option<(usize, Choice)>,
    responding: Responding,
}

fn plain_label(label: &str) -> &str {
    label.strip_suffix(RECOMMENDED).unwrap_or(label)
}

impl QuestionForm {
    pub fn new(task: &str, permission: u32, questions: &[AgentQuestion]) -> Self {
        Self {
            task: task.to_owned(),
            permission,
            questions: questions.to_vec(),
            answers: vec![Answer::default(); questions.len()],
            focused: 0,
            hovered: None,
            responding: Responding::default(),
        }
    }

    /// The request this form answers.
    pub fn permission(&self) -> u32 {
        self.permission
    }

    pub fn questions(&self) -> &[AgentQuestion] {
        &self.questions
    }

    /// The choices of question `position`, "Other" last.
    pub fn choices(&self, position: usize, i18n: &I18n) -> Vec<ChoiceView> {
        let (Some(question), Some(answer)) = (self.questions.get(position), self.answers.get(position)) else {
            return Vec::new();
        };
        let mut out: Vec<ChoiceView> = question
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| ChoiceView {
                choice: Choice::Option(o.label.clone()),
                label: plain_label(&o.label).to_owned(),
                recommended: o.label.ends_with(RECOMMENDED),
                description: o.description.clone(),
                checked: answer.picked.contains(&o.label),
                key: i + 1,
            })
            .collect();
        out.push(ChoiceView {
            choice: Choice::Other,
            label: i18n.tx("Other"),
            recommended: false,
            description: String::new(),
            checked: answer.other.is_some(),
            key: question.options.len() + 1,
        });
        out
    }

    /// Picks (or, in a multi-select question, unpicks) an option; picking one of a single-select
    /// question drops a typed answer.
    pub fn choose(&mut self, position: usize, label: &str, checked: bool) {
        let multi = self.questions.get(position).is_some_and(|q| q.multi_select);
        let Some(answer) = self.answers.get_mut(position) else { return };
        if !multi {
            *answer = Answer { picked: vec![label.to_owned()], other: None };
        } else if checked {
            let label = label.to_owned();
            if !answer.picked.contains(&label) {
                answer.picked.push(label);
            }
        } else {
            answer.picked.retain(|p| p != label);
        }
    }

    /// Chooses (or unchooses) "Other"; in a single-select question it replaces the picked option.
    pub fn choose_other(&mut self, position: usize, checked: bool) {
        let multi = self.questions.get(position).is_some_and(|q| q.multi_select);
        let Some(answer) = self.answers.get_mut(position) else { return };
        if multi {
            answer.other = checked.then(String::new);
        } else {
            *answer = Answer { picked: Vec::new(), other: Some(String::new()) };
        }
    }

    /// The typed answer of question `position`, while "Other" is chosen.
    pub fn other(&self, position: usize) -> Option<&str> {
        self.answers.get(position)?.other.as_deref()
    }

    pub fn type_other(&mut self, position: usize, text: &str) {
        if let Some(answer) = self.answers.get_mut(position) {
            answer.other = Some(text.to_owned());
        }
    }

    /// Question `position` took focus: number keys answer it.
    pub fn focus(&mut self, position: usize) {
        self.focused = position;
    }

    /// The pointer is over a choice of question `position` (`None`: it left).
    pub fn hover(&mut self, position: usize, choice: Option<Choice>) {
        self.hovered = choice.map(|c| (position, c));
    }

    /// A number key outside the typed answer: toggles that option of the focused question; the one after
    /// the options toggles "Other".
    pub fn press_number(&mut self, number: usize) {
        let position = self.focused;
        let Some(question) = self.questions.get(position) else { return };
        let count = question.options.len();
        if let Some(option) = number.checked_sub(1).and_then(|i| question.options.get(i)) {
            let label = option.label.clone();
            let checked = !self.answers[position].picked.contains(&label);
            self.choose(position, &label, checked);
        } else if number == count + 1 {
            let checked = self.answers[position].other.is_none();
            self.choose_other(position, checked);
        }
    }

    /// Every question is answered.
    pub fn complete(&self) -> bool {
        self.answers.iter().all(Answer::answered)
    }

    /// Submit is enabled.
    pub fn can_submit(&self) -> bool {
        self.complete() && !self.responding.busy()
    }

    /// The fields are disabled while the answer is on its way.
    pub fn busy(&self) -> bool {
        self.responding.busy()
    }

    /// The answer: a JSON object of each question (by its answer key, else its text) to its answers, in
    /// the order the questions were asked.
    pub fn answer_json(&self) -> String {
        let mut keys: Vec<&str> = Vec::new();
        let mut values: Vec<Vec<String>> = Vec::new();
        for (question, answer) in self.questions.iter().zip(&self.answers) {
            let key = question.answer_key.as_deref().unwrap_or(&question.question);
            match keys.iter().position(|k| *k == key) {
                // A repeated key keeps its place and takes the later answer, as an object would.
                Some(i) => values[i] = answer.items(),
                None => {
                    keys.push(key);
                    values.push(answer.items());
                }
            }
        }
        let fields: Vec<String> = keys
            .iter()
            .zip(&values)
            .map(|(key, items)| {
                let key = serde_json::to_string(key).unwrap_or_default();
                let items = serde_json::to_string(items).unwrap_or_default();
                format!("{key}:{items}")
            })
            .collect();
        format!("{{{}}}", fields.join(","))
    }

    /// Sends the answers; `None` until every question is answered.
    pub fn submit(&mut self, store: &mut AgentsStore) -> Option<AgentEffect> {
        if !self.complete() {
            return None;
        }
        let json = self.answer_json();
        self.responding.answer(store, &self.task, self.permission, Some(&json))
    }

    /// Skip: declines the request.
    pub fn skip(&mut self, store: &mut AgentsStore) -> Option<AgentEffect> {
        self.responding.answer(store, &self.task, self.permission, Some("reject"))
    }

    /// Escape in the card. In a typed answer with text (`in_other`, the question's position) it clears
    /// the text; anywhere else it skips.
    pub fn escape(&mut self, in_other: Option<usize>, store: &mut AgentsStore) -> Option<AgentEffect> {
        if let Some(position) = in_other
            && self.other(position).is_some_and(|o| !o.is_empty())
        {
            self.type_other(position, "");
            return None;
        }
        self.skip(store)
    }

    pub fn answered(&mut self, ticket: Ticket, result: Result<(), String>) {
        self.responding.answered(ticket, result);
    }

    /// The preview of question `position`; `None` when none of its options has one.
    pub fn preview(&self, position: usize, i18n: &I18n) -> Option<Preview> {
        let question = self.questions.get(position)?;
        let answer = self.answers.get(position)?;
        if !question.options.iter().any(|o| o.preview.is_some()) {
            return None;
        }
        let hovered = self.hovered.as_ref().filter(|(p, _)| *p == position).map(|(_, c)| c.clone());
        let target = hovered
            .or_else(|| answer.picked.first().map(|l| Choice::Option(l.clone())))
            .or_else(|| answer.other.is_some().then_some(Choice::Other));
        let option = match &target {
            None => question.options.iter().find(|o| o.preview.is_some()),
            Some(Choice::Option(label)) => question.options.iter().find(|o| o.label == *label),
            Some(Choice::Other) => None,
        };
        Some(match option {
            Some(o) => Preview { caption: plain_label(&o.label).to_owned(), body: o.preview.clone() },
            None => Preview { caption: i18n.tx("Other"), body: None },
        })
    }
}

#[cfg(test)]
mod tests;
