All four pages landed whole: Project Trust, TODO, Log and GitHub. Each is compiled, declared and tested through File › Settings… in a project window. Everything is uncommitted and nothing was pushed. All gates pass, but the first loop caught one unstable test. I fixed its wait and ran both whole-suite runs and the twenty runs again; the numbers below are from those reruns.

## 1. Gates (measured)

Logs are in `$S/dset4i/`.

| Gate                                        | Log                                              | Result                                                                                                                                        |
| ------------------------------------------- | ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1, nothing else running           | `suite3.log`                                     | `test result: ok. 8001 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 206.28s` (uptime: load 52.45 before, 61.57 after) |
| Suite run 2                                 | `suite4.log`                                     | `test result: ok. 8001 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 229.25s` (load 52.25 before, 53.87 after)         |
| Twenty runs of my 28 tests, during run 2    | `loop_summary.log`, `loop_1.log` … `loop_20.log` | 20 of 20 runs passed: `test result: ok. 28 passed; 0 failed` each time                                                                        |
| `cargo test --test web_view`                | `web_view.log`                                   | `test result: ok. 1 passed; 0 failed`                                                                                                         |
| `cargo clippy --all-targets -- -D warnings` | `clippy.log`                                     | exit 0                                                                                                                                        |
| `cargo build`                               | `build3.log`                                     | 0 warnings, exit 0                                                                                                                            |

The load was already above 50 before both runs. Nothing of mine was running then, so it came from other processes on the machine.

**The failure the first loop found** (`loop_first/loop_7.log`, run 7 of 20). The two earlier whole-suite runs, `suite1.log` and `suite2.log`, passed 8001 of 8001 with the old code.

- Test: `workbench::tests::proofs31_settings_feature_pages::a_missing_folder_or_a_file_is_not_trusted_and_the_page_says_why`
- Failure, whole: `panicked at src/workbench/tests/proofs31_settings_feature_pages.rs:285:5: assertion 'left == right' failed / left: "IpcError: No such file or directory (os error 2)" / right: "IpcError: trusted location must be an existing directory"`
- Cause: the wait for the second error looked for the word "directory". The first error contains that word too, so the wait ended before the second request had finished.
- Fix: the wait now looks for "must be an existing directory". The assertion is unchanged. I checked that the looped binary was built after the fix (it contains the new string).

**Process check:** HEAD moved from `011d3094` to `4c16b6f8` while I worked. `git diff --stat 011d3094 HEAD -- app-gpui/` prints nothing. No cargo, test or app process of mine is left (`ps` checked). The temp project `/private/tmp/openidea-inspect-6jb5dcsz` is removed.

## 2. The pages

### How the Settings dialog treats these pages in the webview

None of the four pages stages anything; each writes its value the moment it changes.

- **Modified mark:** a page is marked only if it has staged changes (`SettingsDialog.tsx:236-242`), so these pages are never marked.
- **Reset:** shown only for a page with changes (`SettingsDialog.tsx:158`), so it never appears here.
- **Apply:** off unless something is staged (`SettingsDialog.tsx:362-363`).
- **Cancel:** runs `closeSettings` (`SettingsDialog.tsx:360`), which only clears what was staged (`staging.ts:194-205`). A change made on these pages stays.

Native, measured: after a change on each page, `can_apply` is false, `reset_visible` is false and `modified_pages` is empty, and the change is still there after Cancel. This is the helper `assert_nothing_staged` in the trust, TODO, Log and GitHub tests. The real window also showed Apply greyed out.

### Project Trust (`src/features/project-trust/index.tsx:124-192`, registered at `:247`)

**What the webview does**

- It reads the trust decisions when the page is mounted (`:127-131`). The page is mounted again each time it is shown.
- Revoke Trust and Remove run their actions, then read the decisions again (`:134-141`). A failure there is only logged, because `executeAction` catches it (`actions.ts:203-206`).
- Add folder… opens a folder chooser, adds the location, then reads again (`:142-153`).
- A failure is shown as `String(e)`. For a backend error that is "IpcError: <message>" (`ipc.ts:15-20`).

**What the native page now does (my changes to the draft)**

- It reads the store again each time the dialog shows the page, which clears an earlier failure.
- A failed Revoke or Remove is logged, as in the webview. A failed read or Add folder… is shown as "IpcError: <message>".

**Revoke Trust on the window's own project:** the project goes into Restricted Mode at once and the dialog stays open. After Cancel, Run starts nothing and reports "Error running 'touch'" with "Restricted Mode" in its text (the existing assertion).

**Tests** (each under `workbench::tests::proofs31_settings_feature_pages::` unless named in full):

| Test                                                                                                                                                                                        | Failed without its code?                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `revoke_trust_on_the_project_trust_page_restricts_the_project_at_once`                                                                                                                      | Yes, with the page's subscription removed  |
| `adds_a_picked_folder_as_a_trusted_location_from_settings` (also asserts the trust file is under the test's temp home)                                                                      | Yes, subscription removed                  |
| `add_folder_trusts_the_undecided_project_open_inside_it` (a subfolder is covered, the folder above it is not)                                                                               | Yes, with the refresh after adding removed |
| `remove_on_the_project_trust_page_restricts_the_projects_inside_the_location` (two locations, exactly one taken out)                                                                        | Yes, subscription removed                  |
| `a_missing_folder_or_a_file_is_not_trusted_and_the_page_says_why` (shows "IpcError: No such file or directory (os error 2)" and "IpcError: trusted location must be an existing directory") | Yes, subscription removed                  |
| `a_folder_chosen_through_a_link_is_trusted_as_the_folder_it_links_to`                                                                                                                       | Yes, subscription removed                  |
| `the_project_trust_page_reads_the_store_again_when_shown_again`                                                                                                                             | Yes, with the shown-again event removed    |
| `workbench::app_actions::tests::shows_and_trusts_the_canonical_folder_when_opening_through_a_symlink`                                                                                       | Not mutation-checked                       |
| `settings_view::pages::security_project_trust::tests` (2 view tests)                                                                                                                        | Not mutation-checked                       |

**Left out:** the chooser's title "Add Trusted Location". GPUI's chooser options only set the button label (`gpui-pre-macos platform.rs:911-912` calls `setPrompt`), so there is no title to set.

### TODO (`TodoSettingsPage.tsx`, registered at `index.ts:12`)

**What the webview does**

- Every change is stored at once (`store.ts:27-34`).
- Other windows pick it up from storage (`store.ts:36-43`).
- Changed patterns or options rescan the project (`store.ts:298-307`).
- The TODO window's Match Case writes the same setting (`TodoView.tsx:336`).

**The defect was real.** Before the change, the native TODO window always started from the defaults and never read what the page stored. `the_todo_window_matches_with_the_stored_patterns_when_first_shown` checks this.

**What I changed**

- The four storage items moved from the settings page into `features_a/todo.rs`, renamed `TODO_SETTINGS_KEY`, `parse_todo_settings`, `todo_settings_json` and `stored_todo_settings`. The window no longer imports from a settings page.
- `todo_window.rs`:
  - The window loads the stored settings when it is first shown.
  - It follows the storage, so every window's TODO window rescans when any window's page changes something.
  - Match Case stores the setting.
- The TODO page itself also follows the storage, so a change made elsewhere shows on an open page.
- Restore Defaults is now drawn at its own width; the real window showed it stretched across the page.

**The pattern is not a regular expression.** Both apps escape the keyword before matching (`model.ts:49`, `escape_re` and `regex::escape`), so there is no invalid-pattern case and nothing to report. The new test `features_a::todo::tests::a_keyword_is_matched_as_written_never_as_an_expression` checks that `FIX.ME` and `a(b[` are matched literally.

**Where the scans run (read in the code, not timed):**

- The project scan runs on the backend.
- The scan of open editors (`scan_editor_text`) runs on the window's thread, for text up to 2 MB, with the `regex` crate, whose matching time grows linearly with the text.
- Each settings change, including each keystroke in a keyword field, rescans every open editor on that thread and restarts the project scan.

Hypothesis: with several large editors open, typing a keyword could stall the window. Check by timing `scan_open_editors` with a 2 MB editor open.

**Tests:**

| Test                                                                                                                                       | Failed without its code?                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- |
| `the_todo_window_matches_with_the_stored_patterns_when_first_shown`                                                                        | Yes, see note below                                                              |
| `todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them`                                                       | Yes, see note below                                                              |
| `match_case_in_the_todo_window_is_the_stored_case_sensitive_option`                                                                        | Yes, see note below                                                              |
| `the_todo_window_of_another_window_follows_the_page_at_once`                                                                               | Yes, see note below; also failed with only the page's storage-following disabled |
| `settings_view::pages::editor_todo::tests` (3) and `features_a::todo::tests::stored_settings_read_back_and_bad_values_take_their_defaults` | Not mutation-checked                                                             |

Note on the first four rows: I took the three `todo_window.rs` changes out together, not one at a time.

### Log (`LogSettingsPage.tsx:28`, registered at `vcs/index.ts:166`)

**What the webview does:** a checkbox calls `setLogOptions`, which stores the options at once (`logOptions.ts:178-190`). Other windows reload them from storage (`:129-133`).

**What the native page does:** a checkbox stores the options and sets them on the open Git log through the existing `set_log_options`.

**Tests:**

- `edits_the_logs_display_options` and `every_log_page_option_is_stored_under_its_key_and_read_back_by_the_log` failed with the page handler emptied. The second covers all nine options under `openidea.vcs.logOptions`, and checks that a newly created `GitLog` reads all nine back.
- `settings_view::pages::vcs_log::tests` (1) was not mutation-checked.

**Not covered:**

- Nothing native draws `refsOnLeft` or `showGitHubChecks`; my search found no reader outside the parser.
- An open Git log in another window does not follow a change; the webview's does.

### GitHub (`GithubSettingsPage.tsx:13-17`)

The webview's page only stores the option. The native page does the same. Nothing native reads `autoMarkViewed` yet; my search found it only in `github/diff.rs` and its tests.

**Tests:**

- `updates_an_open_page_when_another_window_saves_or_clears_review_settings` and `ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings` failed with the handler emptied.
- `registers_independently_of_the_pull_request_runtime` still passed with the page's subscription and search labels removed. Hypothesis: it would fail without the page's view being mounted at all; not run.

**Shared:** `the_settings_search_finds_each_page_by_a_label_on_it` failed without `set_page_labels`. "Automatically mark opened files" selects the Keymap page first, so the test asserts that the GitHub page stays in the filtered tree rather than that it gets selected.

## 3. Rows

I did not write to `parity.json`. The `settings:<id>` key form matches `settings:appearance`, `settings:keymap` and `settings:build.debugger`.

| Key                                                 | State             | Proving tests (full names)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:projectTrust`                             | done              | `workbench::tests::proofs31_settings_feature_pages::{revoke_trust_on_the_project_trust_page_restricts_the_project_at_once, adds_a_picked_folder_as_a_trusted_location_from_settings, add_folder_trusts_the_undecided_project_open_inside_it, remove_on_the_project_trust_page_restricts_the_projects_inside_the_location, a_missing_folder_or_a_file_is_not_trusted_and_the_page_says_why, a_folder_chosen_through_a_link_is_trusted_as_the_folder_it_links_to, the_project_trust_page_reads_the_store_again_when_shown_again}`. The page applies at once: Apply stays off and Cancel keeps the decision. |
| `settings:editor.todo`                              | done              | `…::{the_todo_window_matches_with_the_stored_patterns_when_first_shown, todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them, match_case_in_the_todo_window_is_the_stored_case_sensitive_option, the_todo_window_of_another_window_follows_the_page_at_once}`                                                                                                                                                                                                                                                                                                               |
| `settings:vcs.log`                                  | partial           | `…::{edits_the_logs_display_options, every_log_page_option_is_stored_under_its_key_and_read_back_by_the_log}`. Missing: nothing native draws `refsOnLeft` or `showGitHubChecks`, and another window's open Git log does not follow a change.                                                                                                                                                                                                                                                                                                                                                              |
| `settings:vcs.github`                               | done              | `…::{registers_independently_of_the_pull_request_runtime, updates_an_open_page_when_another_window_saves_or_clears_review_settings, ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings}`. The page only stores the option, as the webview's does; nothing native reads it yet.                                                                                                                                                                                                                                                                                                    |
| `unit:src/features/github/settingsPages.test.tsx`   | done, 3 of 3      | The same three tests. The spec's "dispose unregisters" half has no native counterpart: native pages are a fixed list.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `unit:src/features/project-trust/trust.test.tsx`    | partial, 5 of 7   | Adds `workbench::app_actions::tests::shows_and_trusts_the_canonical_folder_when_opening_through_a_symlink` and `…::adds_a_picked_folder_as_a_trusted_location_from_settings` to the 3 already listed. Missing: a local opening's decision kept on the client, and an attached member whose trust is lost.                                                                                                                                                                                                                                                                                                 |
| `unit:src/features/vcs/__tests__/logPanes.test.tsx` | partial, 12 of 13 | Adds `…::edits_the_logs_display_options`. The Graph Options view is not built.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `ipc:project_trust_list`                            | done              | `…::revoke_trust_on_the_project_trust_page_restricts_the_project_at_once`, `…::the_project_trust_page_reads_the_store_again_when_shown_again`. The page lists what only `TrustStore::decisions` holds; the adapter is `app.rs:322`.                                                                                                                                                                                                                                                                                                                                                                       |
| `ipc:project_trust_add_location`                    | done              | `…::add_folder_trusts_the_undecided_project_open_inside_it`, `…::adds_a_picked_folder_as_a_trusted_location_from_settings`. The page calls `add_location` then `refresh_project_trust`, as the adapter does (`app.rs:343-346`).                                                                                                                                                                                                                                                                                                                                                                           |

`ipc:project_trust_remove_location` is already done; the remove test above is a second proof of it. `feature:project-trust`'s listed gap "the Project Trust settings page" is now built; that row stays partial for other reasons.

## 4. Hypotheses: which way each went

- **These pages apply at once:** confirmed for all four in the webview source and in the native tests.
- **The dialog closes after revoking the window's own project:** refuted. The dialog stays open, both by test and by reading the webview.
- **A missing folder fails to add:** confirmed. The text is "IpcError: No such file or directory (os error 2)".
- **The TODO window ignores the stored settings:** confirmed, and fixed.
- **An open TODO window elsewhere doesn't follow until shown again:** that was the draft's behaviour; the webview's does follow, so I fixed it.
- **A TODO pattern is a regular expression:** refuted; it is matched literally in both apps.
- **Every Log option is read by the native log:** refuted for two options (`refsOnLeft`, `showGitHubChecks`).
- **The Settings search finds the pages by their labels:** confirmed.
- **The draft's compile risks** (the `fn` pointers in the Log options table, `subscribe_in`, `.id().debug_selector()`): all compiled without changes.

## 5. Shots

In `$S/insp8/shots/`. The only paths the app showed were under `/private/tmp/openidea-inspect-6jb5dcsz/`.

| Shots                                                                           | What I saw                                                                                           |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `dset4i2-01-trust-light-1.png`, `dset4i2-08-trust-dark-1.png`                   | Project Trust with the one trusted temp project and Revoke Trust; no locations; Apply greyed.        |
| `dset4i2-02/07-todo-*`                                                          | TODO page: options, four patterns with swatches and colour lists, Restore Defaults at its own width. |
| `dset4i2-03/06-log-*`                                                           | Log page: five options and the four columns, indented under "Visible columns:".                      |
| `dset4i2-04/05-github-*`                                                        | GitHub page: the single checkbox.                                                                    |
| `dset4i3-01-todo-added-colours-light-1.png`, `dset4i3-02-…-dark-1.png`          | A NOTE pattern added in green, with its colour list open over the page.                              |
| `dset4i5-02-trust-location-light-1.png`, `dset4i5-03-trust-location-dark-1.png` | One trusted project and one trusted location (the temp folder), each with its button.                |
| `dset4i4-01-trust-question-1.png`, `dset4i5-01-after-check-1.png`               | The trust question for the temp project.                                                             |

Two small things I saw and left as they are:

- On the Project Trust page, the "Trusted projects" heading has a rule under it and "Trusted locations" does not; in the webview both are plain `h3` headings.
- The trust question's "Trust projects in…" checkbox label is cut off. That dialog is existing code, not part of these pages.

I deleted my first set of shots (`dset4i1-*`) because the script captured them one step late, so their names were wrong.

## 6. Files I edited

- `app-gpui/src/workbench/settings_feature_pages.rs` (draft handed to me)
- `app-gpui/src/workbench/tests/proofs31_settings_feature_pages.rs` (draft handed to me)
- `app-gpui/src/settings_view/pages/security_project_trust.rs` (draft handed to me)
- `app-gpui/src/settings_view/pages/editor_todo.rs` (draft handed to me)
- `app-gpui/src/settings_view/pages/mod.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/settings_window.rs`
- `app-gpui/src/workbench/todo_window.rs`
- `app-gpui/src/features_a/todo.rs`
- `app-gpui/src/workbench/app_actions.rs`

`vcs_log.rs` and `vcs_github.rs` are now declared and were formatted, but their content is unchanged.

## 7. Wrong or stale in the notes and briefs

- The notes' TODO parser says it matches the webview. It does not: the webview spreads the stored JSON over the defaults without checking each field's type. The native parser is stricter, which I kept.
- The notes say the page must have a separate Remove Pattern test only in the view test; it is now also covered end to end, in the TODO patterns test.
