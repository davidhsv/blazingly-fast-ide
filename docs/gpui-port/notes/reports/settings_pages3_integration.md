# Settings pages, third pass: Run Targets — integration notes

Written without a compiler. One page, `build.runTargets` (Build, Execution, Deployment › Run Targets), drafted completely: list, toolbar, default target, every field of the three kinds, the folder browser with Trust Host and the password step, and the backend seams. Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here imports from the uncompiled SSH (`tools_ssh*`) or Deployment (`build_deployment*`) drafts or from their test files.

## Files written (all new, untracked)

- `app-gpui/src/settings_view/pages/build_run_targets.rs` (726): `PAGE`, `SLOT`, `KINDS`, `targets()` (item-by-item reading), `ssh_configs()`, `new_target()`, `kind_name/kind_label`, `container_summary()`, `Choice`, `Pick`, `RunTargetsView` (list, toolbar, sync, drop-down choices, `lines()`), re-exports of `sources` and `FolderState`. Declares `mod folder; mod form; mod sources;` and `#[cfg(test)] mod proofs30_run_targets_view;` itself.
- `app-gpui/src/settings_view/pages/build_run_targets/sources.rs` (193): the seams `RemoteFolders` (`FolderLister`, `HostTrust`, `SecretSetter`) with the global `RunTargetFolders`, `folders(cx)`, `backend_folders()`; `DockerSource` with the global `RunTargetDocker`, `docker_source(cx)`, `backend_docker()`; `PluginTarget`; `load_sources(view, window, cx)` (project name, plugins' targets, Docker connections, all off the window's thread).
- `app-gpui/src/settings_view/pages/build_run_targets/form.rs` (246): the default-target section, Target Settings, `form_lines()`, one unit test.
- `app-gpui/src/settings_view/pages/build_run_targets/folder.rs` (435): Select Folder on Target (`FolderBrowser`, `FolderState`, `needs_host_trust`, `needs_secret`, `folder_*` actions, `folder_lines`, `folder_panel`).
- `app-gpui/src/settings_view/pages/build_run_targets/proofs30_run_targets_view.rs` (431): 11 view tests over a hand-filled dialog model and a scripted stand-in backend.
- `app-gpui/src/workbench/tests/proofs30_settings_run_targets.rs` (258): 4 end-to-end tests in a project window with the real backend under a temporary home.

View: `RunTargetsView::new(binding: Binding, folders: Option<RemoteFolders>, cx)`; setters `set_project_name`, `set_plugin_targets`, `set_docker_connections` (filled by `load_sources`). Emits `ControlEvent::{Submit, Cancel}`. Accessors: `targets`, `current`, `field(name)`, `placeholder(name)`, `open_choice`, `picks(Choice)`, `can_browse`, `lines`, `folder_open`, `folder_path`, `folder_path_field`, `folder_secret_field`, `folder_state`. Actions: `select`, `add(copy)`, `remove`, `move_by`, `choose(Choice, value)`, `flip(name)`, `folder_go`, `folder_retry`, `folder_trust`, `folder_send_secret`, `folder_select`, `folder_choose`, `close_folder`. Selectors: `settings-targets-{add,remove,copy,up,down,browse}`, `settings-target-<n>`, `settings-text-target.{name,projectRoot,composeFiles,service,image,containerRoot,ports,runOptions,python,node,java}`, `settings-select-{run.defaultTarget,target.kind,target.sshConfig,target.dockerConnection}` with choices `settings-choice-<key>-<value, or its place when the value is empty or not a plain word>`, `settings-toggle-target.{syncBeforeRun,imageUser}`, `settings-target-folder{,-up,-path,-refresh,-row-<n>,-trust,-connect,-ok,-cancel,-error}`, `settings-secret-target.folder`.

## (a) Hunks, in order — group "Run Targets" (land together)

### 1. `app-gpui/src/settings_view/pages/mod.rs`

Anchor (exact):

```rust
pub mod build_maven;
```

Add right after it:

```rust
pub mod build_run_targets;
```

(The Deployment group inserts `pub mod build_deployment;` after `pub mod build_cmake;` and the SSH group `pub mod tools_ssh;` after `pub mod tools_record_table;`: no overlap.)

### 2. `app-gpui/src/settings_view/custom_controls.rs`, inside `install`

Anchor (exact, the two lines just before the insertion):

```rust
    let view = pages::build_maven::page(binding(pages::build_maven::PAGE), toolchains.clone(), cx);
    slots.page_view(pages::build_maven::PAGE, &view, cx);
```

Add right after it:

```rust

    // Build, Execution, Deployment › Run Targets: the targets of the scope shown, on the global SSH
    // configurations, the Services tool window's Docker connections and the plugins' targets.
    let run_targets = binding(pages::build_run_targets::PAGE);
    let folders = pages::build_run_targets::folders(cx);
    let view = cx.new(|cx| pages::build_run_targets::RunTargetsView::new(run_targets, folders, cx));
    pages::build_run_targets::load_sources(&view, install.window_label, cx);
    slots.put(pages::build_run_targets::PAGE, pages::build_run_targets::SLOT, &view, cx);
```

This anchor is not the one the SSH and Deployment groups use (the External Tools lines), so the three groups land in any order. No Apply, Reset or Cancel code is needed: the view stages `runTargets` and `run.defaultTarget` through its `Binding` under `build.runTargets`, in the scope the dialog shows; the browser's password is never staged.

### 3. `app-gpui/src/workbench/mod.rs`, inside `mod tests`

Anchor (exact, as read today; another agent edits this list, so any place in the `mod` list works):

```rust
    mod proofs26_code_preview;
```

Add right after it (after `proofs26_save_all` / `proofs28_*` / `proofs29_*` if those landed):

```rust
    mod proofs30_settings_run_targets;
```

Nothing else. Optional, not needed for a row: add `(pages::build_run_targets::PAGE, Act::Click("settings-targets-add"), "runTargets")` to the table of `settings_view::custom_controls::tests::on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control` (its array length grows by one); `run_targets_reset_and_cancel_discard_a_target_added` covers the same.

## (b) Rows

| Key                                                                  | Expected                                                                | Proving tests                                                                                                                                                                                                                                                                                                                                                                 | Missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.runTargets` (new key, named as `settings:build.run`) | Hypothesis: done once the e2e pass                                      | `workbench::tests::proofs30_settings_run_targets::run_targets_an_ssh_target_added_and_edited_is_written_to_the_project`, `…::run_targets_a_docker_target_offers_the_services_connections_and_applies`, `…::run_targets_the_folder_chosen_in_the_browser_is_the_targets_project_folder`, `…::run_targets_reset_and_cancel_discard_a_target_added`, and the 11 view tests below | every kind of control is changed with the mouse or the keyboard in a project window: toolbar (+), text fields (name, project folder, interpreter, image, ports, run options), drop-downs (default target, type, Docker connection), checkboxes (upload, image's user), Browse… with the browser (row, OK), Apply, Reset, Cancel. Only in view tests: copy / move / remove, the SSH configuration drop-down, Compose fields, Trust Host, the password step, plugin targets (no plugin host runs in a test window). Not built: see (c) "Left out" |
| `unit:src/features/settings/__tests__/dialog.test.tsx`               | partial, 30 of 41 (was 29)                                              | new: `settings_view::pages::build_run_targets::proofs30_run_targets_view::uses_the_attached_primary_name_for_run_target_paths` (`dialog.test.tsx:782-792`: `~/primary`, then `/workspaces/primary` after Type › Docker); the e2e tests also assert `~/<root name>` and `/workspaces/<root name>` from `ide_host::ops::project::info(&project).name`                           | the other 10 missing cases of the row are not this page's (row text `INVENTORY.md:3173`): its "Build › Run Targets has no view (runTargets.tsx:471)" clause goes                                                                                                                                                                                                                                                                                                                                                                                |
| `unit:src/features/settings/__tests__/runTargets.test.ts`            | already `[x]` (`settings_model::run_targets::tests`, 4 of 4), untouched | —                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `ipc:docker_settings` (new key)                                      | Hypothesis: done                                                        | `…::run_targets_a_docker_target_offers_the_services_connections_and_applies`: the choice "Lab daemon" can only come from `ide_docker::load_settings(&data_dir)` reading the temporary data folder's `docker.json`, the call `src-tauri/src/commands/docker.rs:181-184` makes (the test installs the real `backend_docker()` as its source)                                    | also called by `src/features/services/store.ts:126` (measured by grep); not this page's alone                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `ipc:plugin_run_targets`                                             | not proven                                                              | —                                                                                                                                                                                                                                                                                                                                                                             | the page reads `PluginHost::run_targets(&project.root)`, which `ide_host::plugins::run_targets::list` (`crates/ide-host/src/plugins/run_targets.rs:120-122`) wraps; no test window runs a plugin host. Also the webview page does not call it: the plugins feature fills a store (`src/features/plugins/backend.ts:300`, `src/platform/pluginRunTargets.ts:8`)                                                                                                                                                                                  |
| `ipc:remote_list`, `ipc:ssh_trust_host`, `ipc:ssh_set_secret`        | unchanged by this page                                                  | —                                                                                                                                                                                                                                                                                                                                                                             | the browser's real calls (`backend_folders()`) open SSH connections; every test uses a stand-in. `remote_list` is already done by the Deployment tests (`parity.json:8070`)                                                                                                                                                                                                                                                                                                                                                                     |
| `ipc:wsl_distros`, `ipc:wsl_resolve`                                 | unchanged                                                               | —                                                                                                                                                                                                                                                                                                                                                                             | this page has no WSL kind. Callers (measured): `wslDistros` only `src/features/remote-dev/ConnectDialog.tsx:67`; `wslResolve` `src/platform/remote.ts:272`, `src/features/app/windows.ts:59`                                                                                                                                                                                                                                                                                                                                                    |
| other `ipc:docker_*` (`INVENTORY.md:2097-2113`)                      | unchanged                                                               | —                                                                                                                                                                                                                                                                                                                                                                             | the Services tool window's, not this page's                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

Case table of the view tests (`settings_view::pages::build_run_targets::proofs30_run_targets_view::`):

| Test                                                                                           | What it proves                                                                                                                                             |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `uses_the_attached_primary_name_for_run_target_paths`                                          | `dialog.test.tsx:782`; plus the Compose container folder's empty placeholder (`runTargets.tsx:392`)                                                        |
| `targets_are_added_copied_moved_and_removed`                                                   | Add (`Target 1`, first SSH configuration, upload on, `target-…` id), copy `(copy)`, move up/down, remove selects the one in its place (`:442-466`)         |
| `the_default_run_target_offers_the_local_machine_the_targets_plugin_targets_and_a_missing_one` | `:422-428`, plugin rows `:496-503`, choosing stages `run.defaultTarget`                                                                                    |
| `the_type_switches_between_the_ssh_and_the_container_fields`                                   | `:538-560`, `:344-415`, SSH choice labels (`SshConfigSelect.tsx:27-30`), Docker choices with missing (`:320-329`), list summaries (`:338-342`, `:517-519`) |
| `compose_files_and_published_ports_are_stored_as_lists_of_the_words_typed`                     | `:262-301` (split at blanks, commas, semicolons), run options, image's user, Reset                                                                         |
| `a_stored_list_is_read_item_by_item_and_staged_whole_after_an_edit`                            | `remoteSettings.ts:132-152`, `:200-222` (per-field defaults, unknown kind → SSH, ids made unique)                                                          |
| `browse_needs_an_ssh_configuration_and_a_backend_for_the_browser`                              | `:243`; the two error patterns (`:36-37`)                                                                                                                  |
| `the_folder_browser_lists_subfolders_walks_up_and_chooses_one`                                 | `:60-87`, `:102-103`, `:118-124`, `:176-189`, OK `:112`                                                                                                    |
| `the_folder_browser_trusts_an_unknown_host_and_lists_again`                                    | `:89-96`, `:141-150`, OK disabled on error `:112`, Enter in the path `:131-136`, Cancel                                                                    |
| `the_folder_browser_hands_a_typed_password_to_the_backend_and_never_shows_it`                  | `:97-101`, `:151-165`; never listed or staged                                                                                                              |
| `escape_in_a_field_closes_an_open_list_before_it_cancels_the_dialog`                           | the page's keys (Enter = OK, Escape = close list, then Cancel)                                                                                             |

Plus `form::tests::container_fields_follow_the_kind` (plain `#[test]`).

## (c) Behaviours of `src/features/settings/pages/runTargets.tsx`

Built:

- What a run target is: SSH (another machine, project synchronised to a folder), Docker (a new container of an image) or Docker Compose (a new container of a service) (`KIND_OPTIONS`, `:331-335`); "Local machine" is the empty default target (`:424`), not a kind.
- Targets are `runTargets`, the default `run.defaultTarget`, both in the page's scope (`useSetting`, `:419-420`; page `projectScope: true`, `:603`); natively through `Binding` (`build_run_targets.rs` `targets`, `stage_targets`, `choose`).
- Toolbar Add / Remove / Copy / Move Up / Move Down with their enabled states (`:471-489`); Add: `DEFAULT_RUN_TARGET` (upload on, `remoteSettings.ts:105-123`), `Target <n+1>`, `newRemoteId("target")`, first SSH configuration, selected (`:442-453`); Copy `<name> (copy)`; Remove selects `rest[min(index, len-1)]` (`:454-459`); move swaps (`:460-466`).
- List: empty text only without user and plugin targets (`:491-495`); plugin rows `name`, `plugin · Read-only`, not selectable (`:496-503`); rows selected on mouse down (`:510`), `name || id` (`:513`), summary: SSH configuration `name || host || id`, else `image` / `compose:<service>` / `compose` (`:433-436`, `:338-342`, `:517-519`); first target selected at open (`:430`).
- Default run target drop-down: Local machine, targets by `name`, plugin targets `<name> (<plugin>)`, a stored id that is gone `<id> (missing)` (`:422-428`, `:526-533`).
- Target Settings (`:536-579`): Name; Type (SSH / Docker / Docker Compose); SSH configuration drop-down (`SshConfigSelect.tsx:18-31`: `name.trim() || sshLabel`, detail `sshLabel`, `<id> (missing)`, "No SSH configurations" disabled); Project folder on target with placeholder `~/<project>`, hint, Browse… disabled without an SSH configuration (`:223-260`); Upload changed project files before each run (`:552-556`).
- Container fields (`:344-415`): Docker connection drop-down (Docker CLI default, Services connections, `<id> (missing)` only when connections are known) with its hint; Compose: Compose files (list, placeholder, hint) and Service; Docker: Image (placeholder); Container folder with the kind's hint and placeholder (`/workspaces/<project>`, empty for Compose); Published ports (list); Run options; Run as the image's user.
- Interpreters: Python, Node.js, Java with placeholders (`:561-578`); the two-sentence note (`:581-592`).
- Docker connections read once from `docker_settings` (`:303-318`), no source outside the desktop app (`isTauri`); natively `load_sources` with `docker_source`, none in a test build unless set.
- Select Folder on Target (`:39-195`): listing over SFTP with the target's SSH configuration (`:62`), only folders, sorted by `localeCompare` (`:65`, natively `collate::compare`), `~`/relative turned absolute from the first entry without listing again (`:66-71`), Parent Folder disabled at the top (`:118-124`), path typed + Enter (`:125-137`), Refresh (`:138`), Loading… / No subfolders / error (`:141-174`), Trust Host on `^Unknown SSH host key` then list again, its failure shown (`:89-96`, `:144-150`), password field + Connect on the secret patterns, Enter = Connect, `sshSetSecret(id, secret, false)`, field emptied, list again (`:97-101`, `:151-165`), click selects / double click opens (`:183-184`), OK = selected else shown, disabled on error (`:112`), Cancel / Escape close (`:113`).
- Validation: the webview validates nothing on this page (no host, port or path checks; `en.json` has none of its messages); none added. No field becomes a shell command here (run options are stored as typed, as the backend reads them).

Left out:

- The browser is a panel under the Project folder row, not a modal dialog of its own (520 px wide in the webview): the settings pages draw no window-level dialogs; the proxy's Check connection… panel is the precedent.
- Compose files and Published ports are staged on every keystroke (the words typed so far), not on blur (`:283-297`): GPUI's text field reports no blur to its owner here, and a click on Apply does not move the keyboard first. The stored value is the same; the text typed is kept while it spells the staged list.
- `aria-label`s and `title` tooltips other than the toolbar's (`:118`, `:128`, `:138`, `:140`, `:155`, `:512`, `:517`).
- Plugin targets are read once when the dialog opens (the webview follows its store live).

Different by construction: Hypothesis that it matters to no one: the project name is read off the window's thread when the dialog opens; until it arrives the placeholders say `~/project` (the webview's own fallback, `:224`).

## (d) Hypotheses (not settled without a compiler)

1. Child modules see the parent's private items and fields (`self.open`, `self.folder`, `self.binding`, `FIELDS`, `KEY`) and, through `use super::*`, its private `use` imports and the `pub use sources::…` re-exports. Settle: compile.
2. Calling a closure typed `|cx: &App, …|` with a `&mut Context<Self>` coerces (`target_list`'s `muted`). Settle: compile; else pass `&*cx`.
3. `value.iter().filter_map(Value::as_array).flatten()` on an `Option<Value>` (`targets`, `ssh_configs`). Settle: compile.
4. The borrow checker accepts `let Some(browser) = view.folder.as_mut()… ; match result { Ok(()) => view.folder_retry(cx), Err(e) => browser.error = … }` in `folder_trust` / `folder_send_secret` (the borrow is unused on the Ok path). Settle: compile; else re-borrow `view.folder` inside the Err arm.
5. `Arc<PluginHost>` and `Arc<Host>` move into `background_executor().spawn` (both `Send`). Settle: compile.
6. `AsyncApp::update` returns `R` (read: `gpui-pre-0.3.12/src/app/async_context.rs:176`), so `cx.update(|cx| then(result, cx));` is a statement as in `custom_controls.rs:346`.
7. `TextField::set_text` and `set_placeholder` emit no `Changed` (tests brief; `text_field.rs:108-133` not read in full), so `sync` does not stage what it shows.
8. The dialog's scope switch `settings-scope-project` is drawn for this page in a project window (`dialog.rs:672-676`: spec has `project_scope`), and Apply writes the project layer to `<root>/.openidea/settings.toml`.
9. The drop-down lists (deferred, priority 1) are clickable by `click_on` in the dialog, as the Updates source choice is (`proofs19_settings_misc.rs:378-379`).
10. `ide_host::ops::project::info(&project).name` is the root's folder name for a plain temporary project (`ops/project.rs:26-45`, read; `Workspace::load` finds none).
11. `simulate_input` reaches the browser's `SecretField` once focused through `window.focus` (as the proxy tests do).
12. Switching Global / This project notifies the dialog's model (the view re-syncs its fields only on the model's notify). If it notifies only the dialog view, the fields keep the other scope's text until the next edit; External Tools has the same design. Settle: a view test that switches scope with a target in each.
13. A double click delivered as one down/up pair with `click_count: 2` reaches `on_click` with `click_count() == 2` (`bookmarks_window.rs:1136`, `:1320-1335`).

## (e) Tests most likely to fail first

- The e2e clicks low on the page (`settings-toggle-target.*`, `settings-text-target.python`, the browser's rows): each goes through `bring_into_view` first; if a drop-down's list opens below the dialog's buttons, `click_on` of a choice may miss (choose a higher control or scroll after opening).
- `run_targets_a_docker_target_offers_the_services_connections_and_applies`: the Docker list arrives asynchronously from the backend's runtime; the test waits with `settle` on two choices.
- `the_type_switches_between_the_ssh_and_the_container_fields`: the exact window of nine lines (`skip(4).take(9)`) depends on line order in `form_lines`.
- `escape_in_a_field_closes_an_open_list_before_it_cancels_the_dialog`: needs the focused `TextField` to see Escape and Enter before the keymap (`text_field.rs:373` emits `Cancelled`); if the keymap takes them first, claim the keys as `FormView` does (`pages/mod.rs:330-341`).
- `compose_files_and_published_ports_are_stored_as_lists_of_the_words_typed`: whether `type_over` delivers the text at once or char by char (either keeps the draft by hypothesis 7).

## (f) What a look at the real window must check, and the machine sources

- Build, Execution, Deployment › Run Targets: the list box with + − copy ↑ ↓ above the Default run target row; Target Settings with the drop-downs opening over the page; SSH vs Docker vs Compose fields swapping; Browse… opening the folder panel under the Project folder row, rows with folder icons, OK/Cancel; the note at the bottom.
- Machine sources in the real app: the project's settings file and the global settings file (`runTargets`, `run.defaultTarget`; `ssh.configs` from the global one); the app data folder's `docker.json` (read only, no daemon, no `docker` program); the plugin host's registrations for the project (in memory); the project's workspace files read by `ops::project::info` for its name. Browse… opens an SSH/SFTP connection to the configured host over the network, which reads `~/.ssh/known_hosts`, and with OpenSSH authentication `~/.ssh/config`, the SSH agent and the identity files; with a key pair the key file named. Trust Host writes `~/.ssh/known_hosts` (`crates/ide-ssh/src/manager.rs:173-177`). Connect keeps the password in the backend's memory for the session and **forgets a password saved for that configuration** (`manager.rs:89-106`: `save = false` deletes the saved one), as the webview does (`runTargets.tsx:98`).

## Where the page keeps its secrets

Only the browser's password or passphrase: `SecretField` (bullets) → `SecretText` (redacted `Debug`, `database/store.rs:72-104`) → `RemoteFolders::set_secret` → `host.ssh.set_secret(id, Some(secret), false)` (`sources.rs` `backend_folders`), the call of `src-tauri/src/commands/ssh.rs:29-31`. Kept in the SSH manager's session memory; never staged, never in a settings file, never listed (lines show bullets), never logged. The webview does the same (`runTargets.tsx:97-101`). Tests never reach the keychain: the setter is a stand-in in every test that types a secret (`proofs30_run_targets_view.rs` `stand_in`), and the e2e browser test's stand-in is never asked for one.

## (g) What this page, the SSH page and the Deployment page should share once all compile

- One `on_backend<T, F, W>` (this page's `sources.rs` copy is the generic one, as `tools_ssh.rs:90-106`; Deployment's is `()`-only): move it to a new `app-gpui/src/settings_view/pages/ssh_backend.rs` with `HostTrust` (identical type in all three: `Rc<dyn Fn(String, Then<Result<(), IdeError>>, &mut App)>`) and a `backend_trust_host()`; `RemoteFolders::trust_host`, `tools_ssh::SshChecks::trust_host` and `build_deployment::Connections::trust_host` then use it.
- One secret setter: Deployment's `SecretSetter` takes `save`, this page's always passes `false`; the shared one should take `save` and this page pass `false`.
- One SSH configuration drop-down (`SshConfigSelect.tsx`): Hypothesis (Deployment's `connection.rs` read only at its `(missing)` line, `:64`): this page's `picks(Choice::Ssh)` and Deployment's choice build the same labels (`name.trim() || label()`, `(missing)`, "No SSH configurations"); a shared `ssh_config_picks(binding, value, cx)` in `ssh_backend.rs` (or `settings_model::ssh_connection`) replaces both.
- One list toolbar button: a `tool()` fn in SSH and here, and a closure inside `ExternalToolsView::toolbar` (compiled), differing only in the selector prefix (Hypothesis for Deployment's `tool`, imported by `paths.rs` but not read); a `pages::toolbar_button(cx, prefix, name, icon, enabled, title)` in `pages/mod.rs` replaces them.
- Not shared: the secret's path. SSH stages it until Apply; Deployment and this page hand it straight to the backend; keep them apart.

## Corrections to the brief and to existing code

- The brief lists the kinds as "local, SSH, Docker, WSL": the page's kinds are SSH, Docker and Docker Compose (`runTargets.tsx:331-335`); "Local machine" is only the empty default target (`:424`); there is no WSL on this page, and the `wsl_*` rows are other features' (callers above).
- No `ipc:` row is this page's alone: `docker_settings` is also Services' (`services/store.ts:126`); `remote_list`, `ssh_trust_host`, `ssh_set_secret` have other callers; plugin targets reach the webview page through a store.
- Existing backend difference (not changed, in `crates/`): `RunTarget` is `#[serde(default)]` with a derived `Default`, so a target in a settings file without `syncBeforeRun` loads as `false` (`crates/ide-core/src/remote_settings.rs:158-192`), while `validateRunTargets` defaults it to `true` (`remoteSettings.ts:111`, `:210`). Both UIs get the settings already loaded by the backend (natively `settings_value(&effective(..))`, `workbench/settings_window.rs:210-217`), so neither shows the difference; it only decides whether such a target uploads. Fix, if wanted, belongs in `ide-core`. The page's own `read_target` (upload on when missing) only matters for a value staged by hand.
- Strings: of the 26 page strings grepped as keys in `src/platform/locales/text/en.json`, only "Read-only" (`:622`), "Default run target" and "Docker" are there; the rest are the component's literals, passed through `controls::tx` as the neighbours do.
- Tests that name the page (grep of `pages/runTargets|build.runTargets|RUN_TARGET_PAGES|splitList|parentPath` over `src/**/*.test.*`): only `src/features/settings/__tests__/runTargets.test.ts` (4 cases, already native) and `dialog.test.tsx:782-792` (1 case, now native). The other files the brief's `RunTarget` grep returns (`plugins/__tests__/runTargets.test.ts`, `run/*.test.ts`, `languageServerTargets.test.tsx`) test run and language-server targets, not this page.
- `ssh_set_secret` with `save = false` deletes a saved password (`manager.rs:97-104`): the webview's Connect in this dialog (`runTargets.tsx:98`) forgets a password the user saved on the SSH page. Built the same; worth a product decision.
