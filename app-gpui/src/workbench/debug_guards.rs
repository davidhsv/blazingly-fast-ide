//! Debug of a configuration that is already running or being debugged. As in IDEA, a configuration that
//! doesn't allow parallel runs asks before its running and debugged instances are stopped ("Process 'x'
//! Is Running", Stop and Rerun), waits until its Run processes are gone, and only then is debugged; a
//! Debug pressed again between the first press and the debugger's session (while the first one asks,
//! waits or prepares) does nothing. A configuration that allows parallel runs is debugged at once.

use super::Workbench;
use crate::debugger::state::{Ctx, SessionStatus};
use crate::platform::i18n::Localization;
use crate::run::driver::{rerun_question, run_settings};
use gpui_kit::{Context, Window};
use ide_core::DebugSessionId;
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How often a Debug that stopped Run processes looks whether they are gone.
const EXIT_POLL_MS: u64 = 50;
/// How long a Debug waits at most for a debug session of its configuration to end once it is stopping.
const STOPPING_WAIT_MS: u64 = 10_000;

/// The configurations whose Debug is between the press and the debugger's session, by id: `true` once
/// the launch is the debugger's, `false` while the window still asks or waits for stopped instances.
#[derive(Default)]
pub(super) struct DebugLaunches {
    launching: HashMap<String, bool>,
}

impl DebugLaunches {
    /// Whether a Debug of the configuration `id` is under way.
    pub(super) fn contains(&self, id: &str) -> bool {
        self.launching.contains_key(id)
    }
}

impl Workbench {
    /// Debug of `config` (its configurations are listed and its target can be debugged here): asks
    /// before stopping its running and debugged instances unless it allows parallel runs.
    pub(super) fn debug_guarded(
        &mut self,
        config: RunConfig,
        show: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if config.allow_parallel {
            return self.launch_debug(config, show, window, cx);
        }
        // Clicked again while it is still preparing: that launch goes on, no second one.
        if self.debug_launches.contains(&config.id) {
            return;
        }
        let runs = self.live_runs_of(&config.id, cx);
        let debugging = self.debug_roots_of(&config.id, false, cx);
        // A Rerun stops its session before it debugs the configuration again: that session is not asked
        // about, but the new one starts once it is over, so that it takes the finished session's tab.
        let stopping = self.debug_roots_of(&config.id, true, cx);
        if runs.is_empty() && debugging.is_empty() && stopping.is_empty() {
            self.debug_launches.launching.insert(config.id.clone(), true);
            return self.launch_debug(config, show, window, cx);
        }
        self.debug_launches.launching.insert(config.id.clone(), false);
        let settings = crate::platform::settings::effective(&self.label, cx);
        let ask = !(runs.is_empty() && debugging.is_empty()) && run_settings(&settings).confirm_rerun;
        let asked = if ask {
            let i18n = &mut cx.default_global::<Localization>().0;
            let (title, message, ok) = rerun_question(&config.name, runs.len() + debugging.len(), i18n);
            let cancel = i18n.tx("Cancel");
            let level = gpui_kit::PromptLevel::Warning;
            Some(window.prompt(level, &title, Some(message.as_str()), &[ok.as_str(), cancel.as_str()], cx))
        } else {
            None
        };
        cx.spawn_in(window, async move |workbench, cx| {
            let confirmed = match asked {
                Some(answer) => answer.await.ok() == Some(0),
                None => true,
            };
            if !confirmed {
                let _ = workbench.update(cx, |workbench, _| workbench.debug_launches.launching.remove(&config.id));
                return;
            }
            let stopped = workbench.update(cx, |workbench, cx| workbench.stop_instances(&runs, &debugging, cx));
            if stopped.is_err() {
                return;
            }
            // The new process starts once the old ones are gone (they may still hold a port or a lock).
            // There is no time limit for a Run process: a first Stop waits for the shutdown, a second one
            // kills it. A debug session stopped here, or already stopping, is waited for a while only: its
            // adapter may never say it ended.
            let mut waited = 0;
            loop {
                let live = workbench.update(cx, |workbench, cx| {
                    let runs_now = workbench.live_runs_of(&config.id, cx);
                    let roots_now = workbench.debug_roots_of(&config.id, true, cx);
                    let ending = stopping.iter().chain(&debugging).any(|id| roots_now.contains(id));
                    let debug_live = waited < STOPPING_WAIT_MS && ending;
                    runs.iter().any(|id| runs_now.contains(id)) || debug_live
                });
                match live {
                    Ok(true) => cx.background_executor().timer(Duration::from_millis(EXIT_POLL_MS)).await,
                    Ok(false) => break,
                    Err(_) => return,
                }
                waited += EXIT_POLL_MS;
            }
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                // The project closed while this Debug waited: there is nothing left to debug it in.
                let Some(launched) = workbench.debug_launches.launching.get_mut(&config.id) else { return };
                *launched = true;
                workbench.launch_debug(config, show, window, cx);
            });
        })
        .detach();
    }

    /// Hands the debug of `config` to the debugger (it becomes the chooser's most recent configuration).
    fn launch_debug(&mut self, config: RunConfig, show: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(run) = self.run_driver().cloned() {
            run.update(cx, |run, cx| run.mark_recent(&config.id, cx));
        }
        let mut start = json!({ "config": serde_json::to_value(&config).unwrap_or(Value::Null) });
        if let Some(show) = show {
            start["show"] = Value::Bool(show);
        }
        self.run_debug_action("debug.startConfig", Some(&start), window, cx);
        self.settle_debug_launches(cx);
    }

    /// A Debug handed to the debugger is over once the debugger has a live session of its configuration,
    /// or once nothing is starting any more (the start failed or was refused).
    pub(super) fn settle_debug_launches(&mut self, cx: &mut Context<Self>) {
        if !self.debug_launches.launching.values().any(|launched| *launched) {
            return;
        }
        let Some(driver) = self.debug_driver().cloned() else {
            self.debug_launches.launching.clear();
            return;
        };
        let debugger = driver.read(cx).debugger().read(cx);
        let busy = debugger.launching();
        let live: HashSet<&str> = debugger
            .state
            .published()
            .service_sessions
            .iter()
            .filter(|session| session.status != SessionStatus::Terminated)
            .map(|session| session.config.id.as_str())
            .collect();
        self.debug_launches.launching.retain(|id, launched| !*launched || (busy && !live.contains(id.as_str())));
    }

    /// The live Run sessions of the configuration `id`.
    fn live_runs_of(&self, id: &str, cx: &Context<Self>) -> Vec<String> {
        let Some(run) = self.run_driver() else { return Vec::new() };
        let store = run.read(cx).store().read(cx);
        let live = |session: &&crate::run::store::RunSession| session.config.id == id && session.is_live();
        store.sessions.iter().filter(live).map(|session| session.id.clone()).collect()
    }

    /// The debugger's root sessions of the configuration `id`: those stopping (`stopping`), or the other
    /// live ones.
    fn debug_roots_of(&self, id: &str, stopping: bool, cx: &Context<Self>) -> Vec<DebugSessionId> {
        let Some(driver) = self.debug_driver() else { return Vec::new() };
        let debugger = driver.read(cx).debugger().read(cx);
        let wanted = |status: SessionStatus| match status {
            SessionStatus::Stopping => stopping,
            SessionStatus::Terminated => false,
            _ => !stopping,
        };
        let roots = debugger.state.published().service_sessions.iter();
        roots.filter(|session| session.config.id == id && wanted(session.status)).map(|session| session.id).collect()
    }

    /// Stops the Run sessions `runs` and the debug sessions `debugging`, with the processes they attach to.
    fn stop_instances(&mut self, runs: &[String], debugging: &[DebugSessionId], cx: &mut Context<Self>) {
        if let Some(run) = self.run_driver().cloned() {
            run.update(cx, |run, cx| runs.iter().for_each(|id| run.stop(id, cx)));
        }
        if debugging.is_empty() {
            return;
        }
        let Some(driver) = self.debug_driver().cloned() else { return };
        driver.update(cx, |driver, cx| {
            driver.change(cx, |debugger, host| {
                for id in debugging {
                    let mut ctx = Ctx { breakpoints: &mut debugger.breakpoints, env: host.env, i18n: &mut *host.i18n };
                    debugger.state.stop_session(*id, true, &mut ctx);
                }
            });
        });
    }
}
