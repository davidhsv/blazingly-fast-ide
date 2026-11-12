//! HotSwap (IDEA: Run › Debugging Actions › Reload Changed Classes) through java-debug's hot code
//! replace: compile with jdtls, then `redefineClasses` in every Java debug session. When jdtls
//! finishes a build on its own (after a save) java-debug reports `BUILD_COMPLETE` and, like IDEA's
//! default "Reload classes after compilation: Ask", the reload is offered.
//!
//! [`HotSwap`] hands out each thing it waits for as a [`HotSwapStep`] and collects its
//! notifications for [`HotSwap::take_effects`].

use crate::debugger::effects::{BackendCall, Effect, is_cancelled, notification_action, path_text, run_notification};
use crate::debugger::state::{DebugSession, DebugState, SessionStatus};
use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationAction, NotificationLevel};
use ide_core::{DebugSessionId, IdeError, RequestId};
use ide_dap::HotSwapResult;
use ide_lsp::jdtls::JavaBuildStatus;
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Key of every hot swap notification: each replaces the one before.
const KEY: &str = "debug.hotswap";
/// Context key that is true while a hot swap compile runs.
pub const BUILDING_CONTEXT: &str = "debugHotSwapBuilding";
/// The action the editor menu offers on a Java file while a session can reload classes.
pub const RELOAD_ACTION: &str = "debug.reloadChangedClasses";
/// Command of the "Cancel Hot Swap Build" button: [`HotSwap::cancel_build`].
pub const CANCEL_BUILD: &str = "debug.cancelHotSwap";
/// Command of the "Reload" button of the "Classes changed" notification: reload without building.
pub const RELOAD_WITHOUT_BUILD: &str = "debug.hotswap.reload";
/// Command prefix of a "Reset Frame" button; the session id follows ([`reset_frame_session`]).
const RESET_FRAME: &str = "debug.hotswap.resetFrame:";

/// The session a "Reset Frame" button's command names: [`HotSwap::reset_frame`] acts on it.
pub fn reset_frame_session(command: &str) -> Option<DebugSessionId> {
    command.strip_prefix(RESET_FRAME)?.parse().ok().map(DebugSessionId)
}

/// The live sessions whose adapter can redefine classes.
pub fn hot_swap_sessions(state: &DebugState) -> Vec<&DebugSession> {
    state.sessions().values().filter(|s| s.is_live() && s.supports(|caps| caps.hot_code_replace)).collect()
}

/// Editor menu: the action to offer on the files `paths`, which is Reload Changed Classes on one
/// Java file while a debug session can reload classes.
pub fn hot_swap_menu_action(paths: &[&str], state: &DebugState) -> Option<&'static str> {
    let [path] = paths else { return None };
    let extension = path.len().checked_sub(5).and_then(|at| path.get(at..));
    let java = extension.is_some_and(|extension| extension.eq_ignore_ascii_case(".java"));
    (java && !hot_swap_sessions(state).is_empty()).then_some(RELOAD_ACTION)
}

// java-debug reports this after redefineClasses succeeds, with no changedClasses count.
fn obsolete_methods(message: &str) -> bool {
    message.trim() == "JVM contains obsolete methods"
}

/// What a reload came to, in IDEA's wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotSwapMessage {
    pub level: NotificationLevel,
    pub title: String,
    pub body: Option<String>,
}

/// Message for the reload result. `changed`: how many classes were reloaded; `bottom_frame`: a
/// session with obsolete methods is suspended in a frame that has no caller to return to.
pub fn hot_swap_message(changed: usize, errors: &[String], bottom_frame: bool, i18n: &mut I18n) -> HotSwapMessage {
    if errors.iter().any(|error| !obsolete_methods(error)) {
        let body = Some(errors.join("\n"));
        return HotSwapMessage { level: NotificationLevel::Error, title: "Hot Swap failed".to_owned(), body };
    }
    if !errors.is_empty() {
        let mut body =
            i18n.tx("Active frames keep running the old code until they return. New calls use the reloaded code.");
        if bottom_frame {
            body.push(' ');
            body.push_str(&i18n.tx(
                "The bottom frame cannot be reset. Re-enter the method or restart the debug session to run the new \
                 code.",
            ));
        }
        let title = "Classes reloaded".to_owned();
        return HotSwapMessage { level: NotificationLevel::Warning, title, body: Some(body) };
    }
    let title = if changed == 0 {
        "Loaded classes are up to date. Nothing to reload.".to_owned()
    } else {
        // "3 classes", translated per count; the sentence around it is translated on its own.
        let count = [Param::from(changed)];
        let classes = i18n.tx_built(&["", if changed == 1 { " class" } else { " classes" }], &count);
        i18n.tx_built(&["", " reloaded"], &[Param::from(classes)])
    };
    HotSwapMessage { level: NotificationLevel::Info, title, body: None }
}

/// What a reload waits for next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotSwapStep {
    /// Nothing: another reload is under way, there is nothing to reload into, or it is over.
    Done,
    /// Save the open documents (best effort), then [`HotSwap::saved`].
    SaveAll,
    /// Build the jdtls workspace serving `module` (a cancellable request), then [`HotSwap::built`].
    Build { module: String, request: RequestId },
    /// Redefine the changed classes in the session, then [`HotSwap::redefined`].
    Redefine(DebugSessionId),
}

/// A reload under way.
#[derive(Debug, Clone, Default)]
struct Reload {
    sessions: Vec<DebugSessionId>,
    /// Module directories still to compile.
    modules: VecDeque<String>,
    /// Sessions still to reload into; the first is the one being reloaded.
    remaining: VecDeque<DebugSessionId>,
    changed: BTreeSet<String>,
    errors: Vec<String>,
    /// Sessions whose JVM kept running old code in active frames.
    obsolete: Vec<DebugSessionId>,
}

#[derive(Debug, Clone, Default)]
pub struct HotSwap {
    /// `BUILD_COMPLETE` events of our own build are not worth a prompt: until then, none is shown.
    quiet_until: f64,
    /// The compile request a reload is waiting on, and whether it was cancelled.
    active_build: Option<(RequestId, bool)>,
    reload: Option<Reload>,
    /// The frame each "Reset Frame" button resets, with the stack it was offered for.
    reset_offers: HashMap<DebugSessionId, (i64, u64)>,
    effects: Vec<Effect>,
}

impl HotSwap {
    /// The notifications raised since the last call.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Whether a hot swap compile runs (the value of [`BUILDING_CONTEXT`]).
    pub fn building(&self) -> bool {
        self.active_build.is_some()
    }

    /// Cancels a running HotSwap compile; the reload is aborted rather than applied to stale classes.
    pub fn cancel_build(&mut self) -> Option<BackendCall> {
        let (request, cancelled) = self.active_build.as_mut().filter(|(_, cancelled)| !cancelled)?;
        *cancelled = true;
        Some(BackendCall::CancelRequest(*request))
    }

    fn notify(&mut self, level: NotificationLevel, title: &str, body: Option<&str>) {
        self.notify_with(HotSwapMessage { level, title: title.to_owned(), body: body.map(str::to_owned) }, None);
    }

    /// Raises `message`; `actions`: its buttons, which also make it stay until dismissed.
    fn notify_with(&mut self, message: HotSwapMessage, actions: Option<Vec<NotificationAction>>) {
        let mut request = run_notification(message.level, message.title);
        request.key = Some(KEY.to_owned());
        request.body = message.body;
        if let Some(actions) = actions {
            request.sticky = Some(true);
            request.actions = actions;
        }
        self.effects.push(Effect::Notify(Box::new(request)));
    }

    /// The "Reset Frame" buttons for sessions whose JVM has obsolete methods: one per session that
    /// is suspended in a frame java-debug can drop (not the bottom frame, no native caller).
    fn obsolete_frame_actions(&mut self, sessions: &[&DebugSession], i18n: &mut I18n) -> Vec<NotificationAction> {
        let mut actions = Vec::new();
        for s in sessions {
            let droppable = s.status == SessionStatus::Paused
                && s.supports(|caps| caps.restart_frame)
                && s.frames.len() >= 2
                && !s.frames[..2].iter().any(|frame| frame.name.contains("[native method]"));
            if !droppable {
                continue;
            }
            self.reset_offers.insert(s.id, (s.frames[0].id, s.frames_epoch));
            let label = if sessions.len() > 1 {
                i18n.tx_built(&["Reset Frame (", ")"], &[Param::from(s.name.as_str())])
            } else {
                "Reset Frame".to_owned()
            };
            actions.push(notification_action(&label, format!("{RESET_FRAME}{}", s.id.0)));
        }
        actions
    }

    /// A "Reset Frame" button was pressed: the session's top frame is selected and reset, unless
    /// the session has moved on since the button was offered.
    pub fn reset_frame(&mut self, session: DebugSessionId, state: &mut DebugState) -> Option<Effect> {
        let (frame, epoch) = self.reset_offers.get(&session).copied()?;
        let current = state.session(session)?;
        if current.status != SessionStatus::Paused || current.frames_epoch != epoch {
            return None;
        }
        state.select_session(session);
        state.select_frame(session, frame, false);
        Some(Effect::ExecuteAction { id: "debug.resetFrame".to_owned(), arg: None })
    }

    /// Compiles (unless `build` is false) and reloads changed classes into every Java debug
    /// session, or only into the session of the run configuration `config_id`. `desktop`: a
    /// backend is attached.
    pub fn reload_changed_classes(
        &mut self,
        build: bool,
        config_id: Option<&str>,
        state: &DebugState,
        desktop: bool,
        now_ms: f64,
    ) -> HotSwapStep {
        if self.reload.is_some() {
            return HotSwapStep::Done;
        }
        let of_config =
            |s: &&DebugSession| config_id.is_none_or(|id| s.run_config.as_ref().is_some_and(|config| config.id == id));
        let sessions: Vec<&DebugSession> = hot_swap_sessions(state).into_iter().filter(of_config).collect();
        if sessions.is_empty() || !desktop {
            self.notify(NotificationLevel::Info, "No debug session can reload classes", None);
            return HotSwapStep::Done;
        }
        let mut reload = Reload { sessions: sessions.iter().map(|s| s.id).collect(), ..Default::default() };
        reload.remaining = reload.sessions.iter().copied().collect();
        self.quiet_until = now_ms + 60_000.0;
        if !build {
            self.reload = Some(reload);
            return self.after_builds(state, now_ms);
        }
        for s in sessions {
            let module = s.launch.as_ref().and_then(|launch| launch.cwd.as_ref()).map(path_text).unwrap_or_default();
            if !reload.modules.contains(&module) {
                reload.modules.push_back(module);
            }
        }
        self.reload = Some(reload);
        HotSwapStep::SaveAll
    }

    /// The documents were saved (or could not be: the build goes on with what is on disk).
    pub fn saved(&mut self, state: &DebugState, now_ms: f64) -> HotSwapStep {
        self.next_build(state, now_ms)
    }

    fn next_build(&mut self, state: &DebugState, now_ms: f64) -> HotSwapStep {
        let Some(module) = self.reload.as_mut().and_then(|reload| reload.modules.pop_front()) else {
            return self.after_builds(state, now_ms);
        };
        let request = RequestId::next();
        self.active_build = Some((request, false));
        let compiling = HotSwapMessage {
            level: NotificationLevel::Info,
            title: "Compiling changed classes…".to_owned(),
            body: None,
        };
        self.notify_with(compiling, Some(vec![notification_action("Cancel Hot Swap Build", CANCEL_BUILD)]));
        HotSwapStep::Build { module, request }
    }

    /// The reload is over.
    fn finish(&mut self, now_ms: f64) -> HotSwapStep {
        self.reload = None;
        self.quiet_until = now_ms + 2_000.0;
        HotSwapStep::Done
    }

    /// A module's build ended.
    pub fn built(
        &mut self,
        result: Result<JavaBuildStatus, IdeError>,
        state: &mut DebugState,
        now_ms: f64,
    ) -> HotSwapStep {
        let status = match result {
            Ok(status) => Some(status),
            Err(error) => is_cancelled(&error).then_some(JavaBuildStatus::Cancelled),
        };
        let cancelled = self.active_build.take().is_some_and(|(_, cancelled)| cancelled);
        if cancelled || status == Some(JavaBuildStatus::Cancelled) {
            // JDT can write class files before cancellation without publishing their build
            // deltas. java-debug cannot recover those classes on the next build.
            let stale: Vec<DebugSessionId> = hot_swap_sessions(state).iter().map(|s| s.id).collect();
            for id in stale {
                if let Some(session) = state.session_mut(id) {
                    session.hot_swap_needs_restart = true;
                }
            }
            self.notify(NotificationLevel::Info, "Hot Swap cancelled", None);
            return self.finish(now_ms);
        }
        if status != Some(JavaBuildStatus::Succeeded) {
            let body = "Compilation failed: fix the errors and reload again.";
            self.notify(NotificationLevel::Error, "Hot Swap failed", Some(body));
            return self.finish(now_ms);
        }
        self.next_build(state, now_ms)
    }

    /// Every module is compiled: the classes are reloaded, session by session.
    fn after_builds(&mut self, state: &DebugState, now_ms: f64) -> HotSwapStep {
        let Some(reload) = &self.reload else { return HotSwapStep::Done };
        let needs_restart = |id: &DebugSessionId| state.session(*id).is_some_and(|s| s.hot_swap_needs_restart);
        if reload.sessions.iter().any(needs_restart) {
            let body = "A cancelled build may have left classes out of the reload. Restart the debug session to \
                        apply all changes.";
            self.notify(NotificationLevel::Error, "Hot Swap failed", Some(body));
            return self.finish(now_ms);
        }
        match reload.remaining.front() {
            Some(session) => HotSwapStep::Redefine(*session),
            None => self.finish(now_ms),
        }
    }

    /// The session being reloaded answered.
    pub fn redefined(
        &mut self,
        result: Result<HotSwapResult, IdeError>,
        state: &DebugState,
        now_ms: f64,
        i18n: &mut I18n,
    ) -> HotSwapStep {
        let Some(reload) = &mut self.reload else { return HotSwapStep::Done };
        let Some(session) = reload.remaining.pop_front() else { return HotSwapStep::Done };
        match result {
            Ok(reloaded) => {
                reload.changed.extend(reloaded.changed_classes);
                if let Some(error) = reloaded.error_message.filter(|error| !error.is_empty()) {
                    if obsolete_methods(&error) {
                        reload.obsolete.push(session);
                    }
                    reload.errors.push(error);
                }
            }
            Err(error) => reload.errors.push(error.message),
        }
        if let Some(next) = reload.remaining.front() {
            return HotSwapStep::Redefine(*next);
        }
        let reload = self.reload.take().unwrap_or_default();
        let affected: Vec<&DebugSession> = reload.obsolete.iter().filter_map(|id| state.session(*id)).collect();
        let bottom_frame = affected.iter().any(|s| s.status == SessionStatus::Paused && s.frames.len() == 1);
        let message = hot_swap_message(reload.changed.len(), &reload.errors, bottom_frame, i18n);
        let actions =
            (message.level == NotificationLevel::Warning).then(|| self.obsolete_frame_actions(&affected, i18n));
        self.notify_with(message, actions);
        self.finish(now_ms)
    }

    /// A `hotcodereplace` event of java-debug for `session`: `change_type` is `BUILD_COMPLETE`,
    /// `ERROR` or `WARNING` (others say nothing to the user). Events of sessions this window does
    /// not own are dropped.
    pub fn on_hot_code_replace(
        &mut self,
        session: DebugSessionId,
        change_type: &str,
        message: &str,
        state: &DebugState,
        now_ms: f64,
        i18n: &mut I18n,
    ) {
        let Some(s) = state.session(session) else { return };
        let reloading = self.reload.is_some();
        match change_type {
            "BUILD_COMPLETE" => {
                if reloading || now_ms < self.quiet_until {
                    return;
                }
                let changed = HotSwapMessage {
                    level: NotificationLevel::Info,
                    title: "Classes changed".to_owned(),
                    body: Some("Reload the changed classes into the running debug session?".to_owned()),
                };
                let mut request = run_notification(changed.level, changed.title);
                request.key = Some(KEY.to_owned());
                request.body = changed.body;
                request.actions = vec![notification_action("Reload", RELOAD_WITHOUT_BUILD)];
                self.effects.push(Effect::Notify(Box::new(request)));
            }
            "ERROR" | "WARNING" if obsolete_methods(message) => {
                if reloading {
                    return;
                }
                let bottom_frame = s.status == SessionStatus::Paused && s.frames.len() == 1;
                let warning = hot_swap_message(0, &[message.to_owned()], bottom_frame, i18n);
                let actions = self.obsolete_frame_actions(&[s], i18n);
                self.notify_with(warning, Some(actions));
            }
            "ERROR" => self.notify(NotificationLevel::Error, "Hot Swap", Some(message)),
            "WARNING" => self.notify(NotificationLevel::Warning, "Hot Swap", Some(message)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::state::testing::{frame, launch};
    use crate::run::configs::blank_config;
    use crate::workbench::notifications::NotificationRequest;
    use ide_dap::{DebugCapabilities, LaunchConfig};
    use ide_run::RunConfig;

    const NOW: f64 = 1_000_000.0;
    const OBSOLETE: &str = "JVM contains obsolete methods";

    fn id(id: u32) -> DebugSessionId {
        DebugSessionId(id)
    }

    fn app(cwd: &str) -> LaunchConfig {
        LaunchConfig { cwd: Some(crate::debugger::effects::ipc(cwd)), ..launch("App", "java-debug", "launch") }
    }

    /// Adds a running session "App" whose adapter can (`hot`) or cannot redefine classes.
    fn add_session(state: &mut DebugState, session: u32, hot: bool, init: impl FnOnce(&mut DebugSession)) {
        state.adopt_session(id(session), NOW, |s| {
            s.name = "App".to_owned();
            s.adapter = if hot { "java-debug" } else { "debugpy" }.to_owned();
            s.launch = Some(app("/p/app"));
            s.caps = Some(DebugCapabilities { hot_code_replace: hot, ..Default::default() });
            init(s);
        });
        state.take_effects();
    }

    /// Sessions 1 (Java) and 2 (Python), as most cases start.
    fn sessions() -> DebugState {
        let mut state = DebugState::default();
        add_session(&mut state, 1, true, |_| {});
        add_session(&mut state, 2, false, |_| {});
        state
    }

    fn of_config(config: &str) -> impl FnOnce(&mut DebugSession) {
        let config = RunConfig { id: config.to_owned(), ..blank_config("", "App", "application") };
        move |s| s.run_config = Some(config)
    }

    fn reloaded(classes: &[&str], error: Option<&str>) -> Result<HotSwapResult, IdeError> {
        Ok(HotSwapResult {
            changed_classes: classes.iter().map(|class| (*class).to_owned()).collect(),
            error_message: error.map(str::to_owned),
        })
    }

    /// What the backend did during a reload.
    #[derive(Default)]
    struct Backend {
        /// The build results still to hand out; a build beyond them succeeds.
        builds: VecDeque<Result<JavaBuildStatus, IdeError>>,
        /// The redefine results still to hand out; one beyond them reloads two classes.
        redefines: VecDeque<Result<HotSwapResult, IdeError>>,
        built: Vec<String>,
        redefined: Vec<DebugSessionId>,
        notes: Vec<NotificationRequest>,
    }

    impl Backend {
        fn collect(&mut self, hot: &mut HotSwap) {
            let effects = hot.take_effects();
            self.notes.extend(effects.iter().filter_map(Effect::notification).cloned());
        }

        /// Carries a reload from `step` to its end.
        fn run(&mut self, hot: &mut HotSwap, state: &mut DebugState, mut step: HotSwapStep) {
            let mut i18n = I18n::default();
            loop {
                self.collect(hot);
                step = match step {
                    HotSwapStep::Done => return,
                    HotSwapStep::SaveAll => hot.saved(state, NOW),
                    HotSwapStep::Build { module, .. } => {
                        self.built.push(module);
                        let result = self.builds.pop_front().unwrap_or(Ok(JavaBuildStatus::Succeeded));
                        hot.built(result, state, NOW)
                    }
                    HotSwapStep::Redefine(session) => {
                        self.redefined.push(session);
                        let both = reloaded(&["com.x.A", "com.x.B"], None);
                        hot.redefined(self.redefines.pop_front().unwrap_or(both), state, NOW, &mut i18n)
                    }
                };
            }
        }

        /// A reload from start to end; `build`: compile first.
        fn reload(&mut self, hot: &mut HotSwap, state: &mut DebugState, build: bool, config: Option<&str>) {
            let step = hot.reload_changed_classes(build, config, state, true, NOW);
            self.run(hot, state, step);
        }

        fn last(&self) -> &NotificationRequest {
            self.notes.last().expect("a notification")
        }

        /// The level and title of the last notification.
        fn said(&self) -> (NotificationLevel, &str) {
            (self.last().level, self.last().title.as_str())
        }
    }

    fn message(changed: usize, errors: &[&str], bottom_frame: bool) -> HotSwapMessage {
        let errors: Vec<String> = errors.iter().map(|error| (*error).to_owned()).collect();
        hot_swap_message(changed, &errors, bottom_frame, &mut I18n::default())
    }

    /// Session 1 suspended in the frames `names` (top first).
    fn paused_frames(names: &[&str], restart_frame: bool) -> DebugState {
        let mut state = DebugState::default();
        add_session(&mut state, 1, true, |s| {
            s.status = SessionStatus::Paused;
            s.caps = Some(DebugCapabilities { hot_code_replace: true, restart_frame, ..Default::default() });
            s.selected_thread = Some(1);
            s.selected_frame = Some(11);
            let frames = names.iter().enumerate().map(|(i, name)| frame(11 + i as i64, name, "/p/App.java", 11));
            s.set_frames(frames.collect());
        });
        state
    }

    #[test]
    fn is_enabled_only_with_a_session_whose_adapter_can_redefine_classes() {
        let mut state = sessions();
        state.publish_context();
        assert!(state.published().context.can_hot_swap);
        let mut state = DebugState::default();
        add_session(&mut state, 2, false, |_| {});
        assert!(!state.published().context.can_hot_swap);
    }

    #[test]
    fn offers_the_reload_in_the_editor_menu_of_a_java_file_while_a_session_can_reload_classes() {
        let state = sessions();
        assert_eq!(hot_swap_menu_action(&["/p/app/src/A.java"], &state), Some("debug.reloadChangedClasses"));
        assert_eq!(hot_swap_menu_action(&["/p/app/pom.xml"], &state), None);
        let mut state = DebugState::default();
        add_session(&mut state, 2, false, |_| {});
        assert_eq!(hot_swap_menu_action(&["/p/app/src/A.java"], &state), None);
    }

    #[test]
    fn builds_reloads_in_java_sessions_and_reports_the_count() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        backend.reload(&mut hot, &mut state, true, None);
        assert_eq!(backend.built, ["/p/app"]);
        assert_eq!(backend.redefined, [id(1)]);
        assert_eq!(backend.said(), (NotificationLevel::Info, "2 classes reloaded"));
    }

    #[test]
    fn limits_a_configuration_update_to_that_configurations_debug_session() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), DebugState::default(), Backend::default());
        add_session(&mut state, 1, true, of_config("tomcat:one"));
        add_session(&mut state, 2, true, of_config("tomcat:two"));
        backend.reload(&mut hot, &mut state, false, Some("tomcat:two"));
        assert!(backend.built.is_empty());
        assert_eq!(backend.redefined, [id(2)]);
    }

    #[test]
    fn does_not_reload_after_a_cancelled_build() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        backend.builds.push_back(Ok(JavaBuildStatus::Cancelled));
        backend.reload(&mut hot, &mut state, true, None);
        assert!(backend.redefined.is_empty());
        assert_eq!(backend.said(), (NotificationLevel::Info, "Hot Swap cancelled"));
    }

    #[test]
    fn treats_a_cancellation_error_as_cancellation_and_requires_a_restart_before_reloading() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        backend.builds.push_back(Err(IdeError::cancelled()));
        backend.reload(&mut hot, &mut state, true, None);
        assert!(backend.redefined.is_empty());
        assert_eq!(backend.last().title, "Hot Swap cancelled");
        assert!(!hot.building());
        assert!(hot.cancel_build().is_none());
        backend.reload(&mut hot, &mut state, true, None);
        assert!(backend.redefined.is_empty());
        assert_eq!(backend.said(), (NotificationLevel::Error, "Hot Swap failed"));
        assert!(backend.last().body.as_deref().is_some_and(|body| body.contains("Restart")));
        // A restarted session reloads again.
        let mut state = DebugState::default();
        add_session(&mut state, 1, true, |_| {});
        backend.reload(&mut hot, &mut state, true, None);
        assert_eq!(backend.redefined, [id(1)]);
    }

    #[test]
    fn keeps_the_first_request_when_reloads_overlap_and_stops_at_the_cancelled_module() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), DebugState::default(), Backend::default());
        add_session(&mut state, 1, true, |_| {});
        add_session(&mut state, 3, true, |s| s.launch = Some(app("/p/other")));
        assert_eq!(hot.reload_changed_classes(true, None, &state, true, NOW), HotSwapStep::SaveAll);
        let HotSwapStep::Build { module, request } = hot.saved(&state, NOW) else { panic!("a build") };
        assert_eq!(module, "/p/app");
        assert!(hot.building());
        backend.collect(&mut hot);
        assert_eq!(backend.last().title, "Compiling changed classes…");
        assert_eq!(backend.last().actions.len(), 1);
        // Reloads asked for meanwhile join the one under way.
        assert_eq!(hot.reload_changed_classes(true, None, &state, true, NOW), HotSwapStep::Done);
        assert_eq!(hot.reload_changed_classes(false, None, &state, true, NOW), HotSwapStep::Done);
        assert!(matches!(hot.cancel_build(), Some(BackendCall::CancelRequest(cancelled)) if cancelled == request));
        assert!(hot.cancel_build().is_none());
        // A success response racing the cancel must not reload partially compiled classes.
        let step = hot.built(Ok(JavaBuildStatus::Succeeded), &mut state, NOW);
        backend.run(&mut hot, &mut state, step);
        assert!(backend.redefined.is_empty());
        assert_eq!(backend.last().title, "Hot Swap cancelled");
        assert!(!hot.building());
        assert!(hot.cancel_build().is_none());
        backend.reload(&mut hot, &mut state, true, None);
        assert_eq!(backend.built, ["/p/app", "/p/other"]);
        assert!(backend.redefined.is_empty());
        assert!(state.session(id(3)).is_some_and(|s| s.hot_swap_needs_restart));
    }

    #[test]
    fn does_not_report_a_partial_reload_after_edit_cancel_edit_cancel_reload() {
        // The cancelled builds wrote OrderBook.class, but the next build only reports OrderBookTest.
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        backend.builds.extend([Ok(JavaBuildStatus::Cancelled), Ok(JavaBuildStatus::Cancelled)]);
        backend.reload(&mut hot, &mut state, true, None);
        backend.reload(&mut hot, &mut state, true, None);
        backend.redefines.push_back(reloaded(&["com.example.orders.OrderBookTest"], None));
        backend.reload(&mut hot, &mut state, true, None);
        backend.reload(&mut hot, &mut state, false, None);
        assert_eq!(backend.built.len(), 3);
        assert!(backend.redefined.is_empty());
        for note in &backend.notes[backend.notes.len() - 2..] {
            assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Error, "Hot Swap failed"));
            assert!(note.body.as_deref().is_some_and(|body| body.contains("Restart")));
        }
    }

    #[test]
    fn invalidates_all_live_java_sessions_when_a_configurations_workspace_build_is_cancelled() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), DebugState::default(), Backend::default());
        add_session(&mut state, 1, true, of_config("one"));
        add_session(&mut state, 2, false, |_| {});
        add_session(&mut state, 3, true, of_config("two"));
        backend.builds.push_back(Ok(JavaBuildStatus::Cancelled));
        backend.reload(&mut hot, &mut state, true, Some("one"));
        backend.reload(&mut hot, &mut state, false, Some("two"));
        assert!(backend.redefined.is_empty());
        assert!(state.session(id(2)).is_some_and(|s| !s.hot_swap_needs_restart));
        // A session started since still shares the workspace that lost its build deltas.
        add_session(&mut state, 4, true, |_| {});
        backend.reload(&mut hot, &mut state, false, None);
        assert!(backend.redefined.is_empty());
        let mut state = DebugState::default();
        add_session(&mut state, 4, true, |_| {});
        backend.reload(&mut hot, &mut state, false, None);
        assert_eq!(backend.redefined, [id(4)]);
    }

    #[test]
    fn does_not_reload_after_a_failed_build() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        backend.builds.push_back(Ok(JavaBuildStatus::Failed));
        backend.reload(&mut hot, &mut state, true, None);
        assert!(backend.redefined.is_empty());
        assert_eq!(backend.said(), (NotificationLevel::Error, "Hot Swap failed"));
    }

    #[test]
    fn words_results_like_idea() {
        assert_eq!(message(0, &[], false).title, "Loaded classes are up to date. Nothing to reload.");
        assert_eq!(message(1, &[], false).title, "1 class reloaded");
        assert_eq!(message(3, &[], false).title, "3 classes reloaded");
        let failed = message(3, &["boom"], false);
        assert_eq!((failed.level, failed.body.as_deref()), (NotificationLevel::Error, Some("boom")));
    }

    #[test]
    fn reports_obsolete_methods_as_a_reload_warning_whatever_the_count_of_returned_classes() {
        for changed in [0, 1, 3] {
            let warning = message(changed, &[OBSOLETE], false);
            assert_eq!((warning.level, warning.title.as_str()), (NotificationLevel::Warning, "Classes reloaded"));
            assert!(warning.body.is_some_and(|body| body.contains("Active frames keep running the old code")));
            let bottom = message(changed, &[OBSOLETE], true);
            assert!(bottom.body.is_some_and(|body| body.contains("bottom frame cannot be reset")));
        }
    }

    #[test]
    fn keeps_a_real_failure_even_alongside_obsolete_methods() {
        for failure in [
            "Failed to redefine classes: add method not implemented",
            "Failed to redefine classes: schema change not implemented",
            "Failed to redefine classes: JVM contains obsolete methods",
        ] {
            for errors in [vec![failure], vec![OBSOLETE, failure]] {
                let failed = message(1, &errors, false);
                assert_eq!((failed.level, failed.title.as_str()), (NotificationLevel::Error, "Hot Swap failed"));
            }
        }
    }

    #[test]
    fn offers_reset_frame_only_when_supported_and_droppable() {
        let cases: [(bool, &[&str], bool); 5] = [
            (true, &["App.main(String[])"], false),
            (false, &["OrderBook.place()", "App.main(String[])"], false),
            (true, &["OrderBook.place()", "Native.call()[native method]"], false),
            (true, &["Native.call()[native method]", "App.main(String[])"], false),
            (true, &["OrderBook.place()", "App.main(String[])"], true),
        ];
        for (supported, names, offered) in cases {
            let (mut hot, mut backend) = (HotSwap::default(), Backend::default());
            let mut state = paused_frames(names, supported);
            backend.redefines.push_back(reloaded(&[], Some(OBSOLETE)));
            backend.reload(&mut hot, &mut state, false, None);
            let note = backend.last();
            assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Warning, "Classes reloaded"));
            assert_eq!(note.actions.len(), usize::from(offered), "{names:?}");
            if offered {
                state.select_session(id(2));
                let session = reset_frame_session(&note.actions[0].command).expect("the session to reset");
                let reset = hot.reset_frame(session, &mut state);
                assert!(matches!(reset, Some(Effect::ExecuteAction { id, .. }) if id == "debug.resetFrame"));
                assert_eq!(state.active(), Some(id(1)));
                assert_eq!(state.session(id(1)).and_then(|s| s.selected_frame), Some(11));
            }
        }
    }

    #[test]
    fn does_not_reset_a_stale_frame_from_an_earlier_suspension() {
        let names = ["OrderBook.place()", "App.main(String[])"];
        let (mut hot, mut state, mut backend) = (HotSwap::default(), paused_frames(&names, true), Backend::default());
        backend.redefines.push_back(reloaded(&[], Some(OBSOLETE)));
        backend.reload(&mut hot, &mut state, false, None);
        // The session stopped again since: the same frames, of another suspension.
        let session = state.session_mut(id(1)).expect("the session");
        let frames = session.frames.clone();
        session.set_frames(frames);
        let offered = reset_frame_session(&backend.last().actions[0].command).expect("the session to reset");
        assert!(hot.reset_frame(offered, &mut state).is_none());
    }

    #[test]
    fn maps_java_debugs_obsolete_method_error_event_to_the_same_warning() {
        let (mut hot, mut backend) = (HotSwap::default(), Backend::default());
        let state = paused_frames(&["App.main(String[])"], true);
        hot.on_hot_code_replace(id(1), "ERROR", OBSOLETE, &state, NOW, &mut I18n::default());
        backend.collect(&mut hot);
        let note = backend.last();
        assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Warning, "Classes reloaded"));
        assert!(note.body.as_deref().is_some_and(|body| body.contains("bottom frame cannot be reset")));
        assert!(note.actions.is_empty());
    }

    #[test]
    fn does_not_let_the_obsolete_method_event_replace_the_responses_real_failure() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        let mut i18n = I18n::default();
        let step = hot.reload_changed_classes(false, None, &state, true, NOW);
        assert_eq!(step, HotSwapStep::Redefine(id(1)));
        // java-debug sends the event before it answers the request.
        hot.on_hot_code_replace(id(1), "ERROR", OBSOLETE, &state, NOW, &mut i18n);
        let failure = reloaded(&[], Some("Failed to redefine classes: add method not implemented"));
        let step = hot.redefined(failure, &state, NOW, &mut i18n);
        backend.run(&mut hot, &mut state, step);
        assert_eq!(backend.notes.len(), 1);
        assert_eq!(backend.said(), (NotificationLevel::Error, "Hot Swap failed"));
    }

    #[test]
    fn translates_the_reload_count_with_the_sentence() {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        assert_eq!(hot_swap_message(3, &[], false, &mut i18n).title, "已重新加载 3 个类");
    }

    #[test]
    fn offers_a_reload_when_java_debug_reports_a_finished_build() {
        let (mut hot, mut state, mut backend) = (HotSwap::default(), sessions(), Backend::default());
        let mut i18n = I18n::default();
        backend.reload(&mut hot, &mut state, false, None);
        let before = backend.notes.len();
        // Within the quiet period after a reload its own build raises no prompt.
        hot.on_hot_code_replace(id(1), "BUILD_COMPLETE", "", &state, NOW + 1_000.0, &mut i18n);
        backend.collect(&mut hot);
        assert_eq!(backend.notes.len(), before);
        let later = NOW + 120_000.0;
        hot.on_hot_code_replace(id(1), "BUILD_COMPLETE", "", &state, later, &mut i18n);
        backend.collect(&mut hot);
        assert_eq!(backend.last().title, "Classes changed");
        let commands: Vec<&str> = backend.last().actions.iter().map(|action| action.command.as_str()).collect();
        assert_eq!(commands, [RELOAD_WITHOUT_BUILD]);
        hot.on_hot_code_replace(id(1), "ERROR", "schema change", &state, later, &mut i18n);
        backend.collect(&mut hot);
        assert_eq!(backend.said(), (NotificationLevel::Error, "Hot Swap"));
        assert_eq!(backend.last().body.as_deref(), Some("schema change"));
        let before = backend.notes.len();
        hot.on_hot_code_replace(id(99), "ERROR", "not ours", &state, later, &mut i18n);
        backend.collect(&mut hot);
        assert_eq!(backend.notes.len(), before);
    }
}
