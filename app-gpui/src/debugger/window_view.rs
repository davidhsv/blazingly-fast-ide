//! The Debug tool window: a tab per debug session (its name, a close button, italic once it has
//! finished), the toolbar of the shown session (Threads & Variables and Console, Rerun, Stop,
//! Resume, Pause, the steps, Hot Reload and Hot Restart for Dart, View Breakpoints, Mute
//! Breakpoints and the More menu), and below it the Frames pane beside the Variables pane, or the
//! session's console. Without a session it says how to start one.
//!
//! The view chooses the Console while a session starts and Threads & Variables when it suspends;
//! otherwise the tab stays what the user picked. A toolbar button is enabled when the context key
//! of its action's `when` clause is true in what the debugger published.
//!
//! [`DebugWindowView`] reads the [`Debugger`] and changes nothing in it: the buttons run the
//! debugger's actions and the tabs select and close sessions, through [`DebugWindowEvent`]s. The
//! console is the caller's view, given with [`DebugWindowView::set_console`].

use crate::debugger::actions::Debugger;
use crate::debugger::frames_view::FramesView;
use crate::debugger::state::{DebugContext, DebugSession, SessionStatus};
use crate::debugger::variables_view::VariablesView;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_core::DebugSessionId;

/// Width of the Frames pane beside the Variables pane.
const FRAMES_WIDTH: f32 = 300.0;

/// What the Debug tool window asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum DebugWindowEvent {
    /// Show this session: `DebugState::select_session`.
    SelectSession(DebugSessionId),
    /// Close the session's tab: `DebugState::close_session_tab`, asking first when it says so.
    CloseTab(DebugSessionId),
    /// Run this debugger action (`Debugger::run_action`).
    RunAction(&'static str),
    /// Show [`DebugWindowView::more_menu`] here.
    ShowMoreMenu(Point<Pixels>),
}

/// The views of the shown session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugTab {
    /// Threads & Variables: the frames beside the variables.
    Variables,
    Console,
}

/// A button of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub action: &'static str,
    pub icon: &'static str,
    pub enabled: bool,
    /// A toggle that is on (Mute Breakpoints).
    pub pressed: bool,
    /// Drawn in the danger colour while enabled (Stop).
    pub danger: bool,
}

impl ToolbarButton {
    fn new(action: &'static str, icon: &'static str, enabled: bool) -> Self {
        Self { action, icon, enabled, pressed: false, danger: false }
    }
}

pub struct DebugWindowView {
    debugger: Entity<Debugger>,
    frames: Entity<FramesView>,
    variables: Entity<VariablesView>,
    console: Option<AnyView>,
    focus: FocusHandle,
    tab: DebugTab,
    /// The shown session, when it started and its status, as the tab was last chosen for them.
    seen: Option<(DebugSessionId, Option<u64>, SessionStatus)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DebugWindowEvent> for DebugWindowView {}

impl DebugWindowView {
    /// The tool window over `debugger`, with its Frames and Variables panes; subscribe to them
    /// ([`DebugWindowView::frames`], [`DebugWindowView::variables`]) for what they ask.
    pub fn new(debugger: Entity<Debugger>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let frames_of = debugger.clone();
        let frames = cx.new(|cx| FramesView::new(frames_of, window, cx));
        let variables_of = debugger.clone();
        let variables = cx.new(|cx| VariablesView::new(variables_of, window, cx));
        let subscriptions = vec![cx.observe(&debugger, |view, _, cx| {
            view.sync_tab(cx);
            cx.notify();
        })];
        let mut view = Self {
            debugger,
            frames,
            variables,
            console: None,
            focus: cx.focus_handle(),
            tab: DebugTab::Variables,
            seen: None,
            _subscriptions: subscriptions,
        };
        view.sync_tab(cx);
        view
    }

    pub fn frames(&self) -> &Entity<FramesView> {
        &self.frames
    }

    pub fn variables(&self) -> &Entity<VariablesView> {
        &self.variables
    }

    /// The console of the shown session, drawn in the Console tab.
    pub fn set_console(&mut self, console: Option<AnyView>, cx: &mut Context<Self>) {
        self.console = console;
        cx.notify();
    }

    /// A session that starts shows its console (the preparation's output), one that suspends its
    /// frames and variables; another change of the session keeps the tab the user picked.
    fn sync_tab(&mut self, cx: &App) {
        let shown = self
            .debugger
            .read(cx)
            .state
            .active_session()
            .map(|session| (session.id, session.started_at.map(f64::to_bits), session.status));
        if shown == self.seen {
            return;
        }
        match shown.map(|(_, _, status)| status) {
            Some(SessionStatus::Starting) => self.tab = DebugTab::Console,
            Some(SessionStatus::Paused) => self.tab = DebugTab::Variables,
            _ => {}
        }
        self.seen = shown;
    }

    pub fn active_tab(&self) -> DebugTab {
        self.tab
    }

    pub fn select_tab(&mut self, tab: DebugTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Session tabs

    fn context(&self, cx: &App) -> DebugContext {
        self.debugger.read(cx).state.published().context
    }

    /// The session shown.
    pub fn active_session(&self, cx: &App) -> Option<DebugSessionId> {
        self.debugger.read(cx).state.active_session().map(|session| session.id)
    }

    fn sessions<'a>(&self, cx: &'a App) -> Vec<&'a DebugSession> {
        self.debugger.read(cx).state.sessions().values().collect()
    }

    /// Each tab's text: the session's name, a child session's marked as such.
    pub fn tab_titles(&self, cx: &mut App) -> Vec<String> {
        let sessions: Vec<(String, bool)> =
            self.sessions(cx).iter().map(|session| (session.name.clone(), session.parent.is_some())).collect();
        let i18n = &mut cx.default_global::<Localization>().0;
        sessions
            .into_iter()
            .map(|(name, child)| if child { i18n.tx_built(&["", " (child)"], &[Param::from(name)]) } else { name })
            .collect()
    }

    /// Which tabs are of sessions that have finished (drawn in italics).
    pub fn finished_tabs(&self, cx: &App) -> Vec<bool> {
        self.sessions(cx).iter().map(|session| !session.is_live()).collect()
    }

    /// Each tab's status: `Starting…`, `Running`, `Suspended`, `Finished · exit code 1`.
    pub fn tab_statuses(&self, cx: &mut App) -> Vec<String> {
        let sessions: Vec<(SessionStatus, Option<i32>)> =
            self.sessions(cx).iter().map(|session| (session.status, session.exit_code)).collect();
        let i18n = &mut cx.default_global::<Localization>().0;
        sessions
            .into_iter()
            .map(|(status, exit_code)| match (status, exit_code) {
                (SessionStatus::Terminated, Some(code)) => {
                    i18n.tx_built(&["Finished · exit code ", ""], &[Param::from(code)])
                }
                (SessionStatus::Terminated, None) => i18n.tx("Finished"),
                (SessionStatus::Starting, _) => i18n.tx("Starting…"),
                (SessionStatus::Paused, _) => i18n.tx("Suspended"),
                (SessionStatus::Running | SessionStatus::Stopping, _) => i18n.tx("Running"),
            })
            .collect()
    }

    // ---------------------------------------------------------------------------------------------
    // Toolbar

    /// The toolbar's buttons in groups, each enabled as its action's `when` clause reads the
    /// debugger's context keys.
    fn toolbar_groups(&self, cx: &App) -> Vec<Vec<ToolbarButton>> {
        let context = self.context(cx);
        let paused = context.session_paused;
        let muted = self.debugger.read(cx).breakpoints_muted();
        let mut groups = vec![
            vec![
                ToolbarButton::new("debug.rerun", "rerun", context.can_rerun),
                ToolbarButton { danger: true, ..ToolbarButton::new("debug.stop", "stop", context.session_active) },
            ],
            vec![
                ToolbarButton::new("debug.resume", "resume", paused),
                ToolbarButton::new("debug.pause", "pause", context.session_running),
                ToolbarButton::new("debug.stepOver", "step-over", paused),
                ToolbarButton::new("debug.stepInto", "step-into", paused),
                ToolbarButton::new("debug.stepOut", "step-out", paused),
            ],
        ];
        if context.session_dart {
            let hot = context.session_running;
            groups.push(vec![
                ToolbarButton::new("debug.hotReload", "hot-reload", hot),
                ToolbarButton::new("debug.hotRestart", "refresh", hot),
            ]);
        }
        groups.push(vec![
            ToolbarButton::new("debug.viewBreakpoints", "view-breakpoints", true),
            ToolbarButton { pressed: muted, ..ToolbarButton::new("debug.muteBreakpoints", "mute-breakpoint", true) },
        ]);
        groups
    }

    /// The toolbar's buttons, in order (the More button after them is always there).
    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarButton> {
        self.toolbar_groups(cx).into_iter().flatten().collect()
    }

    /// The More menu of the toolbar: the less used stepping and evaluation actions.
    pub fn more_menu() -> Vec<MenuItem<()>> {
        vec![
            MenuItem::action("debug.forceStepOver"),
            MenuItem::action("debug.forceStepInto"),
            MenuItem::action("debug.smartStepInto"),
            MenuItem::Separator,
            MenuItem::action("debug.runToCursor"),
            MenuItem::action("debug.forceRunToCursor"),
            MenuItem::Separator,
            MenuItem::action("debug.showExecutionPoint"),
            MenuItem::Separator,
            MenuItem::action("debug.evaluate"),
            MenuItem::action("debug.resetFrame"),
        ]
    }

    /// What the tool window says while there is no session: its title and the way to start one.
    pub fn empty_text(&self, cx: &App) -> Option<Vec<String>> {
        if self.debugger.read(cx).state.active_session().is_some() {
            return None;
        }
        let shortcut = cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label("run.debug", is_mac_host()));
        let (start, click) =
            (tx(cx, "Start a debug configuration with"), tx(cx, "or click a line number to set a breakpoint."));
        let hint = match shortcut {
            Some(shortcut) => format!("{start} {shortcut} {click}"),
            None => format!("{start} {click}"),
        };
        Some(vec![tx(cx, "Not debugging"), hint])
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn render_session_tabs(&self, cx: &mut Context<Self>) -> Div {
        let titles = self.tab_titles(cx);
        let finished_tabs = self.finished_tabs(cx);
        let ids: Vec<DebugSessionId> = self.sessions(cx).iter().map(|session| session.id).collect();
        let active = self.active_session(cx);
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
        for (index, ((id, title), finished)) in ids.into_iter().zip(titles).zip(finished_tabs).enumerate() {
            let selected = active == Some(id);
            let color = theme::color(cx, if finished { "fg-muted" } else { "fg" });
            let tab_selector = format!("debug-session-tab-{index}");
            let close_selector = format!("debug-session-close-{index}");
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
                    .when(finished, |el| el.italic())
                    .when(selected, |el| el.bg(theme::color(cx, "bg-tab-selected")))
                    .when(!selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(theme::icon(cx, "debug", 14.0, color))
                    .child(SharedString::from(title))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(DebugWindowEvent::SelectSession(id))))
                    .child(
                        div()
                            .id(SharedString::from(close_selector.clone()))
                            .debug_selector(move || close_selector.clone())
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.0))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-muted")))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                cx.emit(DebugWindowEvent::CloseTab(id));
                            })),
                    ),
            );
        }
        row
    }

    fn tool_button(&self, button: ToolbarButton, cx: &mut Context<Self>) -> Stateful<Div> {
        let ToolbarButton { action, icon, enabled, pressed, danger } = button;
        let color = theme::color(
            cx,
            if !enabled {
                "fg-faint"
            } else if danger {
                "danger"
            } else if pressed {
                "fg-strong"
            } else {
                "fg-muted"
            },
        );
        let selector = format!("debug-tool-{action}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(DebugWindowEvent::RunAction(action))))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn view_tab(&self, tab: DebugTab, cx: &mut Context<Self>) -> Stateful<Div> {
        let (selector, label) = match tab {
            DebugTab::Variables => ("debug-tab-variables", "Threads & Variables"),
            DebugTab::Console => ("debug-tab-console", "Console"),
        };
        let selected = self.tab == tab;
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .h(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .text_color(theme::color(cx, if selected { "fg-strong" } else { "fg" }))
            .when(selected, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
            .when(!selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
            .child(SharedString::from(tx(cx, label)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(tab, cx)))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let rule = theme::color(cx, "border");
        let separator = move || div().flex_none().w(px(1.0)).h(px(16.0)).mx(px(4.0)).bg(rule);
        let mut bar = div()
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(rule)
            .child(self.view_tab(DebugTab::Variables, cx))
            .child(self.view_tab(DebugTab::Console, cx));
        for group in self.toolbar_groups(cx) {
            bar = bar.child(separator());
            for button in group {
                bar = bar.child(self.tool_button(button, cx));
            }
        }
        bar.child(
            div()
                .id("debug-tool-more")
                .debug_selector(|| "debug-tool-more".to_owned())
                .size(px(24.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
                .child(theme::icon(cx, "more", 16.0, theme::color(cx, "fg-muted")))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, event: &MouseDownEvent, _, cx| {
                        cx.emit(DebugWindowEvent::ShowMoreMenu(event.position));
                    }),
                ),
        )
    }

    fn render_panel(&self, cx: &mut Context<Self>) -> Div {
        let panel = div().flex_1().min_h_0().flex();
        match self.tab {
            DebugTab::Variables => panel
                .child(
                    div()
                        .w(px(FRAMES_WIDTH))
                        .flex_none()
                        .h_full()
                        .border_r_1()
                        .border_color(theme::color(cx, "border"))
                        .child(self.frames.clone()),
                )
                .child(div().flex_1().min_w_0().h_full().child(self.variables.clone())),
            DebugTab::Console => panel
                .debug_selector(|| "debug-console".to_owned())
                .child(div().size_full().children(self.console.clone())),
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let lines = self.empty_text(cx).unwrap_or_default();
        div()
            .debug_selector(|| "debug-empty".to_owned())
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .p(px(24.0))
            .children(
                lines
                    .first()
                    .cloned()
                    .map(|title| div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title))),
            )
            .children(
                lines
                    .get(1)
                    .cloned()
                    .map(|hint| div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(hint))),
            )
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl Focusable for DebugWindowView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DebugWindowView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_sessions = !self.debugger.read(cx).state.sessions().is_empty();
        let active = self.active_session(cx).is_some();
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        let root = if has_sessions { root.child(self.render_session_tabs(cx)) } else { root };
        if !active {
            return root.child(self.render_empty(cx));
        }
        root.child(self.render_toolbar(cx)).child(self.render_panel(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::effects::testing::Settings;
    use crate::debugger::frames_view::FramesViewEvent;
    use crate::debugger::state::testing::frame;
    use crate::debugger::state::{CloseTab, Ctx};
    use crate::debugger::variables_view::VariablesViewEvent;
    use crate::platform::i18n::I18n;
    use crate::platform::input;
    use crate::run::configs::blank_config;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_dap::{Scope, ThreadInfo, Variable};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DebugWindowEvent>>>;
    type PaneEvents = Rc<RefCell<Vec<VariablesViewEvent>>>;

    /// `main`, suspended at a breakpoint in `/p/app/main.py`, as debugpy reports it.
    fn suspended(session: &mut DebugSession) {
        session.name = "main".to_owned();
        session.adapter = "debugpy".to_owned();
        session.status = SessionStatus::Paused;
        session.stopped_thread = Some(1);
        session.stop_reason = Some("breakpoint".to_owned());
        session.threads = vec![ThreadInfo { id: 1, name: "MainThread".into() }];
        session.selected_thread = Some(1);
        session.frames = vec![frame(7, "main", "/p/app/main.py", 11)];
        session.selected_frame = Some(7);
        session.hits = 1;
    }

    /// A debugger with `sessions` (id, name, status, each otherwise [`suspended`], without frames
    /// unless suspended) showing `active`.
    fn debugger(sessions: &[(u32, &str, SessionStatus)], active: u32) -> Debugger {
        let mut debugger = Debugger::default();
        for (id, name, status) in sessions {
            debugger.state.adopt_session(DebugSessionId(*id), 0.0, |session| {
                suspended(session);
                session.name = (*name).to_owned();
                session.status = *status;
                if *status != SessionStatus::Paused {
                    session.set_frames(Vec::new());
                    session.selected_frame = None;
                    session.stopped_thread = None;
                }
            });
        }
        debugger.state.select_session(DebugSessionId(active));
        debugger.state.take_effects();
        debugger
    }

    fn open(
        cx: &mut TestAppContext,
        debugger: Debugger,
    ) -> (Entity<DebugWindowView>, Entity<Debugger>, Events, PaneEvents, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        // The pane starts over an empty debugger and is handed the sessions once the events are
        // recorded: opening a test window runs the event loop, so requests made on the view's first
        // turn would otherwise go out before anyone listens.
        let (debugger, sessions) = (cx.update(|cx| cx.new(|_| Debugger::default())), debugger);
        let shared = debugger.clone();
        let (view, cx) = cx.add_window_view(|window, cx| DebugWindowView::new(shared, window, cx));
        let (events, panes): (Events, PaneEvents) = (Rc::default(), Rc::default());
        let (seen, pane_seen) = (Rc::clone(&events), Rc::clone(&panes));
        let variables = view.read_with(cx, |view, _| view.variables().clone());
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &DebugWindowEvent, _| seen.borrow_mut().push(event.clone())).detach();
            cx.subscribe(&variables, move |_, event: &VariablesViewEvent, _| {
                pane_seen.borrow_mut().push(event.clone());
            })
            .detach();
        });
        debugger.update(cx, |debugger, cx| {
            *debugger = sessions;
            cx.notify();
        });
        cx.run_until_parked();
        (view, debugger, events, panes, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn change(debugger: &Entity<Debugger>, id: u32, cx: &mut VisualTestContext, edit: impl FnOnce(&mut DebugSession)) {
        debugger.update(cx, |debugger, cx| {
            if let Some(session) = debugger.state.session_mut(DebugSessionId(id)) {
                edit(session);
            }
            debugger.state.publish_context();
            cx.notify();
        });
        cx.run_until_parked();
    }

    /// Closes the tab of session `id` as the workbench does on [`DebugWindowEvent::CloseTab`].
    fn close(debugger: &Entity<Debugger>, id: u32, cx: &mut VisualTestContext) -> CloseTab {
        let closed = debugger.update(cx, |debugger, cx| {
            let settings = Settings::default();
            let mut i18n = I18n::default();
            let mut ctx = Ctx { breakpoints: &mut debugger.breakpoints, env: settings.env(0.0), i18n: &mut i18n };
            let closed = debugger.state.close_session_tab(DebugSessionId(id), &mut ctx);
            debugger.state.take_effects();
            cx.notify();
            closed
        });
        cx.run_until_parked();
        closed
    }

    fn actions(view: &Entity<DebugWindowView>, cx: &mut VisualTestContext) -> Vec<(&'static str, bool)> {
        view.read_with(cx, |view, cx| view.toolbar(cx).iter().map(|button| (button.action, button.enabled)).collect())
    }

    fn titles(view: &Entity<DebugWindowView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.update(cx, |view, cx| view.tab_titles(cx))
    }

    fn active(view: &Entity<DebugWindowView>, cx: &mut VisualTestContext) -> Option<DebugSessionId> {
        view.read_with(cx, |view, cx| view.active_session(cx))
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › offers Hot Reload and Hot
    // Restart for Dart and Flutter sessions only
    #[gpui_kit::test]
    async fn offers_hot_reload_and_hot_restart_for_dart_and_flutter_sessions_only(cx: &mut TestAppContext) {
        let (view, debugger, _, _, cx) = open(cx, debugger(&[(1, "main", SessionStatus::Running)], 1));
        let hot = |view: &Entity<DebugWindowView>, cx: &mut VisualTestContext| {
            actions(view, cx)
                .into_iter()
                .filter(|(action, _)| matches!(*action, "debug.hotReload" | "debug.hotRestart"))
                .collect::<Vec<_>>()
        };
        assert!(hot(&view, cx).is_empty());
        change(&debugger, 1, cx, |session| session.adapter = "dart".to_owned());
        assert_eq!(hot(&view, cx), [("debug.hotReload", true), ("debug.hotRestart", true)]);
        assert!(cx.debug_bounds("debug-tool-debug.hotReload").is_some());
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › says a PHP Remote Debug
    // session listens for Xdebug while nothing is suspended
    #[gpui_kit::test]
    async fn says_a_php_remote_debug_session_listens_for_xdebug_while_nothing_is_suspended(cx: &mut TestAppContext) {
        let mut world = debugger(&[(1, "web", SessionStatus::Running)], 1);
        let mut config = blank_config("php-remote:web", "web", "php-remote");
        config.debug_adapter = Some("php-debug".to_owned());
        config.extras.params.insert("port".to_owned(), "9010".to_owned());
        if let Some(session) = world.state.session_mut(DebugSessionId(1)) {
            session.adapter = "php-debug".to_owned();
            session.threads.clear();
            session.set_frames(Vec::new());
            session.selected_frame = None;
            session.run_config = Some(config);
        }
        let (view, debugger, _, _, cx) = open(cx, world);
        click(cx, "debug-tab-variables");
        assert_eq!(view.read_with(cx, |view, _| view.active_tab()), DebugTab::Variables);
        let frames = view.read_with(cx, |view, _| view.frames().clone());
        let note = |cx: &mut VisualTestContext| frames.update(cx, |frames, cx| frames.note(cx));
        assert_eq!(note(cx).as_deref(), Some("Listening for Xdebug on port 9010"));
        change(&debugger, 1, cx, |session| session.run_config = None);
        assert_eq!(note(cx).as_deref(), Some("Running…"));
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › shows preparation output,
    // preserves a chosen tab and opens variables on pause
    #[gpui_kit::test]
    async fn shows_preparation_output_preserves_a_chosen_tab_and_opens_variables_on_pause(cx: &mut TestAppContext) {
        let (view, debugger, _, _, cx) = open(cx, debugger(&[(1, "main", SessionStatus::Starting)], 1));
        let tab = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.active_tab());
        assert_eq!(tab(cx), DebugTab::Console);
        assert!(cx.debug_bounds("debug-console").is_some());
        click(cx, "debug-tab-variables");
        assert_eq!(tab(cx), DebugTab::Variables);
        click(cx, "debug-tab-console");
        assert_eq!(tab(cx), DebugTab::Console);
        change(&debugger, 1, cx, |session| session.status = SessionStatus::Running);
        assert_eq!(tab(cx), DebugTab::Console);
        change(&debugger, 1, cx, |session| session.status = SessionStatus::Paused);
        assert_eq!(tab(cx), DebugTab::Variables);
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › renders session tabs, views,
    // frames and typed variables
    #[gpui_kit::test]
    async fn renders_session_tabs_views_frames_and_typed_variables(cx: &mut TestAppContext) {
        let (view, _, _, panes, cx) = open(cx, debugger(&[(1, "main", SessionStatus::Paused)], 1));
        assert_eq!(titles(&view, cx), ["main"]);
        assert!(cx.debug_bounds("debug-session-tab-0").is_some());
        assert!(cx.debug_bounds("debug-tab-variables").is_some() && cx.debug_bounds("debug-tab-console").is_some());
        let frames = view.read_with(cx, |view, _| view.frames().clone());
        assert_eq!(frames.update(cx, |frames, cx| frames.frame_lines(cx)), ["main, main.py:12"]);
        let variables = view.read_with(cx, |view, _| view.variables().clone());
        let placeholder = variables.read_with(cx, |variables, _| variables.eval_placeholder().to_owned());
        assert_eq!(placeholder, "Evaluate expression (↩) or add a watch (⇧⌘↩)");
        let scopes = panes.borrow().iter().find_map(|event| match event {
            VariablesViewEvent::LoadScopes { request, frame: 7, .. } => Some(*request),
            _ => None,
        });
        let locals = Scope { name: "Locals".to_owned(), variables_reference: 1, expensive: false };
        variables.update(cx, |variables, cx| {
            variables.scopes_loaded(scopes.expect("the scopes are asked for"), Ok(vec![locals]), cx);
        });
        let children = panes.borrow().iter().find_map(|event| match event {
            VariablesViewEvent::LoadChildren { request, reference: 1, .. } => Some(*request),
            _ => None,
        });
        let variable = |name: &str, value: &str, type_name: &str| Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            type_name: Some(type_name.to_owned()),
            variables_reference: 0,
            evaluate_name: Some(name.to_owned()),
        };
        let values = vec![
            variable("count", "3", "int"),
            variable("label", "'hi'", "str"),
            variable("bar", "high    7.86\nlow     7.66\nName: 772, dtype: object", "Series"),
        ];
        variables.update(cx, |variables, cx| {
            variables.children_loaded(children.expect("the locals are asked for"), Ok(values), cx);
        });
        cx.run_until_parked();
        assert_eq!(
            variables.read_with(cx, |variables, cx| variables.variable_lines(cx)),
            [
                "count = {int} 3",
                "label = {str} 'hi'",
                // A multi-line rendering stays on its row.
                "bar = {Series} high    7.86 low     7.66 Name: 772, dtype: object",
            ]
        );
        assert!(cx.debug_bounds("debug-variable-2").is_some());
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › closes a session tab and moves
    // the selection to the one left
    #[gpui_kit::test]
    async fn closes_a_session_tab_and_moves_the_selection_to_the_one_left(cx: &mut TestAppContext) {
        let sessions = [(1, "main", SessionStatus::Terminated), (2, "app", SessionStatus::Terminated)];
        let (view, debugger, events, _, cx) = open(cx, debugger(&sessions, 1));
        assert_eq!(view.read_with(cx, |view, cx| view.finished_tabs(cx)), [true, true]);
        assert_eq!(view.update(cx, |view, cx| view.tab_statuses(cx)), ["Finished", "Finished"]);
        click(cx, "debug-session-close-0");
        assert_eq!(*events.borrow(), [DebugWindowEvent::CloseTab(DebugSessionId(1))]);
        assert_eq!(close(&debugger, 1, cx), CloseTab::Closed);
        assert_eq!(titles(&view, cx), ["app"]);
        assert_eq!(active(&view, cx), Some(DebugSessionId(2)));
        assert!(debugger.read_with(cx, |debugger, _| debugger.state.session(DebugSessionId(1)).is_none()));
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › passes the selection of a
    // closed tab to the tab on its left
    #[gpui_kit::test]
    async fn passes_the_selection_of_a_closed_tab_to_the_tab_on_its_left(cx: &mut TestAppContext) {
        let sessions = [
            (4, "a", SessionStatus::Terminated),
            (5, "b", SessionStatus::Terminated),
            (6, "c", SessionStatus::Terminated),
        ];
        let (view, debugger, events, _, cx) = open(cx, debugger(&sessions, 5));
        click(cx, "debug-session-close-1");
        assert_eq!(*events.borrow(), [DebugWindowEvent::CloseTab(DebugSessionId(5))]);
        close(&debugger, 5, cx);
        assert_eq!(active(&view, cx), Some(DebugSessionId(4)));
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › keeps a live session when the
    // user doesn't terminate it
    #[gpui_kit::test]
    async fn keeps_a_live_session_when_the_user_doesnt_terminate_it(cx: &mut TestAppContext) {
        let (view, debugger, events, _, cx) = open(cx, debugger(&[(8, "server", SessionStatus::Paused)], 8));
        click(cx, "debug-session-close-0");
        assert_eq!(*events.borrow(), [DebugWindowEvent::CloseTab(DebugSessionId(8))]);
        assert_eq!(close(&debugger, 8, cx), CloseTab::Confirm { name: "server".into() });
        assert_eq!(titles(&view, cx), ["server"]);
        assert_eq!(active(&view, cx), Some(DebugSessionId(8)));
    }

    // src/features/debugger/__tests__/toolWindow.test.tsx › debug tool window › leaves nothing selected once
    // the last tab is closed
    #[gpui_kit::test]
    async fn leaves_nothing_selected_once_the_last_tab_is_closed(cx: &mut TestAppContext) {
        let (view, debugger, _, _, cx) = open(cx, debugger(&[(3, "main", SessionStatus::Terminated)], 3));
        click(cx, "debug-session-close-0");
        close(&debugger, 3, cx);
        assert_eq!(active(&view, cx), None);
        assert!(titles(&view, cx).is_empty());
        let empty = view.read_with(cx, |view, cx| view.empty_text(cx)).expect("the empty state");
        assert_eq!(empty[0], "Not debugging");
        assert!(cx.debug_bounds("debug-empty").is_some());
    }

    // Not from a spec: the toolbar enables what the suspended session allows and runs the button's
    // action; a tab selects its session; the frames pane reports its own events.
    #[gpui_kit::test]
    async fn the_toolbar_follows_the_session_and_runs_its_actions(cx: &mut TestAppContext) {
        let sessions = [(1, "main", SessionStatus::Paused), (2, "worker", SessionStatus::Running)];
        let (view, debugger, events, _, cx) = open(cx, debugger(&sessions, 1));
        let enabled = actions(&view, cx);
        for (action, on) in [
            ("debug.resume", true),
            ("debug.pause", false),
            ("debug.stepOver", true),
            ("debug.stop", true),
            ("debug.rerun", false),
            ("debug.muteBreakpoints", true),
        ] {
            assert!(enabled.contains(&(action, on)), "{action} {on}: {enabled:?}");
        }
        click(cx, "debug-tool-debug.stepOver");
        click(cx, "debug-tool-debug.pause");
        assert_eq!(*events.borrow(), [DebugWindowEvent::RunAction("debug.stepOver")]);
        click(cx, "debug-session-tab-1");
        assert_eq!(events.borrow().last(), Some(&DebugWindowEvent::SelectSession(DebugSessionId(2))));
        debugger.update(cx, |debugger, cx| {
            debugger.state.select_session(DebugSessionId(2));
            debugger.breakpoints.set_muted(true, 0.0);
            debugger.state.take_effects();
            cx.notify();
        });
        cx.run_until_parked();
        let toolbar = view.read_with(cx, |view, cx| view.toolbar(cx));
        let find = |action: &str| toolbar.iter().find(|button| button.action == action).copied();
        assert_eq!(find("debug.pause").map(|button| button.enabled), Some(true));
        assert_eq!(find("debug.muteBreakpoints").map(|button| button.pressed), Some(true));
        click(cx, "debug-tool-more");
        assert!(matches!(events.borrow().last(), Some(DebugWindowEvent::ShowMoreMenu(_))));
        assert_eq!(DebugWindowView::more_menu().iter().filter_map(MenuItem::action_id).count(), 8);
        let frames = view.read_with(cx, |view, _| view.frames().clone());
        let seen: Rc<RefCell<Vec<FramesViewEvent>>> = Rc::default();
        let log = Rc::clone(&seen);
        cx.update(|_, cx| {
            cx.subscribe(&frames, move |_, event: &FramesViewEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        frames.update(cx, |frames, cx| frames.jump_to_source(cx));
        assert!(seen.borrow().is_empty(), "the running session has no selected frame");
    }
}
