# Database extras: reconciled integration notes

This file replaces `docs/gpui-port/notes/integration/database_extras_integration.md` for the integration. Paths are relative to `app-gpui/src/` unless they start with `app-gpui/`, `src/`, `src-tauri/`, `crates/` or `docs/`.

**What was measured and what was read.** Nothing was compiled or run. Measured (commands run in this session, output read): every anchor below was counted with `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dbx/anchors.py` (Python `str.count` on the exact text, multi-line anchors included) in the working tree at HEAD `a9da8e9f`. Every anchor counts 1. `workbench/mod.rs` and `workbench/tab_views.rs` were being edited by another process during this session: their line numbers moved between two counts (`database::register_actions` 589 → 601, the pane body 1437 → 1447, `wrap_markdown`'s anchor 740 → 753), the anchors did not. The `file:line` given below is the last count; re-run the script (`python3 -I <that path>` from `app-gpui/src`) before applying. Read, not measured: the API conformance of the eight draft files (every call checked against today's signature in `app-gpui/src`, `crates/ide-db`, `crates/ide-host/src/ops/db.rs` and `gpui-pre-0.3.12/src`).

**The drafted files.** As edited in this session, they are in the worktree and, byte-identical (`cmp`), in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dbx/edited/`. The committed versions (HEAD `a9da8e9f`) are in `.../scratchpad/dbx/orig/`. If `git status` shows a draft unmodified, copy it from `edited/` first.

| file                                          | lines | declared by |
| --------------------------------------------- | ----- | ----------- |
| `workbench/database_extras.rs`                | 1661  | §2a         |
| `database/dialog_parts.rs`                    | 92    | §1          |
| `database/history_view.rs`                    | 398   | §1          |
| `database/plan_view.rs`                       | 463   | §1          |
| `database/modify_table_dialog.rs`             | 707   | §1          |
| `database/truncate_dialog.rs`                 | 223   | §1          |
| `database/import_dialog.rs`                   | 384   | §1          |
| `workbench/tests/proofs6_database_console.rs` | 95    | §2f         |

**Count.** 37 edit sites in 4 files. 34 are kept as the old notes wrote them, anchors re-counted today (§4's text is unchanged; only its explanation is new: three call sites now). 1 is re-anchored (2c). 1 is rewritten (2e's `db_expand` line, wrapped to stay under 120 columns). 1 is new (2f, the `mod` line of the test draft). None is dropped: no newer code serves the purpose of any hunk (the arms the hunks replace still log "not available in this window").

Sections 1 to 4 are all required together (see §10 for the order): `database_extras.rs` uses the `pub(super)` items of §3a, its tests use the helpers of §2e, and each `pub(super)` method of `database_extras.rs` has its only caller in a hunk, so a partial apply fails `-D warnings` on dead code.

---

## 1. `database/mod.rs`

Add, in alphabetical order among the `pub mod` lines (today `database/mod.rs:6` `pub mod console_view;` to `:26` `pub mod vendors;`):

```rust
pub mod dialog_parts;
pub mod history_view;
pub mod import_dialog;
pub mod modify_table_dialog;
pub mod plan_view;
pub mod truncate_dialog;
```

## 2. `workbench/mod.rs`

### 2a. The module

After `mod database;` (`workbench/mod.rs:19`, 1 in the file) add:

```rust
mod database_extras;
```

### 2b. The actions

In `pub fn register_actions(cx: &mut App)` (`workbench/mod.rs:563`), after `    database::register_actions(registry);` (`workbench/mod.rs:601`, 1 in the file) add:

```rust
    database_extras::register_actions(registry);
```

### 2c. The console around an SQL editor (in `fn pane_element`) — RE-ANCHORED

The old anchor `(None, None, editor) => editor.map(IntoElement::into_any_element),` is gone: the body match now takes `Option` patterns and returns `Option<AnyElement>` (`let body: Option<gpui_kit::AnyElement> = match (view, host, editor) {`, `workbench/mod.rs:1444`). Replace (`workbench/mod.rs:1447`, 1 in the file)

```rust
            (None, None, Some(editor)) => Some(editor.into_any_element()),
```

with

```rust
            // An SQL editor is drawn inside its query console, under the console's toolbar.
            (None, None, Some(editor)) => Some(match self.sql_console_of(&editor) {
                Some(console) => console.into_any_element(),
                None => editor.into_any_element(),
            }),
```

The right-click menu is unaffected: `let menu_editor = editor.clone();` is taken before the match (read at `workbench/mod.rs:1443`) and the handler sits on the wrapping `div` (`.when_some(menu_editor, |el, editor| {`, `workbench/mod.rs:1559`), so the editor's context menu still opens over the console.

### 2d. No `Workbench` field

`ExtrasSlot` lives in `DatabaseSlot` (§3b). `reset_database` drops it with `self.database.take()` (read at `workbench/database.rs:836`), so `mod.rs` needs no field and no reset hunk. `reset_database` is called from `close_project` and `open_project` (read, `workbench/mod.rs:2975`, `:3145`); a trust change does not reset the Database tools (the `IdeEvent::ProjectTrustChanged` arm at `workbench/mod.rs:1941` only refreshes the agents and redraws).

### 2e. Test helpers visible to `database_extras::tests`

`database_extras::tests` imports them as `super::super::tests::{…}`, from a sibling of `workbench::tests`, so they must be `pub(super)`. Each anchor is the full signature line and counts 1 in `workbench/mod.rs`.

- `workbench/mod.rs:6408` `    fn trusted_project<'a>(` → `    pub(super) fn trusted_project<'a>(`
- `workbench/mod.rs:12181` `    fn sqlite_source(root: &std::path::Path, cx: &mut VisualTestContext) {` → `    pub(super) fn sqlite_source(root: &std::path::Path, cx: &mut VisualTestContext) {`
- `workbench/mod.rs:12206` `    fn db_query(cx: &mut VisualTestContext, text: &str) -> Result<Vec<Vec<Option<String>>>, String> {` → `    pub(super) fn db_query(cx: &mut VisualTestContext, text: &str) -> Result<Vec<Vec<Option<String>>>, String> {` (112 columns, measured)
- `workbench/mod.rs:12227` `    fn db_tree(` → `    pub(super) fn db_tree(`
- `workbench/mod.rs:12235` `    fn db_row_key(` → `    pub(super) fn db_row_key(`
- `workbench/mod.rs:11689` `    fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {` → `    pub(super) fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {`
- REWRITTEN: `workbench/mod.rs:12250`, replace the line

```rust
    fn db_expand(tree: &Entity<crate::database::tree_view::DatabaseTree>, label: &str, cx: &mut VisualTestContext) {
```

with (the one-line `pub(super)` form is 127 columns, measured)

```rust
    pub(super) fn db_expand(
        tree: &Entity<crate::database::tree_view::DatabaseTree>,
        label: &str,
        cx: &mut VisualTestContext,
    ) {
```

`start` (`:4278` area), `wait_long`, `click_on`, `run_action` and `notification_titles` are already `pub(super)` (read today). The Database window tests live in `workbench/mod.rs`'s `mod tests` and in `workbench/tests/proofs*_database*.rs`; `workbench/database.rs` has no `mod tests` (grep `mod tests` in it: no match).

### 2f. NEW: the test draft's `mod` line

In `mod tests`, after `    mod proofs6_database;` (`workbench/mod.rs:4287`, 1 in the file) add:

```rust
    mod proofs6_database_console;
```

`proofs6_database_console.rs` uses `super::*` (the tests' private helpers, as `proofs13_database.rs` does) and the `#[cfg(test)] pub(super)` accessors `database_console` and `database_history_dialog` of `database_extras.rs`; it compiles only once §2a is in.

## 3. `workbench/database.rs`

### 3a. Visibility for the sibling module

- `workbench/database.rs:873` `    fn db_step(` → `    pub(super) fn db_step(`
- `workbench/database.rs:956` `    fn carry_out_db(` → `    pub(super) fn carry_out_db(`
- `workbench/database.rs:1113` `    fn db_call<T, F, W>(` → `    pub(super) fn db_call<T, F, W>(`
- `workbench/database.rs:1517` `    fn run_tree_action(` → `    pub(super) fn run_tree_action(`
- In `pub(super) struct DatabaseSlot` (`workbench/database.rs:684`):
  - `workbench/database.rs:687` `    store: DbStore,` → `    pub(super) store: DbStore,`
  - `workbench/database.rs:690` `    panel: Entity<DatabasePanel>,` → `    pub(super) panel: Entity<DatabasePanel>,`

### 3b. The extras' state in the slot

In `pub(super) struct DatabaseSlot`, after `    pending_run: Option<PendingRun>,` (`workbench/database.rs:699`) add:

```rust
    /// The consoles, the Query History, object and import dialogs, and the Query Plan tab.
    pub(super) extras: super::database_extras::ExtrasSlot,
```

In `ensure_database`, in `self.database = Some(DatabaseSlot { … })` (the only constructor, `workbench/database.rs:812`), after `            pending_run: None,` (`workbench/database.rs:825`) add:

```rust
            extras: Default::default(),
```

### 3c. The Query Plan tab in the lower half

`Lower` is matched exhaustively twice, both in `workbench/database.rs` and both covered below (`active_editor`, `:292`–`:293`, and `Render`, `:392`–`:394`; measured with `grep -n "Lower::" workbench/database.rs`); `workbench/mod.rs` only asserts it in two tests (`:12430` `Lower::Table(id)`, `:12487` `Lower::Results`). `DatabasePanel` has one constructor (`DatabasePanel::new`).

1. In `pub(super) enum Lower` (`workbench/database.rs:251`), replace

```rust
    Results,
    Table(u64),
}
```

with

```rust
    Results,
    Table(u64),
    /// The Query Plan tab.
    Plan,
}
```

2. In `pub(super) struct DatabasePanel`, after `    editors: Vec<Entity<TableEditorView>>,` (`workbench/database.rs:268`) add:

```rust
    /// The Query Plan tab, once a statement was explained.
    plan: Option<AnyView>,
```

3. Replace `        Self { tree, results, editors: Vec::new(), lower: Lower::Hidden }` (`workbench/database.rs:276`) with `        Self { tree, results, editors: Vec::new(), plan: None, lower: Lower::Hidden }`.

4. Replace `            Lower::Hidden | Lower::Results => None,` (`workbench/database.rs:293`, in `active_editor`) with `            Lower::Hidden | Lower::Results | Lower::Plan => None,`.

5. In `impl Render for DatabasePanel`, after `            Lower::Results => Some(self.results.clone().into()),` (`workbench/database.rs:393`) add:

```rust
            Lower::Plan => self.plan.clone(),
```

6. In `fn strip`, replace (`workbench/database.rs:355`; `strip` is `let mut strip = div()`, read)

```rust
            .child(results);
```

with

```rust
            .child(results);
        if self.plan.is_some() {
            let label = tx(cx, "Query Plan");
            let plan = Self::strip_tab(label, self.lower == Lower::Plan, cx)
                .id("db-lower-plan")
                .debug_selector(|| "db-lower-plan".to_owned())
                .on_click(cx.listener(|panel: &mut Self, _: &ClickEvent, _, cx| panel.select(Lower::Plan, cx)));
            strip = strip.child(plan);
        }
```

7. Before `    fn close_editor(&mut self, id: u64, cx: &mut Context<Self>) {` (`workbench/database.rs:315`) add:

```rust
    /// The Query Plan tab's view; `None` removes the tab, and the result pane shows instead.
    pub(super) fn set_plan(&mut self, plan: Option<AnyView>, cx: &mut Context<Self>) {
        let gone = plan.is_none();
        self.plan = plan;
        if gone && self.lower == Lower::Plan {
            self.select(Lower::Results, cx);
        }
        cx.notify();
    }
```

### 3d. `carry_out_db`: the arms this module answers

1. Before `                Command::Results(update) => {` (`workbench/database.rs:984`) insert:

```rust
                Command::Results(ResultsUpdate::Plan { id, statement, plan, .. }) => {
                    self.show_query_plan(id, statement, plan, window, cx);
                }
                Command::Results(ResultsUpdate::Running { id, running }) if self.is_query_plan(id, cx) => {
                    self.query_plan_running(running, cx);
                }
                Command::Results(ResultsUpdate::Close(id)) if self.is_query_plan(id, cx) => {
                    self.close_query_plan(cx);
                }
                Command::Results(ResultsUpdate::Select(selected)) => {
                    if selected.is_some_and(|id| self.is_query_plan(id, cx)) {
                        panel.update(cx, |panel, cx| panel.select(Lower::Plan, cx));
                    } else {
                        if panel.read(cx).lower == Lower::Plan {
                            panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));
                        }
                        results.update(cx, |results, cx| results.select_tab(selected, cx));
                    }
                }
```

`ResultsUpdate::Plan` is still `Plan { id, title, statement, plan }` (`database/store.rs:412`) and `apply_results_update`'s `Plan` arm still only logs (`workbench/database.rs:1971`); it is then never reached and still compiles.

2. Replace these three lines (`workbench/database.rs:1035`, 1 in the file as a block; `panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));` alone occurs twice)

```rust
                Command::ShowResults => {
                    self.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, false));
                    panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));
```

with

```rust
                Command::ShowResults => {
                    self.with_layout(window, cx, |layout| layout.show(DATABASE_TOOL_WINDOW, false));
                    panel.update(cx, |panel, cx| {
                        // Explain Plan shows the Query Plan tab; anything else the result pane.
                        if panel.lower != Lower::Plan {
                            panel.select(Lower::Results, cx);
                        }
                    });
```

3. Replace the whole arm (`workbench/database.rs:1031`)

```rust
                Command::MarkOutcomes { path, marks, .. } => {
                    let marks = marks.len();
                    tracing::debug!(%path, marks, "Database: the console's statement marks are not drawn");
                }
```

with

```rust
                Command::MarkOutcomes { path, banner, .. } => self.mark_sql_console(&path, banner, cx),
```

(`MarkOutcomes { path, marks, banner }`, `database/store.rs:467`.)

4. Replace the whole arm (`workbench/database.rs:1042`)

```rust
                Command::ShowHistory => {
                    tracing::info!("Database: the Query History dialog is not available in this window");
                }
```

with

```rust
                Command::ShowHistory => self.show_query_history(window, cx),
```

5. Replace `                Command::SqlDone { job, ok } => tracing::debug!(job, ok, "Database: statements ran"),` (`workbench/database.rs:1059`) with:

```rust
                Command::SqlDone { job, ok } => self.database_sql_done(job, ok, cx),
```

6. At the end of `carry_out_db`, replace (`workbench/database.rs:1073`)

```rust
        cx.notify();
    }

    /// Runs a store request on the backend; its answer goes back to the store.
```

with

```rust
        self.sync_database_extras(window, cx);
        cx.notify();
    }

    /// Runs a store request on the backend; its answer goes back to the store.
```

### 3e. The tree's other actions

In `fn database_tree_run` (`workbench/database.rs:1589`), replace (`workbench/database.rs:1676`)

```rust
            other => {
                tracing::info!(action = other.title(), "Database: not available in this window");
                Vec::new()
            }
```

with

```rust
            other => self.run_extra_tree_action(other, node, window, cx),
```

`JumpToConsole` keeps its arm in `database_tree_run` (read: `TreeAction::OpenConsole | TreeAction::JumpToConsole`); only the registration of `db.jumpToConsole` is new.

### 3f. The table editor's Import and Browse…

In `fn table_editor_event` (`workbench/database.rs:1854`; `let id = editor.read(cx).id();` is its first line), replace (`workbench/database.rs:1898`)

```rust
            TableEditorEvent::Import(_) => {
                tracing::info!("Database: Import Data is not available in this window");
                Vec::new()
            }
            TableEditorEvent::BrowseExport { .. } => {
                tracing::info!("Database: the export's save dialog is not available in this window");
                Vec::new()
            }
```

with

```rust
            TableEditorEvent::Import(table) => {
                self.open_import_dialog(table.target, table.schema, table.table, window, cx);
                Vec::new()
            }
            TableEditorEvent::BrowseExport { path, extension } => {
                self.browse_table_export(id, path, extension, window, cx);
                Vec::new()
            }
```

The export form survives a `BrowseExport` (read: `table_editor_view.rs:786`, only `Export` and `CopyText` close it), so the chosen path can go back to it.

### 3g. The dialog layer draws this module's dialogs too

In `pub(super) fn database_dialog_layer(&self, cx: &App) -> Option<AnyElement>` (`workbench/database.rs:924`), replace (`:925`)

```rust
        let dialog = self.database.as_ref()?.source_dialog.as_ref()?.dialog.clone();
```

with

```rust
        let dialog: AnyView = match self.database.as_ref()?.source_dialog.as_ref() {
            Some(slot) => slot.dialog.clone().into(),
            None => self.database_extras_dialog()?,
        };
```

The layer then wraps it in the shared dialog frame, `.child(super::dialog_frame(dialog, cx))`, which takes `impl IntoElement` (read, `workbench/mod.rs:4193`: radius `radius-popup`, shadow `shadow-dialog`), so an `AnyView` needs no other change.

## 4. `workbench/tab_views.rs`: mounting the console

In `pub(super) fn wrap_markdown(` (`workbench/tab_views.rs:745`), replace (`workbench/tab_views.rs:753`)

```rust
        if !is_markdown_path(&path) {
            return;
        }
        let shared = markdown_shared(cx);
```

with

```rust
        self.wrap_sql_console(editor, window, cx);
        if !is_markdown_path(&path) {
            return;
        }
        let shared = markdown_shared(cx);
```

`wrap_markdown` now has three call sites: the load in `mod.rs` (`workbench.wrap_markdown(&tab_id, &editor, window, cx);`), `restore_unsaved` in `mod.rs`, and `light_edit_tabs.rs:143` (new since the draft). At both `mod.rs` sites the editor is inserted into `editors` before the call (read), so `sync_database_extras`, which keeps only the consoles of `self.editors()`, keeps the new console. In a single-file window `wrap_sql_console` does nothing: it calls `ensure_database`, which returns `false` when `project_root` is `None` (read: `project_root` is `Some` for `Content::Project` only, `workbench/mod.rs:1217`). It also returns at once for an editor that is not SQL, is read-only, or already has a console.

## 5. Menus

Nothing changes in `menu_spec.rs` or the context menus. The tree's context-menu rows for every registered tree action already exist in `database/tree_view.rs` (`menu_for`, read at `:1217`–`:1347`: SQL Scripts with DDL to Clipboard / Console and Run SQL Script, New › Table / Column, Modify Table…, Import/Export › Import Data from File…, Object Actions › Truncate…, Navigation › Go to Referenced Table, the Jump to Query Console row, the Separate Procedures and Functions toggle at `:1188`), mirroring `src/features/database/treeMenu.ts:79`–`:214` and `index.ts:46`. They reach `TreeEvent::Run` → `database_tree_run` → §3e. Query History is reached from the console toolbar (`console-history`) and the action.

## 6. Actions registered → end-to-end test → IPC

Label, `when`, `keyWhen` and icon are taken from `app-gpui/assets/gen/actions.json` (read today; the manifest carries no shortcut field). The tree actions are registered as `database.rs` registers its own (`manifest().get(id).clone().when("projectOpen").key_when("databaseViewFocus")`; both values equal the manifest's). Shortcuts come from `app-gpui/assets/gen/keymaps.json` by id. Native call sites are in `workbench/database_extras.rs` unless named; the store's requests run through `run_op` (`workbench/database.rs:158`). Adapters are in `src-tauri/src/commands/db.rs`.

| action id                           | label / when / keyWhen (manifest)                                   | IntelliJ macOS key | end-to-end test (`workbench::database_extras::tests` unless named)                                                                                                                            | IPC → adapter → native call                                                                                                                                                                                                                 |
| ----------------------------------- | ------------------------------------------------------------------- | ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `db.ddlToClipboard`                 | Generate DDL to Clipboard / projectOpen / databaseViewFocus         | ⌘⌥⇧G               | `generate_ddl_copies_a_tables_definition_and_appends_it_to_the_console` (action, ⌘⌥⇧G in the tree, not in an editor); `generate_ddl_in_restricted_mode_is_refused_with_the_backends_reason`   | `db_ddl` (`db.rs:142`) → `ops::db::ddl` at `generate_ddl` (`:183`); `db_object_ddl` (`:165`) → `ops::db::object_ddl` (same fn); `db_schemas` (`:113`), `db_tables` (`:119`) for a source, database or schema                                |
| `db.ddlToConsole`                   | Generate DDL to Query Console / projectOpen / databaseViewFocus     | ⌘⌥⇧B               | same first test                                                                                                                                                                               | as above, then `db_console_path` (`:308`) → `ops::db::console_path` (`run_op`, `DbOp::ConsolePath`, via `store.open_console_with`)                                                                                                          |
| `db.goToReferencedTable`            | Go to Referenced Table / projectOpen / databaseViewFocus            | none               | `the_trees_navigation_actions_reach_the_referenced_table_and_the_console`                                                                                                                     | the reveal's stages (`store.rs` `advance_reveal`: databases, schemas, tables, details, then `TreeUpdate::Reveal` and `ShowToolWindow`, read): `db_databases`, `db_schemas`, `db_tables`, `db_table_details` (`:130`), each through `run_op` |
| `db.jumpToConsole`                  | Jump to Query Console… / projectOpen / databaseViewFocus            | ⌘⇧F10              | same                                                                                                                                                                                          | `db_console_path` (`store.open_console`, `run_op`)                                                                                                                                                                                          |
| `db.separateProceduresAndFunctions` | Separate Procedures and Functions / projectOpen / databaseViewFocus | none               | same                                                                                                                                                                                          | none (`DatabaseTree::perform` toggles it, `tree_view.rs:1115`)                                                                                                                                                                              |
| `db.modifyTable`                    | Modify Table… / projectOpen / databaseViewFocus                     | ⌘F6                | `modify_table_new_column_and_new_table_change_the_file_and_the_tree` (+ ⌘F6 in the tree, not in an editor)                                                                                    | `db_table_details` → `ops::db::table_details` at `open_modify_table` (`:748`); `db_execute` (`:249`) → `ops::db::execute` (`store.run_sql`, `run_op` `DbOp::Execute`)                                                                       |
| `db.newColumn`                      | Column / projectOpen / databaseViewFocus                            | none               | same                                                                                                                                                                                          | same                                                                                                                                                                                                                                        |
| `db.newTable`                       | Table / projectOpen / databaseViewFocus                             | none               | same                                                                                                                                                                                          | `db_execute`                                                                                                                                                                                                                                |
| `db.truncate`                       | Truncate… / projectOpen / databaseViewFocus                         | none               | `truncate_asks_and_deletes_every_row_of_the_table`                                                                                                                                            | `db_execute`                                                                                                                                                                                                                                |
| `db.importData`                     | Import Data from File… / projectOpen / databaseViewFocus            | none               | `import_data_inserts_the_rows_of_a_csv_file_into_the_table`; from the table editor: `the_table_editors_import_and_export_browse_open_their_dialogs` (new)                                     | `db_table_details` at `open_import_dialog` (`:830`); `db_apply_edits` (`:295`) → `ops::db::apply_edits` at `apply_import` (`:907`)                                                                                                          |
| `db.runSqlScript`                   | Run SQL Script… / projectOpen / databaseViewFocus                   | none               | `run_sql_script_runs_a_picked_file_and_a_template_opens_in_the_console`                                                                                                                       | `db_statements` (`:235`) is replaced by `store.run_script`, which splits with `ide_db::split::statements_in` as `ops::db::statements` does; `db_execute` (`run_op`)                                                                         |
| `db.queryHistory`                   | Browse Query History / projectOpen / (none)                         | none               | `query_history_lists_the_statements_run_and_pastes_one_into_the_console`; `workbench::tests::proofs6_database_console::delete_in_query_history_removes_the_statement_from_the_stored_history` | `db_history` (`:191`) → `ops::db::history` (`run_op`, `DbOp::History`); `db_remove_history` (`:201`) → `ops::db::remove_history` (`DbOp::RemoveHistory`)                                                                                    |
| `db.goToColumn`                     | Go to Column / dbTableEditorFocus / (none)                          | ⌘F12               | `go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard` (+ ⌘F12 in the table editor, not in the tree)                                                                          | none                                                                                                                                                                                                                                        |

None of the 13 ids is registered anywhere else today (grep of each quoted id in `app-gpui/src`, outside `database_extras.rs`: only `platform/keymap/tests/proofs_presets.rs` names `db.modifyTable` and `db.goToColumn`, as keymap data, and `proofs6_database_console.rs` runs `db.queryHistory`). `smoke_registered_actions.rs` checks only that registered ids are in the manifest; no compiled test holds a literal list these ids would change (grep of the labels in `app-gpui/src`: only the drafts and the views that draw them).

Other rows the tests prove further (as the old notes said, unchanged; the two proofs6 rows were moved to `proofs6_database_console.rs` by `docs/gpui-port/notes/reports/integrate_plugins_per_window.md:84`): `action:db.execute` (the chooser for a plain `.sql` file with two sources), `action:db.explainPlan` and `ipc:db_explain` (`db_explain`, `db.rs:178` → `ops::db::explain`, `run_op` `DbOp::Explain`), `action:db.openConsole`, `ipc:db_set_manual_commit` (`db.rs:79`) and `ipc:db_finish_transaction` (`db.rs:90`) by `manual_commit_keeps_changes_pending_until_commit_or_roll_back`, `ipc:db_completion` (`db.rs:225`) by `proofs6_database_console::a_sqlite_console_runs_in_the_default_schema_its_completion_model_names`, `ipc:db_remove_history` by the proofs6 delete test.

Not registered: `db.new`, `db.quickDoc`, `db.sqlScript`, `db.pluginConnect` (§8).

### 6a. Webview differences found (step 4)

- **Labels, `when`, `keyWhen`, icons:** the 13 entries of `actions.json` match `src/features/database/treeActions.ts:240`–`:466` and `index.ts:201`, `:239` (read). Shortcuts in `src/platform/keymaps/intellij-macos.ts:360`–`:397` match `keymaps.json` (⌘⌥⇧G, ⌘⌥⇧B, ⌘⇧F10, ⌘F6, ⌘F12; the others none). Other presets bind `db.modifyTable` (Eclipse ⌥⌘C / ⇧⌥C) and `db.goToColumn` (Eclipse ⌘O, Ctrl+F3 / Ctrl+O; VS Code ⌘⇧O); they come from the same generated file, nothing to do.
- **Notification texts** (`Cannot generate the DDL`, `Cannot generate the SQL script`, `Cannot run the script`, `Script finished: N statement(s)`, `Cannot read the file`, `{table}: {n} row(s) imported`, `Nothing was imported`, key `db-import`) match `treeActions.ts:123`, `:143`, `:177`, `DataDialogs.tsx:167`, `:177`, `:182` (read). The draft now also puts the import's success notice in the `tools` group, as its failure notice and every store notice (`Command::Notify`) are.
- **Browse Query History's data source:** the webview takes the console's source, else `selectedSource`, else the only source (`index.ts:250`). The draft takes the console's source, else the tree's `selected_target()` (the selected node's connection target, which for a database node is `source/database`, else the only source). Hypothesis: the history of a database node's target is the source's; confirm by reading `crates/ide-db/src/history.rs` keys against `split_target`.
- **Run SQL Script's results:** the webview selects the Output tab and shows the results view; `store.run_script` does the same (`Results(Select(None))`, `ShowResults`, read at `database/store.rs:2501`–`:2515`).
- **Menu place:** none of the 13 ids is in the application menu (`grep -n "db\." src-tauri/src/menu.rs`: only `db.showToolWindow`, `db.newDataSource`, `db.openConsole` under Tools).
- **Run SQL Script's file dialog:** the webview filters to `sql`, `ddl` and titles it "Run SQL Script"; GPUI's `PathPromptOptions` has no filter or title (read), so any file can be picked.
- **Generate DDL, SQL script templates and Import** go through `db_call` and do not ask for a password and retry, where the webview wraps them in `withConnection` (`treeActions.ts:95`). Unchanged from the old notes.

## 7. Assumptions, settled where reading could settle them

1. **Pinned connection (settled by reading).** `DbManager::set_manual_commit` keeps a session per data source id (`crates/ide-db/src/lib.rs:1197`, `sessions.lock().entry(id)`), a statement of that id runs on the session when there is one (`:1157`), and `finish_transaction` commits or rolls back that session's connection (`:1213`). The test's second source `s2` has its own id, so its own connection.
2. **History on every execute (settled by reading).** A statement that ran (not cancelled, not refused) is sent to the recorder (`crates/ide-db/src/lib.rs:1076`–`:1082`, `remember_statement` at `:1087`), a thread fed through a channel (`:328`–`:341`, `openidea-db-history`), so a record lands after `execute` returns; both history tests wait for their statements with `wait_long`. Delete edits the dialog's entries in place after the backend answers (`store.rs:1551`–`:1559`), and the test now reads `rows().first()` so a brief change of the list cannot panic.
3. **`Editor::selection()`** returns `Range<u32>` (read, `editor/view.rs:475`).
4. **Keymap.** The keys are bound from `keymaps.json` (read, §6a). Hypothesis: ⌘F12 in an editor is File Structure and does not reach `db.goToColumn` while the tree has the keyboard; the test asserts only the tree and the table editor. Confirm with the test itself.
5. **Path dialogs in tests.** `simulate_path_prompt_response`, `did_prompt_for_paths` (`gpui-pre-0.3.12/src/app/test_context.rs:405`, `:414`) and `simulate_new_path_selection` (`:397`) exist and are used by `build_tools.rs:1538`, `diff_tools.rs:950` and `shelf.rs:2685` the same way.
6. **SQLite EXPLAIN.** Hypothesis: `EXPLAIN QUERY PLAN` of `select name from users where id = 1` gives at least two rows under the statement row (the test asserts `rows.len() >= 2`). Confirm with the test.
7. **Select-all in the dialogs' fields (settled by reading).** `TextField`'s key handler selects all on ⌘A (`ui/text_field.rs:290`), and `edit.selectAll` (`workbench/edit_actions.rs:28`) reaches a focused `TextField` through `impl EditTarget for TextField` (`:60`–`:75`, `EditCommand::SelectAll => self.select_all(cx)`), so `cmd-a` then `simulate_input` replaces the text in the view tests and in the workbench window.
8. **Restricted Mode test (settled by reading).** Revoking trust does not drop the Database tools (§2d), so the saved row key stays valid. `db_call` refuses through `host.authorize_command(label, "db_ddl")` → `project.require_trust()` with `"Project is in Restricted Mode. Trust the project to enable execution."` (`crates/ide-host/src/project.rs:796`–`:801`, read). `db_ddl` and `db_object_ddl` are not allowed in Restricted Mode (measured: `grep -n '"db_' crates/ide-host/src/trust.rs` finds only `db_console_path` and `db_export_file`, `:261`–`:262`, and no `db_` prefix is in the list at `:207`).

## 8. What is not built

- **Console and table editor as editor tabs, and query sessions in Services (design only).** Unchanged from the old notes §8: results and table editors stay in the lower half of the Database tool window, because `workbench/mod.rs` tests assert `Lower::Table(id)` (`:12430`) and `Lower::Results` (`:12487`). The design there (a `dbTable` `TabKind`, `ExtrasSlot.tables`, a `dbResults` service view) still applies. `ConsoleView::set_results` / `set_results_height` (`console_view.rs:163`, `:168`) are not used: the console shows no results of its own.
- **Gutter marks, timing and statement highlights in the console editor.** Only the error banner is shown.
- **Data type suggestions** in Modify Table (free text).
- **The Query Plan tab's close button.**
- **`db.quickDoc`, `db.new`, `db.sqlScript`, `db.pluginConnect`**, Rename and Drop of schema objects, SSH Configurations, index and key dialogs: as the old notes said.
- **`withConnection`** around Generate DDL, templates and Import (§6a).
- **The plan tab's source label** comes from the active editor's console binding.
- **Literal `px` lengths of the six draft views** stay `px(…)`: the compiled `database/` views use no `theme::zoomed` either (measured: 0 uses in `console_view.rs`, `tree_view.rs`, `source_dialog.rs`, `table_editor_view.rs`), and the backlog queues "zoom's literal px" as its own sweep. The shared parts the drafts now draw through (frame, title, buttons, check boxes, field frame, icons) do follow the zoom.

## 9. Edits made to the draft files in this session

`database/dialog_parts.rs` (162 → 92 lines): `dialog_frame` is now `app_views::frame(cx, width)` with `max_h(theme::zoomed(cx, 720.0))` and `app_views::title_row(cx, title).flex_none()`; `button` is `app_views::button`; `checkbox(cx, label, on, enabled)` is `app_views::checkbox(cx, label, on)` plus `whitespace_nowrap` and the 0.5 opacity when disabled; `field_frame(cx)` is `app_views::field_frame(cx, false)`; `tx` is `app_views::tx`. Kept (no shared equivalent, grep of `app_views/mod.rs`: `tx`, `t`, `frame`, `title_row`, `footer`, `button`, `checkbox`, `field_frame`, `path_line` only): `tx_template`, `code_font` (already `installed_fonts::mono_family`), `dialog_body`, `dialog_footer` (same metrics as `app_views::footer`, which takes a `Vec<Stateful<Div>>` the call sites do not build), `label`, `sql_preview`. The public signatures are unchanged, so no call site changes.

Tooltips: the compiled `console_view.rs:366` and `:537` still attach `.tooltip(tooltip(title))` with a title translated beforehand, so `database/` is now mixed; the drafts follow the shared `WithTooltip` (`ui/tooltip.rs:263`).

`database/history_view.rs`: the search field's hand-made frame is `field_frame(cx)`; `icon_button` takes `title: &'static str` and attaches `.with_tooltip(title)` (the shared tooltip translates and delays it) instead of `.tooltip(tooltip(tx(…)))`, which translated twice and skipped the shared delay; import `crate::ui::tooltip::WithTooltip`.

`database/plan_view.rs`: Explain Again's tooltip is `.with_tooltip("Explain Again")`; import `WithTooltip`.

`database/modify_table_dialog.rs`: `icon_button` takes `title: &'static str` and uses `.with_tooltip(title)`; import `WithTooltip`.

`database/import_dialog.rs`: Choose File…'s tooltip is `.with_tooltip("Choose File…")`; import `WithTooltip`.

`workbench/database_extras.rs`:

- `apply_import`: the success notice gets `group = "tools"` like the failure one (`let (mut request, ok) = …`).
- `query_history_lists_the_statements_run_and_pastes_one_into_the_console`: the second Execute waits for the first run to end (`!bar().running` and the first statement in the stored history). Execute is disabled while a run is under way (`console_view.rs:192`, and `press` returns for a disabled button), so the old second click could do nothing. After Delete it waits on `rows().first()` instead of indexing `rows()[0]`.
- `the_console_banner_and_the_query_plan_tab_follow_the_runs`: `Lower::Plan` and the cleared banner are waited for with `wait_long` instead of asserted at once after the plan appears (the plan, `Select` and `ShowResults` come in one batch whose last step decides the tab).
- NEW test `the_table_editors_import_and_export_browse_open_their_dialogs`: the table editor's `table-import` opens the import dialog on `main.users` and Cancel closes it; `table-export` › `export-browse` asks for a new path (`simulate_new_path_selection`) and the form's `target_path()` becomes it. It covers §3f, which no test reached before.

`database/truncate_dialog.rs`, `workbench/tests/proofs6_database_console.rs`: no change needed (read: every call matches today's API; the proofs6 delete test already waits for the first statement before the second Execute).

### 9a. API check of the draft files (step 2)

Every call was checked against today's code; none changed signature or disappeared. Read: `DbStore` (`console_bar`, `source_of`, `history`, `loaded`, `ensure_console`, `execute_console`, `explain_console`, `cancel_console`, `open_history`, `set_max_rows(u64)`, `set_manual_commit`, `finish_transaction`, `set_console_schema`, `bind_console`, `set_history_query`, `move_history`, `select_history`, `paste_history`, `remove_history`, `close_history`, `console_source`, `target_label`, `run_sql`, `reveal_in_tree(target, schema, table, kind, column)`, `open_console_with`, `run_script`, `rerun_result`, `sources`, `load_sources`; `readable_ddl`), `Command::{MarkOutcomes { path, marks, banner }, SqlDone, ShowHistory}`, `ResultsUpdate::{Plan, Running, Close, Select}`, `HistoryState` and `ConsoleBar` fields, `ConsoleEvent` (9 variants, all matched), `ConsoleView::{new(&str, AnyView, …), set_text_source, set_state, bar, set_banner, banner, toolbar}`, `DbNode`/`NodeKind`/`TableAt`/`InTable`/`SchemaObjectAt`/`Folder`/`table_folder`, `TreeAction` (`PartialEq`), `TreeCommand`, `DatabaseTree::{perform, selected, selected_target, select, set_expanded, separate, rows}`, `TableEditorView::{show_columns, columns_query, shown_columns, pick_column, revealed_column, export_form, state}`, `ExportForm::{file_picked, target_path}`, `TableRef`, `objects::{can_modify_table, script_sql, truncate_sql, TruncateOptions (Copy, Default), ScriptKind (Copy), ColumnDraft, draft_of, create_table_sql, alter_table_sql, can_alter_column}`, `model::{js_number_string(f64), guess_separator, parse_delimited, import_edits}`, `ide_db::{PlanNode, PlanProperty, HistoryEntry, CellValue, DbRowEdit, ColumnInfo, TableDetails, TableInfo, ForeignKey, DbDriver (6 variants, matched exhaustively in `first_type`), DbTableKind, DbObjectKind, CompletionModel, CompletionTable}`, `ide_host::ops::db::{ddl, object_ddl, schemas, tables, table_details, apply_edits -> f64, execute (8 args), history, save_sources}`, `NotificationRequest` fields, `on_workbench`, `ActionRegistry::handle`, `Action::new`, `ActionMeta::{when, key_when}`, `ActionCall.arg: Option<&Value>`, `TextField::{new, with_placeholder, set_text, text}`, `TextFieldEvent` (3 variants), `input::{claim_keys, release_keys, init}`, `Workbench::{editor, editors, project_root, open_project_file, notify, with_layout, is_tool_window_visible, notifications, project_restricted (app_actions.rs:312), ensure_database, database_tree, database_table_editor, database_panel, database_results}`, `App::{prompt_for_paths, prompt_for_new_path}`, `Styled::line_through` (`gpui-pre-0.3.12/src/styled.rs:619`), the icons used (`arrow-left`, `db-key`, `db-table`, `refresh`, `chevron-down`, `chevron-right`, `minus`, `trash`, `folder`, `check`, `plus`, `db-column`, `db-column-key`; all in `assets/gen/icons.json`).

### 9b. Secrets (step 5)

Measured by grep over the eight draft files: no `{:?}` and no `Debug` derive on anything holding a `DataSource`, a password or a URL; the `Debug` derives are on events and requests carrying SQL, table and column names and imported cell values. `tracing` lines (3) print a file-dialog error and a tree action's title. No `DataSource` field other than `id`, `name` and `driver` is read (`source_of(…).driver`, `sources()` ids, `target_label`, which uses the name). The clipboard gets DDL text only. Notification bodies carry `IdeError.message` from the backend, as `database.rs`'s own notices do. Hypothesis: no backend error of `db_ddl`, `db_object_ddl`, `db_schemas`, `db_tables`, `db_table_details` or `db_apply_edits` contains a URL's userinfo; `DataSource`'s `Debug` redacts the URL's password (`crates/ide-db/src/source.rs:387`) and ClickHouse errors redact it (`clickhouse.rs:297`). Confirm by grepping `crates/ide-db/src/{postgres,mysql,sqlserver,oracle}*.rs` for errors built from the connection string. Nothing to fix in the drafts.

## 10. Order to apply (each step compiles)

1. **§1 alone** (the six `pub mod` lines of `database/mod.rs`). Hypothesis (nothing was compiled): the six views compile on their own, since they depend only on `app_views`, `theme`, `ui`, `platform` and `database` models, all compiled today, and their `#[gpui_kit::test]`s run. Hypothesis: `cargo clippy -p openidea_gpui --all-targets -- -D warnings` is clean after this step, since the views are `pub` in a `pub mod` of the library (`lib.rs:9` declares `pub mod database`, read), so their unused `pub` accessors raise no dead-code warning.
2. **§2, §3 and §4 together, in one step.** `database_extras.rs` (§2a) needs §3a's `pub(super)` items and §3b's field; its `pub(super)` methods are called only from §2c, §3c–§3g and §4, and its tests need §2e. Inside the step, apply §3 first, then §2a–§2e, §4, and §2f last. Splitting this step leaves either unresolved names or dead-code errors under `-D warnings`.
3. Run `cargo nextest run -p openidea_gpui database` and the tests of §11.

## 11. Compile risks not settled by reading

- **Match guards in `carry_out_db` (§3d.1).** `self.is_query_plan(id, cx)` takes `&App` while `cx: &mut Context<Workbench>` and `self: &mut Self` are used only by shared reference in the guard. Hypothesis: this compiles (guards may read through `&mut` bindings). If not, do not hoist the plan id above the loop (the plan can be created inside the same loop): fold the three arms into one `Command::Results(update) if matches!(update, ResultsUpdate::Plan { .. } | …)` arm that calls a method of `database_extras.rs`.
- **`picked_path<E>`** takes `Result<anyhow::Result<Option<Vec<PathBuf>>>, E>`; awaiting `cx.prompt_for_paths` gives `Result<anyhow::Result<…>, oneshot::Canceled>` (read, `app.rs:2032`). Hypothesis: `E` is inferred; else write the `match` inline as `build_tools.rs:803` does.
- **`#[expect(clippy::too_many_arguments)]` on `apply_import`** (self + 7). Settled by reading: `store.rs:2568` (`run_finished`, self + 7) compiles with the same `expect`, so clippy counts `self` and the expectation is fulfilled.
- **`ExtrasSlot` derives `Default`**: `HashMap`, `Option`s and `Option<Option<String>>` only.
- **Borrows in `sync_database_extras`**: `&slot.store` is held while `slot.extras.history_wanted.take()` runs; these are disjoint fields of one `&mut DatabaseSlot`. Hypothesis: accepted by the borrow checker.
- **`with_tooltip(title)`** with `title: &'static str` converts through `From<&'static str> for SharedString`; the call sites pass literals.

## 12. Tests most likely to fail on the first run

1. `the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_and_executes_on_it` and `manual_commit_keeps_changes_pending_until_commit_or_roll_back`: they click the deferred chooser list (`console-option-DataSource-0`, `console-option-Transaction-1`, which is "Tx: Manual", read at `console_view.rs:262`) inside the workbench, among the window's other deferred layers.
2. `the_table_editors_import_and_export_browse_open_their_dialogs` (new): it depends on the maximized tool window showing the whole toolbar, as `proofs13_database.rs`'s export test does. `table-import` is enabled for `users` (read: `is_editable` needs a page and `TableEditorState::editable`, which needs a primary key, a writable source and not ClickHouse, `table_editor.rs:57`; `users` has `id integer primary key`), and `export-browse` has its `debug_selector` (`table_editor_view.rs:1392`–`:1393`).
3. `the_console_banner_and_the_query_plan_tab_follow_the_runs`: the order `Plan` → `Select(Some(plan))` → `ShowResults` must leave `Lower::Plan` (§3d.2); the test now waits for it. The banner's `[app]` prefix is the store's format (read, `database/store.rs:2721`: `format!("[{name}] {e}")`).
4. `go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard`: §7.4.
5. `modify_table_new_column_and_new_table_change_the_file_and_the_tree`: Hypothesis: a successful `run_sql` refreshes the table's details in the tree, so `email` and `books` show (`db_row_key` waits for them up to 30 s).
6. Any test that opens a `.sql` file of a project now builds the Database tools and reads the data sources, as the webview's console bar does; a test asserting the exact notification list after opening a `.sql` file in Restricted Mode would also see "Cannot read data sources" (key `db-sources`), if `db_sources` is gated.
