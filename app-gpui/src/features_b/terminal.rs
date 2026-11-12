//! One terminal session's life around its PTY: where it starts (a restored tab whose directory is
//! gone starts in the project root), spawning and restarting the shell, typed input while the PTY
//! is still starting, the pane's first fit, the tab's own font zoom, and opening the file a
//! terminal link names.
//!
//! Nothing here calls the backend. [`LiveSession`] returns the [`SessionEffect`]s its caller
//! carries out and takes the answers back; an answer for an older spawn (before a restart) is
//! dropped. [`FileLinkOpen`] asks for one directory listing or file search at a time.

use crate::platform::command_line::parse_shell_command;
use crate::platform::i18n::I18n;
use crate::platform::misc::platform_info::PlatformInfo;
use crate::terminal::fit::GridSize;
use crate::terminal::links::{LinkTarget, TerminalLink, resolve_path};
use crate::terminal::store::{SessionCommand, SessionSnapshot, SessionStatus, TerminalSession};
use crate::terminal::windows_pty::{ConPty, windows_pty};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::workflow_settings::TerminalSettings;
use ide_core::{IpcPath, Pos, TerminalId};
use ide_index::FileMatch;
use ide_terminal::{TerminalInfo, TerminalOptions};
use ide_vfs::{EntryKind, FileEntry};

/// Printed under a shell that ended or could not start.
pub const RESTART_HINT: &str = "\x1b[2mPress Enter to restart\x1b[0m";
/// How long the exit message waits, so output still in flight lands before it.
pub const EXIT_MESSAGE_DELAY_MS: f64 = 60.0;
/// The font sizes a tab's zoom stays within.
pub const MIN_FONT_SIZE: f32 = 8.0;
pub const MAX_FONT_SIZE: f32 = 36.0;
/// How many fits in a row may find no box or no measured cell before the session waits for its
/// pane to change size.
const MAX_FIT_ATTEMPTS: u32 = 20;
/// Results a project-wide lookup of a link's file asks for.
const FIND_LIMIT: usize = 5;

/// Where a session starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartCwd<'a> {
    /// In this directory, without asking the file system.
    Ready(&'a str),
    /// In this directory if it still is one; ask for its entry kind, then [`checked_start_cwd`].
    Check(&'a str),
}

/// Where a session that remembers `cwd` starts. A restored tab has the project root as
/// `fallback_root`: its saved directory may have been deleted since, and the backend would start
/// such a shell in the home directory instead.
pub fn start_cwd<'a>(cwd: &'a str, fallback_root: Option<&str>) -> StartCwd<'a> {
    match fallback_root {
        Some(root) if root != cwd => StartCwd::Check(cwd),
        _ => StartCwd::Ready(cwd),
    }
}

/// The start directory once the kind of `cwd` is known (`None`: it is gone or cannot be reached).
pub fn checked_start_cwd<'a>(cwd: &'a str, fallback_root: &'a str, kind: Option<EntryKind>) -> &'a str {
    if kind == Some(EntryKind::Dir) { cwd } else { fallback_root }
}

/// What the caller of a [`LiveSession`] carries out.
#[derive(Debug, Clone)]
pub enum SessionEffect {
    /// Ask for the entry kind of `path`, then call [`LiveSession::dir_checked`].
    CheckDir {
        generation: u64,
        path: String,
    },
    /// Spawn the PTY, then call [`LiveSession::spawned`].
    Spawn {
        generation: u64,
        options: TerminalOptions,
    },
    /// Write to the PTY; a failure goes to [`LiveSession::write_failed`].
    Write {
        id: TerminalId,
        data: String,
    },
    Kill(TerminalId),
    /// Show this text on the terminal's screen.
    Print(String),
    /// Call [`LiveSession::start`] with the current settings and grid.
    Start,
    /// Call [`LiveSession::exit_message`] after this long.
    AnnounceExit {
        after_ms: f64,
    },
}

/// What a fit of the pane leads to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FitOutcome {
    /// The terminal can be shown now (once): the pane has a box.
    pub open: bool,
    /// Fit again on the next schedule: the box or the cell is not measured yet.
    pub retry: bool,
    /// Resize the grid to this.
    pub resize: Option<GridSize>,
    /// Start the shell ([`LiveSession::start`]).
    pub start: bool,
    /// Tell the PTY this size once it settles.
    pub resize_pty: Option<GridSize>,
}

/// Whether ending the session would kill something the user started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyCheck {
    No,
    Yes,
    /// Ask the backend whether a command runs in the shell.
    AskBackend(TerminalId),
}

/// One session and its PTY.
#[derive(Debug, Clone)]
pub struct LiveSession {
    pub key: String,
    command: Option<SessionCommand>,
    /// The directory the session was opened in.
    cwd: String,
    fallback_root: Option<String>,
    snapshot: SessionSnapshot,
    pty: Option<TerminalId>,
    /// Bumped by every start, so the answers of an earlier spawn are dropped.
    generation: u64,
    /// The spawn waiting for its directory check.
    pending_spawn: Option<(u64, TerminalOptions)>,
    /// Keystrokes typed while the PTY is still starting.
    pending_input: String,
    /// The tab's font size offset from the settings' size.
    font_zoom: f32,
    font_size: f32,
    opened: bool,
    fit_attempts: u32,
    conpty: Option<ConPty>,
    disposed: bool,
}

impl LiveSession {
    /// The session of `session` (a tab's, from [`crate::terminal::store::Terminals::session`]):
    /// its font starts at `font_base`, the settings' size (see
    /// [`crate::terminal::theme::terminal_font`]); `conpty` is [`windows_pty`] for its window.
    pub fn new(session: &TerminalSession, font_base: f32, conpty: Option<ConPty>) -> Self {
        Self {
            key: session.key.clone(),
            command: session.command.clone(),
            cwd: session.state.cwd.clone(),
            fallback_root: session.fallback_root.clone(),
            snapshot: SessionSnapshot { title: "Local".to_owned(), ..session.state.clone() },
            pty: None,
            generation: 0,
            pending_spawn: None,
            pending_input: String::new(),
            font_zoom: 0.0,
            font_size: font_base.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE),
            opened: false,
            fit_attempts: 0,
            conpty,
            disposed: false,
        }
    }

    pub fn state(&self) -> &SessionSnapshot {
        &self.snapshot
    }

    pub fn terminal_id(&self) -> Option<TerminalId> {
        self.pty
    }

    pub fn conpty(&self) -> Option<ConPty> {
        self.conpty
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// The session's owner handles its end (a plugin's session): it does not restart.
    fn owned(&self) -> bool {
        self.command.as_ref().is_some_and(|command| command.plugin_operation.is_some())
    }

    /// The window's platform info arrived or changed (a restored terminal is made before it is
    /// known). Returns the pseudo console to configure, `None` once the session is disposed.
    pub fn platform_changed(&mut self, platform: &PlatformInfo, remote: bool) -> Option<Option<ConPty>> {
        if self.disposed {
            return None;
        }
        self.conpty = windows_pty(platform, remote);
        Some(self.conpty)
    }

    /// The pane was laid out: `has_box` when it has a size, `proposed` the grid that fills it once
    /// the cell is measured, `current` the grid's size.
    pub fn fit(&mut self, has_box: bool, proposed: Option<GridSize>, current: GridSize) -> FitOutcome {
        let mut outcome = FitOutcome::default();
        if self.disposed {
            return outcome;
        }
        if !has_box {
            outcome.retry = self.retry_fit();
            return outcome;
        }
        if !self.opened {
            self.opened = true;
            outcome.open = true;
        }
        let Some(grid) = proposed else {
            outcome.retry = self.retry_fit();
            return outcome;
        };
        self.fit_attempts = 0;
        outcome.resize = (grid != current).then_some(grid);
        if self.snapshot.status == SessionStatus::Idle {
            outcome.start = true;
        } else if self.pty.is_some() {
            outcome.resize_pty = Some(grid);
        }
        outcome
    }

    fn retry_fit(&mut self) -> bool {
        self.fit_attempts += 1;
        self.fit_attempts <= MAX_FIT_ATTEMPTS
    }

    /// The pane changed size or was shown again: fits may retry from the start.
    pub fn refit(&mut self) {
        self.fit_attempts = 0;
    }

    /// Starts the shell (or the session's command) with `settings` for a `grid` sized terminal;
    /// `windows_host` when the shell runs on Windows. Nothing while it starts or runs.
    pub fn start(&mut self, settings: &TerminalSettings, windows_host: bool, grid: GridSize) -> Option<SessionEffect> {
        if self.disposed || matches!(self.snapshot.status, SessionStatus::Starting | SessionStatus::Running) {
            return None;
        }
        self.generation += 1;
        let generation = self.generation;
        self.snapshot.status = SessionStatus::Starting;
        self.snapshot.exit_code = None;
        self.snapshot.app_title = None;
        self.snapshot.command_running = self.command.as_ref().is_some_and(|command| !command.shell);
        let options = self.spawn_options(settings, windows_host, grid);
        match start_cwd(&self.cwd, self.fallback_root.as_deref()) {
            StartCwd::Ready(_) => Some(SessionEffect::Spawn { generation, options }),
            StartCwd::Check(path) => {
                let path = path.to_owned();
                self.pending_spawn = Some((generation, options));
                Some(SessionEffect::CheckDir { generation, path })
            }
        }
    }

    fn spawn_options(&self, settings: &TerminalSettings, windows_host: bool, grid: GridSize) -> TerminalOptions {
        let (shell, args) = match &self.command {
            Some(command) => (Some(command.program.clone()), command.args.clone()),
            None => {
                let words = parse_shell_command(&settings.shell_path, windows_host);
                (words.first().cloned(), words.iter().skip(1).cloned().collect())
            }
        };
        let mut env = if self.owned() { Default::default() } else { settings.env.clone() };
        if let Some(command) = &self.command {
            env.extend(command.env.clone());
        }
        TerminalOptions {
            cwd: IpcPath::from(self.cwd.as_str()),
            shell,
            args,
            env,
            cols: grid.cols.max(2),
            rows: grid.rows.max(1),
            shell_integration: self.command.as_ref().is_none_or(|command| command.shell) && settings.shell_integration,
        }
    }

    /// The entry kind of the saved directory (`None`: it is gone): the spawn goes ahead, in the
    /// project root when the directory is no longer one.
    pub fn dir_checked(&mut self, generation: u64, kind: Option<EntryKind>) -> Option<SessionEffect> {
        let (pending, mut options) = self.pending_spawn.take()?;
        if pending != generation || generation != self.generation || self.disposed {
            return None;
        }
        let root = self.fallback_root.as_deref().unwrap_or(&self.cwd);
        let cwd = checked_start_cwd(&self.cwd, root, kind).to_owned();
        if cwd != self.cwd {
            options.cwd = IpcPath::from(cwd.as_str());
            self.snapshot.cwd = cwd;
        }
        Some(SessionEffect::Spawn { generation, options })
    }

    /// The answer to the spawn of `generation`: the PTY, or why it could not start.
    pub fn spawned(&mut self, generation: u64, result: Result<&TerminalInfo, &str>) -> Vec<SessionEffect> {
        let current = generation == self.generation && !self.disposed;
        match result {
            Ok(info) if !current => vec![SessionEffect::Kill(info.id)],
            Ok(info) => {
                self.pty = Some(info.id);
                self.snapshot.status = SessionStatus::Running;
                self.snapshot.title = if info.title.is_empty() { "Local".to_owned() } else { info.title.clone() };
                let queued = std::mem::take(&mut self.pending_input);
                self.input(&queued)
            }
            Err(_) if !current => Vec::new(),
            Err(error) => {
                self.pending_input.clear();
                let failed = format!("\x1b[31mFailed to start terminal: {error}\x1b[0m\r\n");
                let mut effects = vec![SessionEffect::Print(failed)];
                if !self.owned() {
                    effects.push(SessionEffect::Print(format!("{RESTART_HINT}\r\n")));
                }
                self.snapshot.status = SessionStatus::Error;
                effects
            }
        }
    }

    /// Whether ending the session would kill something the user started (as IDEA judges it): a
    /// program run instead of a shell always counts; a shell counts while a command runs in it.
    pub fn busy_check(&self) -> BusyCheck {
        let Some(id) = self.pty.filter(|_| self.snapshot.status == SessionStatus::Running) else {
            return BusyCheck::No;
        };
        let program = self.command.as_ref().is_some_and(|command| !command.shell);
        if program { BusyCheck::Yes } else { BusyCheck::AskBackend(id) }
    }

    /// The backend reports the PTY exited.
    pub fn exited(&mut self, code: Option<i32>) -> Vec<SessionEffect> {
        if self.disposed {
            return Vec::new();
        }
        self.pty = None;
        self.pending_input.clear();
        self.snapshot.status = SessionStatus::Exited;
        self.snapshot.exit_code = code;
        vec![SessionEffect::AnnounceExit { after_ms: EXIT_MESSAGE_DELAY_MS }]
    }

    /// The text that tells the session ended, if it still has (it was not restarted or disposed
    /// in the meantime).
    pub fn exit_message(&self) -> Option<String> {
        if self.disposed || self.snapshot.status != SessionStatus::Exited {
            return None;
        }
        let suffix = match self.snapshot.exit_code {
            Some(code) if code != 0 => format!(" (exit code {code})"),
            _ => String::new(),
        };
        let hint = if self.owned() { String::new() } else { format!("{RESTART_HINT}\r\n") };
        Some(format!("\r\n\x1b[2m[Process completed{suffix}]\x1b[0m\r\n{hint}"))
    }

    /// Starts an ended or failed session again (Enter); a session its owner ends does not.
    pub fn restart(&mut self) -> Vec<SessionEffect> {
        if self.owned() || matches!(self.snapshot.status, SessionStatus::Starting | SessionStatus::Running) {
            return Vec::new();
        }
        self.pending_input.clear();
        self.snapshot.status = SessionStatus::Idle;
        vec![SessionEffect::Print("\r\n".to_owned()), SessionEffect::Start]
    }

    /// Input for the PTY (typed, pasted, or a shortcut's translation; sending a shortcut also
    /// scrolls the terminal to the bottom). Enter, or Shift+Enter's Esc+Return, restarts an ended or
    /// failed session; input while the PTY starts is sent once it exists.
    pub fn input(&mut self, data: &str) -> Vec<SessionEffect> {
        if data.is_empty() {
            return Vec::new();
        }
        if matches!(self.snapshot.status, SessionStatus::Exited | SessionStatus::Error) {
            return if data == "\r" || data == "\x1b\r" { self.restart() } else { Vec::new() };
        }
        match self.pty {
            Some(id) => vec![SessionEffect::Write { id, data: data.to_owned() }],
            None => {
                self.pending_input.push_str(data);
                Vec::new()
            }
        }
    }

    /// The PTY refused input: the user is told instead of the input vanishing.
    pub fn write_failed(&self, error: &str, i18n: &I18n) -> NotificationRequest {
        NotificationRequest {
            body: Some(i18n.tx(error)),
            ..NotificationRequest::new(NotificationLevel::Error, i18n.tx("Cannot write to terminal"))
        }
    }

    /// Settings › Terminal changed: the font size for the settings' size `base`, with the tab's
    /// zoom kept.
    pub fn apply_settings(&mut self, base: f32) -> f32 {
        self.font_size = (base + self.font_zoom).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.font_size
    }

    /// Steps the tab's font size by `delta` px from the settings' size `base` (`0` resets it).
    /// Returns the new size when it changed: the terminal is refitted.
    pub fn step_font_size(&mut self, base: f32, delta: f32) -> Option<f32> {
        if delta == 0.0 {
            self.font_zoom = 0.0;
        } else {
            self.font_zoom = (base + self.font_zoom + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE) - base;
        }
        let size = (base + self.font_zoom).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        (size != self.font_size).then(|| {
            self.font_size = size;
            size
        })
    }

    /// Ends the session: its PTY is killed and nothing it answers later changes it.
    pub fn dispose(&mut self) -> Vec<SessionEffect> {
        if self.disposed {
            return Vec::new();
        }
        self.disposed = true;
        self.pending_input.clear();
        self.pending_spawn = None;
        self.pty.take().map(SessionEffect::Kill).into_iter().collect()
    }
}

/// A step of opening a terminal link's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLinkStep {
    /// List this directory, then call [`FileLinkOpen::dir_listed`].
    ReadDir(String),
    /// Look the file up in the project, then call [`FileLinkOpen::files_found`].
    FindFiles { query: String, limit: usize },
    /// Open the file and focus it, with the cursor at `selection`.
    Open { path: String, selection: Option<Pos> },
    /// Nothing matches.
    NotFound(NotificationRequest),
}

/// Opening the file of a terminal link: the path against the terminal's directory when that
/// directory lists it, else a project-wide lookup that ranks the link's directory part over its
/// bare name.
#[derive(Debug, Clone)]
pub struct FileLinkOpen {
    path: String,
    name: String,
    link_path: String,
    selection: Option<Pos>,
    /// The project-wide queries not asked yet, the next one last.
    queries: Vec<String>,
    /// The query being looked up.
    query: Option<String>,
}

impl FileLinkOpen {
    /// Starts opening `link` (a file link; `None` for a URL) of a terminal in `cwd`.
    pub fn start(link: &TerminalLink, cwd: &str) -> Option<(Self, FileLinkStep)> {
        let LinkTarget::File { path, line, col } = &link.target else { return None };
        let selection =
            line.filter(|&line| line > 0).map(|line| Pos::new(line - 1, col.unwrap_or(1).saturating_sub(1)));
        let resolved = resolve_path(path, cwd, None);
        let rel = path.replace('\\', "/");
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
        let queries = if rel == name { vec![name.clone()] } else { vec![name.clone(), rel] };
        let dir = match resolved.rfind('/') {
            Some(slash) if slash > 0 => resolved[..slash].to_owned(),
            _ => "/".to_owned(),
        };
        let open = Self { path: resolved, name, link_path: path.clone(), selection, queries, query: None };
        Some((open, FileLinkStep::ReadDir(dir)))
    }

    /// The listing of the link's directory (`None`: it could not be read).
    pub fn dir_listed(&mut self, entries: Option<&[FileEntry]>) -> FileLinkStep {
        let entries = entries.unwrap_or_default();
        if entries.iter().any(|entry| entry.name == self.name && entry.kind != EntryKind::Dir) {
            return FileLinkStep::Open { path: self.path.clone(), selection: self.selection };
        }
        self.next_query()
    }

    /// The files the lookup asked for found (`None`: it failed). A hit must sit on the queried path.
    pub fn files_found(&mut self, hits: Option<&[FileMatch]>) -> FileLinkStep {
        let query = self.query.take().unwrap_or_default();
        let suffix = format!("/{query}");
        let mut hits = hits.unwrap_or_default().iter();
        match hits.find(|hit| hit.rel_path == query || hit.rel_path.ends_with(&suffix)) {
            Some(hit) => {
                FileLinkStep::Open { path: hit.path.to_string_lossy().into_owned(), selection: self.selection }
            }
            None => self.next_query(),
        }
    }

    fn next_query(&mut self) -> FileLinkStep {
        self.query = self.queries.pop();
        match &self.query {
            Some(query) => FileLinkStep::FindFiles { query: query.clone(), limit: FIND_LIMIT },
            None => FileLinkStep::NotFound(NotificationRequest {
                body: Some(self.link_path.clone()),
                ..NotificationRequest::new(NotificationLevel::Warning, "File not found")
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::misc::platform_info::Os;
    use crate::terminal::links::find_terminal_links;
    use crate::terminal::store::{SplitOrientation, TerminalEffect, Terminals};
    use crate::terminal::theme::terminal_font;
    use std::collections::HashMap;

    const GRID: GridSize = GridSize { cols: 80, rows: 24 };

    fn file_link(text: &str) -> TerminalLink {
        let link = find_terminal_links(text).into_iter().next();
        link.filter(|link| !link.is_url()).unwrap_or_else(|| panic!("not a file link: {text}"))
    }

    fn entry(name: &str, kind: EntryKind) -> FileEntry {
        FileEntry {
            name: name.to_owned(),
            path: IpcPath::from(name),
            kind,
            size: 0,
            mtime_ms: 0.0,
            ignored: false,
            excluded: false,
            hidden: false,
            target_kind: None,
        }
    }

    fn hit(path: &str, rel_path: &str) -> FileMatch {
        FileMatch { path: IpcPath::from(path), rel_path: rel_path.to_owned(), score: 0, indices: Vec::new() }
    }

    fn open(path: &str, line: u32) -> FileLinkStep {
        FileLinkStep::Open { path: path.to_owned(), selection: Some(Pos::new(line, 0)) }
    }

    fn session(cwd: &str, fallback_root: Option<&str>) -> LiveSession {
        let state = SessionSnapshot { cwd: cwd.to_owned(), ..SessionSnapshot::default() };
        let fallback_root = fallback_root.map(str::to_owned);
        let session = TerminalSession { key: "s".to_owned(), command: None, fallback_root, state };
        LiveSession::new(&session, 14.0, None)
    }

    fn info(id: u32) -> TerminalInfo {
        TerminalInfo { id: TerminalId(id), title: "zsh".to_owned(), pid: None }
    }

    fn spawn_cwd(effect: Option<&SessionEffect>) -> (u64, String) {
        let Some(SessionEffect::Spawn { generation, options }) = effect else { panic!("not a spawn: {effect:?}") };
        (*generation, options.cwd.to_string_lossy().into_owned())
    }

    fn platform(os: Os, windows_build_number: Option<u32>) -> PlatformInfo {
        PlatformInfo { os, windows_build_number, ..PlatformInfo::guessed(None, true) }
    }

    #[test]
    fn updates_a_restored_terminal_when_platform_info_arrives_and_unsubscribes_on_disposal() {
        let mut live = session("/repo", None);
        live.platform_changed(&platform(Os::Windows, None), false);
        let conpty = live.platform_changed(&platform(Os::Windows, Some(22631)), false);
        assert_eq!(conpty, Some(Some(ConPty { build_number: Some(22631) })));
        let _ = live.dispose();
        assert_eq!(live.platform_changed(&platform(Os::Windows, Some(19045)), false), None);
        assert_eq!(live.conpty(), Some(ConPty { build_number: Some(22631) }));
    }

    #[test]
    fn opens_the_resolved_path_when_the_directory_lists_it() {
        let (mut open_link, step) = FileLinkOpen::start(&file_link("a/main.php:2"), "C:\\Users\\me\\proj").unwrap();
        assert_eq!(step, FileLinkStep::ReadDir("C:/Users/me/proj/a".to_owned()));
        let step = open_link.dir_listed(Some(&[entry("main.php", EntryKind::File)]));
        assert_eq!(step, open("C:/Users/me/proj/a/main.php", 1));
    }

    #[test]
    fn ranks_the_links_directory_over_the_bare_name_in_the_project_wide_fallback() {
        let (mut open_link, _) = FileLinkOpen::start(&file_link("a/main.php:2"), "/p").unwrap();
        let step = open_link.dir_listed(Some(&[]));
        assert_eq!(step, FileLinkStep::FindFiles { query: "a/main.php".to_owned(), limit: 5 });
        let hits = [hit("/p/b/main.php", "b/main.php"), hit("/p/a/main.php", "a/main.php")];
        assert_eq!(open_link.files_found(Some(&hits)), open("/p/a/main.php", 1));
    }

    #[test]
    fn falls_back_to_the_bare_name_for_a_basename_only_link() {
        let (mut open_link, _) = FileLinkOpen::start(&file_link("main.php:1"), "/p").unwrap();
        let step = open_link.dir_listed(Some(&[]));
        assert_eq!(step, FileLinkStep::FindFiles { query: "main.php".to_owned(), limit: 5 });
        let step = open_link.files_found(Some(&[hit("/p/x/main.php", "x/main.php")]));
        assert_eq!(step, open("/p/x/main.php", 0));
    }

    #[test]
    fn warns_when_nothing_matches() {
        let (mut open_link, _) = FileLinkOpen::start(&file_link("gone.php:1"), "/p").unwrap();
        assert!(matches!(open_link.dir_listed(None), FileLinkStep::FindFiles { .. }));
        let FileLinkStep::NotFound(notification) = open_link.files_found(Some(&[])) else {
            panic!("the file was found");
        };
        assert_eq!(notification.level, NotificationLevel::Warning);
        assert_eq!(notification.title, "File not found");
        assert_eq!(notification.body.as_deref(), Some("gone.php"));
    }

    #[test]
    fn keeps_a_saved_directory_that_still_exists() {
        assert_eq!(start_cwd("/repo/logs", Some("/repo")), StartCwd::Check("/repo/logs"));
        assert_eq!(checked_start_cwd("/repo/logs", "/repo", Some(EntryKind::Dir)), "/repo/logs");
    }

    #[test]
    fn falls_back_to_the_project_root_when_the_saved_directory_is_gone() {
        assert_eq!(checked_start_cwd("/repo/logs", "/repo", None), "/repo");
    }

    #[test]
    fn falls_back_when_the_saved_path_is_a_file_rather_than_a_directory() {
        assert_eq!(checked_start_cwd("/repo/logs", "/repo", Some(EntryKind::File)), "/repo");
    }

    #[test]
    fn does_not_touch_the_file_system_without_a_fallback_or_when_the_paths_match() {
        assert_eq!(start_cwd("/repo", Some("/repo")), StartCwd::Ready("/repo"));
        assert_eq!(start_cwd("/anywhere", None), StartCwd::Ready("/anywhere"));
    }

    #[test]
    fn shows_rejected_terminal_input_instead_of_silently_discarding_it() {
        let mut live = session("/repo", None);
        let (generation, _) = spawn_cwd(live.start(&TerminalSettings::default(), false, GRID).as_ref());
        assert!(live.spawned(generation, Ok(&info(9))).is_empty());
        let effects = live.input("paste");
        assert!(matches!(&effects[..], [SessionEffect::Write { id: TerminalId(9), data }] if data == "paste"));
        let notification = live.write_failed("input queue full", &I18n::default());
        assert_eq!(notification.level, NotificationLevel::Error);
        assert_eq!(notification.title, "Cannot write to terminal");
        assert_eq!(notification.body.as_deref(), Some("input queue full"));
    }

    #[test]
    fn spawns_a_restored_session_in_the_project_root_when_its_saved_directory_is_gone() {
        let mut live = session("/repo/gone", Some("/repo"));
        let Some(SessionEffect::CheckDir { generation, path }) = live.start(&TerminalSettings::default(), false, GRID)
        else {
            panic!("the directory is not checked");
        };
        assert_eq!(path, "/repo/gone");
        assert_eq!(spawn_cwd(live.dir_checked(generation, None).as_ref()), (generation, "/repo".to_owned()));
        assert!(live.spawned(generation, Ok(&info(7))).is_empty());
        assert_eq!(live.state().cwd, "/repo");
        assert_eq!(live.state().status, SessionStatus::Running);
        assert!(matches!(live.dispose()[..], [SessionEffect::Kill(TerminalId(7))]));
    }

    #[test]
    fn keeps_spawning_in_a_saved_directory_that_still_exists() {
        let mut live = session("/repo/logs", Some("/repo"));
        let Some(SessionEffect::CheckDir { generation, .. }) = live.start(&TerminalSettings::default(), false, GRID)
        else {
            panic!("the directory is not checked");
        };
        let spawn = live.dir_checked(generation, Some(EntryKind::Dir));
        assert_eq!(spawn_cwd(spawn.as_ref()).1, "/repo/logs");
        assert!(live.spawned(generation, Ok(&info(8))).is_empty());
        assert_eq!(live.state().cwd, "/repo/logs");
    }

    #[test]
    fn restarts_a_session_that_failed_to_start_on_shift_enters_esc_return_as_on_enter() {
        let settings = TerminalSettings::default();
        let mut live = session("/repo", None);
        let (generation, _) = spawn_cwd(live.start(&settings, false, GRID).as_ref());
        let effects = live.spawned(generation, Err("no shell"));
        assert!(matches!(&effects[..], [SessionEffect::Print(failed), SessionEffect::Print(hint)]
            if failed.contains("Failed to start terminal: no shell") && hint.contains(RESTART_HINT)));
        assert_eq!(live.state().status, SessionStatus::Error);
        assert!(live.input("\x1b").is_empty());
        assert_eq!(live.state().status, SessionStatus::Error);
        let effects = live.input("\x1b\r");
        assert!(matches!(effects[..], [SessionEffect::Print(_), SessionEffect::Start]));
        let (second, _) = spawn_cwd(live.start(&settings, false, GRID).as_ref());
        assert_eq!(second, generation + 1);
        let _ = live.dispose();
    }

    #[test]
    fn waits_to_open_xterm_until_the_pane_has_a_size() {
        let mut live = session("/tmp", None);
        let unsized_fit = live.fit(false, None, GRID);
        assert!(!unsized_fit.open && unsized_fit.retry && !unsized_fit.start);
        let grid = GridSize { cols: 50, rows: 20 };
        let sized = live.fit(true, Some(grid), GRID);
        assert!(sized.open && sized.start);
        assert_eq!(sized.resize, Some(grid));
        assert!(!live.fit(true, Some(grid), grid).open);
    }

    #[test]
    fn keeps_the_offset_through_settings_refit_and_moving_splits_fresh_splits_and_restores_start_at_the_base() {
        fn base(font_size: f32) -> f32 {
            let settings = TerminalSettings { use_editor_font: false, font_size, ..TerminalSettings::default() };
            terminal_font(&settings, None, "monospace", None).size
        }
        #[derive(Default)]
        struct World {
            terminals: Terminals,
            live: HashMap<String, LiveSession>,
            storage: HashMap<String, String>,
        }
        fn sync(world: &mut World, root: Option<&str>, size: f32) {
            let storage = &world.storage;
            world.terminals.sync_project(root, |key| storage.get(key).cloned());
            apply(world, size);
        }
        fn apply(world: &mut World, size: f32) {
            for effect in world.terminals.take_effects() {
                match effect {
                    TerminalEffect::SessionCreated(key) => {
                        let session = world.terminals.session(&key).unwrap();
                        world.live.insert(key, LiveSession::new(session, base(size), None));
                    }
                    TerminalEffect::SessionDisposed { key, .. } => {
                        world.live.remove(&key);
                    }
                    TerminalEffect::Store { key, value } => {
                        world.storage.insert(key, value);
                    }
                }
            }
        }
        let size_of = |world: &World, key: &str| world.live[key].font_size();
        let mut world = World::default();
        let mut size = 14.0;
        sync(&mut world, Some("/font-zoom"), size);
        let a = world.terminals.new_tab("/font-zoom", "Local", None);
        let b = world.terminals.new_tab("/font-zoom", "Local", None);
        apply(&mut world, size);
        world.live.get_mut(&a.session).unwrap().step_font_size(base(size), 2.0);
        assert_eq!(size_of(&world, &a.session), 16.0);
        assert_eq!(size_of(&world, &b.session), 14.0);
        size = 18.0;
        for session in world.live.values_mut() {
            session.apply_settings(base(size));
        }
        assert_eq!(size_of(&world, &a.session), 20.0);
        world.live.get_mut(&a.session).unwrap().refit();
        assert_eq!(size_of(&world, &a.session), 20.0);
        world.terminals.split_tab(&a.id, SplitOrientation::Horizontal, true, "Local").unwrap();
        apply(&mut world, size);
        assert_eq!(world.terminals.tab(&a.id).map(|tab| tab.session.as_str()), Some(a.session.as_str()));
        assert_eq!(size_of(&world, &a.session), 20.0);
        let split = world.terminals.split_tab(&a.id, SplitOrientation::Vertical, false, "Local").unwrap();
        apply(&mut world, size);
        let split_session = world.terminals.tab(&split).unwrap().session.clone();
        assert_eq!(size_of(&world, &split_session), 18.0);
        world.live.get_mut(&a.session).unwrap().step_font_size(base(size), 0.0);
        assert_eq!(size_of(&world, &a.session), 18.0);
        world.live.get_mut(&b.session).unwrap().step_font_size(base(size), 3.0);
        assert_eq!(size_of(&world, &b.session), 21.0);
        assert_eq!(size_of(&world, &a.session), 18.0);
        sync(&mut world, None, size);
        sync(&mut world, Some("/font-zoom"), size);
        assert_eq!(world.terminals.tabs().len(), 3);
        for tab in world.terminals.tabs() {
            assert_eq!(size_of(&world, &tab.session), 18.0);
        }
    }
}
