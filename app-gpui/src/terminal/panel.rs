//! The Terminal tool window: its tabs, each a shell of its own, and the tab strip above them.
//!
//! [`Terminals`] decides what tabs and sessions exist and remembers them per project; this view
//! carries its effects out (a terminal for a new session, an end for a disposed one, the
//! remembered tabs to storage) and shows the active tab's terminal.

use super::store::{SessionCommand, SessionSnapshot, SessionStatus, TerminalEffect, Terminals, start_directory};
use super::view::{TerminalEvent, TerminalView};
use crate::platform::command_line::parse_shell_command;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::workbench::notifications::NotificationRequest;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels, Point, Render, Subscription,
    Window, div, prelude::*,
};
use ide_terminal::TerminalOptions;
use std::collections::HashMap;
use std::path::PathBuf;

pub mod groups;

/// What the terminal tool window tells its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalPanelEvent {
    /// The last tab closed: the tool window has nothing to show.
    Emptied,
    /// The header's Options button was pressed here: the window's Options menu.
    ShowOptions(Point<Pixels>),
    /// The header's Hide button.
    Hide,
    /// A right click on tab `tab`: its menu.
    TabMenu { tab: String, position: Point<Pixels> },
    /// A group's More Terminal Options button was pressed here; its group is the active one now.
    MoreMenu(Point<Pixels>),
    /// Close these tabs as their Close Tab button does (its ✕, the tab menu, ⌘W in its terminal),
    /// asking first whether to terminate a command that runs.
    CloseTabs(Vec<String>),
    /// A paste failed: the notification that says why.
    Notify(NotificationRequest),
}

pub struct TerminalPanel {
    focus: FocusHandle,
    window_label: String,
    /// The project folder terminals start in.
    root: String,
    terminals: Terminals,
    /// The terminal of each session, with what the panel listens to it for.
    views: HashMap<String, (Entity<TerminalView>, Subscription)>,
    /// The panel has just started with a tab opened for the occasion, which no one has claimed.
    fresh: bool,
    /// The sizes the groups were dragged to, and the drag going on.
    splits: groups::Splits,
    /// The pointer is over the tool window: its Options and Hide buttons show.
    hovered: bool,
}

impl EventEmitter<TerminalPanelEvent> for TerminalPanel {}

impl TerminalPanel {
    /// The terminals of the project at `root`: the tabs it was left with, or one new tab.
    pub fn new(window_label: &str, root: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            focus: cx.focus_handle(),
            window_label: window_label.to_owned(),
            root: root.to_string_lossy().into_owned(),
            terminals: Terminals::new(),
            views: HashMap::new(),
            fresh: false,
            splits: groups::Splits::default(),
            hovered: false,
        };
        let root = panel.root.clone();
        let stored = |key: &str| cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned);
        panel.terminals.sync_project(Some(&root), stored);
        if panel.terminals.tabs().is_empty() {
            panel.open_tab(cx);
            panel.fresh = true;
        } else {
            panel.carry_out(cx);
        }
        panel
    }

    pub fn terminals(&self) -> &Terminals {
        &self.terminals
    }

    /// The project folder terminals start in.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Carries out `change` on the tabs, then what it asked for: the shells of the tabs that show
    /// start and the project's tabs are stored. The tab the panel started with counts as claimed.
    pub fn change<R>(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Terminals) -> R) -> R {
        self.fresh = false;
        let result = change(&mut self.terminals);
        self.carry_out(cx);
        result
    }

    /// The terminal of tab `id`, once its shell has started.
    pub fn view_of_tab(&self, id: &str) -> Option<&Entity<TerminalView>> {
        let tab = self.terminals.tab(id)?;
        self.views.get(&tab.session).map(|(view, _)| view)
    }

    /// The tab groups the window shows, in layout order, each with its selected tab.
    pub fn panes(&self) -> Vec<(String, Option<String>)> {
        let selected = self.terminals.selected();
        let groups = self.terminals.layout().groups().into_iter();
        groups.map(|group| (group.to_owned(), selected.get(group).cloned())).collect()
    }

    /// The terminal of the active tab.
    pub fn active_view(&self) -> Option<&Entity<TerminalView>> {
        let session = self.terminals.active_session()?;
        self.views.get(&session.key).map(|(view, _)| view)
    }

    /// The tabs' labels in strip order, with whether each is the active one.
    pub fn tab_titles(&self) -> Vec<(String, bool)> {
        let active = self.terminals.active_tab();
        self.terminals
            .tabs()
            .iter()
            .map(|tab| {
                (self.terminals.title(&tab.id).unwrap_or_else(|| tab.name.clone()), active == Some(tab.id.as_str()))
            })
            .collect()
    }

    /// Whether the panel has only just started with a tab opened for the occasion that nobody
    /// has claimed yet; asking claims it.
    pub fn take_fresh(&mut self) -> bool {
        std::mem::take(&mut self.fresh)
    }

    /// Opens a tab with a shell in the start directory (Settings › Terminal).
    pub fn open_tab(&mut self, cx: &mut Context<Self>) {
        self.fresh = false;
        let terminal = settings::effective(&self.window_label, cx).terminal;
        let cwd = start_directory(&terminal.start_directory, &self.root);
        self.terminals.new_tab(&cwd, &terminal.tab_name, None);
        self.carry_out(cx);
    }

    /// Selects tab `id`.
    pub fn select_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.terminals.select_tab(id);
        self.carry_out(cx);
    }

    /// Closes tab `id` and ends its shell.
    pub fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.terminals.close_tab(id);
        self.carry_out(cx);
    }

    /// Closes the active tab.
    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.terminals.active_tab().map(str::to_owned) {
            self.close_tab(&id, cx);
        }
    }

    /// How the terminal of session `key` starts: in the session's directory, with its own command
    /// or the shell command line of Settings › Terminal.
    fn options(&self, key: &str, cx: &App) -> Option<TerminalOptions> {
        let session = self.terminals.session(key)?;
        let terminal = settings::effective(&self.window_label, cx).terminal;
        let (shell, args, shell_integration) = match &session.command {
            Some(SessionCommand { program, args, shell, .. }) => {
                (Some(program.clone()), args.clone(), *shell && terminal.shell_integration)
            }
            None => {
                let mut command = parse_shell_command(&terminal.shell_path, cfg!(windows));
                let shell = (!command.is_empty()).then(|| command.remove(0));
                (shell, command, terminal.shell_integration)
            }
        };
        let mut env = terminal.env;
        if let Some(command) = &session.command {
            env.extend(command.env.clone());
        }
        Some(TerminalOptions {
            cwd: ide_core::IpcPath(PathBuf::from(&session.state.cwd)),
            shell,
            args,
            env,
            // The grid is fitted to the tool window as soon as it is painted.
            cols: 80,
            rows: 24,
            shell_integration,
        })
    }

    /// Does what the tab model asked for since the last time.
    fn carry_out(&mut self, cx: &mut Context<Self>) {
        for effect in self.terminals.take_effects() {
            match effect {
                // A session's shell starts when its tab first shows: see `start_active`.
                TerminalEffect::SessionCreated(_) => {}
                // Dropping the terminal ends its shell.
                TerminalEffect::SessionDisposed { key, .. } => {
                    self.views.remove(&key);
                }
                TerminalEffect::Store { key, value } => {
                    if cx.has_global::<UiStorage>() {
                        cx.global_mut::<UiStorage>().set(&key, value);
                    }
                }
            }
        }
        self.start_active(cx);
        // Reporting a session's state may have asked for more (the remembered tabs).
        for effect in self.terminals.take_effects() {
            if let TerminalEffect::Store { key, value } = effect
                && cx.has_global::<UiStorage>()
            {
                cx.global_mut::<UiStorage>().set(&key, value);
            }
        }
        if self.terminals.tabs().is_empty() {
            cx.emit(TerminalPanelEvent::Emptied);
        }
        cx.notify();
    }

    /// Starts the shells of the tabs that show, one per tab group, that have none yet. A tab that is
    /// not showing, such as one remembered from the last run, costs no process until it is selected.
    fn start_active(&mut self, cx: &mut Context<Self>) {
        let shown: Vec<String> = self
            .terminals
            .layout()
            .groups()
            .into_iter()
            .filter_map(|group| self.terminals.selected().get(group))
            .filter_map(|id| self.terminals.tab(id))
            .map(|tab| tab.session.clone())
            .collect();
        for key in shown {
            self.start_session(&key, cx);
        }
    }

    /// Starts the shell of session `key` if it has none yet.
    fn start_session(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.views.contains_key(key) {
            return;
        }
        let Some(options) = self.options(key, cx) else { return };
        let label = self.window_label.clone();
        let view = cx.new(|cx| TerminalView::new(&label, options, cx));
        let session = key.to_owned();
        let events = cx.subscribe(&view, move |panel: &mut TerminalPanel, view, event: &TerminalEvent, cx| {
            panel.terminal_event(&session, &view, event, cx);
        });
        let state = self.snapshot(key, &view, cx);
        self.views.insert(key.to_owned(), (view, events));
        self.terminals.set_session_state(key, state);
    }

    /// How many sessions have a running terminal behind them.
    pub fn started(&self) -> usize {
        self.views.len()
    }

    /// What the terminal of session `key` reports about itself.
    fn snapshot(&self, key: &str, view: &Entity<TerminalView>, cx: &App) -> SessionSnapshot {
        let terminal = view.read(cx);
        let session = self.terminals.session(key);
        let before = session.map(|session| session.state.clone()).unwrap_or_default();
        // A session started for a program runs it from start to end. Whether a shell is at its
        // prompt or running a command is told by shell integration's marks, which the emulator
        // does not report: a shell's tab keeps its own name.
        let command_running =
            session.and_then(|session| session.command.as_ref()).is_some_and(|command| !command.shell);
        let status = match (terminal.error(), terminal.exit()) {
            (Some(_), _) => SessionStatus::Error,
            (None, Some(_)) => SessionStatus::Exited,
            (None, None) => SessionStatus::Running,
        };
        SessionSnapshot {
            status,
            exit_code: terminal.exit().flatten(),
            title: terminal.shell_title().to_owned(),
            app_title: terminal.program_title().map(str::to_owned),
            command_running,
            cwd: before.cwd,
        }
    }

    fn terminal_event(
        &mut self,
        key: &str,
        view: &Entity<TerminalView>,
        event: &TerminalEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::TitleChanged => {
                let state = self.snapshot(key, view, cx);
                self.terminals.set_session_state(key, state);
            }
            TerminalEvent::Exited(_) => {
                let state = self.snapshot(key, view, cx);
                self.terminals.set_session_state(key, state);
                let close = settings::effective(&self.window_label, cx).terminal.close_on_exit;
                self.terminals.session_exited(key, close);
            }
            // The window closes it as the tab's Close Tab button does, asking about a running command.
            TerminalEvent::CloseRequested => {
                if let Some(id) = self.terminals.tab_of_session(key).map(|tab| tab.id.clone()) {
                    cx.emit(TerminalPanelEvent::CloseTabs(vec![id]));
                }
            }
            TerminalEvent::PasteFailed(notification) => cx.emit(TerminalPanelEvent::Notify(notification.clone())),
            TerminalEvent::Bell | TerminalEvent::FindRequested => {}
        }
        self.carry_out(cx);
    }
}

impl Focusable for TerminalPanel {
    /// The keyboard given to the tool window goes to the terminal that shows; without a tab, to
    /// the tool window itself.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_view().map_or_else(|| self.focus.clone(), |view| view.focus_handle(cx))
    }
}

impl Render for TerminalPanel {
    /// Each tab group with its own header and tab strip; see [`groups`].
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window's own buttons show while the pointer is over it or the keyboard is in it.
        let buttons = self.hovered || self.focus.contains_focused(window, cx);
        div()
            .id("terminal-window")
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .on_hover(cx.listener(|panel, hovered: &bool, _, cx| {
                if panel.hovered != *hovered {
                    panel.hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(self.render_groups(buttons, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{AppDirs, Backend};
    use crate::terminal::persist::storage_key;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::time::Duration;

    mod proofs23_groups;

    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within thirty seconds");
    }

    /// The tool window starts with one tab, each new tab is a shell of its own with the next
    /// default name, the keyboard goes to the tab that shows, the tabs are remembered for the
    /// project, a shell that exits closes its tab, and the last tab closing says so.
    #[gpui_kit::test]
    async fn tabs_are_shells_of_their_own(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let root = folder.path().canonicalize().expect("the folder exists");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            crate::platform::input::init(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            crate::terminal::test_support::trusted_folder("w1", &root, cx);
        });
        let panel_root = root.clone();
        let (panel, cx) = cx.add_window_view(|_, cx| TerminalPanel::new("w1", panel_root, cx));
        let titles = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.tab_titles());
        assert_eq!(titles(cx), [("Local".to_owned(), true)]);
        let first = panel.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a terminal");

        panel.update(cx, |panel, cx| panel.open_tab(cx));
        assert_eq!(titles(cx), [("Local".to_owned(), false), ("Local (2)".to_owned(), true)]);
        let second = panel.read_with(cx, |panel, _| panel.active_view().cloned()).expect("a terminal");
        assert_ne!(first.entity_id(), second.entity_id());
        assert_ne!(first.read_with(cx, |view, _| view.id()), second.read_with(cx, |view, _| view.id()));
        let stored =
            cx.update(|_, cx| cx.global::<UiStorage>().get(&storage_key(&root.to_string_lossy())).map(str::to_owned));
        assert!(stored.is_some_and(|tabs| tabs.contains("Local (2)")), "the tabs are remembered");

        // The keyboard given to the tool window reaches the terminal that shows.
        cx.update(|window, cx| {
            let focus = panel.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|window, cx| second.focus_handle(cx).is_focused(window)));
        let screen = |view: &Entity<TerminalView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| view.emulator().lines().join("\n"))
        };
        cx.simulate_input("echo second-$((1+1))");
        cx.simulate_keystrokes("enter");
        settle(cx, "the second shell answers", |cx| screen(&second, cx).contains("second-2"));
        assert!(!screen(&first, cx).contains("second-2"), "the first shell saw none of it");

        // A shell that exits takes its tab along (Close session when it ends); the other shows.
        cx.simulate_input("exit");
        cx.simulate_keystrokes("enter");
        settle(cx, "the tab closes", |cx| titles(cx).len() == 1);
        assert_eq!(titles(cx), [("Local".to_owned(), true)]);
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.active_view().map(|view| view.entity_id())),
            Some(first.entity_id())
        );

        let emptied: std::rc::Rc<std::cell::Cell<bool>> = Default::default();
        let seen = emptied.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &TerminalPanelEvent, _| {
                seen.set(*event == TerminalPanelEvent::Emptied)
            })
        });
        panel.update(cx, |panel, cx| panel.close_active_tab(cx));
        cx.run_until_parked();
        assert!(emptied.get() && titles(cx).is_empty());
    }

    /// Tabs remembered for a project come back with it, but only the one that shows gets a
    /// shell; another starts its shell when it is first selected.
    #[gpui_kit::test]
    async fn remembered_tabs_start_their_shells_when_first_shown(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
        let root = folder.path().canonicalize().expect("the folder exists");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            crate::platform::input::init(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            crate::terminal::test_support::trusted_folder("w1", &root, cx);
            crate::terminal::test_support::trusted_folder("w2", &root, cx);
        });
        {
            let panel_root = root.clone();
            let (panel, cx) = cx.add_window_view(|_, cx| TerminalPanel::new("w1", panel_root, cx));
            panel.update(cx, |panel, cx| {
                panel.open_tab(cx);
                panel.open_tab(cx);
            });
            assert_eq!(panel.read_with(cx, |panel, _| (panel.tab_titles().len(), panel.started())), (3, 3));
        }
        let panel_root = root.clone();
        let (panel, cx) = cx.add_window_view(|_, cx| TerminalPanel::new("w2", panel_root, cx));
        let titles: Vec<String> =
            panel.read_with(cx, |panel, _| panel.tab_titles().into_iter().map(|(title, _)| title).collect());
        assert_eq!(titles, ["Local", "Local (2)", "Local (3)"]);
        assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 1, "only the tab that shows has a shell");
        let first = panel.read_with(cx, |panel, _| panel.terminals().tabs()[0].id.clone());
        let shown = panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned));
        assert_ne!(shown.as_deref(), Some(first.as_str()), "the last tab opened was the one showing");
        panel.update(cx, |panel, cx| panel.select_tab(&first, cx));
        assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 2);
        assert!(panel.read_with(cx, |panel, _| panel.active_view().is_some()));
    }
}
