//! Where projects and files open: one project per window, loose files in LightEdit (a window
//! with editor tabs only, one collecting them all), "Open project in" (this window, a new one,
//! attached to this window's workspace, or ask), and the launches the backend hands a window
//! (the project, files, caret positions and diff pair it was opened or told to show).
//!
//! The backend decides which window takes what ([`OpenStep::RequestWindow`],
//! [`LooseStep::Route`]); these models decide what this window does with the answer.

use super::close_guard::{Answer, Prompt, PromptButton};
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::{basename, dirname};
use crate::platform_more::file_drop::{DropAction, open_dropped};
use crate::platform_more::remote::RemoteTarget;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::IdeError;
use ide_core::error::ErrorKind;

/// The workspace a window's project belongs to: its name and member folders, the primary first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceMembers {
    pub name: String,
    pub projects: Vec<String>,
}

/// What the window shows when something is to be opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowState {
    /// The root of the project open in it.
    pub project: Option<String>,
    pub workspace: Option<WorkspaceMembers>,
    /// It edits loose files without a project.
    pub light_edit: bool,
    /// Its project lives on another machine.
    pub remote: bool,
    /// A project is being opened (session restore).
    pub opening: bool,
    /// System Settings › Project › "Open project in": `new`, `same`, anything else asks.
    pub open_project_in: String,
}

impl WindowState {
    fn has_content(&self) -> bool {
        self.project.is_some() || self.light_edit
    }

    /// The project at `path` is this window's own (a remote window's root is another machine's).
    fn has_local(&self, path: &str) -> bool {
        !self.remote && self.project.as_deref() == Some(path)
    }
}

/// The backend's answer to "open this project": where it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    /// Already open in `label`; that window was focused.
    Focused { label: String },
    /// A new window `label` opens it itself.
    Opened { label: String },
    /// This window opens it.
    Current,
}

/// A file to open as an editor tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOpen {
    pub path: String,
    /// The tab the editor shows in the end: the last one.
    pub focus: bool,
    /// Where the caret goes (0-based line and column), from the command line.
    pub position: Option<(u32, u32)>,
}

/// A caret position the command line asked for (`--line` / `--column`), 0-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePosition {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

/// `files` as editor tabs, the last focused, each at its position when one was asked for.
pub fn editor_opens(files: &[String], positions: &[FilePosition]) -> Vec<FileOpen> {
    files
        .iter()
        .enumerate()
        .map(|(index, path)| FileOpen {
            path: path.clone(),
            focus: index + 1 == files.len(),
            position: positions.iter().find(|p| p.path == *path).map(|p| (p.line, p.column)),
        })
        .collect()
}

fn notice(level: NotificationLevel, title: String, body: Option<String>, key: Option<String>) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.body = body;
    request.key = key;
    request
}

/// "Cannot open <name>" for `path`, with the error; nothing for a cancellation.
pub fn open_failed_notice(path: &str, error: &IdeError, i18n: &I18n) -> Option<NotificationRequest> {
    (error.kind != ErrorKind::Cancelled).then(|| failure_notice(path, &error.message, i18n))
}

fn failure_notice(path: &str, message: &str, i18n: &I18n) -> NotificationRequest {
    let title = i18n.t("app.window.openFailed", &[("name", Param::Text(basename(path).into_owned()))]);
    notice(NotificationLevel::Error, title, Some(message.to_owned()), Some(format!("app.open:{path}")))
}

/// What the caller of an [`OpenFlow`] does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenStep {
    /// Ask where the project opens, then report [`OpenFlow::answered`].
    Ask(Prompt),
    /// Store this "Open project in" answer globally, then report [`OpenFlow::policy_written`]
    /// whether or not it worked: the choice applies to this open either way.
    WriteOpenPolicy(String),
    /// Ask the backend to open the project (`window_open_project`), then report
    /// [`OpenFlow::window_answered`].
    RequestWindow {
        path: String,
        new_window: bool,
        files: Vec<String>,
    },
    /// Confirm leaving what the window shows (the close guard's leave check), then report
    /// [`OpenFlow::left`].
    ConfirmLeave,
    /// Ask whether `path` is a folder of a WSL distribution, then report [`OpenFlow::wsl_resolved`].
    ResolveWsl {
        path: String,
    },
    /// Connect this window to the remote folder, then report [`OpenFlow::project_opened`].
    ConnectRemote {
        target: RemoteTarget,
        path: String,
    },
    /// Open the local project in this window, then report [`OpenFlow::project_opened`].
    OpenProject {
        path: String,
    },
    /// Confirm the folder is trusted, then report [`OpenFlow::trust_confirmed`].
    ConfirmTrust {
        path: String,
    },
    /// Read whether the folder is trusted now, then report [`OpenFlow::trust_read`].
    ReadTrust {
        path: String,
    },
    /// Attach the folder to this window's workspace, then report [`OpenFlow::attached`].
    Attach {
        path: String,
    },
    /// Read the window's project again (it has a workspace now), then report
    /// [`OpenFlow::project_read`].
    ReadCurrentProject,
    /// Reveal the workspace member in the Project view, then report [`OpenFlow::revealed`].
    RevealProject {
        path: String,
    },
    /// Open the files as tabs, reporting a file that fails with [`open_failed_notice`], then
    /// report [`OpenFlow::files_opened`].
    OpenFiles(Vec<FileOpen>),
    /// Opening failed: release the editor's close decision (`cancel_close`) and raise `notice`,
    /// then report [`OpenFlow::reported`].
    Failed {
        cancel_close: bool,
        notice: Option<NotificationRequest>,
    },
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Asking,
    WritingPolicy,
    Requesting,
    Leaving,
    Resolving,
    Opening,
    Trusting,
    ReadingTrust,
    Attaching,
    ReadingProject,
    Revealing,
    OpeningFiles,
    Failing,
    Done,
}

/// Opening one project: where it opens, then in this window leaving what it shows, then the
/// files in its editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFlow {
    path: String,
    files: Vec<String>,
    window: WindowState,
    phase: Phase,
    /// The answer "this window" asked the backend first; if it cannot answer, the project opens
    /// here anyway.
    fallback_here: bool,
    /// The chosen mode while its "Don't ask again" is stored.
    chosen: String,
    opened_here: bool,
}

impl OpenFlow {
    fn new(path: &str, files: &[String], window: &WindowState) -> Self {
        Self {
            path: path.to_owned(),
            files: files.to_vec(),
            window: window.clone(),
            phase: Phase::Done,
            fallback_here: false,
            chosen: String::new(),
            opened_here: false,
        }
    }

    /// Whether the project is now this window's own (it was already, or it opened here).
    pub fn opened_here(&self) -> bool {
        self.opened_here
    }

    fn request(&mut self, new_window: bool) -> OpenStep {
        self.phase = Phase::Requesting;
        OpenStep::RequestWindow { path: self.path.clone(), new_window, files: self.files.clone() }
    }

    fn done(&mut self) -> OpenStep {
        self.phase = Phase::Done;
        OpenStep::Done
    }

    fn fail(&mut self, cancel_close: bool, notice: Option<NotificationRequest>) -> OpenStep {
        self.phase = Phase::Failing;
        OpenStep::Failed { cancel_close, notice }
    }

    fn open_files(&mut self) -> OpenStep {
        if self.files.is_empty() {
            return self.done();
        }
        self.phase = Phase::OpeningFiles;
        OpenStep::OpenFiles(editor_opens(&self.files, &[]))
    }

    /// Opens `path` in a window, focusing the one that already has it: a new one when
    /// `new_window` is asked for or this one shows a project or LightEdit session, else this one.
    /// `files` then open in the project's editor.
    pub fn in_window(path: &str, new_window: bool, files: &[String], window: &WindowState) -> (Self, OpenStep) {
        let mut flow = Self::new(path, files, window);
        let step = flow.request(new_window || window.has_content());
        (flow, step)
    }

    /// File › Open… with an explicit choice of window: `new_window` as asked, whatever this window
    /// shows (opening here still confirms leaving it).
    pub fn explicit(path: &str, new_window: bool, window: &WindowState) -> (Self, OpenStep) {
        let mut flow = Self::new(path, &[], window);
        let step = flow.request(new_window);
        (flow, step)
    }

    /// Opens `path` in this window (a launch, a "this window" answer): nothing to do when it is
    /// this window's project already.
    pub fn here(path: &str, window: &WindowState) -> (Self, OpenStep) {
        let mut flow = Self::new(path, &[], window);
        let step = flow.start_here();
        (flow, step)
    }

    /// Opens `path` following "Open project in" when this window shows a project or LightEdit
    /// session, then `files` in its editor. A member of this window's workspace is revealed.
    pub fn with_policy(path: &str, files: &[String], window: &WindowState, i18n: &I18n) -> (Self, OpenStep) {
        if !window.has_content() {
            return Self::in_window(path, false, files, window);
        }
        let mut flow = Self::new(path, files, window);
        if window.has_local(path) {
            let step = flow.open_files();
            return (flow, step);
        }
        let member = window.workspace.as_ref().is_some_and(|workspace| workspace.projects.iter().any(|p| p == path));
        if !window.remote && member {
            flow.phase = Phase::Revealing;
            return (flow, OpenStep::RevealProject { path: path.to_owned() });
        }
        let step = match window.open_project_in.as_str() {
            mode @ ("new" | "same") => flow.act(mode),
            _ => {
                flow.phase = Phase::Asking;
                OpenStep::Ask(window_prompt(&basename(path), window.project.is_some() && !window.remote, i18n))
            }
        };
        (flow, step)
    }

    fn act(&mut self, mode: &str) -> OpenStep {
        match mode {
            "new" => self.request(true),
            "attach" => {
                self.phase = Phase::Trusting;
                OpenStep::ConfirmTrust { path: self.path.clone() }
            }
            _ => {
                self.fallback_here = true;
                self.request(false)
            }
        }
    }

    fn start_here(&mut self) -> OpenStep {
        if self.window.has_local(&self.path) {
            self.opened_here = true;
            return self.open_files();
        }
        if self.window.has_content() {
            self.phase = Phase::Leaving;
            return OpenStep::ConfirmLeave;
        }
        self.resolve()
    }

    fn resolve(&mut self) -> OpenStep {
        self.phase = Phase::Resolving;
        OpenStep::ResolveWsl { path: self.path.clone() }
    }

    /// The "where" prompt was answered.
    pub fn answered(&mut self, answer: &Answer) -> OpenStep {
        let button = answer.button.as_str();
        if self.phase != Phase::Asking || !matches!(button, "same" | "new" | "attach") {
            return self.done();
        }
        if answer.checked && button != "attach" {
            self.phase = Phase::WritingPolicy;
            self.chosen = button.to_owned();
            return OpenStep::WriteOpenPolicy(button.to_owned());
        }
        self.act(button)
    }

    /// The remembered answer was stored, or storing it failed.
    pub fn policy_written(&mut self) -> OpenStep {
        let chosen = std::mem::take(&mut self.chosen);
        self.act(&chosen)
    }

    /// The backend answered where the project opens.
    pub fn window_answered(&mut self, answer: Result<OpenOutcome, IdeError>, i18n: &I18n) -> OpenStep {
        match answer {
            Ok(OpenOutcome::Current) => self.start_here(),
            Ok(_) => self.done(),
            Err(_) if self.fallback_here => self.start_here(),
            Err(error) => {
                let notice = open_failed_notice(&self.path, &error, i18n);
                self.fail(false, notice)
            }
        }
    }

    /// Leaving what the window shows was confirmed (`true`) or the user stayed.
    pub fn left(&mut self, confirmed: bool) -> OpenStep {
        if confirmed { self.resolve() } else { self.done() }
    }

    /// The folder is on the share of a WSL distribution (`Some`: the distribution and the path in
    /// it): it runs there, not as a network folder.
    pub fn wsl_resolved(&mut self, wsl: Option<(RemoteTarget, String)>) -> OpenStep {
        self.phase = Phase::Opening;
        match wsl {
            Some((target, path)) => OpenStep::ConnectRemote { target, path },
            None => OpenStep::OpenProject { path: self.path.clone() },
        }
    }

    /// The project opened in this window, or did not (a failure or a cancellation).
    pub fn project_opened(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> OpenStep {
        match result {
            Ok(()) => {
                self.opened_here = true;
                self.open_files()
            }
            Err(error) => {
                let notice = open_failed_notice(&self.path, &error, i18n);
                self.fail(true, notice)
            }
        }
    }

    /// The trust prompt was answered (`Err`: cancelled or failed).
    pub fn trust_confirmed(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> OpenStep {
        match result {
            Ok(()) => {
                self.phase = Phase::ReadingTrust;
                OpenStep::ReadTrust { path: self.path.clone() }
            }
            Err(error) => {
                let notice = open_failed_notice(&self.path, &error, i18n);
                self.fail(false, notice)
            }
        }
    }

    /// Whether the folder is trusted now. A restricted folder is not attached: attaching would
    /// run its build tools.
    pub fn trust_read(&mut self, trusted: Result<bool, IdeError>, i18n: &mut I18n) -> OpenStep {
        match trusted {
            Ok(true) => {
                self.phase = Phase::Attaching;
                OpenStep::Attach { path: self.path.clone() }
            }
            Ok(false) => {
                let title = i18n.tx("Project not attached");
                let body = restricted_text(&self.path, i18n);
                self.fail(false, Some(notice(NotificationLevel::Warning, title, Some(body), None)))
            }
            Err(error) => {
                let notice = open_failed_notice(&self.path, &error, i18n);
                self.fail(false, notice)
            }
        }
    }

    /// The folder was attached, or attaching failed.
    pub fn attached(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> OpenStep {
        match result {
            Ok(()) => {
                self.phase = Phase::ReadingProject;
                OpenStep::ReadCurrentProject
            }
            Err(error) => {
                let notice = open_failed_notice(&self.path, &error, i18n);
                self.fail(false, notice)
            }
        }
    }

    /// The window has its project with the workspace as the backend has it now.
    pub fn project_read(&mut self) -> OpenStep {
        self.phase = Phase::Revealing;
        OpenStep::RevealProject { path: self.path.clone() }
    }

    pub fn revealed(&mut self) -> OpenStep {
        self.open_files()
    }

    pub fn files_opened(&mut self) -> OpenStep {
        self.done()
    }

    /// A [`OpenStep::Failed`] was carried out.
    pub fn reported(&mut self) -> OpenStep {
        self.done()
    }
}

/// "Cannot attach <path> because it is restricted. …", translated.
pub fn restricted_text(path: &str, i18n: &mut I18n) -> String {
    i18n.tx_built(
        &[
            "Cannot attach ",
            " because it is restricted. Open it separately and choose Trust Project, then attach it again.",
        ],
        &[Param::Text(path.to_owned())],
    )
}

/// "Open project in" asked: Cancel, Attach (a local project window only), New Window, This Window.
fn window_prompt(name: &str, can_attach: bool, i18n: &I18n) -> Prompt {
    let button = |id: &'static str, key: &str, primary: bool| PromptButton { id, label: i18n.t(key, &[]), primary };
    let mut buttons = vec![button("cancel", "common.cancel", false)];
    if can_attach {
        buttons.push(button("attach", "app.window.askAttach", false));
    }
    buttons.extend([button("new", "app.window.askNew", false), button("same", "app.window.askThis", true)]);
    Prompt {
        title: i18n.t("app.window.askTitle", &[]),
        body: i18n.t("app.window.askBody", &[("name", name.into())]),
        buttons,
        checkbox: Some(i18n.t("app.window.askRemember", &[])),
        warning: false,
    }
}

/// What the caller of [`LooseFiles`] does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LooseStep {
    /// Ask the backend which of the files this window opens itself (`window_open_files`): those
    /// inside a project open elsewhere go there, the rest to the LightEdit window. Report
    /// [`LooseFiles::routed`].
    Route(Vec<String>),
    /// Turn this window into LightEdit with the file, then report [`LooseFiles::light_edit_opened`].
    OpenLightEdit {
        path: String,
    },
    /// Open the tabs (a failing file is reported with [`open_failed_notice`]).
    OpenFiles(Vec<FileOpen>),
    /// Raise the notice.
    Notify(NotificationRequest),
    Done,
}

/// Loose files opened with File › Open File…, dropped on the Welcome screen or routed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseFiles {
    window: WindowState,
    here: Vec<String>,
}

impl LooseFiles {
    /// In this window's local project they open in its editor; otherwise the backend routes them.
    pub fn open(files: &[String], window: &WindowState) -> (Self, LooseStep) {
        let flow = Self { window: window.clone(), here: Vec::new() };
        let step = if files.is_empty() {
            LooseStep::Done
        } else if window.project.is_some() && !window.remote {
            LooseStep::OpenFiles(editor_opens(files, &[]))
        } else {
            LooseStep::Route(files.to_vec())
        };
        (flow, step)
    }

    /// The backend left `here` to this window, or could not route (`Err`: reported for the first
    /// file).
    pub fn routed(&mut self, here: Result<Vec<String>, IdeError>, first: &str, i18n: &I18n) -> LooseStep {
        match here {
            Ok(here) if here.is_empty() => LooseStep::Done,
            Ok(here) if !self.window.has_content() => {
                let path = here[0].clone();
                self.here = here;
                LooseStep::OpenLightEdit { path }
            }
            Ok(here) => LooseStep::OpenFiles(editor_opens(&here, &[])),
            Err(error) => open_failed_notice(first, &error, i18n).map_or(LooseStep::Done, LooseStep::Notify),
        }
    }

    /// The window is in LightEdit now, or turning it into LightEdit failed.
    pub fn light_edit_opened(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> LooseStep {
        let here = std::mem::take(&mut self.here);
        match result {
            Ok(()) => LooseStep::OpenFiles(editor_opens(&here, &[])),
            Err(error) => {
                let first = here.first().map_or("", String::as_str);
                open_failed_notice(first, &error, i18n).map_or(LooseStep::Done, LooseStep::Notify)
            }
        }
    }
}

/// Dropped paths: the files (opened as loose files, [`LooseFiles::open`]) and the first folder
/// (opened as a project, [`OpenFlow::with_policy`]). A path `is_dir` cannot tell is a file.
pub fn split_dropped(paths: &[String], is_dir: impl FnMut(&str) -> Option<bool>) -> (Vec<String>, Option<String>) {
    let mut files = Vec::new();
    let mut folder = None;
    for action in open_dropped(paths, is_dir) {
        match action {
            DropAction::OpenFile { path, .. } => files.push(path),
            DropAction::OpenProject { path } => folder = Some(path),
        }
    }
    (files, folder)
}

/// LightEdit › Open Containing Folder as Project: the active file's folder, with the file to
/// open there ("Open project in" applies). Nothing outside LightEdit or without an active file.
pub fn light_edit_file_as_project(window: &WindowState, active: Option<&str>) -> Option<(String, Vec<String>)> {
    let path = active.filter(|_| window.light_edit)?;
    Some((dirname(path).to_owned(), vec![path.to_owned()]))
}

/// LightEdit: the window closes once its last tab is closed (through the close guard, so the last
/// window shows the Welcome screen instead). A window leaving LightEdit is not closed.
pub fn light_edit_should_close(active_tab: bool, light_edit: bool, open_tabs: usize) -> bool {
    !active_tab && light_edit && open_tabs == 0
}

/// Which project tabs of a window group a Close … Project Tabs action closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSide {
    Others,
    Left,
    Right,
}

/// The labels to close around `keep` (the current tab when not given), keeping it; `None` when
/// there is nothing to close or no such tab.
pub fn tabs_to_close(labels: &[(String, bool)], keep: Option<&str>, side: TabSide) -> Option<(String, Vec<String>)> {
    let current = || labels.iter().find(|(_, current)| *current).map(|(label, _)| label.clone());
    let keep = keep.map(str::to_owned).or_else(current)?;
    let index = labels.iter().position(|(label, _)| *label == keep)?;
    let close: Vec<String> = labels
        .iter()
        .enumerate()
        .filter(|(i, _)| match side {
            TabSide::Left => *i < index,
            TabSide::Right => *i > index,
            TabSide::Others => *i != index,
        })
        .map(|(_, (label, _))| label.clone())
        .collect();
    (!close.is_empty()).then_some((keep, close))
}

/// What the caller of [`RemoveFromWorkspace`] does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveStep {
    /// Confirm detaching, then report [`RemoveFromWorkspace::answered`].
    Ask(Prompt),
    /// Write the workspace with these members (`workspace_update`) and show the project it answers.
    Update {
        name: String,
        projects: Vec<String>,
    },
    Done,
}

/// Remove from Workspace / Detach Project. The primary project of a workspace it opened is never
/// removed, and detaching one of its attachments is confirmed first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveFromWorkspace {
    update: Option<(String, Vec<String>)>,
}

impl RemoveFromWorkspace {
    pub fn start(member: &str, window: &WindowState, i18n: &I18n) -> (Self, RemoveStep) {
        let none = (Self { update: None }, RemoveStep::Done);
        let (Some(root), Some(workspace)) = (window.project.as_deref(), window.workspace.as_ref()) else { return none };
        if !workspace.projects.iter().any(|p| p == member) {
            return none;
        }
        let projects: Vec<String> = workspace.projects.iter().filter(|p| *p != member).cloned().collect();
        let update = (workspace.name.clone(), projects);
        if workspace.projects.first().map(String::as_str) != Some(root) {
            return (Self { update: None }, RemoveStep::Update { name: update.0, projects: update.1 });
        }
        if member == root {
            return none;
        }
        let prompt = Prompt {
            title: i18n.t("app.window.detachTitle", &[]),
            body: i18n.t("app.window.detachBody", &[("name", Param::Text(basename(member).into_owned()))]),
            buttons: vec![
                PromptButton { id: "cancel", label: i18n.t("common.cancel", &[]), primary: false },
                PromptButton { id: "detach", label: i18n.t("app.window.detach", &[]), primary: true },
            ],
            checkbox: None,
            warning: false,
        };
        (Self { update: Some(update) }, RemoveStep::Ask(prompt))
    }

    pub fn answered(&mut self, answer: &Answer) -> RemoveStep {
        match self.update.take() {
            Some((name, projects)) if answer.button == "detach" => RemoveStep::Update { name, projects },
            _ => RemoveStep::Done,
        }
    }
}

/// What a window was opened for or told to show, as the backend hands it over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub project: Option<String>,
    pub files: Vec<String>,
    pub positions: Vec<FilePosition>,
    /// A remote project folder to connect to.
    pub remote: Option<(RemoteTarget, String)>,
    /// `openidea diff a b`: the left and right file.
    pub diff: Option<(String, String)>,
    /// Files requested after the pending diff take the focus after it opens.
    pub diff_before_files: bool,
}

/// What the caller of [`LaunchQueue`] does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchStep {
    /// Listen for the backend's launch event; report [`LaunchQueue::listening`] once it listens.
    Listen,
    /// Stop listening.
    Unlisten,
    /// Take this window's pending launch (`window_take_launch`), then report [`LaunchQueue::taken`].
    Take,
    /// Wait until the window's project has finished opening, then report [`LaunchQueue::restored`].
    WaitForRestore,
    /// Carry out the step of opening the launch's project here; answer it through
    /// [`LaunchQueue::open`].
    Open(OpenStep),
    /// Turn the window into LightEdit with the file, then report [`LaunchQueue::light_edit_opened`].
    OpenLightEdit { path: String },
    /// Open the two files side by side in the diff viewer (both editable), then report
    /// [`LaunchQueue::diff_opened`].
    OpenDiff { left: String, right: String },
    /// Open the tabs (a failing file is reported with [`open_failed_notice`]), then report
    /// [`LaunchQueue::files_opened`].
    OpenFiles(Vec<FileOpen>),
    /// Connect the window to the remote folder, then report [`LaunchQueue::remote_connected`].
    ConnectRemote { target: RemoteTarget, path: String },
    /// Raise the notice, then report [`LaunchQueue::notified`].
    Notify(NotificationRequest),
    /// Release the window's launch (`window_complete_launch`), then report [`LaunchQueue::completed`].
    Complete,
    /// Nothing to do until the next launch event.
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Project,
    DiffBefore,
    Files,
    DiffAfter,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    launch: Launch,
    window: WindowState,
    stage: Stage,
    flow: Option<OpenFlow>,
    /// Where the run goes on after a notice.
    after_notice: Stage,
    failure: Option<String>,
}

/// This window's launches, handled one at a time and in the order they arrive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchQueue {
    disposed: bool,
    listening: bool,
    busy: bool,
    queued: usize,
    run: Option<Run>,
    last_failure: Option<String>,
}

impl LaunchQueue {
    /// Starts launch handling: it listens first, so that a launch arriving meanwhile is not missed.
    pub fn install() -> (Self, LaunchStep) {
        (Self::default(), LaunchStep::Listen)
    }

    /// The listener is registered: the pending launch is taken now (or, after disposal, it stops).
    pub fn listening(&mut self) -> LaunchStep {
        if self.disposed {
            return LaunchStep::Unlisten;
        }
        self.listening = true;
        self.request()
    }

    /// The window goes: it stops listening once it listens.
    pub fn dispose(&mut self) -> LaunchStep {
        self.disposed = true;
        if self.listening { LaunchStep::Unlisten } else { LaunchStep::Idle }
    }

    /// The backend's launch event, or a request to take the pending launch. While one runs, this
    /// one waits its turn.
    pub fn request(&mut self) -> LaunchStep {
        if self.disposed {
            return LaunchStep::Idle;
        }
        if self.busy {
            self.queued += 1;
            return LaunchStep::Idle;
        }
        self.busy = true;
        LaunchStep::Take
    }

    /// The failure the last finished launch ended with, if any.
    pub fn last_failure(&self) -> Option<&str> {
        self.last_failure.as_deref()
    }

    fn next(&mut self) -> LaunchStep {
        self.busy = false;
        if self.queued > 0 && !self.disposed {
            self.queued -= 1;
            self.busy = true;
            return LaunchStep::Take;
        }
        LaunchStep::Idle
    }

    /// The pending launch (`None`: there was none) and what the window shows now.
    pub fn taken(&mut self, launch: Option<Launch>, window: &WindowState) -> LaunchStep {
        let Some(launch) = launch else { return self.next() };
        let run = Run {
            launch,
            window: window.clone(),
            stage: Stage::Project,
            flow: None,
            after_notice: Stage::Complete,
            failure: None,
        };
        self.run = Some(run);
        if window.opening {
            // Session restore may be opening this window's project already.
            return LaunchStep::WaitForRestore;
        }
        self.from(Stage::Project)
    }

    /// The project the window was restoring is open (or not); `window` is what it shows now.
    pub fn restored(&mut self, window: &WindowState) -> LaunchStep {
        if let Some(run) = self.run.as_mut() {
            run.window = window.clone();
        }
        self.from(Stage::Project)
    }

    fn from(&mut self, stage: Stage) -> LaunchStep {
        let Some(run) = self.run.as_mut() else { return LaunchStep::Idle };
        run.stage = stage;
        let launch = &run.launch;
        match stage {
            Stage::Project => match launch.project.clone() {
                Some(project) => {
                    let (flow, step) = OpenFlow::here(&project, &run.window);
                    run.flow = Some(flow);
                    self.flow_step(step)
                }
                None => self.from(Stage::DiffBefore),
            },
            Stage::DiffBefore | Stage::DiffAfter => {
                let wanted = launch.diff_before_files == (stage == Stage::DiffBefore);
                match launch.diff.clone().filter(|_| wanted) {
                    Some((_, right)) if !run.window.has_content() => LaunchStep::OpenLightEdit { path: right },
                    Some((left, right)) => LaunchStep::OpenDiff { left, right },
                    None if stage == Stage::DiffBefore => self.from(Stage::Files),
                    None => self.from(Stage::Complete),
                }
            }
            Stage::Files => {
                if launch.project.is_some() || (run.window.has_content() && !launch.files.is_empty()) {
                    if launch.files.is_empty() {
                        return self.from(Stage::DiffAfter);
                    }
                    return LaunchStep::OpenFiles(editor_opens(&launch.files, &launch.positions));
                }
                if let Some(first) = launch.files.first() {
                    return LaunchStep::OpenLightEdit { path: first.clone() };
                }
                match launch.remote.clone() {
                    Some((target, path)) => LaunchStep::ConnectRemote { target, path },
                    None => self.from(Stage::DiffAfter),
                }
            }
            Stage::Complete => LaunchStep::Complete,
        }
    }

    fn flow_step(&mut self, step: OpenStep) -> LaunchStep {
        if step != OpenStep::Done {
            return LaunchStep::Open(step);
        }
        let Some(run) = self.run.as_mut() else { return LaunchStep::Idle };
        if !run.flow.as_ref().is_some_and(OpenFlow::opened_here) {
            return self.from(Stage::Complete);
        }
        run.window.project = run.launch.project.clone();
        run.window.light_edit = false;
        self.from(Stage::DiffBefore)
    }

    /// Answers the current [`LaunchStep::Open`] step: `answer` reports its outcome to the flow.
    pub fn open(&mut self, answer: impl FnOnce(&mut OpenFlow) -> OpenStep) -> LaunchStep {
        let Some(flow) = self.run.as_mut().and_then(|run| run.flow.as_mut()) else { return LaunchStep::Idle };
        let step = answer(flow);
        self.flow_step(step)
    }

    fn notify(&mut self, notice: NotificationRequest, then: Stage) -> LaunchStep {
        if let Some(run) = self.run.as_mut() {
            run.after_notice = then;
        }
        LaunchStep::Notify(notice)
    }

    /// The window turned into LightEdit for the diff's right file or the first file, or did not.
    pub fn light_edit_opened(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> LaunchStep {
        let Some(run) = self.run.as_mut() else { return LaunchStep::Idle };
        if result.is_ok() {
            run.window.light_edit = true;
        }
        match run.stage {
            // The diff opens whether or not LightEdit could.
            Stage::DiffBefore | Stage::DiffAfter => match run.launch.diff.clone() {
                Some((left, right)) => LaunchStep::OpenDiff { left, right },
                None => self.from(Stage::Complete),
            },
            _ => match result {
                Ok(()) => LaunchStep::OpenFiles(editor_opens(&run.launch.files, &run.launch.positions)),
                Err(error) => {
                    let first = run.launch.files.first().cloned().unwrap_or_default();
                    match open_failed_notice(&first, &error, i18n) {
                        Some(notice) => self.notify(notice, Stage::DiffAfter),
                        None => self.from(Stage::DiffAfter),
                    }
                }
            },
        }
    }

    /// The diff opened, or did not. Before the files a failure is reported and the files still
    /// open; after them the launch ends with the failure.
    pub fn diff_opened(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> LaunchStep {
        let Some(run) = self.run.as_mut() else { return LaunchStep::Idle };
        match (run.stage, result) {
            (Stage::DiffBefore, Ok(())) => self.from(Stage::Files),
            (Stage::DiffBefore, Err(error)) => {
                let right = run.launch.diff.as_ref().map(|(_, right)| right.clone()).unwrap_or_default();
                match open_failed_notice(&right, &error, i18n) {
                    Some(notice) => self.notify(notice, Stage::Files),
                    None => self.from(Stage::Files),
                }
            }
            (_, Ok(())) => self.from(Stage::Complete),
            (_, Err(error)) => {
                run.failure = Some(error.message);
                self.from(Stage::Complete)
            }
        }
    }

    pub fn files_opened(&mut self) -> LaunchStep {
        self.from(Stage::DiffAfter)
    }

    /// The remote connection was made, or failed (reported, cancellations included).
    pub fn remote_connected(&mut self, result: Result<(), IdeError>, i18n: &I18n) -> LaunchStep {
        let path = self.run.as_ref().and_then(|run| run.launch.remote.as_ref()).map(|(_, path)| path.clone());
        match (result, path) {
            (Err(error), Some(path)) => {
                let notice = failure_notice(&path, &error.message, i18n);
                self.notify(notice, Stage::DiffAfter)
            }
            _ => self.from(Stage::DiffAfter),
        }
    }

    pub fn notified(&mut self) -> LaunchStep {
        let then = self.run.as_ref().map_or(Stage::Complete, |run| run.after_notice);
        self.from(then)
    }

    /// The launch was released, or releasing it failed: either way the next launch is handled.
    pub fn completed(&mut self, result: Result<(), String>) -> LaunchStep {
        let failure = self.run.take().and_then(|run| run.failure);
        self.last_failure = failure.or(result.err());
        self.next()
    }
}

/// The window actions of this model, with the context their menu entries need (`when`).
pub const WINDOW_ACTIONS: [(&str, Option<&str>); 9] = [
    ("project.attach", Some("projectLocal && !projectOpenAsk")),
    ("workspace.removeProject", Some("workspaceOpen && !projectAttached")),
    ("project.detach", Some("projectAttached")),
    ("workspace.manage", Some("workspaceContainer")),
    ("file.openInNewWindow", None),
    ("file.openFile", None),
    ("file.openPaths", None),
    ("lightEdit.openProject", Some("lightEdit")),
    ("file.openProjectPath", None),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn s(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|&p| p.to_owned()).collect()
    }

    fn project(root: &str) -> WindowState {
        WindowState { project: Some(root.to_owned()), ..WindowState::default() }
    }

    fn with_policy(mut window: WindowState, policy: &str) -> WindowState {
        window.open_project_in = policy.to_owned();
        window
    }

    fn request(path: &str, new_window: bool, files: &[&str]) -> OpenStep {
        OpenStep::RequestWindow { path: path.to_owned(), new_window, files: s(files) }
    }

    fn failed(message: &str) -> IdeError {
        IdeError::new(ErrorKind::Unavailable, message)
    }

    fn open_tabs(step: &OpenStep) -> Vec<String> {
        match step {
            OpenStep::OpenFiles(files) => files.iter().map(log).collect(),
            _ => Vec::new(),
        }
    }

    /// A tab as the spec's log lines: `<path>[ (focus)][ @line:col]`.
    fn log(file: &FileOpen) -> String {
        let focus = if file.focus { " (focus)" } else { "" };
        let at = file.position.map(|(l, c)| format!(" @{l}:{c}")).unwrap_or_default();
        format!("{}{focus}{at}", file.path)
    }

    // openProjectInWindow › opens in this window when the backend says `current`
    #[test]
    fn opens_in_this_window_when_the_backend_says_current() {
        let i18n = I18n::default();
        let (mut flow, step) = OpenFlow::in_window("/p/a", false, &[], &WindowState::default());
        assert_eq!(step, request("/p/a", false, &[]));
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ResolveWsl { path: "/p/a".into() });
        assert_eq!(flow.wsl_resolved(None), OpenStep::OpenProject { path: "/p/a".into() });
        assert_eq!(flow.project_opened(Ok(()), &i18n), OpenStep::Done);
        assert!(flow.opened_here());
    }

    // openProjectInWindow › connects to the distribution when the folder is on the WSL share
    #[test]
    fn connects_to_the_distribution_when_the_folder_is_on_the_wsl_share() {
        let share = r"\\wsl.localhost\Debian\srv\app";
        let (mut flow, _) = OpenFlow::in_window(share, false, &[], &WindowState::default());
        let i18n = I18n::default();
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ResolveWsl { path: share.into() });
        let target = RemoteTarget::Wsl { distro: "Debian".to_owned() };
        assert_eq!(
            flow.wsl_resolved(Some((target.clone(), "/srv/app".to_owned()))),
            OpenStep::ConnectRemote { target, path: "/srv/app".into() }
        );
    }

    // openProjectInWindow › asks for a new window when this one already has a project
    #[test]
    fn asks_for_a_new_window_when_this_one_already_has_a_project() {
        let (mut flow, step) = OpenFlow::in_window("/p/a", false, &[], &project("/p/b"));
        assert_eq!(step, request("/p/a", true, &[]));
        let opened = OpenOutcome::Opened { label: "project-1".into() };
        assert_eq!(flow.window_answered(Ok(opened), &I18n::default()), OpenStep::Done);
        assert!(!flow.opened_here());
    }

    // openProjectInWindow › asks for a new window from a LightEdit window
    #[test]
    fn asks_for_a_new_window_from_a_light_edit_window() {
        let light = WindowState { light_edit: true, ..WindowState::default() };
        assert_eq!(OpenFlow::in_window("/p/a", false, &[], &light).1, request("/p/a", true, &[]));
    }

    // openProjectInWindow › does nothing locally when another window was focused
    #[test]
    fn does_nothing_locally_when_another_window_was_focused() {
        let (mut flow, _) = OpenFlow::in_window("/p/a", true, &[], &WindowState::default());
        let focused = OpenOutcome::Focused { label: "main".into() };
        assert_eq!(flow.window_answered(Ok(focused), &I18n::default()), OpenStep::Done);
    }

    // replacing a project in this window › releases the close decision after opening fails or is cancelled: %s
    #[test]
    fn releases_the_close_decision_after_opening_fails_or_is_cancelled() {
        let i18n = I18n::default();
        for error in
            [failed("local folder unavailable"), IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")]
        {
            let cancelled = error.kind == ErrorKind::Cancelled;
            let (mut flow, step) =
                OpenFlow::with_policy("/local", &[], &with_policy(project("/remote"), "same"), &i18n);
            assert_eq!(step, request("/local", false, &[]));
            assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ConfirmLeave);
            flow.left(true);
            flow.wsl_resolved(None);
            let OpenStep::Failed { cancel_close, notice } = flow.project_opened(Err(error), &i18n) else {
                panic!("fails")
            };
            assert!(cancel_close);
            assert_eq!(notice.is_none(), cancelled);
            assert!(!flow.opened_here());
            assert_eq!(flow.reported(), OpenStep::Done);
        }
    }

    // replacing a project in this window › opens the local project when its path matches the remote root
    #[test]
    fn opens_the_local_project_when_its_path_matches_the_remote_root() {
        let i18n = I18n::default();
        let remote = WindowState { remote: true, ..with_policy(project("/same"), "same") };
        let (mut flow, step) = OpenFlow::with_policy("/same", &[], &remote, &i18n);
        assert_eq!(step, request("/same", false, &[]));
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ConfirmLeave);
        flow.left(true);
        assert_eq!(flow.wsl_resolved(None), OpenStep::OpenProject { path: "/same".into() });
    }

    fn loose(files: &[&str], window: &WindowState, here: &[&str]) -> Vec<LooseStep> {
        let i18n = I18n::default();
        let (mut flow, mut step) = LooseFiles::open(&s(files), window);
        let mut steps = Vec::new();
        loop {
            steps.push(step.clone());
            step = match step {
                LooseStep::Route(_) => flow.routed(Ok(s(here)), files[0], &i18n),
                LooseStep::OpenLightEdit { .. } => flow.light_edit_opened(Ok(()), &i18n),
                _ => return steps,
            };
        }
    }

    fn tabs(steps: &[LooseStep]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|step| match step {
                LooseStep::OpenFiles(files) => Some(files.iter().map(log)),
                _ => None,
            })
            .flatten()
            .collect()
    }

    // loose files (LightEdit) › turns a window without a project into LightEdit when no LightEdit window exists
    #[test]
    fn turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists() {
        let steps = loose(&["/x/a.txt", "/y/b.md"], &WindowState::default(), &["/x/a.txt", "/y/b.md"]);
        assert_eq!(steps[0], LooseStep::Route(s(&["/x/a.txt", "/y/b.md"])));
        assert_eq!(steps[1], LooseStep::OpenLightEdit { path: "/x/a.txt".into() });
        assert_eq!(tabs(&steps), ["/x/a.txt", "/y/b.md (focus)"]);
    }

    // loose files (LightEdit) › leaves files to the existing LightEdit window or their project's window
    #[test]
    fn leaves_files_to_the_existing_light_edit_window_or_their_projects_window() {
        let steps = loose(&["/x/a.txt"], &WindowState::default(), &[]);
        assert_eq!(steps, [LooseStep::Route(s(&["/x/a.txt"])), LooseStep::Done]);
    }

    // loose files (LightEdit) › opens only the files the backend leaves to this window
    #[test]
    fn opens_only_the_files_the_backend_leaves_to_this_window() {
        let steps = loose(&["/p/a.rs", "/x/b.txt"], &WindowState::default(), &["/x/b.txt"]);
        assert_eq!(steps[1], LooseStep::OpenLightEdit { path: "/x/b.txt".into() });
        assert_eq!(tabs(&steps), ["/x/b.txt (focus)"]);
    }

    // loose files (LightEdit) › adds tabs to this LightEdit window
    #[test]
    fn adds_tabs_to_this_light_edit_window() {
        let light = WindowState { light_edit: true, ..WindowState::default() };
        let steps = loose(&["/x/c.txt"], &light, &["/x/c.txt"]);
        assert_eq!(steps[0], LooseStep::Route(s(&["/x/c.txt"])));
        assert!(!steps.iter().any(|step| matches!(step, LooseStep::OpenLightEdit { .. })));
        assert_eq!(tabs(&steps), ["/x/c.txt (focus)"]);
    }

    // loose files (LightEdit) › opens files in the project's editor in a project window
    #[test]
    fn opens_files_in_the_projects_editor_in_a_project_window() {
        let steps = loose(&["/elsewhere/a.txt"], &project("/p"), &[]);
        assert!(!steps.iter().any(|step| matches!(step, LooseStep::Route(_))));
        assert_eq!(tabs(&steps), ["/elsewhere/a.txt (focus)"]);
    }

    // loose files (LightEdit) › routes dropped files to LightEdit and the dropped folder to a project
    #[test]
    fn routes_dropped_files_to_light_edit_and_the_dropped_folder_to_a_project() {
        let (files, folder) = split_dropped(&s(&["/x/a.txt", "/work/app"]), |p| Some(p == "/work/app"));
        let window = with_policy(WindowState::default(), "new");
        let steps = loose(&["/x/a.txt"], &window, &["/x/a.txt"]);
        assert_eq!((files, folder.as_deref()), (s(&["/x/a.txt"]), Some("/work/app")));
        assert_eq!(steps[1], LooseStep::OpenLightEdit { path: "/x/a.txt".into() });
        // The window is in LightEdit by now: the folder goes to a new window.
        let light = WindowState { light_edit: true, ..window };
        let (_, step) = OpenFlow::with_policy("/work/app", &[], &light, &I18n::default());
        assert_eq!(step, request("/work/app", true, &[]));
    }

    // loose files (LightEdit) › opens a dropped folder alone exactly as before
    #[test]
    fn opens_a_dropped_folder_alone_exactly_as_before() {
        let (files, folder) = split_dropped(&s(&["/work/app"]), |_| Some(true));
        assert!(files.is_empty());
        assert_eq!(LooseFiles::open(&files, &WindowState::default()).1, LooseStep::Done);
        let (mut flow, step) =
            OpenFlow::with_policy(&folder.expect("a folder"), &[], &WindowState::default(), &I18n::default());
        assert_eq!(step, request("/work/app", false, &[]));
        flow.window_answered(Ok(OpenOutcome::Current), &I18n::default());
        assert_eq!(flow.wsl_resolved(None), OpenStep::OpenProject { path: "/work/app".into() });
    }

    // loose files (LightEdit) › classifies paths through the file system by default
    #[test]
    fn classifies_paths_through_the_file_system_by_default() {
        // A path whose folder listing fails is a file.
        let (files, folder) = split_dropped(&s(&["/x/a.txt"]), |_| None);
        assert_eq!((files, folder), (s(&["/x/a.txt"]), None));
    }

    // Open Containing Folder as Project › is an action available in LightEdit only
    #[test]
    fn is_an_action_available_in_light_edit_only() {
        let when = |id: &str| WINDOW_ACTIONS.iter().find(|(known, _)| *known == id).map(|(_, when)| *when);
        assert_eq!(when("lightEdit.openProject"), Some(Some("lightEdit")));
        assert_eq!(when("file.openFile"), Some(None));
    }

    // Open Containing Folder as Project › opens the folder in a new window with the file, per 'Open project in'
    #[test]
    fn opens_the_folder_in_a_new_window_with_the_file_per_open_project_in() {
        let light = with_policy(WindowState { light_edit: true, ..WindowState::default() }, "new");
        let (dir, files) = light_edit_file_as_project(&light, Some("/x/notes/a.txt")).expect("in LightEdit");
        let (_, step) = OpenFlow::with_policy(&dir, &files, &light, &I18n::default());
        assert_eq!(step, request("/x/notes", true, &["/x/notes/a.txt"]));
    }

    // Open Containing Folder as Project › or turns this window into the project and reopens the file there
    #[test]
    fn or_turns_this_window_into_the_project_and_reopens_the_file_there() {
        let i18n = I18n::default();
        let light = with_policy(WindowState { light_edit: true, ..WindowState::default() }, "same");
        let (dir, files) = light_edit_file_as_project(&light, Some("/x/notes/a.txt")).expect("in LightEdit");
        let (mut flow, step) = OpenFlow::with_policy(&dir, &files, &light, &i18n);
        assert_eq!(step, request("/x/notes", false, &["/x/notes/a.txt"]));
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ConfirmLeave);
        flow.left(true);
        assert_eq!(flow.wsl_resolved(None), OpenStep::OpenProject { path: "/x/notes".into() });
        assert_eq!(open_tabs(&flow.project_opened(Ok(()), &i18n)), ["/x/notes/a.txt (focus)"]);
    }

    // Open Containing Folder as Project › does nothing outside LightEdit
    #[test]
    fn does_nothing_outside_light_edit() {
        assert_eq!(light_edit_file_as_project(&project("/p"), Some("/p/a.txt")), None);
    }

    /// Runs launches through `queue`: opens succeed except the project paths in `failing`
    /// (with `error`), editor tabs fail for `missing`, diffs fail when `diff_fails`. Returns the
    /// log: `tab …`, `diff …`, `light …`, `project …`, `notify …`, `complete`.
    struct Script {
        launches: Vec<Option<Launch>>,
        window: WindowState,
        failing: Option<(String, IdeError)>,
        missing: Vec<String>,
        diff_fails: bool,
        complete_fails: bool,
    }

    impl Script {
        fn new(window: WindowState, launches: Vec<Option<Launch>>) -> Self {
            Self { launches, window, failing: None, missing: Vec::new(), diff_fails: false, complete_fails: false }
        }

        fn run(mut self, requests: usize) -> (Vec<String>, LaunchQueue) {
            let i18n = I18n::default();
            let mut queue = LaunchQueue::default();
            let mut log_lines = Vec::new();
            let mut pending: Vec<LaunchStep> = (0..requests).map(|_| queue.request()).collect();
            pending.retain(|step| *step != LaunchStep::Idle);
            let mut launches = self.launches.drain(..);
            let Some(mut step) = pending.pop() else { return (log_lines, queue) };
            loop {
                step = match step {
                    LaunchStep::Take => {
                        let launch = launches.next().flatten();
                        queue.taken(launch, &self.window)
                    }
                    LaunchStep::WaitForRestore => queue.restored(&self.window),
                    LaunchStep::Open(open) => match open {
                        OpenStep::ConfirmLeave => queue.open(|flow| flow.left(true)),
                        OpenStep::ResolveWsl { .. } => queue.open(|flow| flow.wsl_resolved(None)),
                        OpenStep::OpenProject { path } => {
                            log_lines.push(format!("project {path}"));
                            let result = match &self.failing {
                                Some((failing, error)) if *failing == path => Err(error.clone()),
                                _ => {
                                    self.window.project = Some(path.clone());
                                    Ok(())
                                }
                            };
                            queue.open(|flow| flow.project_opened(result, &i18n))
                        }
                        OpenStep::Failed { notice, .. } => {
                            if let Some(notice) = notice {
                                log_lines.push(format!("notify {}", notice.body.unwrap_or_default()));
                            }
                            queue.open(OpenFlow::reported)
                        }
                        other => panic!("unexpected {other:?}"),
                    },
                    LaunchStep::OpenLightEdit { path } => {
                        log_lines.push(format!("light {path}"));
                        self.window.light_edit = true;
                        queue.light_edit_opened(Ok(()), &i18n)
                    }
                    LaunchStep::OpenDiff { left, right } => {
                        log_lines.push(format!("diff {left} {right}"));
                        let result = if self.diff_fails { Err(failed("Diff file unavailable")) } else { Ok(()) };
                        queue.diff_opened(result, &i18n)
                    }
                    LaunchStep::OpenFiles(files) => {
                        for file in &files {
                            if self.missing.contains(&file.path) {
                                let notice = open_failed_notice(&file.path, &failed("File not found"), &i18n);
                                log_lines.push(format!("notify {}", notice.and_then(|n| n.body).unwrap_or_default()));
                            } else {
                                log_lines.push(format!("tab {}", log(file)));
                            }
                        }
                        queue.files_opened()
                    }
                    LaunchStep::ConnectRemote { path, .. } => {
                        log_lines.push(format!("remote {path}"));
                        queue.remote_connected(Ok(()), &i18n)
                    }
                    LaunchStep::Notify(notice) => {
                        log_lines.push(format!("notify {}", notice.body.unwrap_or_default()));
                        queue.notified()
                    }
                    LaunchStep::Complete => {
                        log_lines.push("complete".to_owned());
                        let result = if std::mem::take(&mut self.complete_fails) {
                            Err("Connection interrupted".to_owned())
                        } else {
                            Ok(())
                        };
                        let next = queue.completed(result);
                        if let Some(failure) = queue.last_failure() {
                            log_lines.push(format!("rejected {failure}"));
                        }
                        next
                    }
                    LaunchStep::Idle | LaunchStep::Listen | LaunchStep::Unlisten => return (log_lines, queue),
                };
            }
        }
    }

    fn launch(project: Option<&str>, files: &[&str]) -> Launch {
        Launch { project: project.map(str::to_owned), files: s(files), ..Launch::default() }
    }

    fn at(path: &str, line: u32, column: u32) -> Vec<FilePosition> {
        vec![FilePosition { path: path.to_owned(), line, column }]
    }

    // launch › subscribes before taking a launch received while the listener is registering
    #[test]
    fn subscribes_before_taking_a_launch_received_while_the_listener_is_registering() {
        let (mut queue, step) = LaunchQueue::install();
        assert_eq!(step, LaunchStep::Listen);
        // A launch event before the listener is registered is not lost: the take follows it.
        assert_eq!(queue.listening(), LaunchStep::Take);
        let opened = queue.taken(Some(launch(Some("/p"), &["/p/late.cs"])), &project("/p"));
        assert_eq!(opened, LaunchStep::OpenFiles(editor_opens(&s(&["/p/late.cs"]), &[])));
        assert_eq!(queue.files_opened(), LaunchStep::Complete);
        assert_eq!(queue.completed(Ok(())), LaunchStep::Idle);
        assert_eq!(queue.dispose(), LaunchStep::Unlisten);
    }

    // launch › does not consume a launch after disposal while the listener is registering
    #[test]
    fn does_not_consume_a_launch_after_disposal_while_the_listener_is_registering() {
        let (mut queue, _) = LaunchQueue::install();
        assert_eq!(queue.dispose(), LaunchStep::Idle);
        assert_eq!(queue.listening(), LaunchStep::Unlisten);
        assert_eq!(queue.request(), LaunchStep::Idle);
    }

    // launch › serializes launch events while their project is opening
    #[test]
    fn serializes_launch_events_while_their_project_is_opening() {
        let second = Launch { positions: at("/p/b.cs", 10, 3), ..launch(None, &["/p/b.cs"]) };
        let script = Script::new(WindowState::default(), vec![Some(launch(Some("/p"), &["/p/a.cs"])), Some(second)]);
        let (log, _) = script.run(2);
        assert_eq!(log, ["project /p", "tab /p/a.cs (focus)", "complete", "tab /p/b.cs (focus) @10:3", "complete"]);
    }

    // launch › waits for an existing project restore before opening a received file
    #[test]
    fn waits_for_an_existing_project_restore_before_opening_a_received_file() {
        let mut queue = LaunchQueue::default();
        assert_eq!(queue.request(), LaunchStep::Take);
        let opening = WindowState { opening: true, ..WindowState::default() };
        let received = Launch { positions: at("/p/a.cs", 8, 2), ..launch(None, &["/p/a.cs"]) };
        assert_eq!(queue.taken(Some(received), &opening), LaunchStep::WaitForRestore);
        let step = queue.restored(&project("/p"));
        assert_eq!(step, LaunchStep::OpenFiles(editor_opens(&s(&["/p/a.cs"]), &at("/p/a.cs", 8, 2))));
        assert_eq!(queue.files_opened(), LaunchStep::Complete);
    }

    // launch › keeps the open project and applies later locations in order
    #[test]
    fn keeps_the_open_project_and_applies_later_locations_in_order() {
        let launches = [5, 8, 12]
            .map(|line| Some(Launch { positions: at("/p/a.cs", line, 0), ..launch(Some("/p"), &["/p/a.cs"]) }));
        let (log, _) = Script::new(project("/p"), launches.to_vec()).run(3);
        let tabs: Vec<&String> = log.iter().filter(|line| line.starts_with("tab")).collect();
        assert_eq!(tabs, ["tab /p/a.cs (focus) @5:0", "tab /p/a.cs (focus) @8:0", "tab /p/a.cs (focus) @12:0"]);
        assert!(!log.iter().any(|line| line.starts_with("project")));
    }

    // launch › reports a missing file and still handles the next launch
    #[test]
    fn reports_a_missing_file_and_still_handles_the_next_launch() {
        let mut script =
            Script::new(project("/p"), vec![Some(launch(None, &["/p/missing.cs"])), Some(launch(None, &["/p/a.cs"]))]);
        script.missing = s(&["/p/missing.cs"]);
        let (log, _) = script.run(2);
        assert_eq!(log, ["notify File not found", "complete", "tab /p/a.cs (focus)", "complete"]);
    }

    // launch › completes a failed or cancelled project launch and handles the next: %s
    #[test]
    fn completes_a_failed_or_cancelled_project_launch_and_handles_the_next() {
        for error in [failed("Project unavailable"), IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")] {
            let launches = vec![Some(launch(Some("/p"), &["/p/a.cs"])), Some(launch(Some("/q"), &["/q/b.cs"]))];
            let mut script = Script::new(WindowState::default(), launches);
            script.failing = Some(("/p".to_owned(), error));
            let (log, _) = script.run(2);
            let tabs: Vec<&String> = log.iter().filter(|line| line.starts_with("tab")).collect();
            assert_eq!(tabs, ["tab /q/b.cs (focus)"]);
            assert_eq!(log.iter().filter(|line| *line == "complete").count(), 2);
        }
    }

    // launch › keeps handling launches after completing a launch fails
    #[test]
    fn keeps_handling_launches_after_completing_a_launch_fails() {
        let mut script =
            Script::new(project("/p"), vec![Some(launch(None, &["/p/a.cs"])), Some(launch(None, &["/p/b.cs"]))]);
        script.complete_fails = true;
        let (log, _) = script.run(2);
        assert_eq!(
            log,
            ["tab /p/a.cs (focus)", "complete", "rejected Connection interrupted", "tab /p/b.cs (focus)", "complete"]
        );
    }

    // launch › opens the pending project once
    #[test]
    fn opens_the_pending_project_once() {
        let script = Script::new(WindowState::default(), vec![Some(launch(Some("/p/launch"), &[])), None]);
        let (log, _) = script.run(2);
        assert_eq!(log, ["project /p/launch", "complete"]);
    }

    // launch › opens launched files in LightEdit without a project
    #[test]
    fn opens_launched_files_in_light_edit_without_a_project() {
        let (log, _) = Script::new(WindowState::default(), vec![Some(launch(None, &["/x/a.txt"]))]).run(1);
        assert_eq!(log, ["light /x/a.txt", "tab /x/a.txt (focus)", "complete"]);
    }

    // launch › opens launched files in the launched project
    #[test]
    fn opens_launched_files_in_the_launched_project() {
        let launches = vec![Some(launch(Some("/x/notes"), &["/x/notes/a.txt"]))];
        let (log, _) = Script::new(WindowState::default(), launches).run(1);
        assert_eq!(log, ["project /x/notes", "tab /x/notes/a.txt (focus)", "complete"]);
    }

    // launch › places the caret where the command line asked (openidea --line --column)
    #[test]
    fn places_the_caret_where_the_command_line_asked() {
        let launched = Launch {
            positions: at("/x/notes/a.txt", 119, 7),
            ..launch(Some("/x/notes"), &["/x/notes/a.txt", "/x/notes/b.txt"])
        };
        let (log, _) = Script::new(WindowState::default(), vec![Some(launched)]).run(1);
        assert_eq!(log[1..3], ["tab /x/notes/a.txt @119:7", "tab /x/notes/b.txt (focus)"]);
    }

    fn diff_launch(files: &[&str], before: bool) -> Launch {
        Launch {
            diff: Some(("/p/a.txt".to_owned(), "/p/b.txt".to_owned())),
            diff_before_files: before,
            ..launch(None, files)
        }
    }

    // launch › opens the CLI diff pair in LightEdit without a project (openidea diff a b)
    #[test]
    fn opens_the_cli_diff_pair_in_light_edit_without_a_project() {
        let (log, _) = Script::new(WindowState::default(), vec![Some(diff_launch(&[], false))]).run(1);
        assert_eq!(log, ["light /p/b.txt", "diff /p/a.txt /p/b.txt", "complete"]);
    }

    // launch › opens the CLI diff pair in the open project without LightEdit
    #[test]
    fn opens_the_cli_diff_pair_in_the_open_project_without_light_edit() {
        let (log, _) = Script::new(project("/p"), vec![Some(diff_launch(&[], false))]).run(1);
        assert_eq!(log, ["diff /p/a.txt /p/b.txt", "complete"]);
    }

    // launch › completes a failed CLI diff and handles the next file launch
    #[test]
    fn completes_a_failed_cli_diff_and_handles_the_next_file_launch() {
        let next = Launch { positions: at("/p/next.cs", 8, 2), ..launch(None, &["/p/next.cs"]) };
        let mut script = Script::new(project("/p"), vec![Some(diff_launch(&[], false)), Some(next)]);
        script.diff_fails = true;
        let (log, _) = script.run(2);
        assert_eq!(
            log,
            [
                "diff /p/a.txt /p/b.txt",
                "complete",
                "rejected Diff file unavailable",
                "tab /p/next.cs (focus) @8:2",
                "complete"
            ]
        );
    }

    // launch › applies a later source location after a pending diff
    #[test]
    fn applies_a_later_source_location_after_a_pending_diff() {
        let next = Launch { positions: at("/p/next.cs", 8, 2), ..launch(None, &["/p/next.cs"]) };
        let (log, _) = Script::new(project("/p"), vec![Some(diff_launch(&[], false)), Some(next)]).run(2);
        assert_eq!(log, ["diff /p/a.txt /p/b.txt", "complete", "tab /p/next.cs (focus) @8:2", "complete"]);
    }

    // launch › preserves the last request's focus for $project (diff first: $diffBeforeFiles)
    #[test]
    fn preserves_the_last_requests_focus() {
        for (root, before) in [(None, false), (Some("/p"), false), (None, true), (Some("/p"), true)] {
            let launched = Launch {
                project: root.map(str::to_owned),
                positions: at("/p/a.cs", 8, 2),
                diff: Some(("/p/left.txt".to_owned(), "/p/right.txt".to_owned())),
                diff_before_files: before,
                ..launch(None, &["/p/a.cs"])
            };
            let (log, _) = Script::new(project("/p"), vec![Some(launched)]).run(1);
            let (tab, diff) = ("tab /p/a.cs (focus) @8:2", "diff /p/left.txt /p/right.txt");
            let expected = if before { [diff, tab, "complete"] } else { [tab, diff, "complete"] };
            assert_eq!(log, expected, "{root:?} {before}");
        }
    }

    // launch › opens later files when their preceding diff fails
    #[test]
    fn opens_later_files_when_their_preceding_diff_fails() {
        let launched = Launch { positions: at("/p/next.cs", 8, 2), ..diff_launch(&["/p/next.cs"], true) };
        let mut script = Script::new(project("/p"), vec![Some(launched)]);
        script.diff_fails = true;
        let (log, _) = script.run(1);
        assert_eq!(
            log,
            ["diff /p/a.txt /p/b.txt", "notify Diff file unavailable", "tab /p/next.cs (focus) @8:2", "complete"]
        );
    }

    // launch › does not open a diff when its project launch is cancelled (diff first: %s)
    #[test]
    fn does_not_open_a_diff_when_its_project_launch_is_cancelled() {
        for before in [false, true] {
            let launched = Launch {
                project: Some("/p".to_owned()),
                diff: Some(("/p/left.txt".to_owned(), "/p/right.txt".to_owned())),
                diff_before_files: before,
                ..launch(None, &["/p/a.cs"])
            };
            let mut script = Script::new(WindowState::default(), vec![Some(launched)]);
            script.failing = Some(("/p".to_owned(), IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")));
            let (log, _) = script.run(1);
            assert_eq!(log, ["project /p", "complete"]);
        }
    }

    fn workspace(root: &str, name: &str, members: &[&str]) -> WindowState {
        WindowState {
            workspace: Some(WorkspaceMembers { name: name.to_owned(), projects: s(members) }),
            ..project(root)
        }
    }

    // Attach › attaches in place without reopening or asking to leave
    #[test]
    fn attaches_in_place_without_reopening_or_asking_to_leave() {
        let mut i18n = I18n::default();
        let (mut flow, step) = OpenFlow::with_policy("/p/b", &s(&["/p/b/file.txt"]), &project("/p/a"), &i18n);
        let OpenStep::Ask(prompt) = step else { panic!("asks where") };
        assert!(prompt.buttons.iter().any(|button| button.id == "attach"));
        assert_eq!(flow.answered(&Answer::new("attach", false)), OpenStep::ConfirmTrust { path: "/p/b".into() });
        assert_eq!(flow.trust_confirmed(Ok(()), &i18n), OpenStep::ReadTrust { path: "/p/b".into() });
        assert_eq!(flow.trust_read(Ok(true), &mut i18n), OpenStep::Attach { path: "/p/b".into() });
        assert_eq!(flow.attached(Ok(()), &i18n), OpenStep::ReadCurrentProject);
        assert_eq!(flow.project_read(), OpenStep::RevealProject { path: "/p/b".into() });
        assert_eq!(open_tabs(&flow.revealed()), ["/p/b/file.txt (focus)"]);
        assert_eq!(flow.files_opened(), OpenStep::Done);
    }

    fn attaching(i18n: &I18n) -> OpenFlow {
        let (mut flow, _) = OpenFlow::with_policy("/p/b", &[], &project("/p/a"), i18n);
        flow.answered(&Answer::new("attach", false));
        flow
    }

    // Attach › does not attach when trust is cancelled
    #[test]
    fn does_not_attach_when_trust_is_cancelled() {
        let i18n = I18n::default();
        let mut flow = attaching(&i18n);
        let step = flow.trust_confirmed(Err(IdeError::new(ErrorKind::Cancelled, "Cancelled")), &i18n);
        assert_eq!(step, OpenStep::Failed { cancel_close: false, notice: None });
    }

    // Attach › does not attach a folder previewed in restricted mode
    #[test]
    fn does_not_attach_a_folder_previewed_in_restricted_mode() {
        let mut i18n = I18n::default();
        let mut flow = attaching(&i18n);
        flow.trust_confirmed(Ok(()), &i18n);
        assert!(matches!(flow.trust_read(Ok(false), &mut i18n), OpenStep::Failed { .. }));
    }

    // Attach › explains how to attach a folder already decided as restricted
    #[test]
    fn explains_how_to_attach_a_folder_already_decided_as_restricted() {
        let mut i18n = I18n::default();
        let mut flow = attaching(&i18n);
        flow.trust_confirmed(Ok(()), &i18n);
        let OpenStep::Failed { notice: Some(notice), .. } = flow.trust_read(Ok(false), &mut i18n) else {
            panic!("explains")
        };
        assert_eq!(notice.level, NotificationLevel::Warning);
        assert_eq!(
            notice.body.as_deref(),
            Some(
                "Cannot attach /p/b because it is restricted. Open it separately and choose Trust Project, then attach \
                 it again."
            )
        );
    }

    // Attach › refreshes an existing workspace without reopening its editor session
    #[test]
    fn refreshes_an_existing_workspace_without_reopening_its_editor_session() {
        let i18n = I18n::default();
        let window = workspace("/workspace", "A", &["/p/a"]);
        let (mut flow, step) = OpenFlow::with_policy("/p/b", &[], &window, &i18n);
        assert!(matches!(step, OpenStep::Ask(_)));
        flow.answered(&Answer::new("attach", false));
        flow.trust_confirmed(Ok(()), &i18n);
        flow.trust_read(Ok(true), &mut I18n::default());
        assert_eq!(flow.attached(Ok(()), &i18n), OpenStep::ReadCurrentProject);
        assert!(!flow.opened_here());
    }

    // removes the first workspace member while retaining the workspace state root
    #[test]
    fn removes_the_first_workspace_member_while_retaining_the_workspace_state_root() {
        let window = workspace("/workspace", "A", &["/a", "/b"]);
        let (_, step) = RemoveFromWorkspace::start("/a", &window, &I18n::default());
        assert_eq!(step, RemoveStep::Update { name: "A".into(), projects: s(&["/b"]) });
    }

    // confirms detaching an attachment and never removes the primary
    #[test]
    fn confirms_detaching_an_attachment_and_never_removes_the_primary() {
        let i18n = I18n::default();
        let window = workspace("/a", "A", &["/a", "/b"]);
        assert_eq!(RemoveFromWorkspace::start("/a", &window, &i18n).1, RemoveStep::Done);
        let (mut remove, step) = RemoveFromWorkspace::start("/b", &window, &i18n);
        assert!(matches!(step, RemoveStep::Ask(ref prompt) if prompt.title == "Detach Project"));
        assert_eq!(remove.answered(&Answer::new("cancel", false)), RemoveStep::Done);
        let (mut remove, _) = RemoveFromWorkspace::start("/b", &window, &i18n);
        assert_eq!(
            remove.answered(&Answer::new("detach", false)),
            RemoveStep::Update { name: "A".into(), projects: s(&["/a"]) }
        );
        let when = WINDOW_ACTIONS.iter().find(|(id, _)| *id == "project.detach").map(|(_, when)| *when);
        assert_eq!(when, Some(Some("projectAttached")));
    }

    // explicit project-window requests › honours current-window requests even when a project is already open
    #[test]
    fn honours_current_window_requests_even_when_a_project_is_already_open() {
        let i18n = I18n::default();
        let (mut flow, step) = OpenFlow::explicit("/p/new", false, &project("/p/old"));
        assert_eq!(step, request("/p/new", false, &[]));
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &i18n), OpenStep::ConfirmLeave);
        flow.left(true);
        assert_eq!(flow.wsl_resolved(None), OpenStep::OpenProject { path: "/p/new".into() });
    }

    // explicit project-window requests › opens a new window without replacing the current project
    #[test]
    fn opens_a_new_window_without_replacing_the_current_project() {
        let (mut flow, step) = OpenFlow::explicit("/p/new", true, &project("/p/old"));
        assert_eq!(step, request("/p/new", true, &[]));
        let opened = OpenOutcome::Opened { label: "project-1".into() };
        assert_eq!(flow.window_answered(Ok(opened), &I18n::default()), OpenStep::Done);
    }

    // explicit project-window requests › keeps the ordinary unsaved-change guard for current-window opens
    #[test]
    fn keeps_the_ordinary_unsaved_change_guard_for_current_window_opens() {
        let (mut flow, _) = OpenFlow::explicit("/p/new", false, &project("/p/old"));
        assert_eq!(flow.window_answered(Ok(OpenOutcome::Current), &I18n::default()), OpenStep::ConfirmLeave);
        assert_eq!(flow.left(false), OpenStep::Done);
        assert!(!flow.opened_here());
    }

    // LightEdit window › closes when its last editor tab closes
    #[test]
    fn closes_when_its_last_editor_tab_closes() {
        assert!(!light_edit_should_close(true, true, 2));
        assert!(!light_edit_should_close(true, true, 1));
        assert!(light_edit_should_close(false, true, 0));
    }

    // LightEdit window › stays open in a project window or while leaving LightEdit
    #[test]
    fn stays_open_in_a_project_window_or_while_leaving_light_edit() {
        assert!(!light_edit_should_close(false, false, 0));
    }

    // Not from a spec: Close Other / Left / Right Project Tabs keep the given (or current) tab.
    #[test]
    fn closes_project_tabs_around_the_kept_one() {
        let tabs: Vec<(String, bool)> =
            [("main", false), ("project-1", true), ("project-2", false)].map(|(l, c)| (l.to_owned(), c)).to_vec();
        assert_eq!(tabs_to_close(&tabs, None, TabSide::Left), Some(("project-1".into(), s(&["main"]))));
        assert_eq!(
            tabs_to_close(&tabs, Some("main"), TabSide::Others),
            Some(("main".into(), s(&["project-1", "project-2"])))
        );
        assert_eq!(tabs_to_close(&tabs, Some("project-2"), TabSide::Right), None);
        assert_eq!(tabs_to_close(&tabs, Some("gone"), TabSide::Others), None);
    }
}
