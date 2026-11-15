# Integrating Deployment into the workbench

New files, not compiled yet: `app-gpui/src/deployment/{mod.rs, remote_host_view.rs, sync_dialog.rs, sync_preview.rs, secret_dialog.rs}` and `app-gpui/src/workbench/deployment.rs`. They build on the existing models `app-gpui/src/features_b/deployment/*` and `app-gpui/src/platform_more/remote_settings.rs`, which were not edited. Every anchor below is quoted from the file as it is now; line numbers are left out because `mod.rs` moves. Run `cargo fmt` after pasting: several written lines run past 120 columns.

No backend seam is needed. A server of kind `local` goes through `ide_ssh::filesystem::RemoteFs::Local` (tokio fs, `crates/ide-ssh/src/filesystem.rs`), and every `ide_host::ops::deploy` function the Tauri adapters call (`src-tauri/src/commands/deploy.rs`) works with it. The tests give each local server an absolute, canonicalised temp `rootPath`: an empty or `~` root would resolve through `dirs::home_dir()` (`RemoteFs::resolve`). `remote_open` copies into `<data_dir>/remote/…` (`crates/ide-host/src/lib.rs`, `RemoteService::new(ssh, data_dir.join("remote"))`), which is the tests' temp home. No test opens a socket, reads SSH configuration or touches the user's home.

## 1. `app-gpui/src/lib.rs`

After `pub mod debugger;` add:

```rust
pub mod deployment;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod debug_extras;` add:

```rust
mod deployment;
```

### 2b. The field

In `pub struct Workbench`, after

```rust
    /// What the TODO tool window is fed from.
    todo: Option<todo_window::TodoFeed>,
```

add:

```rust
    /// Deployment: the part that had the keyboard last, the comparison, the SSH prompt and the
    /// Remote Host trees of the project open here.
    deployment: deployment::DeploymentState,
```

### 2c. Its initial value

In `Workbench::new`, after `            todo: None,` add:

```rust
            deployment: Default::default(),
```

### 2d. The tool window

Replace `const TOOL_WINDOWS: [&str; 19] = [` with `const TOOL_WINDOWS: [&str; 20] = [`, and after `    PULL_REQUESTS_TOOL_WINDOW,` (the last entry of that array) add:

```rust
    crate::features_b::deployment::REMOTE_HOST_WINDOW,
```

`tool_window_declaration("remoteHost")` reads `assets/gen/contributions.json`, which already declares it (title Remote Host, icon web, right, order 30, stripe onUse). The array's length change also registers `toolwindow.remoteHost`.

### 2e. The actions

In `pub fn register_actions(cx: &mut App)`, after `    database_extras::register_actions(registry);` add:

```rust
    deployment::register_actions(registry);
```

All 11 ids are in `assets/gen/actions.json` (`grep -c '"deployment\.' app-gpui/assets/gen/actions.json` gives 11 action ids), so `registry.handle` takes their title, `when: projectOpen` and keywords from there. None has a default shortcut (no `deployment.` entry in `src/platform/keymaps/`). Their main-menu rows already exist in `workbench/menu_spec.rs` (Tools › Deployment, the same rows as `src-tauri/src/menu.rs`); no menu hunk is needed.

### 2f. Mounting the tool window

In `fn mount_panels`, after `                TODO_TOOL_WINDOW => self.mount_todo(window, cx),` add:

```rust
                crate::features_b::deployment::REMOTE_HOST_WINDOW => self.mount_remote_host(window, cx),
```

and after `        self.sync_pull_requests_window(cx);` (it occurs once, in `mount_panels`) add:

```rust
        self.sync_remote_host(window, cx);
```

### 2g. The dialog layer

In `fn render`, replace

```rust
            .children(self.vcs_dialog_layer(cx))
```

with

```rust
            .children(self.deployment_layer(cx))
            .children(self.vcs_dialog_layer(cx))
```

The Sync with Deployed dialog and the SSH Password dialog must be under the confirmations (Unknown Host Key, which waits on the comparison) and under the popup slot (the Compare by and operation menus open from the dialog).

### 2h. The part that had the keyboard last

In `fn publish_context`, after

```rust
        set_context(window, cx, "projectViewFocus", focused_panel.as_deref() == Some(PROJECT_TOOL_WINDOW));
```

add:

```rust
        self.track_deployment_focus(focused_panel.as_deref() == Some(PROJECT_TOOL_WINDOW), focused);
```

`focused` is the editor-focus flag the method computes in its first lines. An action run from Find Action works on the Project view's selection or the editor's file, whichever had the keyboard last.

### 2i. Settings changes

In `pub fn backend_event`, in the arm `IdeEvent::SettingsChanged { key } => {`, after `                self.code_insight_settings_changed(cx);` add:

```rust
                self.deployment_settings_changed(cx);
```

This shows the servers as they now are in the Remote Host window, and lists again a server whose connection, root or mappings changed.

## 3. What `workbench/deployment.rs` holds

- `DeploymentState` holds `part` and `slot`. `DeploymentSlot` is per project root and is made again when another project opens. It holds `Entity<DeploymentStore>`, the comparison on screen (`SyncSlot`: the dialog, a `ComparisonWindow`, a `DeploymentComparison` and the preview abort flags), the SSH prompt (`SecretSlot`), and the Remote Host start flag and window handle.
- Backend calls go through `deploy_call` (`Backend::with_host`, off the main thread). `with_ssh` retries after `ssh_retry`: "Unknown Host Key" with Trust and Connect, which calls `host.ssh.trust_host`, or the SSH Password dialog, which calls `host.ssh.set_secret`. Like the webview, it does at most three remedies, and never for a local server (`ssh == ""`).
- `bridge` converts this crate's `features_b::deployment::types` shapes to and from `ide_remote`'s through serde. `app-gpui` does not depend on `ide-remote`, so those types cannot be named. `types.rs` says the shapes are field for field the same.
- Flows:
  - transfers;
  - Delete from Server, which asks first;
  - Open in Browser;
  - Automatic Upload, which writes the project's `deployment.autoUpload`;
  - Configure, which runs `file.settings` with `"build.deployment"`;
  - Sync with Deployed: `ComparisonWindow` launch, then `DeploymentStore::open_sync`, then `SyncEffect`s, then `SyncDialog` in standalone mode. `BeforeSynchronize` and `SaveDocuments` save the open editors.
  - Remote Host: `RemoteHost` requests and effects, and the context menu's commands.

## 4. `parity.json` rows this slice should earn

Test names are full paths under `openidea_gpui::`. "→ done" means every case of the spec or every behaviour of the row has a native test that goes through the app; "→ partial" says what is missing.

### Unit rows

- `unit:src/features/deployment/SyncPreview.test.tsx` → done, 3 of 3 cases:
  - `deployment::sync_preview::tests::ignores_a_late_response_for_a_file_that_is_no_longer_selected`
  - `deployment::sync_preview::tests::shows_read_errors_and_allows_retrying_without_closing_the_comparison`
  - `deployment::sync_preview::tests::explains_why_files_have_no_text_preview`
  - The model's `features_b::deployment::preview::tests` stay.
- `unit:src/features/deployment/Dialogs.test.tsx` → done, 7 of 7 cases, in `deployment::sync_dialog::tests::`:
  - `fills_a_secondary_window_without_creating_an_in_page_modal_and_saves_local_edits_before_synchronization`
  - `shows_mirrored_metadata_and_previews_the_selected_file_with_identical_rows_filtered`
  - `filters_by_name_and_category_navigates_with_arrows_and_cycles_operations_with_space`
  - `updates_a_rows_operation_and_only_applies_explicit_synchronization`
  - `finds_content_hidden_by_identical_metadata_and_skips_byte_identical_files`
  - `synchronizes_only_the_current_row_from_the_toolbar_or_enter_and_disables_skipped_selections`
  - `preserves_manual_choices_reports_failures_and_ignores_a_cancelled_scan`
- `unit:src/features/deployment/actions.test.tsx` → partial, 11 of 11 cases on the model (`features_b::deployment::actions::tests`, `features_b::deployment::store::tests`, now reachable). The cases also run through the app:
  - rejected paths: `workbench::deployment::tests::upload_to_explains_the_rejected_paths_and_links_settings`
  - the chooser: `workbench::deployment::tests::download_from_chooses_the_server_and_replaces_the_local_file`
  - saving before a sync, and a single-file sync: `workbench::deployment::tests::sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`
  - Synchronize All: `workbench::deployment::tests::sync_with_default_compares_previews_and_synchronizes_all`

  Missing: cases 2, 3 and 4 ("opens … in a native window on desktop", "reports a comparison window that cannot open"). The comparison opens over the project window, not in a native window of its own.

- `unit:src/features/deployment/comparison.test.ts` → done, 7 of 7 (`features_b::deployment::comparison::tests`). The model is now reached by the preview and the content scan in `workbench::deployment::tests::sync_with_default_compares_previews_and_synchronizes_all`.
- `unit:src/features/deployment/model.test.ts` → done, every case in `features_b::deployment::model::tests`. It is now reached by every action.
- `unit:src/features/deployment/remoteHost.test.ts` → done, 3 of 3 (`features_b::deployment::remote_host::tests`). It is now reached by `workbench::deployment::tests::the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`.
- `unit:src/features/deployment/window.test.ts` → done, 2 of 2 (`features_b::deployment::window::tests`). The model is reached by `open_comparison` and `close_comparison`. Its "Save local changes?" branch is never taken in the app, because the comparison has no editable side.
- `unit:src/platform/deploymentSettings.test.ts` → done, 3 of 3 (`platform_more::remote_settings::tests`). `normalized_deployment` and `deployment_connection` are now on every path.

### IPC rows

Each is proven by a state only that backend call produces, called as the adapter calls it. All tests are in `workbench::deployment::tests::`.

- `ipc:deploy_coverage` → done: `upload_to_explains_the_rejected_paths_and_links_settings`. The body "README.md: Mounted: Not covered by this server's mappings" is the backend's reason, prefixed per server.
- `ipc:deploy_upload` → done: `upload_to_default_server_copies_the_file_to_the_server_and_reports_it`. The file appears in the server folder.
- `ipc:deploy_download` → done: `download_from_chooses_the_server_and_replaces_the_local_file` and `download_from_default_server_replaces_the_local_file_without_asking`. The local file gets the server's text.
- `ipc:deploy_delete_remote` → done: `delete_from_server_asks_and_deletes_the_remote_copy`. The server file is gone and the local one stays.
- `ipc:deploy_info` → done: `open_in_browser_opens_the_deployed_url_of_the_file`. `cx.opened_url()` is the mapped web URL.
- `ipc:deploy_compare` → done: `sync_with_default_compares_previews_and_synchronizes_all`. The rows are `src/a.php` (local only) and `src/b.php` (remote only).
- `ipc:deploy_sync` → done: the same test, where the upload and the download both land; and `sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`, where the edited editor text lands on the server.
- `ipc:deploy_server_root` → done: `the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`. The tree's root is the server folder.
- `ipc:remote_list` → done: the same test (rows `a.php`, `notes.txt`), and `the_remote_host_server_chooser_shows_the_other_servers_files`.
- `ipc:remote_mkdir` → done: the same test. New Folder… makes `logs` on disk.
- `ipc:remote_rename` → done: the same test. `todo.txt` has the old file's text.
- `ipc:remote_delete` → done: the same test. `todo.txt` is gone.
- `ipc:remote_open` → done: the same test. The editor shows the server file's text. The preview's snapshot in `sync_with_default_compares_previews_and_synchronizes_all` also uses it.
- `ipc:deploy_test_connection` → not earned. Only the Settings › Deployment page calls it (`src/features/settings/pages/deployment.tsx`), and this slice does not build that page.

### Action rows

All tests are in `workbench::deployment::tests::`.

- `action:deployment.uploadToDefault` → done: `upload_to_default_server_copies_the_file_to_the_server_and_reports_it`
- `action:deployment.uploadTo` → done: `upload_to_explains_the_rejected_paths_and_links_settings`
- `action:deployment.downloadFromDefault` → done: `download_from_default_server_replaces_the_local_file_without_asking`
- `action:deployment.downloadFrom` → done: `download_from_chooses_the_server_and_replaces_the_local_file`
- `action:deployment.syncWith` → done: `sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`
- `action:deployment.syncWithDefault` → done: `sync_with_default_compares_previews_and_synchronizes_all`
- `action:deployment.deleteFromServer` → done: `delete_from_server_asks_and_deletes_the_remote_copy`
- `action:deployment.toggleAutoUpload` → done: `automatic_upload_toggles_the_projects_setting`. It reads the toggled state off and on, and the stored `always` / `never`.
- `action:deployment.browseRemoteHost` → done: `the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`
- `action:deployment.openInBrowser` → done: `open_in_browser_opens_the_deployed_url_of_the_file`
- `action:deployment.configure` → done: `deployment_configuration_opens_the_deployment_settings_page`

### Points and feature

- `point.registerToolWindow:remoteHost` → partial:
  - `deployment::remote_host_view::tests` (6 tests)
  - `workbench::deployment::tests::the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`
  - `workbench::deployment::tests::the_remote_host_server_chooser_shows_the_other_servers_files`

  Missing: speed search in the tree.

- `point.registerOverlay:deployment.dialogs` → partial: the Sync dialog through the app (the two sync tests above), and the SSH Password dialog on its own (`deployment::secret_dialog::tests`, 2 tests). The SSH prompt and Unknown Host Key are never reached in a project window, because every test server is local.
- `point.registerMenuGroup:deployment` → not earned (left out, see 5).
- `feature:deployment` → partial: everything above. Missing: the menu group, the native comparison window, the editable local side of the comparison, and SFTP proven end to end.

## 5. Left out, and why

- **The `deployment` menu group of the Project view, editor and editor tab menus** (`point.registerMenuGroup:deployment`). Its rows are plain `fn`s over `GroupFacts` (`workbench/context_menus.rs`), which carries neither the deployment settings nor the project root. The webview's `prepare` step calls `deploy_info` asynchronously before the menu opens; for SFTP that connects, so it cannot be done in the row function.
- **The comparison in a native window of its own.** The dialog is drawn over the project window in standalone mode, driven by the same `ComparisonWindow` launch and close model.
- **The editable local side of the embedded diff.** The preview's `DiffView` is built from the local file's current text (`ops::fs::read_text`, which prefers an open editor's buffer) and the server snapshot; editing it does not reach the file. For the same reason the close guard's "Save local changes?" is never asked.
- **Speed search** in the Remote Host tree.
- **The status tooltip** on a row's operation cell.
- **The `<select>` controls.** The server, Compare by and per-row operation choosers are popup menus.
- **The chooser's title** ("Upload to", "Download from", "Sync with Deployed to"). The webview's `chooseItem` shows one; here the chooser is a menu under the window's top, as Restore Tool Window Layout's is (`workbench/app_actions.rs`, `pick_layout`).
- **Live name validation.** `PromptDialog`'s `FieldRule` has no "no /" rule, so "A name cannot contain “/”" is reported after OK, as a failed New Folder / Rename, instead of under the field.
- **SFTP end to end.** `crates/ide-ssh/src/testing.rs` has a loopback sshd behind the `testing` feature. I did not check whether `SshManager::session` reads the user's `~/.ssh` (known_hosts, config, agent) on the way, so no test uses it. The trust and secret remedies run only for SFTP and are untested in a window.

## 6. Hypotheses (not checked without a compiler)

1. **Project settings writes.** `settings::set("w1", SettingsScope::Project, "deployment.servers", <array>)` and `"deployment.defaultServer"` are taken by `ide_host::ops::project::set_settings` as dotted keys, as the webview's `deployment.autoUpload` is.
   - Check: `crates/ide-host/src/ops/project.rs` `set_settings`.
2. **Serde round trip.** `bridge` works: `types.rs` matches `ide_remote`'s `DeploymentCoverage`, `TransferReport`, `SyncEntry`, `SyncStep`, `DeploymentRoot` and `DeployedPath` (`web_url`, camelCase) field for field.
3. **Web URL format.** `Mappings::web_url` makes `http://localhost:8080/app/a.php` for `webUrl` `http://localhost:8080`, mapping `src` → `/`, web `app`.
4. **Rows after a sync.** `compare` lists `src/a.php` before `src/b.php` (it collects into a `BTreeMap`, `deploy.rs`). After an upload with `preserveTimestamps` (the default), the row is Same, so it is hidden.
5. **Opening the cache copy.** `open_project_file` opens a file outside the project root (the copy under `<data_dir>/remote`) in a text tab.
6. **Cancelled chooser.** The `show_menu` command closure is dropped when the menu closes without a choice, so the chooser's oneshot resolves `Canceled`. If it is kept, a cancelled chooser leaves a parked task and nothing else.
7. **Key claims.** `input::claim_keys` sends up, down, enter, space and escape to the Sync dialog's table before the keymap, while the table's focus handle is focused (the `PromptDialog` pattern).
8. **Re-exports.** `gpui_kit` exports `AnyWindowHandle`, `relative`, `Hsla`, `ClipboardItem` and `ScrollStrategy`: each is already used elsewhere in the crate, but I did not check that every one is imported from the crate root as I import it.
9. **Settings page id.** `file.settings` with `"build.deployment"` selects that page id even though the page is not built natively; `selected_page` returns it.
10. **Report counts.** `deploy_delete_remote` reports `files: 1` for one file, so the title is "Deleted 1 files from Mounted".
11. **Layer order.** Two `deferred(..).with_priority(1)` layers paint in child order (the same assumption as the conflicts notes).

## 7. Tests most likely to fail first

- `workbench::deployment::tests::the_remote_host_window_lists_the_server_and_its_menu_changes_its_files`. It is long and drives seven menus by keyboard, and it types into prompts after `select_all`. The right click below the rows assumes the tree area reaches the bottom of the tool window.
- `workbench::deployment::tests::sync_with_chooses_the_server_saves_the_edited_file_and_synchronizes_the_selected_row`. It depends on the save order (`BeforeSynchronize`, then `SaveDocuments`) and on the refreshed row being skipped or hidden (hypothesis 4).
- `workbench::deployment::tests::open_in_browser_opens_the_deployed_url_of_the_file` (hypothesis 3).
- `workbench::deployment::tests::deployment_configuration_opens_the_deployment_settings_page` (hypothesis 9).
- `deployment::sync_dialog::tests::filters_by_name_and_category_navigates_with_arrows_and_cycles_operations_with_space`. The keys reach the table only through the claims (hypothesis 7). The filter is typed with `simulate_input` after focusing the field.
- Every `workbench::deployment` test, if hypothesis 1 fails: the servers would not be stored.

## 8. Found in the existing models (not edited)

`features_b::deployment::store::{SecretAnswer, SecretForm}` derive `Debug` over the plain-`String` secret (`store.rs`, the structs after `SecretPrompt`), so the secret would show in a `Debug` print. The new code does not use them: the secret travels as `database::store::SecretText`, whose `Debug` is redacted. Give both structs a redacting `Debug` (as `SecretText` has), or drop them.
