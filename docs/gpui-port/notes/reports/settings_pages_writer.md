I finished only the first of the four main pages, Deployment (`build.deployment`). SSH Configurations, Run Targets, Project Structure and the small four are not started, and none is half written. Nothing has been compiled or run, so every API use and every test result below is a hypothesis until the integrator builds it.

**Files written** (all new, untracked):

| File                                                                            | Lines | What it holds                                                          |
| ------------------------------------------------------------------------------- | ----- | ---------------------------------------------------------------------- |
| `app-gpui/src/settings_view/pages/build_deployment.rs`                          | 616   | the editor, its backend calls, staging; declares the three files below |
| `app-gpui/src/settings_view/pages/build_deployment/connection.rs`               | 487   | Connection tab, Test Connection, trust host key, password field        |
| `app-gpui/src/settings_view/pages/build_deployment/paths.rs`                    | 296   | Mappings table; Excluded Paths uses the existing `RecordTable`         |
| `app-gpui/src/settings_view/pages/build_deployment/proofs28_deployment_view.rs` | 327   | 7 view tests, backend replaced by a fake                               |
| `app-gpui/src/workbench/tests/proofs28_settings_deployment.rs`                  | 313   | 4 end-to-end tests in a project window                                 |
| `$S/drafts/settings_pages_integration.md`                                       | 159   | the integration notes                                                  |

The notes give three hunks, each with its exact anchor: a `pub mod` line in `pages/mod.rs`, the editor installed in `custom_controls.rs` `install()`, and the test `mod` line in `workbench/mod.rs`.

**Rows** (full test names are in the notes):

| Key                                    | Expected               | Missing                                                               |
| -------------------------------------- | ---------------------- | --------------------------------------------------------------------- |
| `settings:build.deployment` (new key)  | partial                | remote folder chooser, local chooser start folder, autofocus, spinner |
| `ipc:deploy_test_connection` (new key) | done                   | —                                                                     |
| `ipc:ssh_set_secret` (new key)         | done, but a hypothesis | rests on test builds saving to a vault file                           |
| `ipc:ssh_trust_host`                   | stays open             | only tested against the fake                                          |

- **`ipc:deploy_test_connection`:** a local server on a temp folder gets "Connected successfully", "Folder does not exist: …" and "Not a folder: …". Only `ide_host::ops::deploy::deploy_test_connection` produces those, called with the same arguments as `src-tauri/src/commands/deploy.rs:144-150`.
- **`ipc:ssh_set_secret`:** the backend keeps the password (`host.ssh.secret(&config)`), and the only file holding it is `ssh-passwords.json.vault`. Five other places in the app also call this command.
- **Unit rows:** no spec tests the Deployment page component. `deploymentModel.test.ts` was already done and is unchanged.

**Left out on the Deployment page, and why:**

- **Remote folder chooser** (`deployment.tsx:101-237`) for both Root path Browse… and Deployment path Browse…: a view of its own, about 400 lines. Neither button is drawn.
- **Local chooser's start folder and title:** the existing `ControlEvent::Browse` has neither.
- **Autofocus of the password field:** the callback that shows it has no window to focus with.
- **Spinner:** the crate has none; a muted refresh icon stands in.

**Secrets and machine sources:**

- **Where the password goes:** it is the field shown after a failed SFTP test; the page stores no password itself. The path is `deployment.tsx:75` → `src-tauri/src/commands/ssh.rs:29-31` → `crates/ide-ssh/src/manager.rs:89-106`. With Save it is kept in the system keychain in the real app, and in a vault file in test builds.
- **Never exposed:** it is never staged, never in a settings file, never drawn (bullets only) or in `lines()`, and its `Debug` output is redacted.
- **Keychain guard:** the SFTP end-to-end test asserts `secrets_in_files()` before typing, so it fails rather than reach the keychain. It ends with Cancel, so no SFTP server is left in the settings.
- **What the page reads in the real app:** the global `ssh.configs` setting, the window's project root, and the local disk (Test Connection writes and removes a probe file). Over SFTP it reaches the server only when the user clicks Test Connection.
- **Network in tests:** none. Tests use local folders, an SSH configuration id that does not exist, or the fake.

**Hypotheses most likely to bite on the first build:**

- Exact error texts match the backend's.
- `sshConfig-0` is the configuration the test wrote with `set_global` before opening the dialog.
- The page edits Global when no project is open.
- `AppDirs::under(home)` counts as isolated, which is what switches saved passwords to files.
- Closure and method signatures: `Context::listener` taking `&self`; methods passed as `fn` pointers; `theme::icon(..).into_any_element()`.
- Rows near the bottom may need `bring_into_view` before the end-to-end test can click them.

The full list is in the notes, (d) and (e).

**What turned out wrong:**

- **Strings file:** `src/platform/locales/text/en.json` does not contain the page's strings (all 41 checked are missing). English is the source text itself, and the missing-choice template is `"{name} (missing)"`.
- **My own claim:** I first said `tools_record_table.rs` binds to one settings key. It does not: it reads and writes through its owner. I corrected this by building Excluded Paths on it.
- **SSH case count:** I counted 11 `it(` lines in `ssh.test.tsx`; the inventory says 15.
- **Not read, though the brief named them:** `deployment/secret_dialog.rs`, `features_b/deployment/store.rs`, the tests in `workbench/deployment.rs`, and `languages_sdk.rs`.
- **Read under your home:** I ran one grep under `~/.cargo/registry/.../gpui-pre-0.3.12/src/` before noticing the "nothing under the user's home" rule. `wiring_brief.md:5` points there for GPUI signatures, so the two instructions conflict. Nothing else under the home was read.
- **Hypothesis on the small four:** a grep finds only their page specs (`features.rs:42,71-73`) and no view, slot or test. They likely draw no control today.
