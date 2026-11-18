I've written the test tree as new, undeclared files plus the integration notes; none of it has been compiled or run. Nothing references the new files, and no existing file was edited.

## Files written

| File                                                         | Lines | Contents                                                                                                  |
| ------------------------------------------------------------ | ----- | --------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/run/test_tree_pending.rs`                      | 11    | Declares `model`, `rows`, `view` and the test module                                                      |
| `app-gpui/src/run/test_tree_pending/model.rs`                | 283   | Per-session test results and parser, fed from completed output lines; Rerun Failed helpers; 4 model tests |
| `app-gpui/src/run/test_tree_pending/rows.rs`                 | 379   | Filters, the filtered tree index, rows, failed-test stepping; 4 tests, one with 20,000 tests              |
| `app-gpui/src/run/test_tree_pending/view.rs`                 | 779   | The `TestTreeView` itself                                                                                 |
| `app-gpui/src/run/test_tree_pending/view_tests.rs`           | 328   | 12 window tests fed with canned cargo output through the existing parser                                  |
| `app-gpui/src/workbench/tests/proofs45_test_tree_pending.rs` | 154   | One end-to-end test                                                                                       |
| `$S/drafts/test_tree_integration.md`                         | 333   | Hunks, behaviours with `file:line`, hypotheses, parity rows                                               |

Declarations the integrator adds:

- `pub mod test_tree_pending;` in `run/mod.rs`
- `mod proofs45_test_tree_pending;` in the `workbench/mod.rs` test list, after line 4684

## The view

- **Constructor:** `TestTreeView::new(tests, cx)`. The caller feeds it with `show(Option<TreeSession { id, running, cwd, base_config }>)`.
- **Events:**
  - `Selected` (the console should show that test's output)
  - `OpenLocation { target, cwd, focus }`
  - `RunTests { ids, debug }`
  - `ShowDiff` (the argument of `diff.compareTexts`)
  - `ShowMenu`
  - `RunAction` (the header buttons run `run.expandAllTests`, `run.collapseAllTests`, `navigate.prevOccurrence`, `navigate.nextOccurrence`)
- **Test accessors:** `rows`, `marks`, `durations`, `selected`, `selected_output`, `header`, `header_icon`, `header_buttons`, `empty_text`, `filters`, `context_menu`.
- **Actions:** `expand_all`, `collapse_all`, `toggle_show_passed`, `toggle_show_ignored`, `step_failed`, `jump_to_source`, `run_command`.
- **Keys:** up, down, home, end, page up/down, left, right, enter, F4, ⌘↓; double-click jumps to source.
- **Filters:** stored under `openidea.run.testFilters`; passed tests hidden and ignored shown by default.
- **20,000 tests:** the tree index and counts are rebuilt only when the results version, the session or the filters change (at most once per 50 ms console flush). Rows are rebuilt only when the expansion changes. `uniform_list` draws only the rows in view.

## Coverage

**Covered, in your priority order:** the tree with statuses, counts and duration; selection and the selected test's output; keys and jump to source; filters and Expand/Collapse All; Previous/Next Failed Test; Rerun Failed (model plus the end-to-end test); Show Diff; the gutter states (recorded by the model, read through a hunk).

**Not covered:**

- multi-selection and speed search
- the plugin framework's asynchronous Run/Debug selection
- .NET and JVM report files read at exit
- decoding machine-readable stdout (go `-json`, TeamCity, Ruby reporter) for the console
- a spinner for running tests: none exists in the crate, so running shows `test-pending`
- zig: there is no native zig parser

**Spec cases:** `TestTreeView.test.tsx` 10 of 11; the missing one is the asynchronous "enables Run and Debug only when the async selector returns support".

**End-to-end test:** a temp-dir shell script named `cargo` stands in for the toolchain. The test runs `run.run`, then asserts:

- the rows `src/lib.rs, tests, slow, subtracts`
- the header `1 failing, 2 passed, 1 ignored, 10 ms`
- the tab icon `error`
- the selected test's output in the console
- Enter opens `src/lib.rs` at (19, 8)
- the gutter shows red and green run marks
- Rerun Failed runs `test -- tests::subtracts --exact` in the same tab and keeps the original configuration

## Changes existing code needs (hunks in the notes)

- `RunStore::take_final_lines`, because the exit notification's failed-test count is computed before the last lines are known.
- A `RunDriver::tests()` entity, started when a process starts (from the session's configuration), fed in `flushed`, finished in `finish`, and dropped on tab close; `failed_tests` then comes from it.
- In `RunView`: a `test_tree()` accessor, the tree drawn beside the console (32% width, at least 140px), the console switched to the selected test's output, a `run-rerun-failed` toolbar button, and a `RunViewEvent::RerunFailed` variant.
- Handlers for `run.rerunFailed`, `run.expandAllTests` and `run.collapseAllTests`; the Run tool window added to `go_to_occurrence`; the `testTreeFocus` and `run.canRerunFailed` context keys.
- `editor_gutter_run_marks` takes `cx` and reads the recorded states.
- A `focus` parameter on `RunDriver::open_location`, which always focuses the editor today.

No keymap change is needed: the generated keymaps already carry these actions' default shortcuts.

## Hypotheses still open (each with its settling check in the notes)

- Completed console lines reach the parser without ANSI escapes.
- The cargo parser's paths have the shapes the tests assume, with and without a `Running` header.
- `ConsoleModel::append` followed by `flush` gives the lines the selected-output console needs.
- Rust producer targets have paths like `["tests", "adds"]`, which the gutter assertion relies on.
- How the `testTreeFocus` context key is set.
- Whether the native Run menu is built from `actions.json` categories.

## Tests most likely to fail first

- Row clicks, if `uniform_list` rows aren't laid out when the test clicks them.
- Key tests, if a parent takes focus from the tree.
- The two-window filters test, if opening a second window in one test conflicts with the first.
- The end-to-end test, since it depends on every hunk.

## Parity rows this would raise

- The seven `unit:src/features/run/testing/*.test.ts` rows saying the parser is "never called". dotnet and plugin stay partial; go, phpunit and ruby keep raw console output.
- `rerun.test.ts` and `states.test.ts`: the "model only" clause.
- `point.registerToolWindow:run`: "no test tree".
- `feature:run`: "the test tree" and the gutter markers.
- `e2e:e2e/run.spec.ts`: Rerun Failed.
- `action:navigate.next/prevOccurrence`: Run joins the tool windows that step.
- Rows to add, since none exist: `unit:src/features/run/TestTreeView.test.tsx`, `action:run.rerunFailed`, `action:run.expandAllTests`, `action:run.collapseAllTests`, `context:testTreeFocus`, `context:run.canRerunFailed`.

## Where the native code contradicts the prompt

- **Sort:** the webview test tree has none.
- **"Click to see difference":** this link doesn't exist; the comparison is the context menu's "Show Diff".
- **Counts in the tab:** the tab shows none; the counts are in the tree header, and the tab only shows the failed-exit icon.
- **"Called by nothing":** not quite; `driver.rs:31` uses `with_test_reporting`.
- **Lost output:** the driver drops the flushed and final output lines and hardcodes `failed_tests: 0`.
- **Gutter state:** the gutter passes no state at all.
- **Two `TestStates` types:** there are two, in `run_testing/states.rs` and `run_producers/context_run.rs`. The gutter's icon function uses the second, so the draft does too.
- **Rerun Failed shortcut:** it has no IntelliJ default; only Eclipse binds it (Alt+Shift+G).
- **Rerun Failed with nothing to rerun:** I dropped the end-to-end test for this. The action's `when` clause refuses it when there are no failures, so the "No failed tests to rerun" notification can't be reached.
