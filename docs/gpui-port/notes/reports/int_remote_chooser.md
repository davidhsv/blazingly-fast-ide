## Report: the remote folder chooser on Settings › Build › Deployment, shared with Run Targets

Both parts landed. A puts the chooser on the Deployment page and B moves Run Targets onto the shared browser; the old Run Targets browser is gone. Every gate is green, and clippy and the build both finished after the last source edit (02:54:09; clippy finished 02:56:49, build 02:59:37). I edited no code after the coordinator's message. Nothing is committed.

### 1. Gates (all measured, logs in `$S/drfci/`)

- **Before any edit:** `cargo test --lib -- proofs28 proofs30` gave 30 passed, 0 failed (`before_tests.log`).
- **Whole suite, run 1 (`suite1.log`):** `test result: ok. 8378 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 224.84s`. Load averages were 3.10 4.36 7.13 before and 35.59 22.80 14.12 after.
- **Whole suite, run 2 (`suite2.log`):** `test result: ok. 8378 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 225.03s`. Load averages were 30.42 22.11 13.97 before and 33.92 29.79 19.02 after.
- **Twenty runs by filter** (`proofs37 remote_folder asks_for_the_ftp proofs30`, `repeat20.log`): 20 of 20 runs gave `41 passed; 0 failed`. No test failed even once.
- **`cargo test --test web_view`** (`webview.log`): 1 passed, 0 failed.
- **`cargo clippy --all-targets -- -D warnings`** (`clippy.log`): exit 0, no warnings.
- **`cargo build`** (`cbuild.log`): exit 0, 0 warnings.
- No failure was seen at any time, including in the first targeted runs (57 passed after A, 53 passed after B). None of the eight load-sensitive tests the brief names failed.
- Disk free before every heavy command was about 350 GB (`build.log`).
- Formatting was done only with `$S/fmt.sh` on the files from `git diff --name-only`, which are exactly the files I edited plus the drafts handed to me.

### 2. Parts

**A. The chooser on the Deployment page**

- **Hunks A1–A15 applied as noted:**
  - `settings_view/pages/mod.rs`
  - `build_deployment.rs`: modules, field, close hooks
  - `build_deployment/connection.rs`: `ssh_configs` made `pub(super)`, Root path's Browse…, the chooser's text and panel
  - `build_deployment/paths.rs`: a mapping's remote Browse… and the chooser under the table
  - `workbench/mod.rs`: the e2e test module
- **Draft files compiled and integrated:** `remote_folder_browser.rs`, `remote_folder_browser/panel.rs`, `remote_folder_browser/proofs37_remote_folder_browser.rs`, `build_deployment/remote_folder.rs`, `build_deployment/proofs37_deployment_chooser_view.rs`, `workbench/tests/proofs37_settings_deployment_chooser.rs`.
- **Changes beyond the notes, each following the webview source:**
  - **FTP (recheck §3):**
    - The FTP connection is titled `Choose Remote Folder` (`deployment.tsx:197`).
    - The password key is `ftp:<server id>` through a new `secret_key()` that uses `ide_ssh::ftp::secret_key` (`deployment.tsx:224`, `remoteSettings.ts:303-307`).
    - `reachable` asks an FTP or FTPS server for a non-blank host (`remoteSettings.ts:312-316`, used at `deployment.tsx:476` and `:548`).
    - The FTP rule is added in `settings_model/ssh_connection.rs` (`sshConnection.ts:15`).
    - FTP arms are added to the test matches.
  - **Auth lookup:** the SSH configuration is found by the connection's password key, not by `server.ssh_config` (`deployment.tsx:65`). FTP and local servers therefore get `openSsh`.
  - **Focus on open:** opening the chooser puts the keyboard in its path field, as the Dialog focuses the first focusable element of its body (`Dialog.tsx:287-312`).
  - **List keys:** while the list has the keyboard, Up, Down, Enter and Backspace are kept even when the list is empty or nothing is selected. Escape closes the chooser. In the webview the listbox owns Enter (`Dialog.tsx:210`, `:325`) and Escape closes only the topmost dialog (`:315-318`).
  - **One close path:** every close goes through `close_chooser`, which empties the password field first. The browser's `on_release` also empties it.
  - **Escape in the path field** closes only the chooser.

**B. Run Targets on the shared browser**

- **B1:** `build_run_targets/folder.rs` is replaced by the draft content, plus an `Ftp` arm that is refused like `Local`.
- **B2:** the four lines of `build_run_targets/proofs30_run_targets_view.rs` are changed as noted.
- **Styling:** Target rows now use `h-list-row`, `bg-tree-hover` and `bg-tree-selected`, the list is 260 px and the status line has 8 px padding. This follows `RunTargets.module.css:23-50`; the draft's 22 px rows did not.
- **Kept as before:** title, OK-then-Cancel order, selectors, a single listing, and `save=false` (`runTargets.tsx:98`).
- All existing proofs30 tests pass unchanged apart from the B2 lines.

**Tests added**, each passing in both suite runs and 20 of 20 filtered runs:

- `app-gpui/src/settings_view/pages/build_deployment/proofs37_deployment_chooser_view.rs`:
  - `the_chooser_closes_with_another_server_ssh_configuration_page_and_a_mapping_removed`
  - `an_ftp_servers_chooser_needs_its_host_and_hands_the_password_over_under_the_server`
  - `a_password_typed_in_the_chooser_is_emptied_when_it_closes`
  - The 4 tests from the draft.
- `app-gpui/src/workbench/tests/proofs37_settings_deployment_chooser.rs`:
  - `the_test_listing_refuses_anything_but_local_folders_under_the_server_folder`
  - `deployment_chooser_keys_walk_its_list_and_escape_closes_the_chooser_not_settings`
  - The 2 e2e tests from the draft.
- `app-gpui/src/settings_model/ssh_connection.rs`: `asks_for_the_ftp_password_whatever_the_authentication`.
- The draft's 9 browser tests and 1 unit test are unchanged, except that the 100,000-folder test now prints its timings.

**Safety points, one by one**

1. **Listing.**
   - With no `DeploymentFolders` set, a test build lists nothing: `browse_needs_a_listing_an_ssh_configuration_and_for_a_mapping_an_absolute_root_path`.
   - The e2e tests wrap the real `backend_list()`. The wrapper now also rejects `..` components: the draft's `Path::starts_with` accepted `<server>/../..`.
   - `the_test_listing_refuses_anything_but_local_folders_under_the_server_folder` checks that `~`, an empty path, the parent of the server folder, a `..` escape, a relative path, an SFTP connection and an FTP connection are all refused. Zero of them reach the backend stand-in; only the inside local path does.
2. **Password.**
   - Save starts unticked and is handed over as it stands: `save=false` in `a_password_refused_is_shown_under_the_problem_it_keeps`, `save=true` in `a_password_asked_for_is_typed_as_bullets_and_handed_over_with_save_as_ticked`, both with the stand-in setter.
   - Run Targets still hands `false`: proofs30 asserts `secret s1 typed save=false`.
   - The password is drawn as bullets only and never appears in `lines()`.
   - It is cleared when a new listing starts (`go()`) and when the chooser closes (`a_password_typed_in_the_chooser_is_emptied_when_it_closes`, which holds the field's handle).
   - Over FTP it is handed over as `ftp:s1` (FTP test).
   - None of the `Debug` types near the state can hold the secret: `Flavour` and `BrowseTarget` hold none, `FolderState::Failed` holds the error text, and `FolderBrowserEvent::Chosen` holds a path. The secret only lives in `SecretField` and `SecretText`, whose `Debug` hides it. I checked this by reading the types.
3. **Off the window's thread, and large folders.**
   - The listing runs through `on_backend`, the sort on `background_executor`. Late answers are dropped (`an_answer_to_a_listing_the_user_moved_on_from_is_dropped`).
   - With 100,000 entries in a debug build (measured): sort 26.13 ms; open-to-listed-and-drawn 16.98 ms (window, listing, background sort and first draw); a later frame 0.29 ms.
   - Only rows in view are drawn: `row-99999` has no bounds.
   - Hypothesis: the open-to-drawn time is shorter than the sort alone because the first sort warmed the collator. Confirm by swapping the two timings in the test.
4. **Inline, not modal.**
   - **What a user sees differently:** the webview's dialog blocks the whole Settings page behind it. Natively, the server list, the tabs, the Type drop-down, the root path field, Test Connection and Settings' Cancel/Apply/OK all stay clickable while the chooser is open.
   - The chooser scrolls with the page, and in a 1536×1079 window its OK sits below the fold.
   - It closes with another server, tab, page, type or SSH configuration, and with a mapping added or removed. Tests: `the_chooser_closes_with_another_tab_another_type_and_cancel`, `the_chooser_closes_with_another_server_ssh_configuration_page_and_a_mapping_removed`, `a_mappings_deployment_path_is_chosen_relative_to_the_root_path` (mapping added).
   - An edit that keeps the same connection, such as a rename, keeps it open (asserted).
5. **Keys.** Test: `deployment_chooser_keys_walk_its_list_and_escape_closes_the_chooser_not_settings`.
   - Escape in the path field closes the chooser and Settings stays open.
   - In the list, Down then Enter walks into `site`, Backspace goes back up, and Escape closes the chooser with Settings still open.
   - The real binary showed the same after Escape.
   - Not proven by any test: that keys in an empty list never reach Settings. The browser test only shows they list nothing.
6. **Existing tests:** proofs28 and proofs30 passed by filter before (30 tests) and after (in the 53-test run). The `Root path:` line and the mapping rows of `lines()` are unchanged, since proofs28 passes.
7. **Theme tokens:** `h-list-row`, `bg-tree-hover` and `bg-tree-selected` exist in `tokens.css` and are used by other lists: `popup_menu.rs`, the database views and `bookmarks_popup.rs`.

**Shots** (real binary through `run.sh`, on a neutral `/private/tmp/openidea-inspect-…` folder that has since been removed; one local folder server, no password, no SFTP; the app was stopped by its PID). All are in `$S/insp8/shots/`:

- `drfc1-01-page-light-1.png`: the Connection tab with `Browse…` to the right of Root path.
- `drfc2-01-chooser-light-1.png`: `Choose Folder` under the root path hint; the up arrow, monospace path with caret (it has the keyboard), refresh, and rows `other` and `site` with folder icons.
- `drfc3-01-root-selected-light-1.png` and `drfc3-02-root-selected-dark-1.png`: `site` selected with the tree selection colour in both themes. The chooser's Cancel/OK sit below the Settings footer.
- `drfc3-03-after-escape-1.png`: after Escape in the list, the chooser is closed and Settings is still open.
- `drfc3-04-mappings-tab-1.png`: the Mappings tab with the second folder icon beside Deployment path.
- `drfc4-01-mapping-chooser-light-1.png` and `drfc4-02-mapping-chooser-dark-1.png`: the mapping's chooser drawn under the table, with the row highlighted.
- **Not taken:** "the field after Select". The inspect script has no scroll step and OK is below the fold. The two e2e tests prove the field and the settings file after OK and Apply.

**Left out.** These are recheck §6 items, not on the chooser's path:

1. The FTP and FTPS type choice.
2. The FTP host, port, user, password, Save password and Passive rows.
3. Staging the FTP password, and the stored-password placeholder.
4. Test Connection with the typed FTP password.
5. Test Connection's FTP failure is still plain text. The remedy rule now exists, but `connection.rs` `remedy()` still answers only SFTP.
6. Server groups.
7. Pruning removed servers from groups.
8. The page's search keywords.
9. Hypothesis: FTP port 0 is read as `Some(0)`. Confirm with a unit test on `{"servers":[{"kind":"ftp","ftp":{"port":0}}]}`.

Also not built:

- The local chooser's title and start folder: `ControlEvent::Browse` carries neither.
- The spinner: a static `refresh` icon is drawn as the busy mark.
- The webview's second listing of the absolute login folder.

### 3. Rows (I edited no `parity.json`)

| Key                         | State           | Why                                                                                                                                                                                                                                                                              |
| --------------------------- | --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.deployment` | stays partial   | The chooser landed with the two e2e tests, but the webview page lets a user choose FTP/FTPS, fill the FTP fields and manage server groups, which the native page cannot. Its text should also name those, not only the local chooser's title and start folder and the busy mark. |
| `ipc:remote_list`           | done; add proof | `workbench::tests::proofs37_settings_deployment_chooser::deployment_root_path_browse_walks_a_local_server_into_a_subfolder_and_applies_it` and `…::deployment_mapping_browse_puts_the_folder_chosen_relative_to_the_root_path_and_applies_it`                                    |
| `settings:build.runTargets` | unchanged, done | proofs30 passes on the shared browser                                                                                                                                                                                                                                            |

### 4. Hypotheses from the notes

- **Confirmed by compiling and running:** the spawn and background-sort pattern; `collate` off the window's thread; dropping the subscription inside its own callback; `uniform_list` inside the page with `debug_bounds`; focus and `claim_keys` ahead of the Settings keys; `TextField` emitting `Submitted` after `set_text`; select-all plus Backspace emitting `Changed`; an entity as a child; NLL (I rewrote that function anyway); `ErrorKind` serialising to camelCase; `bring_into_view` in the e2e tests.
- **Wrong:** the guess that the 100,000-folder sort takes about a second. It takes 26 ms.
- **Still a hypothesis:** that nobody relies on the webview's second listing of `~`. Confirm by reading every `remoteList` caller.

### 5. False or stale in the notes and brief

- **`cargo fmt`:** the notes say to run it after pasting; the rules forbid it.
- **Run Targets row height:** "Run Targets' stay 22 px" is wrong. `RunTargets.module.css` uses `h-list-row` and the tree hover and selection colours.
- **`h-list-row` users:** the writer's report says it has no other user in the crate; `popup_menu.rs` and the database views use it.
- **The draft's e2e guard** accepted `..` escapes.
- **The draft's keyboard:** it gave no focus on open, so Escape could reach Settings, and its list released keys while empty.
- **HEAD:** the brief names `c3d522d56`; HEAD is `00bd90824`, and `git diff --stat c3d522d56 HEAD -- app-gpui/src` prints nothing.
- **`git diff` output:** for `build_deployment.rs` and `paths.rs` it shows whole-file rewrites, but there are no CR characters and `diff <(git show HEAD:f) f` shows only my lines.
- **The page's own FTP failure:** the merge report calls plain text for it right; the webview at HEAD contradicts that (recheck §6.5).
- **Other agent's files:** `app-gpui/src/run/test_tree_pending/` (untracked) and `workbench/tests/proofs45_*` belong to another agent and were not touched.
