I wrote 15 new tests in three files and the notes file, all as new files with no existing file touched. Nothing has been compiled or run; the three files only pass the `rustfmt --check` parse.

**Files written:**

- `app-gpui/src/workbench/build_tools/tests/proofs21_execute_popup.rs`: 294 lines, 9 tests.
- `app-gpui/src/workbench/build_tools/tests/proofs21_build_windows.rs`: 166 lines, 3 tests. It also has a small helper for "files removed" that the popup tests import.
- `app-gpui/src/workbench/tests/proofs21_shell.rs`: 95 lines, 3 tests.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs21.md`: the notes.

**`mod` lines to add:**

- In `build_tools.rs`, after `    mod proofs20_build_title;`: `mod proofs21_build_windows;` and `mod proofs21_execute_popup;`.
- In `workbench/mod.rs`, after `    mod proofs20_shell;`: `mod proofs21_shell;`.

No accessor hunks are needed. I re-read `build_tools.rs` at the end and its anchors are unchanged.

| Row                            | Tests                                                                                                                                                | State after they pass                                                                     |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `unit:…/ExecutePopup.test.tsx` | the 9 spec sentences, driven by real `tab`, `pageup`/`pagedown`, a click on the drawn heading, and clicks on the project names in the popup's footer | done                                                                                      |
| `unit:…/actions.test.tsx`      | `registers_the_maven_tool_window_only_while_the_project_has_a_pom` (POM written then removed; title, icon, right anchor, stripe button)              | partial, 3 of 4 (the action list still lacks Add Maven Projects and Analyze Dependencies) |
| `unit:…/linkage.test.tsx`      | `routes_reload_to_the_chosen_independent_build`, plus `a_build_picked_after_the_window_changed_projects_is_linked_nowhere`                           | partial, 5 of 16                                                                          |
| `ipc:app_info`                 | `the_welcome_screen_shows_the_builds_version_and_about_offers_the_configuration_folder`, `the_title_bar_takes_the_frame_of_this_platforms_windows`   | partial                                                                                   |
| `ipc:window_start_drag`        | `the_title_bars_empty_space_is_left_to_the_system_to_move_the_window`                                                                                | partial                                                                                   |
| `ipc:user_font_data`           | none                                                                                                                                                 | stays blank                                                                               |

Where a spec literal can't hold in a real Maven window, the tests take the value from the drawn list instead. The `"site-deploy "` line is replaced by the last drawn completion. Enter on `-Dunknown` runs the line here, because there is a Maven project to run it in.

**Cases left out or not test-only:**

- **Late link result (linkage):** the app does drop a link answer that arrives after the project changed, but no test can hold the link call open to make the answer late. What a test can drive is a project change while the file picker is open. There the app does not match the webview: from reading the code, it goes on and writes the first project's build into the new project's settings. The test above demands the webview's behaviour and **will fail** until the 4-line hunk in the notes adds that check after the picker.
- **`user_font_data`:** test builds panic on purpose when asked for the system's fonts, and nothing under the home folder may be used. Only a real-window check can prove it: apply a font that exists only in `~/Library/Fonts`.
- **`window_start_drag`:** on macOS GPUI ignores the title bar's drag area entirely, and the test platform never hit-tests it. The move and the double-click zoom come from AppKit. The test only checks the window settings that leave the drag to AppKit and that gestures on the empty space open nothing. Not enough to close the row.
- **Show Config:** the test only sees the link drawn, because clicking it would crash the test platform (its "reveal path" is unimplemented). The Windows terminal setting and Wayland resize handles from `app_info` are also untested.

**Riskiest three:**

1. `a_build_picked_after_the_window_changed_projects_is_linked_nowhere`: fails by design until the fix. Before the fix it can rarely pass by accident, if two writes of the same setting race.
2. Tab after a mouse press inside the popup, in the section-click test and the Gradle build-change test. If the press takes focus from the command line, that is a real app bug to fix in the popup, not in the test. The section-click test also waits for the list to scroll back to its top after the rows shrink.
3. `registers_the_maven_tool_window_only_while_the_project_has_a_pom`: it removes a tool window while it is showing, and it assumes `stripe:maven` is drawn on the right at the default test window size.
