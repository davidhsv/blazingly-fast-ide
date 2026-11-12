//! The Frames pane of the Debug tool window: the thread chooser and the stack of the active
//! session's selected thread. Runs of library frames collapse into one row (a click shows them
//! all) and a library frame that stays is dimmed. While nothing is suspended the pane says why
//! there are no frames: the session runs, waits for its debuggee, or has finished.
//!
//! [`FramesView`] reads the sessions of the [`Debugger`] and changes nothing in them: choosing a
//! thread or a frame, Jump to Source and Reset Frame are [`FramesViewEvent`]s the workbench carries
//! out on the model. Only whether library frames are collapsed is the view's own.

use crate::debugger::actions::Debugger;
use crate::debugger::focused_view::{DebugView, DebugViewCommands};
use crate::debugger::frames::{FrameItem, frame_label, frame_path, group_frames};
use crate::debugger::remote::remote_waiting_note;
use crate::debugger::state::{DebugSession, SessionStatus};
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, FocusOutEvent, Focusable,
    Hsla, IntoElement, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::DebugSessionId;
use std::rc::Rc;

/// What the Frames pane asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum FramesViewEvent {
    /// Show this thread's stack: `DebugState::select_thread`.
    SelectThread { session: DebugSessionId, thread: i64 },
    /// `DebugState::select_frame`; `reveal`: also open the frame's source, without the focus.
    SelectFrame { session: DebugSessionId, frame: i64, reveal: bool },
    /// Open the frame's source with the focus: the effect of `DebugState::reveal_frame(frame, true)`.
    JumpToSource { session: DebugSessionId, frame: i64 },
    /// Run this debugger action (Reset Frame of the frame just selected).
    RunAction(&'static str),
    /// Show [`FramesView::frame_menu`] here.
    ShowFrameMenu(Point<Pixels>),
    /// Show [`FramesView::thread_menu`] here; its commands go to [`FramesView::choose_thread`].
    ShowThreadMenu(Point<Pixels>),
    /// The pane took the keyboard focus: `FocusedView::focus` with these commands.
    Focused(DebugViewCommands),
    /// The pane lost the keyboard focus: `FocusedView::blur`.
    Blurred(DebugView),
}

/// A row of the frame context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameCommand {
    JumpToSource,
    ResetFrame,
    ToggleLibraryFrames,
    CopyStack,
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq)]
enum FrameRow {
    Frame {
        id: i64,
        name: String,
        /// `, main.py:12`: the file name and the 1-based line.
        file: Option<String>,
        library: bool,
    },
    Hidden {
        count: usize,
    },
}

/// What the pane can do for the global actions while it has the focus.
const COMMANDS: DebugViewCommands =
    DebugViewCommands { view: DebugView::Frames, set_value: false, jump_to_source: true };

pub struct FramesView {
    debugger: Entity<Debugger>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    project_root: Option<String>,
    /// Show All Frames: library frames are not collapsed.
    show_all: bool,
    /// The frame the context menu was opened on.
    menu_frame: Option<i64>,
    key_claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FramesViewEvent> for FramesView {}

impl FramesView {
    pub fn new(debugger: Entity<Debugger>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |view, cx| view.key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let subscriptions = vec![
            cx.observe(&debugger, |_, _, cx| cx.notify()),
            cx.on_focus_in(&focus, window, |_, _, cx| cx.emit(FramesViewEvent::Focused(COMMANDS))),
            cx.on_focus_out(&focus, window, |_, _: FocusOutEvent, _, cx| {
                cx.emit(FramesViewEvent::Blurred(DebugView::Frames));
            }),
        ];
        Self {
            debugger,
            focus,
            scroll: UniformListScrollHandle::new(),
            project_root: None,
            show_all: false,
            menu_frame: None,
            key_claim,
            _subscriptions: subscriptions,
        }
    }

    /// The open project's folder: frames outside it are library frames.
    pub fn set_project_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.project_root = root;
        cx.notify();
    }

    fn session<'a>(&self, cx: &'a App) -> Option<&'a DebugSession> {
        self.debugger.read(cx).state.active_session()
    }

    fn rows(&self, cx: &App) -> Vec<FrameRow> {
        let Some(session) = self.session(cx) else { return Vec::new() };
        group_frames(&session.frames, self.project_root.as_deref(), session.selected_frame, self.show_all)
            .into_iter()
            .map(|item| match item {
                FrameItem::Frame { frame, library } => FrameRow::Frame {
                    id: frame.id,
                    name: frame.name.clone(),
                    file: frame_path(frame).map(|path| format!(", {}:{}", basename(&path), frame.line + 1)),
                    library,
                },
                FrameItem::Hidden { count, .. } => FrameRow::Hidden { count },
            })
            .collect()
    }

    /// The ids of the frames listed, in order.
    fn frame_ids(&self, cx: &App) -> Vec<i64> {
        let rows = self.rows(cx);
        rows.iter().filter_map(|row| if let FrameRow::Frame { id, .. } = row { Some(*id) } else { None }).collect()
    }

    // ---------------------------------------------------------------------------------------------
    // What is shown

    /// Each row: a frame's name with its file and line (`main, main.py:12`), or the count of a run
    /// of collapsed library frames.
    pub fn frame_lines(&self, cx: &mut App) -> Vec<String> {
        let rows = self.rows(cx);
        rows.iter()
            .map(|row| match row {
                FrameRow::Frame { name, file, .. } => format!("{name}{}", file.as_deref().unwrap_or_default()),
                FrameRow::Hidden { count } => hidden_label(*count, cx),
            })
            .collect()
    }

    /// Which rows are dimmed library frames.
    pub fn dimmed_rows(&self, cx: &App) -> Vec<bool> {
        self.rows(cx).iter().map(|row| matches!(row, FrameRow::Frame { library: true, .. })).collect()
    }

    /// The row of the selected frame.
    pub fn selected_row(&self, cx: &App) -> Option<usize> {
        let selected = self.session(cx)?.selected_frame?;
        self.rows(cx).iter().position(|row| matches!(row, FrameRow::Frame { id, .. } if *id == selected))
    }

    /// Why there are no frames while nothing is suspended: the session has finished, waits for its
    /// debuggee to connect, or runs.
    pub fn note(&self, cx: &mut App) -> Option<String> {
        let (status, config) = {
            let state = &self.debugger.read(cx).state;
            let session = state.active_session()?;
            let root = state.session(state.root_of(session.id));
            (session.status, root.and_then(|root| root.run_config.clone()))
        };
        match status {
            SessionStatus::Paused => None,
            SessionStatus::Terminated => Some(tx(cx, "Finished")),
            SessionStatus::Starting | SessionStatus::Running | SessionStatus::Stopping => {
                let i18n = &mut cx.default_global::<Localization>().0;
                let waiting = config.and_then(|config| remote_waiting_note(&config.kind, &config.extras.params, i18n));
                Some(waiting.unwrap_or_else(|| i18n.tx("Running…")))
            }
        }
    }

    /// The thread chooser's text, while the session has threads: the selected thread's name.
    pub fn thread_label(&self, cx: &App) -> Option<String> {
        let session = self.session(cx).filter(|session| !session.threads.is_empty())?;
        let selected = session.selected_thread.and_then(|id| session.threads.iter().find(|thread| thread.id == id));
        Some(selected.map_or_else(String::new, |thread| thread.name.clone()))
    }

    /// The thread chooser's menu: every thread, the suspended one saying so.
    pub fn thread_menu(&self, cx: &App) -> Vec<MenuItem<i64>> {
        let Some(session) = self.session(cx) else { return Vec::new() };
        let suspended = tx(cx, "suspended");
        session
            .threads
            .iter()
            .map(|thread| {
                let item = MenuItem::item(thread.name.clone(), thread.id);
                if session.stopped_thread == Some(thread.id) { item.shortcut(suspended.clone()) } else { item }
            })
            .collect()
    }

    /// A thread was chosen in the thread chooser.
    pub fn choose_thread(&mut self, thread: i64, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx).map(|session| session.id) else { return };
        cx.emit(FramesViewEvent::SelectThread { session, thread });
    }

    /// The context menu of the frame it was opened on.
    pub fn frame_menu(&self, cx: &App) -> Vec<MenuItem<FrameCommand>> {
        let session = self.session(cx);
        let frame = session.and_then(|session| session.frames.iter().find(|frame| Some(frame.id) == self.menu_frame));
        let paused = session.is_some_and(|session| session.status == SessionStatus::Paused);
        let can_reset = paused && session.is_some_and(|session| session.supports(|caps| caps.restart_frame));
        let toggle = if self.show_all { "Hide Library Frames" } else { "Show All Frames" };
        vec![
            MenuItem::item("Jump to Source", FrameCommand::JumpToSource)
                .shortcut("F4")
                .disabled(!frame.is_some_and(|frame| frame_path(frame).is_some())),
            MenuItem::item("Reset Frame", FrameCommand::ResetFrame).icon("reset-frame").disabled(!can_reset),
            MenuItem::Separator,
            MenuItem::item(toggle, FrameCommand::ToggleLibraryFrames),
            MenuItem::item("Copy Stack", FrameCommand::CopyStack).icon("copy"),
        ]
    }

    pub fn run_frame_command(&mut self, command: FrameCommand, cx: &mut Context<Self>) {
        match command {
            FrameCommand::JumpToSource => {
                let session = self.session(cx).map(|session| session.id);
                if let (Some(session), Some(frame)) = (session, self.menu_frame) {
                    cx.emit(FramesViewEvent::JumpToSource { session, frame });
                }
            }
            FrameCommand::ResetFrame => cx.emit(FramesViewEvent::RunAction("debug.resetFrame")),
            FrameCommand::ToggleLibraryFrames => {
                self.show_all = !self.show_all;
                cx.notify();
            }
            FrameCommand::CopyStack => self.copy_stack(cx),
        }
    }

    /// Jump to Source (F4) while the pane has the focus: the selected frame's source.
    pub fn jump_to_source(&mut self, cx: &mut Context<Self>) {
        let target = self.session(cx).and_then(|session| Some((session.id, session.selected_frame?)));
        if let Some((session, frame)) = target {
            cx.emit(FramesViewEvent::JumpToSource { session, frame });
        }
    }

    /// Copy Stack: each frame as `name:line, file`, one per line.
    fn copy_stack(&self, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx) else { return };
        let text = session
            .frames
            .iter()
            .map(|frame| match frame_path(frame) {
                Some(path) => format!("{}, {}", frame_label(frame), basename(&path)),
                None => frame_label(frame),
            })
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    // ---------------------------------------------------------------------------------------------
    // Pointer and keyboard

    fn select_frame(&mut self, frame: i64, reveal: bool, cx: &mut Context<Self>) {
        if let Some(session) = self.session(cx).map(|session| session.id) {
            cx.emit(FramesViewEvent::SelectFrame { session, frame, reveal });
        }
    }

    fn clicked_frame(&mut self, frame: i64, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.select_frame(frame, true, cx);
    }

    fn opened_frame_menu(&mut self, frame: i64, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.menu_frame = Some(frame);
        self.select_frame(frame, false, cx);
        cx.emit(FramesViewEvent::ShowFrameMenu(at));
    }

    fn show_all_frames(&mut self, cx: &mut Context<Self>) {
        self.show_all = true;
        cx.notify();
    }

    /// Up and Down select the frame above or below the selected one, Home and End the first and the
    /// last; the frame's source opens without the focus.
    fn key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let key = keystroke.key.as_str();
        if !matches!(key, "up" | "down" | "home" | "end") {
            return false;
        }
        let ids = self.frame_ids(cx);
        let selected = self.session(cx).and_then(|session| session.selected_frame);
        let current = selected.and_then(|selected| ids.iter().position(|id| *id == selected));
        let next = match key {
            "down" => Some(current.map_or(0, |at| at + 1)),
            "up" => current.and_then(|at| at.checked_sub(1)),
            "home" => Some(0),
            _ => ids.len().checked_sub(1),
        };
        if let Some(frame) = next.and_then(|at| ids.get(at)).copied().filter(|frame| Some(*frame) != selected) {
            self.select_frame(frame, true, cx);
            let row = self.rows(cx).iter().position(|row| matches!(row, FrameRow::Frame { id, .. } if *id == frame));
            if let Some(row) = row {
                self.scroll.scroll_to_item(row, ScrollStrategy::Top);
            }
        }
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn render_thread_chooser(&self, label: String, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("debug-thread-chooser")
            .debug_selector(|| "debug-thread-chooser".to_owned())
            .flex_none()
            .h(px(24.0))
            .mx(px(6.0))
            .my(px(4.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .text_color(theme::color(cx, "fg"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(SharedString::from(label)))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, event: &MouseDownEvent, _, cx| {
                    cx.emit(FramesViewEvent::ShowThreadMenu(event.position));
                }),
            )
    }
}

/// `3 library frames hidden`.
fn hidden_label(count: usize, cx: &mut App) -> String {
    cx.default_global::<Localization>().0.tx_built(&["", " library frames hidden"], &[Param::from(count)])
}

/// The last component of a path.
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl Focusable for FramesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The colours and metrics of the rows, read once per frame.
#[derive(Clone, Copy)]
struct RowStyle {
    fg: Hsla,
    muted: Hsla,
    subtle: Hsla,
    hover: Hsla,
    selected: Hsla,
    height: Pixels,
    radius: Pixels,
}

/// Row `index` of the list; `selected`: the selected frame.
fn render_row(
    row: &FrameRow,
    index: usize,
    selected: Option<i64>,
    style: RowStyle,
    cx: &mut Context<FramesView>,
) -> AnyElement {
    let selector = format!("debug-frame-{index}");
    let base = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .h(style.height)
        .w_full()
        .px(px(6.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded(style.radius)
        .whitespace_nowrap()
        .overflow_hidden();
    match row {
        FrameRow::Hidden { count } => base
            .text_color(style.subtle)
            .hover(|el| el.bg(style.hover))
            .child(SharedString::from(hidden_label(*count, cx)))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.show_all_frames(cx)))
            .into_any_element(),
        FrameRow::Frame { id, name, file, library } => {
            let frame = *id;
            let is_selected = selected == Some(frame);
            let color = if *library { style.muted } else { style.fg };
            base.when(is_selected, |el| el.bg(style.selected))
                .when(!is_selected, |el| el.hover(|el| el.bg(style.hover)))
                .child(theme::icon(cx, "method", 16.0, color))
                .child(div().flex_none().text_color(color).child(SharedString::from(name.clone())))
                .children(file.clone().map(|file| {
                    div().min_w_0().overflow_hidden().text_color(style.subtle).child(SharedString::from(file))
                }))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.clicked_frame(frame, window, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                        view.opened_frame_menu(frame, event.position, window, cx);
                    }),
                )
                .into_any_element()
        }
    }
}

impl Render for FramesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::new(self.rows(cx));
        let note = self.note(cx);
        let thread = self.thread_label(cx);
        let selected = self.session(cx).and_then(|session| session.selected_frame);
        let focused = self.focus.is_focused(window);
        let style = RowStyle {
            fg: theme::color(cx, "fg"),
            muted: theme::color(cx, "fg-muted"),
            subtle: theme::color(cx, "fg-subtle"),
            hover: theme::color(cx, "bg-tree-hover"),
            selected: theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" }),
            height: theme::length(cx, "h-row"),
            radius: theme::length(cx, "radius-small"),
        };
        let subtle = style.subtle;
        let count = rows.len();
        let list = uniform_list(
            "debug-frames",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let start = range.start;
                rows[range]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| render_row(row, start + offset, selected, style, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .debug_selector(|| "debug-frames".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .children(thread.map(|label| self.render_thread_chooser(label, cx)))
            .children(note.map(|note| {
                div()
                    .debug_selector(|| "debug-frames-note".to_owned())
                    .flex_none()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(subtle)
                    .child(SharedString::from(note))
            }))
            .child(div().flex_1().min_h_0().px(px(2.0)).child(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::state::testing::frame;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_dap::{DebugCapabilities, ThreadInfo};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<FramesViewEvent>>>;

    /// A debugger whose session 1 `init` describes.
    fn debugger(init: impl FnOnce(&mut DebugSession)) -> Debugger {
        let mut debugger = Debugger::default();
        debugger.state.adopt_session(DebugSessionId(1), 0.0, init);
        debugger.state.take_effects();
        debugger
    }

    /// Suspended at a breakpoint in `main`, called from `run` and a library frame.
    fn suspended(session: &mut DebugSession) {
        session.name = "main".to_owned();
        session.status = SessionStatus::Paused;
        session.stopped_thread = Some(1);
        session.threads =
            vec![ThreadInfo { id: 1, name: "MainThread".into() }, ThreadInfo { id: 2, name: "worker".into() }];
        session.selected_thread = Some(1);
        session.frames = vec![
            frame(7, "main", "/p/app/main.py", 11),
            frame(8, "run", "/p/app/run.py", 3),
            frame(9, "invoke", "/usr/lib/python3/runpy.py", 40),
            frame(10, "call", "/usr/lib/python3/runpy.py", 80),
            frame(11, "start", "/p/app/start.py", 0),
        ];
        session.selected_frame = Some(7);
    }

    fn open(
        cx: &mut TestAppContext,
        debugger: Debugger,
    ) -> (Entity<FramesView>, Entity<Debugger>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let debugger = cx.update(|cx| cx.new(|_| debugger));
        let shared = debugger.clone();
        let (view, cx) = cx.add_window_view(|window, cx| FramesView::new(shared, window, cx));
        view.update(cx, |view, cx| view.set_project_root(Some("/p".into()), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &FramesViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, debugger, events, cx)
    }

    fn lines(view: &Entity<FramesView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.update(cx, |view, cx| view.frame_lines(cx))
    }

    /// Focuses the pane in an active window: focus listeners only report to an active window.
    fn focus(view: &Entity<FramesView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.activate_window();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn selections(events: &Events) -> Vec<(i64, bool)> {
        let selected = |event: &FramesViewEvent| match event {
            FramesViewEvent::SelectFrame { frame, reveal, .. } => Some((*frame, *reveal)),
            _ => None,
        };
        events.borrow().iter().filter_map(selected).collect()
    }

    // Not from a spec: a run of library frames collapses into one row that shows them on a click;
    // a library frame on its own stays, dimmed.
    #[gpui_kit::test]
    async fn library_frames_collapse_and_show_dimmed_on_a_click(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, debugger(suspended));
        assert_eq!(
            lines(&view, cx),
            ["main, main.py:12", "run, run.py:4", "2 library frames hidden", "start, start.py:1"]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.selected_row(cx)), Some(0));
        let centre = cx.debug_bounds("debug-frame-2").expect("the hidden frames row is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(lines(&view, cx).len(), 5);
        assert_eq!(view.read_with(cx, |view, cx| view.dimmed_rows(cx)), [false, false, true, true, false]);
        let labels: Vec<String> = view.read_with(cx, |view, cx| {
            view.frame_menu(cx).iter().filter_map(|item| item.label().map(str::to_owned)).collect()
        });
        assert!(labels.contains(&"Hide Library Frames".to_owned()), "{labels:?}");
    }

    // Not from a spec: Up and Down select the neighbouring frame, revealing its source; the model
    // decides the selection, so the pane follows it.
    #[gpui_kit::test]
    async fn the_arrow_keys_select_the_neighbouring_frames(cx: &mut TestAppContext) {
        let (view, debugger, events, cx) = open(cx, debugger(suspended));
        focus(&view, cx);
        cx.simulate_keystrokes("up");
        assert!(selections(&events).is_empty(), "the top frame has nothing above it");
        cx.simulate_keystrokes("down");
        assert_eq!(selections(&events), [(8, true)]);
        debugger.update(cx, |debugger, cx| {
            debugger.state.select_frame(DebugSessionId(1), 8, false);
            debugger.state.take_effects();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.selected_row(cx)), Some(1));
        cx.simulate_keystrokes("end");
        assert_eq!(selections(&events).last(), Some(&(11, true)));
        cx.simulate_keystrokes("home");
        assert_eq!(selections(&events).last(), Some(&(7, true)));
        assert!(events.borrow().contains(&FramesViewEvent::Focused(COMMANDS)), "{:?}", events.borrow());
    }

    // Not from a spec: a click selects a frame and reveals it; the context menu selects without
    // revealing, offers Reset Frame only where the adapter restarts frames, and copies the stack.
    #[gpui_kit::test]
    async fn the_frame_menu_follows_the_frame_and_the_adapter(cx: &mut TestAppContext) {
        let (view, debugger, events, cx) = open(cx, debugger(suspended));
        let centre = cx.debug_bounds("debug-frame-1").expect("the second frame is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(selections(&events), [(8, true)]);
        cx.update(|window, cx| view.update(cx, |view, cx| view.opened_frame_menu(11, Point::default(), window, cx)));
        cx.run_until_parked();
        assert_eq!(selections(&events).last(), Some(&(11, false)));
        assert!(events.borrow().contains(&FramesViewEvent::ShowFrameMenu(Point::default())));
        let reset = |view: &Entity<FramesView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| {
                let menu = view.frame_menu(cx);
                menu.iter().find(|item| item.label() == Some("Reset Frame")).map(MenuItem::is_disabled)
            })
        };
        assert_eq!(reset(&view, cx), Some(true));
        debugger.update(cx, |debugger, cx| {
            if let Some(session) = debugger.state.session_mut(DebugSessionId(1)) {
                session.caps = Some(DebugCapabilities { restart_frame: true, ..DebugCapabilities::default() });
            }
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(reset(&view, cx), Some(false));
        view.update(cx, |view, cx| view.run_frame_command(FrameCommand::ResetFrame, cx));
        assert_eq!(events.borrow().last(), Some(&FramesViewEvent::RunAction("debug.resetFrame")));
        view.update(cx, |view, cx| view.run_frame_command(FrameCommand::JumpToSource, cx));
        let jump = FramesViewEvent::JumpToSource { session: DebugSessionId(1), frame: 11 };
        assert_eq!(events.borrow().last(), Some(&jump));
        view.update(cx, |view, cx| view.run_frame_command(FrameCommand::CopyStack, cx));
        let copied = cx.read_from_clipboard().and_then(|item| item.text()).expect("the stack is copied");
        assert_eq!(copied.lines().collect::<Vec<_>>()[..2], ["main:12, main.py", "run:4, run.py"]);
    }

    // Not from a spec: the thread chooser names the selected thread and lists every thread, the
    // suspended one marked; choosing one asks for its stack.
    #[gpui_kit::test]
    async fn the_thread_chooser_lists_every_thread(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, debugger(suspended));
        assert_eq!(view.read_with(cx, |view, cx| view.thread_label(cx)).as_deref(), Some("MainThread"));
        let menu = view.read_with(cx, |view, cx| view.thread_menu(cx));
        assert_eq!(menu, [MenuItem::item("MainThread", 1).shortcut("suspended"), MenuItem::item("worker", 2)]);
        let centre = cx.debug_bounds("debug-thread-chooser").expect("the chooser is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(FramesViewEvent::ShowThreadMenu(_))));
        view.update(cx, |view, cx| view.choose_thread(2, cx));
        let chosen = FramesViewEvent::SelectThread { session: DebugSessionId(1), thread: 2 };
        assert_eq!(events.borrow().last(), Some(&chosen));
    }

    // Not from a spec: while nothing is suspended the pane says whether the session runs or has
    // finished, in place of the frames.
    #[gpui_kit::test]
    async fn without_a_suspended_thread_the_pane_says_why_there_are_no_frames(cx: &mut TestAppContext) {
        let (view, debugger, _, cx) = open(cx, debugger(suspended));
        assert_eq!(view.update(cx, |view, cx| view.note(cx)), None);
        let set_status = |status: SessionStatus, debugger: &Entity<Debugger>, cx: &mut VisualTestContext| {
            debugger.update(cx, |debugger, cx| {
                if let Some(session) = debugger.state.session_mut(DebugSessionId(1)) {
                    session.status = status;
                    session.set_frames(Vec::new());
                    session.selected_frame = None;
                }
                cx.notify();
            });
            cx.run_until_parked();
        };
        set_status(SessionStatus::Running, &debugger, cx);
        assert_eq!(view.update(cx, |view, cx| view.note(cx)).as_deref(), Some("Running…"));
        assert!(lines(&view, cx).is_empty());
        assert!(cx.debug_bounds("debug-frames-note").is_some());
        set_status(SessionStatus::Terminated, &debugger, cx);
        assert_eq!(view.update(cx, |view, cx| view.note(cx)).as_deref(), Some("Finished"));
    }
}
