//! What must be settled before a window closes, its project is left or the app quits: the exit
//! confirmation (the last window, `general.confirmExit`), the processes the close would end
//! (`run.confirmTerminate`), and the documents with unsaved changes.
//!
//! [`CloseDecision`] asks in that order and hands each question and each piece of work to the
//! caller as a [`CloseStep`]. The processes are only stopped after the unsaved-changes
//! question, so cancelling that keeps them running. An editor with documents of its own
//! (notebooks, custom editors) is prepared before their unsaved state is read: it stops
//! editing and executing until the close is decided, then commits or resumes.

use crate::platform::i18n::I18n;
use ide_core::settings::Settings;
use std::collections::HashSet;

/// What the close is about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseInput {
    /// Quit is in progress: the exit was confirmed already.
    pub quitting: bool,
    /// Project windows open, this one included.
    pub windows: u32,
    pub confirm_exit: bool,
    pub confirm_terminate: bool,
    /// Names of the Run/Debug processes the close would end ([`process_names`]).
    pub processes: Vec<String>,
    /// Documents with unsaved changes ([`DirtyDocuments::count`]).
    pub dirty: usize,
    /// The project the unsaved-changes question names; the app's name without one.
    pub project: Option<String>,
    /// Whether the editor is asked to prepare its own documents ([`CloseStep::PrepareClose`]).
    pub prepare: bool,
}

/// A button of a [`Prompt`]. Dismissing a prompt answers `cancel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptButton {
    pub id: &'static str,
    pub label: String,
    pub primary: bool,
}

/// A question for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: String,
    pub body: String,
    pub buttons: Vec<PromptButton>,
    /// The "Don't ask again" checkbox.
    pub checkbox: Option<String>,
    pub warning: bool,
}

/// How a [`Prompt`] was answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub button: String,
    pub checked: bool,
}

impl Answer {
    pub fn new(button: &str, checked: bool) -> Self {
        Self { button: button.to_owned(), checked }
    }
}

/// What becomes of the editor's prepared documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedEnd {
    /// Nothing was prepared.
    None,
    /// The close goes ahead: the documents stay frozen and close.
    Commit,
    /// The close does not happen: the documents resume editing and executing.
    Resume,
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseStep {
    /// Show the prompt, then report [`CloseDecision::answered`].
    Ask(Prompt),
    /// Write `general.confirmExit = false` globally, then report [`CloseDecision::setting_written`]
    /// whether or not it worked.
    DisableConfirmExit,
    /// Write `run.confirmTerminate = false` globally, then report [`CloseDecision::setting_written`]
    /// whether or not it worked.
    DisableConfirmTerminate,
    /// Have the editor prepare its own documents, then report [`CloseDecision::prepared`] with
    /// how many of them are unsaved (`None`: it prepared nothing).
    PrepareClose,
    /// Save every document, then report [`CloseDecision::saved`].
    SaveAll,
    /// Discard the prepared documents' unsaved changes, then report [`CloseDecision::discarded`].
    DiscardPrepared,
    /// Show "Terminating …" for `names` with a Kill button (which stops the processes killed),
    /// stop them gracefully and wait until none is left, then report [`CloseDecision::terminated`].
    Terminate { names: Vec<String> },
    /// The decision: whether the window may close, what becomes of the prepared documents, and a
    /// failure to report (saving failed: "Could not save all files" with it as the body).
    Decided { close: bool, prepared: PreparedEnd, failure: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Exit,
    ExitSetting,
    Terminate,
    TerminateSetting,
    Preparing,
    Unsaved,
    Saving,
    Discarding,
    Terminating,
    Done,
}

/// One close decision, from the first question to [`CloseStep::Decided`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseDecision {
    input: CloseInput,
    phase: Phase,
    terminate: Vec<String>,
    prepared: bool,
}

impl CloseDecision {
    /// Starts deciding `input`; the first step is returned with it.
    pub fn start(input: CloseInput, i18n: &I18n) -> (Self, CloseStep) {
        let mut decision = Self { input, phase: Phase::Exit, terminate: Vec::new(), prepared: false };
        let step = if !decision.input.quitting && decision.input.windows <= 1 && decision.input.confirm_exit {
            CloseStep::Ask(exit_prompt(i18n))
        } else {
            decision.after_exit(i18n)
        };
        (decision, step)
    }

    pub fn is_decided(&self) -> bool {
        self.phase == Phase::Done
    }

    fn after_exit(&mut self, i18n: &I18n) -> CloseStep {
        if self.input.confirm_terminate && !self.input.processes.is_empty() {
            self.phase = Phase::Terminate;
            return CloseStep::Ask(terminate_prompt(&self.input.processes, i18n));
        }
        self.prepare(i18n)
    }

    fn prepare(&mut self, i18n: &I18n) -> CloseStep {
        if self.input.prepare {
            self.phase = Phase::Preparing;
            return CloseStep::PrepareClose;
        }
        self.prepared(None, i18n)
    }

    fn decided(&mut self, close: bool, failure: Option<String>) -> CloseStep {
        self.phase = Phase::Done;
        let prepared = match (self.prepared, close) {
            (false, _) => PreparedEnd::None,
            (true, true) => PreparedEnd::Commit,
            (true, false) => PreparedEnd::Resume,
        };
        CloseStep::Decided { close, prepared, failure }
    }

    fn after_unsaved(&mut self) -> CloseStep {
        if self.terminate.is_empty() {
            return self.decided(true, None);
        }
        self.phase = Phase::Terminating;
        CloseStep::Terminate { names: self.terminate.clone() }
    }

    /// The prompt of the last [`CloseStep::Ask`] was answered.
    pub fn answered(&mut self, answer: &Answer, i18n: &I18n) -> CloseStep {
        match self.phase {
            Phase::Exit if answer.button != "exit" => self.decided(false, None),
            Phase::Exit if answer.checked => {
                self.phase = Phase::ExitSetting;
                CloseStep::DisableConfirmExit
            }
            Phase::Exit => self.after_exit(i18n),
            Phase::Terminate if answer.button != "terminate" => self.decided(false, None),
            Phase::Terminate => {
                self.terminate = self.input.processes.clone();
                if answer.checked {
                    self.phase = Phase::TerminateSetting;
                    CloseStep::DisableConfirmTerminate
                } else {
                    self.prepare(i18n)
                }
            }
            Phase::Unsaved => match answer.button.as_str() {
                "save" => {
                    self.phase = Phase::Saving;
                    CloseStep::SaveAll
                }
                "discard" if self.prepared => {
                    self.phase = Phase::Discarding;
                    CloseStep::DiscardPrepared
                }
                "discard" => self.after_unsaved(),
                _ => self.decided(false, None),
            },
            _ => self.decided(false, None),
        }
    }

    /// A "Don't ask again" setting was written, or writing it failed: the close goes on either way.
    pub fn setting_written(&mut self, i18n: &I18n) -> CloseStep {
        match self.phase {
            Phase::ExitSetting => self.after_exit(i18n),
            Phase::TerminateSetting => self.prepare(i18n),
            _ => self.decided(false, None),
        }
    }

    /// The editor prepared its documents; `dirty` of them are unsaved (`None`: nothing prepared).
    pub fn prepared(&mut self, dirty: Option<usize>, i18n: &I18n) -> CloseStep {
        self.prepared = dirty.is_some();
        let dirty = self.input.dirty + dirty.unwrap_or(0);
        if dirty == 0 {
            return self.after_unsaved();
        }
        self.phase = Phase::Unsaved;
        CloseStep::Ask(unsaved_prompt(dirty, self.input.project.as_deref(), i18n))
    }

    /// Preparing the documents failed: the window stays and the failure is reported.
    pub fn prepare_failed(&mut self, error: String) -> CloseStep {
        self.decided(false, Some(error))
    }

    /// Save All finished.
    pub fn saved(&mut self, result: Result<(), String>) -> CloseStep {
        match result {
            Ok(()) => self.after_unsaved(),
            Err(error) => self.decided(false, Some(error)),
        }
    }

    /// The prepared documents' changes were discarded, or discarding them failed.
    pub fn discarded(&mut self, result: Result<(), String>) -> CloseStep {
        self.saved(result)
    }

    /// The processes are gone, or stopping them failed.
    pub fn terminated(&mut self, result: Result<(), String>) -> CloseStep {
        match result {
            Ok(()) => self.decided(true, None),
            Err(error) => self.decided(false, Some(error)),
        }
    }
}

fn exit_prompt(i18n: &I18n) -> Prompt {
    Prompt {
        title: i18n.t("app.exit.title", &[]),
        body: i18n.t("app.exit.body", &[]),
        buttons: vec![PromptButton { id: "exit", label: i18n.t("app.exit.confirm", &[]), primary: true }],
        checkbox: Some(i18n.t("app.exit.dontAsk", &[])),
        warning: false,
    }
}

fn terminate_prompt(names: &[String], i18n: &I18n) -> Prompt {
    let (title, body) = match names {
        [name] => (
            i18n.t("app.terminateProcesses.titleOne", &[("name", name.as_str().into())]),
            i18n.t("app.terminateProcesses.bodyOne", &[("name", name.as_str().into())]),
        ),
        _ => (
            i18n.t("app.terminateProcesses.titleMany", &[("count", names.len().into())]),
            i18n.t(
                "app.terminateProcesses.bodyMany",
                &[("count", names.len().into()), ("names", names.join(", ").into())],
            ),
        ),
    };
    Prompt {
        title,
        body,
        buttons: vec![PromptButton {
            id: "terminate",
            label: i18n.t("app.terminateProcesses.terminate", &[]),
            primary: true,
        }],
        checkbox: Some(i18n.t("app.terminateProcesses.dontAsk", &[])),
        warning: true,
    }
}

fn unsaved_prompt(dirty: usize, project: Option<&str>, i18n: &I18n) -> Prompt {
    let project = project.map_or_else(|| i18n.t("app.title.default", &[]), str::to_owned);
    Prompt {
        title: i18n.t("app.unsaved.title", &[]),
        body: i18n.t("app.unsaved.body", &[("count", dirty.into()), ("project", project.into())]),
        buttons: vec![
            PromptButton { id: "discard", label: i18n.t("app.unsaved.discard", &[]), primary: false },
            PromptButton { id: "save", label: i18n.t("app.unsaved.save", &[]), primary: true },
        ],
        checkbox: None,
        warning: true,
    }
}

/// `general.confirmExit` of the window's settings; no settings ask nothing.
pub fn confirm_exit_enabled(settings: Option<&Settings>) -> bool {
    settings.is_some_and(|settings| settings.general.confirm_exit)
}

/// The process names a close would end: one entry per name, nameless ones left out, in order.
/// A list that could not be read blocks nothing.
pub fn process_names<E>(running: Result<Vec<String>, E>) -> Vec<String> {
    let mut seen = HashSet::new();
    running.unwrap_or_default().into_iter().filter(|name| !name.is_empty() && seen.insert(name.clone())).collect()
}

/// The close that leaving the window's project needs (opening another project in place, Close
/// Project): no exit confirmation, the processes and unsaved documents as for any close.
pub fn leave_input(
    confirm_terminate: bool,
    processes: Vec<String>,
    dirty: usize,
    project: Option<String>,
) -> CloseInput {
    CloseInput {
        quitting: false,
        windows: 2,
        confirm_exit: false,
        confirm_terminate,
        processes: if confirm_terminate { processes } else { Vec::new() },
        dirty,
        project,
        prepare: true,
    }
}

/// Quit: the exit is confirmed once here (when `general.confirmExit` is on); every window then
/// asks about its own processes and documents as the backend closes it. `None`: quit at once.
pub fn quit_input(confirm_exit: bool) -> Option<CloseInput> {
    confirm_exit.then(|| CloseInput { windows: 1, confirm_exit: true, ..CloseInput::default() })
}

/// The backend's answer to "what is this close": quitting, and how many project windows there are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CloseContext {
    pub quitting: bool,
    pub windows: u32,
}

/// How a window's close request is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseRoute {
    /// A document window this window opened refused to close: the close is called off
    /// ([`CloseRoute::cancel`]).
    Refused,
    /// The last window with a project (or LightEdit session) does not close: leaving its project
    /// is confirmed ([`leave_input`]), then the project closes and the window shows the Welcome
    /// screen. The close request itself is called off.
    ToWelcome,
    /// Decide the close of this input; when it is not to close, it is called off.
    Decide(CloseInput),
}

/// What calls a close off: the quit as a whole, or this window's (group's) close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelClose {
    Quit,
    WindowGroup,
}

impl CancelClose {
    pub fn of(context: CloseContext) -> Self {
        if context.quitting { Self::Quit } else { Self::WindowGroup }
    }
}

/// What the window knows when a close is requested.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowCloseState {
    pub has_project: bool,
    pub light_edit: bool,
    pub settings_confirm_exit: bool,
    pub confirm_terminate: bool,
    pub processes: Vec<String>,
    pub dirty: usize,
    pub project: Option<String>,
}

/// A window's close requests: one at a time; a second while one is decided is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloseRequests {
    closing: bool,
}

impl CloseRequests {
    /// A close was requested. `None`: one is already being decided, this one is refused.
    /// `documents_closed`: the document windows this window opened agreed to close.
    pub fn request(
        &mut self,
        context: CloseContext,
        documents_closed: bool,
        window: WindowCloseState,
    ) -> Option<CloseRoute> {
        if self.closing {
            return None;
        }
        self.closing = true;
        if !documents_closed {
            return Some(CloseRoute::Refused);
        }
        if !context.quitting && context.windows <= 1 && (window.has_project || window.light_edit) {
            return Some(CloseRoute::ToWelcome);
        }
        let processes = if window.confirm_terminate { window.processes } else { Vec::new() };
        Some(CloseRoute::Decide(CloseInput {
            quitting: context.quitting,
            windows: context.windows,
            confirm_exit: window.settings_confirm_exit,
            confirm_terminate: window.confirm_terminate,
            processes,
            dirty: window.dirty,
            project: window.project,
            prepare: true,
        }))
    }

    /// The request was settled (or failed): the next one is handled.
    pub fn finished(&mut self) {
        self.closing = false;
    }
}

/// Documents the backend reports unsaved in this window, counted with those the editor knows
/// of itself (local edits not yet sent, restored buffers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyDocuments {
    reported: HashSet<u64>,
}

impl DirtyDocuments {
    /// The backend's `docDirty` event.
    pub fn doc_dirty(&mut self, doc: u64, dirty: bool) {
        if dirty {
            self.reported.insert(doc);
        } else {
            self.reported.remove(&doc);
        }
    }

    /// The window's project (or LightEdit session) changed: what was reported is gone with it.
    pub fn project_changed(&mut self) {
        self.reported.clear();
    }

    /// An auxiliary window of this one closed and released `documents`: they are no longer
    /// unsaved here unless this window's editor (`own`) edits them too.
    pub fn auxiliary_closed(&mut self, documents: &[u64], own: &[u64]) {
        for doc in documents.iter().filter(|doc| !own.contains(doc)) {
            self.reported.remove(doc);
        }
    }

    /// Unsaved documents: the reported ones and the editor's own (`editor`), each once.
    pub fn count(&self, editor: &[u64]) -> usize {
        self.reported.iter().chain(editor).collect::<HashSet<_>>().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_more::running_processes::{
        RunningProcess, RunningProcessesProvider, RunningProcessesProviders,
    };

    fn i18n() -> I18n {
        I18n::default()
    }

    fn base() -> CloseInput {
        CloseInput { windows: 1, project: Some("openidea".to_owned()), ..CloseInput::default() }
    }

    /// Runs a decision, answering prompts in turn (`cancel` once they run out) and every other
    /// step with success; the prompts asked and the decision.
    fn run(input: CloseInput, answers: &[Answer]) -> (Vec<Prompt>, Vec<CloseStep>, CloseStep) {
        run_with(input, answers, |_| Ok(()))
    }

    fn run_with(
        input: CloseInput,
        answers: &[Answer],
        mut work: impl FnMut(&CloseStep) -> Result<(), String>,
    ) -> (Vec<Prompt>, Vec<CloseStep>, CloseStep) {
        let i18n = i18n();
        let (mut decision, mut step) = CloseDecision::start(input, &i18n);
        let (mut asked, mut steps) = (Vec::new(), Vec::new());
        let mut answers = answers.iter();
        loop {
            steps.push(step.clone());
            step = match &step {
                CloseStep::Decided { .. } => return (asked, steps, step.clone()),
                CloseStep::Ask(prompt) => {
                    asked.push(prompt.clone());
                    let answer = answers.next().cloned().unwrap_or_else(|| Answer::new("cancel", false));
                    decision.answered(&answer, &i18n)
                }
                CloseStep::DisableConfirmExit | CloseStep::DisableConfirmTerminate => decision.setting_written(&i18n),
                CloseStep::PrepareClose => decision.prepared(None, &i18n),
                CloseStep::SaveAll => decision.saved(work(&step)),
                CloseStep::DiscardPrepared => decision.discarded(work(&step)),
                CloseStep::Terminate { .. } => decision.terminated(work(&step)),
            };
        }
    }

    fn closes(step: &CloseStep) -> bool {
        matches!(step, CloseStep::Decided { close: true, .. })
    }

    // closes silently when nothing needs asking
    #[test]
    fn closes_silently_when_nothing_needs_asking() {
        let (asked, _, decided) = run(base(), &[]);
        assert!(closes(&decided));
        assert!(asked.is_empty());
    }

    // confirms exit for the last window when enabled
    #[test]
    fn confirms_exit_for_the_last_window_when_enabled() {
        let (asked, steps, decided) = run(CloseInput { confirm_exit: true, ..base() }, &[Answer::new("exit", false)]);
        assert!(closes(&decided));
        assert_eq!(asked[0].title, "Confirm Exit");
        assert!(!steps.contains(&CloseStep::DisableConfirmExit));
    }

    // cancelling the exit prompt keeps the window
    #[test]
    fn cancelling_the_exit_prompt_keeps_the_window() {
        let (_, _, decided) = run(CloseInput { confirm_exit: true, ..base() }, &[Answer::new("cancel", false)]);
        assert!(!closes(&decided));
    }

    // 'don't ask again' turns the setting off
    #[test]
    fn dont_ask_again_turns_the_setting_off() {
        let (_, steps, decided) = run(CloseInput { confirm_exit: true, ..base() }, &[Answer::new("exit", true)]);
        assert!(steps.contains(&CloseStep::DisableConfirmExit));
        assert!(closes(&decided));
    }

    // does not confirm exit for one of several windows or during quit
    #[test]
    fn does_not_confirm_exit_for_one_of_several_windows_or_during_quit() {
        let (several, ..) = run(CloseInput { confirm_exit: true, windows: 2, ..base() }, &[]);
        let (quitting, ..) = run(CloseInput { confirm_exit: true, quitting: true, ..base() }, &[]);
        assert!(several.is_empty() && quitting.is_empty());
    }

    fn running(names: &[&str]) -> CloseInput {
        CloseInput { confirm_terminate: true, processes: names.iter().map(|&n| n.to_owned()).collect(), ..base() }
    }

    // stops the processes gracefully and waits for them before closing
    #[test]
    fn stops_the_processes_gracefully_and_waits_for_them_before_closing() {
        let (_, steps, decided) = run(running(&["app"]), &[Answer::new("terminate", false)]);
        assert!(steps.contains(&CloseStep::Terminate { names: vec!["app".to_owned()] }));
        assert!(closes(&decided));
        // The close is only decided once the processes are gone.
        let i18n = i18n();
        let (mut decision, _) = CloseDecision::start(running(&["app"]), &i18n);
        let step = decision.answered(&Answer::new("terminate", false), &i18n);
        assert_eq!(step, CloseStep::Terminate { names: vec!["app".to_owned()] });
        assert!(!decision.is_decided());
        assert!(closes(&decision.terminated(Ok(()))));
    }

    // does not stop anything when the terminate prompt is cancelled
    #[test]
    fn does_not_stop_anything_when_the_terminate_prompt_is_cancelled() {
        let (_, steps, decided) = run(running(&["app"]), &[Answer::new("cancel", false)]);
        assert!(!closes(&decided));
        assert!(!steps.iter().any(|step| matches!(step, CloseStep::Terminate { .. })));
    }

    // asks before terminating a running process
    #[test]
    fn asks_before_terminating_a_running_process() {
        let (asked, _, decided) = run(running(&["app"]), &[Answer::new("terminate", false)]);
        assert!(closes(&decided));
        assert_eq!(asked.len(), 1);
        assert!(asked[0].title.contains("app") && asked[0].body.contains("app"));
    }

    // names every running process in the prompt
    #[test]
    fn names_every_running_process_in_the_prompt() {
        let (asked, _, decided) = run(running(&["app", "worker"]), &[Answer::new("terminate", false)]);
        assert!(closes(&decided));
        assert!(asked[0].body.contains("app, worker"));
    }

    // cancelling the terminate prompt keeps the window
    #[test]
    fn cancelling_the_terminate_prompt_keeps_the_window() {
        let (_, _, decided) = run(running(&["app"]), &[Answer::new("cancel", false)]);
        assert!(!closes(&decided));
    }

    // asks about processes before discarding unsaved changes
    #[test]
    fn asks_about_processes_before_discarding_unsaved_changes() {
        let (asked, steps, decided) =
            run(CloseInput { dirty: 1, ..running(&["app"]) }, &[Answer::new("cancel", false)]);
        assert!(!closes(&decided));
        assert_eq!(asked.len(), 1);
        assert!(!steps.contains(&CloseStep::SaveAll));
    }

    // 'don't ask again' turns process confirmation off
    #[test]
    fn dont_ask_again_turns_process_confirmation_off() {
        let (_, steps, _) = run(running(&["app"]), &[Answer::new("terminate", true)]);
        assert!(steps.contains(&CloseStep::DisableConfirmTerminate));
    }

    // does not ask about processes when the setting is off
    #[test]
    fn does_not_ask_about_processes_when_the_setting_is_off() {
        let (asked, ..) = run(CloseInput { confirm_terminate: false, ..running(&["app"]) }, &[]);
        assert!(asked.is_empty());
    }

    // saves all on Save
    #[test]
    fn saves_all_on_save() {
        let (_, steps, decided) = run(CloseInput { dirty: 2, ..base() }, &[Answer::new("save", false)]);
        assert!(steps.contains(&CloseStep::SaveAll));
        assert!(closes(&decided));
    }

    // discards without saving
    #[test]
    fn discards_without_saving() {
        let input = CloseInput { dirty: 1, quitting: true, ..base() };
        let (_, steps, decided) = run(input, &[Answer::new("discard", false)]);
        assert!(closes(&decided));
        assert!(!steps.contains(&CloseStep::SaveAll));
    }

    // keeps the window when saving fails or the user cancels
    #[test]
    fn keeps_the_window_when_saving_fails_or_the_user_cancels() {
        let dirty = CloseInput { dirty: 1, ..base() };
        let (_, _, failed) = run_with(dirty.clone(), &[Answer::new("save", false)], |_| Err("disk full".to_owned()));
        assert_eq!(
            failed,
            CloseStep::Decided { close: false, prepared: PreparedEnd::None, failure: Some("disk full".to_owned()) }
        );
        let (_, _, cancelled) = run(dirty, &[Answer::new("cancel", false)]);
        assert!(!closes(&cancelled));
    }

    // reads general.confirmExit
    #[test]
    fn reads_general_confirm_exit() {
        let mut settings = Settings::default();
        settings.general.confirm_exit = true;
        assert!(confirm_exit_enabled(Some(&settings)));
        settings.general.confirm_exit = false;
        assert!(!confirm_exit_enabled(Some(&settings)));
        assert!(!confirm_exit_enabled(None));
    }

    // includes dirty changes produced while preparing a custom document
    #[test]
    fn includes_dirty_changes_produced_while_preparing_a_custom_document() {
        let i18n = i18n();
        let (mut decision, step) = CloseDecision::start(CloseInput { prepare: true, ..base() }, &i18n);
        assert_eq!(step, CloseStep::PrepareClose);
        // Executing the document produced its final output while it was prepared.
        let CloseStep::Ask(prompt) = decision.prepared(Some(1), &i18n) else { panic!("asks about unsaved changes") };
        assert!(prompt.body.contains('1'));
        assert_eq!(decision.answered(&Answer::new("save", false), &i18n), CloseStep::SaveAll);
        assert_eq!(
            decision.saved(Ok(())),
            CloseStep::Decided { close: true, prepared: PreparedEnd::Commit, failure: None }
        );
    }

    // resumes a prepared document when %s cannot authorize closing
    #[test]
    fn resumes_a_prepared_document_when_an_answer_cannot_authorize_closing() {
        let i18n = i18n();
        for button in ["cancel", "save", "discard"] {
            let (mut decision, _) = CloseDecision::start(CloseInput { prepare: true, ..base() }, &i18n);
            decision.prepared(Some(1), &i18n);
            let step = match decision.answered(&Answer::new(button, false), &i18n) {
                CloseStep::SaveAll => decision.saved(Err("disk full".to_owned())),
                CloseStep::DiscardPrepared => decision.discarded(Err("reload failed".to_owned())),
                step => step,
            };
            assert!(
                matches!(step, CloseStep::Decided { close: false, prepared: PreparedEnd::Resume, .. }),
                "{button}: {step:?}"
            );
        }
    }

    // Don't Save discards prepared custom edits without saving them
    #[test]
    fn dont_save_discards_prepared_custom_edits_without_saving_them() {
        let i18n = i18n();
        let (mut decision, _) = CloseDecision::start(CloseInput { prepare: true, ..base() }, &i18n);
        decision.prepared(Some(1), &i18n);
        assert_eq!(decision.answered(&Answer::new("discard", false), &i18n), CloseStep::DiscardPrepared);
        assert_eq!(
            decision.discarded(Ok(())),
            CloseStep::Decided { close: true, prepared: PreparedEnd::Commit, failure: None }
        );
    }

    // includes pending local edits and restored dirty buffers before backend dirty events
    #[test]
    fn includes_pending_local_edits_and_restored_dirty_buffers_before_backend_dirty_events() {
        let mut dirty = DirtyDocuments::default();
        dirty.doc_dirty(11, true);
        dirty.doc_dirty(12, true);
        let editor = [10, 11];
        assert_eq!(dirty.count(&editor), 3);
        let input = leave_input(true, Vec::new(), dirty.count(&editor), Some("openidea".to_owned()));
        let (asked, _, decided) = run(input, &[]);
        assert!(!closes(&decided));
        assert!(asked[0].body.contains('3'));
        // What an auxiliary window released is no longer unsaved here, unless the editor has it.
        dirty.auxiliary_closed(&[11, 12], &editor);
        assert_eq!(dirty.count(&editor), 2);
        dirty.project_changed();
        assert_eq!(dirty.count(&[]), 0);
    }

    struct Fixed(Vec<&'static str>);

    impl RunningProcessesProvider for Fixed {
        fn running(&self) -> Vec<RunningProcess> {
            self.0.iter().map(|&name| RunningProcess { name: name.to_owned() }).collect()
        }
    }

    // confirmLeave asks about running processes before closing
    #[test]
    fn confirm_leave_asks_about_running_processes_before_closing() {
        let mut providers = RunningProcessesProviders::new();
        providers.register("test.run", Fixed(vec!["app"]));
        let names = process_names::<()>(Ok(providers.running().into_iter().map(|p| p.name).collect()));
        let (asked, _, decided) = run(leave_input(true, names, 0, Some("openidea".to_owned())), &[]);
        assert!(!closes(&decided));
        assert!(asked[0].title.contains("app") && asked[0].body.contains("app"));
    }

    // Not from a spec: a close request on the last project window returns it to the Welcome
    // screen, one request is handled at a time, and quitting confirms once.
    #[test]
    fn routes_close_requests_and_quits() {
        let mut requests = CloseRequests::default();
        let last = CloseContext { quitting: false, windows: 1 };
        let project = WindowCloseState { has_project: true, ..WindowCloseState::default() };
        assert_eq!(requests.request(last, true, project.clone()), Some(CloseRoute::ToWelcome));
        assert_eq!(requests.request(last, true, project.clone()), None);
        requests.finished();
        assert_eq!(requests.request(last, false, project.clone()), Some(CloseRoute::Refused));
        requests.finished();
        let quitting = CloseContext { quitting: true, windows: 1 };
        assert!(matches!(requests.request(quitting, true, project), Some(CloseRoute::Decide(input)) if input.quitting));
        assert_eq!(CancelClose::of(quitting), CancelClose::Quit);
        assert_eq!(CancelClose::of(last), CancelClose::WindowGroup);
        assert_eq!(quit_input(false), None);
        let (asked, steps, _) = run(quit_input(true).expect("asks"), &[Answer::new("exit", false)]);
        assert_eq!(asked.len(), 1);
        assert!(!steps.contains(&CloseStep::PrepareClose));
        assert_eq!(process_names::<()>(Ok(vec!["a".into(), String::new(), "a".into()])), ["a"]);
        assert!(process_names(Err("unavailable")).is_empty());
    }
}
