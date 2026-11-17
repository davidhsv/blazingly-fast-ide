# I-ACTIONS-ENABLED: an action's `enabled(ctx, arg)` predicate, `indicatesRunning`, batched registration

Everything below was read at HEAD `13c01811f` (webview files from the worktree, `app-gpui/` files through `git show HEAD:`). A statement that was not read or counted is marked Hypothesis. Line numbers are HEAD's.

## (a) The webview's behaviour

### The registry (`src/platform/actions.ts`, 260 lines)

- [ ] `Action.enabled?: (c: ActionContext) => boolean` beside `when` (`actions.ts:37-40`); `ActionContext = { ctx: ContextKeys, arg?: unknown }` (`:22-26`).
- [ ] `isActionEnabled(action, ctx = getContext(), arg?)` (`:164-171`): first `projectOpening` (only `app.quit`, `window.next`, `window.previous`, `window.toggleFullScreen` stay), then `evaluateWhen(action.when, ctx)`, then `action.enabled?.({ ctx, arg }) ?? true`. Short-circuit order: projectOpening, `when`, predicate.
- [ ] `isActionKeyEnabled(action, ctx)` (`:174-176`) = `isActionEnabled(action, ctx)` with **no arg**, then `keyWhen`. So the predicate does run for key dispatch, with `arg === undefined`.
- [ ] `runAction` (`:217-249`) checks `isActionEnabled(action, ctx, arg)` with the invoker's arg (`:224`) and returns `false` (nothing runs, no listener hears it) when it fails. Spec: "checks argument-specific enablement before dispatch".
- [ ] `indicatesRunning?: boolean` (`:59-64`): "toolbar buttons of this action show a spinner and ignore clicks until `run` settles". `useRunningActions` (`:75`) holds `id → count` of runs in flight, per webview (so per window); `trackRun(id, ±1)` (`:77-85`) deletes the entry at 0. `runAction` increments before `run` (`:232`) and decrements in `finally` after `await pending` (`:246`), on success and on failure. A run that is refused (unknown or disabled) is never counted.
- [ ] `registerActions(actions)` (`:142-154`): one store update for the whole batch (the comment says per-action cloning was O(n²)); returns one disposer that removes each action only if it is still the registered one (`addSource`, `:122-133`).
- [ ] `keymap?: boolean` (`:52-53`): a `hidden` action still listed in Settings › Keymap. This flag is Q-FU4's (its brief item 2), not this slice's; see (d).

### The spec (`src/platform/actions.test.ts`, 5 cases)

1. `counts a run of an \`indicatesRunning\` action until it settles, on success and on failure` (`:14`): two concurrent runs count 2, then 1, then 0 and the id is gone from the map; a failing async run ends at 0.
2. `does not track actions that did not ask for it` (`:52`).
3. `propagates host launch errors while retaining ordinary action error handling` (`:66`): already proved natively (`workbench::tests::proofs20_shell::propagates_host_launch_errors_while_retaining_ordinary_action_error_handling`, `app-gpui/src/workbench/tests/proofs20_shell.rs:19`).
4. `checks argument-specific enablement before dispatch` (`:85`): `enabled: ({arg}) => arg === "/tracked.txt"`; executing with `/untracked.txt` returns false and does not run; with `/tracked.txt` runs once with that arg.
5. `registers a batch with one store update and disposes every action` (`:103`).

### Every action with an `enabled` predicate: 42 ids at 29 property sites (measured)

Found with `git grep -n -P "^\s*enabled\??\s*[:(]"` plus single-line and shorthand patterns over `src/**/*.ts(x)` without tests, each hit read in place. `indicatesRunning` has exactly one site.

| Ids                                                                                                                                               | Site                                                                          | What the predicate reads                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `db.toggleBooleanCell`, `db.setCellNull`, `db.setCellDefault`, `db.localFilter`, `db.editCell`                                                    | `src/features/database/cellActions.ts:68,76,84,92,100`                        | `handle(c)` = `arg as CellActions`, else the grid that last took focus (`active`, set by a `focusin` listener on `[data-db-cell-grid]` outside inputs, `:52-60`); then that grid's `canToggle()` / `canSetNull()` / `canSetDefault()` / `canFilter()` / `canEditCell()`. All five have `keyWhen: "dbCellGridFocus"`, no `when`.                                                                                                                                    |
| `db.copyAsTsv`, `db.copyAsCsv`, `db.copyAsOneRow`, `db.copyWithHeader`, `db.copyRowsAsTsv`, `db.copyRowsAsCsv`                                    | `cellActions.ts:108` (mapped over `COPIES`, `:32-39`)                         | the same grid's `hasSelection()`.                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `db.reloadPage`, `db.firstPage`, `db.previousPage`, `db.nextPage`, `db.lastPage`, `db.addRow`, `db.exportData`, `db.deleteRows`, `db.restoreRows` | `src/features/database/index.ts:338` (mapped over `TABLE_COMMANDS`, `:66-76`) | `tableTarget(arg)` → `{ rows, editor }`; `(editor ?? focusedTableEditor())?.commands?.[command]?.enabled(rows)`. The host's own `enabled`s: `TableEditor.tsx:239-252` (`!loading`, `canPrevious`, `canNext`, `editable`, `!!page`, `editable && rows.length > 0`, `rows.some(deleted)`) or `ResultsView.tsx:463-478` (`!busy`, pager, `editable && !busy`, `rowsEditable`). `run` re-checks `handler.enabled(rows)` (`:341-345`). `keyWhen: "dbTableEditorFocus"`. |
| `db.closeResultTab`, `db.pinResultTab`, `db.renameResultTab`                                                                                      | `src/features/database/resultTabActions.ts:47,85,97` (`closable`, `:11-14`)   | `target(c)` (`:6-9`): `arg` if a number, else `ctx.dbResultsFocus ? activeResult : null`, kept only if it is the Output tab or an existing result; closable excludes the Output tab. Reads a context key **and** the database store.                                                                                                                                                                                                                               |
| `db.closeOtherResultTabs`                                                                                                                         | `resultTabActions.ts:55`                                                      | target exists and another result exists.                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `db.closeAllResultTabs`, `db.nextResultTab`, `db.previousResultTab`                                                                               | `resultTabActions.ts:63,112`                                                  | target exists and `results.length > 0`.                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `db.closeAllButPinnedResultTabs`                                                                                                                  | `resultTabActions.ts:71`                                                      | target exists, some result pinned and some not.                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `debug.closeTab`, `debug.closeOtherTabs`, `debug.closeAllTabs`, `debug.pinTab`                                                                    | `src/features/debugger/tabActions.ts:46,53,63,71`                             | `target` (`:13-18`): arg session id or the active one, mapped to its tab's owner; `owners().length > 1`; `owners().length > 0`; `pinnable` (a session with no parent, `:35-38`).                                                                                                                                                                                                                                                                                   |
| `run.closeTab`, `run.closeOtherTabs`, `run.closeAllTabs`, `run.pinTab`                                                                            | `src/features/run/runTabs.ts:46,53,64,72`                                     | `target` (`:33-36`): arg string or `activeSession`, kept if in `runTabIds()` (sessions less archived agent tabs, then stack-trace tabs, `:10-17`); `runTabIds().length > 1`; `> 0`; `session(target)` exists (a stack-trace tab is not pinnable).                                                                                                                                                                                                                  |
| `project.openInRightSplit`                                                                                                                        | `src/features/project-tree/actions.ts:303`                                    | `selectedFiles().length > 0` (folders keep Enter's expand). `when: FOCUS`.                                                                                                                                                                                                                                                                                                                                                                                         |
| `vcs.compareWithRevision`, `vcs.showCurrentRevision`                                                                                              | `src/features/vcs/actions.tsx:497,640`                                        | `hasFileHistory(argPath(arg) ?? activePath())` (`src/features/vcs/fileRevisions.ts:43`). `when: VCS`.                                                                                                                                                                                                                                                                                                                                                              |
| `vcs.showDiffForLines`                                                                                                                            | `vcs/actions.tsx:536`                                                         | the active editor has VCS hunks in its selection.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `diff.focusOppositePane`                                                                                                                          | `src/features/vcs/compare.tsx:201`                                            | the focused diff viewer has more than one pane. `when: "diffEditorFocus"`.                                                                                                                                                                                                                                                                                                                                                                                         |
| `diff.showInExternalTool`                                                                                                                         | `compare.tsx:227`                                                             | `workflowSettings().externalDiff.enabled && !!path`.                                                                                                                                                                                                                                                                                                                                                                                                               |
| plugin commands (dynamic ids)                                                                                                                     | `src/features/plugins/points.tsx:213`                                         | `indicatesRunning: true` (no predicate); `run` awaits `runPluginCommand`.                                                                                                                                                                                                                                                                                                                                                                                          |

Not action predicates although they match the word: `src/features/vcs/DiffEditor.tsx:454` and `src/workbench/softWrap.ts:5` (a soft-wrap surface's `enabled()`), `src/features/vcs/MergeEditor.tsx:117` and `mergeActions.ts:41` (the merge host publishes its enablement as `merge.<command>` context keys, `mergeActions.ts:48-52`, and the actions gate on `when`, `:69`), `src/editor/lsp/actions.ts:158` (a save participant).

### Every place the webview shows or runs an action, and what "disabled" does there

| Surface                                     | Code                                                                                                                                                                                              | Evaluates                                                                                                                                    | Disabled looks / does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Key dispatch                                | `src/platform/keymap.ts:474-479` (`runIfEnabled`), `:486-496` (mouse), `:543-553` (double tap), `:605-626` (keystrokes)                                                                           | `isActionKeyEnabled`: projectOpening, `when`, predicate (no arg), `keyWhen`; once per candidate of the pressed stroke, on the key press only | The candidate is skipped and the next binding of the same stroke is tried (`:617-625`); with none left the key goes on (chord prefix check `:627`, else the target gets it). Candidate order is "`keyWhen` matches first, then the rest, each in reversed preset order" (`:605-616`), and the preference reads only `keyWhen`, never the predicate.                                                                                                                                                                                                                                                                                                                                                   |
| Popup and context menus                     | `src/ui/Menu.tsx:93`                                                                                                                                                                              | `isActionEnabled(a, undefined, item.arg)`: the row's arg is passed                                                                           | Greyed row (`styles.disabled`: `color: var(--fg-faint)`, `cursor: default`, `Menu.module.css:32-35`; `aria-disabled`, `Menu.tsx:374,381`), skipped by the keyboard (`selectable`, `:110`). No tooltip. Computed while the menu renders (Hypothesis: re-rendered only on the menu's own state changes, so effectively when it opens).                                                                                                                                                                                                                                                                                                                                                                  |
| Main menu bar                               | `src/features/app/menuBridge.ts:176-183` (`disabledMenuActions`), pushed at `:224-240`                                                                                                            | `isActionEnabled(action)`: **no arg**, also for dynamic items that carry one                                                                 | Native menu item disabled. Recomputed 120 ms after a context-key change (`:273`), after any registry change (`useActions.subscribe`, `:274`), on window focus and blur (`:250-258`); sent only when the disabled set changed (`:234-235`).                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Toolbar buttons                             | `src/ui/Button.tsx:54-103` (`IconButton` with `action`)                                                                                                                                           | Does **not** evaluate `when` or the predicate. Reads `useRunningActions` (`:78`)                                                             | While running: `aria-busy`, `aria-disabled` (not `disabled`, to keep focus, `:86-90`), a `Spinner` instead of the icon (`:100`), clicks ignored (`:93`). Disabled only when the caller passes `disabled`; callers that do: `src/features/debugger/ui/useActionEnabled.ts:5-9` (when + predicate, no arg, re-evaluated on every context-key change; used by `DebugToolWindow.tsx:60`, `MemoryView.tsx:127`, `InstancesPane.tsx:37`, `HeapCapture.tsx:65`). A button whose action is predicate-disabled and whose caller passes nothing looks enabled and its click does nothing (`executeAction` returns false). Plugin command buttons: `src/features/vcs/CommitView.tsx:1229` (commit-message slot). |
| Find Action / Search Everywhere Actions tab | `src/features/search/everywhere/deps.ts:28-34`, `model.ts:306-350`, `SearchEverywhere.tsx:809-840`                                                                                                | **`when` only**, against the context captured on open: no predicate, not even `projectOpening`                                               | Faint row (`SearchEverywhere.module.css:235-239`), score −150 (`model.ts:336`), hidden unless "Include disabled actions" (`SearchEverywhere.tsx:212,538`); a toggle's switch is disabled (`:831`). A row whose predicate is false is drawn enabled; choosing it runs `executeActionFrom("search")`, which then refuses silently.                                                                                                                                                                                                                                                                                                                                                                      |
| Settings › Keymap                           | `src/features/settings/keymap/model.ts:27-33`                                                                                                                                                     | Nothing about enablement (lists by `hidden` / `keymap` / bindings)                                                                           | Never shown disabled.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Others                                      | `src/features/services/selection.ts:56` (combined Services menu: `isActionEnabled(action)`, no arg), `src/editor/lsp/outsideProject.ts:88-91` (a notice's reload buttons, left out when disabled) |                                                                                                                                              |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

## (b) The native state

- `ActionMeta` (`app-gpui/src/platform/actions.rs:22-65`) has no `enabled`, `indicatesRunning` or `keymap` field; it deserializes from `assets/gen/actions.json` (`:131-144`).
- `ActionCall { ctx, arg: Option<&serde_json::Value> }` (`:172-176`); `RunFn = dyn Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()>` (`:178`), synchronous; `ToggledFn = dyn Fn(&Window, &App) -> bool` (`:179`).
- `Action { pub meta, run: Rc<RunFn>, toggled }` (`:182-186`); `run_handler` (`:210-212`).
- `is_action_in_window` (`:239-241`), `is_action_enabled(meta, ctx)` (`:244-252`: window, projectOpening, `when`), `is_action_key_enabled(meta, ctx)` (`:255-257`). Both take only the meta: no window, no arg.
- `ActionRegistry` (`:271-350`) is one `Global` per process: `register` (`:286-288`) just inserts into a `BTreeMap`, `handle`/`handle_toggle` look the meta up in the manifest and panic on an unknown id (`:294-314`), `unregister` (`:316-318`). Nothing observes registrations (no `observe_global::<ActionRegistry>`, measured by grep), so a batch costs nothing natively; the one batch-shaped caller is plugin commands (`app-gpui/src/plugins.rs:405-441`: unregister the previous ids, register the new ones, keep the list).
- `run` (`:357-385`) checks `is_action_enabled(&action.meta, &ctx)` without the arg (`:363`), calls listeners at nesting 0, runs the handler synchronously. `execute_action` / `execute_action_or_throw` / `execute_action_from` (`:416-450`).
- Tests at `:452-524`: `manifest_describes_the_webview_actions`, `enablement_follows_when_and_key_when`, `only_window_and_quit_actions_run_while_a_project_opens`, `localizes_titles_through_catalog_keys_then_source_text`.
- Generated vs hand-written: the manifest holds `id`, `title`, `category`, `description`, `icon`, `when`, `keyWhen`, `keywords`, `verbatim`, `hidden` plus `*Key`/`*Template`/`*Variants` and `toggle: true` when the source has a `toggled` function (`docs/gpui-port/gen-data.mjs:493-504,529-534`); 959 actions (measured: `toggle` 51, `keyWhen` 105, no `enabled`, `indicatesRunning` or `keymap` key). A non-static property such as `enabled: () => …` or `enabled: closable` is recorded on the generator's record as `$enabled` (`gen-data.mjs:304-317,356`) and then dropped because `ACTION_FIELDS` does not list it. Handlers, toggled functions and anything else that is code are hand-written natively.
- None of the 42 predicate ids is registered natively at HEAD (measured: no `git grep` hit for any of the ids, or for their id stems, under `app-gpui/src`). They arrive with the feature slices that depend on this one.
- Key dispatch: `DispatchEnv::key_enabled(&self, id)` (`app-gpui/src/platform/dispatcher.rs:115-119`), `first_enabled` takes the last binding whose action is key-enabled (`:198-201`), used by modifiers, chords, mouse and swipes (`:228-384`). `input::Env { keymap, registry, ctx }` (`app-gpui/src/platform/input.rs:224-238`) answers it with `is_action_key_enabled(&action.meta, ctx)`. `dispatch` (`:241-259`) builds the env from `cx.global`s with `window: &mut Window, cx: &mut App` in hand; keystrokes arrive through `cx.intercept_keystrokes` (`:139`), mouse presses from a plain `capture_any_mouse_down` closure of the workbench's root (`app-gpui/src/workbench/mod.rs:4409-4413`, it reads the workbench itself), and `with_dispatcher` (`input.rs:263-283`) from the editor's pointer handler (`app-gpui/src/editor/intelligence.rs:1522-1541`), which "runs while the editor's press handling" is under way.
- Popup menus: `popup_menu::resolve(items, window, cx)` (`app-gpui/src/ui/popup_menu.rs:80-136`) computes `disabled: item.disabled || !is_action_enabled(&action.meta, &context)` (`:110`) once when a menu or submenu opens (`:194,251`); disabled rows are greyed (`:396`) and skipped by the keyboard (`:72-74`). The workbench has `show_menu` (built inside its own update, `workbench/mod.rs:3509-3519`) and `show_menu_later` (deferred "because a row's check mark may be read through the workbench", `workbench/context_menus.rs:814-833`).
- Main menu bar: `state()` puts `disabled` = ids whose `is_action_enabled` fails (`app-gpui/src/workbench/menu.rs:487-493`, no arg); `schedule_sync` (`:528-547`) re-syncs 120 ms (`SYNC_DELAY`, `:47`) after `(window id, context_changes, MenuBar.changes)` changes; `sync` (`:504-525`) is not run inside an update of the window's workbench (`:502-503`).
- Find Action: `everywhere/view.rs:196-219` uses `is_action_enabled(meta, &context)` (window, projectOpening, `when`).
- The New popup: `Workbench::popup_actions` (`app-gpui/src/workbench/project_files.rs:1081-1094`) evaluates `is_action_enabled` for **every** registered action, from inside a workbench method.
- Toolbar: no shared native action button. Each view builds its own (`build_tools/view.rs:233-244`, `ToolbarButton { enabled: true, … }`). Plugin slot buttons: `context_menus::slot_buttons` (`workbench/context_menus.rs:1097-1131`) draw an icon per item whose slot `when` holds and run the action on click. No spinner or animation exists natively (measured: no `with_animation`, `Spinner` or spinner icon under `app-gpui/src` or in `assets/gen/icons.json`).
- Plugin command handler: `plugins.rs:434-438` registers `Action::new(meta, …)` whose `run_command` (`:496-554`) spawns the host call and `.detach()`es it, so the handler returns before the command settles.
- Keymap page: `KeymapAction` (`app-gpui/src/settings_model/keymap_model.rs:39-48`) and `is_keymap_action` (`:97-99`) have no `keymap` flag (Q-FU4's).
- Inventory rows this slice moves: `mod:src/platform/actions.ts` (`docs/gpui-port/INVENTORY.md:1987`: missing predicate, `indicatesRunning`, `keymap`), `unit:src/platform/actions.test.ts` (`:3944`: 1 of 5). The plan also lists `act:edit.selectAll` and `act:edit.undo` (`INVENTORY.md:401-402`); their missing pieces are the `keymap` listing (Q-FU4), the reading pane's Select All and ⌘Z in the tree, none of which is an enablement question.

## (c) The design

### Where the predicate lives

A predicate is code, so it is hand-written next to its handler in the feature module that registers the action, and the manifest only says that one exists. The generator gains two fields; the registry gains an optional predicate on `Action`, a tracked handler kind, and one full enablement check that takes the window, the app and the arg.

Why not publish every predicate as a context key (the merge editor's pattern, `mergeActions.ts:48-52`, native `vcs_more/merge_actions.rs`)? Half of the 42 depend on the invoker's arg (a row, a tab id, a path), which a context key cannot carry, and a key per action would have to be republished whenever any store it reads changes. The merge editor keeps its pattern; nothing here changes it.

Why `Fn(&ActionCall, &Window, &App) -> bool` and not `&mut`? It must run inside read-only places (a menu being built, the key interceptor's env that borrows globals) exactly like `ToggledFn` does today, and an immutable `&App` lets `Env` hold the registry, the keymap and the app at once.

### `assets/gen/actions.json` and `docs/gpui-port/gen-data.mjs`

In `actionManifest()` (`gen-data.mjs:529-534`), next to the `toggle` line:

```js
if ("$enabled" in record) entry.enabledFn = true;
```

and add `"indicatesRunning"` to `ACTION_FIELDS` (`:493-504`; the plugin site's `indicatesRunning: true` is a static literal, so it lands on the `dynamic` entry of `points.tsx`). Expected effect (Hypothesis until regenerated): 42 entries gain `"enabledFn": true`, the plugin `dynamic` entry gains `"indicatesRunning": true`, nothing else moves. Regenerate with `node docs/gpui-port/gen-data.mjs`, then `--check` must pass.

### `app-gpui/src/platform/actions.rs`

```rust
pub struct ActionMeta {
    // … existing fields …
    /// The webview's action has an `enabled(ctx, arg)` predicate: the native handler must give one.
    #[serde(default)]
    pub enabled_fn: bool,
    /// Toolbar buttons of this action show it is running and ignore clicks until its run settles.
    #[serde(default)]
    pub indicates_running: bool,
}

/// An action's own enablement beyond `when`: for a parameterised action or the current selection.
type EnabledFn = dyn Fn(&ActionCall, &Window, &App) -> bool;
/// A run that settles later: the task ends when the action is done (IDEA's "indicates running").
type TrackedRunFn = dyn Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<Task<anyhow::Result<()>>>;

enum Handler {
    Now(Rc<RunFn>),
    Tracked(Rc<TrackedRunFn>),
}

pub struct Action {
    pub meta: ActionMeta,
    handler: Handler,
    toggled: Option<Rc<ToggledFn>>,
    enabled: Option<Rc<EnabledFn>>,
}

impl Action {
    pub fn new(meta: ActionMeta, run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()> + 'static) -> Self;
    /// An action whose run settles when the returned task does; sets `meta.indicates_running`.
    pub fn tracked(
        meta: ActionMeta,
        run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<Task<anyhow::Result<()>>> + 'static,
    ) -> Self;
    /// Gives the action its own enablement (the webview's `enabled`); sets `meta.enabled_fn`.
    pub fn enabled(self, enabled: impl Fn(&ActionCall, &Window, &App) -> bool + 'static) -> Self;
    pub fn has_enabled(&self) -> bool;
    pub fn toggled(self, …) -> Self;            // unchanged
    pub fn is_toggled(&self, …) -> Option<bool>; // unchanged
    /// Unchanged contract; a tracked handler's task is detached and its failure logged.
    pub fn run_handler(&self, call: &ActionCall, window: &mut Window, cx: &mut App) -> anyhow::Result<()>;
}

/// Whether `action` may run now with `arg`, as the webview's `isActionEnabled(action, ctx, arg)`:
/// window, projectOpening, `when`, then the predicate (in that order, short-circuiting).
pub fn is_enabled(action: &Action, arg: Option<&Value>, ctx: &ContextKeys, window: &Window, cx: &App) -> bool;

/// Whether a shortcut may run `action`, as `isActionKeyEnabled`: [`is_enabled`] with no arg, then `keyWhen`.
pub fn is_key_enabled(action: &Action, ctx: &ContextKeys, window: &Window, cx: &App) -> bool;
```

`is_action_enabled(meta, ctx)` and `is_action_key_enabled(meta, ctx)` stay, documented as "the static part: window, projectOpening, `when` (`keyWhen`)". They remain right for Find Action (which in the webview reads `when` only) and for the 30-odd tests that call them; nothing that uses them today changes behaviour.

`ActionRegistry` additions:

```rust
pub struct ActionRegistry {
    // … existing …
    /// Runs in flight of tracked actions, per window (the webview's store is per webview).
    running: HashMap<(WindowId, String), u32>,
    /// Grows once per registration change (one per batch): what follows the registry compares it.
    generation: u64,
}

impl ActionRegistry {
    pub fn register(&mut self, action: Action);            // + generation += 1
    /// Like `handle`, with the action's own enablement.
    pub fn handle_enabled(
        &mut self,
        id: &str,
        enabled: impl Fn(&ActionCall, &Window, &App) -> bool + 'static,
        run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()> + 'static,
    );
    /// Like `handle`, for an action whose run settles later.
    pub fn handle_tracked(&mut self, id: &str, run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<Task<anyhow::Result<()>>> + 'static);
    /// Registers every action with one generation step; returns their ids, for `unregister_all`.
    pub fn register_all(&mut self, actions: impl IntoIterator<Item = Action>) -> Vec<String>;
    /// Removes every id with one generation step.
    pub fn unregister_all<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>);
    pub fn unregister(&mut self, id: &str);                // + generation += 1
    pub fn generation(&self) -> u64;
    /// How many runs of `id` are in flight in `window`.
    pub fn running(&self, window: WindowId, id: &str) -> u32;
}
```

`handle` keeps its signature and adds `debug_assert!(!meta.enabled_fn, "action {id} has an enabled predicate in the webview: register it with handle_enabled")` and the same for `indicates_running` → `handle_tracked`. A `debug_assert!` and not a `panic!`: a regenerated manifest must not crash a release build at start-up; every test build trips it. Safe at HEAD because no predicate id is registered natively (b).

`run()` changes:

1. `is_action_enabled(&action.meta, &ctx)` → `is_enabled(&action, arg, &ctx, window, cx)` (the arg now takes part, spec case 4).
2. `Handler::Now`: as today.
3. `Handler::Tracked`: `running[(window_id, id)] += 1`, `cx.refresh_windows()`; call the handler inside the nesting counter; on `Err` decrement at once (same as a rejected promise) and return `ActionError::Failed`; on `Ok(task)` spawn

```rust
cx.spawn(async move |cx: &mut AsyncApp| {
    let result = task.await;
    cx.update(|cx| {
        settle(window_id, &id, cx); // decrement, remove at 0, refresh windows
        if let Err(error) = result { tracing::error!("action {id} failed: {error:#}") }
    })
})
.detach();
```

and return `Ok(())`. So `execute_action_or_throw` reports only a start failure for a tracked action; the webview's `executeActionOrThrow` also awaited the async failure. Hypothesis: no host request targets a tracked action (only plugin commands are tracked, and host requests run IDE actions); the note in (h) says how to confirm.

Why a second handler kind and not a drop guard handed to every handler: a guard's `Drop` has no `&mut App` to refresh the windows or touch the registry, and changing `RunFn`'s return type would touch about 380 registrations (measured: 380 `registry.handle(` / `handle_toggle(` / `.register(Action::new` sites). Only one webview site is `indicatesRunning`, so the second kind costs one call site.

Why per window: the native registry serves every window (`actions.rs:270`) while the webview's `useRunningActions` lives in each webview. A plugin command running in one project window must not spin the same button in another.

### How the predicates are written (the convention the feature slices follow)

Each feature that registers predicate actions writes one `target` function per family and uses it in both closures, as the webview does (`resultTabActions.ts:6-14`, `runTabs.ts:33-38`, `tabActions.ts:13-18`):

```rust
/// The run tab an action is for: its argument, else the selected tab; `None` when there is none.
fn run_tab_target(call: &ActionCall, window: &Window, cx: &App) -> Option<RunTabId> { … }

registry.handle_enabled(
    "run.closeTab",
    |call, window, cx| run_tab_target(call, window, cx).is_some(),
    |call, window, cx| {
        let Some(id) = run_tab_target(call, window, cx) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, window, cx| workbench.close_run_tab(id, window, cx));
        Ok(())
    },
);
```

Rules for every predicate:

- R1. Pure and cheap: no `update`, no `notify`, no I/O, no allocation beyond what reading needs. It answers from state already in memory.
- R2. It reads entities with `read(cx)` and therefore must never be asked while one of those entities is being updated (GPUI panics on reading a leased entity; the codebase already works around it for toggles: `context_menus.rs:814-816`, `menu.rs:502-503`). Every surface below is placed so that holds; R2 is what the per-surface tests prove.
- R3. A predicate that only restates `when` is not written: use `when`. A predicate repeats inside `run` what it needs to stay safe (`database/index.ts:341-345` does), because a surface may have shown a stale answer.
- R4. With `arg == None` it answers for the focused thing (key dispatch and the main menu never pass an arg).

Where each family will live (Hypothesis: the owner files are where the slice that writes it will put it): `db.*` cell and table commands → `app-gpui/src/workbench/database.rs` / `database_extras.rs` (F-DB-GRID-A, F-DB-GRID-B); `db.*ResultTab*` → the same (F-DB-RESULT-TABS); `run.*Tab*`, `debug.*Tab*` → the Run window's module and `workbench/debug.rs` (F-RUN-TABS); `project.openInRightSplit` → the project view's actions (F-PROJECT-VIEW); `vcs.compareWithRevision`, `vcs.showCurrentRevision`, `vcs.showDiffForLines` → `vcs_ops/actions.rs` or `workbench/annotate.rs` (F-VCS-HISTORY); `diff.focusOppositePane`, `diff.showInExternalTool` → `workbench/diff_tab.rs` / `diff_tools.rs` (F-VCS-DIFF-TOOLBAR, F-VCS-DIFF-FIND).

### Each surface: what it asks, when, and what invalidates the answer

No predicate runs per frame for every action. The table is the whole list of places that call `is_enabled` / `is_key_enabled`.

| Surface                         | Native change                                                                                                                                                                                                                                                                                                                                            | Asked                                                                                                                   | Invalidated by                                                                                                                       |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Key dispatch                    | `input::Env` gains `window: &'a Window, cx: &'a App`; `key_enabled` → `registry.get(id).is_some_and(\|a\| is_key_enabled(&a, self.ctx, self.window, self.cx))`. `dispatch` and `with_dispatcher` build it with `window: &*window, cx: &*cx` before `cx.global_mut` is used again. `DispatchEnv` and `Dispatcher` do not change.                          | Once per candidate bound to the pressed stroke, at the press.                                                           | Nothing is cached.                                                                                                                   |
| `run()` itself                  | `is_enabled` with the arg.                                                                                                                                                                                                                                                                                                                               | Each run.                                                                                                               | —                                                                                                                                    |
| Popup and context menus         | `popup_menu::resolve` (`popup_menu.rs:110`): `!is_enabled(&action, item.arg.as_ref(), &context, window, cx)`.                                                                                                                                                                                                                                            | When a menu or submenu opens (`:194,251`), never while it is drawn.                                                     | Closing and reopening; a menu stays as it opened (as the webview's does, Hypothesis).                                                |
| Main menu bar                   | `menu.rs:489-493`: `!is_enabled(&action, None, &context, window, cx)` (no arg, as `menuBridge.ts:180`). `schedule_sync`'s key (`:531`) gains `registry.generation()` (the webview's `useActions.subscribe`, `menuBridge.ts:274`).                                                                                                                        | At most once per 120 ms, only after the window, its context keys, `MenuBar.changes` or the registry generation changed. | Those four; a predicate over other state (run tab count) refreshes with the next context change or window switch, as in the webview. |
| Toolbar: plugin slot buttons    | `context_menus::slot_buttons` (`:1101-1131`): reads `registry.running(window_id, &id)`; while `> 0` the button carries `debug_selector("slot:{slot}:{id}:running")`, its icon at `fg-faint`, no hover background, and its click returns without running (`Button.tsx:93`). It still does not consult `when` or the predicate (as `Button.tsx` does not). | When the slot is drawn (on notify / refresh, not in a paint loop).                                                      | `settle` and the start of a tracked run call `cx.refresh_windows()`.                                                                 |
| Other toolbars                  | None in this slice. A view whose button is bound to a predicate action computes `enabled` with `is_enabled` where it builds its toolbar model from the workbench (outside its own update), as `useActionEnabled.ts` does. The feature slices do this per view.                                                                                           | When the model is built.                                                                                                | Their own notify.                                                                                                                    |
| Find Action / Search Everywhere | **No change**: it keeps `is_action_enabled` (`everywhere/view.rs:206`), because the webview's Actions tab reads `when` only (`deps.ts:28-34`). A predicate-disabled action is listed as enabled and choosing it does nothing, as in the webview.                                                                                                         | —                                                                                                                       | —                                                                                                                                    |
| Settings › Keymap               | No change (the webview shows no enablement).                                                                                                                                                                                                                                                                                                             | —                                                                                                                       | —                                                                                                                                    |
| New popup                       | `project_files.rs:1081-1094`: keep the static check for every action, and do not call predicates there: it runs inside a workbench method (R2) and no New-popup action has a predicate (measured: none of the 42 is a `file.new*`/template action). A comment there says why.                                                                            | —                                                                                                                       | —                                                                                                                                    |

### How `when` and `enabled` combine in key dispatch, exactly

For each candidate the dispatcher tries (today: reversed preset order; after Q-FU4: `keyWhen`-matching candidates first, then the rest, the preference computed from `keyWhen` alone): run it if `in_window && (!projectOpening || WHILE_OPENING) && when && enabled(ctx, None, window, cx) && keyWhen`, else try the next; when none passes, the dispatcher's existing chord-prefix / pass-through logic applies unchanged. This slice does not touch the candidate order (Q-FU4 does, and lands first per the plan).

## (d) Native files that change

- `docs/gpui-port/gen-data.mjs`: `ACTION_FIELDS` + `"indicatesRunning"`; `entry.enabledFn = true` when `"$enabled" in record`.
- `app-gpui/assets/gen/actions.json`: regenerated (42 `enabledFn`, one dynamic `indicatesRunning`).
- `app-gpui/src/platform/actions.rs`: the fields, `EnabledFn`, `TrackedRunFn`, `Handler`, `Action::{tracked, enabled, has_enabled}`, `is_enabled`, `is_key_enabled`, registry `running` / `generation` / `register_all` / `unregister_all` / `handle_enabled` / `handle_tracked` / debug asserts in `handle`, `run()` (arg check, tracked runs, settle). Its unit tests.
- `app-gpui/src/platform/input.rs`: `Env` with window and app; `key_enabled`; `dispatch` and `with_dispatcher` build it.
- `app-gpui/src/ui/popup_menu.rs`: `resolve` uses `is_enabled` with the row's arg; a test.
- `app-gpui/src/workbench/menu.rs`: `state()` uses `is_enabled(…, None, …)`; `schedule_sync` follows `generation()`; a test.
- `app-gpui/src/workbench/context_menus.rs`: `slot_buttons` running state; a test.
- `app-gpui/src/plugins.rs`: `run_command` returns the `Task<()>` it spawned instead of detaching (`:535-553`), mapped to `Task<anyhow::Result<()>>`; registration via `Action::tracked`; the re-registration at `:405-441` uses `unregister_all` + `register_all`.
- `app-gpui/src/workbench/project_files.rs`: a comment at `:1087` only (it keeps `is_action_enabled`).
- `app-gpui/src/workbench/tests/smoke_registered_actions.rs`: every registered action whose manifest entry has `enabledFn` has a predicate (`has_enabled()`), so a feature that registers through `register(Action::new(manifest meta, …))` and bypasses `handle` is caught too.
- `app-gpui/src/workbench/mod.rs`: `mod proofs_actions_enabled;` in the tests list (`:4534` neighbourhood).
- New: `app-gpui/src/workbench/tests/proofs_actions_enabled.rs` (the spec cases and the surface tests that need a workbench window).
- `docs/gpui-port/INVENTORY.md` rows `:1987` and `:3944`, `parity.json`: the integrator's, not the writer's (Hypothesis about the process).

Not changed: `dispatcher.rs` (the trait is enough), `everywhere/*`, `settings_model/keymap_model.rs` (Q-FU4 owns `keymap`).

## (e) Parts, in order

1. **Part 1: registry and manifest** (Hypothesis: about 330 lines of code, 300 of tests). `gen-data.mjs` and the regenerated `actions.json`; everything in `actions.rs`; `run()` checks the predicate with the arg; tracked runs and their per-window count; batch registration and the generation; the `handle` debug asserts; the smoke test's `enabledFn` check; `plugins.rs` moves to `Action::tracked` and `register_all`. Lands alone: no surface asks a predicate yet, no native action has one, plugin commands behave the same except that their count is kept. Spec cases 1, 2, 4, 5 (3 is already proved).
2. **Part 2: surfaces** (Hypothesis: about 120 lines of code, 450 of tests). `input.rs` Env, `popup_menu.rs`, `menu.rs`, `context_menus.rs` slot buttons, the `project_files.rs` comment, and one test per surface including R2 (a predicate that reads the workbench is asked by every surface without a panic). Lands alone after part 1.

Total Hypothesis: about 1,200 lines, two writers or one writer twice; each part well under 1,500.

## (f) Tests per part

Tests that need a window go in `workbench/tests/proofs_actions_enabled.rs` with `start(cx)` and `cx.add_window_view(|window, cx| Workbench::new("w1", window, cx))` as `proofs20_shell.rs:21-22` does; actions are registered with `ActionMeta::new("test.…", …)` (no manifest lookup). Each spec-derived test carries the `// src/platform/actions.test.ts › <case>` line `proofs20_shell.rs:18` uses.

Part 1:

- `counts_a_run_of_an_indicates_running_action_until_it_settles_on_success_and_on_failure` (spec case 1): `Action::tracked` whose task awaits a `futures::channel::oneshot` receiver kept in a `RefCell<Vec<Sender>>`; run twice → `running == 2`; send the first, `run_until_parked` → 1; the second → 0 and the key is gone (assert through a `running_ids()` test accessor or `running == 0` plus the map length). A second tracked action whose task returns `Err` → 0 after parking, and its error is logged not returned.
- `does_not_track_actions_that_did_not_ask_for_it` (spec case 2): a plain `Action::new` whose handler spawns and detaches a pending task: `running == 0` during and after.
- `checks_argument_specific_enablement_before_dispatch` (spec case 4): predicate `call.arg == Some(&json!("/tracked.txt"))`; `execute_action(…, Some(&json!("/untracked.txt")))` is false and the handler did not run, nor did an `on_action_executed` listener hear it; with `/tracked.txt` true, one run, the arg it saw is `/tracked.txt`.
- `registers_a_batch_with_one_generation_step_and_disposes_every_action` (spec case 5): `generation` grows by exactly 1 over `register_all` of two actions, both `get` with their titles; `unregister_all` grows it by 1 and both are gone; an id re-registered by someone else in between is still removed (native has no identity check; say so in the test's doc: the webview's `addSource` disposer skips a replaced action, the native one does not, Hypothesis that no caller depends on it: plugins re-register their own ids only).
- `actions::tests::is_enabled_runs_the_predicate_after_when_and_project_opening`: a predicate that counts its calls is not called when `when` fails or `projectOpening` holds; `is_key_enabled` calls it with `arg == None` and then applies `keyWhen`.
- `actions::tests::the_manifest_marks_the_webviews_predicates`: `manifest()["db.closeResultTab"].enabled_fn` and `["project.openInRightSplit"].enabled_fn` are true, `["navigate.searchEverywhere"].enabled_fn` is false, and the count of `enabled_fn` entries is 42.
- `a_tracked_run_counts_in_its_own_window_only`: two workbench windows; a run in the first counts 1 there and 0 in the second.
- `handle_refuses_an_id_whose_webview_action_has_a_predicate` (`#[should_panic]`, debug only) on `db.closeResultTab`.
- smoke: `every_action_registered_in_a_project_window_is_known_to_the_manifest` gains the `enabledFn ⇒ has_enabled()` assertion.

Part 2 (one per surface, each with a predicate that reads the workbench entity through `workbench::for_window(window, cx)?.read(cx)` so R2 is exercised):

- Key dispatch: `a_shortcut_skips_an_action_its_predicate_disables_and_runs_the_next_binding`: two test actions bound to the same stroke through keymap overrides, the later one with a predicate false: the earlier runs; flip the predicate's backing `Rc<Cell<bool>>`: the later runs. Plus the arg: the predicate saw `None`.
- Mouse: `a_mouse_shortcut_over_the_editor_asks_the_predicate_without_reading_a_busy_editor`: bind a predicate action that reads the active editor to a mouse stroke, press it over editor text (the `with_dispatcher` path, `intelligence.rs:1530`); passes if no panic and the predicate's answer decides. This is the test that settles risk 1 below.
- Popup menu: in `popup_menu.rs` tests (next to `:480-540`): two rows of one action with args `"/tracked.txt"` and `"/untracked.txt"`: the second row is `disabled`, keyboard selection skips it. And `show_menu_later` with a workbench-reading predicate opens without a panic.
- Main menu: `the_menu_bar_disables_an_item_its_predicate_refuses` through `menu::state(window, cx).disabled` (Hypothesis: `state` is reachable from tests as it is from `sync`; otherwise through `sync` and `MenuBar.shown`); and a registration change alone makes `schedule_sync` sync again.
- Toolbar: `a_plugin_slot_button_shows_a_running_command_and_ignores_clicks`: a tracked test action as a slot item (through the slot-item test support of `context_menus.rs:1136-1140`), click → the `…:running` selector is drawn; a second click runs nothing (run count stays 1); settle → the plain selector is back and a click runs again.
- Find Action: `find_action_lists_a_predicate_disabled_action_as_enabled_as_the_webview_does`: the predicate is never called while the Actions tab lists (count stays 0) and the row is not drawn faint; choosing it runs nothing.

## (g) What tests and the real window must never do

- Never run a real plugin command (it can call a model or a server) to exercise `indicatesRunning`: tests use a `test.*` tracked action over a oneshot channel; a real-window check uses the repo's sample plugins only if one of them has a command that does no network work (Hypothesis that one exists under `plugins/`; otherwise skip the real-window spinner check).
- Never wait on wall-clock time for a run to settle: settle through the channel and `run_until_parked`.
- Never assert animation frames of the running state.
- Never call a predicate from inside an update of an entity it reads, in tests or code (R2); a test that does it on purpose to show the panic is not wanted either.
- Never register a predicate test action under a manifest id (the debug assert would fire, and the smoke test would attribute it to a feature).
- Never leave a test action or a predicate counter registered in a non-test build.
- Never change the dispatcher's candidate order in this slice (Q-FU4 owns it).

## (h) Open questions only a compiler or the running app settles

1. Does `Env { window: &*window, cx: &*cx, registry: cx.global(), keymap: &cx.global::<Keymap>().0, ctx }` borrow-check inside `dispatch` and `with_dispatcher` (both later call `cx.global_mut`)? Hypothesis: yes, because the env's block ends before. Settle: `cargo check -p openidea-gpui`.
2. Is the editor entity leased while `intelligence.rs:1530` calls `with_dispatcher` (the press is "the editor's press handling")? If yes, a predicate that reads the active editor panics there. Settle: the mouse test in (f); if it panics, `with_dispatcher` asks only the static check for predicate actions and the editor's deferred `navigate_from_pointer` asks the full check before running.
3. Are any of the about 380 handlers calling `execute_action` (nested) from inside an `on_workbench`/entity update, where a nested predicate reading the workbench would panic? Hypothesis: the codebase defers instead (`run_later`, `defer_debug_action`, `run_action_later`). Settle: `git grep -n "execute_action" app-gpui/src` and read each call inside an `update(` closure, plus the part-2 tests.
4. Does gpui-kit 0.9.1 export an animation API (`with_animation`, `Transformation::rotate` on `svg()`) for a real spinner? Hypothesis: upstream GPUI has it; no native code uses it. Settle: look in the compiler session's cargo registry copy of `gpui-kit`, or a one-line `cargo check` use. Until then the running button is drawn dimmed, not spinning.
5. Does `node docs/gpui-port/gen-data.mjs --check` pass at HEAD before the change (so the regeneration diff is only the two new fields)? Settle: run it first; if it is already stale, regenerate in a separate commit.
6. Does any host request (`plugins.rs:725` `execute_action_or_throw`) target a tracked action and rely on its async failure? Settle: grep the plugin host's action requests; if one does, add `execute_tracked_or_throw` returning the task.
7. Does `menu::state` stay reachable from a test without a real macOS menu bar? Settle: compile the test; else assert through `MenuBar.shown`.
8. Real window, after part 2: with a sample plugin's command in the commit-message slot (if one exists), click it and screenshot the dimmed running button; check the main menu still greys View/Edit items as before (no predicate action is registered natively, so nothing else should change visibly).
