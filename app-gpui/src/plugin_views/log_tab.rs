//! The log tab a plugin opens in the editor area: the output of a program the host approved for it
//! (`views.open-log-tab`, started by the tab's key through the plugin's `process:` grant), or the
//! snapshot of one of its output channels (`output.reveal`).
//!
//! The tab is read-only text. Output arrives in chunks: carriage-return line ends become line
//! breaks and terminal colour codes are dropped; what is kept is bounded (the last 2^20 UTF-16
//! code units, then the last 5,000 lines). An exit, a start failure or a stop is told on a line of
//! its own. Above the text, one switch per toggle the plugin declared (turning one restarts the
//! program with its arguments added), Soft-wrap and Clear.
//!
//! [`PluginLogTab`] runs nothing: [`PluginLogEvent::Start`] asks the workbench to start the program
//! with the toggles that are on, [`PluginLogEvent::Stop`] to kill it; the output comes back through
//! [`PluginLogTab::append`], [`PluginLogTab::exited`] and [`PluginLogTab::failed`].

use crate::editor::tabs::EditorInput;
use crate::platform::i18n::{Localization, Param};
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FollowMode, IntoElement, ListAlignment,
    ListState, Render, SharedString, Window, div, list, prelude::*, px,
};
use ide_plugin::{ServiceLog, ServiceLogToggle};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::LazyLock;

/// The editor kind of plugin log tabs.
pub const PLUGIN_LOG_KIND: &str = "pluginLog";
/// The icon of their tabs.
pub const PLUGIN_LOG_ICON: &str = "file-log";
/// How much text a tab keeps, in UTF-16 code units, and how many lines of it.
const MAX_UNITS: usize = 1 << 20;
const MAX_LINES: usize = 5000;
/// The terminal size a log tab's program runs in.
pub const LOG_COLUMNS: u16 = 240;
pub const LOG_ROWS: u16 = 40;
const ROW: f32 = 18.0;

/// Terminal colour and cursor sequences, dropped from the output.
static ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\u{1b}\\[[0-9;?]*[A-Za-z]").expect("the escape pattern is valid"));

/// Where a log tab's text comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogOrigin {
    /// The plugin's code opened it: the host starts what it approved for the tab's key.
    Code,
    /// An output channel's snapshot, replaced as the channel changes.
    Output,
}

/// What a log tab's input carries, as the editor session keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTabData {
    pub plugin_id: String,
    pub view: String,
    #[serde(default)]
    pub key: Option<String>,
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub toggles: Vec<ServiceLogToggle>,
    pub origin: LogOrigin,
}

impl LogTabData {
    /// The data of `input`, when it is a log tab's.
    pub fn of(input: &EditorInput) -> Option<Self> {
        if input.kind != PLUGIN_LOG_KIND {
            return None;
        }
        serde_json::from_value(input.data.clone()?).ok()
    }
}

/// The input of the log tab `key` that plugin `plugin_id`'s code opened, running `log`.
pub fn code_log_input(plugin_id: &str, key: &str, title: &str, log: &ServiceLog) -> EditorInput {
    let data = LogTabData {
        plugin_id: plugin_id.to_owned(),
        view: String::new(),
        key: Some(key.to_owned()),
        title: title.to_owned(),
        program: log.program.clone(),
        args: log.args.clone(),
        toggles: log.toggles.clone(),
        origin: LogOrigin::Code,
    };
    let tab_key = format!("pluginLog:{}", serde_json::json!([plugin_id, "", key]));
    EditorInput { kind: PLUGIN_LOG_KIND.to_owned(), key: tab_key, path: None, data: serde_json::to_value(data).ok() }
}

/// The input of the tab showing output channel `channel` of plugin `plugin_id`, titled `title`.
pub fn output_input(plugin_id: &str, channel: &str, title: &str) -> EditorInput {
    let data = LogTabData {
        plugin_id: plugin_id.to_owned(),
        view: String::new(),
        key: Some(channel.to_owned()),
        title: title.to_owned(),
        program: String::new(),
        args: Vec::new(),
        toggles: Vec::new(),
        origin: LogOrigin::Output,
    };
    let tab_key = format!("pluginOutput:{}", serde_json::json!([plugin_id, channel]));
    EditorInput { kind: PLUGIN_LOG_KIND.to_owned(), key: tab_key, path: None, data: serde_json::to_value(data).ok() }
}

/// The title of a log tab: the one it was opened with, else its key.
pub fn log_tab_title(input: &EditorInput) -> String {
    LogTabData::of(input).map(|data| data.title).filter(|title| !title.is_empty()).unwrap_or_else(|| input.key.clone())
}

/// A chunk of terminal output as text: `\r\n` line ends become `\n`, colour codes go.
pub fn clean_output(chunk: &str) -> String {
    ANSI.replace_all(&chunk.replace("\r\n", "\n"), "").into_owned()
}

/// The end of `text` a tab keeps: at most [`MAX_UNITS`] UTF-16 code units, then at most
/// [`MAX_LINES`] lines.
pub fn bounded_log(text: &str) -> String {
    let mut units = 0;
    let mut start = text.len();
    for (index, c) in text.char_indices().rev() {
        units += c.len_utf16();
        if units > MAX_UNITS {
            break;
        }
        start = index;
    }
    let kept = &text[start..];
    let breaks = kept.matches('\n').count();
    if breaks < MAX_LINES {
        return kept.to_owned();
    }
    // Lines are separated by breaks: the last MAX_LINES lines start after the break before them.
    let skip = breaks + 1 - MAX_LINES;
    let cut = kept.match_indices('\n').nth(skip - 1).map_or(0, |(at, _)| at + 1);
    kept[cut..].to_owned()
}

/// What a log tab asks of its workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginLogEvent {
    /// Start the tab's program, with the toggles `on` (any running one was stopped first).
    Start { on: Vec<String> },
    /// Kill the tab's program.
    Stop,
}

pub struct PluginLogTab {
    data: LogTabData,
    text: String,
    /// Output (or a Clear) arrived since the program started: the waiting line is gone.
    received: bool,
    /// The stream ended for good; nothing is added any more.
    stopped: bool,
    /// The program was asked to start and has not been stopped since.
    running: bool,
    on: BTreeSet<String>,
    wrap: bool,
    lines: Rc<Vec<SharedString>>,
    list: ListState,
    focus: FocusHandle,
}

impl EventEmitter<PluginLogEvent> for PluginLogTab {}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

impl PluginLogTab {
    /// The tab of `data`, soft-wrapping as `wrap` says. It shows its waiting line until
    /// [`Self::start`] (or, for an output channel, [`Self::set_output`]).
    pub fn new(data: LogTabData, wrap: bool, cx: &mut Context<Self>) -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(400.0));
        list.set_follow_mode(FollowMode::Tail);
        let mut tab = Self {
            data,
            text: String::new(),
            received: false,
            stopped: false,
            running: false,
            on: BTreeSet::new(),
            wrap,
            lines: Rc::default(),
            list,
            focus: cx.focus_handle(),
        };
        tab.text = tx(cx, "Waiting for output…");
        tab.relist(true);
        tab
    }

    pub fn data(&self) -> &LogTabData {
        &self.data
    }

    pub fn title(&self) -> &str {
        &self.data.title
    }

    /// The lines shown, top to bottom.
    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().map(ToString::to_string).collect()
    }

    /// The text shown.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The plugin's toggles: id, label, and whether it is on.
    pub fn toggles(&self) -> Vec<(String, String, bool)> {
        self.data.toggles.iter().map(|t| (t.id.clone(), t.label.clone(), self.on.contains(&t.id))).collect()
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Starts the program with the toggles that are on, the waiting line shown until it prints.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.data.origin == LogOrigin::Output {
            return;
        }
        self.text = tx(cx, "Waiting for output…");
        (self.received, self.stopped, self.running) = (false, false, true);
        self.relist(true);
        cx.emit(PluginLogEvent::Start { on: self.on.iter().cloned().collect() });
        cx.notify();
    }

    /// Output of the program.
    pub fn append(&mut self, chunk: &str, cx: &mut Context<Self>) {
        if self.stopped {
            return;
        }
        let clean = clean_output(chunk);
        let previous = if self.received { std::mem::take(&mut self.text) } else { String::new() };
        self.received = true;
        let joined = previous.clone() + &clean;
        self.text = bounded_log(&joined);
        // Only the last line and what follows it changed, unless the start was cut away.
        self.relist(self.text.len() != joined.len() || previous.is_empty());
        cx.notify();
    }

    /// The program ended with `code` (`None`: unknown).
    pub fn exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        let line = match code {
            Some(code) => cx
                .default_global::<Localization>()
                .0
                .tx_built(&["\n[Process finished with exit code ", "]"], &[Param::Text(code.to_string())]),
            None => tx(cx, "\n[Process finished]"),
        };
        self.append(&line, cx);
        self.running = false;
    }

    /// The program could not be started, or its stream broke.
    pub fn failed(&mut self, error: &str, cx: &mut Context<Self>) {
        let line = cx.default_global::<Localization>().0.tx_built(&["\nError: ", ""], &[Param::Text(error.to_owned())]);
        self.append(&line, cx);
        self.running = false;
    }

    /// Ends the stream: the program is killed, and with `report` the tab says so. Nothing is added
    /// afterwards.
    pub fn stop(&mut self, report: bool, cx: &mut Context<Self>) {
        if self.stopped {
            return;
        }
        if self.running {
            cx.emit(PluginLogEvent::Stop);
        }
        if report {
            let line = tx(cx, "\n[Log stream stopped]");
            self.append(&line, cx);
        }
        (self.stopped, self.running) = (true, false);
        cx.notify();
    }

    /// The channel's text now (an output tab).
    pub fn set_output(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = text.to_owned();
        self.received = true;
        self.relist(true);
        cx.notify();
    }

    /// Clear: the text goes; what comes next starts an empty log.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.received = true;
        self.relist(true);
        cx.notify();
    }

    /// Soft-wrap on or off.
    pub fn toggle_wrap(&mut self, cx: &mut Context<Self>) {
        self.wrap = !self.wrap;
        self.list.remeasure();
        cx.notify();
    }

    /// Turns toggle `id` on or off: the program restarts with its arguments added or left out.
    /// An unknown toggle changes nothing.
    pub fn set_toggle(&mut self, id: &str, checked: bool, cx: &mut Context<Self>) {
        if !self.data.toggles.iter().any(|t| t.id == id) {
            return;
        }
        let changed = if checked { self.on.insert(id.to_owned()) } else { self.on.remove(id) };
        if !changed {
            return;
        }
        if self.running {
            cx.emit(PluginLogEvent::Stop);
            self.running = false;
        }
        if self.data.origin == LogOrigin::Code {
            self.start(cx);
        }
        cx.notify();
    }

    /// Splits the text into lines and tells the list which changed: every one with `all`, else the
    /// last one shown before and those after it.
    fn relist(&mut self, all: bool) {
        let lines: Vec<SharedString> = self.text.split('\n').map(|line| SharedString::from(line.to_owned())).collect();
        let before = self.lines.len();
        if all || before == 0 {
            self.list.reset(lines.len());
        } else {
            let from = before - 1;
            self.list.splice(from..before, lines.len() - from);
        }
        self.lines = Rc::new(lines);
    }

    fn switch(&self, cx: &App, selector: String, label: String, on: bool) -> gpui_kit::Stateful<Div> {
        let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(6.0))
            .h(px(24.0))
            .rounded(theme::length(cx, "radius-small"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(
                div()
                    .flex_none()
                    .size(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                    .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                    .children(mark),
            )
            .child(SharedString::from(label))
    }
}

impl Focusable for PluginLogTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PluginLogTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toggles: Vec<_> = self
            .toggles()
            .into_iter()
            .map(|(id, label, on)| {
                let toggle = id.clone();
                self.switch(cx, format!("plugin-log-toggle-{id}"), label, on)
                    .on_click(cx.listener(move |tab, _: &ClickEvent, _, cx| tab.set_toggle(&toggle, !on, cx)))
            })
            .collect();
        let wrap_label = tx(cx, "Soft-wrap");
        let wrap = self
            .switch(cx, "plugin-log-wrap".to_owned(), wrap_label, self.wrap)
            .on_click(cx.listener(|tab, _: &ClickEvent, _, cx| tab.toggle_wrap(cx)));
        let clear = div()
            .id("plugin-log-clear")
            .debug_selector(|| "plugin-log-clear".to_owned())
            .flex_none()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(theme::icon(cx, "trash", 14.0, theme::color(cx, "fg-muted")))
            .on_click(cx.listener(|tab, _: &ClickEvent, _, cx| tab.clear(cx)));
        let toolbar = div()
            .flex_none()
            .h(px(30.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .children(toggles)
            .child(wrap)
            .child(div().flex_1())
            .child(clear);
        let wrapping = self.wrap;
        let lines = Rc::clone(&self.lines);
        let output = list(
            self.list.clone(),
            cx.processor(move |_, index: usize, _, cx| {
                let line = lines.get(index).cloned().unwrap_or_default();
                div()
                    .w_full()
                    .min_h(px(ROW))
                    .px(px(12.0))
                    .text_color(theme::color(cx, "fg"))
                    .when(!wrapping, |el| el.whitespace_nowrap().overflow_hidden())
                    .child(line)
                    .into_any_element()
            }),
        )
        .size_full();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .debug_selector(|| "plugin-log-tab".to_owned())
            .bg(theme::color(cx, "bg-editor"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(toolbar)
            .child(div().flex_1().min_h_0().font_family(theme::fonts::BUNDLED_MONO_FAMILY).child(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<PluginLogEvent>>>;

    fn data(toggles: &[(&str, &str, &[&str])]) -> LogTabData {
        let log = ServiceLog {
            program: "bash".to_owned(),
            args: vec!["-c".to_owned(), "echo".to_owned()],
            toggles: toggles
                .iter()
                .map(|(id, label, args)| ServiceLogToggle {
                    id: (*id).to_owned(),
                    label: (*label).to_owned(),
                    args: args.iter().map(|a| (*a).to_owned()).collect(),
                })
                .collect(),
        };
        LogTabData::of(&code_log_input("web-log", "code", "Probe logs (code)", &log)).expect("a log tab's input")
    }

    fn open(cx: &mut TestAppContext, data: LogTabData) -> (Entity<PluginLogTab>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(move |_, cx| PluginLogTab::new(data, true, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &PluginLogEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }

    // src/features/plugins/__tests__/logEditor.test.tsx › plugin streaming log tabs
    #[test]
    fn bounds_newline_free_output_and_retained_lines() {
        let long = "x".repeat(MAX_UNITS + 10);
        assert_eq!(bounded_log(&long).len(), MAX_UNITS);
        let many: Vec<String> = (0..6000).map(|i| i.to_string()).collect();
        let kept = bounded_log(&many.join("\n"));
        let lines: Vec<&str> = kept.split('\n').collect();
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0], "1000");
        assert_eq!(lines[MAX_LINES - 1], "5999");
        // A character outside the basic plane counts two units and is never split.
        let wide = format!("a{}", "😀".repeat(MAX_UNITS / 2));
        assert_eq!(bounded_log(&wide), "😀".repeat(MAX_UNITS / 2));
    }

    /// Terminal output reads as text: CRLF line ends and colour codes are gone.
    #[test]
    fn terminal_line_ends_and_colour_codes_are_cleaned() {
        assert_eq!(clean_output("\u{1b}[32mINFO\u{1b}[0m ready\r\nnext\r\n"), "INFO ready\nnext\n");
        assert_eq!(clean_output("\u{1b}[?25lhidden"), "hidden");
    }

    /// The input names the plugin and the key; its tab is titled as opened, and an input of another
    /// kind is not a log tab's.
    #[test]
    fn a_code_log_input_carries_what_the_tab_runs() {
        let data = data(&[("verbose", "Verbose", &["--verbose"])]);
        assert_eq!(
            (data.plugin_id.as_str(), data.key.as_deref(), data.origin),
            ("web-log", Some("code"), LogOrigin::Code)
        );
        let input = code_log_input(
            "web-log",
            "code",
            "Probe logs (code)",
            &ServiceLog { program: "bash".to_owned(), args: Vec::new(), toggles: Vec::new() },
        );
        assert_eq!(input.key, r#"pluginLog:["web-log","","code"]"#);
        assert_eq!(log_tab_title(&input), "Probe logs (code)");
        let output = output_input("p", "test", "Output: test");
        assert_eq!(output.key, r#"pluginOutput:["p","test"]"#);
        let file = EditorInput { kind: "file".to_owned(), key: "/a".to_owned(), path: None, data: None };
        assert_eq!(LogTabData::of(&file), None);
    }

    // src/features/plugins/__tests__/logEditor.test.tsx › plugin streaming log tabs
    #[gpui_kit::test]
    async fn renders_partial_output_read_only_handles_early_exit_clears_and_wraps(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, data(&[]));
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["Waiting for output…"]);
        view.update(cx, |view, cx| view.start(cx));
        assert_eq!(events.borrow().as_slice(), [PluginLogEvent::Start { on: Vec::new() }]);
        view.update(cx, |view, cx| view.append("INFO one\r\nINFO t", cx));
        view.update(cx, |view, cx| view.append("wo\r\n", cx));
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["INFO one", "INFO two", ""]);
        view.update(cx, |view, cx| view.exited(Some(42), cx));
        assert_eq!(
            view.read_with(cx, |view, _| view.lines()).last().map(String::as_str),
            Some("[Process finished with exit code 42]")
        );
        assert!(!view.read_with(cx, |view, _| view.is_running()));
        click(cx, "plugin-log-clear");
        assert_eq!(view.read_with(cx, |view, _| view.text().to_owned()), "");
        assert!(view.read_with(cx, |view, _| view.wraps()));
        click(cx, "plugin-log-wrap");
        assert!(!view.read_with(cx, |view, _| view.wraps()));
    }

    // src/features/plugins/__tests__/logEditor.test.tsx › plugin streaming log tabs
    #[gpui_kit::test]
    async fn kills_the_previous_process_on_toggle_and_every_lifecycle_boundary(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, data(&[("verbose", "Verbose", &["--verbose"])]));
        view.update(cx, |view, cx| view.start(cx));
        view.update(cx, |view, cx| view.append("INFO 1\n", cx));
        assert_eq!(view.read_with(cx, |view, _| view.toggles()), [("verbose".to_owned(), "Verbose".to_owned(), false)]);
        click(cx, "plugin-log-toggle-verbose");
        assert_eq!(
            events.borrow().as_slice(),
            [
                PluginLogEvent::Start { on: Vec::new() },
                PluginLogEvent::Stop,
                PluginLogEvent::Start { on: vec!["verbose".to_owned()] },
            ]
        );
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["Waiting for output…"], "a restart starts a new log");
        view.update(cx, |view, cx| view.stop(true, cx));
        assert_eq!(events.borrow().last(), Some(&PluginLogEvent::Stop));
        assert_eq!(view.read_with(cx, |view, _| view.lines()).last().map(String::as_str), Some("[Log stream stopped]"));
        view.update(cx, |view, cx| view.append("late\n", cx));
        assert!(!view.read_with(cx, |view, _| view.text().contains("late")), "nothing is added once stopped");
        let count = events.borrow().len();
        view.update(cx, |view, cx| view.stop(true, cx));
        assert_eq!(events.borrow().len(), count, "a stopped stream is stopped once");
    }

    // src/features/plugins/__tests__/logEditor.test.tsx › plugin streaming log tabs
    #[gpui_kit::test]
    async fn shows_output_channel_snapshots_in_the_read_only_log_editor_without_starting_a_process(
        cx: &mut TestAppContext,
    ) {
        let data = LogTabData::of(&output_input("p", "test", "Output: test")).expect("an output tab's input");
        let (view, events, cx) = open(cx, data);
        view.update(cx, |view, cx| view.start(cx));
        view.update(cx, |view, cx| view.set_output("one\ntwo", cx));
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["one", "two"]);
        view.update(cx, |view, cx| view.set_output("three", cx));
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["three"]);
        assert!(events.borrow().is_empty(), "an output tab starts nothing");
    }
}
