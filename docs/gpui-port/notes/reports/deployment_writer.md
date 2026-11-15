I've written the Deployment feature as six new Rust files plus the integration notes, but none of it has been compiled or run. No backend seam was needed: a server of kind `local` (`RemoteFs::Local`) lets every end-to-end test use a temp folder as the server, so no test touches the network, SSH files or the home folder.

## Files written

| File                                          | Lines | Constructor · events · accessors                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/deployment/mod.rs`              | 8     | the four `pub mod` lines                                                                                                                                                                                                                                                                                                           |
| `app-gpui/src/deployment/remote_host_view.rs` | 916   | `RemoteHostView::new(cx)`, fed by `set_state(RemoteHostState)` · `RemoteHostEvent` {ShowServerMenu, ShowMenu, Refresh, CollapseAll, Retry, RunAction, Expand, Open}, menu rows are `HostCommand` · `rows()`, `toolbar()`, `selected()`, `menu_items()`, `server_label()`, `state()`                                                |
| `app-gpui/src/deployment/sync_dialog.rs`      | ~1240 | `SyncDialog::new(store, project_root, server_root, standalone, window, cx)` · `SyncDialogEvent` {BeforeSynchronize, Run, Refresh, Scan, Preview, ShowMenu, Close} · `rows`, `columns`, `buttons`, `errors`, `footer`, `title`, `selected`, `menu_items`; answered by `scan_answered`, `preview_loaded`, `prepared`, `synchronized` |
| `app-gpui/src/deployment/sync_preview.rs`     | 294   | `SyncPreviewView::new` · `SyncPreviewEvent::Requests` · `show`, `retry`, `loaded`, `state`, `message`, `error`, `diff`                                                                                                                                                                                                             |
| `app-gpui/src/deployment/secret_dialog.rs`    | 232   | `SecretDialog::new(message, window, cx)` · `SecretDialogEvent` {Answered{SecretText, save}, Cancelled}; the secret is only ever painted as bullets                                                                                                                                                                                 |
| `app-gpui/src/workbench/deployment.rs`        | ~2120 | registration, handlers, backend calls off the main thread, and 13 end-to-end tests                                                                                                                                                                                                                                                 |

## Actions registered

All 11 are registered, each run through the registry by its own end-to-end test in a project window:

- **Transfers:** `uploadToDefault` and `uploadTo` (`deploy_coverage`, `deploy_upload`); `downloadFromDefault` and `downloadFrom` (`deploy_download`).
- **Sync with Deployed:** `syncWith` and `syncWithDefault` (`deploy_compare`, `deploy_sync`, `remote_open`).
- **`deleteFromServer`:** asks first, then `deploy_delete_remote`.
- **`toggleAutoUpload`:** writes the project setting; the test reads the toggled state off and on.
- **`openInBrowser`:** `deploy_info`; the test checks `cx.opened_url()`.
- **`browseRemoteHost`:** `deploy_server_root`, `remote_list`, `remote_mkdir`, `remote_rename`, `remote_delete`, `remote_open`.
- **`configure`:** opens Settings on `build.deployment`.

The main menu rows already exist in `menu_spec.rs`, and none of these actions has a default shortcut.

## Spec coverage

- **Done through views or the app:**
  - `SyncPreview.test.tsx`: 3 of 3 cases.
  - `Dialogs.test.tsx`: 7 of 7.
  - `comparison`, `model`, `remoteHost`, `window` and `deploymentSettings`: their existing model tests now count, because those models are reached from the actions.
- **Partial:** `actions.test.tsx` is 11 of 11 on the model, but 4 cases assume a native window of its own; here the comparison is drawn over the project window.
- **IPC rows:** all of the listed ones except `deploy_test_connection`, which only the Settings page (not built here) calls.

## Left out

- **Context-menu group:** the `deployment` group of the Project view, editor and tab menus is not wired. Its row functions can't read the settings, and the webview runs `deploy_info` asynchronously before the menu opens, which for SFTP means connecting.
- **Comparison window:** there is no separate native window.
- **Read-only local side:** the comparison's local side can't be edited, so the "Save local changes?" guard is never asked.
- **Remote Host tree:** there is no speed search.
- **Choosers are popup menus:** the server, Compare by and per-row operation choosers have no title.
- **"/" in a folder name:** it is reported after OK rather than under the field.
- **SFTP end to end:** untested, including the trust-host-key and password flows. There is a loopback test sshd in `ide-ssh`, but I didn't check whether connecting reads `~/.ssh`.

## Hypotheses

These are listed with checks in the notes:

- Project settings accept the dotted keys `deployment.servers` and `deployment.defaultServer`. If not, every end-to-end test fails.
- Types are converted to and from `ide_remote`'s through serde, and the shapes match.
- The web URL is `http://localhost:8080/app/a.php`.
- An uploaded row becomes Same and is hidden.
- `open_project_file` opens the server file's cached copy, which lies outside the project.
- A chooser closed without a choice drops its waiter.
- `claim_keys` delivers the Sync dialog table's keys.
- The `build.deployment` settings page id can be selected.

## Most likely to fail first

- `the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`: long, with keyboard-driven menus and typing into prompts.
- `sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`.
- `open_in_browser_opens_the_deployed_url_of_the_file`.
- `deployment_configuration_opens_the_deployment_settings_page`.
- The sync dialog's keyboard test, which depends on the key claims.

## Integrator hunks

- `lib.rs`: `pub mod deployment;`.
- `workbench/mod.rs`:
  - the `mod` line;
  - the field and its initial value;
  - `TOOL_WINDOWS` grows from 19 to 20 with `remoteHost`;
  - the `register_actions` call;
  - the mount arm and the sync call in `mount_panels`;
  - the dialog layer, placed before the existing dialog layer;
  - focus tracking in `publish_context`;
  - a hook for settings changes.

Separately, the existing `store::{SecretAnswer, SecretForm}` derive `Debug` over the plain secret, so a debug print would show it. My code doesn't use them; they should get a redacting `Debug`.

Integration notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/deployment_integration.md`
