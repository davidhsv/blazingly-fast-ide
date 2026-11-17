# Run and Debug guards: integration notes

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Every anchor below was copied from `git show HEAD:<file>` (HEAD `e0854289f`). Written without a compiler: nothing here was built or run.

New files (untracked, nothing declares them yet):

- `app-gpui/src/run/proofs40_start_gate.rs` (test helper: a start held before the process host is asked, or with its answer; two unit tests of its own)
- `app-gpui/src/run/store/tests/proofs40_run_guards.rs` (model tests, parts 1 and 3)
- `app-gpui/src/run/store/tests/proofs40_run_tabs_pending.rs` (pinned-tab cases; needs `RunSession::pinned` and `RunSettings::pin_tabs`, which do not exist: do not declare it until they do)
- `app-gpui/src/workbench/tests/proofs40_run_start.rs` (end to end, part 1)
- `app-gpui/src/workbench/debug_guards.rs` (logic, part 2: `impl Workbench`)
- `app-gpui/src/workbench/debug/tests/proofs40_debug_guards.rs` (end to end, parts 2 and 3)

## (a) Hunks for existing files, in order

### Part 1: Stop while a run is starting (request id on the process start)

**1.1 `app-gpui/src/run/mod.rs`** — declare the gate (test builds only). Anchor:

```rust
pub mod navigate;
pub mod remote_debug;
```

becomes

```rust
pub mod navigate;
#[cfg(test)]
pub mod proofs40_start_gate;
pub mod remote_debug;
```

**1.2 `app-gpui/src/run/store.rs`** — a new effect. Anchor:

```rust
    /// The process exited before its id was known: finish the session as after
    /// [`RunStore::on_run_exited`].
    Exited {
        session_id: String,
        code: Option<i32>,
    },
}
```

becomes

```rust
    /// The process exited before its id was known: finish the session as after
    /// [`RunStore::on_run_exited`].
    Exited {
        session_id: String,
        code: Option<i32>,
    },
    /// Stop came while the process host was starting the process: the process that started anyway is
    /// stopped, and nothing is reported.
    Stop(RunId),
}
```

**1.3 `app-gpui/src/run/store.rs`** — the store remembers the starts Stop cancelled. Anchor:

```rust
    /// The temporary configuration "Modify Run Configuration…" opened the edit dialog on.
    modifying: Option<String>,
}
```

becomes

```rust
    /// The temporary configuration "Modify Run Configuration…" opened the edit dialog on.
    modifying: Option<String>,
    /// Requests of launches Stop cancelled: what a process start among them still answers is dropped,
    /// and a process it started anyway is stopped.
    cancelled_starts: HashSet<RequestId>,
}
```

**1.4 `app-gpui/src/run/store.rs`** (`reset_runs`) — a start cancelled before the project closed still has its late process stopped. Anchor:

```rust
            listed: std::mem::take(&mut self.listed),
            ..Self::default()
```

becomes

```rust
            listed: std::mem::take(&mut self.listed),
            cancelled_starts: std::mem::take(&mut self.cancelled_starts),
            ..Self::default()
```

**1.5 `app-gpui/src/run/store.rs`** (`stop_session`). Anchor:

```rust
            if let Some(runtime) = self.runtimes.get_mut(session_id) {
                runtime.resolve_aborted = true;
            }
```

becomes

```rust
            if let Some(runtime) = self.runtimes.get_mut(session_id) {
                runtime.resolve_aborted = true;
            }
            self.cancelled_starts.insert(request);
```

**1.6 `app-gpui/src/run/store.rs`** — accessor (the tests read it; it is what `runtime(id).resolveRequest` is in the specs). Anchor:

```rust
    fn resolve_aborted(&self, session_id: &str) -> bool {
```

becomes

```rust
    /// The cancellable request a session's launch waits on, while it waits.
    pub fn resolve_request(&self, session_id: &str) -> Option<RequestId> {
        self.runtimes.get(session_id).and_then(|runtime| runtime.resolve_request)
    }

    fn resolve_aborted(&self, session_id: &str) -> bool {
```

**1.7 `app-gpui/src/run/store.rs`** (`StartRun` fields). Anchor:

```rust
    session_id: Option<String>,
    generation: u64,
    launch: Option<RunConfig>,
    preparation: Option<JavaPreparation>,
    effects: Vec<StartEffect>,
}
```

becomes

```rust
    session_id: Option<String>,
    generation: u64,
    launch: Option<RunConfig>,
    preparation: Option<JavaPreparation>,
    /// The request the process start goes out with, until the process host answers.
    request: Option<RequestId>,
    effects: Vec<StartEffect>,
}
```

and in `StartRun::start`, anchor:

```rust
            preparation: None,
            effects: Vec::new(),
        };
        let step = start.begin(store, env);
```

becomes

```rust
            preparation: None,
            request: None,
            effects: Vec::new(),
        };
        let step = start.begin(store, env);
```

**1.8 `app-gpui/src/run/store.rs`** (`process_started`). Anchor (from the doc line to `let Some(id)`):

```rust
    /// The process host answered.
    pub fn process_started(
        &mut self,
        started: Result<RunInfo, IdeError>,
        store: &mut RunStore,
        now_ms: f64,
        i18n: &mut I18n,
    ) -> StartStep {
        if self.stale(store) {
            return self.done();
        }
        let info = match started {
            Ok(info) => info,
            Err(error) => return self.start_failed(&error.message, store, now_ms, i18n),
        };
        let Some(id) = self.session_id.clone() else { return self.done() };
```

becomes

```rust
    /// The process start goes out with `request`, which Stop cancels while the start waits (a run
    /// target's validation, a container's preparation); then [`Self::process_started`].
    pub fn process_requested(&mut self, request: RequestId, store: &mut RunStore) {
        self.request = Some(request);
        let stale = self.stale(store);
        if let Some(id) = self.session_id.as_deref().filter(|_| !stale) {
            store.set_resolve_request(id, Some(request));
        }
    }

    /// The process host answered. A start Stop cancelled meanwhile reports nothing: a process it
    /// started anyway is stopped, and its tab stays as Stop left it.
    pub fn process_started(
        &mut self,
        started: Result<RunInfo, IdeError>,
        store: &mut RunStore,
        now_ms: f64,
        i18n: &mut I18n,
    ) -> StartStep {
        let cancelled = self.request.take().is_some_and(|request| store.cancelled_starts.remove(&request));
        let stale = self.stale(store);
        if !stale && let Some(id) = &self.session_id {
            store.set_resolve_request(id, None);
        }
        let stopped = !stale
            && self.session_id.as_deref().and_then(|id| store.session(id)).is_some_and(|session| session.stopped);
        if cancelled || stopped {
            if let Ok(info) = &started {
                self.effects.push(StartEffect::Stop(info.id));
            }
            return self.done();
        }
        if stale {
            return self.done();
        }
        let info = match started {
            Ok(info) => info,
            // Cancelled with no Stop of this tab: the tab ends without an error.
            Err(error) if error.kind == ide_core::error::ErrorKind::Cancelled => {
                return self.launch_ended(store, now_ms);
            }
            Err(error) => return self.start_failed(&error.message, store, now_ms, i18n),
        };
        let Some(id) = self.session_id.clone() else { return self.done() };
```

Also adjust the doc of `StartStep::RunStart` (optional): "answer with [`StartRun::process_started`]" can read "give it its request with [`StartRun::process_requested`], answer with [`StartRun::process_started`]".

**1.9 `app-gpui/src/run/driver.rs`** (`drive`, the `RunStart` arm). Anchor:

```rust
                StartStep::RunStart(launch) => {
                    let session = start.session_id().map(str::to_owned);
                    let generation = start.generation();
                    let started = match session {
                        Some(session) => Self::run_process(&this, session, generation, *launch, cx).await,
                        None => Err(IdeError::internal("the run has no tab")),
                    };
```

becomes

```rust
                StartStep::RunStart(launch) => {
                    let session = start.session_id().map(str::to_owned);
                    let generation = start.generation();
                    // Stop cancels the start while it waits: a run target's validation, a container.
                    let request = Backend::next_request_id();
                    Self::advance(&this, cx, |store, _, _, _| start.process_requested(request, store))?;
                    let started = match session {
                        Some(session) => Self::run_process(&this, session, generation, *launch, request, cx).await,
                        None => Err(IdeError::internal("the run has no tab")),
                    };
```

**1.10 `app-gpui/src/run/driver.rs`** (`run_process`). Anchor:

```rust
    async fn run_process(
        this: &WeakEntity<Self>,
        session: String,
        generation: u64,
        launch: RunConfig,
        cx: &mut AsyncApp,
    ) -> Result<RunInfo, IdeError> {
```

becomes

```rust
    async fn run_process(
        this: &WeakEntity<Self>,
        session: String,
        generation: u64,
        launch: RunConfig,
        request: ide_core::RequestId,
        cx: &mut AsyncApp,
    ) -> Result<RunInfo, IdeError> {
```

and anchor:

```rust
        Self::on_host(cx, move |host| async move {
            #[cfg(test)]
            crate::build_tools::guard::check_run(&host, &label, &launch).await?;
            ide_host::ops::run::start(&host, &label, launch, on_output).await
        })
        .await
```

becomes

```rust
        Self::on_host(cx, move |host| async move {
            #[cfg(test)]
            crate::build_tools::guard::check_run(&host, &label, &launch).await?;
            #[cfg(test)]
            let held = launch.command.clone();
            let start = ide_host::ops::run::start_request(&host, &label, launch, on_output, Some(request));
            // A test may hold the start of its stand-in before the host is asked, or hold the answer.
            #[cfg(test)]
            let start = crate::run::proofs40_start_gate::gated(held, start);
            start.await
        })
        .await
```

**1.11 `app-gpui/src/run/driver.rs`** (`apply_effects`). Anchor:

```rust
                StartEffect::Exited { session_id, code } => self.finish_later(session_id, code, cx),
```

becomes

```rust
                StartEffect::Exited { session_id, code } => self.finish_later(session_id, code, cx),
                // Stop came while the process was starting: it ends now, quietly (its tab says stopped).
                StartEffect::Stop(run) => {
                    if let Some(backend) = cx.try_global::<Backend>() {
                        let label = self.label.clone();
                        let stopping =
                            backend.with_host(move |host| async move { ide_host::ops::run::stop(&host, &label, run) });
                        cx.background_executor()
                            .spawn(async move {
                                if let Err(error) = flatten(stopping.await) {
                                    tracing::debug!("cannot stop the process of a cancelled start: {}", error.message);
                                }
                            })
                            .detach();
                    }
                }
```

**1.12 test declarations.** `app-gpui/src/run/store.rs`, anchor `    mod proofs8_removal;` becomes `    mod proofs40_run_guards;` + `    mod proofs8_removal;` (the part-3 cases in that file need part 3's hunks: declare it after part 3 lands, or split the file's last three tests out). `app-gpui/src/workbench/mod.rs` (`mod tests`), anchor:

```rust
    mod proofs3_windows;
    mod proofs4_run;
```

becomes

```rust
    mod proofs3_windows;
    mod proofs40_run_start;
    mod proofs4_run;
```

### Part 2: Debug on a running configuration asks; a second Debug while the first one starts does nothing

**2.1 `app-gpui/src/run/driver.rs`** — the question's texts are shared. Anchor:

```rust
fn rerun_question(name: &str, count: usize, i18n: &mut I18n) -> (String, String, String) {
```

becomes

```rust
pub(crate) fn rerun_question(name: &str, count: usize, i18n: &mut I18n) -> (String, String, String) {
```

**2.2 `app-gpui/src/debugger/state.rs`** — accessor. Anchor:

```rust
    pub fn sessions_running(&self) -> bool {
        self.sessions.values().any(DebugSession::is_live)
    }
```

becomes

```rust
    pub fn sessions_running(&self) -> bool {
        self.sessions.values().any(DebugSession::is_live)
    }

    /// Whether a start is under way: a launch being prepared, waiting for the breakpoints, or for the
    /// backend's session.
    pub fn starting(&self) -> bool {
        self.pending_starts > 0 || !self.preparations.is_empty() || !self.waiting_for_breakpoints.is_empty()
    }
```

**2.3 `app-gpui/src/debugger/actions.rs`** — accessor. Anchor:

```rust
    /// The live root sessions Stop offers beside the run processes: (id, name).
```

becomes

```rust
    /// Whether a Debug is between its press and its session: a start flow waits for an answer (the
    /// configurations, a prepared or saved configuration, a Java launch), or the sessions' start is
    /// under way. The Xdebug probes only warn and do not count.
    pub fn launching(&self) -> bool {
        let start = |waiting: &Waiting| {
            matches!(
                waiting,
                Waiting::Start(..) | Waiting::Java(..) | Waiting::PluginPrepare { .. } | Waiting::PluginSave { .. }
            )
        };
        self.state.starting() || self.waiting.values().any(start)
    }

    /// The live root sessions Stop offers beside the run processes: (id, name).
```

**2.4 `app-gpui/src/workbench/mod.rs`** — module, field, initial value. Anchor `mod debug_extras;` + newline + `mod deployment;` becomes `mod debug_extras;` + `mod debug_guards;` + `mod deployment;`. Anchor:

```rust
    debug_extras: debug_extras::DebugExtrasSlot,
```

becomes

```rust
    debug_extras: debug_extras::DebugExtrasSlot,
    /// Debugs of configurations between the press and the debugger's session.
    debug_launches: debug_guards::DebugLaunches,
```

Anchor `            debug_extras: Default::default(),` becomes the same line plus `            debug_launches: Default::default(),`.

**2.5 `app-gpui/src/workbench/debug.rs`** — imports. Anchor `use serde_json::{Value, json};` becomes `use serde_json::Value;` (`json!` was only used in the lines 2.6 removes; Hypothesis, checked with grep at HEAD: the only other `json!` in the file is `serde_json::json!` in the tests).

**2.6 `app-gpui/src/workbench/debug.rs`** (`debug_config`). Anchor:

```rust
                run.update(cx, |run, cx| run.mark_recent(&config.id, cx));
                let mut start = json!({ "config": serde_json::to_value(&config).unwrap_or(Value::Null) });
                if let Some(show) = show {
                    start["show"] = Value::Bool(show);
                }
                workbench.run_debug_action("debug.startConfig", Some(&start), window, cx);
```

becomes

```rust
                workbench.debug_guarded(config, show, window, cx);
```

(`run` is still used two lines above by `config_from_arg`.)

**2.7 `app-gpui/src/workbench/debug.rs`** (`debugger_changed`). Anchor:

```rust
        self.sync_smart_step_chooser(window, cx);
        cx.notify();
    }
```

becomes

```rust
        self.sync_smart_step_chooser(window, cx);
        self.settle_debug_launches(cx);
        cx.notify();
    }
```

**2.8 `app-gpui/src/workbench/debug.rs`** (`reset_debugger`). Anchor:

```rust
        self.debug_extras = Default::default();
        if let Some(slot) = self.debug.take() {
```

becomes

```rust
        self.debug_extras = Default::default();
        self.debug_launches = Default::default();
        if let Some(slot) = self.debug.take() {
```

**2.9 test declaration**, `app-gpui/src/workbench/debug.rs` (`mod tests`). Anchor:

```rust
    mod proofs3_debug;
    mod proofs6_debug;
```

becomes

```rust
    mod proofs3_debug;
    mod proofs40_debug_guards;
    mod proofs6_debug;
```

### Part 3: which tab a start takes

**3.1 `app-gpui/src/run/store.rs`** — field. Anchor:

```rust
    /// Ids of every configuration listed in this window, so a run of one deleted since can be told apart.
    listed: HashSet<String>,
```

becomes

```rust
    /// Ids of every configuration listed in this window, so a run of one deleted since can be told apart.
    listed: HashSet<String>,
    /// Tabs a start is about to restart (a rerun waiting for its documents to be saved): no other start
    /// takes them meanwhile.
    claimed_tabs: HashSet<String>,
```

**3.2 `app-gpui/src/run/store.rs`** — the choice. Anchor:

```rust
    /// What rerunning a session launches: its configuration as saved now (edited since, as IDEA), or
```

becomes

```rust
    /// The finished tab a run of `config` takes the place of, as IDEA reuses run content: that of the
    /// same configuration first, then the selected one, then the first; never one a start is about to
    /// restart. An agent's commands keep to their own tabs.
    pub fn reusable_tab(&self, config: &RunConfig) -> Option<&str> {
        let agent = |config: &RunConfig| config.extras.params.contains_key(ide_host::mcp::AGENT_PARAM);
        let free: Vec<&RunSession> =
            self.sessions.iter().filter(|s| !s.is_live() && !self.claimed_tabs.contains(&s.id)).collect();
        if let Some(same) = free.iter().copied().find(|session| session.config.id == config.id) {
            return Some(same.id.as_str());
        }
        if agent(config) {
            return None;
        }
        let others: Vec<&RunSession> = free.into_iter().filter(|session| !agent(&session.config)).collect();
        let active = others.iter().copied().find(|s| self.active_session.as_deref() == Some(s.id.as_str()));
        active.or(others.first().copied()).map(|session| session.id.as_str())
    }

    /// What rerunning a session launches: its configuration as saved now (edited since, as IDEA), or
```

**3.3 `app-gpui/src/run/store.rs`** — `StartRun` loses `idle` (the tab is chosen when it opens). Anchor:

```rust
    /// The finished tab of the same configuration when the start began.
    idle: Option<String>,
```

is removed; anchor `            idle: None,` (in `StartRun::start`) is removed.

**3.4 `app-gpui/src/run/store.rs`** (`instances`). Anchor:

```rust
        let idle = store.sessions.iter().filter(same).find(|session| !session.is_live());
        self.idle = idle.map(|session| session.id.clone());
```

becomes

```rust
        // The tab this start restarts is not another start's to take while this one goes on.
        if let Some(session) = &self.session {
            store.claimed_tabs.insert(session.clone());
        }
```

**3.5 `app-gpui/src/run/store.rs`** (`rerun_confirmed`). Anchor:

```rust
    pub fn rerun_confirmed(&mut self, confirmed: bool, store: &mut RunStore) -> StartStep {
        if confirmed { self.stop_instances(store) } else { StartStep::Done(None) }
    }
```

becomes

```rust
    pub fn rerun_confirmed(&mut self, confirmed: bool, store: &mut RunStore) -> StartStep {
        if confirmed {
            return self.stop_instances(store);
        }
        if let Some(session) = &self.session {
            store.claimed_tabs.remove(session);
        }
        StartStep::Done(None)
    }
```

**3.6 `app-gpui/src/run/store.rs`** (`saved`). Anchor:

```rust
        let reuse = self.session.take().or_else(|| self.idle.take());
```

becomes

```rust
        if let Some(session) = &self.session {
            store.claimed_tabs.remove(session);
        }
        // The tab is chosen as it opens: runs started together each open their own.
        let reuse = self.session.take().or_else(|| store.reusable_tab(&self.config).map(str::to_owned));
```

**3.7 the existing test this changes**, `app-gpui/src/run/store.rs`, `a_failed_run_notifies_when_the_run_window_is_hidden` (see (e)). Anchor:

```rust
        let failed = store.finish_session(&again, Some(0), Some(1), 60.0, tests).notification.expect("a notification");
```

becomes

```rust
        let generation = store.generation(&again);
        let failed = store.finish_session(&again, generation, Some(1), 60.0, tests).notification.expect("a notification");
```

and anchor:

```rust
        assert_eq!(store.finish_session(&quiet, Some(0), Some(1), 70.0, visible).notification, None);
```

becomes

```rust
        let generation = store.generation(&quiet);
        assert_eq!(store.finish_session(&quiet, generation, Some(1), 70.0, visible).notification, None);
```

## (b) Parity rows (every key checked in `parity.json` at HEAD)

| Key                                                 | In parity.json                | Native tests (new)                                                                                                                                                                                        | Becomes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------------------------------------------- | ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:run_start`                                     | yes, partial                  | `workbench::tests::proofs40_run_start::cancels_startup_and_handles_a_late_cancelled_response`, `…::cancels_startup_and_handles_a_late_success_response`, `…::a_start_let_go_before_stop_runs_its_program` | done (the driver calls `ide_host::ops::run::start_request` with `Some(request)` as `src-tauri/src/commands/run.rs:110` does)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `action:run.debugConfig`                            | yes, partial                  | `workbench::debug::tests::proofs40_debug_guards::debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another`, `…::a_second_debug_while_the_first_one_prepares_does_nothing`             | done (its two missing clauses)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `action:run.runConfig`                              | yes, partial                  | none for its clauses                                                                                                                                                                                      | stays partial: `store.ts:865` (rerun asks to stop the configuration's live debug sessions) not built; `store.ts:881` (runs started together, own tabs) proven only at the model level by `run::store::tests::proofs40_run_guards::gives_runs_started_together_their_own_tabs`; `store.ts:929` (new tab pinned) waits for pinned tabs                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `action:debug.rerun`                                | yes, partial                  | `workbench::debug::tests::proofs40_debug_guards::rerun_stops_the_live_session_first_and_takes_its_own_tabs_place`                                                                                         | stays partial: "keeps its pin" waits for pinned debug tabs                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `unit:src/features/run/javaModule.test.ts`          | yes, partial                  | the request clause: `workbench::tests::proofs40_run_start::a_start_let_go_before_stop_runs_its_program`                                                                                                   | Hypothesis: done once the integrator accepts that the six named cases' existing model tests (`run::store::tests::runs_from_another_directory_using_the_stored_modules_classpath`, `…::finds_the_source_module_for_an_old_saved_configuration_without_a_module`, `…::colours_spring_boot_runs_without_classpath_resolution`, `…::preserves_an_explicit_spring_boot_console_setting`, `…::keeps_plain_java_runs_with_an_explicit_classpath_free_of_spring_boot_options`, `…::adds_the_spring_boot_console_option_once_when_resolving_the_module_classpath`) plus the driver's one request path cover the spec's `expect.any(Number)` third argument: the request is given to every `StartStep::RunStart` in `RunDriver::drive`, whatever the launch's kind |
| `unit:src/features/run/closeRunTab.test.ts`         | NO (only `INVENTORY.md:3598`) | none                                                                                                                                                                                                      | MISSING                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `unit:src/features/run/resolveCancellation.test.ts` | yes, partial                  | see below                                                                                                                                                                                                 | done                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `unit:src/features/run/runTabReuse.test.ts`         | NO (only `INVENTORY.md:3639`) | see below                                                                                                                                                                                                 | partial (2 of 4 whole, 1 in part, 1 pending)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

Per spec file, every case at HEAD:

`src/features/run/resolveCancellation.test.ts` (3 cases):

- Stop during Java launch resolution › cancels the request and ignores a late %s response → `run_producers::store_cases::cancels_the_request_and_ignores_a_late_cancelled_response`, `run_producers::store_cases::cancels_the_request_and_ignores_a_late_success_response` (existing)
- Java run host separator › uses $separator from the host on $client → `run_producers::store_cases::uses_the_separator_from_the_host` (existing)
- Stop during target validation › cancels startup and handles a late %s response → `run::store::tests::proofs40_run_guards::cancels_startup_and_handles_a_late_cancelled_response`, `run::store::tests::proofs40_run_guards::cancels_startup_and_handles_a_late_success_response`, `workbench::tests::proofs40_run_start::cancels_startup_and_handles_a_late_cancelled_response`, `workbench::tests::proofs40_run_start::cancels_startup_and_handles_a_late_success_response`

`src/features/run/closeRunTab.test.ts` (1 case):

- hides the Run tool window once its last tab is closed → MISSING (no `run.closeTab` action and no stack-trace tabs natively; the Run tool window is not hidden when its last tab closes; belongs with the Run tab actions of `F-RUN-TABS`)

`src/features/run/runTabReuse.test.ts` (4 cases):

- takes the place of the same configuration's finished tab, then the selected one, never a pinned one → `run::store::tests::proofs40_run_guards::takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one` (all but the pinned clause); the pinned clause: `run::store::tests::proofs40_run_tabs_pending::takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one_never_a_pinned_one` (pending)
- opens a new tab when every finished tab is pinned, pinned when the setting asks → `run::store::tests::proofs40_run_tabs_pending::opens_a_new_tab_when_every_finished_tab_is_pinned_pinned_when_the_setting_asks` (pending)
- keeps an agent's commands out of other tabs → `run::store::tests::proofs40_run_guards::keeps_an_agents_commands_out_of_other_tabs`
- gives runs started together their own tabs → `run::store::tests::proofs40_run_guards::gives_runs_started_together_their_own_tabs`

`src/features/run/javaModule.test.ts`: 19 cases at HEAD per the row; the 13 covered are unchanged; the 6 listed lack only the request clause (see the row above).

## (c) Behaviours, webview `file:line`, built or left out

Part 1:

- `src/features/run/store.ts:1019-1026`: a request id goes with `runStart` and is deleted when it answers. Built: 1.8 to 1.10 (`process_requested`, `run_process` with `start_request(…, Some(request))`, cleared in `process_started`).
- `store.ts:1336-1341`: Stop of a starting session with a request aborts, marks it stopped and finished, cancels the request. Was already native (`RunStore::stop_session` → `StopAction::CancelRequest`, `RunDriver::carry_out_stop` → `host.cancel_request`); now also reached by the process start, not only the Java resolution.
- `store.ts:1027-1030`: a start answered after Stop (or stopped) stops the process it started (`runStop(info.id)`, errors ignored). Built: 1.8 + `StartEffect::Stop` carried out in 1.11 (quiet on failure, as `.catch(() => undefined)`).
- `store.ts:1042-1043`: a cancelled or stopped start's error is not reported (no "Error running" line, no notification). Built: 1.8.
- `crates/ide-host/src/ops/run.rs:228-257`: `start_request` begins the request (a cancel before `begin` starts it out cancelled, `crates/ide-host/src/requests.rs:42-55`), checks it at the start of `start_inner` (`run.rs:267`) and after a plugin run target's preparation (`run.rs:326`); a cancelled start returns `IdeError::cancelled()`. Read, nothing changed.
- `src/features/run/resolveCancellation.test.ts` "late success": the native "late success" stand-in holds the answer after the process host started the process (the backend has no wait point for a local program); the stand-in's record shows the process got the stop.

Part 2:

- `src/features/run/index.ts:175`: a configuration that allows parallel runs is debugged at once. Built (`debug_guarded`).
- `index.ts:176-178`, `:198-199`: a Debug pressed again between the press and the debugger's session does nothing. Built (`DebugLaunches`, ended by `settle_debug_launches` when a live root session of the configuration is published or the debugger has no start under way, `index.ts:184-195`).
- `index.ts:182` → `store.ts:1391-1404` (`stopRunningInstances`): count the live Run sessions and live debug roots of the configuration; with `run.confirmRerun` ask `confirmRerun` (`src/features/run/dialogs.tsx:34-53`: title "Process '<name>' Is Running", message "'<name>' is not allowed to run in parallel.\nWould you like to stop the running one?" or "…stop <n> running instances?", button "Stop and Rerun"); stop them; wait for the Run processes only, no time limit. Built, with the driver's own question texts (`rerun_question`).
- `dialogs.tsx:41`: the question's "Do not ask again" checkbox. Left out: the native question is a GPUI prompt without a checkbox, as the Run driver's own rerun question already is.
- Deviation, built on purpose: a debug root already `Stopping` is not asked about; the Debug waits up to 10 s for it to end. Native Rerun (`DebugState::rerun_session`, `app-gpui/src/debugger/state.rs:1205-1222`) stops the session and runs `run.debugConfig` without waiting, where the webview awaits `stopSession` first (`src/features/debugger/state.ts:961-962`); without this, every native Rerun would ask.

Part 3:

- `src/features/debugger/state.ts:318-324`: a rerun's session takes its own finished tab's place and keeps its pin. Native: `DebugState::start_session` drops every finished session (`state.rs:918-920`, `drop_finished`), so the own tab goes (proven by the rerun test, which waits for the old session to end through 2.x's wait). The pin is left out (no pinned debug tabs). Mismatch left as is: the webview keeps the other finished tabs (`state.ts:325-331`, one replaced, preferring the same name, then the selected one); native drops all of them.
- `src/features/run/store.ts:776-787` (`reusableTab`): built (3.2) without the pinned filter.
- `store.ts:789-791`, `:820-827`, `:884-891` (`claimedTabs`): built as `claimed_tabs` for an explicit reuse (3.1, 3.4-3.6). The pick of a free tab needs no claim natively: it happens in `saved`, which opens the tab in the same call.
- `store.ts:929` (new tab pinned under `run.pinTabs`): left out (pending file).
- `store.ts:865-869` (a Run of a configuration also counts and stops its live debug sessions): left out. The Run driver does not see the debugger; it needs the window to hand it the live debug roots and a new step to stop them (an extra `StartStep` or a `RunDriverEvent`).
- `src/features/run/runTabs.ts:20-27` (`closeRunTab` hides the Run tool window with its last tab, stack traces included): left out (see `closeRunTab.test.ts`).
- `store.ts:1601-1609` (closing a live tab asks "Do you want to terminate the process '<name>'?", Terminate): already native (`RunDriver::close_tab`, tested by `run::driver::tests::asks_before_terminating_a_running_process_and_keeps_the_tab_when_cancelled`).
- Native claim timing: the explicit-reuse claim is taken in `instances` (after before launch tasks), the webview's in `startRun` before them (`store.ts:820-826`). Left as is.

## (d) Hypotheses, and how the integrator settles each

1. Hypothesis: `AsyncWindowContext` (the `cx` of `cx.spawn_in(window, …)` in `debug_guards.rs`) has `background_executor()`; existing workbench code calls it on `AsyncApp`. Settle: compile; else use `cx.update(|_, cx| cx.background_executor().clone())` first.
2. Hypothesis: `Window::prompt` takes `(&str, Option<&str>, &[&str], &mut App)` as `RunDriver::ask` calls it (`app-gpui/src/run/driver.rs:1368`). Settle: compile.
3. Hypothesis: Settings › Run "show on start" is on by default, so the Run tool window shows and `run-stop` is drawn in the part-1 tests (`a_second_stop_kills_a_process_that_ignores_the_interrupt` clicks it the same way). Settle: run the tests.
4. Hypothesis: for kind `program` the fake adapter's `program` is the configuration's `command` (an absolute path in the part-2 tests; the adapter's `os.path.join(cwd, program)` keeps it). Settle: run `debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another`.
5. Hypothesis: the debugger publishes a preparation session among `service_sessions`; for the fake adapter there is none (the start goes `launch` → `Waiting::Start(Saving)` → `start_session`, `app-gpui/src/debugger/actions.rs:910-925`), so a second Debug in the same turn meets the guard. Settle: `a_second_debug_while_the_first_one_prepares_does_nothing`.
6. Hypothesis: in the webview a preparation session counts as a live service session (so a Debug during a Java build asks rather than doing nothing). Settle: read `src/features/debugger/state.ts` `serviceSessions` derivation.
7. Hypothesis: the six javaModule cases may be ticked from their model tests plus the driver's single request path. Settle: the parity reviewer's call.
8. Hypothesis: other existing tests than `a_failed_run_notifies_when_the_run_window_is_hidden` assume a new tab for each configuration in one store (now a finished tab of another configuration is taken). Settle: run `cargo nextest run -p openidea_gpui run::` and `workbench::tests::proofs3_run` and fix any `finish_session(…, Some(0), …)` that meets a reused tab.
9. Hypothesis: `RequestId` is `Hash + Eq` (it keys `HashMap`s in `crates/ide-host/src/requests.rs:26`). Settle: compile.
10. Hypothesis: the debug start of the stopped `main` in part 2 happens after its Run process is gone because the guard polls the run store; the poll uses the executor's timer, which the tests' `long` advances. Settle: run the test.

## (e) Existing tests whose expectations change

- `run::store::tests::a_failed_run_notifies_when_the_run_window_is_hidden`: `npm:test` now takes `npm:build`'s finished tab (`src/features/run/store.ts:781-787`, "then the selected one"), so its generation is 1, not 0, and `finish_session(…, Some(0), …)` is a stale exit that notifies nothing. Hunk 3.7 passes the tab's generation.
- Hypothesis: `workbench::tests::proofs3_run::run_configuration_runs_the_named_or_given_configuration_and_stop_process_ends_the_process` keeps passing: `greet` takes `hello`'s tab and `nap` takes `greet`'s, but each is asserted before the next starts.

## (f) Tests most likely to fail first, and why

1. `workbench::debug::tests::proofs40_debug_guards::rerun_stops_the_live_session_first_and_takes_its_own_tabs_place`: depends on the old session ending before the new one starts (else `drop_finished` keeps it and the tabs read `["main", "main"]`); the guard waits up to 10 s for it, but the fake adapter's end is real time.
2. `workbench::debug::tests::proofs40_debug_guards::a_second_debug_while_the_first_one_prepares_does_nothing`: if `launching()` misses a waiting variant the guard ends at once and the second press starts a second session.
3. `workbench::tests::proofs40_run_start::cancels_startup_and_handles_a_late_success_response`: the stand-in's `stopped` record needs `ops::run::stop` to reach the shell's trap; a stop that kills at once (no interrupt first) leaves no record. Then assert on the session instead.
4. `run::store::tests::proofs40_run_guards::gives_runs_started_together_their_own_tabs`: a long line may need rustfmt; the `[String]` vs `[&str; 2]` comparison relies on `PartialEq<[B; N]> for [A]`.
5. Compile: `debug_guards.rs` (hypotheses 1, 2) and the `if !stale && let Some(id) = &self.session_id` let chain in 1.8.

What each test starts: `proofs40_run_start` starts `/bin/sh` stand-in scripts written in its temporary project (and their `sleep 0.1`); `proofs40_debug_guards` starts the same kind of stand-ins and `python3` running the fake adapter script in its temporary home; `proofs40_run_guards`, `proofs40_run_tabs_pending` and the gate's unit tests start nothing.
