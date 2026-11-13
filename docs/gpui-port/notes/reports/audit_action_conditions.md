# Audit: action `when` / `keyWhen` lost by the manifest generator

Read-only audit of worktree `agent-a307dd904b5ae1f01`. Nothing in the repo was changed. Labels: **[read]** = verified by reading the file (path:line quoted). **[measured]** = output of a read-only node/python script run in this session. **[inferred]** = reasoning, with the check that would confirm it.

## Method and its limits

- The "real" webview conditions come from a read-only node script. It loads the generator's own `staticValue` / `staticObject` / `instances` (sliced out of `docs/gpui-port/gen-data.mjs`) plus `docs/gpui-port/tsmodel.mjs`. It visits the same action literals the generator visits, and adds an independent step: it walks outward from each literal through helper calls, `.map` callbacks and object spreads, and merges the `when` / `keyWhen` those wrappers add, respecting spread order.
- Limit: an id reported "(same)" was resolved by the generator's own `staticValue`, so it is not independently checked. The independent parts are the wrapper walk, the scan for computed or variant conditions, and a manual read of every `const x: Action[] = [...]` and `return [...]` terminal of a shortcut action (vcs/actions.tsx:196→641, vcs/historyActions.tsx:161→642, app/windows.ts:327 via app/index.ts:35, settings/index.ts:62→127, settings/appearanceRuntime.ts:94→settings/index.ts:125, viewers/index.ts:38/62→115, app/projectActions.ts:174→app/index.ts:49, toolwindows/actions.ts:50→235, search/index.ts:389). All of them reach `registerActions` with no condition-adding wrapper **[read]**.
- Spot reads of const-identifier conditions resolved correctly, for example `EDITOR_WHEN` (editor/lsp/actions.ts:33) and `VCS` (vcs/actions.tsx) **[measured: no diff reported]**.
- Native build membership comes from a python walk of `mod x;` lines from `app-gpui/src/lib.rs` and `main.rs`, skipping `#[cfg(test)]` mods: 783 of 823 `.rs` files are reached **[measured]**. Unreached files that matter here: `src/workbench/github.rs`, `src/workbench/debug.rs`, `src/workbench/shelf.rs`, `src/workbench/log_actions.rs`, `src/workbench/vcs_operations.rs` and others. `workbench/mod.rs` has no `mod github` line; `lib.rs:19 pub mod github` is `src/github/`, which has no `registry.register` / `handle` call **[measured]**.
- Side note: the script wrote two intermediates beside this report, `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/analysis.json` and `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/err.txt` (empty). Nothing was written inside the repo.

## 1. The generator

- **Path:** `docs/gpui-port/gen-data.mjs`, with shared resolution in `docs/gpui-port/tsmodel.mjs`.
- **How it is run:** from the repo root, `node docs/gpui-port/gen-data.mjs`; `--check` fails when a file is stale (gen-data.mjs:9-10) **[read]**. No `package.json` script and no `.github` workflow invokes it **[measured: grep]**.
- **Native consumer:** the manifest is embedded at `app-gpui/src/platform/actions.rs:136`. `ActionRegistry::handle` copies the manifest meta as it is (actions.rs:275-283) **[read]**.
- **Unconfirmed claim:** DECISIONS.md:32 says a Rust test fails for every manifest action without a handler. No such test was found: grep of `app-gpui/src` and `app-gpui/tests` for manifest iteration found none **[measured]**.

### Shapes it understands

- **Keymaps:** `keymaps()` (gen-data.mjs:48-83) evaluates `src/platform/keymaps/*.ts` in a vm, so spreads and the Windows translation apply. Presets are `Record<actionId, string[]>`: entries carry **no per-entry `when`** (intellij-macos.ts:5) **[read]**.
- **Actions:** `actionManifest()` (gen-data.mjs:238-271) records an object literal only if it has literal `id` **and** `run` properties and its type satisfies `isAction` (`id`, `title`, `run`; tsmodel.mjs `isAction`) (gen-data.mjs:245-246) **[read]**.
- **Fields:** only `ACTION_FIELDS` (gen-data.mjs:225-236) plus their `…Variants` / `…Key` forms are copied. `when` and `keyWhen` are among them; there is no `enabled` / `isEnabled` field. The webview `Action` type has none either (src/platform/actions.ts:28-56): enablement is `when` only, plus the `projectOpening` gate (actions.ts:127-133) **[read]**.
- **Values `staticValue` resolves** (gen-data.mjs:90-135): string, number and boolean literals; no-substitution template literals; `-n`; arrays; object literals, including a spread of a statically known object; `const` identifiers; factory parameters bound through `instances()`; literal-typed expressions. **[read]**
- **Factories:** `text("id", "Title", run, { when })` resolves through `instances()` (gen-data.mjs:192-221): one record per call site, with parameters bound to arguments **[read]**.
- **Conditionals:** a value like `mac ? "a" : "b"` keeps the first branch as `when` and writes `whenVariants` (gen-data.mjs:177-182) **[read]**.

### Shapes it silently drops

| Shape                                                                                                                                         | Where it is lost                                                                                                                                                                                                                                                                                                       | Example                                                                                                                                   |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| A. The literal is passed to a helper that spreads it over defaults (`tree = (a) => ({ category, when: OPEN, keyWhen: DB_VIEW_FOCUS, ...a })`) | The outer literal has no literal `id`/`run`, so it is never visited (gen-data.mjs:245-246). The inner literal has neither field. `category` is lost too.                                                                                                                                                               | src/features/database/treeActions.ts:199-204 **[read]**                                                                                   |
| B. Value computed by a call or a template with substitutions (`X.map(...).join(" \|\| ")`, `` `${A} && ${f("x")}` ``)                         | `staticValue` returns NONE and `literals()` returns nothing, so the field becomes `$when: "computed"`, which is not in `ACTION_FIELDS`. It is dropped without a warning (gen-data.mjs:183, 249-253).                                                                                                                   | `GENERATE_KEY_WHEN`, src/editor/lsp/actions.ts:59; github `handlerContextKey`, src/features/github/index.ts:211-237 **[read + measured]** |
| C. Shorthand property of a local const (`const when = "projectOpen"; { …, when, … }`)                                                         | Measured: `staticValue` fails for it. Inferred cause: at gen-data.mjs:124, `declarationsOf(node)` uses `getSymbolAtLocation`, which for a shorthand name gives the property symbol, not the variable, so the const branch never runs. Check: log `declarationsOf(node).map(d => ts.SyntaxKind[d.kind])` for that node. | src/features/deployment/actions.tsx:414-521, 11 actions, none with a shortcut **[measured]**                                              |
| D. Spec without `run`, given `run` later in `.map(a => ({ ...a, run }))`                                                                      | Fails the `names.has("run")` test, so the **whole action** is missing from the manifest                                                                                                                                                                                                                                | src/features/github/index.ts:56-58, 251 **[read]**                                                                                        |
| E. Computed id (`` `toolwindow.${id}` ``, `` `navigate.toggleBookmark${d}` ``)                                                                | Goes to `dynamic` with no id (gen-data.mjs:257), so `manifest().get(id)` cannot find it. The condition itself is recorded.                                                                                                                                                                                             | src/workbench/toolwindows/actions.ts:160, 247; src/features/search/index.ts:476, 486 **[read]**                                           |
| F. Parameter of an outer factory forwarded into an inner factory                                                                              | `env.outer` is read at gen-data.mjs:123 but never assigned, so the value becomes NONE, then `$computed`. No shortcut action uses this shape today **[inferred from reading]**.                                                                                                                                         | —                                                                                                                                         |
| G. `registerKeyClaim` (vim, plugin content editor)                                                                                            | A runtime predicate (src/platform/keymap.ts:163-178), outside the manifest. Native has its own `input::claim_keys` (e.g. app-gpui/src/everywhere/structure_window.rs:151). Not comparable statically **[read]**.                                                                                                       | src/features/vim/index.ts:79; src/features/plugins/views/contentEditor.tsx:518                                                            |

## 2. Counts

- **Action ids with a non-empty default shortcut in at least one preset:** 343 **[measured]**.
- **Present in the manifest but with a lost condition:** 16 (15 `db.*` tree actions via shape A, plus `code.generate` via shape B) **[measured]**.
- **Absent from the manifest:** 50 **[measured]**.
  - 45 have computed ids (shape E): 20 bookmark digits, 11 `toolwindow.<id>`, 4 `toolwindow.resize*`, 10 `code.expand(All)ToLevelN`.
  - 5 are GitHub actions missing because they have no `run` (shape D).
- **Of the 16, registered natively:** 9, all hand-guarded **[read]**.
- **Of the 50, registered natively:** 35 (bookmarks, tool windows, resize), all with hand-written conditions that match the webview **[read]**.
- **Dangerous set today (registered + shortcut + condition lost + no hand guard): 0.**
- **Latent dangerous set:** 7 ids, listed in section 4.

## 3. Dangerous set

**Empty.** Every natively registered action whose manifest condition is lost has a hand-written guard:

`app-gpui/src/workbench/database.rs:690-711` **[read]**:

```rust
    for (id, action) in [
        ("db.properties", TreeAction::Properties),
        ... ("db.copyReference", TreeAction::CopyReference), ("db.collapseAll", TreeAction::CollapseAll),
    ] {
        let meta = manifest().get(id)...clone()
            .when(PROJECT_OPEN)
            .key_when(DATABASE_VIEW_FOCUS);
```

| action id          | keys (IntelliJ macOS / Windows) | webview when / keyWhen          | manifest    | native registration                  | effect  |
| ------------------ | ------------------------------- | ------------------------------- | ----------- | ------------------------------------ | ------- |
| db.properties      | Shift+Enter / Shift+Enter       | projectOpen / databaseViewFocus | none / none | database.rs:691, guarded at :706-707 | correct |
| db.refresh         | ⌘R / Ctrl+F5                    | same                            | none        | database.rs:692, guarded             | correct |
| db.disconnect      | ⌘F2 / Ctrl+F2                   | same                            | none        | database.rs:693, guarded             | correct |
| db.editData        | F4 / F4                         | same                            | none        | database.rs:694, guarded             | correct |
| db.goToDdl         | ⌘B / Ctrl+B                     | same                            | none        | database.rs:695, guarded             | correct |
| db.duplicateSource | ⌘D / Ctrl+D                     | same                            | none        | database.rs:696, guarded             | correct |
| db.rename          | ⇧F6 / ⇧F6                       | same                            | none        | database.rs:697, guarded             | correct |
| db.drop            | Delete, ⌘⌫ / Delete             | same                            | none        | database.rs:698, guarded             | correct |
| db.copyReference   | ⌥⇧⌘C / Ctrl+Alt+Shift+C         | same                            | none        | database.rs:699, guarded             | correct |

Source of the webview values: treeActions.ts:44-45 and 199-204 **[read]**. Keys come from `app-gpui/assets/gen/keymaps.json` **[measured]**.

## 4. Other mismatches (not registered natively today)

These would become dangerous as soon as someone ports them with `registry.handle(id, …)`, which copies the manifest meta unchanged (actions.rs:275-283).

### features/database

All six ids get `projectOpen` / `databaseViewFocus` from the webview through `tree()`; the manifest has neither. None is registered natively **[measured: grep for the literal in app-gpui/src]**.

| action id         | keys (IntelliJ macOS / Windows) | where it would wrongly fire once ported                                                 |
| ----------------- | ------------------------------- | --------------------------------------------------------------------------------------- |
| db.new            | ⌘N / Alt+Insert                 | Everywhere, and it competes with project.newFile, services.addService and code.generate |
| db.jumpToConsole  | ⇧⌘F10 / Ctrl+Shift+F10          | Everywhere                                                                              |
| db.quickDoc       | F1 / Ctrl+Q                     | Everywhere, and it would shadow editor Quick Documentation                              |
| db.modifyTable    | ⌘F6 / Ctrl+F6                   | Everywhere                                                                              |
| db.ddlToClipboard | ⌥⇧⌘G                            | Everywhere                                                                              |
| db.ddlToConsole   | ⌥⇧⌘B                            | Everywhere                                                                              |

Same feature, no shortcut, so menus and Find Action only: db.truncate, db.newTable, db.newColumn, db.importData, db.runSqlScript, db.sqlScript, db.goToReferencedTable, db.separateProceduresAndFunctions and db.collapseAll lose `projectOpen` and `category: "Database"` **[measured]**. db.collapseAll is hand-guarded at database.rs:700.

### editor/lsp

- **code.generate** (⌘N / Alt+Insert).
  - Webview: `when` = `EDITOR_WHEN` and `keyWhen` = `GENERATE_KEY_WHEN`, which is `editorLangId == java || … || editorLangId == php` over 21 languages (lsp/actions.ts:39-59, 125, 328) **[read]**.
  - Manifest: keyWhen missing **[measured]**.
  - Native: not registered. The only built non-test hit is the menu entry `app-gpui/src/workbench/menu_spec.rs:438` **[measured]**.
  - Once ported, ⌘N / Alt+Insert in a markdown, yaml or other non-Generate file would run Generate instead of yielding to the New popup (project.newFile).

### Absent from the manifest

**Computed ids (shape E, editor/core):**

- `code.expandToLevel1-5` and `code.expandAllToLevel1-5` (⌘* N / ⇧⌘* N).
- Webview `when`: `editorTextFocus || textEditorActive && popupOpen` (manifest `dynamic`, from editor/core/actions.ts:650/796) **[measured]**.
- Not registered natively; the only hits are menu_spec.rs:477-490 **[measured]**.

**Computed ids, hand-ported correctly [read]:**

- `navigate.toggleBookmark0-9`: `projectOpen && textEditorActive`, at workbench/mod.rs:651-659.
- `navigate.gotoBookmark0-9`: `projectOpen`, at mod.rs:660-668.
- `toolwindow.resize*`: `toolWindowFocus`, at mod.rs:498-516.
- `toolwindow.<id>` (11 ids): no condition in either app (toolwindows/actions.ts:160-168; mod.rs:518-527).

**features/github, no `run` (shape D):**

| action id                           | keys (IntelliJ macOS) | webview when / keyWhen                                                                 |
| ----------------------------------- | --------------------- | -------------------------------------------------------------------------------------- |
| github.createPullRequest            | ⌘N                    | projectOpen / `activeToolWindow == 'pullRequests'`                                     |
| github.refresh                      | ⌘R                    | projectOpen / `… && githubPrTab == 'list'`                                             |
| github.pullRequest.refresh          | ⌘R                    | `projectOpen && githubConnected` / `… && githubPrTab == 'pr'`                          |
| github.pullRequest.timeline.refresh | ⌘R                    | `projectOpen && githubConnected && githubTimelineActive` / `editorAreaFocus`           |
| github.pullRequest.toggleViewed     | ⇧⌘S                   | `projectOpen && githubConnected && (github.toggleViewed \|\| githubReviewDiff)` / none |

- Source: src/features/github/index.ts:29-33, 72-220 **[read]**.
- Hand-written registrations exist in `app-gpui/src/workbench/github.rs:472, 509, 549` (createPullRequest, refresh, pullRequest.refresh), but that file is **not in the build** **[measured]**.
- So none of the five is registered natively today.

## 5. Context keys referenced by lost conditions but never set natively

I looked for `set_context` / `clear_context` / insert calls in built, non-test code **[measured]**.

- **Set natively:**
  - `projectOpen`: workbench/mod.rs:1862
  - `databaseViewFocus`: mod.rs:1888
  - `editorLangId`: mod.rs:1907, 1913
  - `textEditorActive`: mod.rs:1864
  - `editorTextFocus` / `editorAreaFocus`: loop at mod.rs:1872
  - `popupOpen`: mod.rs:1852
  - `activeToolWindow`: mod.rs:1897
  - `toolWindowFocus`: mod.rs:1882
- **Never set in the build:**
  - `githubConnected` and `githubTimelineActive`: only set in the unbuilt workbench/github.rs, at :649.
  - `githubPrTab`: the only hits are doc comments, github/toolwindow.rs:29 and window_view.rs:66.
  - `github.toggleViewed` (from `handlerContextKey`): no hits.
  - `githubReviewDiff`: no hits.
- All of these belong to GitHub actions that are not registered natively, so there is no live effect. Porting any GitHub action that uses them would leave it always disabled until these keys are published.

## 6. Smallest generator change (not applied)

All changes go in `docs/gpui-port/gen-data.mjs`.

1. **Helper spread (A, also fixes the lost `category`).**
   - **Where:** in `actionManifest()`, right after `for (const { record, site } of instances(node))` (gen-data.mjs:247).
   - **What:** walk outward from `site`. While `site` is argument `i` of a call whose callee resolves (`declarationsOf`) to an arrow or function whose returned object literal `R` contains `...p_i` (its `i`-th parameter), replace the record with `staticObject(R, new Map([[symbol(p_i), site]]))` and continue from that call.
   - **Why it is small:** the existing spread branch (gen-data.mjs:141-145) already resolves `...a` through `env` to the inner literal and merges in declaration order, so defaults before the spread and overrides after it come out right with no new merge code. The same walk over a `.map((a) => ({ ...a, … }))` callback covers shape D once the entry test accepts `id` + `title` objects whose `run` arrives through that wrapper.
2. **Make drops loud (B, C, F).**
   - **Where:** at gen-data.mjs:249-253, if `record.$when` or `record.$keyWhen` is `"computed"`, collect `at(site)` and throw at the end, as `--check` does.
   - **Why:** this turns every future silent drop into a generator failure.
   - **What to add to `staticValue`:**
     - A `TemplateExpression` case: head plus spans, each `staticValue`'d and concatenated. With `env` this covers `IN_WINDOW` and `handlerContextKey("x")`, if call-of-local-arrow is bound the way `instances()` binds parameters.
     - A `BinaryExpression` `+` case.
   - **`GENERATE_KEY_WHEN` (`const[].map(tpl).join(sep)`):** either special-case `.map(arrow).join(literal)` over a static array, or have the source export the joined string.
3. **Shorthand (C).**
   - **Where:** gen-data.mjs:124.
   - **What:** when `ts.isShorthandPropertyAssignment(node.parent)`, iterate `symbol.declarations` (the value symbol computed at :120-122) instead of `declarationsOf(node)`.
4. **`env.outer` (F).**
   - **Where:** `instances()`, gen-data.mjs:214.
   - **What:** set `env.outer` to the caller's env, or leave the case to the throw in step 2.

After the change, regenerate. `db.*`, `code.generate`, the deployment actions and the three `handlerContextKey` GitHub actions in the manifest (index.ts:222-240) should gain their conditions. The hand patch at database.rs:702-707 then becomes redundant, though harmless: `manifest().get(id)` would already carry the same `when` / `keyWhen`.
