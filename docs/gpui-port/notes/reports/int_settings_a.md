All three pages landed whole: SSH Configurations, then Deployment servers, then Run Targets. Every gate passed after the last code change. Nothing is committed and nothing was pushed.

## Part 0: the SSH files of a test backend

- **What changed:** an isolated backend (a test, or any run with `OPENIDEA_GPUI_HOME`) now uses `<home>/ssh/known_hosts` and reads no OpenSSH config. That comes from a new `AppDirs::ssh_files()` used in `Backend::start` (`app-gpui/src/backend.rs`). A normal run still gets `SshFiles::default()`.
- **Why `crates/` changed:** `Host.ssh` is `pub`, but `Host::new` hands a clone of it to `ide_remote::RemoteService` in `host.remote`. Rebuilding that from `app-gpui` would need `ide-remote`, which is not in `app-gpui/Cargo.toml`, so a new dependency. Read in `crates/ide-host/src/lib.rs` and `crates/ide-remote/src/service.rs:77`.
- **The seam:** `Host::with_ssh_files(config, ssh_files)` in `crates/ide-host/src/lib.rs`. `Host::new` is unchanged and calls it with `SshFiles::default()`, so the Tauri app behaves as before.
- **Crate test:** `crates/ide-host/tests/ssh_files.rs`, two tests:
  - `a_host_with_ssh_files_of_its_own_trusts_and_checks_host_keys_there`: an in-process sshd on the loopback address; `host.remote` is refused, then the key is recorded in the temp `known_hosts`, then `host.remote` connects.
  - `a_host_built_without_ssh_files_keeps_the_default_ones`.
- **Crate gates** (logs in `$S/dseti/`):
  - `cargo test -p ide-host` exit 0 (`ide_host_test.log`).
  - `cargo clippy -p ide-host --all-targets -- -D warnings` exit 0 (`ide_host_clippy.log`).
  - `cargo test -p openidea export_bindings` gave `test result: ok. 1 passed` (`export_bindings.log`).
  - `git status --short src/ src-tauri/` printed nothing.
  - The manager still has to run the whole Tauri workspace suite.
- **The native test:** `backend::tests::the_ssh_files_of_a_backend_under_a_temporary_home_are_under_that_home` passed on its own (`part0.log`) before any end-to-end test ran. The check it uses, `crate::backend::assert_ssh_files_under`, runs first in every slice test that touches SSH: `start_isolated` in proofs29, the SFTP test in proofs28, and two proofs30 tests.

## Safety, bullet by bullet

- **Network:** each test that presses Test Connection, and where it connects:
  - `ssh_test_connection_shows_the_backends_answer_…`: a listener the test binds on `127.0.0.1:0`, which accepts and drops. The test asserts host `127.0.0.1`, the port and password auth before the click. The path to the socket was read: `SshManager::test` → `connect` → `resolve_hops` (password auth builds the hop from host and port and reads no config, `target.rs:507-528`) → `connect_first_hop` → TCP.
  - `ssh_trust_host_asks_the_backend_…`: a stand-in tester. The real `trust_host` returns "no host key to trust" before touching any file (`manager.rs:172-177`).
  - Deployment local-folder tests: `FsConnection::Local` connects nowhere (`deploy.rs:172` → `service.rs:81-89`).
  - Deployment SFTP test: a stand-in tester. Its configuration is now `127.0.0.1` on a port bound and released by the test, and the address the backend holds is asserted.
  - Run Targets: SSH configs are now `127.0.0.1` on a released port. The folder browser is a stand-in, and `ide_docker::load_settings` is a plain file read of the temp data folder (`crates/ide-docker/src/connection.rs:162-170`).
  - All view tests use stand-ins. Nothing depends on DNS.
- **Keychain:** an isolated home calls `keep_secrets_in_files()` (`backend.rs`, `if dirs.isolated`). After that the OS store reports itself unavailable (`crates/ide-ssh/src/secrets.rs:80-84`). Tests assert `secrets_in_files()` and `!default_store_installed()` before typing a secret, and the Apply test asserts the only file holding the password is `ssh-passwords.json.vault`.
- **Never drawn:** `lines()` shows bullets only; the view tests assert the bullet text and that the typed secret appears in no line.
- **Never in a settings file:** after Apply/OK every file under the temp home is scanned and only the `.vault` file contains the secret.
- **Never logged:** only partly tested. The capturing subscriber is thread-local, so it covers the SSH Apply path, which runs `set_secret` on the main thread (`settings_window.rs:161-163`). It does not cover Deployment's `set_secret`, which runs on the backend runtime. For that one, by reading: `manager.rs:88-106` and the `tracing` lines in `secrets.rs` log ids and errors, never the value.
- **Never in `Debug`:** `SecretText`, staging's `Secret` and `SecretField` all have hand-written redacting `Debug`s (`database/store.rs:100-104`, `staging.rs:115-119`, `secret_field.rs:41-45`). The Deployment and SSH tests assert the dialog's `Debug` output omits the secret.
- **Secret memory on a closed page:** `deployment.tsx:40-95` keeps the secret in component state, so it goes when the page unmounts. Natively, all three pages now reset when the dialog shows another page:
  - Deployment drops the test result and the typed password. Asserted in `deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown` and in shot `dseti6-13`.
  - Run Targets closes the folder browser, and the password typed in it goes with it (asserted in the folder test).
  - SSH clears its test state. Its password stays staged in the dialog until Apply or Cancel, as `ssh.tsx:265` stages it.
- **User's SSH config and keys:** the page's configurations are the global `ssh.configs` (`tools_ssh.rs` `configs`). Nothing on that path reads `~/.ssh/config`, and test backends have `config: None` plus a temp `known_hosts`.
  - Hypothesis: OpenSSH and key-pair authentication would still reach the SSH agent and `~/.ssh/id_*` (`session.rs:603-660`). No test gets past the TCP handshake to authentication.
- **Validation:** Port is validated with `parse_ssh_port` as `ssh.tsx:85-95` does (view test and shot). No host, port or path becomes a shell command on these pages.
- **Real binary:** run only through `$S/insp8/run.sh`, with temp projects under `/private/tmp/openidea-inspect-*`, both removed. Only the local-folder Test Connection was pressed, and it left the folder empty. No SSH or SFTP Test Connection or Trust, no Browse… on Run Targets, no password typed, no Agents or Services window. The app was stopped by its PID.

## Gates (after the last change)

All logs are in `$S/dseti/final/`.

| Gate                                        | Log                           | Result                                                                                                                                |
| ------------------------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1                                 | `suite1.log`                  | `test result: ok. 7973 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 202.95s` (load 6.41 before, 117.94 after) |
| Suite run 2                                 | `suite2.log`                  | `test result: ok. 7973 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 203.50s`                                  |
| Slice tests ×20                             | `$S/dseti/loop20/summary.txt` | 20/20 runs `ok. 56 passed; 0 failed`; no test failed once                                                                             |
| `cargo test --test web_view`                | `web_view.log`                | `test result: ok. 1 passed`                                                                                                           |
| `cargo clippy --all-targets -- -D warnings` | `clippy.log`                  | rc 0                                                                                                                                  |
| `cargo build`                               | `build.log`                   | rc 0, 0 warnings                                                                                                                      |

The ×20 loop ran after suite run 2, not during it, to keep to one heavy cargo command at a time. An earlier full round of all these gates, before the page-reset change, was also green.

## Rows

New keys, named like `settings:tools.externalTools` and `settings:build.run`. Only `unit:…/dialog.test.tsx` already exists in `parity.json`. I did not write to it.

| Key                                                    | State         | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------ | ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:tools.sshConfigurations`                     | done          | `workbench::tests::proofs29_settings_ssh::{ssh_a_configuration_added_and_edited_is_written_to_the_global_settings, ssh_reset_and_cancel_discard_a_configuration_added_and_a_password_typed, ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored}` and the 18 view tests in `settings_view::pages::tools_ssh::proofs29_ssh_view`                                                                                                                                                                                                               |
| `unit:src/features/settings/__tests__/ssh.test.tsx`    | done, 15/15   | the 15 spec-named tests in `settings_view::pages::tools_ssh::proofs29_ssh_view`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `ipc:ssh_test`                                         | done          | `…proofs29_settings_ssh::ssh_test_connection_shows_the_backends_answer_for_the_configuration_as_edited`                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `ipc:ssh_needs_secret`                                 | done          | `…proofs29_settings_ssh::ssh_a_password_stored_before_shows_as_stored_and_an_unknown_one_does_not`                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `ipc:ssh_set_secret`                                   | done          | `…proofs29_settings_ssh::ssh_a_password_typed_is_stored_by_the_backend_on_apply_and_then_shown_as_stored` and `workbench::tests::proofs28_settings_deployment::deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown`                                                                                                                                                                                                                                                                                                                                  |
| `ipc:ssh_trust_host`                                   | partial       | `…ssh_trust_host_asks_the_backend_for_the_configuration_tested`. Missing: a real pending key, which needs an in-process sshd; `app-gpui` lacks `ide-ssh`'s `testing` feature and I did not add it                                                                                                                                                                                                                                                                                                                                                                         |
| `settings:build.deployment`                            | partial       | `workbench::tests::proofs28_settings_deployment::{deployment_a_local_server_tested_by_the_backend_is_written_to_the_project, deployment_without_a_project_writes_the_server_to_the_global_settings, deployment_reset_and_cancel_discard_a_server_added, deployment_an_sftp_password_typed_is_kept_by_the_backend_and_never_shown, deployment_shown_after_another_page_selects_the_projects_first_server}` and the 8 view tests in `settings_view::pages::build_deployment::proofs28_deployment_view`. Missing: the remote folder chooser of Root path and Deployment path |
| `ipc:deploy_test_connection`                           | done          | `…deployment_a_local_server_tested_by_the_backend_is_written_to_the_project` ("Connected successfully", "Folder does not exist", "Not a folder", probe file removed)                                                                                                                                                                                                                                                                                                                                                                                                      |
| `settings:build.runTargets`                            | done          | `workbench::tests::proofs30_settings_run_targets::{run_targets_an_ssh_target_added_and_edited_is_written_to_the_project, run_targets_a_docker_target_offers_the_services_connections_and_applies, run_targets_the_folder_chosen_in_the_browser_is_the_targets_project_folder, run_targets_reset_and_cancel_discard_a_target_added, run_targets_a_target_stored_without_its_upload_choice_shows_the_value_the_backend_loads, run_targets_shown_after_another_page_select_the_first_target}` and 11 view tests plus `form::tests::container_fields_follow_the_kind`         |
| `unit:src/features/settings/__tests__/dialog.test.tsx` | one more case | `settings_view::pages::build_run_targets::proofs30_run_targets_view::uses_the_attached_primary_name_for_run_target_paths`                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ipc:docker_settings`                                  | done          | `…run_targets_a_docker_target_offers_the_services_connections_and_applies`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |

Run Targets' Connect passes `save` = `false`, as `runTargets.tsx:98` does. This forgets a password saved for that configuration. It is asserted in `the_folder_browser_hands_a_typed_password_to_the_backend_and_never_shows_it` (`secret s1 typed save=false`).

## Other changes beyond the drafts

- **Shared module:** `settings_view/pages/ssh_backend.rs` holds `on_backend`, `HostTrust`, `SecretSetter`, `backend_trust_host()` and `backend_set_secret()`. All three pages use it, replacing three copies.
- **Deployment Test Connection:** it is now `Option<Tester>`, and a test build has none unless a test installs one, as on the SSH page. Proven by `a_test_build_has_no_test_connection_of_its_own`.
- **Deployment password field:** it takes the keyboard when it appears. Asserted in the view test.
- **SSH Trust Host and Retry:** the button was drawn outside the dialog (measured at x=1460 against OK's right edge at 1433). The row now wraps, as the webview's `inlineRow` does.
- **Page reset on show/hide:** measured in the real window, Deployment shown after another page had no server selected, because its view is built with the dialog. All three pages now reset when the dialog shows or leaves them.
- Hypothesis: the webview remounts the page only on page change, not on a scope toggle. Natively, the selection now follows the page shown, not the scope.

## Hypotheses, and which way they went

- **Compile risks in the notes:** all settled; the drafts compiled.
- **Focus after a secret remedy (SSH):** passed.
- **"Cannot connect" prefix:** passed.
- **Dialog-row clicks:** `settings-ssh-trust` needed scrolling into view and the row fix above.
- **`syncBeforeRun`:** settled, nothing to fix.
  - Measured natively: the backend loads a missing value as `false` and the page shows it unchecked.
  - Webview, by reading: its base is `commands.settingsGet()` (`project.ts:88`), the same typed settings, so also `false`. The draft's `read_target` default of `true` only applies to hand-staged values.
- **Strings:** English is the literal text, and other catalogs are keyed by it (`ko.json:2376`, `fr.json:2216`). The native pages go through `controls::tx` → `Localization::tx`, as their neighbours do.

## Left out

- **Remote folder chooser (Deployment, `deployment.tsx:101-237`):** not built. The natural base is Run Targets' `folder.rs` browser (435 lines), generalised to take `FsConnection::Local` as well as SFTP; that is over the 300-line bound.
- **Chooser title and start folder:** GPUI's `PathPromptOptions` has neither (`gpui-pre-0.3.12/src/platform.rs:2935-2944`).
- **Spinner:** the crate has none; Deployment shows a muted icon.

## Stale or false inputs found

- `app-gpui/src/settings_model/remote_settings.rs` does not exist; it is `app-gpui/src/platform_more/remote_settings.rs`.
- The notes' reason for `ipc:ssh_trust_host` being partial ("would read the user's `known_hosts`") no longer holds after Part 0.
- The SSH notes say Run Targets "not started", which is stale.
- The Run Targets notes put the `syncBeforeRun` fix in `ide-core`; no fix is needed.

## Shots

All under `$S/insp8/shots/`. Cropped copies are in `…/scratchpad/dseti/crops5/` and `…/scratchpad/dseti/crops6/`. I describe only the shots I opened.

- **SSH Configurations:**
  - `dseti1-06-ssh-empty-dark-1.png`: empty list, "No SSH configurations. Click + to add one." It is actually light; the `dseti1` labels lag one step.
  - `dseti2-07-ssh-added-dark-1.png`: dark, SSH 1 added with OpenSSH placeholders and the OpenSSH note.
  - `dseti3-03-ssh-auth-open-light-1.png`: the authentication drop-down open over the page.
  - `dseti5-14-ssh-keypair-dark-1.png`: dark, key pair with Private key file, Passphrase and the red Port.
- **Deployment:**
  - `dseti1-01-…png`: the empty Deployment page, light ("No servers…" and the SFTP note).
  - `dseti3-01-deployment-local-connection-light-1.png`: local server on the Connection tab.
  - `dseti5-05-deployment-test-local-light-1.png`: "Connected successfully" in green.
  - `dseti5-06-deployment-mappings-light-1.png`: the mapping row.
  - `dseti5-07-deployment-excluded-light-1.png`: "cache", "logs".
  - `dseti6-08-deployment-test-missing-light-1.png`: "Folder does not exist: …/missing" in red.
  - `dseti6-13-deployment-dark-1.png`: dark, the result gone after leaving and returning to the page.
- **Run Targets:**
  - `dseti3-02-runtargets-three-light-1.png`: the page opens on Global, empty.
  - `dseti5-11-runtargets-compose-light-1.png`: the Compose target's fields and hints.
- **Pre-fix, kept as evidence:**
  - `dseti4-05-…png`: Deployment with no server selected.
  - `dseti4-10-…png`: a misclick left the SSH target selected; it shows "ssh-demo (missing)", Browse… enabled and Upload ticked.
  - `dseti4-02/04-…png`: SSH Password and Port 70000 flagged red.
- **Captured but not opened:** `dseti2-01..06` and `dseti5-01/02/03/09/10/12`.

## Files edited

- **`app-gpui`:**
  - `src/backend.rs`, `src/settings_view/custom_controls.rs`, `src/settings_view/pages/mod.rs`, `src/workbench/mod.rs`
  - new: `src/settings_view/pages/ssh_backend.rs`
  - SSH: `src/settings_view/pages/tools_ssh.rs`, `tools_ssh/form.rs`, `tools_ssh/proofs29_ssh_view.rs` (formatted only), `src/workbench/tests/proofs29_settings_ssh.rs`
  - Deployment: `build_deployment.rs`, `build_deployment/connection.rs`, `build_deployment/paths.rs` (formatted only), `build_deployment/proofs28_deployment_view.rs`, `src/workbench/tests/proofs28_settings_deployment.rs`
  - Run Targets: `build_run_targets.rs`, `build_run_targets/sources.rs`, `build_run_targets/folder.rs`, `build_run_targets/form.rs` (formatted only), `build_run_targets/proofs30_run_targets_view.rs`, `src/workbench/tests/proofs30_settings_run_targets.rs`
- **`crates/`:** `crates/ide-host/src/lib.rs` and new `crates/ide-host/tests/ssh_files.rs`.

No process of mine is left. The one other matching process (PID 86105, a loop in `~/hm-dataset`) belongs to another session and was not touched.
