# Test tree of the Run tool window: integration notes

Draft files, not declared yet, not compiled. Nothing in them is referenced from an existing file. All `file:line` anchors are at HEAD of branch `gpui`.

## Files

- `app-gpui/src/run/test_tree_pending.rs` (11 lines): declares `model`, `rows`, `view` and `#[cfg(test)] mod view_tests`, and re-exports the public types.
- `app-gpui/src/run/test_tree_pending/model.rs` (283 lines): `RunTests`, which holds one `TestResults` and one parser per session start, keyed by session id and generation. It has `started`, `feed`, `finish`, `closed`, `reset`, `results`, `failed_paths`, `failed_count`, `states` and `clear_states`. Helpers: `finished_states`, `node_output`, `first_located`, `rerun_failed_target`, `rerun_failed_starts`, `can_rerun_failed` and `can_run_tests`. Four plain `#[test]`s.
- `app-gpui/src/run/test_tree_pending/rows.rs` (379 lines):
  - `TestFilters`: the stored filters, under key `openidea.run.testFilters`.
  - `TreeIndex`: the filtered tree, built in one pass, with per-node status and duration over the tests in view, default expansion, ancestors and the failed-test steps.
  - `TestRow`, `all_suites` and `status_icon`.
  - Four plain `#[test]`s, one of them on 20,000 tests.
- `app-gpui/src/run/test_tree_pending/view.rs` (779 lines): the `TestTreeView` entity, with `TreeSession`, `TestTreeEvent`, `TestTreeCommand` and `HeaderButton`.
- `app-gpui/src/run/test_tree_pending/view_tests.rs` (328 lines): 12 `#[gpui_kit::test]`s. Each opens a test window, feeds `RunTests` with canned cargo output through `CargoParser` (by way of `create_test_parser`), and asserts the rows, marks, header, selected output and emitted events.
- `app-gpui/src/workbench/tests/proofs45_test_tree_pending.rs` (154 lines): one end-to-end test. The test run uses a temp-dir shell script named `cargo` as the stand-in and kind `program`, so `framework_for` picks Cargo through the `cargo` word in the command line.

## What the draft covers

How 20,000 tests stay usable: `TreeIndex` (with the counts and duration) is rebuilt only when the results' `version`, the session or the filters change, which is at most once per console flush (50 ms). The rows are rebuilt only when the index or the expansion changes, and the neighbouring-failure flags only on a selection change. Render reads the cached `Rc`s, and `uniform_list` draws only the rows in view (`rows::tests::twenty_thousand_tests_index_once_and_list_only_the_open_suites`).

Covered, in priority order:

1. The tree: status icons, the header counts and duration, selection and the selected test's output, keys (up, down, home, end, page up/down, left, right, enter, f4, ⌘↓), double-click, and the jump to source.
2. The filters (persisted) and Expand All / Collapse All.
3. Previous / Next Failed Test.
4. Rerun Failed Tests: the model functions and the e2e test. The button and the action are integration hunks.
5. Show Diff (the comparison link).
6. The gutter states: the model records them, and the gutter read is an integration hunk.

Not covered:

- Multi-selection: shift- and ⌘-click ranges, and ⌘A in the tree. The context menu acts on the selected test only.
- Tree speed search.
- The plugin framework's asynchronous Run / Debug and Rerun Failed. `Selected::Plugin` counts as unavailable.
- The .NET TRX reports and the JVM JUnit XML reports applied at exit (`store.ts:1157-1175`, `applyTestReports` at `store.ts:1206`).
- `prependOutput`.
- Decoding machine-readable stdout (go `-json`, TeamCity, Ruby reporter) for the console.
- A spinner for running tests. The crate has no spinner element, so the running icon is `test-pending`.
- Zig: there is no native zig parser.

## Webview behaviour, with references

Tree element and header:

- When the tree appears: `rt.tests` exists only when `config.isTest` (`src/features/run/store.ts:686`). The tree is then shown left of the console in a split, 32% by default with a 140px minimum (`src/features/run/RunView.tsx:256-276`).
- Header (`src/features/run/TestTreeView.tsx:268-320`):
  - Status icon: running while the process lives, else failed when any test failed, passed when any test exists, pending otherwise (`:265`).
  - Counts: `{n} failing` in `--danger`, `{n} passed`, `{n} ignored` in `--fg-subtle`. With no tests it says "Running…" or "No tests". The summary duration follows (`:272-277`).
  - Buttons: Show Passed (`test-show-passed`, pressed state) and Show Ignored (`test-show-ignored`) at `:280-295`.
  - Expand All (`expand-all`, action `run.expandAllTests`) and Collapse All (`collapse-all`, `run.collapseAllTests`) at `:296-303`. Both are disabled without suites (`:252`).
  - Previous Failed Test (`arrow-up`, `navigate.prevOccurrence`) and Next Failed Test (`arrow-down`, `navigate.nextOccurrence`) at `:304-319`. Each is disabled when there is no failure in that direction (`:262-263`).
  - There is no sort control.
- Status icons: `test-passed`, `test-failed`, `test-ignored`, a spokes spinner for running, `test-pending` otherwise (`:84-97`).
- Rows: status icon, name (`--fg-subtle` when skipped), and a duration in `--fg-subtle` mono `--fs-small` (`:343-353`, `Run.module.css:252-288`). The duration is the node's own, else the sum over its children (`:70-82`), using `formatDuration` (`:62-68`).
- Empty tree: "Waiting for test results…" while running, "No test results" when there are none, else "Passed tests are hidden" (`:354-362`).

Filters, expansion and selection:

- Filters are stored in `localStorage` under `openidea.run.testFilters`. `showPassed` defaults to false and `showIgnored` to true (`:23-47`). Filtering is by a leaf's own status, and a suite with no kept leaf disappears (`:117-128`, `:222-226`). A suite's status and duration are computed over the kept children, because `nodeStatus` runs on the filtered copy.
- Expansion: top-level suites and failed suites start open, and the user's toggles win afterwards (`:228-241`). Expand All opens every suite of the unfiltered results; Collapse All closes all of them, which leaves the top-level suites in view (`:243-251`). Both actions have `when: projectOpen && activeToolWindow == 'run'` and `keyWhen: testTreeFocus` (`src/features/run/index.ts:682-699`).
- Default shortcuts:
  - IntelliJ macOS: `run.expandAllTests` is ⌘=, `run.collapseAllTests` is ⌘- (`src/platform/keymaps/intellij-macos.ts:226-227`).
  - IntelliJ Windows: Ctrl+= and Ctrl+- (`intellij-windows.ts:227-228`).
  - Eclipse: numpad bindings, and `run.rerunFailed` is Alt+Shift+G (`eclipse-windows.ts:157`). IntelliJ has no `run.rerunFailed` binding.
  - These are already in `app-gpui/assets/gen/keymaps.json` and `actions.json:7088`, `7167`, `7299`.
- Click selects, and the console then shows `nodeOutput` of the selection with a "Show all output" link. With no output it shows all output (`RunView.tsx:122-145`).
- Double-click, Enter (`activateExpandable`), F4 and ⌘↓ jump to source (`TestTreeView.tsx:335-341`, `src/ui/Tree.tsx:541-549`, `:607-611`). The target is the node's location, else the first located failed descendant (`TestTreeView.tsx:168-171`, `:368-376`).
- Arrow keys (`Tree.tsx:563-597`):
  - Left collapses an open suite, else selects the parent.
  - Right expands a closed suite, else selects the first child.
  - Home, End, Page Up and Page Down move the selection.

Failed-test navigation and context menu:

- Previous / Next Failed Test (`TestTreeView.tsx:254-261`, `Tree.tsx:885-921`) walks every node in tree order, expanded or not. It steps to the first failure after or before the lead, does not wrap, returns `{index (1-based), total}`, expands ancestors, and opens the node's own location, with focus only when asked.
- Context menu (`TestTreeView.tsx:173-220`):
  - Run '{name}' and Debug '{name}', disabled until the framework can narrow the run (`canRunSelectedTests`, `store.ts:1525-1531`).
  - Show Diff, when a single failed test has a comparison. It runs `diff.compareTexts` with path `{path joined by /}.txt`, title "Comparison Failure: {name}", and left/right `{text,label}` (Expected/Actual, or the framework's labels) at `:49-60`.
  - Jump to Source (F4), disabled without a location.

Rerun Failed Tests:

- A toolbar button sits in the session's toolbar (`RunView.tsx:200-209`): icon `rerun`, danger tone, title "Rerun Failed Tests". It appears only when `rt.tests` exists, and is disabled unless `rerunFailedConfigs(baseConfig, failedTests)` is non-empty and the process has ended (`:97-120`).
- The action `run.rerunFailed` (`index.ts:661-680`) has `when: projectOpen && run.canRerunFailed`. It uses the active session if it has failures, else the last one that has. If none has, it shows the info notification "No failed tests to rerun".
- `rerunFailed` (`store.ts:1476-1487`) starts each derived configuration, the first in the same tab, all with base config `base`.

Feeding the parser and finishing:

- Feeding (`store.ts:684-720`): every non-system console line goes to `parser.feed`. For go, phpunit, jvm and non-rspec ruby, stdout lines go through a decoder that feeds raw lines and shows decoded text.
- At exit (`store.ts:1155-1177`): `parser.end()`, then .NET and JVM reports, then `tests.finish(code)` (still-running leaves become passed after exit 0, else skipped), then `recordTestStates`.

Gutter and tab:

- The gutter reads `testState({path: target.path, file: path if it matches .py/.js/.ts/.zig})` (`src/features/run/runGutter.ts:139-150`). Icons: `gutter-run-failed`, `gutter-run-passed`, else `gutter-run-all` or `gutter-run`.
- The tab shows only its icon, colour and tooltip (`RunView.tsx:283-298`). There are no test counts in the tab.

## Existing functions the draft calls (HEAD)

Models and helpers:

- `app-gpui/src/run_testing/results.rs:59-331`:
  - `TestResults::{new, roots, children, get, node_at, status, apply, finish, counts, is_empty}`, and the `version`, `finished` and `summary` fields.
  - `TestNode::is_leaf`, and the fields `id`, `name`, `path`, `own`, `duration_ms`, `location`, `output` (`:16-35`).
- `app-gpui/src/run_testing/parsers.rs:174` `pub fn create_test_parser(config: &RunConfig) -> Box<dyn TestOutputParser>`.
- `app-gpui/src/run_testing/types.rs:126` `trait TestOutputParser { fn feed(&mut self, line: &str) -> Vec<TestEvent>; fn end(&mut self) -> Vec<TestEvent> }`.
- `types.rs:60-76` `TestEvent::result`, and `types.rs:116` `test_path`.
- `app-gpui/src/run_testing/rerun.rs`:
  - `:37` `failed_test_paths(&TestResults)`
  - `:249` `leaf_test_paths(&TestResults, &[&TestNode])`
  - `:259` `enum Selected<T>`, and `:267` `Selected::ready`
  - `:321` `rerun_failed_configs(&RunConfig, &[Vec<String>])`
  - `:327` `tests_configs(&RunConfig, &[Vec<String>], &str)`
- `app-gpui/src/run/navigate.rs:19` `run_directory`, `app-gpui/src/run/configs.rs:104` `cwd_of` and `:86` `blank_config` (for hunk 7 and the tests).
- `app-gpui/src/run_testing/comparison.rs` `pub fn parse_comparison(output: &[String]) -> Option<Comparison>`, with `Comparison { expected, actual, labels: Option<SideLabels { expected, actual }> }`.
- `app-gpui/src/run_producers/context_run.rs`:
  - `:315` `enum TestState`
  - `:358` `TestStates` with `record(impl IntoIterator<Item = (Vec<String>, TestState)>)`, `clear`, `version`, `test_state(&[String], Option<&str>)` and `gutter_state(&RunTarget, &str)`
  - `:420` `marker_icon(TargetKind, Option<TestState>)`
- `app-gpui/src/run/console.rs:13` `ChunkKind`, and `:70` `CompletedLine { text, kind }`.
- `app-gpui/src/run/store.rs`:
  - `:1496` `StartOptions { reuse, base_config, .. }`
  - `:827` `RunStore::session`
  - `:883` `RunStore::base_config_of`
  - the fields `RunStore::active_session` and `sessions`
- `app-gpui/src/run/sessions.rs:29` `format_duration(f64) -> String`, which matches the webview's.
- `app-gpui/src/platform/storage.rs:42` `UiStorage::get`, and `:52` `UiStorage::set`.
- `app-gpui/src/platform_more/occurrences.rs` `Direction` and `OccurrenceStep { index, total }`.
- `app-gpui/src/vcs_ops/menu.rs:63` `MenuItem::item`, with `.icon`, `.shortcut`, `.disabled`, `label`, `is_disabled`, `command` and `MenuItem::Separator`.

UI and test helpers:

- `app-gpui/src/theme/mod.rs:126` `theme::icon`, plus `theme::color` and `theme::length`.
- The `uniform_list` and `UniformListScrollHandle` pattern of `app-gpui/src/services/view.rs:587-676`.
- `WithTooltip::{with_tooltip, with_tooltip_shortcut}` (`app-gpui/src/ui/tooltip.rs:311-319`).
- E2E helpers in `app-gpui/src/workbench/mod.rs`:
  - `:8315` `run_project`
  - `:8335` `wait_long`
  - `:8595` `click_on`
  - `:8617` `run_action`
  - `:1485` `Workbench::editor`
  - `:2086` `Workbench::notifications`
- `editor/view.rs:536` `Editor::path`, and `:643` `Editor::caret_position`.
- `workbench/editor_gutter/run_marks.rs:251` `editor_gutter_run_marks`.

## Hunks for existing files

1. **Declare the modules.**
   - In `app-gpui/src/run/mod.rs`, after `pub mod terminal_output;`, add `pub mod test_tree_pending;`. Rename it `test_tree` when promoting it.
   - In `app-gpui/src/workbench/mod.rs`, in the test module list after `mod proofs44_editor_tabs;` (`:4684`), add `mod proofs45_test_tree_pending;`.

2. **Own the tests in the driver.**
   - In `struct RunDriver` (`app-gpui/src/run/driver.rs:140`), add the field `tests: Entity<RunTests>`.
   - In `RunDriver::new` (`:167`), `new` takes no `cx`. Either change the signature to take `cx: &mut Context<Self>` and add `tests: cx.new(|_| RunTests::new())`, or create the entity in the workbench where the driver is made and pass it with a `with_tests(tests)` builder like `with_window`.
   - Add `pub fn tests(&self) -> &Entity<RunTests> { &self.tests }`.
   - In `reset` (`:354`), after `self.flushes.clear();`, add `self.tests.update(cx, |tests, _| tests.reset());`.

3. **Start the tree for each start.**
   - In `run_process` (`driver.rs:1093`), inside the `this.update(cx, |driver, cx| { driver.pump(session, generation, chunks, cx); ... })` closure, before `driver.pump(...)`, add:
     ```rust
     let config = driver.store.read(cx).session(&session).map(|s| s.config.clone());
     if let Some(config) = config {
         driver.tests.update(cx, |tests, cx| {
             tests.started(&session, generation, &config);
             cx.notify();
         });
     }
     ```
   - The parser is chosen from the session's configuration, as `newRuntime(config)` does, not from the launch. `session` is moved into `pump`, so clone it first.

4. **Feed the flushed lines.**
   - In `schedule_flush` (`driver.rs:1178`) and `flush` (`:1198`), pass `flushed.lines` on. Change `fn flushed(&mut self, session: &str, changed: bool, open_browser: bool, cx)` to also take `lines: Vec<CompletedLine>`.
   - At the top of `flushed`, add:
     ```rust
     let generation = self.store.read(cx).generation(session);
     if let Some(generation) = generation && !lines.is_empty() {
         self.tests.update(cx, |tests, cx| if tests.feed(session, generation, &lines) { cx.notify() });
     }
     ```
   - A late flush for a closed session finds no entry and is dropped.

5. **Finish the tree, then count failures.**
   - In `finish` (`driver.rs:1238`), the store computes the notification inside `finish_session` (`store.rs:1128-1158`), but the final lines come out of that same call (`store.rs:1148-1152`).
   - Add to `RunStore` a `pub fn take_final_lines(&mut self, session_id: &str, now_ms: f64) -> Vec<CompletedLine>`. It does `let mut lines = self.flush(session_id, now_ms).lines; runtime.console.flush(); lines.extend(self.completed_lines(session_id)); lines`. That is the body of `store.rs:1148-1152`; `finish_session` then calls it in their place.
   - In `driver.finish`, before building `FinishContext`, add:
     ```rust
     let current = self.store.read(cx).generation(session);
     if generation.is_none() || generation == current {
         let lines = self.store.update(cx, |store, _| store.take_final_lines(session, now_ms()));
         if let Some(generation) = current {
             self.tests.update(cx, |tests, cx| {
                 tests.feed(session, generation, &lines);
                 tests.finish(session, generation, code);
                 cx.notify();
             });
         }
     }
     ```
   - Replace `failed_tests: 0,` (`driver.rs:1242`) with `failed_tests: self.tests.read(cx).failed_count(session),`.

6. **Forget a closed tab.** In `closed` (`driver.rs:1380`), after `self.flushes.remove(session);`, add `self.tests.update(cx, |tests, _| tests.closed(session));`.

7. **Draw the tree beside the console** (`app-gpui/src/run/view.rs`).
   - `RunView::new` (`:202`) takes `tests: Entity<RunTests>` and creates `let test_tree = cx.new(|cx| TestTreeView::new(tests.clone(), cx));`. Subscribe to it and observe `tests` so `refresh` runs.
   - Add fields `test_tree: Entity<TestTreeView>`, `test_output: Option<ConsoleModel>` and `tests: Entity<RunTests>`, plus `pub fn test_tree(&self) -> Option<&Entity<TestTreeView>>`. It returns `Some` while the current session has results.
   - In `refresh` (`:522`), after `let Some((id, version, terminal)) = current else {...};`, call `self.test_tree.update(cx, |tree, cx| tree.show(session, cx))` with:
     ```rust
     TreeSession {
         id,
         running: session.is_live(),
         cwd: run_directory(self.project_root.as_deref(), cwd_of(&session.config)),
         base_config: store.base_config_of(session).clone(),
     }
     ```
     Pass `None` when `self.tests.read(cx).results(&id)` is `None`.
   - On `TestTreeEvent::Selected`, set `self.test_output` from `tree.selected_output(cx)`: `ConsoleModel::new(ConsoleOptions::default())`, then `append(ChunkKind::Stdout, &(lines.join("\n") + "\n"))`, then `flush()`. Then reset the list as a tab switch does.
   - In `fn console` (`:338`), before the terminal check, add `if self.test_output.is_some() { return self.test_output.as_ref(); }`.
   - In `notice` (`:389`), while `test_output` is set, return the "Show all output" link text. Clicking it calls `test_tree.select(None)`, which clears `test_output`.
   - In `render` (`:1530`), the body is `div().flex_1().min_h_0().flex().child(self.render_toolbar(&session, cx)).child(<console column>)`. When `test_tree()` is `Some`, insert `.child(div().w(relative(0.32)).min_w(px(140.0)).h_full().child(self.test_tree.clone()))` between the toolbar and the console column.

8. **Rerun Failed button and event.**
   - In `render_toolbar` (`view.rs:1189`), after the `run-rerun` child (`:1212-1215`), add, only when the session has results, `self.tool_button("run-rerun-failed", "rerun", ButtonState { enabled: <can>, danger: true, ..on }, cx, move |_, _, cx| cx.emit(RunViewEvent::RerunFailed(id.clone())))` with `.with_tooltip_shortcut("Rerun Failed Tests", "run.rerunFailed")`.
   - `<can>` is `can_rerun_failed(store, tests.read(cx), &session.id)`, computed in `refresh` and cached in a field, not computed per frame.
   - Add the variant `RerunFailed(String)` to `RunViewEvent` (`:49`).
   - In `Workbench::run_view_event` (`workbench/mod.rs:2776`), add an arm:
     ```rust
     RunViewEvent::RerunFailed(session) => driver.update(cx, |driver, cx| {
         let starts = rerun_failed_starts(driver.store().read(cx), driver.tests().read(cx), &session);
         for (config, options) in starts.into_iter().flatten() {
             driver.start(config, options, cx).detach();
         }
     }),
     ```

9. **Tree events in the workbench.**
   - Where `RUN_TOOL_WINDOW` is mounted (`workbench/mod.rs:2563-2568`), the view is created as `RunView::new(store, types, cx)`. Pass `run.read(cx).tests().clone()` too, and `cx.subscribe_in(&view.read(cx).test_tree_entity(), window, Self::test_tree_event)`. Alternatively, let `RunView` re-emit the tree's events as `RunViewEvent` variants.
   - `test_tree_event` handles:
     - `OpenLocation { target, cwd, .. }` with `driver.open_location(target, cwd, cx)` (`driver.rs:1410`).
     - `RunTests { session, ids, debug }` as `runSelectedTests` (`store.ts:1494-1523`): `tests_configs(base, &leaf_test_paths(results, &nodes), name)`. Each config goes through `RunStore::adopt_temporary` with the id `context:tests:{base.id}:{index}:{paths joined}`, then `driver.start(config, StartOptions { base_config: Some(base), ..})`, or `run.debugConfig` with the config as JSON when `debug`.
     - `ShowDiff(arg)` with `execute_action_from(ActionSource::Toolbar, "diff.compareTexts", Some(&arg), window, cx)`.
     - `ShowMenu(position)` with `self.show_menu(&items, position, run, window, cx)`, using `tree.context_menu(cx)` and `tree.run_command(command, cx)`, as the `RunViewEvent::ShowMenu` arm does (`:2811-2818`).
     - `RunAction(id)` with `window.defer(cx, move |window, cx| execute_action_from(ActionSource::Toolbar, id, None, window, cx))`.

10. **Actions and keys.**
    - Next to `registry.handle("run.rerun", …)` (`workbench/mod.rs:901`), register these:
    - `run.rerunFailed` (the `when` gates it, see above): `on_run(window, cx, |driver, cx| { let target = rerun_failed_target(driver.store().read(cx), driver.tests().read(cx)); … })`. Start `rerun_failed_starts` for the target; with none, emit `RunDriverEvent::Notify(NotificationRequest { key: Some("run-rerun-failed".into()), ..NotificationRequest::new(NotificationLevel::Info, "No failed tests to rerun") })`. Check the `key` field name in `NotificationRequest`.
    - `run.expandAllTests` and `run.collapseAllTests`: reach the Run tool window's `RunView` with `tool_window_view(RUN_TOOL_WINDOW)` and call `test_tree.update(cx, TestTreeView::expand_all)` or `collapse_all`.
    - The key conditions (`keyWhen: testTreeFocus`) need the context key `testTreeFocus` set while the tree's focus handle is focused. Hypothesis: a focus listener calling `crate::platform::context::set_context(window, cx, "testTreeFocus", true)` (`platform/context.rs:347`) and `clear_context` on blur.
    - The context key `run.canRerunFailed` must follow `can_rerun_failed` for the active session. Set it where `run_configs.rs:157-162` sets `run.selectedKind` and `run.updatableRunning`.
    - Previous / Next Failed Test: in `go_to_occurrence` (`workbench/editor_extras.rs:204`), add to the `match id.as_str()` an arm before `_ => None`:
      ```rust
      RUN_TOOL_WINDOW => self.tool_window_view(RUN_TOOL_WINDOW)
          .and_then(|v| v.downcast::<RunView>().ok())
          .and_then(|v| v.read(cx).test_tree().cloned())
          .map(|tree| tree.update(cx, |tree, cx| tree.step_failed(direction, focus_editor, cx))),
      ```
    - Shortcuts need no keymap change: the generated `assets/gen/keymaps.json` already has `run.expandAllTests` and `run.collapseAllTests`, and the Eclipse binding of `run.rerunFailed`.

11. **Menu entries.** The webview registers `run.rerunFailed` with category Run (`index.ts:661-666`). It is listed in `actions.json:7299`, so the Run menu and Find Action pick it up once it has a handler. Hypothesis: the native Run menu is built from `actions.json` categories. Check with `git grep -n '"run.rerun"' app-gpui/src/workbench/menus*`.

12. **Gutter states.**
    - In `editor_gutter_run_marks` (`run_marks.rs:251-261`), replace `marker_icon(target.kind, None)` with:
      ```rust
      marker_icon(target.kind, self.run_driver().and_then(|d| d.read(cx).tests().read(cx).states().gutter_state(target, &followed.path)))
      ```
      The function takes `&self` only today, so it becomes `editor_gutter_run_marks(&self, editor: &Entity<Editor>, cx: &App)`, as `proofs45` calls it; update its callers. Do the same in `run_mark_look` (`:346-364`).
    - Watch the driver's `tests` entity: `cx.observe(&tests, …)` in `watch_run_configs` (`run_marks.rs:150`) calls `cx.notify()` when `states().version()` changes, so the gutters redraw.
    - Two `TestStates` types exist. `run_testing::states::TestStates` is unused by the draft. `run_producers::context_run::TestStates` is the one `marker_icon` and `gutter_state` speak. Keep one; the draft uses `context_run`'s.
    - The webview's file-rooted pattern includes `.zig`, the native `FILE_ROOTED` (`context_run.rs:322`) does not.

## Settled by reading

- `driver.open_location` always focuses the editor: it calls `open_resolved(&target, path, true, …)` (`driver.rs:1453`), and the third parameter of `open_resolved` is `focus` (`run/navigate.rs:241`). A non-focusing Previous / Next Failed Test needs a `focus: bool` parameter on `RunDriver::open_location`, which is an addition to `driver.rs`. The event already carries `focus`.
- `diff.compareTexts` takes `{path, title, left: {text, label}, right: {text, label}}`; its handler is at `workbench/diff_tools.rs:145`, used by the test at `:999-1005`.
- `session_icon` is `error` for a finished non-zero exit that was not stopped (`run/sessions.rs:10-27`).
- `NotificationRequest` has `key: Option<String>` and `group: Option<String>` (`workbench/notifications.rs:223-237`). Use `group: Some("run".into())`, as the webview does.
- `RunConfig::is_test` is a plain serialized field (`crates/ide-run/src/lib.rs:47`).

## Hypotheses not settled by reading

Each item ends with how to settle it.

- `CompletedLine.text` is the line without ANSI escapes, as the parsers expect. Settle: `cargo test -p openidea_gpui run::store::tests` with a line holding `\x1b[32mok\x1b[0m`, asserting `flushed.lines[0].text`.
- `CargoParser` builds the path `[module…, test]` without a `Running` header, and `[file, module…, test]` with one. The view tests assume the former, the e2e test the latter. Settle: `cargo test -p openidea_gpui run::test_tree_pending`.
- `ConsoleModel::append` followed by `flush` puts the selected test's lines in `lines()`. Settle: a unit test in `run/console.rs`.
- `file_targets` gives the `#[test] fn adds` and `fn subtracts` lines of a Rust file `RunTarget`s with `path` `["tests", "adds"]`, which the e2e gutter assertion needs. Settle: `cargo test -p openidea_gpui run_producers::producers` for a Rust module.
- `UiStorage::set` persists on its own. Settle: read `platform/storage.rs:52`.
- `Modifiers::platform` is ⌘ in key events (for ⌘↓). Settle: the services view uses it the same way (`services/view.rs:357`).

## Tests most likely to fail first, and why

- `view_tests::*` that click rows. `uniform_list` rows exist only after a layout, and `open` runs `run_until_parked` after `show`. If `debug_bounds("test-row:…")` is `None`, add `cx.update(|window, cx| window.refresh())` or a draw before the click.
- `view_tests::arrows_move_and_fold_and_enter_and_f4_jump_to_the_source`. Keystrokes reach the view only if the click gave it focus; `click_row` calls `window.focus`. A parent `track_focus` might take it, as the view rules warn.
- `view_tests::hides_passed_tests_by_default_and_remembers_the_toggle` opens a second window in the same test. If `add_window_view` on the same `TestAppContext` conflicts with the borrowed first `VisualTestContext`, restructure the test.
- `rows::tests::twenty_thousand_tests_index_once_and_list_only_the_open_suites`: the row count depends on top-level suites starting open.
- `proofs45::a_cargo_test_run_fills_the_tree_jumps_to_the_failure_and_reruns_only_it` depends on every hunk above.
  - The stand-in's heredoc must reach the console as separate lines.
  - `framework_for` must pick Cargo for kind `program` through the `cargo` word in the temp path.
  - The view must swap the console to the selected output.
  - The gutter must have marks for the Rust tests.
  - The call file must hold `test` and then `test -- tests::subtracts --exact` (`cargo_args`, `rerun.rs:95-111`).
- No e2e test drives `run.rerunFailed` without failures: its `when` (`projectOpen && run.canRerunFailed`) refuses the action (`platform/actions.rs:372`, `is_action_enabled`), so `run_action` would panic. The "No failed tests to rerun" branch is reachable only when the context key is stale.

## Inventory rows this would raise

`docs/gpui-port/parity.json`:

- `unit:src/features/run/testing/comparison.test.ts`, `dotnet.test.ts`, `go.test.ts`, `phpunit.test.ts`, `plugin.test.ts`, `ruby.test.ts`, `unittest.test.ts`: each "missing" clause reads "nothing in the app reaches it (… RunStore::output keeps console lines only and run_testing::parsers::create_test_parser is never called …)".
  - Hunks 3 to 5 resolve the `create_test_parser` part for all of them, proved by `proofs45`.
  - Comparison is reached by Show Diff (`view_tests::context_menu_run_debug_the_test_and_show_diff_of_an_assertion_failure`).
  - dotnet stays partial: TRX reports are not read at exit.
  - plugin stays partial: plugin output is not routed.
  - go, phpunit and ruby: their parser is reached, but the console still shows raw records because there is no stdout decoder.
- `unit:src/features/run/testing/rerun.test.ts` and `states.test.ts`: the "this row was model only" clause is resolved by `proofs45` (rerun command line, gutter state). The zig cases stay uncovered.
- `point.registerToolWindow:run`: the "Missing: … there are no test tree or stack-trace tabs" clause loses "no test tree". Stack-trace tabs and tabs in the header stay missing.
- `feature:run`: the "Not yet: gutter run markers, the test tree" clause loses "the test tree", and the gutter marker states with hunk 12.
- `e2e:e2e/run.spec.ts`: "Missing: Rerun Failed Tests of a Go run in the Run window". `proofs45` proves Rerun Failed of a cargo run. The Go flow in `proofs18_e2e_run_pending.rs` becomes runnable once the `run-rerun-failed` button exists (hunk 8), though the go stand-in's JSON shows raw in the console.
- `action:navigate.nextOccurrence` and `action:navigate.prevOccurrence`: hunk 10 adds the Run tool window to the windows that step, by `view_tests::steps_through_the_failed_tests_in_tree_order_and_stops_at_both_ends`. The clause about Find, Inspection and Hierarchy stays.
- No rows were found for these keys, which should be added:
  - `unit:src/features/run/TestTreeView.test.tsx`: covers 10 of its 11 spec cases. The it.each "enables Run and Debug only when the async selector returns support" is not covered, because the native selection is synchronous except for plugins.
  - `action:run.rerunFailed`, `action:run.expandAllTests`, `action:run.collapseAllTests`.
  - `context:testTreeFocus`, `context:run.canRerunFailed`.
- `unit:src/features/run/RunView.test.tsx` is already "done"; its test-icon case ("shows the execution arrow while running, the check for tests") is unaffected.

## What contradicts the brief

- The webview has no sort in the test tree and no "Click to see difference" link. The comparison is the context menu's Show Diff.
- The tab shows no counts. The counts are in the tree header, and the tab shows only the failed-exit icon.
- `run_testing` is not wholly uncalled: `driver.rs:31` uses `with_test_reporting`. The parsers, `TestResults`, rerun and states are uncalled.
- The driver drops `Flushed.lines` (`driver.rs:1189`, `:1200`) and `Finished.lines`, and passes `failed_tests: 0` (`:1242`). The exit notification computes `failed_tests` before the final lines are known (`store.rs:1148-1157`).
- The gutter passes `None` as the test state (`run_marks.rs:257`, `:358`). Two `TestStates`/`TestState` types exist (`run_testing/states.rs:66`, `run_producers/context_run.rs:315-358`), and `marker_icon` takes the `context_run` one.
- There is no zig parser natively, and there is no spinner element for running tests.
- Rerun Failed Tests has no IntelliJ default shortcut; only Eclipse binds it (Alt+Shift+G).
