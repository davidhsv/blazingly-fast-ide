I finished four of the five pages: Project Trust, TODO, Log and GitHub are whole, and Project Structure is not started (no file written). Nothing was compiled or run, and I ran nothing that changes git. All new files are untracked.

**Files written**

- `app-gpui/src/settings_view/pages/security_project_trust.rs` (260 lines)
- `app-gpui/src/settings_view/pages/editor_todo.rs` (548 lines)
- `app-gpui/src/settings_view/pages/vcs_log.rs` (214 lines)
- `app-gpui/src/settings_view/pages/vcs_github.rs` (140 lines)
- `app-gpui/src/workbench/settings_feature_pages.rs` (247 lines): puts the four views on their pages and carries out what they ask.
- `app-gpui/src/workbench/tests/proofs31_settings_feature_pages.rs` (378 lines, 10 end-to-end tests)
- Notes: `$S/drafts/settings_pages4_integration.md` (303 lines). It has the hunks with exact anchors, grouped per page, plus behaviours with `file:line`, hypotheses, likely failures, the real-window checklist and what to share.

**Hunks to existing files**

- `workbench/mod.rs`: two `mod` lines (the new module and the test file).
- `settings_window.rs`: one line calling the new mount function.
- `settings_view/pages/mod.rs`: four `pub mod` lines.
- `todo_window.rs`, three changes. Today the TODO window always starts from the default patterns and never reads `openidea.todo.v1`, so it needs:
  - to load the stored settings when it is first shown;
  - a new `set_todo_settings` that stores the change and rescans;
  - its own Match Case button to store too, as the webview does at `TodoView.tsx:336`.

**Rows** (none of the `settings:` keys or the two new `ipc:` keys exist yet in `parity.json`)

| Key                                                 | Expected          | What proves it                                                                                                                    |
| --------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `settings:projectTrust`                             | done              | 5 tests: Revoke Trust, Add folder…, Add folder on an undecided project's folder, Remove, a missing folder shows the error         |
| `settings:editor.todo`                              | done              | the end-to-end test that the TODO window follows each change, plus view tests                                                     |
| `settings:vcs.log`                                  | done              | `edits_the_logs_display_options`                                                                                                  |
| `settings:vcs.github`                               | done              | 3 tests, named after the 3 spec cases                                                                                             |
| `settings:projectStructure`                         | not started       | none                                                                                                                              |
| `unit:src/features/github/settingsPages.test.tsx`   | done, 3 of 3      | the 3 GitHub tests above                                                                                                          |
| `unit:src/features/project-trust/trust.test.tsx`    | partial, 4 of 7   | adds `adds_a_picked_folder_as_a_trusted_location_from_settings`; the symlink, client-side and lost-member cases are still missing |
| `unit:src/features/vcs/__tests__/logPanes.test.tsx` | partial, 12 of 13 | adds `edits_the_logs_display_options`; the Graph Options case is still missing                                                    |
| `ipc:project_trust_list`                            | done              | the Revoke Trust test lists the project only the trust store holds                                                                |
| `ipc:project_trust_add_location`                    | done              | an undecided project leaves Restricted Mode after Add folder…, which only the store's addition plus the trust refresh produce     |

The "done" rows are done only on the terms these pages work by: each applies at once, so the tests check the effect immediately and that Cancel keeps it.

**What I left out**

- **Project Structure:** there is no native view to build on. The component is 2023 lines with six sections, its own dialog and a Download JDK dialog. Finishing it properly was far more than this pass could do whole, so I wrote the existing native models to build on and the section outline into the notes, marked as hypothesis.
- **Accessibility labels:** "Enable {name}" and "{keyword} colour" have no native accessibility tree to go in.
- **Chooser title:** "Add Trusted Location" is left out, because GPUI's path prompt takes only a button label.
- **Escape:** it does not close the TODO colour list (the settings form's lists don't close on Escape either).
- **TODO across windows:** an open TODO window does not pick up another window's change until it is shown again.

**Hypotheses** (each with how to settle it in the notes)

- Hypothesis: closures in the `static` flag tables of `vcs_log.rs` coerce to `fn` pointers.
- Hypothesis: `fn(&mut Self, &mut Context<Self>)` accepts `Self::toggle_case_sensitive` in `editor_todo.rs`.
- Hypothesis: `row(..).id(..)` followed by `.debug_selector` and `.on_click` compiles in that order.
- Hypothesis: revoking the trust of the window's own project does not close the Settings dialog. If it does, the last lines of the Revoke test fail.
- Hypothesis: `set_page_labels` takes English labels and the search translates them.
- Hypothesis: the TODO item lines end with the texts the TODO test expects (`"TODO first"`, `"BUG: in code"`).
- Hypothesis: the Project Structure outline and its machine sources (build model, SDK detection, JDK download) — taken from names and line ranges, not a full reading.

**Secrets and machine sources**
None of the four pages holds a secret. I checked this by reading the code:

- Project Trust reads and writes the trust store, `project-trust.json` in the data folder (`trust.rs:29`). On first run without that file, the store is filled from the recent projects list.
- TODO, Log and GitHub read and write `ui-storage.json` in the config folder, under `openidea.todo.v1`, `openidea.vcs.logOptions` and `openidea.github.reviewDiff.v1`.
- No page reads SSH, Docker, WSL or detected SDKs, and none uses the keychain, `gh` or the network.

**What turned out wrong in the brief or the native code**

- **Project Structure view:** the brief allowed that one might exist and only the row be missing; none exists, as `parity.json:3404` already says.
- **GitHub "accounts":** the page has no accounts, only one checkbox (accounts belong to the `gh` CLI), so no stand-in for `gh` was needed.
- **Apply / Reset / Cancel criterion:** it does not fit any of the four pages. The webview applies each change at once (`index.tsx:157`, `store.ts:27-34`, `logOptions.ts:178-190`, `GithubSettingsPage.tsx:2-3`), so I tested that behaviour instead.
- **Trust gating:** none is needed. `project_trust_*` commands are allowed in Restricted Mode (`crates/ide-host/src/trust.rs:223`).
- **Native TODO window:** it ignores the stored settings entirely; the `todo_window.rs` hunks fix that.
- **GitHub option has no effect yet:** nothing native reads "Automatically mark opened files as viewed" outside the `github` tests, so storing it changes nothing until the pull request diff view exists.
- **Layering:** the TODO storage functions sit in my settings page only because `features_a/todo.rs` could not be edited. They should move there, since `todo_window.rs` would otherwise import from a settings page.
- **Strings:** none of these pages' strings are in `en.json`, but `zh.json` has them keyed by the English text, so the lookup works.
