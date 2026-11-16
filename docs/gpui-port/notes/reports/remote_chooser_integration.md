# Integrating the remote folder chooser (Deployment servers page, shared with Run Targets)

New files, not compiled yet, all untracked:

- `app-gpui/src/settings_view/pages/remote_folder_browser.rs`: the shared chooser `RemoteFolderBrowser` (an entity with `FolderBrowserEvent::{Chosen, Closed, Changed}`), its backend seam `FolderBackend { list: ListFolder, trust_host, set_secret }`, `backend_list()` (`remote_list`), `needs_host_trust` / `needs_secret`, `FolderState`, and `Flavour::{Target, Deployment}` for what the two pages want differently. It declares its own children `remote_folder_browser/panel.rs` (how it is drawn: rows, status, problem, `impl Render`) and the test file `remote_folder_browser/proofs37_remote_folder_browser.rs`; neither needs a hunk.
- `app-gpui/src/settings_view/pages/build_deployment/remote_folder.rs`: the Deployment page's side (`DeploymentFolders` global seam, `folder_lister`, `BrowseTarget`, `Chooser`, Browse… buttons, open/choose/close, lines and panel). Has an inline `#[cfg(test)] mod tests`.
- `app-gpui/src/settings_view/pages/build_deployment/proofs37_deployment_chooser_view.rs`: view tests of the page with the chooser.
- `app-gpui/src/workbench/tests/proofs37_settings_deployment_chooser.rs`: end-to-end tests in a project window, real backend, a local folder server at `server/` inside the temp project; the lister the tests install refuses anything that is not an absolute local path under that folder.
- Replacement for an existing file, kept outside the worktree: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/remote_chooser_run_targets_folder.rs` becomes the whole content of `app-gpui/src/settings_view/pages/build_run_targets/folder.rs` (hunk B1).

Every anchor below is quoted from the file as it was read; line numbers are left out. Run `cargo fmt` after pasting: a few lines may pass 120 columns.

## (a) Hunks, in order

### A. The chooser on the Deployment page

#### A1. `app-gpui/src/settings_view/pages/mod.rs`: the module

After

```rust
pub mod notifications;
```

add

```rust
pub mod remote_folder_browser;
```

#### A2. `app-gpui/src/settings_view/pages/build_deployment.rs`: modules and re-exports

Replace

```rust
mod connection;
mod paths;
#[cfg(test)]
mod proofs28_deployment_view;
```

with

```rust
mod connection;
mod paths;
#[cfg(test)]
mod proofs28_deployment_view;
#[cfg(test)]
mod proofs37_deployment_chooser_view;
mod remote_folder;
```

and after

```rust
pub use super::ssh_backend::{HostTrust, SecretSetter};
```

add

```rust
pub use remote_folder::{BrowseTarget, DeploymentFolders, folder_lister};
```

#### A3. `build_deployment.rs`: the field

In `pub struct DeploymentServersView`, after

```rust
    /// The selected server's excluded paths.
    excluded: Entity<RecordTable>,
```

add

```rust
    /// The folder chooser open under Root path or under the mappings, and the field it fills.
    chooser: Option<remote_folder::Chooser>,
```

#### A4. `build_deployment.rs`: its initial value

In `DeploymentServersView::new`, in `let mut view = Self { … }`, replace

```rust
            excluded,
            window: None,
```

with

```rust
            excluded,
            chooser: None,
            window: None,
```

#### A5. `build_deployment.rs`: another server closes it

In `pub fn select`, replace

```rust
        if self.selected != id {
            self.test = None;
            self.reset_problem(cx);
            self.show_excluded_of(id.clone(), cx);
        }
```

with

```rust
        if self.selected != id {
            self.test = None;
            self.reset_problem(cx);
            self.show_excluded_of(id.clone(), cx);
            self.chooser = None;
        }
```

#### A6. `build_deployment.rs`: another tab closes it

In `pub fn show_tab`, replace

```rust
        self.tab = tab;
        self.open_select = None;
        cx.notify();
```

with

```rust
        if self.tab != tab {
            self.chooser = None;
        }
        self.tab = tab;
        self.open_select = None;
        cx.notify();
```

#### A7. `build_deployment.rs`: another type or SSH configuration closes it

In `fn sync`, replace

```rust
        self.sync_mappings(cx);
        cx.notify();
    }
```

with

```rust
        self.sync_mappings(cx);
        self.drop_stale_chooser(cx);
        cx.notify();
    }
```

#### A8. `build_deployment.rs`: another page closes it

In `fn follow_page`, replace

```rust
        self.page_shown = shown;
        self.test = None;
        self.reset_problem(cx);
```

with

```rust
        self.page_shown = shown;
        self.test = None;
        self.reset_problem(cx);
        self.chooser = None;
```

#### A9. `app-gpui/src/settings_view/pages/build_deployment/connection.rs`: the SSH configurations for the chooser's authentication

Replace

```rust
fn ssh_configs(view: &DeploymentServersView, cx: &App) -> Vec<SshConfiguration> {
```

with

```rust
pub(super) fn ssh_configs(view: &DeploymentServersView, cx: &App) -> Vec<SshConfiguration> {
```

#### A10. `connection.rs`: Root path's chooser as text

In `connection_lines`, replace

```rust
        out.push(format!("    {}", tx(cx, ROOT_HINT)));
```

with

```rust
        out.push(format!("    {}", tx(cx, ROOT_HINT)));
        out.extend(self.chooser_lines(true, cx));
```

The `Root path: …` line stays byte-identical (proofs28 asserts it); Browse…'s state is read through `can_browse_root`.

#### A11. `connection.rs`: Root path's Browse… and its chooser drawn

In `connection_tab`, replace

```rust
        let root = self.text_row("Root path", "rootPath", &self.root.clone(), true, &query, cx);
```

with

```rust
        let root = self.text_row("Root path", "rootPath", &self.root.clone(), true, &query, cx);
        let root = root.child(self.root_browse_button(cx));
```

and replace

```rust
            .child(div().pl(px(4.0)).child(hint_line(cx, &tx(cx, ROOT_HINT), 0.0)))
            .child(web)
```

with

```rust
            .child(div().pl(px(4.0)).child(hint_line(cx, &tx(cx, ROOT_HINT), 0.0)))
            .children(self.chooser_panel(true))
            .child(web)
```

#### A12. `app-gpui/src/settings_view/pages/build_deployment/paths.rs`: a mapping's chooser as text

In `mapping_lines`, replace

```rust
        out.push(tx(cx, MAPPINGS_NOTE));
        out
```

with

```rust
        out.extend(self.chooser_lines(false, cx));
        out.push(tx(cx, MAPPINGS_NOTE));
        out
```

#### A13. `paths.rs`: the Deployment path's Browse… in each row

In `mappings_tab`, replace

```rust
        let can_browse = self.project_root.is_some();
```

with

```rust
        let can_browse = self.project_root.is_some();
        let can_remote = self.can_browse_mapping(cx);
```

and replace

```rust
                            .children(browse)
                            .into_any_element()
```

with

```rust
                            .children(browse)
                            .children((name == "deployment").then(|| {
                                super::remote_folder::mapping_browse_button(index, can_remote, cx)
                            }))
                            .into_any_element()
```

The local column keeps `settings-deployment-mapping-{i}-browse`; the remote one is `settings-deployment-mapping-{i}-remote`.

#### A14. `paths.rs`: a mapping's chooser drawn under the table

In `mappings_tab`, replace

```rust
                    .child(div().min_h(px(72.0)).flex().flex_col().children(rows).children(empty)),
            )
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
```

with

```rust
                    .child(div().min_h(px(72.0)).flex().flex_col().children(rows).children(empty)),
            )
            .children(self.chooser_panel(false))
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
```

(The same `.child(div().text_color(… note)))` line also ends `excluded_tab`; the anchor above includes the line before it, which is only in `mappings_tab`.)

#### A15. `app-gpui/src/workbench/mod.rs`: the end-to-end tests

Inside `mod tests`, after

```rust
    mod proofs35_settings_vim;
```

add

```rust
    mod proofs37_settings_deployment_chooser;
```

Nothing in `custom_controls.rs` changes: the page reads its listing (`folder_lister(cx)`) when Browse… is drawn or pressed, so `DeploymentServersView::new` keeps its signature.

### B. Run Targets onto the shared chooser

#### B1. `app-gpui/src/settings_view/pages/build_run_targets/folder.rs`: replace the whole file

Replace the whole content with the content of `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/remote_chooser_run_targets_folder.rs`. The old 437-line browser goes; `folder.rs` keeps only the page's glue (open, OK into `project_root`, close, the accessors) and re-exports `FolderState`, `needs_host_trust`, `needs_secret` so `build_run_targets.rs` (`pub use folder::FolderState;`), `form.rs` (`folder_lines`, `folder_panel`, `open_folder`) and `sources.rs` (its `RemoteFolders` keyed by SSH configuration) need no change. Selectors (`settings-target-folder-*`, `settings-secret-target.folder`), texts, the single listing and `save=false` are kept by `Flavour::Target`. `needs_host_trust` / `needs_secret` are re-exported only under `#[cfg(test)]` (only proofs30 imports them from `super::folder`).

#### B2. `app-gpui/src/settings_view/pages/build_run_targets/proofs30_run_targets_view.rs`: two accessors now read the chooser

`folder_path` and `folder_state` take `cx: &App` and return owned values (the chooser is an entity now). Replace

```rust
    let state = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.folder_state());
    assert_eq!(state(cx), Some(FolderState::Listed(vec!["app".to_owned(), "src".to_owned()])));
    assert_eq!(view.read_with(cx, |view, _| view.folder_path().map(str::to_owned)).as_deref(), Some("/home/me"));
```

with

```rust
    let state = |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.folder_state(cx));
    assert_eq!(state(cx), Some(FolderState::Listed(vec!["app".to_owned(), "src".to_owned()])));
    assert_eq!(view.read_with(cx, |view, cx| view.folder_path(cx)).as_deref(), Some("/home/me"));
```

and replace

```rust
    assert_eq!(view.read_with(cx, |view, _| view.folder_state()), Some(FolderState::Listed(vec!["srv".to_owned()])));
    assert_eq!(view.read_with(cx, |view, _| view.folder_path().map(str::to_owned)).as_deref(), Some("/"));
```

with

```rust
    assert_eq!(view.read_with(cx, |view, cx| view.folder_state(cx)), Some(FolderState::Listed(vec!["srv".to_owned()])));
    assert_eq!(view.read_with(cx, |view, cx| view.folder_path(cx)).as_deref(), Some("/"));
```

`workbench/tests/proofs30_settings_run_targets.rs` needs no change (it uses `folder_open`, `FolderLister` over the SSH id, and the same selectors).

## (b) Parity keys

| Key                         | Expected                                                                                                                                                                                                                     | Proving tests                                                                                                                                                                                                                                                                                                                                | Missing                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:build.deployment` | partial (done if the integrator accepts the two items in the last column as out of this row)                                                                                                                                 | `workbench::tests::proofs37_settings_deployment_chooser::deployment_root_path_browse_walks_a_local_server_into_a_subfolder_and_applies_it`, `workbench::tests::proofs37_settings_deployment_chooser::deployment_mapping_browse_puts_the_folder_chosen_relative_to_the_root_path_and_applies_it`, plus the five proofs28 tests already listed | The row's text also names "the local chooser's title and start folder" (mappings' local Browse… emits `ControlEvent::Browse { directories, then }`, which carries neither `Choose Folder` nor the start folder of `deployment.tsx:404-411`) and "a busy mark" (Hypothesis: the Spinner beside Test Connection, `deployment.tsx:351`; native draws a static `refresh` icon). Neither is the remote chooser; both are untouched here. |
| `ipc:remote_list`           | done (already done; the two proofs37 e2e tests can be added to its proof: they call `ide_host::ops::deploy::remote_list` through `backend_list()` as `src-tauri/src/commands/deploy.rs:99-104` does, for a local connection) | existing Remote Host tests, plus the two proofs37 e2e tests                                                                                                                                                                                                                                                                                  | none                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `settings:build.runTargets` | done (unchanged)                                                                                                                                                                                                             | existing proofs30 tests, run against the shared chooser after B1/B2                                                                                                                                                                                                                                                                          | none                                                                                                                                                                                                                                                                                                                                                                                                                                |

No webview spec file names the Deployment chooser (`grep` of `src/features/settings/__tests__` and `src/features/deployment` for its texts finds none), so no `unit:` row waits on it.

View tests (not parity proofs by themselves): `settings_view::pages::remote_folder_browser::proofs37_remote_folder_browser::*` (9 tests), `settings_view::pages::build_deployment::proofs37_deployment_chooser_view::*` (4 tests), `settings_view::pages::build_deployment::remote_folder::tests::a_mapping_browses_only_under_an_absolute_root_path`.

## (c) The webview's chooser, behaviour by behaviour

All in `src/features/settings/pages/deployment.tsx`.

- Lists through `commands.remoteList(connection, path)` (`:125`), the connection from `deploymentConnection(server)` of the server as edited, saved or not (`:284`, `:508`). Built: `backend_list()` → `ide_host::ops::deploy::remote_list`, connection from `deployment_connection(&server)`.
- Starts at the field's text trimmed, `~` when empty (`:112`); Root path passes `server.rootPath` (`:367`); a mapping passes the root path without trailing slashes joined with the deployment path without leading slashes (`:509`). Built.
- `~` (or empty) is renamed by `remoteParent(first.path)` (`:128-134`); a relative path is not. Built. The webview then lists the absolute path a second time (`setPath` changes the effect's key, `:118`, `:132`, `:143`); the native chooser takes the absolute path without listing again (same rows, one connection fewer). Left out on purpose; Hypothesis: nobody relies on the second listing.
- Only folders, sorted with `localeCompare` (`:135`). Built (`collate::compare`, off the window's thread).
- No cap on entries (`:135`, `:219`). Built without a cap; only rows in view are drawn (`uniform_list`).
- Late answers dropped (`live` flag and result key, `:119`, `:124-141`). Built (request counter).
- Title `Choose Folder` for a local connection, else `Choose Remote Folder` (`:173`). Built.
- Buttons Cancel then OK; OK disabled on error or when the chosen path is `~` (`:176-179`). Built.
- Parent Folder disabled at `/` or `~`, goes to `remoteParent(path)` (`:189-195`). Built.
- Path field; submit goes to the text trimmed, `~` when empty (`:182-187`). Built (Enter).
- Refresh lists again (`:150-153`, `:197`). Built.
- Rows: folder icon and name; mouse down selects, double click opens (`:219-231`). Built (click selects; Hypothesis: press-versus-release timing is not observable).
- Keyboard of the list, only while folders are listed: Up/Down select (from none: the first), Enter opens the selected, Backspace opens `remoteParent(path)` (`:154-167`). Built via `claim_keys`. Escape closes (the Dialog's `onClose`, `:174`). Built.
- Loading row with a spinner and `Loading…`, `No subfolders` (`:209-218`). Built (busy mark is the `refresh` icon, as the page's Test Connection draws it).
- What OK returns: the selected folder's path, else the path shown (`:168`). Root path takes it as is (`:369-372`); a mapping takes `folderWithin(remoteRoot, path)` and nothing when outside (`:511-515`). Built.
- Errors through `ConnectionProblem` (`:199-201`, `:40-96`): message through `txTemplate` (`:35`), remedy from `sshTestRemedy(kind, message, auth)` with the SSH configuration's auth (`:41-43`); Trust Host Key and Retry (`:63-69`); password field (bullets) with Save checkbox (default off) and Connect disabled while busy or empty (`:70-92`); failure line (`:93`); success lists again (`:53`, `onRetry = reload`). Built, including the password field taking the keyboard (`autoFocus`, `:80`). The problem's state resets once a new listing starts (the component unmounts during Loading); built by clearing on every listing.
- For a local connection the problem is computed with ssh `""` (`:200`). Built (the same remedy rules; a local error never matches them).
- Root path's Browse… disabled for SFTP without an SSH configuration (`:332`). Built (`can_browse_root`); also unavailable in a test build without `DeploymentFolders`.
- A mapping's Browse… needs an absolute root path and, for SFTP, a configuration; its title explains (`:399-401`, `:482-490`). Built.
- The chooser is a modal Dialog (`:171-180`, width 520). Native draws it inline under the field (as Run Targets' does), 520 wide; it closes with another server, another tab, another page, another type/SSH configuration of the server, and (a mapping's) when a mapping is added or removed, since it fills its row by place (the modal made all of those impossible).

`save` in the webview: the Deployment chooser hands `commands.sshSetSecret(ssh, secret, save)` the value of its Save checkbox (`deployment.tsx:75`, state `:45` default `false`, checkbox `:87`). The native Deployment chooser does the same; Run Targets' keeps `false` (`runTargets.tsx:98`).

## (d) Hypotheses (not settled without a compiler)

- `Context::spawn(async move |this, cx| …)` with `this.update(cx, …)` and `cx.background_executor().spawn(…)` in a view: copied from `workbench/build_tools.rs`. Settle: compile.
- `collate::compare` is callable from a background task (`CollatorBorrowed` in a `static OnceLock` should be `Sync`). Settle: compile.
- A `Subscription` dropped inside its own callback (`page.chooser = None` / `page.folder = None` in the `Chosen` handler) is safe in GPUI. Settle: run the OK tests.
- `uniform_list` inside a 240 px box inside the Settings page's scroll area draws its rows and gives their `debug_bounds`. Settle: `ok_hands_back_the_folder_selected_else_the_one_shown`.
- Focus stays on the list's `track_focus` element while its child switches between the rows and `Loading…` (keyboard test). Settle: `the_list_takes_up_down_enter_and_backspace_and_escape_closes_it`.
- `claim_keys` delivers Up/Down/Enter/Backspace/Escape to the list before the Settings dialog's own keys. Settle: same test.
- `TextField` emits `Submitted` on Enter after `set_text` (tests set the path with `set_text`, then press Enter). Settle: `ok_is_unavailable_on_the_unnamed_login_folder_and_after_a_failed_listing`.
- Select-all plus Backspace in the root field emits `Changed` with an empty text. Settle: `browse_needs_a_listing_an_ssh_configuration_and_for_a_mapping_an_absolute_root_path`.
- `Entity<V>` used as `.child(…)` / `into_any_element()` for a view inside a page (as `paths.rs` does with the excluded table). Settle: compile.
- `drop_stale_chooser` assigns `self.chooser = None` in a `let … else` branch while `chooser` (a borrow of `self.chooser`) is used after that branch on the other path; NLL should accept it. Settle: compile; else clone the target and the count first.
- `ErrorKind` serialises to the camelCase names `ssh_test_remedy` expects (`permissionDenied`), as `connection.rs` already relies on.
- The 100,000-entry test's sort takes about a second in a debug build. Settle: time it; shrink to 20,000 if it is slow.
- `bring_into_view` scrolls the page, not the chooser's list, when the list sits just above OK (it scrolls at 40 px above the dialog's OK). Settle: the e2e tests.

## (e) Tests most likely to fail first

- `the_list_takes_up_down_enter_and_backspace_and_escape_closes_it`: depends on focus and key claims inside a test window.
- The two e2e tests: the Settings page's scroll (`bring_into_view`) with a 240 px chooser near the bottom, and macOS temp paths (`/private/var/...`): `trusted_project` canonicalises the project root and the tests type `<root>/server`, so the backend's entries share its prefix.
- `a_password_asked_for_is_typed_as_bullets_and_handed_over_with_save_as_ticked`: the auto-focus uses the window handle stored at render and a deferred `window.focus`.
- `a_folder_of_a_hundred_thousand_subfolders_draws_only_the_rows_in_view`: run time.
- proofs30 Run Targets tests after B1/B2: listings are now sorted off the window's thread, so a test that asserts between a listing and `run_until_parked` would see `Loading…` (none does: `click` parks).

## (f) Look at the real window

Only with a local folder server whose root is a neutral temp folder (e.g. `mktemp -d` with `site/public`, `other` inside); never an SFTP server. Rows of Deployment's list use the theme's `h-list-row` (24 px), Run Targets' stay 22 px; check the hover (`bg-tree-hover`) and selection (`bg-tree-selected`) colours. Check: Root path's Browse… sits right of the field and is faint for an SFTP server without SSH configuration; the chooser opens under the root path's hint, 520 wide, titled `Choose Folder`, path row with Parent Folder / path / Refresh, a 240 px list with folder icons, Cancel then OK at the right; `Loading…` with its mark while listing; selection colour and hover in the list; Up/Down/Enter/Backspace after clicking a row; OK fills Root path; on the Mappings tab the second folder icon beside Deployment path, its tooltip when the root path is relative, and the chooser drawn under the table; the Run Targets page's chooser looks as before (title, OK then Cancel).
