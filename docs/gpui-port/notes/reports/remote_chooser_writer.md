I wrote the remote folder chooser for both Browse… buttons (Root path and a mapping's Deployment path), with tests, and moved Run Targets onto the same chooser. Nothing has been compiled or run: I never ran cargo, edited no existing file and ran no git command that changes anything.

**Files written** (all new, untracked):

| File                                                                                                               | Lines |
| ------------------------------------------------------------------------------------------------------------------ | ----- |
| `…/settings_view/pages/remote_folder_browser.rs` (the shared chooser)                                              | 652   |
| `…/settings_view/pages/remote_folder_browser/panel.rs` (how it is drawn)                                           | 234   |
| `…/settings_view/pages/remote_folder_browser/proofs37_remote_folder_browser.rs` (9 view tests)                     | 332   |
| `…/settings_view/pages/build_deployment/remote_folder.rs` (the Deployment page's side, 1 unit test)                | 244   |
| `…/settings_view/pages/build_deployment/proofs37_deployment_chooser_view.rs` (4 view tests)                        | 213   |
| `…/workbench/tests/proofs37_settings_deployment_chooser.rs` (2 end-to-end tests)                                   | 194   |
| `$S/drafts/remote_chooser_run_targets_folder.rs` (new full content for the existing `build_run_targets/folder.rs`) | 115   |
| `$S/drafts/remote_chooser_integration.md` (the notes)                                                              | 418   |

The `…` stands for `<worktree>/app-gpui/src`, and `$S` for `$S`.

- **Hunks to apply:** A1–A15 put the chooser on the Deployment page. B1 replaces `folder.rs` and deletes the old Run Targets browser. B2 changes 4 lines of `proofs30_run_targets_view.rs`, because `folder_path` and `folder_state` now take `cx`. `custom_controls.rs` and `DeploymentServersView::new` are unchanged.
- **Kept unchanged on purpose:** Run Targets keeps its selectors, texts, single listing and `save=false`. The existing `Root path: …` and mapping row lines in `lines()` stay byte-identical, so the proofs28 tests are unaffected.

**Rows**

| Key                         | Expected        | Proving tests                                                                                                                                                                                                                                                                                                                 | Missing                                                                                                                                 |
| --------------------------- | --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.deployment` | partial         | `workbench::tests::proofs37_settings_deployment_chooser::deployment_root_path_browse_walks_a_local_server_into_a_subfolder_and_applies_it`, `workbench::tests::proofs37_settings_deployment_chooser::deployment_mapping_browse_puts_the_folder_chosen_relative_to_the_root_path_and_applies_it`, plus the five proofs28 tests | The row also names the local chooser's title and start folder, and "a busy mark". It is done if those are accepted as outside this row. |
| `ipc:remote_list`           | done, as before | the two tests above can join its proof                                                                                                                                                                                                                                                                                        | none                                                                                                                                    |
| `settings:build.runTargets` | done, unchanged | the existing proofs30 tests                                                                                                                                                                                                                                                                                                   | none                                                                                                                                    |

No webview spec names this chooser, so no spec case was waiting on it.

**Left out, and why**

- **Local Browse… title and start folder:** the mapping's local Browse… still opens the system chooser without `Choose Folder` or a start folder. `ControlEvent::Browse { directories, then }` carries neither, and adding them means changing a shared event that many pages use.
- **The busy mark:** Hypothesis: it is the spinner beside Test Connection (`deployment.tsx:351`). It is not part of the chooser, so I left it.
- **Second listing of `~`:** the webview lists the login folder, then lists its absolute path a second time, because renaming the path re-runs its listing. The native chooser takes the absolute path without listing again: same rows, one connection fewer.
- **Inline, not modal:** the chooser is drawn under its field, as the Run Targets one is. Because it is not modal, it closes when the server, tab, page, type or SSH configuration changes. A mapping's chooser also closes when a mapping is added or removed, since it fills its row by position.

**What the chooser passes for `save`:** the state of its own Save checkbox, which starts unticked. That is `commands.sshSetSecret(ssh, secret, save)` at `deployment.tsx:75`, with the state at `:45` and the checkbox at `:87`. The native Deployment chooser does the same; Run Targets keeps `false` (`runTargets.tsx:98`).

**How the safety rules are met**

- **Test builds:** with no `DeploymentFolders` set, the page lists nothing and Browse… is unavailable, the same gate `sources.rs` uses.
- **End-to-end tests:** they install the real backend's listing, wrapped so that it answers only absolute local paths under `<project>/server`. Anything else is refused before it reaches the backend.
- **The password:** it is drawn as bullets only, travels only as `SecretText`, and is cleared whenever a new listing starts.
- **Off the window's thread:** listing runs on the backend and sorting in a background task.
- **Late answers:** dropped by a request counter.
- **Large folders:** there is no cap, as in the webview. Only the rows in view are drawn (`uniform_list`), and one test lists 100,000 folders.

**Hypotheses**

- These calls are copied from code already in the crate and should compile: `cx.spawn(async move |this, cx| …)`, a background sort using `collate`, and a view entity drawn as a child of a page.
- Dropping a subscription inside its own callback (closing the chooser on OK) is safe in GPUI.
- `drop_stale_chooser` clears `self.chooser` while a borrow of it is still used on another path; the borrow checker (NLL) should accept it.
- The list keeps keyboard focus while its content switches between rows and `Loading…`, and `claim_keys` gets Up/Down/Enter/Backspace/Escape before the Settings dialog does.
- `TextField` emits `Submitted` on Enter after `set_text`, and select-all plus Backspace emits `Changed`.
- `bring_into_view` scrolls the Settings page and not the chooser's list.
- The 100,000-folder sort takes about a second in a debug build.
- The `(d)` section of the notes says how to settle each one. The tests most likely to fail first are the keyboard test and the two end-to-end tests (scrolling, macOS temp paths).

**Wrong in the native code or the brief**

- `folder.rs` is 437 lines, not 435.
- The old Run Targets browser sorted folders on the window's thread; the shared one sorts in the background.
- The parity row's "Missing" names the _local_ chooser's title, not the remote chooser's.
- No webview spec names the Deployment chooser.
- The two webview choosers differ in title, button order, when Parent Folder works, what an empty path does, which paths become absolute, the error remedies, and the keyboard. Making "one browser" therefore needed a per-page `Flavour` setting rather than merging their behaviour.
- The view tests sit in a `remote_folder_browser/` subfolder, not beside the file.
- `connection.rs`'s `ssh_configs` was private, so hunk A9 makes it `pub(super)`.
- Hypothesis: `bg-tree-hover` and `h-list-row` resolve in the native theme. Other views use `bg-tree-hover`; `h-list-row` has no other user in the crate.
