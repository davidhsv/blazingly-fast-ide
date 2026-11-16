I finished the SSH Configurations page (`tools.sshConfigurations`): the view, its tests and the integration notes. **Run Targets is not started**: I wrote no file, hunk or test for it and did not read `runTargets.tsx`. Nothing was compiled or run, and I edited no existing file and ran no git command.

## Files written (new, untracked)

- `app-gpui/src/settings_view/pages/tools_ssh.rs` (775 lines): the page view, the backend calls, staging of the configurations and the secret, Test Connection, Trust, Connect, Browse….
- `app-gpui/src/settings_view/pages/tools_ssh/form.rs` (305): the Connection section.
- `app-gpui/src/settings_view/pages/tools_ssh/proofs29_ssh_view.rs` (555): 18 view tests with a stand-in backend and a stand-in password store.
- `app-gpui/src/workbench/tests/proofs29_settings_ssh.rs` (346): 6 end-to-end tests against the real backend under a temporary home.
- Notes: `$S/drafts/settings_pages2_integration.md` (hunks a–g, the case table, behaviours with `file:line`).

The integrator applies three hunks: `pub mod tools_ssh;` in `pages/mod.rs`, one block in `custom_controls.rs` `install`, and `mod proofs29_settings_ssh;` in the workbench `mod tests`. I re-read all three anchors at the end. No Apply code is needed: `settings_window.rs:161-165` and `:322-330` already store staged SSH secrets.

## Rows

I checked `parity.json`: every key below except the last is new.

| Key                                                 | Expected                                                                                                                          | Missing                                                                                     |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `settings:tools.sshConfigurations`                  | done (Hypothesis: once the end-to-end tests pass)                                                                                 | the file chooser's title "Private Key File" (`ControlEvent::Browse` has no title)           |
| `unit:src/features/settings/__tests__/ssh.test.tsx` | done, 15 of 15 cases (the case-by-case table is in the notes)                                                                     | the Apply-failure case checks the model's error, not the footer text                        |
| `ipc:ssh_test`                                      | done: the real backend reports `cannot connect to SSH host 127.0.0.1:<port>` from a loopback listener that drops the connection   | —                                                                                           |
| `ipc:ssh_needs_secret`                              | done: "Password already stored" shows for a password stored beforehand, and not for one it doesn't know                           | —                                                                                           |
| `ipc:ssh_set_secret`                                | done: after Apply the backend holds the password and only the vault file contains it (the Deployment draft proposes this key too) | —                                                                                           |
| `ipc:ssh_trust_host`                                | partial: the real trust answers "no host key to trust"                                                                            | a waiting host key only exists after a real handshake, which reads the user's `known_hosts` |
| `settings:build.runTargets`                         | not started                                                                                                                       | everything                                                                                  |

## Left out, and why

- The chooser title: there is no way to pass one.
- Browse… disabled outside Tauri: the native app always has a file chooser.
- `aria-label`s and most tooltips.
- Hypothesis: the stored-password question is asked when Settings opens, not when the page is first shown, because every page's view is built at open. I don't think that matters to anyone.

## Hypotheses (checked only by compiling or running)

- The keyboard moves to the password field after an auth failure, through a window-handle update from the deferred answer. This is the test most likely to fail. The fallback is to focus in `render` on the next frame.
- `use super::*` gives the test modules the parent's private items and imports.
- `cx.weak_entity()` and `cx.defer` work inside the view's constructor.
- Setting a field's text from code emits no `Changed` (read in the source, not run).
- The page's scope is Global even with a project open.
- The error text prefix from `crates/ide-ssh/src/session.rs:568-582`.
- `ssh.configs` reads `[]` after Cancel.
- The end-to-end clicks may need the page scrolled; `bring_into_view` in `proofs19_settings_misc` does that.

## Where secrets go, and what the page reads on the machine

- **Secrets:** the masked field shows bullets only. What is typed is staged with `stage_ssh_secret`, whose `Debug` is redacted. Apply hands it to `SshManager::set_secret`, which keeps it in the OS credential store (the keychain on macOS) and never in a settings file. Test Connection receives it only as a `SecretText`, also redacted.
- **Tests:** the vault file of an isolated temporary home stands in for the keychain. Every end-to-end test asserts that before touching a secret.
- **Machine sources in the real app:**
  - the global `ssh.configs` setting;
  - the saved passwords, asked when Settings opens;
  - on Test Connection: the network, plus `~/.ssh/config`, the SSH agent and identity files for OpenSSH, `known_hosts` on every handshake, and the key file for a key pair;
  - Trust writes `~/.ssh/known_hosts`.

  The page lists no hosts from the machine.

## What turned out wrong

- **Existing code:** the backend's `SshManager` always uses the user's real `~/.ssh` (`target.rs:37-42`), even under a temporary home. The Deployment draft falls back to the real backend in test builds, so a future SFTP or OpenSSH test without a stand-in would read the user's SSH files. This page gives test builds no Test Connection unless a test installs one, as the HTTP proxy page does.
- **Brief:** the secret's storage path through the backend already exists natively; the page only needs to stage the secret.
- **Brief:** none of the page's strings are in `en.json`. I checked four of them; the text comes from `ssh.tsx`.

**For the Run Targets writer:**

- `settings_model/run_targets.rs` already holds the page spec, and its unit row is ticked.
- `runTargets.tsx:91` and `:98` call `sshTrustHost` and `sshSetSecret`, so this page's backend calls can serve it.
- The `wsl_*` and `docker_*` IPC rows are unticked.
- A `dialog.test.tsx` case waits on that view.
