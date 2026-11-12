//! The run widget of the title bar: the selected configuration's icon and name (a click opens the
//! configuration chooser), Run and Debug while it is idle, Rerun while it runs, Stop while anything
//! runs, and More Actions.
//!
//! A click on Run or Debug shows "Starting…" at once, before the session exists, until the session
//! starts or the launch ends without one ([`RunWidget::launch_ended`]). Between Stop and the exit,
//! Stop turns into Kill process.
//!
//! [`RunWidget`] owns no process: it reads the [`RunStore`] and reports clicks as [`RunWidgetEvent`]s.

use crate::platform::i18n::Localization;
use crate::run::config_types::RunConfigTypes;
use crate::run::configs::config_icon;
use crate::run::sessions::selected_target;
use crate::run::store::{RunSession, RunStore, SessionStatus};
use crate::run::types::is_debug_only;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, Context, Div, Entity, EventEmitter, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use std::convert::Infallible;
use std::rc::Rc;

/// Side of a square widget button.
const BUTTON: f32 = 30.0;
/// The widest the configuration name gets before it is cut.
const CHOOSER_MAX_WIDTH: f32 = 260.0;

/// What the widget asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum RunWidgetEvent {
    /// The configuration name was clicked: `run.chooseConfig` with `"select"` (which closes the
    /// chooser when it is open).
    OpenChooser,
    /// Run (or Rerun) the selection: `run.run`; with `debug`, `run.debug`. Answer with
    /// [`RunWidget::launch_ended`] once the action returns.
    Launch { debug: bool },
    /// Stop: `run.stop` with `"widget"` (the selected configuration's processes, else the only one,
    /// else a choice of them).
    Stop,
    /// Show [`RunWidget::more_menu`] here.
    ShowMore(Point<Pixels>),
}

/// A button of the widget as it is drawn now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetButton {
    /// Its debug selector: `run-widget-run`, `-debug`, `-rerun`, `-stop`, `-kill` or `-more`.
    pub id: &'static str,
    pub title: String,
    pub enabled: bool,
}

/// What the widget shows for the store and the launches in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetState {
    /// The selected configuration's name and kind icon.
    pub selected: Option<(String, String)>,
    pub current_file: bool,
    /// Only Debug applies to the selection (Remote JVM Debug and the like).
    pub debug_only: bool,
    /// The selection (or a compound's configuration) has a live session.
    pub active: bool,
    /// "Starting…" replaces Run / Rerun (or Debug, for a debug launch).
    pub loading: bool,
    pub debugging: bool,
    /// The selection's live session is shutting down: Rerun waits for it.
    pub target_stopping: bool,
    /// How many processes are live.
    pub running: usize,
    /// One of them is shutting down: Stop kills it.
    pub stopping: bool,
    /// The name Stop shows when it stops one configuration.
    pub stop_name: Option<String>,
}

/// The widget's state: `pending` are the launches clicked and not ended yet, as (configuration id,
/// debug). The selection's session decides, never an unrelated process.
pub fn widget_state(store: &RunStore, types: &RunConfigTypes, pending: &[(Option<String>, bool)]) -> WidgetState {
    let selected_id = store.selected_id.as_deref();
    let selected = store.selected_config();
    let running: Vec<&RunSession> = store.running_sessions();
    let compound: Vec<&RunSession> = selected
        .filter(|config| config.kind == "compound")
        .map(|config| store.sessions_of(config).into_iter().filter(|session| session.is_live()).collect())
        .unwrap_or_default();
    let target = selected_target(store, selected_id);
    let pending = pending.iter().find(|(id, _)| id.as_deref() == selected_id);
    let debugging = pending.is_some_and(|(_, debug)| *debug);
    let status_is = |status: SessionStatus| {
        let own = target.is_some_and(|session| session.status == status);
        own || compound.iter().any(|session| session.status == status)
    };
    let starting = pending.is_some() || status_is(SessionStatus::Starting);
    let stop_name = if compound.is_empty() {
        target.or(running.first().copied()).map(|session| session.config.name.clone())
    } else {
        selected.map(|config| config.name.clone())
    };
    WidgetState {
        selected: selected.map(|config| (config.name.clone(), config_icon(&config.kind, config.is_test, types))),
        current_file: store.current_file_selected(),
        debug_only: selected.is_some_and(|config| is_debug_only(&config.kind, types)),
        active: target.is_some() || !compound.is_empty(),
        loading: starting && !status_is(SessionStatus::Stopping),
        debugging,
        target_stopping: target.is_some_and(|session| session.status == SessionStatus::Stopping),
        running: running.len(),
        stopping: running.iter().any(|session| session.status == SessionStatus::Stopping),
        stop_name,
    }
}

fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// Text built at run time, translated through the catalog's templates (`Run '{name}'`).
fn tx_template(cx: &App, text: String) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.clone(), |localization| localization.0.tx_template(&text))
}

pub struct RunWidget {
    store: Entity<RunStore>,
    types: Rc<RunConfigTypes>,
    /// Launches clicked here whose action has not returned: (configuration id, debug).
    pending: Vec<(Option<String>, bool)>,
    debug_available: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<RunWidgetEvent> for RunWidget {}

impl RunWidget {
    pub fn new(store: Entity<RunStore>, types: Rc<RunConfigTypes>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self { store, types, pending: Vec::new(), debug_available: true, _subscriptions: subscriptions }
    }

    /// Whether Debug can be offered (a debugger is available in this window).
    pub fn set_debug_available(&mut self, available: bool, cx: &mut Context<Self>) {
        self.debug_available = available;
        cx.notify();
    }

    pub fn state(&self, cx: &App) -> WidgetState {
        widget_state(self.store.read(cx), &self.types, &self.pending)
    }

    /// The name shown: the selected configuration's, "Current File", or "Select Configuration".
    pub fn label(&self, cx: &App) -> String {
        let state = self.state(cx);
        match state.selected {
            Some((name, _)) => name,
            None => tx(cx, if state.current_file { "Current File" } else { "Select Configuration" }),
        }
    }

    /// The selected configuration's icon.
    pub fn icon(&self, cx: &App) -> Option<String> {
        self.state(cx).selected.map(|(_, icon)| icon)
    }

    /// Whether the chooser the widget opens is open (its button shows pressed).
    pub fn chooser_open(&self, cx: &App) -> bool {
        self.store.read(cx).chooser_open
    }

    /// Whether "Starting…" shows.
    pub fn loading(&self, cx: &App) -> bool {
        self.state(cx).loading
    }

    /// The buttons right of the name, in order.
    pub fn buttons(&self, cx: &App) -> Vec<WidgetButton> {
        let state = self.state(cx);
        let name = state.selected.as_ref().map(|(name, _)| name.clone());
        let has_target = state.selected.is_some() || state.current_file;
        let button = |id, title: String, enabled| WidgetButton { id, title, enabled };
        let mut buttons = Vec::new();
        if state.loading {
            // The launch that is starting shows "Starting…" in its own place; the other is disabled.
            if state.debugging {
                buttons.push(button("run-widget-run", tx(cx, "Run"), false));
            } else if self.debug_available {
                buttons.push(button("run-widget-debug", tx(cx, "Debug"), false));
            }
        } else if state.active {
            let title = tx_template(cx, format!("Rerun '{}'", name.clone().unwrap_or_default()));
            buttons.push(button("run-widget-rerun", title, !state.target_stopping));
        } else {
            let run = match (&name, state.current_file) {
                (Some(name), _) if state.debug_only => tx_template(cx, format!("'{name}' can only be debugged")),
                (Some(name), _) => tx_template(cx, format!("Run '{name}'")),
                (None, true) => tx(cx, "Run Current File"),
                (None, false) => tx(cx, "Run"),
            };
            buttons.push(button("run-widget-run", run, has_target && !state.debug_only));
            if self.debug_available {
                let debug = match (&name, state.current_file) {
                    (Some(name), _) => tx_template(cx, format!("Debug '{name}'")),
                    (None, true) => tx(cx, "Debug Current File"),
                    (None, false) => tx(cx, "Debug"),
                };
                buttons.push(button("run-widget-debug", debug, has_target));
            }
        }
        if state.running > 0 {
            if state.stopping {
                buttons.push(button("run-widget-kill", tx(cx, "Kill process"), true));
            } else {
                let title = if state.active || state.running == 1 {
                    tx_template(cx, format!("Stop '{}'", state.stop_name.unwrap_or_default()))
                } else {
                    tx_template(cx, format!("Stop ({} running)", state.running))
                };
                buttons.push(button("run-widget-stop", title, true));
            }
        }
        buttons.push(button("run-widget-more", tx(cx, "More Actions"), true));
        buttons
    }

    /// More Actions: run or debug the active file's context, Edit Configurations and reloading them.
    pub fn more_menu(&self) -> Vec<MenuItem<Infallible>> {
        vec![
            MenuItem::action("run.runContext"),
            MenuItem::action("run.debugContext"),
            MenuItem::Separator,
            MenuItem::action("run.editConfigs"),
            MenuItem::action("run.reloadConfigs"),
        ]
    }

    /// Run or Debug was clicked: "Starting…" shows until the session starts or the launch ends.
    /// A second click while the selection's launch is pending does nothing.
    pub fn launch(&mut self, debug: bool, cx: &mut Context<Self>) {
        let selected = self.store.read(cx).selected_id.clone();
        if self.pending.iter().any(|(id, _)| *id == selected) {
            return;
        }
        self.pending.push((selected, debug));
        cx.emit(RunWidgetEvent::Launch { debug });
        cx.notify();
    }

    /// The launch of the configuration `id` (the selection when it was clicked) returned.
    pub fn launch_ended(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        self.pending.retain(|(pending, _)| pending.as_deref() != id);
        cx.notify();
    }

    /// Carries out a click on the button `id`.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        let state = self.state(cx);
        match id {
            "run-widget-run" => self.launch(false, cx),
            "run-widget-debug" => self.launch(true, cx),
            // Rerun goes through Run (`run.run` restarts the live session), under the debugger
            // when the session debugs.
            "run-widget-rerun" => self.launch(state.debugging, cx),
            "run-widget-stop" | "run-widget-kill" => cx.emit(RunWidgetEvent::Stop),
            _ => {}
        }
    }

    fn render_button(&self, button: &WidgetButton, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = button.id;
        let (icon, size, color) = match id {
            "run-widget-run" => ("run", 20.0, if button.enabled { "success" } else { "fg-faint" }),
            "run-widget-debug" => ("debug", 20.0, if button.enabled { "fg-icon" } else { "fg-faint" }),
            "run-widget-rerun" => ("rerun-filled", 16.0, if button.enabled { "fg-on-accent" } else { "fg-faint" }),
            "run-widget-stop" => ("stop-filled", 16.0, "fg-on-accent"),
            "run-widget-kill" => ("stop-filled", 14.0, "fg-on-accent"),
            _ => ("more", 20.0, "fg-icon"),
        };
        let filled = match id {
            "run-widget-rerun" if button.enabled => Some("success"),
            "run-widget-stop" => Some("danger"),
            "run-widget-kill" => Some("warning"),
            _ => None,
        };
        let hover = theme::color(cx, "bg-toolbar-hover");
        let el = div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(BUTTON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(if filled.is_some() { 22.0 } else { BUTTON }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-toolbar-button"))
                    .when_some(filled, |el, fill| el.bg(theme::color(cx, fill)))
                    .child(theme::icon(cx, icon, size, theme::color(cx, color))),
            );
        if !button.enabled {
            return el;
        }
        let el = el.rounded(theme::length(cx, "radius-toolbar-button")).hover(move |el| el.bg(hover));
        if id == "run-widget-more" {
            return el.on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    cx.emit(RunWidgetEvent::ShowMore(event.position));
                }),
            );
        }
        el.on_click(cx.listener(move |widget, _, _, cx| widget.press(id, cx)))
    }
}

impl Render for RunWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let label = self.label(cx);
        let buttons = self.buttons(cx);
        let pressed = self.chooser_open(cx);
        let subtle = theme::color(cx, "fg-subtle");
        let icon = state.selected.as_ref().map(|(_, icon)| {
            let dot = (state.active && !state.loading).then(|| {
                div()
                    .absolute()
                    .top(px(-2.0))
                    .right(px(-2.0))
                    .size(px(7.0))
                    .rounded(px(4.0))
                    .bg(theme::color(cx, "success"))
            });
            div().relative().flex_none().child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-icon"))).children(dot)
        });
        let chooser = div()
            .id("run-widget-chooser")
            .debug_selector(|| "run-widget-chooser".to_owned())
            .h(px(BUTTON))
            .max_w(px(CHOOSER_MAX_WIDTH))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-toolbar-button"))
            .text_color(theme::color(cx, "fg-strong"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
            .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
            .children(icon)
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().child(SharedString::from(label)))
            .child(theme::icon(cx, "chevron-down", 14.0, subtle))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(RunWidgetEvent::OpenChooser)));
        let mut row = div().flex().items_center().gap(px(2.0)).text_size(theme::length(cx, "fs-ui")).child(chooser);
        let mut loading = state.loading.then(|| {
            div()
                .id("run-widget-loading")
                .debug_selector(|| "run-widget-loading".to_owned())
                .size(px(BUTTON))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(14.0)).rounded(px(7.0)).border_1().border_color(theme::color(cx, "fg-subtle")))
        });
        // "Starting…" stands where the launch that starts would be: first for a run, after the
        // disabled Run for a debug launch.
        if !state.debugging {
            row = row.children(loading.take());
        }
        for button in &buttons {
            row = row.child(self.render_button(button, cx));
            if button.id == "run-widget-run" {
                row = row.children(loading.take());
            }
        }
        row.children(loading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::run::application::{JvmContext, application_config};
    use crate::run::config_types::RunConfigType;
    use crate::run::configs::blank_config;
    use crate::run::sessions::{StopTarget, stop_target};
    use crate::run::types::register_builtin_types;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_run::RunConfig;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<RunWidgetEvent>>>;

    fn types() -> Rc<RunConfigTypes> {
        let mut types = RunConfigTypes::new();
        register_builtin_types(&mut types);
        types.register(RunConfigType::new("test-remote", "Remote", "debug", |_, _| None).debug_only());
        Rc::new(types)
    }

    fn config(id: &str) -> RunConfig {
        RunConfig { command: "sleep".into(), args: vec!["60".into()], ..blank_config(id, id, "shell") }
    }

    fn app() -> RunConfig {
        config("SdCloudOaApplication")
    }

    fn npm() -> RunConfig {
        config("sdcloud-oa-vue3: npm run dev")
    }

    fn run(config: &RunConfig, started_at: f64) -> RunSession {
        RunSession {
            id: config.id.clone(),
            config: config.clone(),
            run_id: Some(ide_core::RunId(1)),
            status: SessionStatus::Running,
            exit_code: None,
            started_at,
            ended_at: None,
            stdin_closed: false,
            stopped: false,
            base_config: None,
            terminal: false,
            http_port: None,
        }
    }

    fn with_status(mut session: RunSession, status: SessionStatus) -> RunSession {
        session.status = status;
        session
    }

    fn store_with(sessions: Vec<RunSession>) -> RunStore {
        let mut store = RunStore::new();
        store.set_configs(vec![app(), npm()]);
        store.loaded = true;
        store.selected_id = Some(app().id);
        store.sessions = sessions;
        store
    }

    fn open(
        cx: &mut TestAppContext,
        store: RunStore,
    ) -> (Entity<RunWidget>, Entity<RunStore>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let store = cx.update(|cx| cx.new(|_| store));
        let shared = store.clone();
        let (widget, cx) = cx.add_window_view(|_, cx| RunWidget::new(shared, types(), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&widget, move |_, event: &RunWidgetEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (widget, store, events, cx)
    }

    /// The titles of the enabled and the disabled buttons, More Actions left out.
    fn titles(widget: &Entity<RunWidget>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        let buttons = widget.read_with(cx, |widget, cx| widget.buttons(cx));
        buttons.into_iter().filter(|button| button.id != "run-widget-more").map(|b| (b.title, b.enabled)).collect()
    }

    fn has(widget: &Entity<RunWidget>, cx: &mut VisualTestContext, title: &str) -> Option<bool> {
        titles(widget, cx).into_iter().find(|(own, _)| own == title).map(|(_, enabled)| enabled)
    }

    fn loading(widget: &Entity<RunWidget>, cx: &mut VisualTestContext) -> bool {
        widget.read_with(cx, |widget, cx| widget.loading(cx))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn set(store: &Entity<RunStore>, cx: &mut VisualTestContext, change: impl FnOnce(&mut RunStore)) {
        store.update(cx, |store, cx| {
            change(store);
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn widget_stops(store: &Entity<RunStore>, cx: &mut VisualTestContext) -> StopTarget {
        store.read_with(cx, |store, _| stop_target(store, false, true))
    }

    // src/features/run/RunWidget.test.tsx › reruns and stops the selected configuration among several running
    // sessions
    #[gpui_kit::test]
    async fn reruns_and_stops_the_selected_configuration_among_several_running_sessions(cx: &mut TestAppContext) {
        let (widget, store, events, cx) = open(cx, store_with(vec![run(&app(), 1.0), run(&npm(), 3.0)]));
        assert_eq!(has(&widget, cx, "Rerun 'SdCloudOaApplication'"), Some(true));
        assert_eq!(has(&widget, cx, "Stop 'SdCloudOaApplication'"), Some(true));
        click(cx, "run-widget-stop");
        assert_eq!(*events.borrow(), [RunWidgetEvent::Stop]);
        assert_eq!(widget_stops(&store, cx), StopTarget::Sessions(vec![app().id]));
    }

    // src/features/run/RunWidget.test.tsx › offers Run/Debug for an idle selection and Stop for the other
    // process
    #[gpui_kit::test]
    async fn offers_run_debug_for_an_idle_selection_and_stop_for_the_other_process(cx: &mut TestAppContext) {
        let (widget, store, events, cx) = open(cx, store_with(vec![run(&npm(), 1.0)]));
        let npm_name = npm().name;
        assert_eq!(
            titles(&widget, cx),
            [
                ("Run 'SdCloudOaApplication'".to_owned(), true),
                ("Debug 'SdCloudOaApplication'".to_owned(), true),
                (format!("Stop '{npm_name}'"), true),
            ]
        );
        click(cx, "run-widget-stop");
        assert_eq!(*events.borrow(), [RunWidgetEvent::Stop]);
        assert_eq!(widget_stops(&store, cx), StopTarget::Sessions(vec![npm().id]));
    }

    // src/features/run/RunWidget.test.tsx › offers a stop list when the selection is idle and several other
    // sessions run
    #[gpui_kit::test]
    async fn offers_a_stop_list_when_the_selection_is_idle_and_several_other_sessions_run(cx: &mut TestAppContext) {
        let (widget, store, _, cx) = open(cx, store_with(vec![run(&npm(), 1.0), run(&config("other"), 2.0)]));
        assert_eq!(has(&widget, cx, "Stop (2 running)"), Some(true));
        assert!(titles(&widget, cx).iter().all(|(title, _)| !title.starts_with("Rerun")));
        assert!(matches!(widget_stops(&store, cx), StopTarget::Choose(ids) if ids.len() == 2));
    }

    // src/features/run/RunWidget.test.tsx › turns Stop into Kill process while the process is shutting down
    #[gpui_kit::test]
    async fn turns_stop_into_kill_process_while_the_process_is_shutting_down(cx: &mut TestAppContext) {
        let stopping = with_status(run(&app(), 1.0), SessionStatus::Stopping);
        let (widget, _, events, cx) = open(cx, store_with(vec![stopping]));
        assert_eq!(has(&widget, cx, "Kill process"), Some(true));
        // Rerun waits for the exit.
        assert_eq!(has(&widget, cx, "Rerun 'SdCloudOaApplication'"), Some(false));
        click(cx, "run-widget-kill");
        assert_eq!(*events.borrow(), [RunWidgetEvent::Stop]);
    }

    // src/features/run/RunWidget.test.tsx › matches a detected Spring configuration after its object and id are
    // regenerated
    #[gpui_kit::test]
    async fn matches_a_detected_spring_configuration_after_its_object_and_id_are_regenerated(cx: &mut TestAppContext) {
        let detect = || {
            let path = "src/main/java/com/example/SdCloudOaApplication.java";
            application_config("com.example.SdCloudOaApplication", true, path, JvmContext::default())
        };
        let (original, regenerated) = (detect(), detect());
        assert_eq!(regenerated.id, original.id);
        let mut store = store_with(vec![run(&original, 1.0), run(&npm(), 2.0)]);
        store.set_configs(vec![regenerated.clone(), npm()]);
        store.selected_id = Some(regenerated.id.clone());
        let (widget, _, _, cx) = open(cx, store);
        assert_eq!(has(&widget, cx, &format!("Rerun '{}'", regenerated.name)), Some(true));
        assert_eq!(has(&widget, cx, &format!("Stop '{}'", original.name)), Some(true));
    }

    // src/features/run/RunWidget.test.tsx › keeps Stop available during startup and restores Rerun immediately
    // when running
    #[gpui_kit::test]
    async fn keeps_stop_available_during_startup_and_restores_rerun_immediately_when_running(cx: &mut TestAppContext) {
        let starting = with_status(run(&app(), 1.0), SessionStatus::Starting);
        let (widget, store, _, cx) = open(cx, store_with(vec![starting]));
        assert!(loading(&widget, cx));
        assert!(cx.debug_bounds("run-widget-loading").is_some());
        assert_eq!(has(&widget, cx, "Rerun 'SdCloudOaApplication'"), None);
        assert_eq!(has(&widget, cx, "Stop 'SdCloudOaApplication'"), Some(true));
        set(&store, cx, |store| store.sessions[0].status = SessionStatus::Running);
        assert!(!loading(&widget, cx));
        assert!(cx.debug_bounds("run-widget-loading").is_none());
        assert_eq!(has(&widget, cx, "Stop 'SdCloudOaApplication'"), Some(true));
        assert_eq!(has(&widget, cx, "Rerun 'SdCloudOaApplication'"), Some(true));
    }

    // src/features/run/RunWidget.test.tsx › does not replay startup feedback when selecting an already running
    // configuration
    #[gpui_kit::test]
    async fn does_not_replay_startup_feedback_when_selecting_an_already_running_configuration(cx: &mut TestAppContext) {
        let (widget, store, _, cx) = open(cx, store_with(vec![run(&app(), 1.0), run(&npm(), 2.0)]));
        set(&store, cx, |store| store.selected_id = Some(npm().id));
        assert!(!loading(&widget, cx));
        assert_eq!(has(&widget, cx, &format!("Rerun '{}'", npm().name)), Some(true));
        set(&store, cx, |store| store.selected_id = Some(app().id));
        assert!(!loading(&widget, cx));
    }

    // src/features/run/RunWidget.test.tsx › stops the selected starting process while other Run and Debug
    // sessions remain live
    #[gpui_kit::test]
    async fn stops_the_selected_starting_process_while_other_run_and_debug_sessions_remain_live(
        cx: &mut TestAppContext,
    ) {
        let starting = with_status(run(&app(), 1.0), SessionStatus::Starting);
        let (_, store, events, cx) = open(cx, store_with(vec![starting, run(&npm(), 2.0)]));
        click(cx, "run-widget-stop");
        assert_eq!(*events.borrow(), [RunWidgetEvent::Stop]);
        assert_eq!(widget_stops(&store, cx), StopTarget::Sessions(vec![app().id]));
    }

    // src/features/run/RunWidget.test.tsx › stops every child of a starting compound, including its starting
    // debugger
    #[gpui_kit::test]
    async fn stops_every_child_of_a_starting_compound_including_its_starting_debugger(cx: &mut TestAppContext) {
        let mut compound = RunConfig { command: String::new(), ..blank_config("compound", "compound", "compound") };
        compound.extras.compound = vec![app().id, npm().id];
        let mut store = store_with(vec![with_status(run(&app(), 1.0), SessionStatus::Starting)]);
        store.set_configs(vec![app(), npm(), compound.clone()]);
        store.selected_id = Some(compound.id.clone());
        let (widget, store, _, cx) = open(cx, store);
        assert!(loading(&widget, cx));
        assert_eq!(has(&widget, cx, "Stop 'compound'"), Some(true));
        click(cx, "run-widget-stop");
        assert_eq!(widget_stops(&store, cx), StopTarget::Sessions(vec![app().id]));
    }

    // src/features/run/RunWidget.test.tsx › keeps the selected starting session stoppable after switching
    // configurations
    #[gpui_kit::test]
    async fn keeps_the_selected_starting_session_stoppable_after_switching_configurations(cx: &mut TestAppContext) {
        let starting = with_status(run(&app(), 1.0), SessionStatus::Starting);
        let (widget, store, _, cx) = open(cx, store_with(vec![starting, run(&npm(), 2.0)]));
        set(&store, cx, |store| store.selected_id = Some(npm().id));
        assert!(!loading(&widget, cx));
        assert_eq!(has(&widget, cx, &format!("Rerun '{}'", npm().name)), Some(true));
        set(&store, cx, |store| store.selected_id = Some(app().id));
        assert!(loading(&widget, cx));
        click(cx, "run-widget-stop");
        assert_eq!(widget_stops(&store, cx), StopTarget::Sessions(vec![app().id]));
    }

    // src/features/run/RunWidget.test.tsx › clears startup feedback when the launch is %s
    #[gpui_kit::test]
    async fn clears_startup_feedback_when_the_launch_is_failed_or_finished(cx: &mut TestAppContext) {
        let starting = with_status(run(&app(), 1.0), SessionStatus::Starting);
        let (widget, store, _, cx) = open(cx, store_with(vec![starting]));
        for status in [SessionStatus::Failed, SessionStatus::Finished] {
            set(&store, cx, |store| {
                store.sessions[0].status = status;
                store.sessions[0].ended_at = Some(2.0);
            });
            assert!(!loading(&widget, cx));
            assert_eq!(has(&widget, cx, "Run 'SdCloudOaApplication'"), Some(true));
            set(&store, cx, |store| store.sessions[0].status = SessionStatus::Starting);
            assert!(loading(&widget, cx));
        }
    }

    // src/features/run/RunWidget.test.tsx › shows feedback for a %s launch and clears it on failure
    #[gpui_kit::test]
    async fn shows_feedback_for_a_keyboard_or_menu_launch_and_clears_it_on_failure(cx: &mut TestAppContext) {
        let (widget, store, _, cx) = open(cx, store_with(Vec::new()));
        assert!(!loading(&widget, cx));
        // A launch from elsewhere shows through its session.
        set(&store, cx, |store| store.sessions = vec![with_status(run(&app(), 1.0), SessionStatus::Starting)]);
        assert!(loading(&widget, cx));
        assert_eq!(has(&widget, cx, "Stop 'SdCloudOaApplication'"), Some(true));
        set(&store, cx, |store| store.sessions.clear());
        assert!(!loading(&widget, cx));
    }

    // src/features/run/RunWidget.test.tsx › clears feedback when %s returns without a session
    #[gpui_kit::test]
    async fn clears_feedback_when_run_or_debug_returns_without_a_session(cx: &mut TestAppContext) {
        let (widget, _, events, cx) = open(cx, store_with(Vec::new()));
        for (selector, debug) in [("run-widget-run", false), ("run-widget-debug", true)] {
            click(cx, selector);
            assert!(loading(&widget, cx));
            widget.update(cx, |widget, cx| widget.launch_ended(Some("SdCloudOaApplication"), cx));
            cx.run_until_parked();
            assert!(!loading(&widget, cx));
            assert_eq!(has(&widget, cx, "Run 'SdCloudOaApplication'"), Some(true));
            assert_eq!(events.borrow().last(), Some(&RunWidgetEvent::Launch { debug }));
        }
    }

    // src/features/run/RunWidget.test.tsx › keeps a debug-only configuration stoppable during attachment
    #[gpui_kit::test]
    async fn keeps_a_debug_only_configuration_stoppable_during_attachment(cx: &mut TestAppContext) {
        let remote = RunConfig { kind: "test-remote".into(), ..app() };
        let mut store = store_with(Vec::new());
        store.set_configs(vec![remote.clone()]);
        let (widget, store, _, cx) = open(cx, store);
        assert_eq!(has(&widget, cx, "'SdCloudOaApplication' can only be debugged"), Some(false));
        assert_eq!(has(&widget, cx, "Debug 'SdCloudOaApplication'"), Some(true));
        set(&store, cx, |store| store.sessions = vec![with_status(run(&remote, 1.0), SessionStatus::Starting)]);
        assert!(loading(&widget, cx));
        assert_eq!(has(&widget, cx, "Stop 'SdCloudOaApplication'"), Some(true));
    }

    // src/features/run/RunWidget.test.tsx › allows another configuration to launch while the first launch is
    // pending
    #[gpui_kit::test]
    async fn allows_another_configuration_to_launch_while_the_first_launch_is_pending(cx: &mut TestAppContext) {
        let (widget, store, events, cx) = open(cx, store_with(Vec::new()));
        click(cx, "run-widget-run");
        set(&store, cx, |store| store.selected_id = Some(npm().id));
        click(cx, "run-widget-run");
        assert_eq!(events.borrow().len(), 2);
        assert!(loading(&widget, cx));
        widget.update(cx, |widget, cx| widget.launch_ended(Some(&app().id), cx));
        assert!(loading(&widget, cx));
        widget.update(cx, |widget, cx| widget.launch_ended(Some(&npm().id), cx));
        assert!(!loading(&widget, cx));
    }

    // Not from a spec: the name opens the chooser, shows pressed while it is open, and falls back to Current
    // File or Select Configuration; More Actions asks for its menu.
    #[gpui_kit::test]
    async fn the_name_opens_the_chooser_and_more_actions_its_menu(cx: &mut TestAppContext) {
        let (widget, store, events, cx) = open(cx, store_with(Vec::new()));
        let shown = widget.read_with(cx, |widget, cx| (widget.label(cx), widget.icon(cx)));
        assert_eq!(shown, ("SdCloudOaApplication".to_owned(), Some("file-shell".to_owned())));
        click(cx, "run-widget-chooser");
        assert_eq!(*events.borrow(), [RunWidgetEvent::OpenChooser]);
        set(&store, cx, |store| store.chooser_open = true);
        assert!(widget.read_with(cx, |widget, cx| widget.chooser_open(cx)));
        set(&store, cx, |store| store.selected_id = Some(crate::run::store::CURRENT_FILE.to_owned()));
        assert_eq!(widget.read_with(cx, |widget, cx| widget.label(cx)), "Current File");
        assert_eq!(has(&widget, cx, "Run Current File"), Some(true));
        set(&store, cx, |store| store.selected_id = None);
        assert_eq!(widget.read_with(cx, |widget, cx| widget.label(cx)), "Select Configuration");
        assert_eq!(has(&widget, cx, "Run"), Some(false));
        click(cx, "run-widget-more");
        assert!(matches!(events.borrow().last(), Some(RunWidgetEvent::ShowMore(_))));
        assert_eq!(widget.read_with(cx, |widget, _| widget.more_menu().len()), 5);
    }
}
