All 11 Settings pages are now declared, wired into the dialog and passing their tests: 7 are done and 4 are partial (Updates, HTTP Proxy, Gradle, Maven). The gates pass, with one failure seen once in a test this work does not touch. The language pages are not declared.

## 1. Gates (from `app-gpui/`, output under `$S/dseta/`)

| Gate                                                                                                                                              | Result                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1, nothing else running (`suite1.txt`)                                                                                                  | ok, 7458 passed, 0 failed, 0 ignored, 175.23 s                                                                                |
| Suite run 2, with the 20× loop beside it (`suite2.txt`)                                                                                           | FAILED: 7457 passed, 1 failed, 196.32 s                                                                                       |
| Suite run 3 (`suite3.txt`)                                                                                                                        | ok, 7458 passed, 177.33 s                                                                                                     |
| Suite run 4, after the last test edits (`suite4.txt`)                                                                                             | ok, 7459 passed, 175.24 s                                                                                                     |
| 20× loop of every test added, declared or changed (filters `proofs19_settings_misc settings_view:: smoke_settings_pages settings_model::staging`) | 20/20 green during run 2 (154 tests); 20/20 again after the last edits (`loop2/`, 155 tests). No test failed in any loop run. |
| `cargo test --test web_view`                                                                                                                      | ok, 1 passed (run twice)                                                                                                      |
| `cargo clippy --all-targets -- -D warnings`                                                                                                       | exit 0                                                                                                                        |
| `cargo build`                                                                                                                                     | exit 0, 0 warnings                                                                                                            |

- **The one failure:** `workbench::editor_extras::tests::next_occurrence_steps_through_the_results_of_the_tool_window` failed in run 2 at `editor_extras.rs:1115`: "the caret in b at 0:13 did not happen within thirty seconds … reached later: None". I did not touch this file.
  - It then passed 20/20 alone (`occ/run_*`) and 15/15 while suite run 3 ran beside it (`occ/load_*`).
  - None of the older scratch logs show it failing before.
  - Hypothesis: under load from the concurrent loop, highlighted usages appear before `ctrl-alt-down`, which then steps to a usage instead of the next occurrence. To confirm: loop it during a full suite plus a second test loop, and log `f.at(cx)` before the keystroke.

Earlier failures, all fixed:

- The checker's answer panicked by updating the proxy view inside its own update. `run_check` now defers it.
- Three draft test expectations were wrong:
  - A port is staged as it is typed (7000 before 70000), as in the webview's NumberInput.
  - A table emptied back to the stored `[]` stages nothing.
  - "rel" also matches MinSizeRel.
- The `files.exclude` default is the backend's (`**/.git`, `**/node_modules`, `**/target`), not empty.
- The test build keeps saved passwords in `proxy-password.json.vault`, not `proxy-password.json`.
- File Types: Python and the ignored-names + button are below the visible list or page. The test now filters, and a `bring_into_view` scroll helper was added.

## 2. Per part of the brief

**Declaration and wiring**

- The 13 draft modules are declared in `pages/mod.rs`, and `proofs19_settings_misc` in `workbench/mod.rs`.
- The 11 pages are installed in `settings_view/custom_controls.rs`. CMake, Gradle and Maven get a `Slots::page_view`.
- The existing `SettingsDialogView::page_view` getter is reused; no second getter was added.
- Search Everywhere: the page specs already existed in `builtin_pages`, so `settings_search_entries` lists them without change.

**Smoke test** (`smoke_settings_pages.rs`, changed once)

- A page with nothing for it to click now passes only if every line is a `—` header or ends `: <custom>`, with at least one such line. The existing "unavailable" and "page view" conditions are unchanged.

**No secret in `Debug`** (`settings_model/staging.rs`)

- `StagedChange` and `SettingsDialog` now print with `proxy.password` hidden, including inside `base` / `global_base`. `ApplyRun` holds `StagedChange`, so it is covered too.
- `ProxyForm` got a `Debug` that hides the password.
- The proxy e2e test checks, after typing and after Apply, that the password text is absent from:
  - the lines and the staged changes' `{:?}`;
  - the form's `{:?}` and `form.settings()`;
  - the dialog model's `{:?}` (after asserting `base.proxy.password` really holds it);
  - `settings.toml` and the log.
- The only file holding it is `proxy-password.json.vault`.
- Hypothesis: the password field's text is never copied. Not measured; check with a cmd-c test on `SecretField`.

**Check connection**

- `src-tauri/src/commands/http.rs:84` `proxy_check_connection` calls `ide_http::client::check_connection`. `general_http_proxy::backend_checker` calls the same function on the backend runtime.
- I added `ide-http` as a path dependency. `Cargo.lock` gains only the line `"ide-http"` in openidea-gpui's dependency list; no new package (diffed against a copy taken before).
- The test build has no checker unless a test sets `ConnectionCheck` (test `without_a_checker_given_the_test_build_has_none`). No test makes a request.

**Toolchain seam**

- `build_sdk_field::detector` returns nothing in the test build without `SdkDetection` (test `a_test_build_without_a_detection_of_its_own_finds_nothing`).

**Updates**

- The webview registers `app.checkForUpdates` only when the build has an updater endpoint. Its handler and `app_update_status` are Tauri-only (`src-tauri/src/commands/window.rs:453,490`), so the native page stays hidden.
- No stub was added to production code.

**Issue Navigation**

- Patterns are checked with the `regex` crate, which commit-message linking also uses (`vcs_more/issue_links.rs`).
- Measured (`look_around_and_backreferences_are_invalid_patterns`): `(?=…)`, `(?<=…)`, `(?!…)` and `\1` are flagged invalid; `(?<name>…)` is accepted.
- Hypothesis: JavaScript's RegExp accepts all four.

**Changes made against the webview source**

- Date Formats (`behavior.tsx:476`): a blank pattern now snaps back to the stored one, where the draft flagged it.
- Proxy (`httpProxy.tsx:129`, NumberInput `Form.tsx:155`): Up/Down step the port.
- Paths and thread count are stored trimmed (`LanguagePages.tsx:47,616`), via a new `FieldKind::Trimmed`.
- Path Browse is now a folder icon with a "Browse…" tooltip; Detect again has its tooltip.
- CMake (`cmake.tsx:118`): Build type and Generator suggestions added. CMake and External Tools toolbar tooltips added.
- Actions on Save (`tools.tsx:343`): the trailing "." after the link. `Part::Link` gained `after`, and the sentence now wraps (it was cut off in the window).
- Directory Mappings: full width, root shown in a tooltip.
- Proxy: the Check connection button no longer stretches across the page.

**Real window** (final binary; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR` and project; stopped by PID)

- Ten pages shot, in `$S/dseta/insp/shots/f-*.png`.
- Each was looked at in this window: nothing over another control, labels end in one colon, every page fits or scrolls.
- What was cut and fixed: the Actions on Save note, the stretched Check connection button and the narrow mappings table.
- Directory Mappings cuts a long root path at the edge, as the webview's ellipsis cell does; the full path is in the tooltip.
- The Maven page is taller than the dialog; the Maven e2e test now scrolls to Maven Daemon home path.
- No button was pressed. Drawing Gradle and Maven runs the backend's SDK detection, as the webview does on mount. Hypothesis from reading `ide-sdk/src/discover.rs` and `java.rs`: it reads PATH (`which mvn`), system folders and the temp HOME's folders, and runs no program.

## 3. Rows

R&C below is `reset_and_cancel_discard_an_edit_on_each_kind_of_page`; P is `workbench::tests::proofs19_settings_misc`.

| Row                            | State                                                    | Proving tests                                                                                                 | Missing                                                                                                                                                          |
| ------------------------------ | -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:general.dateFormats` | done                                                     | P::date_formats_override_and_pattern_apply, R&C, `general_date_formats::tests::*`                             | —                                                                                                                                                                |
| `settings:general.httpProxy`   | partial                                                  | P::http_proxy_manual_configuration_applies_and_the_password_stays_secret, R&C, `general_http_proxy::tests::*` | Hypothesis: the real checker works. It is compiled but never run; check by a manual Check connection in the app. The URL prompt is an inline panel, not a popup. |
| `settings:general.updates`     | partial                                                  | P::updates_status_check_now_and_the_automatic_check_apply, R&C                                                | Page hidden in the app: no native updater action or status source.                                                                                               |
| `settings:editor.files`        | done                                                     | P::file_types_patterns_and_ignored_names_are_written_to_the_project, R&C, `editor_files::tests::*`            | —                                                                                                                                                                |
| `settings:build.cmake`         | done                                                     | P::cmake_profile_settings_are_written_to_the_project, R&C, `build_cmake::tests::*`                            | —                                                                                                                                                                |
| `settings:build.gradle`        | partial                                                  | P::gradle_jvm_distribution_and_home_are_written_to_the_project, R&C, `build_gradle::tests::*`                 | Download JDK… (excluded by the brief).                                                                                                                           |
| `settings:build.maven`         | partial                                                  | P::maven_home_and_thread_count_are_written_to_the_project, R&C, `build_maven::tests::*`                       | Download JDK….                                                                                                                                                   |
| `settings:tools.actionsOnSave` | done                                                     | P::actions_on_save_are_written_to_the_project, R&C, `tools_actions_on_save::tests::*`                         | —                                                                                                                                                                |
| `settings:tools.externalTools` | done                                                     | P::external_tools_a_tool_added_applies, R&C, `tools_external_tools::tests::*`                                 | —                                                                                                                                                                |
| `settings:vcs.mappings`        | done (read-only page, nothing to apply, reset or cancel) | P::directory_mappings_show_the_project_root, P::directory_mappings_map_a_repository_root_to_git               | —                                                                                                                                                                |
| `settings:vcs.issueNavigation` | done                                                     | P::issue_navigation_links_are_written_to_the_project, R&C, `vcs_issue_navigation::tests::*`                   | —                                                                                                                                                                |

No IPC row is claimed. `sdk_detect` and `proxy_check_connection` are proven only through fakes, and the File Types list comes from `host.languages.all()`, the same source as the `languages` command.

## 4. Unmeasured

Everything marked "Hypothesis:" above.

## 5. False or stale in notes and briefs

- The misc notes say Check connection needs a new dependency and the row stays partial; the dependency adds no lock package and is now wired.
- The notes list the blank date pattern being flagged and kept as matching the page; the webview snaps it back.
- They expect the port not staged while typing "70000"; the webview stages each valid prefix.
- The `build_sdk_field.rs` doc comment said detection "may run `java_home`"; reading the code found no execution, so the comment is corrected.
- `docs/gpui-port/notes/reports/int_settings_b_brief.md` shows a 4-line diff ("Part 0: proofs21") that is not mine; I did not touch `docs/`.
- For integration B: `Part::Link` gained an `after` field and pages `FieldKind` gained `Trimmed`. A read-only grep of `languages_*.rs` finds no `Part::Link` and only `FieldKind::Text`, so nothing there breaks. Its `.browse(true)` at `languages_pages.rs:557` will now draw the folder icon.

Nothing was committed.
