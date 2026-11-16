# Settings pages: integration notes

Written without a compiler. Only the first main page (`build.deployment`) is drafted; SSH Configurations, Run Targets and Project Structure are not started (see "Where this stopped"). Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`.

## Files written (all new, untracked)

- `app-gpui/src/settings_view/pages/build_deployment.rs` (616 lines): `DeploymentServersView` (server list, toolbar, tabs, staging), `Connections` / `DeploymentConnections` / `connections()` / `backend_connections()`, `servers()`, `TestState`, `ServerTab`. Declares its own child modules.
- `app-gpui/src/settings_view/pages/build_deployment/connection.rs` (487): the Connection tab, Test Connection, Trust Host Key and Retry, the password/passphrase field and Connect.
- `app-gpui/src/settings_view/pages/build_deployment/paths.rs` (296): the Mappings table (with local Browse…) and the Excluded Paths list, which is the shared `RecordTable` (`pages/tools_record_table.rs`) over the selected server's paths (`set_source` on another server, as `build_cmake.rs` does for a profile's environment).
- `app-gpui/src/settings_view/pages/build_deployment/proofs28_deployment_view.rs` (327): 7 view tests on a dialog model, backend stood in.
- `app-gpui/src/workbench/tests/proofs28_settings_deployment.rs` (313): 4 end-to-end tests in a project window.

## (a) Hunks, in order — group "Deployment" (land together)

### 1. `app-gpui/src/settings_view/pages/mod.rs`

Anchor (exact):

```rust
pub mod build_cmake;
```

Add right after it:

```rust
pub mod build_deployment;
```

### 2. `app-gpui/src/settings_view/custom_controls.rs`, inside `install`

Anchor (exact, the three lines just before the insertion):

```rust
    let tools = binding(pages::tools_external_tools::PAGE);
    let view = cx.new(|cx| pages::tools_external_tools::ExternalToolsView::new(tools, cx));
    slots.put(pages::tools_external_tools::PAGE, pages::tools_external_tools::SLOT, &view, cx);
```

Add right after it:

```rust

    // Build, Execution, Deployment › Deployment: the servers, with the project's root for the
    // local folders of the mappings.
    let deployment = binding(pages::build_deployment::PAGE);
    let project_root = cx
        .try_global::<Backend>()
        .and_then(|backend| backend.host.project(install.window_label).ok())
        .map(|project| project.root.to_string_lossy().into_owned());
    let connections = pages::build_deployment::connections(cx);
    let view = cx.new(|cx| {
        pages::build_deployment::DeploymentServersView::new(deployment, project_root, connections, cx)
    });
    slots.put(pages::build_deployment::PAGE, pages::build_deployment::SLOT, &view, cx);
```

`Backend` is already imported in that file (`use crate::backend::Backend;`). `host.project(label)` and `.root` are used exactly so by `pages::vcs_mappings::project_mapping`.

### 3. `app-gpui/src/workbench/mod.rs`, inside `mod tests`

Anchor (exact):

```rust
    mod proofs24_e2e_notebook;
```

Add right after it:

```rust
    mod proofs28_settings_deployment;
```

(The list I read has no `proofs25`–`proofs27` lines although those files exist; another agent may be adding them. Any place in the `mod` list works.)

Nothing else: `build_deployment.rs` declares `mod connection; mod paths; #[cfg(test)] mod proofs28_deployment_view;` itself.

Optional (not required for any row): add `(pages::build_deployment::PAGE, …)` to the reset/cancel table of `settings_view::custom_controls::tests::on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control` (around `custom_controls.rs:890`); I did not read that table's row type, so no hunk is given. The e2e `deployment_reset_and_cancel_discard_a_server_added` covers Reset and Cancel already.

## (b) Rows

| Key                                                                                     | Expected                                                            | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Missing                                                                                                                                                          |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.deployment` (new key, same form as `settings:build.deployment.options`) | partial                                                             | `workbench::tests::proofs28_settings_deployment::deployment_a_local_server_tested_by_the_backend_is_written_to_the_project`, `…::deployment_without_a_project_writes_the_server_to_the_global_settings`, `…::deployment_reset_and_cancel_discard_a_server_added`, `…::deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown`, `settings_view::pages::build_deployment::proofs28_deployment_view::servers_are_added_copied_made_default_and_removed`, `…::the_connection_fields_stage_and_ssh_configurations_are_offered`, `…::test_connection_shows_the_answer_for_the_server_as_edited`, `…::an_unknown_host_key_is_trusted_and_the_test_runs_again`, `…::a_missing_password_is_typed_as_bullets_and_handed_to_the_backend`, `…::mappings_are_edited_added_removed_and_a_local_folder_is_chosen`, `…::excluded_paths_are_added_edited_and_removed` | the remote folder chooser (Root path Browse…, Deployment path Browse…), the local chooser's start folder and title, the secret field's autofocus, a real spinner |
| `ipc:deploy_test_connection` (new key)                                                  | done                                                                | `workbench::tests::proofs28_settings_deployment::deployment_a_local_server_tested_by_the_backend_is_written_to_the_project`: "Connected successfully", "Folder does not exist: <tmp>/missing" and "Not a folder: <tmp>/plain.txt" can only come from `ide_host::ops::deploy::deploy_test_connection(&host, FsConnection::Local, path)`, the call `src-tauri/src/commands/deploy.rs:144-150` makes with the same arguments (the page passes `deployment_connection(server)` and the root path trimmed, `~` when empty, as `deployment.tsx:293`)                                                                                                                                                                                                                                                                                                                         | —                                                                                                                                                                |
| `ipc:ssh_set_secret` (new key)                                                          | done (Hypothesis: rests on the vault behaviour named in (e))        | `…::deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown`: `host.ssh.secret(&config)` returns the password and the only file holding it is `ssh-passwords.json.vault`; both come only from `SshManager::set_secret(id, Some(secret), save)`, which `src-tauri/src/commands/ssh.rs:29-31` calls                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | the command is also called by `ssh.tsx`'s Apply (via `staging.ts:368`), `runTargets.tsx:98`, `deployment/ssh.ts:28`; those callers are other rows                |
| `ipc:ssh_trust_host`                                                                    | stays open                                                          | only `…::an_unknown_host_key_is_trusted_and_the_test_runs_again` with a stand-in (no network)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | a host key to trust exists only after a real SSH handshake                                                                                                       |
| `unit:src/features/settings/pages/deploymentModel.test.ts`                              | already done (`settings_model::deployment_model::tests`), untouched | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | —                                                                                                                                                                |

No `unit:` spec tests `deployment.tsx` itself (measured: `grep -rln` over `src/features/settings/__tests__` and `src/features/deployment/*.test.*` finds no test naming the page component).

## (c) Behaviours of `src/features/settings/pages/deployment.tsx`

Built:

- List of servers, name or id shown, `default` badge on the effective default — `:601-621`; empty text "No servers. Click + to add one." `:602-606`.
- Add (new SFTP server, one empty mapping, selected, Connection tab) `:566-571`, `:584`; Copy `:586-592`; Remove with the next selection `:572-578`, `:585`; Use as Default, disabled for the default `:593-599`; the default kept valid after every change `:556-562`.
- No server selected: "Add an SFTP server to upload, download and synchronize project files." `:641-645`.
- Tabs Connection / Mappings / Excluded Paths `:626-639`; the Connection tab's test result and typed secret start over on another server or when the tab is left (it is mounted per server and only while shown, `:637-638`); an answer that comes back after that is dropped.
- Connection: Name `:302`; Type SFTP / Local or mounted folder `:303-313`; SSH configuration from the global `ssh.configs`, `(missing)` choice, "No SSH configurations" disabled choice, detail `user@host:port` (`components/SshConfigSelect.tsx:25-32`) `:314-321`; Configure… opens `tools.sshConfigurations` `:317-319`; Root path with placeholder "Folder path" / "Login folder" and hint `:322-336`; Web server URL with placeholder `:337-342`.
- Test Connection: disabled without SSH configuration (SFTP) or root path (local) or while running `:343-350`; runs with the server as edited and `~` for an empty root `:289-298`; result kept per type, SSH configuration and root path `:285-286`; "Connected successfully" `:352-356`; local error text `:362`; SFTP error with `ConnectionProblem` `:358-361`.
- `ConnectionProblem`: message `:62`; Trust Host Key and Retry `:63-69`; password or passphrase field (masked), Save, Connect disabled while empty or busy, Enter submits `:70-92`; failure text `:93`; retry after the step `:48-59`. Remedy from `sshTestRemedy` with the configuration's auth (default `openSsh`) `:41-43`.
- Mappings: + / − `:415-436`; columns and placeholders `:381-385`, `:437-444`; empty text `:446-452`; row selection on press `:453-460`; local Browse… disabled without a project, a folder inside the project made relative `:404-411`, `:473-481`; note `:499-505`.
- Excluded Paths: `GlobListEditor` (`components/Form.tsx:283-363`): + edits a new row, Enter commits, trims, keeps each once, an edit to nothing removes, − removes, Up/Down/Enter/F2/Delete/Backspace on the list; placeholder `/var/www/app/cache`, empty text "No excluded paths"; note `:531-541`. Drawn by `RecordTable`; one difference: `RecordTable` takes Escape on the list as the dialog's Cancel (`tools_record_table.rs:8-10`), `GlobListEditor` leaves it to the dialog, which cancels as well.

Left out (the row stays partial):

- `RemoteFolderDialog` (`:101-237`) and its two openers: Root path Browse… (`:332-334`, `:364-374`) and Deployment path Browse… (`:482-490`, `:506-517`). Neither button is drawn. It lists folders through `remote_list` over SFTP or the local disk; a view of its own (path field, Parent Folder, Refresh, folder list with keyboard, the connection problem inside) is about 400 lines.
- The local chooser's start folder (`defaultPath`, `:407`) and title "Choose Folder": `ControlEvent::Browse` has no start path or title (`pages/mod.rs:188`, `custom_controls.rs:340-353` with `prompt: None`).
- `autoFocus` of the password field (`:80`): focusing needs the window, which the test's answer callback does not have.
- `Spinner` (`:351`): drawn as a muted `refresh` icon; no spinner exists in the crate (measured: `grep -rn "fn spinner"` found nothing).
- `aria-label`s and `title` tooltips other than the toolbar's.

## (d) Hypotheses (not settled without a compiler)

0. Not read although the brief named them: `app-gpui/src/deployment/secret_dialog.rs`, `app-gpui/src/features_b/deployment/store.rs`, the tests of `app-gpui/src/workbench/deployment.rs`, `app-gpui/src/settings_view/pages/languages_sdk.rs`. Hypothesis: nothing here depends on them (the page uses `SecretField` directly, as `general_http_proxy.rs:241-256` does, and `SecretText` from `database/store.rs:72-104`, both read).

1. Hypothesis: `Context::listener` takes `&self`, so the nested `cx.listener` calls inside `map` closures borrow as in `tools_external_tools.rs`. Settle: compile.
2. Hypothesis: `fn(&mut Self, &mut Context<Self>)` (in `connection.rs` `action_button`) accepts `Self::run_test` / `Self::trust_and_retry` / `Self::connect`. Settle: compile; else take `impl Fn(&mut Self, &mut Context<Self>) + 'static`.
3. Hypothesis: `theme::icon(..).into_any_element()` compiles (used for the busy mark). Settle: compile.
4. Hypothesis: `Backend::with_host` accepts the generic `on_backend` closure (`W: FnOnce(Arc<Host>) -> F + Send + 'static`), as its signature at `backend.rs:331-336` reads. Settle: compile.
5. Hypothesis: with no project open the dialog's scope for a `project_first` page is Global (`deployment_without_a_project_…` asserts it). Settle: run the test.
6. Hypothesis: `set_global("w1", …)` before the dialog opens puts `ssh.configs` in the dialog's `global_base`, so the SSH configuration drop-down lists `box` as its first choice. Settle: run the SFTP e2e.
7. Hypothesis: `RemoteFs::Local` resolves an absolute folder to the same text it was given (`filesystem.rs:36-50`, `path_string`), so the e2e's "Folder does not exist: <path>" matches. Settle: run the test; on macOS the temp folder is canonicalized first for that reason.
8. Hypothesis: `tx_template` returns English text unchanged for messages like "Folder does not exist: …". Settle: run the test.
9. Hypothesis: clicking the center of a mapping cell's frame lands on the `TextField`, whose mouse-down focuses without stopping propagation (read: `text_field.rs:379-388`), so the row is selected too.
10. Hypothesis: `connections()` falls back to the real backend in test builds (unlike `general_http_proxy::checker`). The tests use only Local servers or the stand-in tester, and the SFTP e2e's configuration is never connected to. A future test with a real SFTP configuration and no stand-in would reach the network.

## (e) Tests most likely to fail first

- `deployment_a_local_server_tested_by_the_backend_is_written_to_the_project`: the exact error texts (d.7, d.8), and the scroll position: the Excluded Paths `+` and the mapping cell may be below the dialog's buttons; if `click_on` misses, use `proofs19_settings_misc::bring_into_view` (it is `pub(super)`).
- `deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown`: d.6 (the `sshConfig-0` choice), and whether `set_secret` with `save: true` writes the vault in test builds (`secrets.rs:28-40` says it does once `keep_secrets_in_files` was called; the proxy e2e relies on the same).
- `excluded_paths_are_added_edited_and_removed`: `RecordTable`'s new row gets the keyboard from `window.focus` inside the + click handler; the same flow is proven for the File Types table (`proofs19_settings_misc.rs:414-417`), so this is a low risk.
- `mappings_are_edited_added_removed_and_a_local_folder_is_chosen`: d.9.

- `deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown` asserts `ide_ssh::secrets::secrets_in_files()` right after `start(cx)`: the backend calls `keep_secrets_in_files()` only when `dirs.isolated` (`backend.rs:189-191`). Hypothesis: `AppDirs::under(home)` is isolated; if not, the assertion fails before any secret is saved, never reaching the keychain. The test ends with Cancel, so no SFTP server stays in the settings (and `deployment_settings_changed`, `workbench/deployment.rs:1323-1333`, connects nothing unless Remote Host was shown).

## (f) What a look at the real window must check

- Build, Execution, Deployment › Deployment shows the server list and, for a selected server, the three tabs; the `default` badge; the Type and SSH configuration drop-downs open over the page; the Root path's hint line; the mapping table fits the page width (three 170 px cells plus the Browse icon).
- Configure… opens Tools › SSH Configurations, which natively still has no editor (that page is not built).
- Machine sources the page reads in the real app: the global `ssh.configs` setting (no `~/.ssh/config` parsing by this page; an `openSsh` configuration's own connection reads it in the backend only on Test Connection); the project root of the window; Test Connection reaches the server named by the SSH configuration (SFTP) or the local disk (local server, writing and removing a probe file `.openidea-write-test-*` in the folder). Saved passwords: the system's credential store through `SshManager::set_secret` (keychain on macOS) in the real app, a vault file in test builds.

## Where the secret goes

`deployment.tsx:75` (`commands.sshSetSecret(ssh, secret, save)`) → `src-tauri/src/commands/ssh.rs:29-31` → `crates/ide-ssh/src/manager.rs:89-106`: kept for the session in `entered`, and with Save in `SecretFile` `ssh-passwords.json` (`crates/ide-host/src/lib.rs:130`), whose secrets are in the OS credential store, or in `<index>.vault` once `keep_secrets_in_files()` ran (`crates/ide-ssh/src/secrets.rs:28-40`). Natively: `SecretField` → `connection.rs` `connect` → `SecretText` → `backend_connections().set_secret` → the same `SshManager::set_secret`. Never staged, never in a settings file, never in `lines()`; `SecretText`'s `Debug` is redacted.

## (g) The small four

Not examined beyond this, because the pass stopped after Deployment: `grep -rn` for each id under `app-gpui/src` finds only the page specs (`settings_model/builtin_pages/features.rs:42,71,72,73`) and their order in `settings_model/builtin_pages.rs:206-219`; no slot, page view or test names `editor.todo`, `vcs.log`, `vcs.github` or `projectTrust`. Hypothesis: those pages draw no control natively today.

## Where this stopped

Deployment is drafted and its notes complete. Not started: `tools.sshConfigurations` (`ssh.tsx`, 396 lines; 11 `it(` lines in `__tests__/ssh.test.tsx`, while `INVENTORY.md:3194` counts 15 cases — probably `it.each` rows, not checked; Apply-time secret writes through `ApplyRun::next_secret` in `settings_model/staging.rs:589-690`), `build.runTargets` (`runTargets.tsx`, 632 lines), `projectStructure` (in `languages.tsx`; note `SettingsDialog` already has `project_structure_open` / `open_project_structure`, `staging.rs:208-212, 316-330`), and the small four. Nothing of them is half written.

## Corrections to the brief and to existing code

- Reading: one `grep` ran under `~/.cargo/registry/src/…/gpui-pre-0.3.12/src/` (for `click_count`) before I noticed the launch instruction "nothing under the user's home"; `wiring_brief.md:5` points there for GPUI signatures. The two instructions conflict; nothing else under the home was read.

- `src/platform/locales/text/en.json` does not hold the page's strings (775 lines; all 41 strings checked are absent). English text is the TSX literal itself; other catalogs key it (e.g. `pt.json:4300` "Connected successfully"), and the missing-choice template is `"{name} (missing)"` (`pt.json:4275`).
- The brief's "the password or passphrase field" of the Deployment page is the `ConnectionProblem` field after a failed SFTP test (`deployment.tsx:70-92`); the page has no stored password. It goes to the backend (`commands.sshSetSecret`, keychain via `SshManager`), never to the settings file.
- `ListEditor` (`pages/list_editor.rs:73-121`) binds to one settings key; `RecordTable` (`pages/tools_record_table.rs:23-26, 77, 109`) reads and writes through its owner, so Excluded Paths is a one-column `RecordTable` (`TableSpec::list`: trimmed, kept once, an edit to nothing removes, Up/Down/Enter/F2/Delete on the list). The Mappings table is drawn by the page because its cells are always-editable fields with a Browse… button in the local column (`deployment.tsx:461-491`), which `RecordTable` (edit one row at a time) does not draw.
