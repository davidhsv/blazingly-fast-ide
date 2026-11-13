I wrote the Java debug branch and its six tests, but nothing has been compiled or run. On two points I followed the webview instead of your test list, so please confirm before it is integrated.

**Decisions for you**

- **No "port never appeared" message or timeout.** `debugWithAgent` (end of `src/features/run/javaDebug.ts`) has no timer. A process that exits without printing a port is just a finished run: the Run tab shows the exit and the plugin hears the exit code. The only timeout on this path is java-debug's own `timeout: 30000` in the attach request, which the native code already sends. The test checks that no debug session and no error notice appear.
- **A refused attach does not stop the process.** `debug.attach` (`src/features/debugger/actions.ts` ~451) stops the process only for Python. A refused Java attach shows "Cannot start debugger for {name}" with the adapter's message, and the JVM keeps waiting in its Run tab. If you want it stopped, the change is small: register `AfterStart::StopProcessOnFailure` for the Java branch in `Debugger::attach` (`debugger/actions.rs`).
- **One deliberate deviation:** if the run does not start, the webview reports success and the plugin never hears an end. I do what the native run path does: the plugin hears an end without a status and gets "Run did not start".

**Files**

- `app-gpui/src/workbench/plugin_java_debug.rs` (588 lines)
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/plugin_java_debug_integration.md` (221 lines)

**Flow as built**

1. **Where the branch goes.** It belongs in `plugin_run_request` in `plugin_contributions.rs`, not in `debugger/actions.rs`. The plugin-target check in `actions.rs` matches the webview and needs no change. The webview makes this decision in `run.pluginStart`.
2. **Start.** `AgentDebug::start` (`jdwp_launch` adds the detector's `debugArgs`), then `RunDriver::start` with that launch, which calls `ops::run::start`. The launch keeps `plugin.operation`, so the backend tells the plugin the exit code, and `killed` after Stop.
3. **Answer.** Once the start task ends, the waiting request is answered (or refused), then `session_started(run_id)` is called.
4. **Port detection.** A store observer reads new console lines, skipping the IDE's own messages and any half-written line, and passes each to `AgentDebug::on_line`.
5. **Attach.** The port must be 1–65535. It then calls `run_debug_action("debug.attach", {name, module, hostName: "127.0.0.1", port, process, runConfig})`, the shape the webview sends. The host is a constant, never taken from the output. From there the existing code runs: `JavaFlow::attach`, then `java_debug_server`, then `debug_start`. Stop in the debugger already stops the process too via `run.stopProcess`.
6. **Logging.** Nothing from the configuration or its environment is logged.

**Fixtures**

- **Plugin:** the checked-in `execution` fixture, with `debugger = "java-debug"` and `debugArgs = ["--java_debug"]` added to the copied manifest. No new plugin and no build step.
- **"JVM":** an `sh` script that prints its arguments and the real "Listening for transport dt_socket at address: N" line. It exits with 3 when the test creates a `release` file, or with 4 after a minute.
- **Adapter:** the existing fake adapter can't attach (it only handles `launch`), so the test patches a copy that can. It records the host and port it was asked for, and refuses when a marker file exists. The test runs it behind a loopback listener it opens itself.

**Hunks in compiled files (exact text in the notes)**

- `mod.rs`: `mod plugin_java_debug;`.
- `plugin_contributions.rs`: the dispatch at `if debug { return self.plugin_debug_request(…`; `pub(super)` on `RUN_NOT_STARTED` and `refuse_plugin_run`; a `java_debug: JavaDebugRuns` field in `PluginContributions`.
- `debugger/driver.rs`, a test-only `JavaDebugServerPort` global. Its arm for asking jdtls for the debug server's port uses it in tests. The Java adapter is hard-wired to jdtls, and a fake jdtls is blocked by its JDK 21 check. A hook inside `ide-host` would not reach app-gpui's tests, so the seam has to live in `driver.rs`.

**Not built**

- Debug from the chooser (`run.debugConfig`) on JDWP configurations such as Maven or Gradle; this is outside a plugin's request.
- `debug.attach` stays unregistered.
- The "Cannot attach the debugger" and "Java debugger is not installed" reports are not exercised, because the test seam always returns a port.

**Check first**

- A refused attach makes `debug_start` fail with the adapter's message. If not, relax that test to the title.
- `launch_arguments` passes the attach's working directory.
- Console line ids increase in order.
- I settled by reading, not running:
  - the start task ends once the process has started;
  - breakpoints reach a new session whatever the file's language;
  - Stop marks the plugin's run as killed;
  - the built-in java-debug config connects to the given port without spawning anything.

**Inventory rows**

- `action:debug.pluginStart` stays partial: the existing gap on the plain path remains.
- `wit:wit/run.wit#run` becomes done once the tests pass.
- `ipc:plugin_prepare_debug_run` is unchanged; the Java branch doesn't call it, as in the webview.
- `unit:src/features/run/java.test.ts` is already done; its two plugin-detector cases (2/2) were covered before this change.
- There are no parity keys for `java_debug_server` or `debug.attach`.
