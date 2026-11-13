//! The Build tool window's model: one build at a time, its console lines, the build → file → message
//! tree with the first error, the finish summaries, and what the workbench must do as the build
//! starts, streams and ends ([`BuildEffect`]).
//!
//! Nothing here runs a build. The workbench starts one with [`BuildRun::start`], sends the backend's
//! `build_run` with the request and the files to clear it returns, feeds the streamed events to
//! [`BuildRun::apply`] and the outcome to [`BuildRun::finish`], [`BuildRun::fail`] or
//! [`BuildRun::cancelled`], and carries out the effects each returns.

use crate::platform::collate;
use ide_build::compile::{BuildEvent, BuildKind, BuildMessage, BuildResult, BuildSystem};
use ide_core::{RequestId, Severity};
use std::collections::VecDeque;

/// Oldest lines are dropped beyond this many.
pub const MAX_CONSOLE_LINES: usize = 20_000;
/// The key of the build's own row, above its files.
pub const ROOT_KEY: &str = "build:root";
/// The context key that is true while a build runs.
pub const BUILD_RUNNING: &str = "build.running";
/// The context key of the last build's outcome (`success`, `failed`, `cancelled`).
pub const BUILD_LAST_STATUS: &str = "build.lastStatus";
/// The id of the build's status bar task.
pub const BUILD_TASK_ID: &str = "build:compile";
/// The id of the Build tool window.
pub const BUILD_TOOL_WINDOW: &str = "build";
/// Run configuration kinds a project build precedes; scripts have nothing to compile.
const COMPILED_KINDS: [&str; 5] = ["", "spring-boot", "application", "maven", "gradle"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

impl BuildStatus {
    /// The value of [`BUILD_LAST_STATUS`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Which stream printed a console line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Out,
    Err,
    /// A command the build ran, echoed.
    System,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleLine {
    pub text: String,
    pub kind: LineKind,
    /// The directory of the step that printed the line: its file links resolve against it.
    pub cwd: Option<String>,
}

/// Appends streamed `text` (lines each ending in `\n`) to `lines`, dropping the oldest beyond
/// [`MAX_CONSOLE_LINES`].
pub fn append_output(lines: &mut VecDeque<ConsoleLine>, text: &str, kind: LineKind, cwd: Option<&str>) {
    let text = text.strip_suffix('\n').unwrap_or(text);
    for part in text.split('\n') {
        let line = part.strip_suffix('\r').unwrap_or(part);
        lines.push_back(ConsoleLine { text: line.to_owned(), kind, cwd: cwd.map(str::to_owned) });
    }
    let excess = lines.len().saturating_sub(MAX_CONSOLE_LINES);
    lines.drain(..excess);
}

/// A row of the build tree.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildNode {
    Root { title: String, status: BuildStatus, errors: usize, warnings: usize },
    File { key: String, path: String, messages: Vec<BuildMessage> },
    Message { key: String, message: BuildMessage },
}

impl BuildNode {
    pub fn key(&self) -> &str {
        match self {
            Self::Root { .. } => ROOT_KEY,
            Self::File { key, .. } | Self::Message { key, .. } => key,
        }
    }
}

/// The path of a message as text; `None` for a message about the build itself.
pub fn message_path(message: &BuildMessage) -> Option<String> {
    message.path.as_ref().map(|path| path.0.to_string_lossy().into_owned())
}

fn rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

fn count(messages: &[BuildMessage], severity: Severity) -> usize {
    messages.iter().filter(|message| message.severity == severity).count()
}

/// The build's own row.
pub fn root_node(title: &str, status: BuildStatus, messages: &[BuildMessage]) -> BuildNode {
    BuildNode::Root {
        title: title.to_owned(),
        status,
        errors: count(messages, Severity::Error),
        warnings: count(messages, Severity::Warning),
    }
}

/// The rows under the build: the files with messages (those with errors first, then by path), each
/// with its messages by position, then the messages about the build itself.
pub fn root_children(messages: &[BuildMessage]) -> Vec<BuildNode> {
    let mut files: Vec<(String, Vec<BuildMessage>)> = Vec::new();
    let mut general = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        match message_path(message) {
            Some(path) => match files.iter_mut().find(|(known, _)| *known == path) {
                Some((_, list)) => list.push(message.clone()),
                None => files.push((path, vec![message.clone()])),
            },
            None => general.push(BuildNode::Message { key: format!("build:msg:{index}"), message: message.clone() }),
        }
    }
    let start = |message: &BuildMessage| message.range.map(|range| (range.start.line, range.start.col));
    for (_, list) in &mut files {
        list.sort_by(|a, b| {
            let (a_at, b_at) = (start(a).unwrap_or_default(), start(b).unwrap_or_default());
            a_at.cmp(&b_at).then(rank(a.severity).cmp(&rank(b.severity)))
        });
    }
    let worst = |list: &[BuildMessage]| list.iter().map(|message| rank(message.severity)).min().unwrap_or(0);
    files.sort_by(|(a_path, a), (b_path, b)| worst(a).cmp(&worst(b)).then_with(|| collate::compare(a_path, b_path)));
    files
        .into_iter()
        .map(|(path, messages)| BuildNode::File { key: format!("build:file:{path}"), path, messages })
        .chain(general)
        .collect()
}

/// The message rows of a file row.
pub fn file_children(key: &str, messages: &[BuildMessage]) -> Vec<BuildNode> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| BuildNode::Message { key: format!("{key}\u{0}{index}"), message: message.clone() })
        .collect()
}

/// The node with `key` in the tree of `messages`, the build's own row excluded.
pub fn find_node(messages: &[BuildMessage], key: &str) -> Option<BuildNode> {
    for child in root_children(messages) {
        if child.key() == key {
            return Some(child);
        }
        if let BuildNode::File { key: file, messages, .. } = &child
            && let Some(hit) = file_children(file, messages).into_iter().find(|node| node.key() == key)
        {
            return Some(hit);
        }
    }
    None
}

/// The first error in tree order: what a failed build selects and opens.
pub fn first_error(messages: &[BuildMessage]) -> Option<BuildNode> {
    root_children(messages).into_iter().find_map(|child| {
        let leaves = match &child {
            BuildNode::File { key, messages, .. } => file_children(key, messages),
            _ => vec![child.clone()],
        };
        leaves
            .into_iter()
            .find(|node| matches!(node, BuildNode::Message { message, .. } if message.severity == Severity::Error))
    })
}

/// `450 ms`, `3 s 120 ms`, `1 m 5 s`.
pub fn format_duration(ms: f64) -> String {
    let total = if ms.is_finite() { ms.max(0.0).round() as u64 } else { 0 };
    if total < 1000 {
        return format!("{total} ms");
    }
    let (minutes, seconds, rest) = (total / 60_000, (total % 60_000) / 1000, total % 1000);
    match (minutes, seconds, rest) {
        (0, seconds, 0) => format!("{seconds} s"),
        (0, seconds, rest) => format!("{seconds} s {rest} ms"),
        (minutes, 0, _) => format!("{minutes} m"),
        (minutes, seconds, _) => format!("{minutes} m {seconds} s"),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The notification title (and the build row's text) when a build finishes.
pub fn finish_summary(success: bool, errors: usize, warnings: usize, duration_ms: f64) -> String {
    let duration = format_duration(duration_ms);
    match (success, errors, warnings) {
        (true, _, 0) => format!("Build completed successfully in {duration}"),
        (true, _, warnings) => {
            format!("Build completed successfully with {} in {duration}", plural(warnings, "warning"))
        }
        (false, 0, _) => "Build failed".to_owned(),
        (false, errors, 0) => format!("Build failed: {}", plural(errors, "error")),
        (false, errors, warnings) => {
            format!("Build failed: {}, {}", plural(errors, "error"), plural(warnings, "warning"))
        }
    }
}

/// What the build row says after the title.
pub fn root_detail(status: BuildStatus, duration_ms: Option<f64>, errors: usize, warnings: usize) -> String {
    match status {
        BuildStatus::Running => "running…".to_owned(),
        BuildStatus::Cancelled => "cancelled".to_owned(),
        BuildStatus::Success => format!("finished {}", format_duration(duration_ms.unwrap_or(0.0))),
        BuildStatus::Failed => {
            let summary = finish_summary(false, errors, warnings, duration_ms.unwrap_or(0.0));
            let rest = summary.strip_prefix("Build failed").unwrap_or(&summary);
            let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
            if rest.is_empty() { "failed".to_owned() } else { rest.to_owned() }
        }
    }
}

/// `2 errors, 1 warning` beside a file; empty without either.
pub fn file_counts(messages: &[BuildMessage]) -> String {
    let (errors, warnings) = (count(messages, Severity::Error), count(messages, Severity::Warning));
    let parts = [(errors, "error"), (warnings, "warning")];
    parts.into_iter().filter(|(n, _)| *n > 0).map(|(n, word)| plural(n, word)).collect::<Vec<_>>().join(", ")
}

/// The files that got build diagnostics, in the order first seen: they are cleared before the next
/// build.
pub fn message_files(messages: &[BuildMessage]) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    for message in messages.iter().filter(|message| message.range.is_some()) {
        if let Some(path) = message_path(message)
            && !files.contains(&path)
        {
            files.push(path);
        }
    }
    files
}

/// Whether the main toolbar shows the Build Project hammer: for a CMake target, or for a Maven or
/// Gradle project whose selected run configuration is compiled.
pub fn title_item_shown(cmake_build: bool, maven: bool, gradle: bool, selected_kind: &str) -> bool {
    cmake_build || ((maven || gradle) && COMPILED_KINDS.contains(&selected_kind))
}

/// How loud a notification is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyLevel {
    Info,
    Warning,
    Error,
}

/// What the workbench must do for the build.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildEffect {
    /// Show the Build tool window without giving it the keyboard.
    ShowToolWindow,
    /// Set [`BUILD_RUNNING`] and [`BUILD_LAST_STATUS`] (`None` clears it).
    Context { running: bool, last_status: Option<BuildStatus> },
    /// Show or update the status bar task [`BUILD_TASK_ID`]; `cancellable` while it runs.
    Task { title: String, message: Option<String>, done: bool },
    /// Notify (group `run`, key `build-finished`); `show_build` adds a Show Build action.
    Notify { level: NotifyLevel, title: String, body: Option<String>, show_build: bool },
    /// Open the location of a message, the editor getting the keyboard when `focus`.
    Navigate { path: String, line: Option<u32>, col: Option<u32>, focus: bool },
    /// Cancel the backend request.
    Cancel(RequestId),
}

/// What [`BuildRun::start`] hands the workbench.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildStart {
    /// The files whose build diagnostics the backend clears first.
    pub clear: Vec<String>,
    pub effects: Vec<BuildEffect>,
}

/// The current build, or the last one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildRun {
    pub request: Option<RequestId>,
    pub kind: Option<BuildKind>,
    /// The module file or folder of a Build Module; `None` for the project.
    pub file: Option<String>,
    pub title: String,
    pub system: Option<BuildSystem>,
    /// `None` before the first build.
    pub status: Option<BuildStatus>,
    /// The running build's progress message.
    pub message: Option<String>,
    pub duration_ms: Option<f64>,
    pub messages: Vec<BuildMessage>,
    pub lines: VecDeque<ConsoleLine>,
    /// The files with diagnostics from the last build.
    pub files: Vec<String>,
    /// The key of the selected row: a message row shows its text beside the tree.
    pub selected: Option<String>,
    /// The directory of the running step.
    pub step_cwd: Option<String>,
}

/// The title of a build of `kind`.
pub fn kind_title(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Build => "Build",
        BuildKind::Rebuild => "Rebuild",
        BuildKind::Artifacts => "Build Artifacts",
    }
}

impl BuildRun {
    pub fn is_running(&self) -> bool {
        self.status == Some(BuildStatus::Running)
    }

    /// The failed build's error count: the tool window's badge.
    pub fn error_badge(&self) -> Option<usize> {
        (self.status == Some(BuildStatus::Failed))
            .then(|| count(&self.messages, Severity::Error))
            .filter(|errors| *errors > 0)
    }

    /// What Rerun builds: the last build's kind and module.
    pub fn rerun_target(&self) -> (BuildKind, Option<String>) {
        (self.kind.unwrap_or(BuildKind::Build), self.file.clone())
    }

    /// The build's row and the tree under it; nothing before the first build.
    pub fn root(&self) -> Option<BuildNode> {
        self.status.map(|status| root_node(&self.title, status, &self.messages))
    }

    /// Starts a build of `kind`, of the module of `file` or of the project, as `request`. A build
    /// already running, or no project, starts nothing. `activate` shows the tool window.
    pub fn start(
        &mut self,
        kind: BuildKind,
        file: Option<String>,
        activate: bool,
        request: RequestId,
        project_open: bool,
    ) -> Option<BuildStart> {
        if self.is_running() || !project_open {
            return None;
        }
        let clear = std::mem::take(&mut self.files);
        let title = kind_title(kind).to_owned();
        *self = Self {
            request: Some(request),
            kind: Some(kind),
            file,
            title: title.clone(),
            status: Some(BuildStatus::Running),
            files: clear.clone(),
            ..Self::default()
        };
        let mut effects = vec![BuildEffect::Context { running: true, last_status: None }];
        if activate {
            effects.push(BuildEffect::ShowToolWindow);
        }
        effects.push(BuildEffect::Task { title, message: None, done: false });
        Some(BuildStart { clear, effects })
    }

    /// One streamed event of the build `request`; events of an older build are dropped. A progress
    /// message updates the status bar task.
    pub fn apply(&mut self, request: RequestId, event: BuildEvent) -> Vec<BuildEffect> {
        if self.request != Some(request) {
            return Vec::new();
        }
        match event {
            BuildEvent::Started { system, title } => {
                self.title = title;
                self.system = Some(system);
                self.step_cwd = None;
            }
            BuildEvent::Step { command, cwd } => {
                let cwd = cwd.0.to_string_lossy().into_owned();
                append_output(&mut self.lines, &format!("{command}\n"), LineKind::System, Some(&cwd));
                self.step_cwd = Some(cwd);
            }
            BuildEvent::Output { text, stderr } => {
                let kind = if stderr { LineKind::Err } else { LineKind::Out };
                append_output(&mut self.lines, &text, kind, self.step_cwd.as_deref());
            }
            BuildEvent::Message { message } => self.messages.push(message),
            BuildEvent::Progress { message, .. } => {
                self.message = Some(message.clone());
                let title = self.kind.map_or("Build", kind_title).to_owned();
                return vec![BuildEffect::Task { title, message: Some(message), done: false }];
            }
        }
        Vec::new()
    }

    /// The build `request` ended with `result`, which replaces the streamed messages. Tells the
    /// outcome; a failure shows the tool window, selects the first error and opens it.
    pub fn finish(&mut self, request: RequestId, result: BuildResult) -> Vec<BuildEffect> {
        if self.request != Some(request) {
            return Vec::new();
        }
        let status = if result.success { BuildStatus::Success } else { BuildStatus::Failed };
        self.status = Some(status);
        self.title = result.title;
        self.system = Some(result.system);
        self.files = message_files(&result.messages);
        self.messages = result.messages;
        self.duration_ms = Some(result.duration_ms);
        self.message = None;
        let (errors, warnings) = (result.errors as usize, result.warnings as usize);
        let level = match (result.success, warnings) {
            (false, _) => NotifyLevel::Error,
            (true, 0) => NotifyLevel::Info,
            (true, _) => NotifyLevel::Warning,
        };
        let mut effects = vec![BuildEffect::Notify {
            level,
            title: finish_summary(result.success, errors, warnings, result.duration_ms),
            body: None,
            show_build: !(result.success && warnings == 0),
        }];
        if !result.success {
            effects.extend(self.show_failure());
        }
        effects.extend(self.end(status));
        effects
    }

    /// The build `request` could not run: the error is printed and listed as a message.
    pub fn fail(&mut self, request: RequestId, error: &str) -> Vec<BuildEffect> {
        if self.request != Some(request) {
            return Vec::new();
        }
        self.status = Some(BuildStatus::Failed);
        self.files.clear();
        self.message = None;
        append_output(&mut self.lines, &format!("{error}\n"), LineKind::Err, None);
        self.messages.push(BuildMessage {
            severity: Severity::Error,
            text: error.to_owned(),
            path: None,
            range: None,
            code: None,
        });
        let mut effects = vec![BuildEffect::Notify {
            level: NotifyLevel::Error,
            title: "Build failed".to_owned(),
            body: Some(error.to_owned()),
            show_build: false,
        }];
        effects.extend(self.show_failure());
        effects.extend(self.end(BuildStatus::Failed));
        effects
    }

    /// The build `request` was cancelled: nothing is notified.
    pub fn cancelled(&mut self, request: RequestId) -> Vec<BuildEffect> {
        if self.request != Some(request) {
            return Vec::new();
        }
        self.status = Some(BuildStatus::Cancelled);
        self.files.clear();
        self.message = None;
        self.end(BuildStatus::Cancelled)
    }

    /// Stop: cancels the running build's request.
    pub fn stop(&self) -> Option<BuildEffect> {
        self.request.filter(|_| self.is_running()).map(BuildEffect::Cancel)
    }

    /// Another project opened: the build stops and the window is empty again.
    pub fn reset(&mut self) -> Vec<BuildEffect> {
        let mut effects: Vec<BuildEffect> = self.stop().into_iter().collect();
        *self = Self::default();
        effects.push(BuildEffect::Context { running: false, last_status: None });
        effects
    }

    /// Selects the row `key` (a message row shows its text beside the tree).
    pub fn select(&mut self, key: Option<String>) {
        self.selected = key;
    }

    /// The selected node, when it is in the tree.
    pub fn selected_node(&self) -> Option<BuildNode> {
        find_node(&self.messages, self.selected.as_deref()?)
    }

    /// A failure shows the tool window, selects the first error and opens its location.
    fn show_failure(&mut self) -> Vec<BuildEffect> {
        let mut effects = vec![BuildEffect::ShowToolWindow];
        if let Some(BuildNode::Message { key, message }) = first_error(&self.messages) {
            self.selected = Some(key);
            effects.extend(navigate_to(&message, true));
        }
        effects
    }

    /// The build is over: no request, the context keys and the finished task.
    fn end(&mut self, status: BuildStatus) -> Vec<BuildEffect> {
        self.request = None;
        let title = self.kind.map_or("Build", kind_title).to_owned();
        vec![
            BuildEffect::Context { running: false, last_status: Some(status) },
            BuildEffect::Task { title, message: None, done: true },
        ]
    }
}

/// Opening the source of a message: nothing for a message about the build itself.
pub fn navigate_to(message: &BuildMessage, focus: bool) -> Option<BuildEffect> {
    let path = message_path(message)?;
    let start = message.range.map(|range| range.start);
    Some(BuildEffect::Navigate { path, line: start.map(|at| at.line), col: start.map(|at| at.col), focus })
}

/// The first message of a node: a message itself or a file's first; the build row has none.
pub fn first_located(node: &BuildNode) -> Option<&BuildMessage> {
    match node {
        BuildNode::Message { message, .. } => Some(message),
        BuildNode::File { messages, .. } => messages.first(),
        BuildNode::Root { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{IpcPath, Pos, Range};
    use std::path::PathBuf;

    fn err(path: Option<&str>, line: u32, text: &str) -> BuildMessage {
        BuildMessage {
            severity: Severity::Error,
            text: text.to_owned(),
            path: path.map(|path| IpcPath(PathBuf::from(path))),
            range: path.map(|_| Range::new(Pos::new(line, 4), Pos::new(line, 4))),
            code: None,
        }
    }

    fn warn(path: &str, line: u32) -> BuildMessage {
        BuildMessage { severity: Severity::Warning, ..err(Some(path), line, "deprecated") }
    }

    fn result(messages: Vec<BuildMessage>, success: bool) -> BuildResult {
        BuildResult {
            system: BuildSystem::Maven,
            title: "demo".to_owned(),
            success,
            errors: count(&messages, Severity::Error) as u32,
            warnings: count(&messages, Severity::Warning) as u32,
            duration_ms: 3000.0,
            messages,
        }
    }

    fn kinds(children: &[BuildNode]) -> Vec<String> {
        children
            .iter()
            .map(|child| match child {
                BuildNode::File { path, .. } => path.clone(),
                BuildNode::Message { .. } => "message".to_owned(),
                BuildNode::Root { .. } => "root".to_owned(),
            })
            .collect()
    }

    fn shows(effects: &[BuildEffect]) -> usize {
        effects.iter().filter(|effect| **effect == BuildEffect::ShowToolWindow).count()
    }

    fn navigations(effects: &[BuildEffect]) -> Vec<&BuildEffect> {
        effects.iter().filter(|effect| matches!(effect, BuildEffect::Navigate { .. })).collect()
    }

    fn notifications(effects: &[BuildEffect]) -> Vec<(NotifyLevel, String)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                BuildEffect::Notify { level, title, .. } => Some((*level, title.clone())),
                _ => None,
            })
            .collect()
    }

    const REQUEST: RequestId = RequestId(1);

    #[test]
    fn groups_messages_by_file_errors_first_build_messages_last() {
        let messages = vec![
            warn("/p/B.java", 3),
            err(None, 0, "BUILD FAILURE"),
            err(Some("/p/A.java"), 9, "cannot find symbol"),
            warn("/p/A.java", 2),
        ];
        let children = root_children(&messages);
        assert_eq!(kinds(&children), ["/p/A.java", "/p/B.java", "message"]);
        let BuildNode::File { messages: in_a, .. } = &children[0] else { panic!("a file") };
        let lines: Vec<Option<u32>> = in_a.iter().map(|m| m.range.map(|range| range.start.line)).collect();
        assert_eq!(lines, [Some(2), Some(9)]);
        assert_eq!(
            root_node("demo", BuildStatus::Failed, &messages),
            BuildNode::Root { title: "demo".to_owned(), status: BuildStatus::Failed, errors: 2, warnings: 2 }
        );
        assert_eq!(message_files(&messages), ["/p/B.java", "/p/A.java"]);
        let first = first_error(&messages).expect("an error");
        let BuildNode::Message { message, .. } = &first else { panic!("a message") };
        assert_eq!(message.range.map(|range| range.start.line), Some(9));
        assert_eq!(find_node(&messages, first.key()), Some(first.clone()));
        assert_eq!(first_error(&[warn("/p/B.java", 3)]), None);
        assert_eq!(file_counts(in_a), "1 error, 1 warning");
    }

    #[test]
    fn formats_ideas_finish_summaries() {
        assert_eq!(format_duration(450.0), "450 ms");
        assert_eq!(format_duration(3000.0), "3 s");
        assert_eq!(format_duration(3120.0), "3 s 120 ms");
        assert_eq!(format_duration(65_000.0), "1 m 5 s");
        assert_eq!(finish_summary(true, 0, 0, 3000.0), "Build completed successfully in 3 s");
        assert_eq!(finish_summary(true, 0, 1, 500.0), "Build completed successfully with 1 warning in 500 ms");
        assert_eq!(finish_summary(false, 2, 0, 1.0), "Build failed: 2 errors");
        assert_eq!(finish_summary(false, 1, 2, 1.0), "Build failed: 1 error, 2 warnings");
        assert_eq!(finish_summary(false, 0, 0, 1.0), "Build failed");
        assert_eq!(root_detail(BuildStatus::Failed, Some(1.0), 1, 2), "1 error, 2 warnings");
        assert_eq!(root_detail(BuildStatus::Failed, None, 0, 0), "failed");
        assert_eq!(root_detail(BuildStatus::Success, Some(3000.0), 0, 0), "finished 3 s");
    }

    #[test]
    fn splits_console_output_into_capped_lines() {
        let mut lines = VecDeque::new();
        append_output(&mut lines, "a\nb\r\n", LineKind::Out, None);
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["a", "b"]);
        let mut many = VecDeque::new();
        append_output(&mut many, &"x\n".repeat(MAX_CONSOLE_LINES + 5), LineKind::Out, None);
        assert_eq!(many.len(), MAX_CONSOLE_LINES);
    }

    #[test]
    fn streams_a_failing_build_into_the_tree_and_console_then_notifies_and_clears_next_time() {
        let mut run = BuildRun::default();
        let start = run.start(BuildKind::Build, None, true, REQUEST, true).expect("starts");
        assert_eq!(start.clear, Vec::<String>::new());
        assert!(start.effects.contains(&BuildEffect::Context { running: true, last_status: None }));
        assert_eq!(shows(&start.effects), 1);
        assert!(run.is_running());
        assert!(run.start(BuildKind::Build, None, true, RequestId(9), true).is_none(), "one build at a time");
        let messages = vec![err(Some("/p/src/A.java"), 11, "cannot find symbol"), warn("/p/src/B.java", 2)];
        run.apply(REQUEST, BuildEvent::Started { system: BuildSystem::Maven, title: "demo".to_owned() });
        run.apply(REQUEST, BuildEvent::Step { command: "mvnw -B test-compile".to_owned(), cwd: IpcPath("/p".into()) });
        run.apply(
            REQUEST,
            BuildEvent::Output { text: "[ERROR] /p/src/A.java:[12,5] cannot find symbol\n".to_owned(), stderr: false },
        );
        run.apply(REQUEST, BuildEvent::Message { message: messages[0].clone() });
        run.apply(RequestId(9), BuildEvent::Message { message: messages[1].clone() });
        let kinds: Vec<LineKind> = run.lines.iter().map(|line| line.kind).collect();
        assert_eq!(kinds, [LineKind::System, LineKind::Out]);
        assert_eq!(run.lines[1].cwd.as_deref(), Some("/p"), "a line resolves against its step's directory");
        assert_eq!(run.title, "demo");
        assert_eq!(run.messages.len(), 1, "an older build's event is dropped");

        let effects = run.finish(REQUEST, result(messages.clone(), false));
        assert_eq!(run.status, Some(BuildStatus::Failed));
        assert_eq!(notifications(&effects), [(NotifyLevel::Error, "Build failed: 1 error, 1 warning".to_owned())]);
        assert_eq!(shows(&effects), 1, "a failure shows the tool window");
        assert_eq!(run.selected.as_deref(), first_error(&messages).as_ref().map(BuildNode::key));
        assert_eq!(
            navigations(&effects),
            [&BuildEffect::Navigate { path: "/p/src/A.java".to_owned(), line: Some(11), col: Some(4), focus: true }]
        );
        assert!(effects.contains(&BuildEffect::Context { running: false, last_status: Some(BuildStatus::Failed) }));
        assert_eq!(run.error_badge(), Some(1));

        let again = run.start(BuildKind::Rebuild, None, true, RequestId(2), true).expect("starts");
        assert_eq!(again.clear, ["/p/src/A.java", "/p/src/B.java"]);
        let effects = run.finish(RequestId(2), result(Vec::new(), true));
        assert_eq!(notifications(&effects), [(NotifyLevel::Info, "Build completed successfully in 3 s".to_owned())]);
        assert!(effects.iter().any(|effect| matches!(effect, BuildEffect::Notify { show_build: false, .. })));
    }

    #[test]
    fn opens_the_build_window_without_focus_when_a_build_starts() {
        let module = Some("/p/core/src/A.java");
        let builds = [(BuildKind::Build, None), (BuildKind::Build, module), (BuildKind::Rebuild, None)];
        for (kind, file) in builds {
            let mut run = BuildRun::default();
            let start = run.start(kind, file.map(str::to_owned), true, REQUEST, true).expect("starts");
            assert_eq!(run.status, Some(BuildStatus::Running));
            assert_eq!(shows(&start.effects), 1);
            let effects = run.finish(REQUEST, result(Vec::new(), true));
            assert_eq!(shows(&effects), 0);
            assert!(navigations(&effects).is_empty());
        }
    }

    #[test]
    fn keeps_before_launch_builds_hidden_until_a_failure() {
        let mut run = BuildRun::default();
        let start = run.start(BuildKind::Build, None, false, REQUEST, true).expect("starts");
        assert_eq!(shows(&start.effects), 0);
        assert_eq!(shows(&run.finish(REQUEST, result(Vec::new(), true))), 0);
        let messages = vec![err(Some("/p/src/A.java"), 11, "cannot find symbol")];
        let start = run.start(BuildKind::Build, None, false, RequestId(2), true).expect("starts");
        assert_eq!(shows(&start.effects), 0);
        let effects = run.finish(RequestId(2), result(messages.clone(), false));
        assert_eq!(shows(&effects), 1);
        assert_eq!(run.selected.as_deref(), first_error(&messages).as_ref().map(BuildNode::key));
        assert_eq!(navigations(&effects).len(), 1);
    }

    #[test]
    fn awaits_a_build_only_when_requested_and_keeps_its_successful_build_hidden() {
        let mut run = BuildRun::default();
        let start = run.start(BuildKind::Artifacts, None, false, REQUEST, true).expect("starts");
        assert!(start.effects.contains(&BuildEffect::Context { running: true, last_status: None }));
        assert_eq!(shows(&start.effects), 0);
        let effects = run.finish(REQUEST, result(Vec::new(), true));
        assert!(effects.contains(&BuildEffect::Context { running: false, last_status: Some(BuildStatus::Success) }));
        assert_eq!(shows(&effects), 0);
        assert_eq!(BuildStatus::Success.as_str(), "success");
    }

    #[test]
    fn keeps_ordinary_build_module_and_artifacts_calls_nonblocking() {
        let mut run = BuildRun::default();
        run.start(BuildKind::Build, Some("/p/core".to_owned()), true, REQUEST, true).expect("starts");
        assert_eq!(run.status, Some(BuildStatus::Running), "running as soon as it starts");
        assert_eq!(run.rerun_target(), (BuildKind::Build, Some("/p/core".to_owned())));
        assert!(BuildRun::default().start(BuildKind::Artifacts, None, true, REQUEST, false).is_none(), "no project");
    }

    #[test]
    fn starts_a_build_without_a_workbench() {
        let mut run = BuildRun::default();
        run.start(BuildKind::Build, None, true, REQUEST, true).expect("starts");
        run.finish(REQUEST, result(Vec::new(), true));
        assert_eq!(run.status, Some(BuildStatus::Success));
        assert_eq!(run.request, None);
    }

    #[test]
    fn builds_the_module_of_the_given_file_and_can_be_stopped() {
        let mut run = BuildRun::default();
        run.start(BuildKind::Build, Some("/p/core/src/A.java".to_owned()), true, REQUEST, true).expect("starts");
        assert_eq!(run.file.as_deref(), Some("/p/core/src/A.java"));
        assert_eq!(run.stop(), Some(BuildEffect::Cancel(REQUEST)));
        let effects = run.cancelled(REQUEST);
        assert_eq!(run.status, Some(BuildStatus::Cancelled));
        assert!(notifications(&effects).is_empty());
        assert!(effects.contains(&BuildEffect::Context { running: false, last_status: Some(BuildStatus::Cancelled) }));
        assert_eq!(run.stop(), None);
    }

    #[test]
    fn a_build_that_cannot_run_lists_its_error() {
        let mut run = BuildRun::default();
        run.start(BuildKind::Build, None, false, REQUEST, true).expect("starts");
        let effects = run.fail(REQUEST, "no build system");
        let last = run.lines.back().map(|line| (line.text.as_str(), line.kind));
        assert_eq!(last, Some(("no build system", LineKind::Err)));
        assert_eq!(notifications(&effects), [(NotifyLevel::Error, "Build failed".to_owned())]);
        assert_eq!(shows(&effects), 1);
        assert!(navigations(&effects).is_empty(), "a message about the build itself has no source");
        assert_eq!(run.selected.as_deref(), Some("build:msg:0"));
    }

    #[test]
    fn shows_the_cmake_hammer_for_compiled_configurations() {
        assert!(title_item_shown(true, false, false, "python"));
        assert!(title_item_shown(false, true, false, "application"));
        assert!(!title_item_shown(false, true, false, "python"));
        assert!(!title_item_shown(false, false, false, ""));
    }
}
