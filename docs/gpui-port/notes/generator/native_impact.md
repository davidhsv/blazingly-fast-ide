# Native impact of the fixed manifest

Labels: **[read]** = file:line read in this session; **[measured]** = grep / script output from this session; **hypothesis** = not run (no cargo in this task), with the check.

## How the manifest is consumed

- `app-gpui/src/platform/actions.rs:133-140`: `manifest()` deserializes `include_str!("../../assets/gen/actions.json")` (the `actions` array only; `dynamic` is ignored) into `id → ActionMeta` **[read]**.
- `ActionRegistry::handle` / `handle_toggle` (actions.rs:275-295) register `manifest().get(id).clone()` unchanged, so `when`, `keyWhen` and `category` come straight from the manifest **[read]**.
- Other built uses of `manifest()` read single ids (keymap_page.rs:193/206, vcs_diff/compare.rs:459, project_view/actions.rs:407, workbench/mod.rs:853, vcs_ops/actions.rs:107, debugger/actions.rs tests). None of them reads a changed id. Nothing iterates the whole manifest outside the test at actions.rs:405 **[measured: grep `manifest()`]**.

## (i) Natively registered actions whose manifest entry changes

I grepped `app-gpui/src` for each of the 57 changed or added ids as a string literal. Then I checked which hits register a handler (`registry.handle`/`register`) in code that is built **[measured]**:

| ids                                                                                                                                              | native registration                                                                                                                                   | hand-written condition                                                                             | manifest after the fix                                | verdict                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| db.properties, db.refresh, db.disconnect, db.editData, db.goToDdl, db.duplicateSource, db.rename, db.drop, db.copyReference, db.collapseAll (10) | `app-gpui/src/workbench/database.rs:690-711`, `manifest().get(id)….clone().when(PROJECT_OPEN).key_when(DATABASE_VIEW_FOCUS)` then `registry.register` | `PROJECT_OPEN = "projectOpen"` (database.rs:63), `DATABASE_VIEW_FOCUS = "databaseViewFocus"` (:64) | `when: "projectOpen"`, `keyWhen: "databaseViewFocus"` | **Duplicates, no contradiction.** The values are identical, so enablement and shortcuts do not change. The override at :706-707 becomes redundant: the loop could become `registry.handle(id, …)`.                                                |
| same 10                                                                                                                                          | same                                                                                                                                                  | none for category                                                                                  | `category: "Database"` (was absent)                   | **Visible change:** Find Action, the keymap page and anything else that calls `ActionMeta::localized` (actions.rs:83-110) now show the category "Database" (localized through `action.category.Database`) for these 10. This matches the webview. |

No other changed id is registered natively **[measured]**:

- The other 14 `db.*` ids have no native hit apart from the 10 above.
- `code.generate` hits are a test (settings_model/keymap_model.rs:552-570), the vim mapping table (features_a/vim/rc.rs:450), keymap order tests (platform/keymap.rs:608-623) and the menu entry (workbench/menu_spec.rs:438). There is no handler.
- `deployment.*` hits are menu entries (menu_spec.rs:701-711), menu-label building (features_b/deployment/model.rs:219-228, 597) and a notification command (features_b/deployment/actions.rs:23/61). There is no `registry` call in `features_b/deployment` **[measured: grep]**.
- `github.*` handlers exist only in `app-gpui/src/workbench/github.rs` (for example :467, 472, 497, 504, 509, 514, 521, 537, 549, 563, 576). That file is **not compiled**: `workbench/mod.rs` declares no `mod github` (its mod list at :4-47) **[measured]**. `src/github/` (built, lib.rs:19) only names the ids in menus and events, with no `registry.handle`/`register` **[measured: grep]**.

**Actions registered natively whose behaviour changes when the new manifest is applied: only the 10 database tree actions, and only their category (now "Database"). Their enablement and shortcuts do not change.**

## (ii) Context keys the new conditions read that the native app never sets

Keys come from parsing every changed or new `when`/`keyWhen` with a port of the native parser (`whenkeys.mjs`) **[measured]**. For each key I grepped `app-gpui/src` for its literal, excluding the unbuilt workbench/github.rs **[measured]**.

Set natively (built code):

- `projectOpen`: workbench/mod.rs:1862
- `databaseViewFocus`: mod.rs:1888, via `database::DATABASE_VIEW_FOCUS`
- `editorLangId`: mod.rs:1907/1913
- `activeToolWindow`: mod.rs:1897
- `editorAreaFocus`: mod.rs:1872 loop

**Never set in the build:**

| key                                                                                                                                         | read by (new conditions)                                                                            | where it is set, if anywhere                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `githubConnected`                                                                                                                           | when of the 14 changed or added `github.pullRequest.*` actions and `github.selectOpenedPullRequest` | only the unbuilt workbench/github.rs:649                                                                       |
| `githubPrTab`                                                                                                                               | keyWhen of `github.refresh`, `github.pullRequest.refresh`                                           | only the unbuilt workbench/github.rs:650; doc comments at github/toolwindow.rs:29 and github/window_view.rs:66 |
| `githubTimelineActive`                                                                                                                      | when of `github.selectOpenedPullRequest`, `github.pullRequest.timeline.refresh`                     | nowhere (workbench/github.rs:523 reads it, never sets it)                                                      |
| `githubReviewDiff`                                                                                                                          | when of `github.pullRequest.toggleViewed`                                                           | nowhere                                                                                                        |
| `githubCurrentBranchPullRequest`                                                                                                            | when of `github.pullRequest.submitReview`                                                           | nowhere                                                                                                        |
| `github.submitReview`, `github.toggleViewed`, `github.showCurrentBranchPullRequest`, `github.updateReviewBranch`, `github.toggleReviewMode` | when of submitReview, toggleViewed, showInToolWindow, updateBranch, reviewMode                      | nowhere (no `"github.{`/`format!("github.` builder either)                                                     |

**Actions registered natively that a never-set key would disable: none.**

- Every action that reads these keys is a GitHub action, and none is registered in the build (see (i)).
- Once someone ports them with `registry.handle(id, …)`, the manifest conditions apply. These actions then stay disabled until the keys are published. The webview publishes them: `github.<handler>` through `provideGithubHandler` (src/features/github/toolwindow/handlers.ts:33-39), and `githubPrTab` in `activate` (src/features/github/index.ts:259).
- If workbench/github.rs is wired back in as it stands, its hand-written metas win (it builds its own `meta(...)`). But it sets only `githubConnected` and `githubPrTab` (:649-650), so `githubTimelineActive` would still keep `github.selectOpenedPullRequest` disabled. That is already the case with its own meta at :521-523.

## Caveats

- Hypothesis: `editorLangId` values published natively (`editor.language().to_string()`, mod.rs:1907) match the webview ids in `GENERATE_LANGUAGES` (`typescriptreact`, `objective-c`, `csharp`…). This matters only once `code.generate` is ported. Check: compare `LanguageId`'s `Display` with `languages/*.toml` ids.
- Hypothesis: `cargo test` in app-gpui still passes with the new manifest. Every new clause compiles under the ported parser. No test asserts a `db.*` category or an absent GitHub id (grep). Check: `cargo nextest run -p <app-gpui package> platform::actions`.

## Ids behind constants, and other manifest readers

- `deployment.configure` is also `CONFIGURE_ACTION` (features_b/deployment/actions.rs:23). It is used only as a notification command (:61). `github.refresh` is also `REFRESH_ACTION` (github/list.rs:691). It is used only as a row-menu entry (:915). Neither is passed to `registry.handle`/`register` **[measured: grep across app-gpui/src]**.
- `actions.json` is referenced only in platform/actions.rs and workbench/database.rs, across all of `app-gpui/` excluding `assets/` and `target/`. That includes `tests/` and build scripts **[measured]**. Inside actions.rs, `manifest()` is iterated only in the test at :414 **[read: all `manifest` lines of the file]**.
