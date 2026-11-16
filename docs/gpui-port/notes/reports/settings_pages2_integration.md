# Settings pages, second pass: integration notes

Written without a compiler. Only `tools.sshConfigurations` (SSH Configurations) is drafted, completely. `build.runTargets` (Run Targets) is NOT started: nothing of it is written, no file exists for it, and `runTargets.tsx` was not read in this pass. Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`.

## Files written (all new, untracked)

- `app-gpui/src/settings_view/pages/tools_ssh.rs` (775 lines): `PAGE`, `SLOT`, `AUTHS`; the backend seam `SshTester` / `HostTrust` / `SecretCheck`, `SshChecks`, the global `SshPageChecks`, `checks(cx)`, `backend_checks()`, `backend_tester()`; `configs()`, `new_config()`, `shown_name()`, `auth_name()`; `TestState`; `SshConfigurationsView` (list, toolbar, sync, secret staging, stored-password question, Test Connection, Trust Host and Retry, Connect, Browse…, `lines()`). Declares `mod form;` and `#[cfg(test)] mod proofs29_ssh_view;` itself.
- `app-gpui/src/settings_view/pages/tools_ssh/form.rs` (305): the Connection section and its `form_lines()`.
- `app-gpui/src/settings_view/pages/tools_ssh/proofs29_ssh_view.rs` (555): 18 view tests (15 spec cases, 3 others) over a dialog model with a stand-in backend and a stand-in saved-password store.
- `app-gpui/src/workbench/tests/proofs29_settings_ssh.rs` (346): 6 end-to-end tests in windows with the real backend under a temporary home; each starts through `start_isolated`, which asserts that saved passwords are files under the temporary home and that the system's credential store is never set up, before any secret is read or written.

View: `SshConfigurationsView::new(binding: Binding, checks: SshChecks, cx)`. Emits `ControlEvent` (`Submit`, `Cancel`, `Browse { directories: false, .. }`). Accessors: `configs`, `current`, `field(name)` (`name`, `host`, `port`, `user`, `keyFile`), `secret_field`, `test_state`, `password_stored`, `port_invalid`, `can_test`, `lines`. Actions: `select`, `add(copy)`, `remove`, `choose_auth`, `run_test`, `trust_and_retry`, `connect`. Selectors: `settings-ssh-{add,remove,copy}`, `settings-ssh-config-<n>`, `settings-text-ssh.{name,host,port,user,keyFile}`, `settings-select-ssh.auth`, `settings-choice-ssh.auth-{password,keyPair,openSsh}`, `settings-ssh-browse-key`, `settings-secret-ssh.secret`, `settings-toggle-ssh.saveSecret`, `settings-ssh-test`, `settings-ssh-test-result`, `settings-ssh-trust`, `settings-ssh-connect`.

## (a) Hunks, in order — group "SSH Configurations" (land together)

No other group: Run Targets is not started.

### 1. `app-gpui/src/settings_view/pages/mod.rs`

Anchor (exact):

```rust
pub mod tools_record_table;
```

Add right after it:

```rust
pub mod tools_ssh;
```

### 2. `app-gpui/src/settings_view/custom_controls.rs`, inside `install`

Anchor (exact, the three lines just before the insertion, as the file reads today):

```rust
    let tools = binding(pages::tools_external_tools::PAGE);
    let view = cx.new(|cx| pages::tools_external_tools::ExternalToolsView::new(tools, cx));
    slots.put(pages::tools_external_tools::PAGE, pages::tools_external_tools::SLOT, &view, cx);
```

Add right after it:

```rust

    // Tools › SSH Configurations: the global SSH configurations, with Test Connection and the
    // saved passwords of the backend.
    let ssh = binding(pages::tools_ssh::PAGE);
    let checks = pages::tools_ssh::checks(cx);
    let view = cx.new(|cx| pages::tools_ssh::SshConfigurationsView::new(ssh, checks, cx));
    slots.put(pages::tools_ssh::PAGE, pages::tools_ssh::SLOT, &view, cx);
```

The Deployment group of the first pass (`$S/drafts/settings_pages_integration.md`, hunk 2) inserts after the same three lines. Landed in either order, both blocks go after those three lines; if Deployment is already in, put this block after Deployment's `slots.put(pages::build_deployment::PAGE, …);` line instead (any place inside `install` before `// Version Control › Directory Mappings` works).

### 3. `app-gpui/src/workbench/mod.rs`, inside `mod tests`

Anchor (exact, as read today; another agent is editing this list, so any place in the alphabetical `mod` list works):

```rust
    mod proofs26_save_all;
```

Add right after it:

```rust
    mod proofs29_settings_ssh;
```

Nothing else. No Apply code is added: `workbench/settings_window.rs` `apply_settings` already stores each staged credential (`apply_next_secret` → `store_ssh_secret` → `host.ssh.set_secret`), and `SettingsDialogView::modified_pages` / `reset` / `close` already treat staged secrets as edits of `tools.sshConfigurations` (`settings_view/dialog.rs:647-654`, `settings_model/staging.rs:526-533`).

Optional, not needed for a row: add `(pages::tools_ssh::PAGE, Act::Click("settings-ssh-add"), "ssh.configs")` to the table of `settings_view::custom_controls::tests::on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control` (its array length `12` becomes `13`). The e2e `ssh_reset_and_cancel_discard_a_configuration_added_and_a_password_typed` covers the same.

## (b) Rows

| Key                                                                                   | Expected                                                           | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Missing                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:tools.sshConfigurations` (new key, named as `settings:tools.externalTools`) | Hypothesis: done once the e2e pass                                 | `workbench::tests::proofs29_settings_ssh::ssh_a_configuration_added_and_edited_is_written_to_the_global_settings`, `…::ssh_reset_and_cancel_discard_a_configuration_added_and_a_password_typed`, `…::ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored`, `…::ssh_a_password_stored_before_shows_as_stored_and_an_unknown_one_does_not`, `…::ssh_test_connection_shows_the_backends_answer_for_the_configuration_as_edited`, `…::ssh_trust_host_asks_the_backend_for_the_configuration_tested`, and the 18 view tests below | every kind of control is changed with the mouse or keyboard (toolbar, list row, five text fields, drop-down, masked field, checkbox, Test Connection, Trust, Connect; Browse… is answered in a view test, the system chooser cannot be driven); not built: the chooser's title "Private Key File" (`ControlEvent::Browse` has no title, `custom_controls.rs:341`) |
| `unit:src/features/settings/__tests__/ssh.test.tsx` (new key)                         | done, 15 of 15 cases                                               | see the case table below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | — (the Apply-failure case asserts the model's error `SSH secret: Unavailable`, which the footer shows after `Could not save: ` (`dialog.rs:698-702`); the footer itself is not drawn in a view test)                                                                                                                                                              |
| `ipc:ssh_test` (new key)                                                              | done                                                               | `…::ssh_test_connection_shows_the_backends_answer_for_the_configuration_as_edited`: the text `cannot connect to SSH host 127.0.0.1:<port>` comes only from `ide_ssh::session::connect` reached through `SshManager::test(&config, secret)`, which is what `src-tauri/src/commands/ssh.rs:16-24` calls with the configuration as edited (the port typed, not saved) and the secret typed                                                                                                                                                                  | —                                                                                                                                                                                                                                                                                                                                                                 |
| `ipc:ssh_needs_secret` (new key)                                                      | done                                                               | `…::ssh_a_password_stored_before_shows_as_stored_and_an_unknown_one_does_not` (and the Apply test): "Password already stored" shows only when `SshManager::needs_secret(&SshManager::config(id)?)` answers false, the call of `commands/ssh.rs:33-38`                                                                                                                                                                                                                                                                                                    | —                                                                                                                                                                                                                                                                                                                                                                 |
| `ipc:ssh_set_secret` (new key; the Deployment draft proposes it too)                  | done                                                               | `…::ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored`: `host.ssh.secret(&config)` returns the password and the only file holding it is `ssh-passwords.json.vault`; only `SshManager::set_secret(id, Some(secret), save)` (`commands/ssh.rs:27-31`) puts it there, called by the existing Apply path (`settings_window.rs:161-165`)                                                                                                                                                                                        | other callers (`runTargets.tsx:98`, `deployment.tsx:75`, `deployment/ssh.ts:28`, `run/remote.ts:151`) are other rows                                                                                                                                                                                                                                              |
| `ipc:ssh_trust_host` (new key)                                                        | partial                                                            | `…::ssh_trust_host_asks_the_backend_for_the_configuration_tested`: the real `SshManager::trust_host("box")` answers "no host key to trust", shown by the page; proves the page reaches the backend's trust with a configuration id, not that a key is trusted                                                                                                                                                                                                                                                                                            | a pending host key exists only after a real SSH handshake, which would read the user's `known_hosts`; also called by `runTargets.tsx:91`, `deployment.tsx:65`, `SourceDialog.tsx:468`, `deployment/ssh.ts:24`, `run/remote.ts:150`                                                                                                                                |
| `unit:src/features/settings/__tests__/sshConnection.test.ts`                          | already `[m]` (`settings_model::ssh_connection::tests`), untouched | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | —                                                                                                                                                                                                                                                                                                                                                                 |
| `settings:build.runTargets`                                                           | not started                                                        | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | everything                                                                                                                                                                                                                                                                                                                                                        |

Measured: of these keys only `unit:src/features/settings/__tests__/sshConnection.test.ts`, `ipc:ssh_connected` and `ipc:ssh_disconnect` exist in `docs/gpui-port/parity.json`; the page, unit and four `ipc:` keys above are new (`INVENTORY.md:1584`, `:3194`, `:2401-2405` list them unticked). Only `ssh.tsx` calls `sshTest` and `sshNeedsSecret` (grep of `src/`), so those two rows are this page's alone.

Other specs that name SSH: `src/features/settings/__tests__/staging.test.ts` "SSH secret staging" (`:156-230`) is already covered by `settings_model::staging::tests` (`writes_only_the_final_value_once_after_settings`, `saves_a_new_configuration_before_its_credential`, `cancel_discards_the_credential_without_writing_it`, `reset_clears_only_this_pages_credentials_and_an_empty_field_leaves_storage_untouched`, `retains_a_failed_write_for_retry_without_reapplying_successful_settings`, `does_not_write_credentials_when_settings_fail`, `does_not_write_a_credential_for_a_configuration_removed_before_apply`); untouched. `dialog.test.tsx` has no SSH case (grep for `ssh`/`SSH`: none); its secret-only Apply/Reset case is `settings_view::dialog::tests::enables_apply_and_reset_for_a_secret_only_edit_and_reset_discards_it`, untouched. Its Run Targets case (`INVENTORY.md:3173`, "Build › Run Targets has no view (runTargets.tsx:471)") still waits.

Existing tests that could expect the page to have no editor (measured by grepping `UNAVAILABLE`, `not available yet` and `SSH_SETTINGS_PAGE` in `app-gpui/src`): none. The page sweep `smoke_settings_pages.rs:110-118` accepts a page made of controls of its own once each is drawn (`SSH Configurations: <custom>`); `workbench/mod.rs` `settings_for_new_projects_and_plugins_open_on_their_pages` checks the Plugins page only; `dialog.rs:1646-1659` builds its own pages.

Case table of `ssh.test.tsx` (each in `settings_view::pages::tools_ssh::proofs29_ssh_view::`; an `it.each` table is a loop in one test):

| Case (file:line)                                                                                     | Native test                                                                                             |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| shows the %s secret row before its Save checkbox (`ssh.test.tsx:75-92`)                              | `shows_the_secret_row_before_its_save_checkbox` (password and key pair, one window, two configurations) |
| shows no secret or Save rows for OpenSSH (`:94-103`)                                                 | `shows_no_secret_or_save_rows_for_openssh`                                                              |
| stages a secret-only edit until Apply with save=%s (`:105-121`)                                      | `stages_a_secret_only_edit_until_apply` (save off and on)                                               |
| Cancel discards a typed credential without storing it (new=%s) (`:123-131`)                          | `cancel_discards_a_typed_credential_without_storing_it`                                                 |
| uses the latest Save choice only when applying, including after a test (`:133-145`)                  | `uses_the_latest_save_choice_only_when_applying_including_after_a_test`                                 |
| treats an emptied field as no new credential for Apply and Test Connection (`:147-160`)              | `treats_an_emptied_field_as_no_new_credential_for_apply_and_test_connection`                            |
| tests the staged passphrase without storing it when the connection fails (`:162-172`)                | `tests_the_staged_passphrase_without_storing_it_when_the_connection_fails`                              |
| reuses the existing input after a missing or rejected password (typed=%s) (`:174-194`)               | `reuses_the_existing_input_after_a_missing_or_rejected_password` (asserts the field has the keyboard)   |
| keeps the typed password when trusting a host and retrying (`:196-209`)                              | `keeps_the_typed_password_when_trusting_a_host_and_retrying`                                            |
| shows an existing password placeholder and leaves it untouched when nothing is typed (`:211-221`)    | `shows_an_existing_password_placeholder_and_leaves_it_untouched_when_nothing_is_typed`                  |
| does not interpret an existing key pair as an already stored password (`:223-231`)                   | `does_not_interpret_an_existing_key_pair_as_an_already_stored_password`                                 |
| reports an Apply failure and keeps the field for retry (`:233-245`)                                  | `reports_an_apply_failure_and_keeps_the_field_for_retry`                                                |
| retains typed values when switching configurations, and drops them on deletion or Reset (`:247-262`) | `retains_typed_values_when_switching_configurations_and_drops_them_on_deletion_or_reset`                |
| clears the displayed secret on Reset (`:264-270`)                                                    | `clears_the_displayed_secret_on_reset`                                                                  |
| discards a staged secret on authentication changes (`:272-284`)                                      | `discards_a_staged_secret_on_authentication_changes`                                                    |

Not from a spec: `configurations_are_added_edited_copied_and_removed`, `placeholders_follow_the_authentication_and_browse_chooses_the_key_file`, `test_connection_needs_a_host_and_a_test_build_has_no_tester`.

## (c) Behaviours of `src/features/settings/pages/ssh.tsx`

Built:

- The configurations are the global `ssh.configs` whatever the dialog's scope (`useSetting(..., "global")`, `:288`); natively read with `staged_value(Scope::Global, …)` and staged with `Scope::Global` (`tools_ssh.rs` `configs`, `commit`).
- Toolbar Add / Remove / Copy, Remove and Copy disabled without a selection (`:317-327`); Add: defaults, `SSH <n+1>`, fresh `ssh-…` id, selected (`:299-305`, `newRemoteId` `remoteSettings.ts:241-243`); Copy: `<name or label> (copy)` (`:301`); Remove: forgets the secret typed, selects the one now in its place else the last (`:306-312`).
- List: empty text (`:329-333`); rows selected on mouse down (`:340`); name, else `sshLabel`, else id (`:342-344`); the label column (`:345-347`); first configuration selected at open (`:290`).
- Form keyed by `id:auth` (`:352`): test result, Port text and stored answer start over on either change (`sync` in `tools_ssh.rs`).
- Name (`:235`); Host with placeholder by auth (`:79`); Port with placeholder `22`, flagged while not a port, staged as `parseSshPort` (null when invalid) (`:71`, `:85-95`); User name with OpenSSH placeholder (`:237-242`); Authentication type drop-down with the three labels (`:25-29`, `:243-250`); auth change discards the staged secret (`:296`).
- Private key file with mono field, placeholder, Browse… (`:102-133`), only for key pair (`:251`).
- Password / Passphrase masked field, value = staged secret, typing stages it, emptying unstages it (`:252-268`, `staging.ts:318-325`); placeholder "Password already stored" from `ssh_needs_secret`, asked only for a password configuration saved as one with nothing staged, again when the staged secret goes (`:214-231`, `:264`).
- OpenSSH note instead of the checkbox (`:269-274`); Save password / Save passphrase checkbox (`:276-280`).
- Test Connection disabled while running or with an empty host (`:175`); sends the configuration as edited and the staged secret, none for OpenSSH (`:151`); "Connecting…" / result / error text coloured (`:178-183`); remedy from `sshTestRemedy` (`:155`); the secret field gets the keyboard on a secret remedy (`:157`, `:282`); Trust note and "Trust Host and Retry" (`:185-194`), trust failure shown with no remedy, success tests again (`:160-169`); Connect for a non-OpenSSH secret remedy (`:195-199`).
- The shared note (`:353-359`).
- Apply / Cancel / Reset of the secret: existing staging (`staging.rs:480-533`, `588-690`) and Apply (`settings_window.rs:322-330`).

Left out:

- Browse…'s title "Private Key File" (`:107`): `ControlEvent::Browse` carries none (`pages/mod.rs:188`, `custom_controls.rs:340-353`).
- Browse… disabled outside Tauri (`:127`): the native app always has a chooser.
- `aria-label`s (`:126`, `:248`, `:317`, `:328`) and `title` tooltips other than the toolbar's.

Different by construction (Hypothesis that it matters to no one): the stored-password question is asked when the dialog opens (every page's view is built by `install`), not when the page is first shown.

## (d) Hypotheses (not settled without a compiler)

1. Child modules see the parent's private items and private `use` imports through `use super::*;` (`proofs29_ssh_view.rs` uses `KEY`, `Then`, `SecretText`, `Rc`, `Focusable` from `tools_ssh.rs`). Settle: compile.
2. Closure parameter types of the `Rc<dyn Fn…>` stand-ins are written out; `on_backend`'s `T` is inferred from the future (`IdeResult<String>`, `IdeResult<()>`, `IdeResult<bool>`). Settle: compile.
3. `cx.weak_entity()` and `cx.defer` work inside `SshConfigurationsView::new` (the first `sync` may ask the stored-password question). Settle: the placeholder view test.
4. `AnyWindowHandle::update(cx: &mut App, …)` succeeds in the test answer: the stand-in answers inside a `cx.defer`, which runs after the click's window update has returned the window. Settle: `reuses_the_existing_input_after_a_missing_or_rejected_password` (the focus assertion). If it fails, keep a flag and focus in `render` through `window.on_next_frame`.
5. `TextField::set_text` and `SecretField::set_text` emit no `Changed` (the tests brief says so for `TextField`; `SecretField::set_text` says "The owner is not told", `secret_field.rs:83-87`), so `sync` does not stage what it shows.
6. `crate::settings_view::controls::section` (`controls.rs:457`) and `pages::select_box` / `drop_down` / `drop_down_row` / `icon_button` / `frame` are used as their compiled callers use them (`editor_font.rs:228-250`, `tools_external_tools.rs`, `languages_sdk.rs:724`).
7. The SSH page's dialog scope is Global with a project open (its spec has no `project_scope`, `builtin_pages/tools.rs:98-121`); the e2e asserts it.
8. The global `ssh.configs` reads `[]` after Cancel when nothing was ever stored (`setting_of(&global(cx), …)` of the default `SshSettings`).
9. A connection accepted and dropped by a loopback listener fails in `russh::client::connect`, mapped to `cannot connect to SSH host 127.0.0.1:<port>: …` (`crates/ide-ssh/src/session.rs:568-582`) before any host key or file is read (`resolve_hops` reads no config for Password, `target.rs:513-527`).
10. `SshManager::trust_host` with no pending key returns `no host key to trust` before touching `known_hosts` (`manager.rs:173-177`, read).
11. `simulate_input` reaches the `SecretField` once it is focused through `window.focus` (as the proxy and deployment tests do); the Apply e2e instead presses the field (`settings-secret-ssh.secret`, whose `SecretField` root is `w_full`, `secret_field.rs:305-315`) and relies on its mouse-down focus.
12. `ide_core::error::ErrorKind` (compiled use: `project_tree.rs:25`) and `AnyView::downcast().ok()` (compiled use: `custom_controls.rs:947`) are used as there.

## (e) Tests most likely to fail first

- `reuses_the_existing_input_after_a_missing_or_rejected_password`: hypothesis 4 (the focus).
- The e2e clicks on rows low in the page (`settings-ssh-test`, `settings-ssh-trust`, `settings-toggle-ssh.saveSecret`): the dialog's page may need scrolling; if `click_on` misses, scroll with `proofs19_settings_misc::bring_into_view` (`pub(super)`).
- `ssh_test_connection_shows_the_backends_answer_for_the_configuration_as_edited`: the exact prefix (hypothesis 9), and whether the error mentions the port typed (the configuration as edited is sent; the saved one has no port).
- `configurations_are_added_edited_copied_and_removed`: whether `type_over` delivers `70000` char by char (`7000` is staged on the way, then none), and the `(invalid)` line.
- `ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored`: the second `needs_secret` after Apply (it runs when the staged secret goes; `global_base` must still hold the password configuration).

## (f) What a look at the real window must check, and the machine sources

- Tools › SSH Configurations: the list box with + − copy, the Connection section below with Host and Port on one row, the drop-down opening over the page, the masked field, Save checkbox / OpenSSH note, Test Connection and its coloured status; Deployment's Configure… now lands on a page with an editor.
- Machine sources in the real app: the global settings file (`ssh.configs`); the backend's saved passwords (the OS credential store: keychain on macOS) through `ssh_needs_secret` when the dialog opens; Test Connection reaches the host typed over the network and, with OpenSSH, reads `~/.ssh/config` (and may run its `Match exec`), the SSH agent and the default identity files; every handshake reads `~/.ssh/known_hosts`; Trust Host and Retry writes `~/.ssh/known_hosts`; a key pair reads the key file named. The page lists no hosts of the machine (no `~/.ssh/config` host list).

## Where the secrets go

`ssh.tsx:265` `stageSshSecret` → dialog state → Apply `staging.ts:368` `commands.sshSetSecret(id, secret, save)` → `src-tauri/src/commands/ssh.rs:27-31` → `SshManager::set_secret` (`crates/ide-ssh/src/manager.rs:89-106`): kept for the session, and with Save in the saved passwords `ssh-passwords.json` (`crates/ide-host/src/lib.rs:130`), whose values are in the OS credential store (a `.vault` file once `keep_secrets_in_files()` ran, `backend.rs:190-191`). Never in a settings file. Natively: `SecretField` (bullets) → `SettingsDialog::stage_ssh_secret` (a `Secret` with a redacted `Debug`) → the existing Apply → `store_ssh_secret` → the same `SshManager::set_secret`; for Test Connection only, a `SecretText` (redacted `Debug`) handed to `SshManager::test`. Tests stand in for the keychain with the isolated home's vault file (the backend of `start()` is under a temp home, `keep_secrets_in_files` asserted), as the proxy and deployment tests do.

## (g) What the SSH page and Deployment's Connection tab should share once both compile

- One backend seam: Deployment's `HostTrust` and SSH's `HostTrust` are the same type (`Rc<dyn Fn(String, Then<Result<(), IdeError>>, &mut App)>`) and both `on_backend` helpers are the same function except for the result type. Hunk once both compile: move `on_backend<T, F, W>` (this page's generic version), `HostTrust` and a `backend_trust_host()` into a new `app-gpui/src/settings_view/pages/ssh_backend.rs` (`pub mod ssh_backend;` in `pages/mod.rs`), and make `build_deployment::Connections::trust_host` and `tools_ssh::SshChecks::trust_host` use it.
- One remedy rule: both call `settings_model::ssh_connection::ssh_test_remedy` with the configuration's auth; Deployment defaults to OpenSSH when the configuration is missing, as `deployment.tsx:41-43` does. Keep both calls; nothing to move.
- Not shared: the secret. Deployment's Connect hands the secret straight to `set_secret` (its webview does, `deployment.tsx:75`); this page stages it until Apply (`ssh.tsx:265`). A shared "secret field + Save + Connect" block would have to take both behaviours, so leave them apart.

## Corrections to the brief and to existing code

- Strings: `src/platform/locales/text/en.json` holds none of "Trust Host and Retry", "Password already stored", "Connecting…", "No SSH configurations. Click + to add one." (measured by grep); the English text is the literal of `ssh.tsx`, passed through `controls::tx` as the neighbours do.

- The brief says the masked field's value is stored "where the webview stores them … the keychain through the backend, or the settings file": it is the keychain through the backend, and natively the Apply path for it already exists (`settings_window.rs:161-165`, `322-330`); the page only stages.
- `SshFiles::default()` (`crates/ide-ssh/src/target.rs:37-42`) is the user's real `~/.ssh`; the host's `SshManager` is built with it even under `AppDirs::under` (`crates/ide-host/src/lib.rs:130`). The Deployment draft's `connections()` falls back to the real backend in test builds, so a future test of an SFTP or OpenSSH server without a stand-in would read the user's `~/.ssh/config` and `known_hosts`. This page's `checks()` gives a test build no tester unless a test sets one, as `general_http_proxy::checker` does.
- `INVENTORY.md:3194` counts 15 cases for `ssh.test.tsx`: 11 `it` plus 4 `it.each`, each table counted once (measured by reading the file).

## Run Targets: not started

No file, hunk or test of `build.runTargets` exists from this pass, and `runTargets.tsx` was not read. What was measured on the way, for the next writer: `app-gpui/src/settings_model/run_targets.rs` (130 lines) already holds the page spec (`run_targets_page()`, `PageSpec::new("build.runTargets", "Run Targets", 364)`) and its unit row `unit:src/features/settings/__tests__/runTargets.test.ts` is ticked (`INVENTORY.md:3192`, `settings_model::run_targets::tests`); `runTargets.tsx:91` and `:98` call `sshTrustHost` and `sshSetSecret` (so the SSH seam of `tools_ssh.rs` can serve it); `wsl_distros`, `wsl_resolve` and the `docker_*` commands are unticked `ipc:` rows (`INVENTORY.md:2097-2113`, `:2548-2549`); `dialog.test.tsx`'s Run Targets case waits on this view (`INVENTORY.md:3173`).
