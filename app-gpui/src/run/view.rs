//! The Run tool window: a tab per run session (its status icon, its name and a close button), the
//! console of the active session beside its toolbar (Rerun, Debug, Stop, Find, Scroll to End,
//! Soft-Wrap, Clear All, Edit Configurations), and what to do when nothing has run yet.
//!
//! The console draws the session's output lines in their stream's colour and their ANSI styles,
//! with links on file locations and URLs, runs of folded lines, find matches, and the input typed
//! for the process. It follows the output while the user has not scrolled up.
//!
//! [`RunView`] owns no process. It reads the [`RunStore`], changes what is only shown there (the
//! active tab, the typed input, a cleared console) and reports what needs the process or the window
//! as [`RunViewEvent`]s.

use crate::find::utf16;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::run::ansi::{AnsiColor, SpanStyle};
use crate::run::config_types::RunConfigTypes;
use crate::run::configs::cwd_of;
use crate::run::console::{ChunkKind, ConsoleLine, ConsoleModel};
use crate::run::links::{FileTarget, OutputLink, find_output_links};
use crate::run::navigate::run_directory;
use crate::run::segments::{SegmentMark, segment_line};
use crate::run::sessions::{failed, finished_tooltip, session_icon};
use crate::run::store::{RunSession, RunStore, SessionStatus};
use crate::run_producers::console_folds::{DisplayRow, Fold, fold_rows};
use crate::theme;
use crate::ui::console_find::{FindOptions, FindResult, LineMatcher, MAX_MATCHES, find_in_lines};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FollowMode, Font,
    FontStyle, FontWeight, Hsla, InteractiveText, IntoElement, Keystroke, ListAlignment, ListState, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, StrikethroughStyle, StyledText,
    Subscription, TextRun, UnderlineStyle, Window, div, font, list, prelude::*, px,
};
use ide_core::settings::ConsoleSettings;
use ide_run::RunConfig;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Height of a console row.
const ROW: f32 = 20.0;
/// Width of the toolbar beside the console.
const TOOLBAR_WIDTH: f32 = 32.0;

/// What the Run tool window asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum RunViewEvent {
    /// Restart the session's tab.
    Rerun(String),
    /// Launch this configuration under the debugger.
    Debug(Box<RunConfig>),
    Stop(String),
    /// Close the session's tab (asking first while its process runs).
    CloseTab(String),
    /// Edit Configurations…, on the configuration with this id.
    EditConfigs(Option<String>),
    /// Send a line of input to the session's process.
    SendInput {
        session: String,
        line: String,
    },
    /// Close the session's stdin.
    SendEof(String),
    OpenUrl(String),
    /// Open a file location printed by a process running in `cwd`.
    OpenLocation {
        target: FileTarget,
        cwd: String,
    },
    /// Show the console's context menu ([`RunView::console_menu`]) here.
    ShowMenu(Point<Pixels>),
}

/// A row of the console's context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleCommand {
    Copy,
    SelectAll,
    CopyAll,
    Find,
    ClearAll,
    SendEof,
}

/// What a console row shows, to tell an appended row from a changed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKey {
    Line { id: u64, fold: Option<(u64, usize, bool)> },
    Fold { id: u64, count: usize },
    Input,
}

/// The find bar of the console.
#[derive(Default)]
struct ConsoleFind {
    open: bool,
    options: FindOptions,
    /// The current match, an index into `result.matches`.
    active: usize,
    /// The line a new search starts from: the top of the viewport when the query changed.
    anchor: usize,
    result: FindResult,
    /// The matches of each line, in UTF-16 columns.
    marks: HashMap<usize, Vec<SegmentMark>>,
}

/// Every match of `pattern` in a line, as a regular expression; `None` when it does not compile.
fn compile_regex(pattern: &str, case_sensitive: bool) -> Option<LineMatcher> {
    let regex = regex::RegexBuilder::new(pattern).case_insensitive(!case_sensitive).build().ok()?;
    Some(Box::new(move |text: &str| regex.find_iter(text).map(|found| found.range()).collect()))
}

/// The primary modifier: ⌘ on macOS, Ctrl elsewhere.
fn primary(modifiers: &Modifiers) -> bool {
    if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control }
}

/// `1234567` as `1,234,567`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The pending input without its last character, or (`word`) without its last word and the white
/// space after it.
fn erased(pending: &str, word: bool) -> String {
    if !word {
        let mut chars = pending.chars();
        chars.next_back();
        return chars.as_str().to_owned();
    }
    let trimmed = pending.trim_end();
    let cut =
        trimmed.rfind(char::is_whitespace).map_or(0, |at| at + trimmed[at..].chars().next().map_or(1, char::len_utf8));
    pending[..cut].to_owned()
}

/// A paste into the console's input: the complete lines to send, and what stays typed.
fn pasted(pending: &str, text: &str) -> (Vec<String>, String) {
    let joined = format!("{pending}{text}").replace("\r\n", "\n").replace('\r', "\n");
    let mut parts: Vec<String> = joined.split('\n').map(str::to_owned).collect();
    let rest = parts.pop().unwrap_or_default();
    (parts, rest)
}

pub struct RunView {
    store: Entity<RunStore>,
    types: Rc<RunConfigTypes>,
    /// The console's keyboard focus: typed input, ⌘F, ⌘A.
    focus: FocusHandle,
    list: ListState,
    /// The rows of the shown console.
    rows: Rc<Vec<DisplayRow>>,
    keys: Vec<RowKey>,
    /// The session whose console is listed, and the output version listed.
    shown: Option<(String, u64)>,
    /// The output of a session that emulates a terminal, as console lines.
    terminal: Option<ConsoleModel>,
    /// Scroll to End: the console follows new output; scrolling up turns it off, back to the end on.
    follow: bool,
    /// The user scrolled since Scroll to End was last set: the list's own following decides then.
    user_scrolled: bool,
    /// The Soft-Wrap toggle of this tab; `None` follows Settings › Console.
    wrap_override: Option<bool>,
    /// Folds the user opened, by the id of their first line.
    expanded: HashSet<u64>,
    /// Select All: the whole buffer is selected.
    all_selected: bool,
    find: ConsoleFind,
    field: Entity<TextField>,
    console_settings: ConsoleSettings,
    project_root: Option<String>,
    debug_available: bool,
    edit_available: bool,
    /// The shortcut of the configuration chooser, shown in the empty state.
    chooser_shortcut: Option<String>,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RunViewEvent> for RunView {}

impl RunView {
    pub fn new(store: Entity<RunStore>, types: Rc<RunConfigTypes>, cx: &mut Context<Self>) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Find in console");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let list = ListState::new(0, ListAlignment::Top, px(400.0));
        list.set_follow_mode(FollowMode::Tail);
        let scrolled = cx.weak_entity();
        list.set_scroll_handler(move |event, _, cx| {
            let following = event.is_following_tail;
            let _ = scrolled.update(cx, |view, cx| view.scrolled(following, cx));
        });
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.observe(&store, |view, _, cx| {
                view.refresh(cx);
                cx.notify();
            }),
            cx.subscribe(&field, |view, _, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    view.search(true, cx);
                    cx.notify();
                }
            }),
        ];
        let console = cx.weak_entity();
        let console_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                console.update(cx, |view, cx| view.console_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let bar = cx.weak_entity();
        let field_focus = field.read(cx).focus_handle(cx);
        let field_claim = input::claim_keys(
            field_focus,
            move |keystroke, window, cx| {
                bar.update(cx, |view, cx| view.find_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| {
            for claim in &view.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        let mut view = Self {
            store,
            types,
            focus,
            list,
            rows: Rc::default(),
            keys: Vec::new(),
            shown: None,
            terminal: None,
            follow: true,
            user_scrolled: false,
            wrap_override: None,
            expanded: HashSet::new(),
            all_selected: false,
            find: ConsoleFind::default(),
            field,
            console_settings: ConsoleSettings::default(),
            project_root: None,
            debug_available: false,
            edit_available: false,
            chooser_shortcut: None,
            key_claims: vec![console_claim, field_claim],
            _subscriptions: subscriptions,
        };
        view.refresh(cx);
        view
    }

    /// Settings › Console: the default soft wrap and the fold patterns.
    pub fn set_console_settings(&mut self, settings: ConsoleSettings, cx: &mut Context<Self>) {
        if self.console_settings != settings {
            self.console_settings = settings;
            self.keys.clear();
            self.refresh(cx);
            cx.notify();
        }
    }

    /// The project's folder: relative paths in the output resolve against the run directory in it.
    pub fn set_project_root(&mut self, root: Option<String>) {
        self.project_root = root;
    }

    /// What the window can do for the toolbar: Debug and Edit Configurations have a handler, and the
    /// shortcut of the configuration chooser.
    pub fn set_capabilities(
        &mut self,
        debug: bool,
        edit_configs: bool,
        chooser: Option<String>,
        cx: &mut Context<Self>,
    ) {
        (self.debug_available, self.edit_available, self.chooser_shortcut) = (debug, edit_configs, chooser);
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // What is shown

    /// The selected tab: the active session, else the last one.
    fn current<'a>(&self, store: &'a RunStore) -> Option<&'a RunSession> {
        let active = store.active_session.as_deref().and_then(|id| store.session(id));
        active.or_else(|| store.sessions.last())
    }

    /// The session of the selected tab.
    pub fn active_tab(&self, cx: &App) -> Option<String> {
        self.current(self.store.read(cx)).map(|session| session.id.clone())
    }

    /// The tabs' names, in order.
    pub fn tab_titles(&self, cx: &App) -> Vec<String> {
        self.store.read(cx).sessions.iter().map(|session| session.config.name.clone()).collect()
    }

    /// The tabs' status icons, in order.
    pub fn tab_icons(&self, cx: &App) -> Vec<String> {
        self.store.read(cx).sessions.iter().map(|session| session_icon(session, &self.types)).collect()
    }

    /// The tooltip of each tab: how long a finished run took and its exit code.
    pub fn tab_tooltips(&self, cx: &mut App) -> Vec<Option<String>> {
        let sessions = self.store.read(cx).sessions.clone();
        let i18n = &mut cx.default_global::<Localization>().0;
        sessions.iter().map(|session| finished_tooltip(session, i18n)).collect()
    }

    fn console<'a>(&'a self, store: &'a RunStore) -> Option<&'a ConsoleModel> {
        if self.terminal.is_some() {
            return self.terminal.as_ref();
        }
        self.shown.as_ref().and_then(|(id, _)| store.console(id))
    }

    /// What the console shows, row by row: each line's text (with the `<N internal lines>` of a fold
    /// after it), a fold's placeholder, and the input typed for the process (on a row of its own
    /// unless it continues a prompt; an empty one is only a caret).
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let store = self.store.read(cx);
        let Some(console) = self.console(store) else { return Vec::new() };
        let pending = self.pending(store);
        let inline = self.input_inline(store);
        let mut out = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let text = match row {
                DisplayRow::Line { index: line, fold } => {
                    let mut text = console.lines().get(*line).map_or_else(String::new, |line| line.text.clone());
                    if let Some(fold) = fold.filter(|fold| !fold.expanded) {
                        text.push(' ');
                        text.push_str(&fold_label(fold, cx));
                    }
                    if inline && index + 1 == self.rows.len() {
                        text.push_str(pending.unwrap_or_default());
                    }
                    text
                }
                DisplayRow::Fold(fold) => fold_label(*fold, cx),
            };
            out.push(text);
        }
        if let Some(pending) = pending.filter(|pending| !inline && !pending.is_empty()) {
            out.push(pending.to_owned());
        }
        out
    }

    /// The stream of each listed line.
    pub fn line_kinds(&self, cx: &App) -> Vec<ChunkKind> {
        let store = self.store.read(cx);
        let Some(console) = self.console(store) else { return Vec::new() };
        let line = |row: &DisplayRow| match row {
            DisplayRow::Line { index, .. } => console.lines().get(*index).map(|line| line.kind),
            DisplayRow::Fold(_) => None,
        };
        self.rows.iter().filter_map(line).collect()
    }

    /// The notice over the console: how many earlier lines the cycle buffer discarded.
    pub fn notice(&self, cx: &mut App) -> Option<String> {
        let dropped = self.console(self.store.read(cx)).map_or(0, ConsoleModel::dropped);
        if dropped == 0 {
            return None;
        }
        let count = grouped(dropped);
        Some(
            cx.default_global::<Localization>()
                .0
                .tx_built(&["", " earlier lines were discarded"], &[Param::from(count.as_str())]),
        )
    }

    /// The empty state's lines, while nothing has run.
    pub fn empty_text(&self, cx: &App) -> Option<Vec<String>> {
        if !self.store.read(cx).sessions.is_empty() {
            return None;
        }
        let launch = format!("{} {}", tx(cx, "Launch a"), tx(cx, "run configuration"));
        let launch = match &self.chooser_shortcut {
            Some(shortcut) => format!("{launch} ({shortcut})"),
            None => launch,
        };
        Some(vec![
            tx(cx, "To run your code, do one of the following:"),
            tx(cx, "Click the Run icon in the editor gutter"),
            tx(cx, "Select \"Run…\" in the editor context menu"),
            launch,
        ])
    }

    /// The output follows new lines (Scroll to End is on).
    pub fn follows(&self) -> bool {
        if self.user_scrolled { self.list.is_following_tail() } else { self.follow }
    }

    /// Scroll to End on or off. Off, the list stays where it is and follows again when the user
    /// scrolls back to the end.
    fn set_follow(&mut self, follow: bool) {
        (self.follow, self.user_scrolled) = (follow, false);
        if follow {
            self.list.set_follow_mode(FollowMode::Tail);
        } else {
            self.list.pause_following_tail();
        }
    }

    /// The user scrolled: the list stops following the tail when scrolled up, and follows it again
    /// once back at the end (which it may only notice when it is next drawn, see `render`).
    fn scrolled(&mut self, following: bool, cx: &mut Context<Self>) {
        self.user_scrolled = true;
        if following != self.follow {
            self.follow = following;
            cx.notify();
        }
    }

    /// Long lines wrap: the tab's Soft-Wrap toggle, else Settings › Console › Use soft wraps.
    pub fn soft_wrap(&self) -> bool {
        self.wrap_override.unwrap_or(self.console_settings.soft_wrap)
    }

    pub fn find_open(&self) -> bool {
        self.find.open
    }

    /// The find bar's query field.
    pub fn find_field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// How many matches the find bar highlights.
    pub fn match_count(&self) -> usize {
        if self.find.open { self.find.result.matches.len() } else { 0 }
    }

    /// The find bar's counter: `2/3`, `0 results`, `Bad regex`, or nothing without a query.
    pub fn find_counter(&self, cx: &App) -> String {
        let result = &self.find.result;
        if result.invalid {
            return tx(cx, "Bad regex");
        }
        if self.field.read(cx).text().is_empty() {
            return String::new();
        }
        if result.matches.is_empty() {
            return tx(cx, "0 results");
        }
        let more = if result.overflow { "+" } else { "" };
        format!("{}/{}{more}", self.find.active + 1, result.matches.len())
    }

    /// The input typed for the process of the shown session, while it takes input.
    pub fn pending_input(&self, cx: &App) -> Option<String> {
        self.pending(self.store.read(cx)).map(str::to_owned)
    }

    fn pending<'a>(&self, store: &'a RunStore) -> Option<&'a str> {
        let (id, _) = self.shown.as_ref()?;
        let session = store.session(id)?;
        (session.accepts_input() && !session.terminal).then(|| store.pending_input(id))
    }

    /// The pending input continues the open line (a prompt) rather than starting a row of its own.
    fn input_inline(&self, store: &RunStore) -> bool {
        let Some(console) = self.console(store).filter(|_| self.pending(store).is_some()) else { return false };
        let Some(open) = console.open_line() else { return false };
        let last = match self.rows.last() {
            Some(DisplayRow::Line { index, fold: None }) => console.lines().get(*index),
            _ => None,
        };
        last.is_some_and(|line| line.id == open.id)
    }

    fn input_row(&self, store: &RunStore) -> bool {
        self.pending(store).is_some() && !self.input_inline(store)
    }

    /// Whether Stop can stop the shown session: its process is alive.
    pub fn can_stop(&self, cx: &App) -> bool {
        self.current(self.store.read(cx)).is_some_and(RunSession::is_live)
    }

    /// The Stop button's title: a second Stop while stopping kills the process.
    pub fn stop_title(&self, cx: &App) -> String {
        let stopping = self.current(self.store.read(cx)).is_some_and(|s| s.status == SessionStatus::Stopping);
        tx(cx, if stopping { "Stopping… (click to kill)" } else { "Stop" })
    }

    // ---------------------------------------------------------------------------------------------
    // Keeping up with the store

    /// Brings the rows in line with the shown session's console; another tab starts afresh.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let current = self.current(store).map(|session| {
            let version = store.versions.get(&session.id).copied().unwrap_or(0);
            (session.id.clone(), version, session.terminal)
        });
        let switched = current.as_ref().map(|(id, _, _)| id) != self.shown.as_ref().map(|(id, _)| id);
        if switched {
            self.wrap_override = None;
            self.expanded.clear();
            self.all_selected = false;
            self.find = ConsoleFind::default();
            self.keys.clear();
            self.list.reset(0);
            self.set_follow(true);
        }
        let Some((id, version, terminal)) = current else {
            (self.shown, self.terminal, self.rows) = (None, None, Rc::default());
            return;
        };
        // A process that starts or stops taking input adds or removes the input row.
        let input_listed = self.keys.last() == Some(&RowKey::Input);
        let unchanged = self.shown.as_ref().is_some_and(|(_, shown)| *shown == version) && !self.keys.is_empty();
        if !switched && unchanged && input_listed == self.input_row(store) {
            return;
        }
        self.terminal = terminal.then(|| {
            let mut console = ConsoleModel::default();
            if let Some(output) = store.terminal(&id) {
                console.append(ChunkKind::Stdout, &output.text());
            }
            console
        });
        self.shown = Some((id, version));
        self.relist(cx);
        if self.find.open {
            self.search(false, cx);
        }
    }

    /// Recomputes the rows and tells the list which changed: those after the longest unchanged start
    /// (the last of it too, since the open line grows in place).
    fn relist(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let rows = match self.console(store) {
            Some(console) => fold_rows(console.lines(), &self.console_settings.fold_patterns, &self.expanded),
            None => Vec::new(),
        };
        let console = self.console(store);
        let line_id = |index: usize| console.and_then(|c| c.lines().get(index)).map_or(0, |line| line.id);
        let mut keys: Vec<RowKey> = rows
            .iter()
            .map(|row| match row {
                DisplayRow::Line { index, fold } => {
                    RowKey::Line { id: line_id(*index), fold: fold.map(|fold| (fold.id, fold.count, fold.expanded)) }
                }
                DisplayRow::Fold(fold) => RowKey::Fold { id: fold.id, count: fold.count },
            })
            .collect();
        self.rows = Rc::new(rows);
        if self.input_row(store) {
            keys.push(RowKey::Input);
        }
        let same = self.keys.iter().zip(&keys).take_while(|(a, b)| a == b).count();
        let from = same.saturating_sub(1);
        if self.keys.is_empty() {
            self.list.reset(keys.len());
        } else if from < self.keys.len() || keys.len() > self.keys.len() {
            self.list.splice(from..self.keys.len(), keys.len() - from);
        }
        self.keys = keys;
    }

    // ---------------------------------------------------------------------------------------------
    // Tabs and toolbar

    /// Shows the session's tab.
    pub fn select_tab(&mut self, session: &str, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            if store.session(session).is_some() {
                store.active_session = Some(session.to_owned());
                cx.notify();
            }
        });
    }

    pub fn close_tab(&mut self, session: &str, cx: &mut Context<Self>) {
        cx.emit(RunViewEvent::CloseTab(session.to_owned()));
    }

    fn shown_id(&self) -> Option<String> {
        self.shown.as_ref().map(|(id, _)| id.clone())
    }

    pub fn rerun(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.shown_id() {
            cx.emit(RunViewEvent::Rerun(id));
        }
    }

    /// Debug: the session's original configuration, launched under the debugger.
    pub fn debug_session(&mut self, session: &str, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let Some(found) = store.session(session) else { return };
        let config = store.base_config_of(found).clone();
        cx.emit(RunViewEvent::Debug(Box::new(config)));
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.shown_id().filter(|_| self.can_stop(cx)) {
            cx.emit(RunViewEvent::Stop(id));
        }
    }

    /// Scroll to End: follow the output again, or stop following it.
    pub fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        self.set_follow(!self.follow);
        cx.notify();
    }

    pub fn toggle_soft_wrap(&mut self, cx: &mut Context<Self>) {
        self.wrap_override = Some(!self.soft_wrap());
        self.list.remeasure();
        cx.notify();
    }

    /// Clear All: empties the shown console.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.shown_id() else { return };
        self.all_selected = false;
        self.store.update(cx, |store, cx| {
            store.clear_console(&id);
            cx.notify();
        });
    }

    pub fn edit_configs(&mut self, cx: &mut Context<Self>) {
        let config = self.current(self.store.read(cx)).map(|session| session.config.id.clone());
        cx.emit(RunViewEvent::EditConfigs(config));
    }

    fn toggle_fold(&mut self, id: u64, cx: &mut Context<Self>) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        self.relist(cx);
        if self.find.open {
            self.search(false, cx);
        }
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Find

    /// Opens the find bar with the keyboard in its field; the output stops following.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.anchor = self.top_line();
        self.find.open = true;
        self.set_follow(false);
        let focus = self.field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.field.update(cx, |field, cx| field.select_all(cx));
        self.search(true, cx);
        cx.notify();
    }

    /// Closes the find bar, clearing its highlights; the keyboard goes back to the console.
    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        self.find.result = FindResult::default();
        self.find.marks.clear();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The toolbar's Find button.
    pub fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.open { self.close_find(window, cx) } else { self.open_find(window, cx) }
    }

    pub fn toggle_find_option(&mut self, option: FindOption, cx: &mut Context<Self>) {
        let flag = match option {
            FindOption::MatchCase => &mut self.find.options.case_sensitive,
            FindOption::Words => &mut self.find.options.whole_word,
            FindOption::Regex => &mut self.find.options.regex,
        };
        *flag = !*flag;
        self.find.anchor = self.top_line();
        self.search(true, cx);
        cx.notify();
    }

    /// The line at the top of the viewport: a new search starts from there, as IDEA does.
    fn top_line(&self) -> usize {
        let top = self.list.logical_scroll_top().item_ix;
        match self.rows.get(top) {
            Some(DisplayRow::Line { index, .. }) => *index,
            Some(DisplayRow::Fold(fold)) => fold.start,
            None => 0,
        }
    }

    /// Searches the whole buffer. `restart`: the query or an option changed, so the current match is
    /// the first one from the anchor; otherwise new output keeps it.
    fn search(&mut self, restart: bool, cx: &mut Context<Self>) {
        if !self.find.open {
            return;
        }
        let query = self.field.read(cx).text().to_owned();
        let store = self.store.read(cx);
        let result = match self.console(store) {
            Some(console) => {
                let lines = console.lines().iter().map(|line| (line.id, line.text.as_str()));
                find_in_lines(lines, &query, self.find.options, MAX_MATCHES, &compile_regex)
            }
            None => FindResult::default(),
        };
        let count = result.matches.len();
        self.find.active = if restart {
            result.matches.iter().position(|found| found.line >= self.find.anchor).unwrap_or(0)
        } else {
            self.find.active.min(count.saturating_sub(1))
        };
        self.find.result = result;
        self.mark_matches(cx);
        if restart {
            self.reveal_match(cx);
        }
    }

    fn mark_matches(&mut self, cx: &App) {
        let store = self.store.read(cx);
        let mut marks: HashMap<usize, Vec<SegmentMark>> = HashMap::new();
        if let Some(console) = self.console(store) {
            for (i, found) in self.find.result.matches.iter().enumerate() {
                let Some(line) = console.lines().get(found.line) else { continue };
                let column = |byte: usize| utf16::len(&line.text[..byte]);
                let mark =
                    SegmentMark { start: column(found.start), end: column(found.end), current: i == self.find.active };
                marks.entry(found.line).or_default().push(mark);
            }
        }
        self.find.marks = marks;
    }

    /// Moves to the next (`forward`) or previous match, wrapping around.
    pub fn go_to_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.find.result.matches.len();
        if count == 0 {
            return;
        }
        self.find.active =
            if forward { (self.find.active + 1) % count } else { (self.find.active + count - 1) % count };
        self.mark_matches(cx);
        self.reveal_match(cx);
        cx.notify();
    }

    /// Brings the current match into view, opening a fold that hides it.
    fn reveal_match(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.find.result.matches.get(self.find.active).map(|found| found.line) else { return };
        let row = self.rows.iter().position(|row| matches!(row, DisplayRow::Line { index, .. } if *index == line));
        if let Some(row) = row {
            self.list.scroll_to_reveal_item(row);
            return;
        }
        let hidden = self.rows.iter().find_map(|row| {
            let fold = match row {
                DisplayRow::Line { fold, .. } => (*fold)?,
                DisplayRow::Fold(fold) => *fold,
            };
            (line >= fold.start && line < fold.start + fold.count).then_some(fold.id)
        });
        if let Some(id) = hidden {
            self.expanded.insert(id);
            self.relist(cx);
            if let Some(row) =
                self.rows.iter().position(|row| matches!(row, DisplayRow::Line { index, .. } if *index == line))
            {
                self.list.scroll_to_reveal_item(row);
            }
        }
    }

    /// The find bar's own keys while its field has the keyboard: Escape closes it, Enter / F3 / ⌘G
    /// go to the next match (Shift: the previous one), Alt+C / W / X toggle an option.
    fn find_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let key = keystroke.key.as_str();
        if key == "escape" && !modifiers.modified() {
            self.close_find(window, cx);
        } else if key == "enter" || key == "f3" || (primary(modifiers) && key == "g") {
            self.go_to_match(!modifiers.shift, cx);
        } else if primary(modifiers) && !modifiers.shift && key == "f" {
            self.field.update(cx, |field, cx| field.select_all(cx));
        } else if modifiers.alt && !modifiers.platform && !modifiers.control && matches!(key, "c" | "w" | "x") {
            let option = match key {
                "c" => FindOption::MatchCase,
                "w" => FindOption::Words,
                _ => FindOption::Regex,
            };
            self.toggle_find_option(option, cx);
        } else {
            return false;
        }
        true
    }

    // ---------------------------------------------------------------------------------------------
    // The console's keys, input and menu

    /// The keys of the console: ⌘F finds, ⌘A selects all and ⌘C copies it; while the process takes
    /// input, typing edits the pending line, Enter sends it, ⌃D sends EOF and ⌘V pastes.
    fn console_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let key = keystroke.key.as_str();
        let plain_primary = primary(modifiers) && !modifiers.alt && !modifiers.shift;
        if plain_primary && key == "f" {
            self.open_find(window, cx);
            return true;
        }
        if plain_primary && key == "a" {
            self.select_all(cx);
            return true;
        }
        if plain_primary && key == "c" {
            self.copy(cx);
            return true;
        }
        let Some(id) = self.shown_id() else { return false };
        let Some(pending) = self.pending(self.store.read(cx)).map(str::to_owned) else { return false };
        if key == "d" && modifiers.control && !modifiers.platform && !modifiers.alt {
            cx.emit(RunViewEvent::SendEof(id));
        } else if key == "enter" && !modifiers.control && !modifiers.platform && !modifiers.alt {
            self.set_pending(&id, "", cx);
            cx.emit(RunViewEvent::SendInput { session: id, line: pending });
        } else if key == "backspace" && !modifiers.control && !modifiers.platform {
            self.set_pending(&id, &erased(&pending, modifiers.alt), cx);
        } else if plain_primary && key == "v" {
            let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return true };
            self.paste(&id, &pending, &text, cx);
        } else if !modifiers.control && !modifiers.platform {
            let typed = keystroke.key_char.clone().or_else(|| (key.chars().count() == 1).then(|| key.to_owned()));
            let Some(typed) = typed.filter(|typed| typed.chars().count() == 1 && !typed.chars().any(char::is_control))
            else {
                return false;
            };
            self.set_pending(&id, &format!("{pending}{typed}"), cx);
        } else {
            return false;
        }
        true
    }

    fn set_pending(&mut self, session: &str, text: &str, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            store.set_pending_input(session, text);
            cx.notify();
        });
    }

    /// A paste sends its complete lines and keeps the rest typed.
    fn paste(&mut self, session: &str, pending: &str, text: &str, cx: &mut Context<Self>) {
        let (lines, rest) = pasted(pending, text);
        for line in lines {
            cx.emit(RunViewEvent::SendInput { session: session.to_owned(), line });
        }
        self.set_pending(session, &rest, cx);
    }

    /// Select All: the whole buffer, including the rows that are not drawn.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.all_selected = true;
        cx.notify();
    }

    /// Copy: after Select All, the whole buffer.
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if self.all_selected {
            self.copy_all(cx);
        }
    }

    pub fn copy_all(&mut self, cx: &mut Context<Self>) {
        let text = self.console(self.store.read(cx)).map(ConsoleModel::text).unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// The console's context menu.
    pub fn console_menu(&self, cx: &App) -> Vec<MenuItem<ConsoleCommand>> {
        let primary = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        let mut items = vec![
            MenuItem::item("Copy", ConsoleCommand::Copy)
                .icon("copy")
                .shortcut(format!("{primary}C"))
                .disabled(!self.all_selected),
            MenuItem::item("Select All", ConsoleCommand::SelectAll).shortcut(format!("{primary}A")),
            MenuItem::item("Copy All", ConsoleCommand::CopyAll),
            MenuItem::item("Find…", ConsoleCommand::Find).icon("search").shortcut(format!("{primary}F")),
            MenuItem::Separator,
            MenuItem::item("Clear All", ConsoleCommand::ClearAll).icon("trash").disabled(self.shown.is_none()),
        ];
        if self.pending(self.store.read(cx)).is_some() {
            items.push(MenuItem::Separator);
            items.push(MenuItem::item("Send EOF", ConsoleCommand::SendEof).shortcut("⌃D"));
        }
        items
    }

    pub fn run_console_command(&mut self, command: ConsoleCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            ConsoleCommand::Copy => self.copy(cx),
            ConsoleCommand::SelectAll => self.select_all(cx),
            ConsoleCommand::CopyAll => self.copy_all(cx),
            ConsoleCommand::Find => self.open_find(window, cx),
            ConsoleCommand::ClearAll => self.clear(cx),
            ConsoleCommand::SendEof => {
                if let Some(id) = self.shown_id() {
                    cx.emit(RunViewEvent::SendEof(id));
                }
            }
        }
    }

    fn open_link(&mut self, link: &OutputLink, cx: &mut Context<Self>) {
        match link {
            OutputLink::Url { url, .. } => cx.emit(RunViewEvent::OpenUrl(url.clone())),
            OutputLink::File { target, .. } => {
                let store = self.store.read(cx);
                let cwd = self.current(store).and_then(|session| cwd_of(&session.config).map(str::to_owned));
                let cwd = run_directory(self.project_root.as_deref(), cwd.as_deref());
                cx.emit(RunViewEvent::OpenLocation { target: target.clone(), cwd });
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn mono_font(cx: &App) -> Font {
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
        font(SharedString::from(family))
    }

    fn ansi(cx: &App, color: AnsiColor) -> Hsla {
        match color {
            AnsiColor::Named(n) => {
                const NAMES: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
                let bright = if n >= 8 { "bright-" } else { "" };
                theme::color(cx, &format!("ansi-{bright}{}", NAMES[usize::from(n % 8)]))
            }
            AnsiColor::Rgb(r, g, b) => {
                gpui_kit::Rgba { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a: 1.0 }
                    .into()
            }
        }
    }

    /// The colour of a stream's text: stderr in the danger colour mixed with the text colour.
    fn stream_color(kind: ChunkKind, cx: &App) -> Hsla {
        match kind {
            ChunkKind::Stdout => theme::color(cx, "fg"),
            ChunkKind::Stderr => {
                let (danger, fg) = (theme::color(cx, "danger").to_rgb(), theme::color(cx, "fg").to_rgb());
                let mix = |a: f32, b: f32| a * 0.88 + b * 0.12;
                gpui_kit::Rgba { r: mix(danger.r, fg.r), g: mix(danger.g, fg.g), b: mix(danger.b, fg.b), a: 1.0 }.into()
            }
            ChunkKind::System => theme::color(cx, "fg-subtle"),
        }
    }

    /// A line's text as styled runs, and the byte ranges of its links with what they open.
    fn styled_line(
        &self,
        line: &ConsoleLine,
        marks: &[SegmentMark],
        cx: &App,
    ) -> (StyledText, Vec<(std::ops::Range<usize>, OutputLink)>) {
        let links = if line.kind == ChunkKind::System { Vec::new() } else { find_output_links(&line.text) };
        let segments = segment_line(&line.spans, &links, marks);
        let base = Self::mono_font(cx);
        let base = if line.kind == ChunkKind::System { Font { style: FontStyle::Italic, ..base } } else { base };
        let stream = Self::stream_color(line.kind, cx);
        let mut text = String::new();
        let mut runs = Vec::new();
        let mut clickable = Vec::new();
        for segment in &segments {
            let start = text.len();
            text.push_str(&segment.text);
            let style: SpanStyle = segment.style;
            let mut color = style.fg.map_or(stream, |fg| Self::ansi(cx, fg));
            let mut background = style.bg.map(|bg| Self::ansi(cx, bg));
            if style.dim {
                color.a *= 0.6;
            }
            let mut underline =
                style.underline.then(|| UnderlineStyle { thickness: px(1.0), color: None, wavy: false });
            if segment.link.is_some() {
                color = theme::color(cx, "link");
                underline = Some(UnderlineStyle { thickness: px(1.0), color: None, wavy: false });
            }
            if segment.mark {
                background =
                    Some(theme::color(cx, if segment.current { "bg-search-match-current" } else { "bg-search-match" }));
                color = theme::color(cx, "fg-search-match");
            }
            let font = Font {
                weight: if style.bold { FontWeight::BOLD } else { base.weight },
                style: if style.italic { FontStyle::Italic } else { base.style },
                ..base.clone()
            };
            let strikethrough = style.strike.then(|| StrikethroughStyle { thickness: px(1.0), color: None });
            runs.push(TextRun {
                len: segment.text.len(),
                font,
                color,
                background_color: background,
                underline,
                strikethrough,
            });
            if let Some(link) = segment.link {
                clickable.push((start..text.len(), link.clone()));
            }
        }
        if text.is_empty() {
            // An empty line keeps its height.
            text.push(' ');
            runs.push(TextRun {
                len: 1,
                font: base,
                color: stream,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
        (StyledText::new(text).with_runs(runs), clickable)
    }

    fn fold_button(&self, fold: Fold, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = fold.id;
        if fold.expanded {
            return div()
                .id(SharedString::from(format!("run-fold-collapse-{id}")))
                .ml(px(6.0))
                .px(px(2.0))
                .rounded(px(3.0))
                .flex()
                .items_center()
                .child(theme::icon(cx, "chevron-up", 10.0, theme::color(cx, "fg-faint")))
                .on_click(cx.listener(move |view, _, _, cx| view.toggle_fold(id, cx)));
        }
        div()
            .id(SharedString::from(format!("run-fold-{id}")))
            .px(px(6.0))
            .rounded(px(3.0))
            .bg(theme::color(cx, "bg-fold-placeholder"))
            .text_color(theme::color(cx, "fg-fold-placeholder"))
            .hover(|el| el.text_color(theme::color(cx, "fg")))
            .child(SharedString::from(fold_label(fold, cx)))
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_fold(id, cx)))
    }

    fn pending_element(&self, pending: &str, window: &Window, cx: &App) -> Div {
        let focused = self.focus.is_focused(window);
        div()
            .flex()
            .items_center()
            .text_color(theme::color(cx, "ansi-green"))
            .child(SharedString::from(pending.to_owned()))
            .child(div().w(px(1.0)).h(px(15.0)).ml(px(1.0)).when(focused, |el| el.bg(theme::color(cx, "fg"))))
    }

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let wrap = self.soft_wrap();
        let row_div = |selector: String| {
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .w_full()
                .min_h(px(ROW))
                .px(px(12.0))
                .flex()
                .flex_row()
                .when(!wrap, |el| el.whitespace_nowrap().overflow_hidden())
                .when(self.all_selected, |el| el.bg(theme::color(cx, "bg-editor-selection")))
        };
        let rows = Rc::clone(&self.rows);
        let store = self.store.read(cx);
        let pending = self.pending(store).map(str::to_owned);
        let inline = self.input_inline(store);
        let Some(row) = rows.get(ix) else {
            return row_div("run-input".to_owned())
                .children(pending.map(|pending| self.pending_element(&pending, window, cx)))
                .into_any_element();
        };
        match row {
            DisplayRow::Fold(fold) => {
                row_div(format!("run-row-fold-{}", fold.id)).child(self.fold_button(*fold, cx)).into_any_element()
            }
            DisplayRow::Line { index, fold } => {
                let line = self.console(store).and_then(|console| console.lines().get(*index)).cloned();
                let Some(line) = line else { return div().into_any_element() };
                let marks = self.find.marks.get(index).cloned().unwrap_or_default();
                let (styled, links) = self.styled_line(&line, &marks, cx);
                let weak = cx.weak_entity();
                let ranges: Vec<std::ops::Range<usize>> = links.iter().map(|(range, _)| range.clone()).collect();
                let targets: Vec<OutputLink> = links.into_iter().map(|(_, link)| link).collect();
                let text = InteractiveText::new(SharedString::from(format!("run-text-{}", line.id)), styled).on_click(
                    ranges,
                    move |clicked, _, cx| {
                        if let Some(link) = targets.get(clicked).cloned() {
                            let _ = weak.update(cx, |view, cx| view.open_link(&link, cx));
                        }
                    },
                );
                let last = ix + 1 == rows.len();
                row_div(format!("run-line-{index}"))
                    .child(div().when(wrap, |el| el.flex_1().min_w_0()).child(text))
                    .children(fold.map(|fold| div().pl(px(4.0)).child(self.fold_button(fold, cx))))
                    .children(
                        pending.filter(|_| inline && last).map(|pending| self.pending_element(&pending, window, cx)),
                    )
                    .into_any_element()
            }
        }
    }

    fn tool_button(
        &self,
        id: &'static str,
        icon: &'static str,
        state: ButtonState,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(
            cx,
            if !state.enabled {
                "fg-faint"
            } else if state.danger {
                "danger"
            } else if state.pressed {
                "fg-strong"
            } else {
                "fg-muted"
            },
        );
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(state.pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(state.enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |view, _, window, cx| on_click(view, window, cx)))
            })
            .child(theme::icon(cx, icon, 14.0, color))
    }

    fn render_toolbar(&self, session: &RunSession, cx: &mut Context<Self>) -> Div {
        let live = session.is_live();
        let rule = theme::color(cx, "border-strong");
        let separator = || div().w(px(16.0)).h(px(1.0)).my(px(3.0)).bg(rule);
        let on = ButtonState { enabled: true, pressed: false, danger: false };
        let terminal = session.terminal;
        let id = session.id.clone();
        let stop_tooltip =
            if session.status == SessionStatus::Stopping { "Stopping… (click to kill)" } else { "Stop" };
        let mac = crate::platform::keymap::is_mac_host();
        let find_keys =
            crate::platform::keymap::format_binding(&crate::platform::keymap::host_binding("Meta+F", mac), mac);
        div()
            .w(px(TOOLBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.0))
            .py(px(4.0))
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(
                self.tool_button("run-rerun", "rerun", on, cx, |view, _, cx| view.rerun(cx))
                    .with_tooltip_shortcut("Rerun", "run.rerun"),
            )
            .child(
                self.tool_button(
                    "run-debug",
                    "debug",
                    ButtonState { enabled: self.debug_available, ..on },
                    cx,
                    move |view, _, cx| view.debug_session(&id, cx),
                )
                .with_tooltip_shortcut("Debug", "run.debug"),
            )
            .child(
                self.tool_button(
                    "run-stop",
                    "stop",
                    ButtonState { enabled: live, danger: live, ..on },
                    cx,
                    |view, _, cx| view.stop(cx),
                )
                .with_tooltip(stop_tooltip),
            )
            .child(separator())
            .when(!terminal, |el| {
                el.child(
                    self.tool_button(
                        "run-find",
                        "search",
                        ButtonState { pressed: self.find.open, ..on },
                        cx,
                        |view, window, cx| view.toggle_find(window, cx),
                    )
                    // The console's own ⌘F / Ctrl+F, which no action binds.
                    .with_tooltip_keys("Find…", find_keys),
                )
                .child(
                    self.tool_button(
                        "run-scroll-to-end",
                        "arrow-down",
                        ButtonState { pressed: self.follows(), ..on },
                        cx,
                        |view, _, cx| view.toggle_follow(cx),
                    )
                    .with_tooltip("Scroll to End"),
                )
                .child(
                    self.tool_button(
                        "run-soft-wrap",
                        "soft-wrap",
                        ButtonState { pressed: self.soft_wrap(), ..on },
                        cx,
                        |view, _, cx| view.toggle_soft_wrap(cx),
                    )
                    .with_tooltip("Soft-Wrap"),
                )
            })
            .child(
                self.tool_button("run-clear", "trash", on, cx, |view, _, cx| view.clear(cx)).with_tooltip("Clear All"),
            )
            .child(separator())
            .child(
                self.tool_button(
                    "run-edit-configs",
                    "settings",
                    ButtonState { enabled: self.edit_available, ..on },
                    cx,
                    |view, _, cx| view.edit_configs(cx),
                )
                .with_tooltip("Edit Configurations…"),
            )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> Div {
        let store = self.store.read(cx);
        let active = self.current(store).map(|session| session.id.clone());
        let tabs: Vec<(usize, String, String, String, bool)> = store
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.id.clone(), s.config.name.clone(), session_icon(s, &self.types), failed(s)))
            .collect();
        let mut row = div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .overflow_hidden()
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        for (index, id, name, icon, failed) in tabs {
            let selected = active.as_deref() == Some(id.as_str());
            let color = theme::color(cx, if failed { "danger" } else { "fg" });
            let (select, close) = (id.clone(), id.clone());
            let tab_selector = format!("run-tab-{index}");
            let close_selector = format!("run-tab-close-{index}");
            row = row.child(
                div()
                    .id(SharedString::from(tab_selector.clone()))
                    .debug_selector(move || tab_selector.clone())
                    .h(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(6.0))
                    .pr(px(2.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(color)
                    .when(selected, |el| el.bg(theme::color(cx, "bg-tab-selected")))
                    .when(!selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(theme::icon(cx, &icon, 14.0, color))
                    .child(SharedString::from(name))
                    .on_click(cx.listener(move |view, _, _, cx| view.select_tab(&select, cx)))
                    .child(
                        div()
                            .id(SharedString::from(close_selector.clone()))
                            .debug_selector(move || close_selector.clone())
                            .with_tooltip("Close Tab")
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.0))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-muted")))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.stop_propagation();
                                view.close_tab(&close, cx);
                            })),
                    ),
            );
        }
        row
    }

    fn render_find_bar(&self, cx: &mut Context<Self>) -> Div {
        let (muted, strong) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-strong"));
        let toggle = |id: &'static str, label: &'static str, on: bool, option: FindOption, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .flex_none()
                .px(px(5.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .text_color(if on { strong } else { muted })
                .when(on, |el| el.bg(theme::color(cx, "bg-selected")))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(label)
                .on_click(cx.listener(move |view, _, _, cx| view.toggle_find_option(option, cx)))
        };
        let none =
            self.find.result.invalid || (!self.field.read(cx).text().is_empty() && self.find.result.matches.is_empty());
        let counter = self.find_counter(cx);
        let has_matches = !self.find.result.matches.is_empty();
        let arrow = |id: &'static str, icon: &'static str, forward: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .when(has_matches, |el| {
                    el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .on_click(cx.listener(move |view, _, _, cx| view.go_to_match(forward, cx)))
                })
                .child(theme::icon(cx, icon, 14.0, if has_matches { muted } else { theme::color(cx, "fg-faint") }))
        };
        div()
            .debug_selector(|| "run-find-bar".to_owned())
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(theme::icon(cx, "search", 14.0, muted))
            .child(div().flex_1().min_w_0().debug_selector(|| "run-find-field".to_owned()).child(self.field.clone()))
            .child(toggle("run-find-case", "Cc", self.find.options.case_sensitive, FindOption::MatchCase, cx))
            .child(toggle("run-find-words", "W", self.find.options.whole_word, FindOption::Words, cx))
            .child(toggle("run-find-regex", ".*", self.find.options.regex, FindOption::Regex, cx))
            .child(
                div()
                    .flex_none()
                    .min_w(px(56.0))
                    .text_color(if none { theme::color(cx, "danger") } else { muted })
                    .child(SharedString::from(counter)),
            )
            .child(arrow("run-find-previous", "arrow-up", false, cx))
            .child(arrow("run-find-next", "arrow-down", true, cx))
            .child(div().flex_1())
            .child(
                div()
                    .id("run-find-close")
                    .debug_selector(|| "run-find-close".to_owned())
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, "close", 14.0, muted))
                    .on_click(cx.listener(|view, _, window, cx| view.close_find(window, cx))),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let lines = self.empty_text(cx).unwrap_or_default();
        let (launch, link) = (tx(cx, "Launch a"), tx(cx, "run configuration"));
        let subtle = theme::color(cx, "fg-subtle");
        let edit_available = self.edit_available;
        let hint = |text: String| div().flex().gap(px(4.0)).child("— ").child(SharedString::from(text));
        let shortcut = self.chooser_shortcut.clone();
        div()
            .debug_selector(|| "run-empty".to_owned())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.0))
            .text_color(subtle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(lines.first().cloned().map(SharedString::from))
                    .children(lines.get(1).cloned().map(hint))
                    .children(lines.get(2).cloned().map(hint))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .child("— ")
                            .child(SharedString::from(launch))
                            .child(
                                div()
                                    .id("run-empty-configurations")
                                    .debug_selector(|| "run-empty-configurations".to_owned())
                                    .text_color(theme::color(cx, "link"))
                                    .when(edit_available, |el| {
                                        el.hover(|el| el.text_color(theme::color(cx, "link-hover"))).on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(RunViewEvent::EditConfigs(None))),
                                        )
                                    })
                                    .child(SharedString::from(link)),
                            )
                            .children(shortcut.map(|shortcut| {
                                div()
                                    .text_color(theme::color(cx, "fg-faint"))
                                    .text_size(theme::length(cx, "fs-small"))
                                    .child(SharedString::from(format!("({shortcut})")))
                            })),
                    ),
            )
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// `<3 internal lines>`.
fn fold_label(fold: Fold, cx: &App) -> String {
    let plural = if fold.count == 1 { "" } else { "s" };
    let text = format!("<{} internal line{plural}>", fold.count);
    cx.try_global::<Localization>().map_or_else(|| text.clone(), |localization| localization.0.tx_template(&text))
}

/// An option of the find bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindOption {
    MatchCase,
    Words,
    Regex,
}

#[derive(Clone, Copy)]
struct ButtonState {
    enabled: bool,
    pressed: bool,
    danger: bool,
}

impl Focusable for RunView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RunView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Scrolled back to the end, the list follows the output again.
        if self.user_scrolled && self.list.is_following_tail() != self.follow {
            self.follow = self.list.is_following_tail();
        }
        // The whole tool window is the focused one while the console or its find bar has the keyboard.
        let root = div().size_full().flex().flex_col().track_focus(&self.focus).text_size(theme::length(cx, "fs-ui"));
        let Some(session) = self.current(self.store.read(cx)).cloned() else {
            return root.child(self.render_empty(cx));
        };
        let notice = self.notice(cx);
        let wrap = self.soft_wrap();
        let console =
            list(self.list.clone(), cx.processor(|view, ix, window, cx| view.render_row(ix, window, cx))).size_full();
        let focused = self.focus.is_focused(window);
        root.child(self.render_tabs(cx)).child(
            div().flex_1().min_h_0().flex().child(self.render_toolbar(&session, cx)).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(notice.map(|notice| {
                        div()
                            .flex_none()
                            .px(px(12.0))
                            .py(px(4.0))
                            .text_size(theme::length(cx, "fs-small"))
                            .text_color(theme::color(cx, "fg-subtle"))
                            .border_b_1()
                            .border_color(theme::color(cx, "border"))
                            .child(SharedString::from(notice))
                    }))
                    .when(self.find.open, |el| el.child(self.render_find_bar(cx)))
                    .child(
                        div()
                            .id("run-console")
                            .debug_selector(|| "run-console".to_owned())
                            .flex_1()
                            .min_h_0()
                            .py(px(4.0))
                            .bg(theme::color(cx, "bg-app"))
                            .text_color(theme::color(cx, "fg"))
                            .font_family(Self::mono_font(cx).family)
                            .text_size(theme::length(cx, "fs-code"))
                            .line_height(px(ROW))
                            .when(!wrap, |el| el.whitespace_nowrap())
                            .when(focused, |el| el.border_color(theme::color(cx, "border")))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                                    view.all_selected = false;
                                    window.focus(&view.focus, cx);
                                    cx.notify();
                                }),
                            )
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                                    window.focus(&view.focus, cx);
                                    cx.emit(RunViewEvent::ShowMenu(event.position));
                                }),
                            )
                            .child(console),
                    ),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run::store::{FinishContext, RunSettings, StartEnv, StartOptions, StartRun, StartStep};
    use crate::run::types::register_builtin_types;
    use gpui_kit::{Bounds, TestAppContext, VisualTestContext};
    use ide_core::{IdeError, RunId};
    use ide_run::RunInfo;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<RunViewEvent>>>;

    fn types() -> Rc<RunConfigTypes> {
        let mut types = RunConfigTypes::new();
        register_builtin_types(&mut types);
        Rc::new(types)
    }

    fn config(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: "x".into(), ..blank_config(id, id, kind) }
    }

    /// Starts `config` in `store` as the driver would, its process getting run `run` (or failing to
    /// start with `error`). Returns the session.
    fn start(store: &mut RunStore, config: &RunConfig, run: u32, error: Option<&str>) -> String {
        let types = types();
        let settings = RunSettings::default();
        let env = StartEnv { types: &types, settings: &settings };
        let mut i18n = crate::platform::i18n::I18n::default();
        let (mut start, mut step) = StartRun::start(config, StartOptions::default(), store, env);
        loop {
            step = match step {
                StartStep::SaveAll => start.saved(store, env, 1.0),
                StartStep::GradleDelegation(launch) => {
                    start.delegation_resolved(Ok(Some(*launch)), store, env, 1.0, &mut i18n)
                }
                StartStep::PrepareType(launch) => start.type_prepared(Ok(*launch), store, 1.0, &mut i18n),
                StartStep::FinalizeLaunch(launch) => start.launch_finalized(Ok(*launch), store, 1.0, &mut i18n),
                StartStep::RunStart(launch) => {
                    let started = match error {
                        Some(message) => Err(IdeError::internal(message)),
                        None => Ok(RunInfo {
                            id: RunId(run),
                            config_id: launch.id.clone(),
                            name: launch.name.clone(),
                            pid: Some(run),
                            running: true,
                            pty: false,
                        }),
                    };
                    start.process_started(started, store, 1.0, &mut i18n)
                }
                StartStep::Done(session) => return session.expect("a session"),
                other => panic!("unexpected step {other:?}"),
            };
        }
    }

    fn print(store: &mut RunStore, session: &str, kind: ChunkKind, text: &str) {
        store.enqueue(session, kind, text, 1000.0, false);
        store.flush(session, 1000.0);
    }

    fn exit(store: &mut RunStore, session: &str, code: i32) {
        store.finish_session(session, None, Some(code), 2000.0, FinishContext::default());
    }

    /// The view on `store` in a window of its own, and what it asked of the window so far.
    fn open(
        cx: &mut TestAppContext,
        store: RunStore,
    ) -> (Entity<RunView>, Entity<RunStore>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let store = cx.update(|cx| cx.new(|_| store));
        let shared = store.clone();
        let (view, cx) = cx.add_window_view(|_, cx| RunView::new(shared, types(), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &RunViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, store, events, cx)
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn focus_console(view: &Entity<RunView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn icons(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.tab_icons(cx))
    }

    fn lines(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.lines(cx))
    }

    // Not from a spec: the toolbar's buttons and a tab's close button say what they do when the pointer
    // rests on them, with the shortcut of the action they run (RunView.tsx's IconButton titles).
    #[gpui_kit::test]
    async fn the_toolbar_buttons_show_their_tooltips(cx: &mut TestAppContext) {
        use crate::platform::keymap::{format_binding, host_binding, is_mac_host};
        use crate::ui::tooltip::hovered_tooltip;
        let mut store = RunStore::new();
        start(&mut store, &config("app", "spring-boot"), 1, None);
        let (_view, _, _, cx) = open(cx, store);
        assert!(hovered_tooltip(cx, "run-rerun", "Rerun", Some("run.rerun")));
        assert!(hovered_tooltip(cx, "run-debug", "Debug", Some("run.debug")));
        assert!(hovered_tooltip(cx, "run-stop", "Stop", None));
        assert!(!hovered_tooltip(cx, "run-stop", "Rerun", None), "each button has its own text");
        let mac = is_mac_host();
        let find = format!("Find…  {}", format_binding(&host_binding("Meta+F", mac), mac));
        assert!(hovered_tooltip(cx, "run-find", &find, None), "{find}");
        assert!(hovered_tooltip(cx, "run-scroll-to-end", "Scroll to End", None));
        assert!(hovered_tooltip(cx, "run-soft-wrap", "Soft-Wrap", None));
        assert!(hovered_tooltip(cx, "run-clear", "Clear All", None));
        assert!(hovered_tooltip(cx, "run-edit-configs", "Edit Configurations…", None));
        assert!(hovered_tooltip(cx, "run-tab-close-0", "Close Tab", None));
    }

    // src/features/run/RunView.test.tsx › sessionIcon › shows the execution arrow while running, the check for
    // tests
    #[gpui_kit::test]
    async fn shows_the_execution_arrow_while_running_the_check_for_tests(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        start(&mut store, &config("app", "spring-boot"), 1, None);
        let mut starting = config("tests", "spring-boot");
        starting.is_test = true;
        let types = types();
        let settings = RunSettings::default();
        let env = StartEnv { types: &types, settings: &settings };
        let (mut started, _) = StartRun::start(&starting, StartOptions::default(), &mut store, env);
        started.saved(&mut store, env, 1.0);
        let (view, _, _, cx) = open(cx, store);
        assert_eq!(icons(&view, cx), ["run", "check"]);
        assert!(cx.debug_bounds("run-tab-0").is_some() && cx.debug_bounds("run-tab-1").is_some());
    }

    // src/features/run/RunView.test.tsx › sessionIcon › shows the red error icon for a failed exit
    #[gpui_kit::test]
    async fn shows_the_red_error_icon_for_a_failed_exit(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        start(&mut store, &config("broken", "spring-boot"), 1, Some("no such command"));
        let exited = start(&mut store, &config("app", "spring-boot"), 2, None);
        exit(&mut store, &exited, 1);
        let (view, _, _, cx) = open(cx, store);
        assert_eq!(icons(&view, cx), ["error", "error"]);
        // The failed start says why in its console.
        view.update(cx, |view, cx| view.select_tab("run1", cx));
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["Error running 'broken': no such command"]);
        assert_eq!(view.read_with(cx, |view, cx| view.line_kinds(cx)), [ChunkKind::Stderr]);
    }

    // src/features/run/RunView.test.tsx › sessionIcon › shows the configuration's own icon once the process
    // ended (IDEA's third state)
    #[gpui_kit::test]
    async fn shows_the_configurations_own_icon_once_the_process_ended_ideas_third_state(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        for (id, kind) in [("boot", "spring-boot"), ("program", "program"), ("npm", "npm")] {
            let session = start(&mut store, &config(id, kind), 1, None);
            exit(&mut store, &session, 0);
        }
        let (view, _, _, cx) = open(cx, store);
        assert_eq!(icons(&view, cx), ["spring-boot", "file-binary", "file-npm"]);
        let tooltips = view.update(cx, |view, cx| view.tab_tooltips(cx));
        assert_eq!(tooltips[0].as_deref(), Some("Finished in 2 s, exit code 0"));
    }

    // src/features/run/RunView.test.tsx › sessionIcon › never shows the error icon for a session the user stopped
    #[gpui_kit::test]
    async fn never_shows_the_error_icon_for_a_session_the_user_stopped(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "spring-boot"), 1, None);
        store.stop_session(&session, 1500.0);
        exit(&mut store, &session, 1);
        let (view, _, _, cx) = open(cx, store);
        assert_eq!(icons(&view, cx), ["spring-boot"]);
    }

    // src/features/run/RunView.test.tsx › debugSession › launches the session's configuration through
    // run.debugConfig
    #[gpui_kit::test]
    async fn launches_the_sessions_configuration_through_run_debug_config(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let app = config("app", "spring-boot");
        start(&mut store, &app, 1, None);
        let (view, _, events, cx) = open(cx, store);
        view.update(cx, |view, cx| view.set_capabilities(true, true, None, cx));
        cx.run_until_parked();
        click(cx, "run-debug");
        assert_eq!(*events.borrow(), [RunViewEvent::Debug(Box::new(app))]);
    }

    // src/features/run/RunView.test.tsx › debugSession › prefers the original configuration of a derived run
    #[gpui_kit::test]
    async fn prefers_the_original_configuration_of_a_derived_run(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let base = config("base", "spring-boot");
        let session = start(&mut store, &config("derived", "spring-boot"), 1, None);
        store.sessions[0].base_config = Some(base.clone());
        let (view, _, events, cx) = open(cx, store);
        view.update(cx, |view, cx| view.debug_session(&session, cx));
        assert_eq!(*events.borrow(), [RunViewEvent::Debug(Box::new(base))]);
    }

    // src/features/run/RunView.test.tsx › debugSession › ignores unknown session ids
    #[gpui_kit::test]
    async fn ignores_unknown_session_ids(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        start(&mut store, &config("app", "spring-boot"), 1, None);
        let (view, _, events, cx) = open(cx, store);
        view.update(cx, |view, cx| view.debug_session("nope", cx));
        // Without a debugger the button does nothing.
        click(cx, "run-debug");
        assert!(events.borrow().is_empty());
    }

    /// A session whose process prints `Name: ` and waits for input.
    fn prompting(pending: &str) -> (RunStore, String) {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "shell"), 1, None);
        print(&mut store, &session, ChunkKind::Stdout, "Name: ");
        store.set_pending_input(&session, pending);
        (store, session)
    }

    fn pending(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, cx| view.pending_input(cx))
    }

    // src/features/run/session.test.tsx › console input › types, erases and sends a line
    #[gpui_kit::test]
    async fn types_erases_and_sends_a_line(cx: &mut TestAppContext) {
        let (store, session) = prompting("ab");
        let (view, _, events, cx) = open(cx, store);
        // The pending input continues the prompt.
        assert_eq!(lines(&view, cx), ["Name: ab"]);
        focus_console(&view, cx);
        cx.simulate_keystrokes("c");
        assert_eq!(pending(&view, cx).as_deref(), Some("abc"));
        cx.simulate_keystrokes("backspace");
        assert_eq!(pending(&view, cx).as_deref(), Some("ab"));
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [RunViewEvent::SendInput { session: session.clone(), line: "ab".into() }]);
        assert_eq!(pending(&view, cx).as_deref(), Some(""));
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(pending(&view, cx).as_deref(), Some(""));
        cx.simulate_keystrokes("x y");
        assert_eq!(pending(&view, cx).as_deref(), Some("xy"));
        cx.simulate_keystrokes("alt-backspace");
        assert_eq!(pending(&view, cx).as_deref(), Some(""));
        cx.simulate_keystrokes("ctrl-d");
        assert_eq!(events.borrow().last(), Some(&RunViewEvent::SendEof(session)));
        assert_eq!(events.borrow().len(), 2);
    }

    // src/features/run/session.test.tsx › console input › sends the complete lines of a paste and keeps the
    // rest pending
    #[gpui_kit::test]
    async fn sends_the_complete_lines_of_a_paste_and_keeps_the_rest_pending(cx: &mut TestAppContext) {
        let (store, session) = prompting("x");
        let (view, _, events, cx) = open(cx, store);
        focus_console(&view, cx);
        cx.write_to_clipboard(ClipboardItem::new_string("1\r\n2\n3".to_owned()));
        cx.simulate_keystrokes("cmd-v");
        let sent = |line: &str| RunViewEvent::SendInput { session: session.clone(), line: line.to_owned() };
        assert_eq!(*events.borrow(), [sent("x1"), sent("2")]);
        assert_eq!(pending(&view, cx).as_deref(), Some("3"));
    }

    /// A finished session whose console holds `text`.
    fn printed(text: &str) -> RunStore {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "shell"), 1, None);
        print(&mut store, &session, ChunkKind::Stdout, text);
        exit(&mut store, &session, 0);
        store
    }

    fn counter(view: &Entity<RunView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| view.find_counter(cx))
    }

    fn matches(view: &Entity<RunView>, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| view.match_count())
    }

    fn open_find(view: &Entity<RunView>, cx: &mut VisualTestContext) {
        focus_console(view, cx);
        cx.simulate_keystrokes("cmd-f");
        cx.run_until_parked();
    }

    const FIND_TEXT: &str = "alpha beta\ngamma Beta\nbeta again\n";

    // src/features/run/findBar.test.tsx › Run console find › opens on ⌘F, searches as you type and highlights
    // every match
    #[gpui_kit::test]
    async fn opens_on_cmd_f_searches_as_you_type_and_highlights_every_match(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        assert!(view.read_with(cx, |view, _| view.find_open()));
        assert!(cx.debug_bounds("run-find-bar").is_some());
        let field = view.read_with(cx, |view, _| view.find_field().clone());
        assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)));
        cx.simulate_input("beta");
        assert_eq!(counter(&view, cx), "1/3");
        assert_eq!(matches(&view, cx), 3);
        let current = view.read_with(cx, |view, _| view.find.marks.values().flatten().filter(|m| m.current).count());
        assert_eq!(current, 1);
    }

    // src/features/run/findBar.test.tsx › Run console find › moves through matches with Enter / Shift+Enter
    // and wraps around
    #[gpui_kit::test]
    async fn moves_through_matches_with_enter_shift_enter_and_wraps_around(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        cx.simulate_input("beta");
        assert_eq!(counter(&view, cx), "1/3");
        cx.simulate_keystrokes("enter");
        assert_eq!(counter(&view, cx), "2/3");
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(counter(&view, cx), "1/3");
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(counter(&view, cx), "3/3");
        click(cx, "run-find-next");
        assert_eq!(counter(&view, cx), "1/3");
    }

    // src/features/run/findBar.test.tsx › Run console find › supports match case, words and regex
    #[gpui_kit::test]
    async fn supports_match_case_words_and_regex(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        cx.simulate_input("beta");
        assert_eq!(counter(&view, cx), "1/3");
        click(cx, "run-find-case");
        assert_eq!(counter(&view, cx), "1/2");
        assert_eq!(matches(&view, cx), 2);
        let field = view.read_with(cx, |view, _| view.find_field().clone());
        field.update(cx, |field, cx| field.set_text("b.ta", cx));
        view.update(cx, |view, cx| view.search(true, cx));
        assert_eq!(counter(&view, cx), "0 results");
        click(cx, "run-find-regex");
        assert_eq!(counter(&view, cx), "1/2");
        click(cx, "run-find-words");
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_keystrokes("alt-w");
        assert_eq!(counter(&view, cx), "1/2");
    }

    // src/features/run/findBar.test.tsx › Run console find › reports an invalid regular expression
    #[gpui_kit::test]
    async fn reports_an_invalid_regular_expression(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        click(cx, "run-find-regex");
        let field = view.read_with(cx, |view, _| view.find_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input("(");
        assert_eq!(counter(&view, cx), "Bad regex");
    }

    // src/features/run/findBar.test.tsx › Run console find › closes on Escape, clears the highlights and
    // returns focus to the console
    #[gpui_kit::test]
    async fn closes_on_escape_clears_the_highlights_and_returns_focus_to_the_console(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        cx.simulate_input("beta");
        assert_eq!(matches(&view, cx), 3);
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.find_open()));
        assert!(cx.debug_bounds("run-find-bar").is_none());
        assert_eq!(matches(&view, cx), 0);
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
    }

    // src/features/run/findBar.test.tsx › Run console find › shows no matches when nothing matches
    #[gpui_kit::test]
    async fn shows_no_matches_when_nothing_matches(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed(FIND_TEXT));
        open_find(&view, cx);
        cx.simulate_input("nothing");
        assert_eq!(counter(&view, cx), "0 results");
        assert_eq!(matches(&view, cx), 0);
    }

    // src/features/run/findBar.test.tsx › Run console find › runs the bar in a controlled open state
    #[gpui_kit::test]
    async fn runs_the_bar_in_a_controlled_open_state(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, printed("one two"));
        // The toolbar's Find button opens the bar; Escape in it closes it and the button is off again.
        click(cx, "run-find");
        assert!(view.read_with(cx, |view, _| view.find_open()));
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.find_open()));
        click(cx, "run-find");
        click(cx, "run-find");
        assert!(!view.read_with(cx, |view, _| view.find_open()));
    }

    // Not from a spec: what nothing having run shows, with the way to the run configurations.
    #[gpui_kit::test]
    async fn the_empty_tool_window_lists_the_three_ways_to_run_something(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, RunStore::new());
        let text = view.read_with(cx, |view, cx| view.empty_text(cx)).expect("the empty state");
        assert_eq!(text[0], "To run your code, do one of the following:");
        assert_eq!(text[3], "Launch a run configuration");
        assert!(cx.debug_bounds("run-empty").is_some() && cx.debug_bounds("run-console").is_none());
        click(cx, "run-empty-configurations");
        assert!(events.borrow().is_empty(), "Edit Configurations has no handler");
        view.update(cx, |view, cx| view.set_capabilities(false, true, Some("⌃⌥R".into()), cx));
        cx.run_until_parked();
        click(cx, "run-empty-configurations");
        assert_eq!(*events.borrow(), [RunViewEvent::EditConfigs(None)]);
        let text = view.read_with(cx, |view, cx| view.empty_text(cx)).expect("the empty state");
        assert_eq!(text[3], "Launch a run configuration (⌃⌥R)");
    }

    // Not from a spec: a tab per session, the active one showing its console; a click selects a tab and
    // its close button asks the window to close it.
    #[gpui_kit::test]
    async fn tabs_select_their_session_and_close_through_the_window(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let first = start(&mut store, &config("server", "shell"), 1, None);
        print(&mut store, &first, ChunkKind::Stdout, "listening\n");
        let second = start(&mut store, &config("worker", "shell"), 2, None);
        print(&mut store, &second, ChunkKind::Stderr, "\x1b[31mboom\x1b[0m\n");
        let (view, store, events, cx) = open(cx, store);
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["server", "worker"]);
        assert_eq!(view.read_with(cx, |view, cx| view.active_tab(cx)), Some(second.clone()));
        assert_eq!(lines(&view, cx), ["boom"]);
        click(cx, "run-tab-0");
        assert_eq!(store.read_with(cx, |store, _| store.active_session.clone()), Some(first.clone()));
        assert_eq!(lines(&view, cx), ["listening"]);
        assert!(cx.debug_bounds("run-line-0").is_some());
        click(cx, "run-tab-close-1");
        assert_eq!(*events.borrow(), [RunViewEvent::CloseTab(second)]);
        click(cx, "run-stop");
        assert_eq!(events.borrow().last(), Some(&RunViewEvent::Stop(first.clone())));
        click(cx, "run-rerun");
        assert_eq!(events.borrow().last(), Some(&RunViewEvent::Rerun(first)));
    }

    // Not from a spec: system lines are not output, stderr keeps its stream, ANSI escapes do not show,
    // and Clear All empties the console.
    #[gpui_kit::test]
    async fn the_console_shows_each_stream_and_clear_all_empties_it(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "shell"), 1, None);
        print(&mut store, &session, ChunkKind::System, "/bin/app --serve\n");
        print(&mut store, &session, ChunkKind::Stdout, "\x1b[1;32mready\x1b[0m at src/main.rs:4\n");
        print(&mut store, &session, ChunkKind::Stderr, "warning\n");
        let (view, _, _, cx) = open(cx, store);
        assert_eq!(lines(&view, cx), ["/bin/app --serve", "ready at src/main.rs:4", "warning"]);
        let kinds = view.read_with(cx, |view, cx| view.line_kinds(cx));
        assert_eq!(kinds, [ChunkKind::System, ChunkKind::Stdout, ChunkKind::Stderr]);
        click(cx, "run-clear");
        assert!(lines(&view, cx).is_empty());
    }

    // Not from a spec: a run of folded lines collapses onto the line before it and opens on a click.
    #[gpui_kit::test]
    async fn folded_lines_collapse_onto_the_line_before_them(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "shell"), 1, None);
        print(&mut store, &session, ChunkKind::Stderr, "Exception\nat org.junit.A\nat org.junit.B\ndone\n");
        let (view, _, _, cx) = open(cx, store);
        view.update(cx, |view, cx| {
            let settings =
                ConsoleSettings { fold_patterns: vec!["at org.junit.".into()], ..ConsoleSettings::default() };
            view.set_console_settings(settings, cx);
        });
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["Exception <2 internal lines>", "done"]);
        let fold = view.read_with(cx, |view, cx| {
            view.console(view.store.read(cx)).map(|console| console.lines()[1].id).expect("a console")
        });
        view.update(cx, |view, cx| view.toggle_fold(fold, cx));
        assert_eq!(lines(&view, cx), ["Exception", "at org.junit.A", "at org.junit.B", "done"]);
    }

    // Not from a spec: the console follows new output until Scroll to End is turned off; Soft-Wrap
    // overrides the setting for the tab.
    #[gpui_kit::test]
    async fn scroll_to_end_and_soft_wrap_toggle_from_the_toolbar(cx: &mut TestAppContext) {
        let (view, store, _, cx) = open(cx, printed("one\n"));
        assert!(view.read_with(cx, |view, _| view.follows() && !view.soft_wrap()));
        click(cx, "run-scroll-to-end");
        assert!(!view.read_with(cx, |view, _| view.follows()));
        click(cx, "run-scroll-to-end");
        assert!(view.read_with(cx, |view, _| view.follows()));
        click(cx, "run-soft-wrap");
        assert!(view.read_with(cx, |view, _| view.soft_wrap()));
        // New output keeps the list at its end.
        store.update(cx, |store, cx| {
            let id = store.sessions[0].id.clone();
            for i in 0..200 {
                print(store, &id, ChunkKind::Stdout, &format!("line {i}\n"));
            }
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("run-line-200").is_some(), "the last line is in view");
        assert!(cx.debug_bounds("run-line-0").is_none());
    }

    // Not from a spec: scrolling up stops following the output, so new lines leave the view where it
    // is; scrolling back to the end follows it again.
    #[gpui_kit::test]
    async fn scrolling_up_stops_following_the_output_until_back_at_the_end(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let session = start(&mut store, &config("app", "shell"), 1, None);
        for i in 0..300 {
            print(&mut store, &session, ChunkKind::Stdout, &format!("line {i}\n"));
        }
        let (view, store, _, cx) = open(cx, store);
        assert!(cx.debug_bounds("run-line-299").is_some());
        let scroll = |cx: &mut VisualTestContext, by: f32| {
            let centre = bounds_of(cx, "run-console").center();
            cx.simulate_event(gpui_kit::ScrollEvent {
                position: centre,
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.0), px(by))),
                ..Default::default()
            });
            cx.run_until_parked();
        };
        scroll(cx, 2000.0);
        assert!(!view.read_with(cx, |view, _| view.follows()));
        assert!(cx.debug_bounds("run-line-299").is_none());
        let top = view.read_with(cx, |view, _| view.list.logical_scroll_top().item_ix);
        store.update(cx, |store, cx| {
            print(store, &session, ChunkKind::Stdout, "more\n");
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.list.logical_scroll_top().item_ix), top);
        assert!(cx.debug_bounds("run-line-300").is_none());
        // A wheel turn is several events; the list reports the rows of the last one it drew.
        scroll(cx, -100_000.0);
        scroll(cx, -20.0);
        assert!(view.read_with(cx, |view, _| view.follows()));
        store.update(cx, |store, cx| {
            print(store, &session, ChunkKind::Stdout, "last\n");
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("run-line-301").is_some());
    }

    // Not from a spec: links in the output are reported with the run directory.
    #[gpui_kit::test]
    async fn a_file_location_in_the_output_opens_through_the_window(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        let mut app = config("app", "shell");
        app.cwd = Some(ide_core::IpcPath(std::path::PathBuf::from("server")));
        let session = start(&mut store, &app, 1, None);
        print(&mut store, &session, ChunkKind::Stderr, "src/main.rs:4:2\n");
        let (view, _, events, cx) = open(cx, store);
        view.update(cx, |view, cx| {
            view.set_project_root(Some("/p".into()));
            let link = find_output_links("src/main.rs:4:2").remove(0);
            view.open_link(&link, cx);
        });
        let seen = events.borrow().clone();
        let [RunViewEvent::OpenLocation { target, cwd }] = seen.as_slice() else {
            panic!("a location opens: {seen:?}")
        };
        assert_eq!(
            (target.path.as_str(), target.line, target.col, cwd.as_str()),
            ("src/main.rs", Some(4), Some(2), "/p/server")
        );
    }

    // Not from a spec: Select All selects the whole buffer, which Copy then copies; the context menu
    // offers Send EOF only while the process takes input.
    #[gpui_kit::test]
    async fn select_all_copies_the_whole_buffer_and_the_menu_follows_the_input(cx: &mut TestAppContext) {
        let (store, _) = prompting("");
        let (view, store, _, cx) = open(cx, store);
        let labels = |view: &Entity<RunView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| {
                view.console_menu(cx).iter().filter_map(|item| item.label().map(str::to_owned)).collect::<Vec<_>>()
            })
        };
        assert_eq!(labels(&view, cx).last().map(String::as_str), Some("Send EOF"));
        focus_console(&view, cx);
        cx.simulate_keystrokes("cmd-a cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("Name: "));
        store.update(cx, |store, cx| {
            let id = store.sessions[0].id.clone();
            exit(store, &id, 0);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(labels(&view, cx).last().map(String::as_str), Some("Clear All"));
    }

    // Not from a spec: the cycle buffer's discarded lines are counted over the console.
    #[gpui_kit::test]
    async fn the_discarded_lines_are_counted_over_the_console(cx: &mut TestAppContext) {
        let mut store = RunStore::new();
        store.console_char_limit = Some(10);
        let session = start(&mut store, &config("app", "shell"), 1, None);
        print(&mut store, &session, ChunkKind::Stdout, "first line\nsecond line\nthird\n");
        let (view, _, _, cx) = open(cx, store);
        let notice = view.update(cx, |view, cx| view.notice(cx)).expect("a notice");
        assert!(notice.ends_with("earlier lines were discarded"), "{notice}");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(erased("say hello ", true), "say ");
        assert_eq!(pasted("x", "1\r\n2\n3"), (vec!["x1".to_owned(), "2".to_owned()], "3".to_owned()));
    }
}
