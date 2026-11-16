I've drafted the whole Run Targets page (`build.runTargets`) in one pass, but nothing is compiled or run yet: nothing declares the files, and per the rules I ran no cargo. All new files are untracked; no existing file was touched and no git command changed anything.

**Two findings first:**

- **There is no WSL on this page.** Its kinds are SSH, Docker and Docker Compose (`runTargets.tsx:331-335`); "Local machine" is only the empty default target (`:424`). `wslDistros` is called only from `remote-dev/ConnectDialog.tsx:67`, and `wslResolve` from `platform/remote.ts:272` and `app/windows.ts:59`.
- **Connect in the folder browser deletes a saved password.** It calls `sshSetSecret(id, secret, false)` (`:98`), and `set_secret` with `save=false` deletes the saved entry (`crates/ide-ssh/src/manager.rs:97-104`). I built it the same way as the webview; it probably needs a product decision.

**Files** (under `app-gpui/src/`):

| File                                                                 | Lines                                     |
| -------------------------------------------------------------------- | ----------------------------------------- |
| `settings_view/pages/build_run_targets.rs`                           | 726                                       |
| `settings_view/pages/build_run_targets/sources.rs`                   | 193                                       |
| `settings_view/pages/build_run_targets/form.rs`                      | 246                                       |
| `settings_view/pages/build_run_targets/folder.rs`                    | 435                                       |
| `settings_view/pages/build_run_targets/proofs30_run_targets_view.rs` | 431 (11 view tests; `form.rs` has 1 more) |
| `workbench/tests/proofs30_settings_run_targets.rs`                   | 261 (4 end-to-end tests)                  |

Notes, with three hunks anchored on quoted lines: `$S/drafts/settings_pages3_integration.md`. The hunks are a `pub mod` line in `pages/mod.rs` after `build_maven`, an install block in `custom_controls.rs` after the Maven `slots.page_view` line, and a `mod` line in `workbench/mod.rs` tests. The install anchor is not the one the SSH and Deployment groups use, so the three pages can land in any order. Nothing is a stub or `*_pending.rs`.

**Rows:**

| Key                                                                                                    | Expected                         | Proof / what's missing                                                                                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------ | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.runTargets` (new key)                                                                  | Hypothesis: done once tests pass | The 4 end-to-end tests run in a project window and change each kind of control with the mouse or keyboard, then Apply, Reset and Cancel. Copy/move/remove, the SSH configuration drop-down, Compose fields, Trust Host, the password step and plugin targets are covered only by view tests. |
| `unit:src/features/settings/__tests__/dialog.test.tsx`                                                 | partial, 30 of 41 (was 29)       | `…::proofs30_run_targets_view::uses_the_attached_primary_name_for_run_target_paths`                                                                                                                                                                                                          |
| `unit:src/features/settings/__tests__/runTargets.test.ts`                                              | already done, untouched          | —                                                                                                                                                                                                                                                                                            |
| `ipc:docker_settings` (new key)                                                                        | Hypothesis: done                 | "Lab daemon" can only come from `ide_docker::load_settings` reading the temp data folder's `docker.json`, the adapter's call. Services also calls it (`services/store.ts:126`).                                                                                                              |
| `ipc:plugin_run_targets`, `remote_list`, `ssh_trust_host`, `ssh_set_secret`, `wsl_*`, other `docker_*` | unchanged                        | No test window runs a plugin host, every browser test uses a stand-in, and the rest belong to other features. No `ipc:` row is this page's alone.                                                                                                                                            |

**Left out, and why:**

- The folder browser is a panel under the "Project folder on target" row, not a modal: no settings page draws window-level dialogs; the proxy's check panel is the precedent.
- Compose files and Published ports are staged on every keystroke rather than on blur, because a click on Apply may not move focus first and the edit would be lost. The stored value is the same.
- `aria-label`s and most `title` tooltips are not built.
- Plugin targets are read once when the dialog opens; the webview follows them live.
- The page has no input validation because the webview validates nothing on it. No field ever becomes a shell command.

**Hypotheses** (all settled by compiling or running):

- Child modules see the parent's private items, imports and re-exports through `use super::*`.
- A closure typed `&App` accepts a `&mut Context` argument (`target_list`).
- `value.iter().filter_map(Value::as_array).flatten()` compiles on an `Option<Value>`.
- The borrow checker accepts `folder_trust` and `folder_send_secret`, where the borrow is unused on the `Ok` path.
- `Arc<PluginHost>` is `Send`.
- `TextField::set_text` and `set_placeholder` emit no `Changed`.
- Clicking "This project" notifies the dialog's model; if it only notifies the dialog view, the fields keep the other scope's text until the next edit. External Tools has the same design.
- Drop-down choices are clickable by `click_on`, and `simulate_input` reaches the focused secret field.
- A down/up pair with `click_count: 2` counts as a double click.
- The project name from `ops::project::info` is the root folder's name.

**Tests most likely to fail first:**

- End-to-end clicks low on the page: `bring_into_view` only scrolls down, so I ordered the steps top to bottom.
- The Docker test, which waits for the asynchronously loaded connection list.
- The Escape/Enter test, if the keymap takes those keys before the text field does.

**Secrets and machine sources:**

- **Secret:** the only one is the browser's password or passphrase. It goes from the masked field to `SecretText` (redacted `Debug`) to `host.ssh.set_secret(id, Some, false)`, so it lives in session memory only. It is never staged, written to settings, drawn (lines show bullets) or logged. Tests use a stand-in setter and never reach the keychain; two tests also assert the system credential store is never set up.
- **What the real window reads:**
  - the project and global settings files (`runTargets`, `run.defaultTarget`; `ssh.configs` is always global);
  - the app data folder's `docker.json`, which is a file read only — no daemon, no `docker` program;
  - the plugin host's registrations for the project;
  - the project's workspace files, read to get its name.
- **What Browse… does:** it opens SFTP over the network and reads `~/.ssh/known_hosts`, plus, with OpenSSH auth, `~/.ssh/config`, the agent and identity files. Trust Host writes `~/.ssh/known_hosts`.
- **What tests read:** a test build has no folder browser and no Docker source unless a test installs one; the e2e Docker test reads only the temp data folder's `docker.json`.

**What turned out wrong in the brief or the native code:**

- The brief's WSL kind, and Connect deleting saved passwords (both above).
- `RunTarget` loads a missing `syncBeforeRun` as `false` (`remote_settings.rs:158-192`), while `validateRunTargets` defaults it to `true` (`remoteSettings.ts:111`, `:210`). Both UIs see the backend-loaded value, so it never shows; it only decides whether such a target uploads.
- Of 26 page strings checked, only "Read-only", "Default run target" and "Docker" are in `en.json`; the rest are the component's literals, passed through `tx` as the neighbouring pages do.
- The only tests that name the page are `runTargets.test.ts` and `dialog.test.tsx:782`. The other files the brief's grep returns test run targets and language-server targets, not this page.

What the three remote pages should share once they all compile (section (g) of the notes): one `on_backend` helper and `HostTrust`, a secret setter that takes `save`, the SSH-configuration choices, and one toolbar button helper. The secret handling should stay separate: the SSH page stages it until Apply, while this page and Deployment hand it straight to the backend.
