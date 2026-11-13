# Integrating a plugin's Java debug request (the JDWP branch)

New file, not compiled yet: `app-gpui/src/workbench/plugin_java_debug.rs` (588 lines). It needs the `mod` line and the hunks below. Every anchor is quoted from the file as it is now and occurs once; line numbers are left out because the files are moving.

## 0. Read first: two divergences from the assignment

The assignment asked for (a) "a process that exits before announcing a port reports the webview's message" and (b) "an attach that the adapter refuses reports its message and stops the process". The webview does neither, so the draft follows the webview and the tests are written to it. Overrule here if wanted.

- **No port timeout, no message.** `debugWithAgent` (`src/features/run/javaDebug.ts`, the function at the end of the file) starts the run with an `onLine` listener and attaches on every `jdwpListeningPort(line)`. It has no timer. A process that exits without announcing a port is only a finished run: the Run tab shows its exit, and the plugin hears the code through `onExit`. The only timeout on this path is java-debug's own `timeout: 30000` in the attach request (`src/features/debugger/java.ts`; natively `JavaFlow::request`, `extra.insert("timeout", json!(30_000))`). It is already sent. Test `a_process_that_exits_before_announcing_a_port_leaves_no_debug_session` asserts that no session exists and that no "Cannot start debugger" notice appears.
- **A refused Java attach does not stop the process.** `debug.attach` (`src/features/debugger/actions.ts`, around line 451) calls `stopAssociatedProcess` only when `isPythonAttachTarget(arg)`. For a JVM, `startSession`'s failure path (`src/features/debugger/state.ts` around line 655) notifies "Cannot start debugger for {name}" with `startFailureMessage(e)`. The JVM keeps waiting in its Run tab. The native model does the same (`Debugger::attach` keeps `AfterStart::StopProcessOnFailure` for Python only; `DebugState::start_failed`). Test `an_attach_the_adapter_refuses_is_reported_with_its_message` asserts that the process is still live and that the plugin hears exit 3 once the test lets it end. To stop it, change `Debugger::attach` in `debugger/actions.rs` so it registers `AfterStart::StopProcessOnFailure(process)` for the Java branch too. That would diverge from the webview.
- **Deliberate deviation, start failure.** When the run does not start (no session, or a `Failed` one), the webview's `debugWithAgent` returns `true` and the plugin's request is answered as a success, but the plugin never hears an end. The draft does what the native run path does (`refuse_plugin_run(.., RUN_NOT_STARTED)`): the plugin hears an end without a status, and a waiting request gets "Run did not start".

## 1. `app-gpui/src/workbench/mod.rs`: the module

After `pub mod plugin_contributions;` add:

```rust
mod plugin_java_debug;
```

No field, no action registration, no render change.

## 2. `app-gpui/src/workbench/plugin_contributions.rs`

### 2a. The Java branch of a debug request

This is the webview's `run.pluginStart` with `a.debug` (`src/features/run/index.ts`): `jdwpLaunch(a.config) && getAction("debug.attach")` goes to `debugWithAgent`, everything else to `debug.pluginStart`. In `pub(super) fn plugin_run_request(`, replace

```rust
        if debug {
            return self.plugin_debug_request(request_id, operation, config, window, cx);
        }
```

with

```rust
        if debug {
            // A detector that starts its JVM with a waiting JDWP agent runs in Run; the Java
            // debugger attaches to the port the JVM announces.
            if self.plugin_java_debug_request(request_id, operation, config, window, cx) {
                return;
            }
            return self.plugin_debug_request(request_id, operation, config, window, cx);
        }
```

The Java path for plugin targets is not skipped in `debugger/actions.rs`. `start_with_config`'s `!plugin_target && is_java_debuggable(&config)` concerns `plugin-target:` run targets and matches `startConfig` in `src/features/debugger/actions.ts` (around line 320) exactly. It needs no change. What was missing is the branch above: a plugin's debug request always went to `debug.pluginStart`, and `launch_config_for` has no launch for a `plugin` configuration with `java-debug`.

### 2b. Visibility for the sibling module

- `const RUN_NOT_STARTED: &str = "Run did not start";` becomes `pub(super) const RUN_NOT_STARTED: &str = "Run did not start";`.
- In `impl Workbench`, `fn refuse_plugin_run(` becomes `pub(super) fn refuse_plugin_run(`.

### 2c. Where the followed runs live

In `pub(super) struct PluginContributions {`, after

```rust
    /// The request ids of the plugin debug runs under way that a plugin waits on, by operation.
    debug_requests: HashMap<String, String>,
```

add:

```rust
    /// The plugin debug runs whose JVM the Java debugger attaches to once it announces its port.
    pub(super) java_debug: super::plugin_java_debug::JavaDebugRuns,
```

`PluginContributions` is `#[derive(Default)]`, and `JavaDebugRuns` is too, so `Workbench::new` needs no change.

## 3. `app-gpui/src/debugger/driver.rs`: the test seam for jdtls' debug server

The Java adapter is hard-wired to jdtls:

- `JavaFlow::attach` always goes `ReadDir` (the Kotlin scan) → `JavaStep::DebugServer` → `BackendCall::JavaDebugServer` → `ops::java::java_debug_server` → `jdtls_command("vscode.java.startDebugSession")`.
- The answer becomes `debugServer`, which makes `ide_dap::Debugger::start_prepared` connect to `127.0.0.1:<port>` (`ProcessLauncher::launch` with `reuse_port`) instead of starting the `[debugger]` command.
- A user `languages/java.toml` with a fake `jdtls` does not work. `ide_sdk::java` sets `launch_error` unless a JDK ≥ 21 is found for a server named `jdtls`, and `ide-lsp` special-cases that name in several places.
- A `[debugger]` in a user language file cannot help either: the port comes from jdtls, never from the language's command.
- A `#[cfg(test)]` hook in `ide-host` would not compile into app-gpui's tests (another crate), so the seam is in app-gpui.

### 3a. The global

After the `pub enum DebugDriverEvent { … }` block, directly before

```rust
/// The transitions' surroundings, from the fields of a [`Debugger`] and a [`Host`].
```

add:

```rust
/// The port of the Java debug server in tests, answered instead of asking jdtls for one (jdtls
/// needs a JDK and the java-debug bundle).
#[cfg(test)]
pub struct JavaDebugServerPort(pub u16);

#[cfg(test)]
impl gpui_kit::Global for JavaDebugServerPort {}
```

### 3b. The answer

In `fn call(`, replace

```rust
            BackendCall::JavaDebugServer { module } => self.ask(
                cx,
                move |host, label| async move { java::java_debug_server(&host, &label, PathBuf::from(module)).await },
                Self::on_debugger(move |debugger, result, host| debugger.debug_server_started(ticket, result, host)),
            ),
```

with

```rust
            BackendCall::JavaDebugServer { module } => {
                #[cfg(test)]
                if let Some(port) = cx.try_global::<JavaDebugServerPort>().map(|server| server.0) {
                    self.with_host(cx, |debugger, host| debugger.debug_server_started(ticket, Ok(port), host));
                    return;
                }
                self.ask(
                    cx,
                    move |host, label| async move {
                        java::java_debug_server(&host, &label, PathBuf::from(module)).await
                    },
                    Self::on_debugger(move |debugger, result, host| {
                        debugger.debug_server_started(ticket, result, host);
                    }),
                );
            }
```

`call` runs inside `pump`'s loop (through `carry_out`), so the effects the answer adds are carried out on the loop's next turn. If clippy objects to `return` in a match arm of a `()` function, use an `else` block instead.

## 4. What `plugin_java_debug.rs` holds

- **`JavaDebugRuns`** (`pub(super)`, `Default`): by operation, each run's `AgentDebug`, its Run session id and generation, the id of the last console line read, and its store observer.
- **`plugin_java_debug_request(request_id, operation, config, window, cx) -> bool`**. It returns false unless the window has a run driver and a debugger and `AgentDebug::start(config, None, None)` gives a launch (`jdwp_launch`: kind `plugin`, `java-debug` or `kotlin-debug-adapter`, valid non-empty `debugArgs`). Plugin configurations never get a pinned port (`needs_pinned_port` is Maven only, as `debugFreePort` is in the webview). It then:
  1. starts the run with `RunDriver::start(config, StartOptions { launch: Some(launch), .. })`. This is `ops::run::start` (IPC `run_start`). The launch keeps the config's `plugin.operation` param, so `PluginHost::run_operation_sink` makes the plugin hear the process's exit (`run-exited` with status and `killed`);
  2. observes the `RunStore`.
- **`java_debug_started`**: once the start task ends, it answers the waiting request (`answer_plugin_command`, IPC `plugin_answer_command`), or refuses it. It then calls `AgentDebug::session_started(run_id)` and reads the output so far.
- **`java_debug_output`**: on each store notification, it reads the console lines with ids past the last one read. It skips `ChunkKind::System` lines and the `open_line()`, because a half-written line would parse as a wrong port. Each line goes through `AgentDebug::on_line` (`jdwp_listening_port`). The run stops being followed when its session ends or its tab restarts (generation).
- **`java_debug_attach`**: the port must be in 1..=65535. It runs `debug.attach` through `Workbench::run_debug_action`, which is the webview's argument: `{ name, module, hostName: "127.0.0.1", port, process: runId, runConfig }`. `show` is sent only when set. The host is a constant, never what the output said.
- From there, the existing model takes over:
  - `Debugger::attach` → `JavaFlow::attach`, with `fs_read_dir`, `java_debug_server` (IPC `java_debug_server`) and `fs_read_text` for `.project` / `pom.xml`;
  - `DebugState::start_session` with `StartOptions { process, run_config, rerunnable: false }` → `ops::debug::debug_start` (IPC `debug_start`);
  - Stop → `stop_session(.., true)` → `run.stopProcess` (IPC `run_stop`) + `debug_stop`.
- **Logging**: no configuration, environment or argument is logged. Only a port number out of range, and the session name when the configuration cannot be serialised.

## 5. Fixtures

1. **Plugin**: `crates/ide-plugin/tests/fixtures/execution`, copied with `plugin_copy` and edited by `java_detector`. That inserts after `framework = "plugin"` (once in the manifest) `debugger = "java-debug"` and `debugArgs = ["--java_debug"]` (the manifest already has `engines.openidea = "^1.4"`, which `runDetectors.debugger` needs). Its `debug` command does `host::start_run("detected", ["appended"], true)`. No new fixture and no build step: the checked-in `dist/plugin.wasm` is used, as the existing tests use it.
2. **The "JVM"**: `$HOME/bin/execution-tool`, a `sh` script granted as `process:execution-tool`, which runs in the project folder. It prints `execution-tool $*`. Then, when `jdwp.port` exists, it prints `Listening for transport dt_socket at address: <its content>`, so detection goes through the real console. It waits for a file `release` (polling at 0.05 s) and exits with 3, or with 4 after a minute without one, so a failed test leaves nothing running. The tests never wait out a delay: they create `release` when they want the exit.
3. **The Java debug adapter**:
   - `attaching_adapter()` is the existing `debug::tests::FAKE_ADAPTER` with its `launch` branch extended to `attach`. The existing fixture emits `initialized` only for `launch`, so it cannot attach. An attach writes `attach.json` (`hostName`, `port`) in its folder and then runs `main.fake` as a launch does. With `refuse-attach` present it answers `success: false` with java-debug's "Failed to attach to remote debuggee VM. Reason: java.net.ConnectException: Connection refused". The replace is asserted to apply, so a change to `FAKE_ADAPTER` fails loudly.
   - `java_debug_server()` binds `127.0.0.1:0` (the only socket, opened by the test). Each accepted connection spawns `python3 java_dap.py` in the project folder and pipes the socket to its stdio. Its port goes into `JavaDebugServerPort`.
   - The home also has the `fakedap` language (as `start_with_fake_debugger`), so `main.fake` takes breakpoints.
4. The helpers `start_with_fake_debugger`, `stored_events` and `heard_exit` of `plugin_contributions::tests` are private, so they are copied (`start_home`, `stored_events`, `Fixture::heard_exit`).

## 6. Tests (`#[cfg(all(test, unix))]`)

| test                                                                           | proves                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `a_plugins_java_debug_request_attaches_to_the_port_the_jvm_announces`          | The Run console shows `execution-tool test appended --java_debug` (`debugArgs`). The Debug tool window's tab is "Detected test". The breakpoint set before on `main.fake:2` stops the session (`frame_lines == ["main, main.fake:2"]`), so it reached the adapter. The adapter got `{hostName: 127.0.0.1, port: 50123}`. The session's `process` is the run's `run_id`. The plugin hears status 3, not killed. |
| `stop_ends_the_process_and_the_session`                                        | `debug.stop`: session Terminated, Run session ended with `stopped`, plugin hears `killed: true`.                                                                                                                                                                                                                                                                                                               |
| `a_process_that_exits_before_announcing_a_port_leaves_no_debug_session`        | Run Finished with 3, plugin hears 3, no debug session, no `attach.json`, no "Cannot start debugger…", the run is no longer followed.                                                                                                                                                                                                                                                                           |
| `an_attach_the_adapter_refuses_is_reported_with_its_message`                   | Notice "Cannot start debugger for Detected test" whose body has "Connection refused", no live session, process still live; then plugin hears 3.                                                                                                                                                                                                                                                                |
| `a_jdwp_port_out_of_range_is_not_attached_to`                                  | `jdwp.port` = 70000: once the window has read past that line there is no session, no `attach.json` and no "Cannot…" notice.                                                                                                                                                                                                                                                                                    |
| `a_plugins_debug_request_without_the_java_debugger_runs_under_its_own_adapter` | The plain path still goes through `debug.pluginStart`: Debug tab, plugin hears 0, no Run session, no attach.                                                                                                                                                                                                                                                                                                   |

## 7. Compile risks

- `observe_in(&store, window, …)` callback: its signature is `FnMut(&mut Workbench, Entity<RunStore>, &mut Window, &mut Context<Workbench>)`, as in gpui-pre 0.3.12 `app/context.rs`.
- The run's `Subscription` is dropped from inside its own callback when the run ends (`runs.remove`). There is precedent: `close_popup` from a popup's own event handler. If gpui objects, defer the removal with `cx.defer_in(window, …)`.
- In `java_debug_output`, a closure reads `run.seen` while `run` is a `&mut` borrow. This relies on disjoint capture and needs no change unless the borrow checker disagrees, in which case copy `run.seen` into a local first.
- `#[cfg(test)]` on an `if let` statement in `driver.rs` (stable for statements).
- `Fixture::heard_exit` uses `bool::then(..).flatten()`. If clippy wants it, rewrite it as `filter` + `and_then`.
- `debug::tests::FAKE_ADAPTER` is `pub(in crate::workbench)` and the module `debug::tests` is `pub(super)`. Both are reachable from `workbench::plugin_java_debug::tests`.

## 8. Assumptions, and what reading settled

Settled by reading:

- **The start task.** `RunDriver::start`'s task (`drive`) ends after `StartStep::FinalizeLaunch` → `process_started`, that is, once the process started and its `run_id` is recorded, not at exit.
- **The store is notified.** This happens on every flush (`RunDriver::flushed`) and when a session finishes (`run_exited` → `finish_session` → `cx.notify()`).
- **`stopped` is set.** `RunStore::stop_session` sets it for a stop through `run.stopProcess` (`stop_run` → `stop`).
- **`killed` comes from Stop.** `ops::run::stop` calls `plugins.run_operation_stopped(id)` first, which sets the operation's `closed`. `run_operation_sink` then reports `killed: closed` on `RunExited`.
- **Breakpoints reach a new session whatever the file's language.** `ide_dap::Debugger` (lib.rs, before `configurationDone`) sends every stored file with an enabled breakpoint to the new session.
- **The adapter is the built-in one, and nothing is spawned.** `find_config(languages, "java-debug")` finds the built-in `languages/java.toml` (`ide-language` `BUILTIN`). `debugServer` makes `ProcessLauncher::launch` connect to the port instead of starting the command.

Still assumptions (not run):

1. The detected configuration that `PluginHost::start_run` sends carries `debug_adapter: Some("java-debug")` and `params.debugArgs`. Read in `run_detectors.rs` `detector_params` and `api.rs` `run_configuration`; not run.
2. `launch_arguments` passes the attach's `cwd` (the project root) or none. Either way the adapter, running in the root, finds `main.fake`.
3. A refused attach response before `initialized` makes `debug_start` fail with a message that contains the adapter's message.
4. The console's `ConsoleLine::id` increases along `lines()`, which the `partition_point` relies on.

## 9. Tests most likely to fail first

- Test 4 on the notice body if `ide-dap` replaces the adapter's message (assumption 3). Relax it to the title.
- Test 1 on the frame text if the frames view shows the adapter's relative `./main.fake` differently from the basename.
- Any test on compile errors in the observer closure (section 7).

## 10. What the webview does that is not built here

- `run.debugConfig` / Debug from the chooser on a JDWP configuration (Maven, Gradle, a plugin detector): `debugConfig` in `src/features/run/index.ts` sends some of these through `debugWithAgent`. Natively, `Workbench::debug_config` goes straight to `debug.startConfig`. This is outside a plugin's request; `AgentDebug` and `needs_pinned_port` (`debugFreePort`) are ready for it.
- The "Cannot attach the debugger" report of a failed Java flow and "The Java debugger is not installed" (`JavaFlow::debug_server_started` with an error) are not exercised: the test seam always answers `Ok(port)`. Both are covered by the model tests in `debugger::java::tests`.
- `debug.attach` stays unregistered (it is not in `DEBUG_ACTIONS`). It is reached through `run_debug_action`, so no test runs it through the registry.

## 11. Inventory rows (`docs/gpui-port/parity.json`)

| key                                                                                                                                                                                                                                                            | test                                                                                                                                                                                                                                                                                                                  | status                                                                                                                                                                                                                 |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:debug.pluginStart`                                                                                                                                                                                                                                     | `workbench::plugin_java_debug::tests::a_plugins_java_debug_request_attaches_to_the_port_the_jvm_announces`, `…::a_plugins_debug_request_without_the_java_debugger_runs_under_its_own_adapter` (plus the existing two in `plugin_contributions::tests`)                                                                | partial. Once these pass, drop "the JDWP branch … is not reached" from the proof. Still missing: on the plain path, a waiting request is answered when the session ends rather than when it starts (not changed here). |
| `ipc:plugin_prepare_debug_run`                                                                                                                                                                                                                                 | unchanged                                                                                                                                                                                                                                                                                                             | partial. The Java branch does not call it (the webview's `debugWithAgent` does not either); the gap in the proof is unchanged.                                                                                         |
| `wit:wit/run.wit#run`                                                                                                                                                                                                                                          | the six tests above                                                                                                                                                                                                                                                                                                   | done once they pass: "Missing: the JDWP debug of a plugin detector" is covered.                                                                                                                                        |
| `wit:wit/run.wit#run-configurations`                                                                                                                                                                                                                           | unchanged                                                                                                                                                                                                                                                                                                             | done                                                                                                                                                                                                                   |
| `unit:src/features/run/java.test.ts`                                                                                                                                                                                                                           | "plugin run detectors with an attaching debugger" 2/2 (`run::java_debug::tests::plugin_detectors_start_the_command_with_the_detectors_debug_arguments_once`, `…::plugin_detectors_need_both_a_jvm_debugger_and_debug_arguments`); "joins arguments for java-debug and reads JDWP's port" in `run::java_launch::tests` | done (already)                                                                                                                                                                                                         |
| `ipc:run_start`                                                                                                                                                                                                                                                | test 1 (launch with debugArgs)                                                                                                                                                                                                                                                                                        | done (already)                                                                                                                                                                                                         |
| `ipc:debug_start`                                                                                                                                                                                                                                              | test 1 (attach), test 4 (refused)                                                                                                                                                                                                                                                                                     | done (already)                                                                                                                                                                                                         |
| No `ipc:java_debug_server`, `ipc:plugin_answer_command`, `action:debug.attach` or `action:run.pluginStart` key exists in `parity.json` (checked). In the tests, `java_debug_server` is answered by `JavaDebugServerPort` and is never exercised against jdtls. |
