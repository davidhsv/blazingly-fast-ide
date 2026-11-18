I wired the test results tree into the Run tool window and it is proven: the four gates are green. Nothing is committed or pushed. The four draft files and the window-test file now live under their final names (renamed with `git mv`, which stages the renames). Of the slice's rows, eight rise, six narrow and three stay where they were.

## 1. Gates (measured, final tree)

- **Whole suite (`cargo test --lib`), twice on the final tree, nothing else running:**
  - Run 3: 8467 passed, 0 failed (246.82 s). Load average 8.59 before, 36.51 after.
  - Run 4: 8467 passed, 0 failed (248.31 s). Load average 36.51 before, 124.13 after.
  - Logs with `uptime` and `df` written in: `$S/dtt/suite3.log` and `$S/dtt/suite4.log`.
  - Two earlier runs, before the last test-only edits, both passed 8465 of 8465.
- **Twenty repeats** of `cargo test --lib -- proofs45_test_tree run::test_tree:: proofs27_gutter run::view::tests run::driver::tests run::store::tests` (135 tests): 20 of 20 clean, no test failed even once. Log: `$S/dtt/twenty.log`.
- **`cargo test --test web_view`:** 1 passed.
- **`cargo clippy --all-targets -- -D warnings`:** clean.
- **`cargo build`:** 0 warnings.
- **`node docs/gpui-port/gen-data.mjs --check`:** exit 0.
- **Failures seen along the way:** two of my own new window tests failed on their first run; both were the test's fault (section 2d). No failures in any whole-suite run.
- **Untouched areas:** `git status --short src/ src-tauri/ crates/ e2e/ docs/` prints nothing.

## 2. What changed

### a. Model, and a run's output reaching it

- **Renamed:** `app-gpui/src/run/test_tree_pending{,/*}` → `app-gpui/src/run/test_tree.rs` and `app-gpui/src/run/test_tree/{model,rows,view,view_tests}.rs`; `app-gpui/src/workbench/tests/proofs45_test_tree_pending.rs` → `app-gpui/src/workbench/tests/proofs45_test_tree.rs`.
- **Declared:** `pub mod test_tree;` in `run/mod.rs` and `mod proofs45_test_tree;` in `workbench/mod.rs`.
- **`app-gpui/src/run/driver.rs`:**
  - The driver owns `tests: Entity<RunTests>`; `RunDriver::new` now takes `cx`, so its 5 call sites changed, including `run/driver/tests/proofs8_chooser_pinned.rs`.
  - The tree is started from the tab's configuration as soon as a start opens or restarts the tab. This hooks the `drive` loop, not `run_process` as the notes said, so a rerun never shows the old tree (webview: `store.ts:686`).
  - Flushed lines feed the parser.
  - On exit: the final lines, then `finish`, then the real failed count in the exit notification (`failed_tests: 0` is gone). Closing a tab and resetting the driver clear the tree.
  - New: `rerun_failed`, `run_selected_tests` (`store.ts:1476-1523`) and `open_location_focusing`. `RunDriverEvent::OpenFile` gained `focus`.
- **`app-gpui/src/run/store.rs`:** `take_final_lines` is pulled out of `finish_session`.

### b. The tree in the Run tool window, with selection and the console

- **`app-gpui/src/run/view.rs`:**
  - The tree is drawn between the toolbar and the console at 32% width, at least 140px (`RunView.tsx:256-276`).
  - The selected test's output replaces the console, worked out on every refresh, with a "Show all output" link (`RunView.tsx:122-145`).
  - New "Rerun Failed Tests" button (`run-rerun-failed`) after Debug, danger tone (`RunView.tsx:200-209`).
  - The tree's events are passed on as `RunViewEvent::Tree`.
- **`app-gpui/src/run/test_tree/view.rs`:**
  - The index cache key now includes the start, so a rerun in the same tab cannot reuse a stale index.
  - Passed count and the duration font match the CSS (section 4).
- **`workbench/mod.rs`, `test_tree_event`:** every action is deferred with `window.defer`. That covers `diff.compareTexts`, `run.debugConfig` and the header buttons' actions.

### c. The three actions and the two context keys

- **`workbench/mod.rs`:** handlers for `run.rerunFailed` (`index.ts:661`) and for `run.expandAllTests` / `run.collapseAllTests` (`:682`, `:691`).
- **`workbench/run_configs.rs`:** `run.canRerunFailed` (`index.ts:110-141`) and `testTreeFocus` are published with the window's other context keys, not from a focus listener.

### d. Context menu, Show Diff and the gutter states

- **Menu rows:** Run / Debug the selected test (a temporary configuration, as `store.ts:1494`), Show Diff, Jump to Source.
- **Gutter:** `editor_gutter/run_marks.rs` reads `TestStates::gutter_state`, and the gutter redraws when the states change. `editor_gutter_run_marks` takes `cx`, so 3 call sites in `proofs27_gutter.rs` changed.
- **Show Diff spec case:** `view_tests` now checks the Expected / Actual labels and the Comparison Failure title.
- **Two failing new tests, both my test's fault:**
  - The test that clicked the menu's Run row expected a second tab. The finished tab is reused, as `reusable_tab` does.
  - The Expand / Collapse test ran the actions before the Run window was active; their `when` needs `activeToolWindow == 'run'`.

### e. Stepping through failures

- `workbench/editor_extras.rs`: a Run tool window arm in `go_to_occurrence`. A step taken from the tree keeps the keyboard there, and this is tested.

**The draft's own tests:** all 21 passed on their first run.

## 3. Rows

Every window test below was seen failing with its guard switched off: one mutation per script run, file restored by a trap, hash compared. 22 mutations, all restored = yes. The script is `$S/dtt/mutate.sh`. Tests below are in `workbench::tests::proofs45_test_tree::` unless named in full.

| Row                                                 | State                     | Test                                                                                                                                                                                                                                                                                                                     | Mutation that fails it                                                   | Clause that remains                                                                                                                                                                                                                    |
| --------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:run.rerunFailed`                            | raised                    | `rerun_failed_tests_reruns_only_the_failed_tests_and_is_refused_while_nothing_failed` (asserts the stand-in got `test -- tests::subtracts --exact`, and the action is refused with no call made while nothing failed)                                                                                                    | M4 (the handler reruns nothing); M5 (key always on); M6 (key always off) | none                                                                                                                                                                                                                                   |
| `action:run.expandAllTests` / `collapseAllTests`    | raised                    | `expand_all_and_collapse_all_run_from_the_registry_and_by_their_keys_in_the_tree` (registry, header buttons, ⌘= / ⌘- in the tree; the same keys in the console change nothing)                                                                                                                                           | M7 (the handler is swapped); M8 (`testTreeFocus` is never on)            | none                                                                                                                                                                                                                                   |
| `context:run.canRerunFailed`                        | raised                    | `can_rerun_failed_is_on_after_a_run_with_failures_and_off_while_running_or_without_any` (off while the stand-in is held, on after it exits with a failure, off after a passing run in the same tab)                                                                                                                      | M5, M6, M1                                                               | none                                                                                                                                                                                                                                   |
| `context:testTreeFocus`                             | raised                    | `test_tree_focus_is_on_in_the_tree_and_off_in_the_console_and_the_editor` (on in the tree, off in the console and in the editor after F4 and ⌘↓; the keyed actions reach the tree only then, per the expand test)                                                                                                        | M8                                                                       | none                                                                                                                                                                                                                                   |
| `unit:src/features/run/TestTreeView.test.tsx` (11)  | raised, 10 of 11          | `run::test_tree::view_tests::*`: one test per spec case, named after it, plus a Show Ignored test. Each of the 14 fails under one of V1 to V9: filter always keeps, no failure to step to, header buttons do nothing, single click jumps, no diff, keys ignored, wrong empty text, Expand All opens nothing, header text | V1 to V9                                                                 | "enables Run and Debug only when the async selector returns support": natively the selection is synchronous; only a plugin framework would answer later, and that counts as unavailable (`model.rs` `can_run_tests`)                   |
| `comparison.test.ts`                                | raised                    | `the_context_menu_of_a_failed_test_shows_its_diff_runs_it_and_jumps_to_its_source` (right-click, clicks the Show Diff row, the diff window shows `1` against `2`)                                                                                                                                                        | M11 (Show Diff does nothing)                                             | closes "nothing in the app reaches it"                                                                                                                                                                                                 |
| `unittest.test.ts`                                  | raised                    | `a_unittest_run_shows_its_tests_in_the_tree_and_its_report_in_the_console` (stand-in `manage.py test`, report on stderr; asserts rows, console text and the failure's own output)                                                                                                                                        | M10 (the parser is fed nothing)                                          | closes "nothing in the app reaches it"                                                                                                                                                                                                 |
| `go.test.ts`                                        | narrowed, not raised      | `go_reruns_keep_each_packages_exact_failures_and_the_original_configuration` (measured: the console shows the raw `{"Action":"fail"…}` lines)                                                                                                                                                                            | M12 (no per-package grouping)                                            | the console shows raw `-json` events; the webview decodes them (`store.ts:704`, `GoTestOutput`)                                                                                                                                        |
| `phpunit.test.ts`, `ruby.test.ts`                   | not raised                | none written                                                                                                                                                                                                                                                                                                             | —                                                                        | stdout decoding (`store.ts:706` `TeamcityOutput`, `:708` `RubyTestOutput`). Measured: a search for these decoders finds nothing native. Hypothesis: their records show raw in the console; confirm with a stand-in run like the Go one |
| `rerun.test.ts`                                     | narrowed                  | `a_cargo_test_run_fills_the_tree_jumps_to_the_failure_and_reruns_only_it` (the rerun command line through the window) plus the Go test                                                                                                                                                                                   | M1 (rerun gets no failed paths)                                          | the 2 zig cases                                                                                                                                                                                                                        |
| `states.test.ts`                                    | narrowed (behaviour only) | the cargo test (asserts `gutter-run-failed` on line 18 and `gutter-run-passed` on line 9)                                                                                                                                                                                                                                | M2 (states never recorded)                                               | the zig case. Measured: nothing outside `run_testing/states.rs` uses `run_testing::states`; the gutter uses `run_producers::context_run::TestStates`. So the row's named proof is still code the app does not reach                    |
| `point.registerToolWindow:run`                      | narrowed                  | cargo test plus 4 others                                                                                                                                                                                                                                                                                                 | M3 (the tree is never drawn; 5 tests fail)                               | the row's "no test tree" is gone; "tabs are drawn in the body rather than the header … no … stack-trace tabs" stays                                                                                                                    |
| `e2e:e2e/run.spec.ts`                               | narrowed                  | `go_reruns_keep_each_packages_exact_failures_and_the_original_configuration`: the spec's own flow (`run.spec.ts:84`), one rerun per package, both keeping `test -json ./...`                                                                                                                                             | M12                                                                      | "CMake building the target before launch"; the Go JSON shows raw in the console                                                                                                                                                        |
| `action:navigate.nextOccurrence` / `prevOccurrence` | narrowed                  | `next_and_previous_occurrence_step_through_the_failed_tests_of_the_run_tool_window` ("1 of 2", "2 of 2", no wrap, caret at each failure, keyboard kept when stepped from the tree)                                                                                                                                       | M9 (the Run window does not step)                                        | "the Find, Inspection and Hierarchy tool windows do not step …, and a TODO step shows no i of n message"                                                                                                                               |

- **Missing row entries:** these keys have no entry in `parity.json` at all (a search finds none): `action:run.rerunFailed`, `action:run.expandAllTests`, `action:run.collapseAllTests`, `context:run.canRerunFailed`, `context:testTreeFocus`, `unit:src/features/run/TestTreeView.test.tsx`. The row text for them has to be written.
- **Code under `crates/`:** none of these tests exercises any.
- **Real programs the tests run:** `/bin/sh`, `cat` and `sleep`, through stand-in scripts in temp folders. No real toolchain.

## 4. Differences from the webview

**Fixed:**

- Header passed count colour is `--fg-secondary` (`Run.module.css:239`).
- Duration uses the mono font (`:274`).
- Rerun Failed button sits after Debug, not after Rerun as the draft had it (`RunView.tsx:200`).
- The tree starts empty at each start of the tab (`store.ts:686`).
- A non-focusing step really keeps the keyboard (`TestTreeView.tsx:254-261`).

**Not fixed:**

- No split handle: the tree is a fixed 32%, at least 140px (`RunView.tsx:257`).
- A running test shows the `test-pending` icon, not a spinner (`TestTreeView.tsx:93`).
- No multi-selection, ⌘A or shift-ranges, and no "{n} tests" menu label (`TestTreeView.tsx:173-176`, `Tree.tsx:539`, `:613`).
- No tree speed search, so no Escape / Backspace on a query (`Tree.tsx:553-560`).
- Page Up / Page Down move 10 rows, not a viewport (`Tree.tsx:597-604`).
- The plugin framework's asynchronous Run / Debug and Rerun Failed (`TestTreeView.tsx:208-216`).
- TRX and JUnit reports read at exit (`store.ts:1157-1175`).
- Go, PHPUnit and Ruby stdout decoding (`store.ts:704-708`).
- No zig parser (`runGutter.ts:137`).
- `activeToolWindow` is the focused tool window natively: Expand All / Collapse All run only while the Run window has the keyboard. A click on the header gives it the keyboard.

## 5. Hypotheses of the notes

- **ANSI-free completed lines:** measured. `run::console::tests::reports_completed_lines_plain_text_only` passed, and the window tests see plain text.
- **Cargo test path:** read in `cargo.rs` `path_of`. Measured by the view tests' rows `[math, subtracts]` and the e2e rows `[src/lib.rs, tests, …]`.
- **Append-then-flush shows the selected test's output:** measured by the cargo window test.
- **Rust gutter target `["tests","adds"]`:** read in `targets.rs:153-190`. Measured by the gutter marks on lines 9 and 18.
- **`UiStorage::set` persists:** read only (`storage.rs:52` calls `write`). The tests use a storage without a file. Hypothesis: confirm with a `UiStorage` loaded from a temp file and read back.
- **⌘ is `Modifiers::platform`:** read in gpui `keystroke.rs:170`. Measured by `cmd-down` in the view test and in the window test with the keymap installed.

## 6. False or stale in the notes and brief

- Hunk 3 (start the tree in `run_process`) would show the previous tree while a rerun prepares.
- Hunk 9 ran actions from the workbench handler without deferring them.
- The notes' anchors were stale.
- The empty rows have no entries in `parity.json`.
- `states.test.ts`'s named proof module is unused by the app.
- Expand All / Collapse All are refused unless the Run window has the keyboard.

**Mechanics:**

- `git mv` stages the renames, which conflicts with rule 2 of `int_rules.md`; the brief asked for it.
- HEAD moved to `0a44775e5` (docs and other agents' pending drafts) while I worked.
- Hypothesis on cost: `publish_run_context` walks the sessions' results to compute `run.canRerunFailed` on every workbench render. Confirm with a 20,000-test run and a frame-time measurement.

## Mouse paths not tested

- The Debug row of the tree's context menu.
- The Previous / Next Failed Test header buttons are clicked only in a view test (the action event is asserted); no window test clicks them.
- Row chevrons (`test-toggle:*`) are clicked only in a view test.

**Not proven in a window:** the "Tests failed: N" notification count, since the Run window was visible in every test.

**Files changed** (all under `$W/app-gpui/src/`):

- `run/test_tree.rs`, `run/test_tree/{model,rows,view,view_tests}.rs`
- `run/driver.rs`, `run/driver/tests/proofs8_chooser_pinned.rs`
- `run/mod.rs`, `run/store.rs`, `run/view.rs`
- `workbench/mod.rs`, `workbench/run_configs.rs`, `workbench/editor_extras.rs`
- `workbench/editor_gutter.rs`, `workbench/editor_gutter/run_marks.rs`
- `workbench/tests/proofs27_gutter.rs`, `workbench/tests/proofs45_test_tree.rs`
