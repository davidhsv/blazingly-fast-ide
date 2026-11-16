I finished both parts of the brief: Part 0 (the secret in a debug output) and the whole Deployment slice, which is now declared, compiles and passes its tests. Nothing under `crates/` changed. Nothing was committed.

While I worked, HEAD moved from 5714c388 to 91b43718 (another session's commits). Those commits touch 14 files, none under `app-gpui/` or `crates/`. The only files I changed are the six below, and `git status --short src/ src-tauri/ crates/` prints nothing.

## 1. Gates (measured; logs in `$S/ddepi/`)

All gate runs below are on the final source. I made one more change after the first full set (path ellipsis, see section 2), so the earlier set only counts as history:

- **First set, before the ellipsis change:** suite 1 and suite 2 each `test result: ok. 7720 passed; 0 failed`, loop 20 of 20, web_view ok, clippy ok, build 0 warnings.
- **Whole suite A** (`suite3.log`, nothing else running):
  - `uptime before: 23:32 … load averages: 6.09 7.61 10.16`
  - `test result: ok. 7720 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 194.27s`
  - `uptime after: 23:35 … load averages: 60.45 20.34 14.39`
- **Whole suite B** (`suite4.log`, with the loop running beside it):
  - `uptime before: 23:35 … load averages: 51.84 19.82 14.28`
  - `test result: FAILED. 7719 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 224.95s`
  - `uptime after: 23:39 … load averages: 32.00 21.48 15.86`
  - The failure is outside this slice, quoted whole (state dumps trimmed; full text in `suite4.log`):
    ```
    ---- workbench::popup_placement::tests::show_usages_opens_under_the_caret stdout ----
    panicked at src/workbench/mod.rs:8012:9:
    the usages popup did not happen within thirty seconds (ran out after 39.860026958s; reached later: None)
    state when it ran out: FSEvents streams of this process: 8 … status=0 … language servers: backend: []; window heard: []  no fake server
    ```
  - I then ran it alone 20 times by filter: 20 of 20 passed (`usages-*.log`). I did not find the cause.
  - Hypothesis: it only fails under heavy load. Check: run it in a loop while a whole-suite run is going.
- **Twenty runs by filter during suite B:** `$BIN deployment` covers all 100 tests of the slice. Every run gave `test result: ok. 100 passed; 0 failed` (`loopB/loop-summary.txt`). No test of the slice failed even once.
- **web_view:** `test result: ok. 1 passed; 0 failed`.
- **clippy** (`--all-targets -- -D warnings`): exit 0.
- **`cargo build`:** exit 0, 0 warnings.
- **Failures seen along the way, all fixed:**
  - `deployment::remote_host_view::tests::folders_expand_from_the_chevron_and_the_keyboard_and_files_open` panicked with `remote-host-toggle:/srv/logs is drawn`. The chevron had an `id` but no `debug_selector`; I added one.
  - Five `variable does not need to be mutable` warnings in the draft tests.
  - One clippy `manual_contains` in the `workbench/deployment.rs` tests.

## 2. Parts

**Part 0, the secret in a debug output.** `app-gpui/src/features_b/deployment/store.rs`:

- `SecretAnswer` and `SecretForm` no longer derive `Debug`. Their hand-written `Debug` prints `secret: Some("<redacted>")` when a secret is there and `None` when it is empty, the same way `database::source_dialog::SourceForm` does.
- Nothing else holds them: the views carry the secret as `database::store::SecretText`, whose `Debug` was already redacted.
- New test `openidea_gpui::features_b::deployment::store::tests::the_debug_print_of_a_secret_shows_its_presence_never_its_text`. It formats the form and the answer (`{:?}` and `{:#?}`) with a known secret and asserts the text is absent and the marker is present.
- Sweep of the slice: no type in `app-gpui/src/deployment/` or `workbench/deployment.rs` derives `Debug` over a secret, and none of the 6 `tracing::` calls takes one. `DeploymentServer` holds no secret (`crates/ide-core/src/remote_settings.rs:117`).

**Settled first:**

- The keys `deployment.servers` and `deployment.defaultServer` are accepted as the settings page writes them (`src/features/settings/pages/deployment.tsx:550-551`). Measured: every `workbench::deployment` test stores its servers that way and they reach the backend.
- A local server never runs SSH code:
  - `server_ssh` returns `""` for `Local` (`features_b/deployment/model.rs:196-201`).
  - `ssh_retry` returns `None` for an empty ssh (`model.rs:377`).
  - The backend maps `Local` to `RemoteFs::Local` (`crates/ide-remote/src/service.rs:83,410`).
  - Hypothesis (read in the code, not measured).

**The slice.** I applied the notes' hunks to `app-gpui/src/lib.rs` and `app-gpui/src/workbench/mod.rs`:

- the `mod` lines and the field;
- `TOOL_WINDOWS` grows to 20 with `remoteHost`;
- `register_actions`, the mount in `mount_panels`, `sync_remote_host`, `deployment_layer`, `track_deployment_focus`, `deployment_settings_changed`.

All anchors were still there. I formatted the six draft files and the edited ones with `$S/fmt.sh`.

**Changes beyond the notes:**

- `remote_host_view.rs`: the chevron's `debug_selector`, and ellipsis on the root path (`Deployment.module.css:21-29`, `.rootPath`).
- `sync_dialog.rs`: ellipsis on the two root headers and on the table cells (`.comparisonRoots > span > span`, `.syncTable td`).
- `workbench/deployment.rs`: new test `workbench::deployment::tests::the_deployment_actions_are_available_only_with_a_project_open`.

**Things I checked:**

- Every native call uses the same `ide_host::ops::deploy::*` function as `src-tauri/src/commands/deploy.rs`, with the same arguments.
- The stripe follows `src/features/deployment/index.ts:76-85` (right, order 30, icon web, `onUse`). Shot `ddepi3-01` shows no globe on the stripe before use; after use it sits on the right stripe under Database.
- No existing test counts the tool windows: both suites pass with 20.

**Left out** (from the notes, still true):

- the `deployment` menu group;
- the native comparison window;
- the editable local side of the comparison;
- speed search in the Remote Host tree;
- the status tooltip;
- the chooser's title;
- live name validation;
- SFTP end to end. The trust-host and SSH Password paths are not tested in a window; the SSH Password dialog is tested on its own view (`deployment::secret_dialog::tests`, 2 tests).

## 3. Rows (all test names under `openidea_gpui::`)

None of these `action:` / `ipc:` / `point.` / `feature:` keys exist in `parity.json` yet; they are unticked rows in `INVENTORY.md`.

| Key                                                                                                                          | State               | Proof / what's missing                                                                                                                                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------------------------------------------------------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:deployment.uploadToDefault`                                                                                          | done                | `workbench::deployment::tests::upload_to_default_server_copies_the_file_to_the_server_and_reports_it`                                                                                                                                                                                                                                                                  |
| `action:deployment.uploadTo`                                                                                                 | done                | `…::upload_to_explains_the_rejected_paths_and_links_settings`                                                                                                                                                                                                                                                                                                          |
| `action:deployment.downloadFromDefault`                                                                                      | done                | `…::download_from_default_server_replaces_the_local_file_without_asking`                                                                                                                                                                                                                                                                                               |
| `action:deployment.downloadFrom`                                                                                             | done                | `…::download_from_chooses_the_server_and_replaces_the_local_file`                                                                                                                                                                                                                                                                                                      |
| `action:deployment.syncWith`                                                                                                 | done                | `…::sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`                                                                                                                                                                                                                                                                              |
| `action:deployment.syncWithDefault`                                                                                          | done                | `…::sync_with_default_compares_previews_and_synchronizes_all`                                                                                                                                                                                                                                                                                                          |
| `action:deployment.deleteFromServer`                                                                                         | done                | `…::delete_from_server_asks_and_deletes_the_remote_copy`                                                                                                                                                                                                                                                                                                               |
| `action:deployment.toggleAutoUpload`                                                                                         | done                | `…::automatic_upload_toggles_the_projects_setting` (toggled state read off/on, stored `always`/`never`)                                                                                                                                                                                                                                                                |
| `action:deployment.browseRemoteHost`                                                                                         | done                | `…::the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`                                                                                                                                                                                                                                                                                            |
| `action:deployment.openInBrowser`                                                                                            | done                | `…::open_in_browser_opens_the_deployed_url_of_the_file` (`cx.opened_url()`)                                                                                                                                                                                                                                                                                            |
| `action:deployment.configure`                                                                                                | done for the action | `…::deployment_configuration_opens_the_deployment_settings_page`. The page it opens is not built natively: shot `ddepi3-02` shows "Deployment servers: This page is not available yet".                                                                                                                                                                                |
| `ipc:deploy_coverage`                                                                                                        | done                | `…::upload_to_explains_the_rejected_paths_and_links_settings`                                                                                                                                                                                                                                                                                                          |
| `ipc:deploy_upload`                                                                                                          | done                | `…::upload_to_default_server_copies_the_file_to_the_server_and_reports_it`                                                                                                                                                                                                                                                                                             |
| `ipc:deploy_download`                                                                                                        | done                | the two `download_*` tests                                                                                                                                                                                                                                                                                                                                             |
| `ipc:deploy_delete_remote`                                                                                                   | done                | `…::delete_from_server_asks_and_deletes_the_remote_copy`                                                                                                                                                                                                                                                                                                               |
| `ipc:deploy_info`                                                                                                            | done                | `…::open_in_browser_opens_the_deployed_url_of_the_file`                                                                                                                                                                                                                                                                                                                |
| `ipc:deploy_compare`, `ipc:deploy_sync`                                                                                      | done                | the two `sync_with_*` tests                                                                                                                                                                                                                                                                                                                                            |
| `ipc:deploy_server_root`, `ipc:remote_list`, `ipc:remote_mkdir`, `ipc:remote_rename`, `ipc:remote_delete`, `ipc:remote_open` | done                | `…::the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`; `remote_list` also by `…::the_remote_host_server_chooser_shows_the_other_servers_files`                                                                                                                                                                                                   |
| `ipc:deploy_test_connection`                                                                                                 | not earned          | only the Settings page calls it                                                                                                                                                                                                                                                                                                                                        |
| `point.registerToolWindow:remoteHost`                                                                                        | partial             | `deployment::remote_host_view::tests` (6 tests) and the two remote-host workbench tests. Missing: speed search                                                                                                                                                                                                                                                         |
| `point.registerOverlay:deployment.dialogs`                                                                                   | partial             | the Sync dialog through the app; SSH Password only on its view; Unknown Host Key never reached                                                                                                                                                                                                                                                                         |
| `point.registerMenuGroup:deployment`                                                                                         | not earned          | the group is not built                                                                                                                                                                                                                                                                                                                                                 |
| `feature:deployment`                                                                                                         | partial             | Missing: menu group, native comparison window, editable local side, speed search, SFTP end to end                                                                                                                                                                                                                                                                      |
| `unit:…/SyncPreview.test.tsx`                                                                                                | partial → done      | 3 of 3, each on the model (`features_b::deployment::preview::tests`) and on the view (`deployment::sync_preview::tests::{ignores_a_late_response_for_a_file_that_is_no_longer_selected, shows_read_errors_and_allows_retrying_without_closing_the_comparison, explains_why_files_have_no_text_preview}`). Preview reached in a project window by `sync_with_default_…` |
| `unit:…/Dialogs.test.tsx`                                                                                                    | done (no change)    | 7 of 7, each on the model (`sync_view::tests`) and on the view (`deployment::sync_dialog::tests::*`, same 7 names). Through a project window: save-before-sync, single-row sync and Synchronize All. Those in-app tests call `dialog.select` / `dialog.synchronize`; clicks and keys are proven only on the view                                                       |
| `unit:…/actions.test.tsx`                                                                                                    | partial (no change) | 11 `it` blocks (16 runtime cases, since the three `it.each` give 2+3+3), all 11 on the model (`features_b::deployment::actions::tests` 8, `store::tests` 4). Through a project window: rejected paths (uploadTo variant only), save-before-sync, single-file sync, compare-every-covered. Missing: the native-window cases, and the other cases in a window            |
| `unit:…/comparison.test.ts`                                                                                                  | done (no change)    | 7 of 7 on the model; reached in the app by the preview                                                                                                                                                                                                                                                                                                                 |
| `unit:…/model.test.ts`                                                                                                       | done (no change)    | 14 of 14 on the model                                                                                                                                                                                                                                                                                                                                                  |
| `unit:…/remoteHost.test.ts`                                                                                                  | done (no change)    | 3 of 3 on the model (plus the extra `sorts_folders_first_and_drops_stale_listings`)                                                                                                                                                                                                                                                                                    |
| `unit:…/window.test.ts`                                                                                                      | done (no change)    | 2 of 2 on the model                                                                                                                                                                                                                                                                                                                                                    |
| `unit:src/platform/deploymentSettings.test.ts`                                                                               | done (no change)    | 3 of 3 on the model (`platform_more::remote_settings::tests`)                                                                                                                                                                                                                                                                                                          |

No `settings:` row changed (`settings:build.deployment.options` was already done and is not part of this slice).

**`when` clauses.** All 11 actions have `when: projectOpen`. The new test reads each one off on the Welcome window (`is_action_enabled` false, and `execute_action_or_throw` refused) and on in a project window.

## 4. Shots (all under `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/`)

- **`ddepi4-01-remote-host-1.png`:** the Remote Host window with the Mounted chooser, refresh/collapse/settings icons, the root path ellipsized, and the rows lib/, index.php, old.txt. `ddepi1-01-…` is the same screen before the ellipsis fix, path clipped.
- **`ddepi2-01-remote-host-row-menu-1.png`:** the row menu (Open, Download to Project, New Folder… disabled, Rename…, Delete…, Refresh, Copy Path) inside the window. Items and disabled states match `RemoteHostView.tsx:71-120`. The right-clicked row shows only a hover tint, not selection. Hypothesis: the web version selects the row on right click; check its Tree component.
- **`ddepi4-02-sync-1.png` and `ddepi4-03-sync-down-1.png`:** the Sync dialog with rows README.md Upload, index.php Upload, old.txt Download, and the footer "3 of 4 files will change". After Down, index.php is selected and its preview shows "hello local" against "hello server" with the changed words marked. The root headers are now ellipsized. The preview's right header ("Mounted: /private/…") is still clipped; it is drawn by the shared diff view (`vcs_diff`), which I did not change.
- **`ddepi3-01-before-use-1.png`:** a fresh home; there is no Remote Host button on the stripe before use.
- **`ddepi3-02-configure-1.png`:** Settings opens on Build, Execution, Deployment › Deployment, which says "This page is not available yet".
- I found no error or panic in the stdout of runs ddepi1 to ddepi4. Nothing was cut or overlapping apart from the preview header above.

## 5. Stale or false in the brief and notes

- `src/platform/deploymentSettings.ts` does not exist. The source is `src/platform/remoteSettings.ts` (`validateDeployment`, `deploymentConnection`); only the test file is named `deploymentSettings.test.ts`.
- The notes say to run `cargo fmt`, which the rules forbid. I used `fmt.sh` instead.
- The notes' `parity.json` keys for actions, IPC, points and feature do not exist in `parity.json` yet.
- `unit:…/SyncPreview.test.tsx` is listed as partial in `parity.json` ("the view is not built"); the view is now built and tested.
- The notes say no backend seam is needed; that held. All 11 hypotheses of the notes held in the tests, except number 9: the page id is selected, but the page itself is not built.
