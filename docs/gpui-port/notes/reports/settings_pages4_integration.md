# Settings pages 4: Project Trust, TODO, Log, GitHub (Project Structure not started)

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Nothing here was compiled. All new files are untracked.

## Status per page

- `projectStructure`: NOT STARTED. No file written. Reason: the native app has no Project Structure view at all (the brief's premise that "a view may exist and only the row is unrecorded" is false: `parity.json:3404` already says "no Project Structure dialog"). The component is `src/features/settings/languages/ProjectStructurePage.tsx`, 2023 lines, six sections plus its own dialog, build-model streaming and a Download JDK dialog; done completely and tested it is several thousand lines of Rust, more than this pass could finish whole, and the brief forbids a half page. What exists natively to build on: `settings_model/staging.rs:147-346` (`open_project_structure`, `ProjectStructureSection`, the dialog's open flag), `settings_model/language_pages.rs:50` (page spec) and `:458-500` (stored-value tests), `settings_model/file_types.rs:324-331`, `platform/project_roots.rs`, `project_view/mark_directory.rs`, `features_b/libraries.rs`, `settings_view/pages/languages_sdk.rs` (`Toolchains`, `SdkDetection` stand-in). Hypothesis (from the component's function and constant names and line ranges, not a reading of every line): what has to be built is section tabs `SECTION_TABS` (`ProjectStructurePage.tsx:97`) and nav groups (`:1851-1869`); Project section (`:295-389`: SDK, language level, content roots `:234`, exclude files `:270`); Modules (`:584-981`, with the Sources tab `:390-583`: mark folders as Sources/Tests/Resources/Excluded `:83` ROLES, paths, dependencies with scopes `:114`); Libraries (`:982-1245`, merged library groups from the build model `:218`); Facets (`:1246-1465`); Artifacts (`:1466-1691`); SDKs (`:1692-1850`, detection through `detectSdks`, Download JDK); the dialog (`:1890-1993`: OK applies then closes, Cancel closes, Apply enabled only with pending changes, Reset of the page, Up/Down move between sections `:1913-1920`, footer "Could not save: …" `:1883`). Hypothesis, not read line by line: machine sources it reads in the real app are detected JDKs/SDKs on the machine (`./api` `detectSdks`), the build model of the project (Maven/Gradle import through `commands.*` with a `Channel`), and the JDK download catalogue (network). Tests would need the `SdkDetection` stand-in `proofs19_settings_misc.rs:166-183` uses, and a stand-in for the build model.
- `projectTrust`: WHOLE (view, wiring, tests).
- `editor.todo`: WHOLE (view, storage model, wiring, tests); needs the `todo_window.rs` hunks below.
- `vcs.log`: WHOLE (view, wiring, tests).
- `vcs.github`: WHOLE (view, wiring, tests).

## Files written

- `app-gpui/src/settings_view/pages/security_project_trust.rs` (260 lines): `ProjectTrustView::new(Binding, cx)`, `set_lists(TrustLists)`, `set_error(String)`, `lists()`, `error()`, `lines(cx)`; event `ProjectTrustEvent::{Revoke(String), RemoveLocation(String), AddLocation}`; `TrustLists::from_decisions(&TrustDecisions)`; selectors `settings-trust-revoke-{i}`, `settings-trust-remove-{i}`, `settings-trust-add`, `settings-trust-error`. 2 view tests.
- `app-gpui/src/settings_view/pages/editor_todo.rs` (548 lines): `STORAGE_KEY = "openidea.todo.v1"`, `parse_todo_settings`, `todo_settings_json`, `stored_todo_settings(cx)`, `new_pattern_id`; `TodoSettingsView::new(Binding, TodoSettings, cx)`, `settings()`, `keyword_field(i)`, `lines(cx)`, and the actions `toggle_case_sensitive`, `toggle_comments_only`, `toggle_enabled(i)`, `set_color(i, color)`, `add_pattern`, `remove_pattern(i)`, `restore_defaults`; event `TodoSettingsEvent::{Changed(TodoSettings), Submit, Cancel}`; selectors `settings-todo-case-sensitive`, `settings-todo-comments-only`, `settings-todo-add`, `settings-todo-enabled-{i}`, `settings-todo-keyword-{i}`, `settings-todo-color-{i}`, `settings-todo-color-{i}-{blue|red|yellow|green|violet|grey}`, `settings-todo-remove-{i}`, `settings-todo-restore`. 2 model tests, 2 view tests.
- `app-gpui/src/settings_view/pages/vcs_log.rs` (214 lines): `stored_log_options(cx)`, `labels()`; `LogSettingsView::new(Binding, cx)`, `flip(name)`, `lines(cx)`; event `LogSettingsEvent::Changed(Box<LogOptions>)`; selectors `settings-log-{compactRefs|tagNames|commitTimestamp|refsOnLeft|showLineStats|showAuthor|showHash|showDate|showGitHubChecks}`. 1 view test.
- `app-gpui/src/settings_view/pages/vcs_github.rs` (140 lines): `stored_review_settings(cx)`, `review_settings_with_auto_mark(cx, on)`; `GithubSettingsView::new(Binding, cx)`, `auto_mark_viewed(cx)`, `lines(cx)`; event `GithubSettingsEvent::SetAutoMarkViewed(bool)`; selector `settings-github-autoMarkViewed`. 2 tests.
- `app-gpui/src/workbench/settings_feature_pages.rs` (247 lines): `Workbench::mount_feature_settings_pages(&dialog, window, cx) -> Vec<Subscription>` and the four event handlers; the trust store work (`carry_out`).
- `app-gpui/src/workbench/tests/proofs31_settings_feature_pages.rs` (378 lines): 10 end-to-end tests.

## (a) Hunks, in order, one group per page

### Group 0 (shared, apply first; needed by every page below)

1. `app-gpui/src/workbench/mod.rs`, module list. Anchor (exact):

```
mod services_window;
mod settings_window;
```

Replace with:

```
mod services_window;
mod settings_feature_pages;
mod settings_window;
```

2. `app-gpui/src/workbench/settings_window.rs`, in `open_settings`. Anchor (exact):

```
        subscriptions.extend(self.mount_plugin_settings_pages(&dialog, has_project, window, cx));
```

Add right after it:

```
        subscriptions.extend(self.mount_feature_settings_pages(&dialog, window, cx));
```

3. `app-gpui/src/workbench/mod.rs`, inside `mod tests`. Anchor (exact):

```
    mod proofs26_save_all;
```

Add right after it:

```
    mod proofs31_settings_feature_pages;
```

`settings_feature_pages.rs` names all four page modules, so to land one page at a time either apply all of groups 1-4 together, or delete the other pages' `cx.new`/`mount_page`/`subscribe_in` lines and handlers from `mount_feature_settings_pages` and their tests from `proofs31_settings_feature_pages.rs` until their group lands.

### Group 1: Project Trust

`app-gpui/src/settings_view/pages/mod.rs`. Anchor (exact):

```
pub mod notifications;
pub mod terminal;
```

Replace with:

```
pub mod notifications;
pub mod security_project_trust;
pub mod terminal;
```

### Group 2: TODO

1. `app-gpui/src/settings_view/pages/mod.rs`. Anchor (exact):

```
pub mod editor_font;
pub mod font_picker;
```

Replace with:

```
pub mod editor_font;
pub mod editor_todo;
pub mod font_picker;
```

2. `app-gpui/src/workbench/todo_window.rs`, imports. Anchor (exact):

```
use crate::features_a::todo_view::{TodoCommand, TodoData, TodoView, TodoViewEvent};
```

Add right after it:

```
use crate::settings_view::pages::editor_todo::{STORAGE_KEY as TODO_SETTINGS_KEY, stored_todo_settings, todo_settings_json};
```

3. `app-gpui/src/workbench/todo_window.rs`, `sync_todo_window`: the window reads the stored settings when first shown (today it always starts from the defaults, so nothing the TODO page stores would ever be matched). Anchor (exact):

```
            let data = TodoData { project_root: root, custom_scopes, ..TodoData::default() };
```

Replace with:

```
            let settings = stored_todo_settings(cx);
            let data = TodoData { project_root: root, custom_scopes, settings, ..TodoData::default() };
```

4. `app-gpui/src/workbench/todo_window.rs`, the method the page's changes go through. Anchor (exact):

```
    /// Scans the Scope Based tab's scope, in place of a scan of another scope still running.
```

Insert right before it:

```
    /// Settings › Editor › TODO changed the patterns or the matching options: the tool window
    /// matches with them from now on; the open editors and the project are scanned again, and the
    /// Scope Based tab's scope on its next showing.
    pub(super) fn set_todo_settings(&mut self, settings: TodoSettings, cx: &mut Context<Self>) {
        let Some(feed) = self.todo.as_mut() else { return };
        if feed.data.settings == settings {
            return;
        }
        feed.data.settings = settings;
        feed.data.scoped_scope = None;
        self.scan_open_editors(cx);
        self.scan_todos(cx);
    }

```

5. `app-gpui/src/workbench/todo_window.rs`, `TodoViewEvent::ToggleMatchCase`: the tool window's Match Case is the same stored setting as the page's Case sensitive (`TodoView.tsx:336` calls `updateTodoSettings`), so it is stored too. Anchor (exact, the three lines inside the `if let`):

```
                    feed.data.settings.case_sensitive = !feed.data.settings.case_sensitive;
                    feed.data.scoped_scope = None;
```

Replace with:

```
                    feed.data.settings.case_sensitive = !feed.data.settings.case_sensitive;
                    feed.data.scoped_scope = None;
                    let stored = todo_settings_json(&feed.data.settings);
                    cx.default_global::<UiStorage>().set(TODO_SETTINGS_KEY, stored);
```

(`UiStorage` is already imported in `todo_window.rs:16`.)

### Group 3: Log

`app-gpui/src/settings_view/pages/mod.rs`. Anchor (exact):

```
pub mod vcs_issue_navigation;
pub mod vcs_mappings;
```

Replace with:

```
pub mod vcs_issue_navigation;
pub mod vcs_log;
pub mod vcs_mappings;
```

### Group 4: GitHub

`app-gpui/src/settings_view/pages/mod.rs`. Anchor (exact):

```
pub mod vcs_git;
pub mod vcs_issue_navigation;
```

Replace with:

```
pub mod vcs_git;
pub mod vcs_github;
pub mod vcs_issue_navigation;
```

## (b) Parity rows

None of the `settings:` keys exist yet in `parity.json` (grep of each returned 0); the format is `settings:<id>` as `settings:appearance`, `settings:keymap`, `settings:build.debugger`. Likewise `ipc:project_trust_list` and `ipc:project_trust_add_location` are new keys (INVENTORY rows exist at lines 2313-2314).

| Key                                                 | Expected          | Proving tests (full names)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Missing                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `settings:projectTrust`                             | done              | `workbench::tests::proofs31_settings_feature_pages::revoke_trust_on_the_project_trust_page_restricts_the_project_at_once`, `…::adds_a_picked_folder_as_a_trusted_location_from_settings`, `…::add_folder_trusts_the_undecided_project_open_inside_it`, `…::remove_on_the_project_trust_page_restricts_the_projects_inside_the_location`, `…::a_missing_folder_is_not_trusted_and_the_page_says_why`, `settings_view::pages::security_project_trust::tests`                                                                                                                                                                                                                                                          | The page has no staged values (decisions take effect at once, `index.tsx:157`), so Apply/Reset do not apply; Cancel is shown to keep the decision.                                                                                                                                                             |
| `settings:editor.todo`                              | done              | `workbench::tests::proofs31_settings_feature_pages::todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them`, `settings_view::pages::editor_todo::tests`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Stored at once (`store.ts:27-34`): no Apply/Reset; Cancel keeps. Remove Pattern is proven in the view test only.                                                                                                                                                                                               |
| `settings:vcs.log`                                  | done              | `workbench::tests::proofs31_settings_feature_pages::edits_the_logs_display_options`, `settings_view::pages::vcs_log::tests`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Stored at once (`logOptions.ts:178-190`): no Apply/Reset; Cancel keeps.                                                                                                                                                                                                                                        |
| `settings:vcs.github`                               | done              | `workbench::tests::proofs31_settings_feature_pages::registers_independently_of_the_pull_request_runtime`, `…::updates_an_open_page_when_another_window_saves_or_clears_review_settings`, `…::ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings`, `settings_view::pages::vcs_github::tests`                                                                                                                                                                                                                                                                                                                                                                                                 | Nothing native reads `autoMarkViewed` yet (no pull request diff view uses `ReviewDiffSettingsStore` outside `github::diff`/`github::review_diff` tests), so the stored option has no effect in this app until that view is built.                                                                              |
| `settings:projectStructure`                         | not started       | none                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Everything; see the status section.                                                                                                                                                                                                                                                                            |
| `unit:src/features/github/settingsPages.test.tsx`   | done, 3 of 3      | registers independently of the pull request runtime → `workbench::tests::proofs31_settings_feature_pages::registers_independently_of_the_pull_request_runtime`; updates an open page when another window saves or clears review settings → `…::updates_an_open_page_when_another_window_saves_or_clears_review_settings` (and the model test `github::diff::tests::updates_an_open_page_when_another_window_saves_or_clears_review_settings`); ignores unrelated storage changes and recovers from invalid saved settings → `…::ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings` (and `github::diff::tests::ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings`) | Case 1's "dispose unregisters" has no native counterpart (pages are a fixed registry).                                                                                                                                                                                                                         |
| `unit:src/features/project-trust/trust.test.tsx`    | partial, 4 of 7   | the three already listed in the row, plus adds a picked folder as a trusted location from settings → `workbench::tests::proofs31_settings_feature_pages::adds_a_picked_folder_as_a_trusted_location_from_settings`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Still missing as the row says: the canonical folder through a symlink, a local opening's decision on the client (two `it.each` cases), an attached member whose trust is lost.                                                                                                                                 |
| `ipc:project_trust_list`                            | done              | `workbench::tests::proofs31_settings_feature_pages::revoke_trust_on_the_project_trust_page_restricts_the_project_at_once`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | The page lists the root that only the backend store's `TrustStore::decisions()` (what `src-tauri/src/commands/app.rs:322` calls) holds, read from the temporary data folder.                                                                                                                                   |
| `ipc:project_trust_add_location`                    | done              | `workbench::tests::proofs31_settings_feature_pages::add_folder_trusts_the_undecided_project_open_inside_it`, `…::adds_a_picked_folder_as_a_trusted_location_from_settings`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | The native path calls `host.trust.add_location` then `host.refresh_project_trust()`, as the adapter (`app.rs:343-346`) does; the first test sees the undecided project's window leave Restricted Mode, which only the refresh after the store's addition produces, and reads the location back from the store. |
| `unit:src/features/vcs/__tests__/logPanes.test.tsx` | partial, 12 of 13 | the 11 already in the row, plus edits the log's display options → `workbench::tests::proofs31_settings_feature_pages::edits_the_logs_display_options`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | The Graph Options case (its view is not built).                                                                                                                                                                                                                                                                |

Specs that render these pages (grep of `src/` and `e2e/` for the component names and page ids): `trust.test.tsx`, `settingsPages.test.tsx` and `logPanes.test.tsx` (its `describe("Settings › Version Control › Log")` case at `:287-297`). No spec renders `TodoSettingsPage`; `todo.test.ts` and `logOptions.test.ts` test the TODO and log models, whose rows are unchanged by this work.

## (c) Behaviours of each webview page

Project Trust, `src/features/project-trust/index.tsx:124-192`:

- `:127-133` the decisions are read when the page opens: built (`TrustRequest::Read` in `mount_feature_settings_pages`).
- `:156-158` intro text: built.
- `:159` error paragraph (`String(e)`): built (`settings-trust-error`; the error's message is shown, never cleared until reopening, as `error` is never reset there).
- `:160-170` Trusted projects, each with Revoke Trust running `project.revokeTrust` with the path, then a refresh: built. The native page does the action's work itself on the backend (`host.set_project_trust(path, false, false)` and the project plugins restricted, as `app_actions.rs:1056-1072` `set_trust` does) rather than running the registered action, because the action runs detached and the page must refresh only after it finished; the result is the same store write and `ProjectTrustChanged`.
- `:171-178` Trusted locations header with Add folder…: built; the chooser is a directory chooser (`plugin-dialog open({directory: true})`); its title "Add Trusted Location" is left out: GPUI's `PathPromptOptions` has only a button label (`prompt`), not a title.
- `:142-153` Add folder…: `projectTrustAddLocation` then refresh, a dismissed chooser does nothing: built.
- `:179-181` the hint: built.
- `:182-189` each location with Remove running `project.removeTrustedLocation`: built, same remark as Revoke (work done as `app_actions.rs:431-452` does).
- Paths: the webview's `.path` wraps anywhere (`trust.module.css:67-69`); built the same, wrapping beside the button.

TODO, `src/features/todo/TodoSettingsPage.tsx`:

- `:26-30` Case sensitive, `:31-35` Only in comments: built.
- `:37-52` Patterns header, Add Pattern (`p` + base-36 time, `NOTE`, green, enabled): built; the id is made unique when two are added in one millisecond (the webview would repeat it).
- `:56-60` a pattern's checkbox: built. `aria-label` "Enable {name}" left out (no accessibility tree natively).
- `:61-67` keyword field, whitespace removed as typed: built.
- `:68` swatch: built. `:69-74` colour select (Blue, Red, Yellow, Green, Violet, Grey, `:8-15`): built; `aria-label` "{keyword} colour" left out.
- `:75-80` Remove Pattern: built.
- `:84-88` Restore Defaults (patterns only; the two options stay): built.
- `store.ts:27-34` every change stored at once under `openidea.todo.v1`: built (workbench writes `UiStorage`).
- `store.ts:298-307` the TODO index rescans when patterns or options change: built through the `set_todo_settings` hunk.
- `store.ts:36-43` another window's save is picked up: not built for the TODO window (it reads storage when first shown; an open TODO window follows only this window's page). Hypothesis: one app process shares one `UiStorage`, so a second project window's page change reaches the other window's TODO window only when it is shown again; settle by a two-window test if needed.
- `highlight.ts:106-111` editor highlighting follows the settings: not applicable, the native editor does not highlight TODO comments (INVENTORY row of `todo.test.ts` says so).

Log, `src/features/vcs/LogSettingsPage.tsx`:

- `:9-15` five option checkboxes, `:17-22` four column checkboxes under "Visible columns:" (`:33-38`): built, labels exact.
- `:28` `setLogOptions` stores at once (`logOptions.ts:178-190`) and the open log shows it: built (`Workbench::set_log_options`, `log_window.rs:446`).
- `logOptions.ts:129-133` another window's save reloads: the page itself follows the storage (`observe_global::<UiStorage>`); the open Git log follows only changes made through `set_log_options` (this window's page or its own menus).

GitHub, `src/features/github/diff/GithubSettingsPage.tsx`:

- `:13-17` the one checkbox, stored at once in `openidea.github.reviewDiff.v1` with the other review settings kept: built.
- Accounts: the page has none (`:2-3` "accounts are gh's"); the brief's "accounts" premise is wrong. No `gh`, no network, no secret.

## Brief items that turned out wrong

- The brief's page criterion "Apply, and Reset and Cancel discard" does not fit these four pages: in the webview each applies at once (`index.tsx:157` "take effect immediately", `store.ts:27-34`, `logOptions.ts:178-190`, `GithubSettingsPage.tsx:2-3` "Applies at once, like the Log page"). The tests assert the effect at once and that Cancel keeps it; the rows are "done" only under those semantics.
- "vcs.github … accounts": the page has no accounts (one checkbox); no `gh` stand-in is needed.
- "projectStructure: if a view exists": none exists.
- Project Trust has no secret and is not gated: `project_trust_` commands are allowed in Restricted Mode (`crates/ide-host/src/trust.rs:223`), so the native page calls the store in any window, as the adapter allows.

## Model addition to make later

- `parse_todo_settings`, `todo_settings_json`, `stored_todo_settings` and `STORAGE_KEY` live in `settings_view/pages/editor_todo.rs` because `features_a/todo.rs` could not be edited; the `todo_window.rs` hunk then imports a settings page from the workbench, which inverts the layering. Move the four items to `features_a/todo.rs` (next to `TodoSettings`) and import them from there in both places.

## Layout

Sizes follow the CSS modules: `TodoSettingsPage.module.css` (page gap 10, options gap 18, header `fg-muted`, list border `border-strong`, keyword 120 px, swatch 10 px), `LogSettingsPage.module.css` (gap 10, columns 18 px in, header `fg-muted`), `trust.module.css` (path wraps anywhere, hint `fg-secondary`). The TODO colour list closes on a press elsewhere on the page, as `FormView`'s lists do (`pages/mod.rs:1198-1205`); Escape does not close it (nor does it in `FormView`).

## (d) Hypotheses (not settled without a compiler)

- Hypothesis: a type can implement both `EventEmitter<TodoSettingsEvent>` and be subscribed with `cx.subscribe_in(&view, window, Self::handler)` where the handler's event type selects the emitter, as `plugin_contributions.rs:832` does with `PluginSettingsEvent`. Settle: compile.
- Hypothesis: non-capturing closures in the `static OPTIONS`/`static COLUMNS` initialisers of `vcs_log.rs` coerce to the `fn` pointer fields. Settle: compile; if refused, write nine small `fn`s.
- Hypothesis: `fn(&mut Self, &mut Context<Self>)` accepts `Self::toggle_case_sensitive` in `editor_todo.rs` `toggle_row`. Settle: compile.
- Hypothesis: `row(cx, ..).id(&'static str)` and `.id(SharedString)` give a `Stateful<Div>` that takes `.debug_selector` and `.on_click` in that order, as `general_date_formats.rs:139-145` does. Settle: compile.
- Hypothesis: revoking the trust of the window's project does not close the Settings dialog (`drop_settings` runs when the project goes or another opens, `settings_window.rs:255`). If it does, the revoke test's final `click_on(cx, "settings-cancel")` fails; settle by running the test, and drop the last three lines if the dialog closes.
- Hypothesis: `set_page_labels` takes English labels and the dialog's search translates them as it does the Keymap's (`settings_window.rs:227-228`). Settle: search "Trusted locations" in a test or the real window.
- Hypothesis: `host.trust.add_location` of a missing folder fails (it canonicalizes first, `trust.rs:111-113`), so the error test sees a non-empty message; measured by reading, not by running.
- Hypothesis: `git_project(cx, init_repo)` makes a repository the Git window opens with a log; the log test only needs `GitLog::options()`, which does not depend on commits being read.
- Hypothesis: typing into the TODO keyword field with `simulate_input` after `select_all` and `window.focus` emits one `Changed` per character, as the `type_over` helper of `pages/mod.rs:1300-1308` relies on.

## (e) Tests most likely to fail first

- `todo_patterns_and_options_are_stored_at_once_and_the_todo_window_matches_with_them`: it depends on the `todo_window.rs` hunks; the exact TODO line ends (`"TODO first"`, `"NOTE later"`, `"BUG: in code"`) depend on `items_from_matches` text extraction (`features_a/todo.rs`); the third line `let x = 1; BUG: in code` is matched only once Only in comments is off. Also the colour drop-down's choice must be clickable over the page (deferred, priority 1).
- `revoke_trust_on_the_project_trust_page_restricts_the_project_at_once`: see the dialog-closing hypothesis.
- `edits_the_logs_display_options`: `toolwindow.git` must have mounted the `GitLog` before Settings opens; the index of the Hash line (7) assumes the line layout of `LogSettingsView::lines`.

## (f) What a look at the real window must check, and machine sources

- Project Trust: the page lists the real trust store `project-trust.json` in the app's data folder (`trust.rs:29`); on first run without that file it is migrated from the recent projects list (`trust.rs:33-39`). Check that Revoke Trust turns on the Restricted Mode banner of the window whose project it was, and Add folder… opens the system folder chooser. No other machine source.
- TODO: reads and writes `ui-storage.json` in the configuration folder (`platform/storage.rs:11`), key `openidea.todo.v1`; the TODO tool window then runs a project search. Check the colour swatch tokens render (`syn-annotation` for Violet) and the drop-down sits over the next rows.
- Log: `ui-storage.json`, key `openidea.vcs.logOptions`; the Git log redraws with tag names and the Hash column.
- GitHub: `ui-storage.json`, key `openidea.github.reviewDiff.v1`. No account, no `gh`, no keychain.
- No page here reads SSH configuration, Docker, WSL or detected SDKs, and none holds a secret: nothing is drawn masked, nothing is stored in the keychain (the webview pages store none either: the trust store is a JSON file of paths, the other three are `localStorage` keys).

## (g) What these pages, the SSH page and the Deployment page should share

- The four pages here and every page drawn by a view of its own repeat "open on page, downcast `page_view`" in tests: one `open_page::<V>` helper in `workbench::tests` (now private copies in `proofs19_settings_misc.rs`, `proofs30_settings_run_targets.rs` and here) would serve all.
- Immediate-effect pages (these four) versus staged pages (SSH, Deployment): a page-level flag on `PageSpec` saying "applies at once" would let the dialog say so and keep Reset/Apply visibly off; today it is only implicit.
- The folder chooser answer (`ControlEvent::Browse` in `custom_controls.rs:340-353`) and `add_trusted_location` here do the same `prompt_for_paths` dance; a shared `pick_folder(cx) -> Task<Option<PathBuf>>` would remove both copies (and the run targets page's).
- Error lines in a page (`settings-trust-error` here, the SSH and Deployment pages' Test Connection results) could use one `error_line(cx, text)` helper next to `hint_line` in `pages/mod.rs`.
