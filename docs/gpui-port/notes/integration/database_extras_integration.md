# Integrating the rest of the Database tool window

New files, not compiled yet:

- `app-gpui/src/workbench/database_extras.rs` (workbench child module: `ExtrasSlot`, `register_actions`, handlers, end-to-end tests)
- `app-gpui/src/database/dialog_parts.rs` (shared dialog parts: frame, buttons, check boxes, SQL preview)
- `app-gpui/src/database/history_view.rs` (`HistoryDialog`, Query History)
- `app-gpui/src/database/plan_view.rs` (`PlanView`, the Query Plan tab)
- `app-gpui/src/database/modify_table_dialog.rs` (`ModifyTableDialog`, Modify / Create Table, New › Column)
- `app-gpui/src/database/truncate_dialog.rs` (`TruncateDialog`)
- `app-gpui/src/database/import_dialog.rs` (`ImportDialog`, Import Data from File)

`ConsoleView` (`database/console_view.rs`, already compiled and tested) is mounted for the first time: it becomes the body of every SQL editor tab. Every anchor below is quoted from the file as it is now and occurs once (`grep -c -F` = 1 checked for each); no line numbers. Sections 1 to 5 are all required: `database_extras.rs` uses the `pub(super)` items of section 3, and its tests use the helpers of section 2e.

## 1. `app-gpui/src/database/mod.rs`

Add, in alphabetical order among the existing `pub mod` lines:

```rust
pub mod dialog_parts;
pub mod history_view;
pub mod import_dialog;
pub mod modify_table_dialog;
pub mod plan_view;
pub mod truncate_dialog;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

After `mod database;` add:

```rust
mod database_extras;
```

### 2b. The actions

In `pub fn register_actions(cx: &mut App)`, after `    database::register_actions(registry);` add:

```rust
    database_extras::register_actions(registry);
```

### 2c. The console around an SQL editor (in `fn pane_element`)

Replace

```rust
            (None, None, editor) => editor.map(IntoElement::into_any_element),
```

with

```rust
            // An SQL editor is drawn inside its query console, under the console's toolbar.
            (None, None, editor) => editor.map(|editor| match self.sql_console_of(&editor) {
                Some(console) => console.into_any_element(),
                None => editor.into_any_element(),
            }),
```

The right-click menu (`menu_editor`) stays on the wrapping `div`, so the editor's context menu still opens over the console.

### 2d. No `Workbench` field

`ExtrasSlot` lives in `DatabaseSlot` (section 3b). It is dropped with `self.database.take()` in `reset_database`, so `mod.rs` needs no field and no reset hunk.

### 2e. Test helpers visible to `database_extras::tests`

In `mod tests`, make these `pub(super)`. Each anchor is the full signature line.

- `    fn trusted_project<'a>(` → `    pub(super) fn trusted_project<'a>(`
- `    fn sqlite_source(root: &std::path::Path, cx: &mut VisualTestContext) {` → `    pub(super) fn sqlite_source(root: &std::path::Path, cx: &mut VisualTestContext) {`
- `    fn db_query(cx: &mut VisualTestContext, text: &str) -> Result<Vec<Vec<Option<String>>>, String> {` → `    pub(super) fn db_query(cx: &mut VisualTestContext, text: &str) -> Result<Vec<Vec<Option<String>>>, String> {` (121 columns: let rustfmt break it)
- `    fn db_tree(` → `    pub(super) fn db_tree(`
- `    fn db_row_key(` → `    pub(super) fn db_row_key(`
- `    fn db_expand(tree: &Entity<crate::database::tree_view::DatabaseTree>, label: &str, cx: &mut VisualTestContext) {` → same line with `pub(super) fn db_expand(`
- `    fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {` → `    pub(super) fn context_is_on(cx: &mut VisualTestContext, key: &str) -> bool {`

`start`, `wait_long`, `click_on`, `run_action` and `notification_titles` are already `pub(super)`.

## 3. `app-gpui/src/workbench/database.rs`

### 3a. Visibility for the sibling module

- `    fn db_step(` → `    pub(super) fn db_step(`
- `    fn carry_out_db(` → `    pub(super) fn carry_out_db(`
- `    fn db_call<T, F, W>(` → `    pub(super) fn db_call<T, F, W>(`
- `    fn run_tree_action(` → `    pub(super) fn run_tree_action(`
- In `pub(super) struct DatabaseSlot`:
  - `    store: DbStore,` → `    pub(super) store: DbStore,`
  - `    panel: Entity<DatabasePanel>,` → `    pub(super) panel: Entity<DatabasePanel>,`

### 3b. The extras' state in the slot

In `pub(super) struct DatabaseSlot`, after `    pending_run: Option<PendingRun>,` add:

```rust
    /// The consoles, the Query History, object and import dialogs, and the Query Plan tab.
    pub(super) extras: super::database_extras::ExtrasSlot,
```

In `ensure_database`, in `self.database = Some(DatabaseSlot { … })`, after `            pending_run: None,` add:

```rust
            extras: Default::default(),
```

### 3c. The Query Plan tab in the lower half

1. In `pub(super) enum Lower`, replace

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

2. In `pub(super) struct DatabasePanel`, after `    editors: Vec<Entity<TableEditorView>>,` add:

```rust
    /// The Query Plan tab, once a statement was explained.
    plan: Option<AnyView>,
```

3. Replace `        Self { tree, results, editors: Vec::new(), lower: Lower::Hidden }` with `        Self { tree, results, editors: Vec::new(), plan: None, lower: Lower::Hidden }`.

4. Replace `            Lower::Hidden | Lower::Results => None,` (in `active_editor`) with `            Lower::Hidden | Lower::Results | Lower::Plan => None,`.

5. In `impl Render for DatabasePanel`, after `            Lower::Results => Some(self.results.clone().into()),` add:

```rust
            Lower::Plan => self.plan.clone(),
```

6. In `fn strip`, replace

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

7. Before `    fn close_editor(&mut self, id: u64, cx: &mut Context<Self>) {` add:

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

1. Before `                Command::Results(update) => {` insert:

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

The `ResultsUpdate::Plan` arm of `apply_results_update` is then never reached. It still compiles; delete it only if a lint objects.

2. Replace these three lines, which are unique together. `panel.update(cx, |panel, cx| panel.select(Lower::Results, cx));` alone occurs twice, so anchor on the arm.

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

3. Replace the whole `Command::MarkOutcomes { path, marks, .. } => { … }` arm (three lines of body: `let marks = marks.len();` and the `tracing::debug!`) with:

```rust
                Command::MarkOutcomes { path, banner, .. } => self.mark_sql_console(&path, banner, cx),
```

4. Replace the whole `Command::ShowHistory => { … }` arm (its body is the `tracing::info!` line) with:

```rust
                Command::ShowHistory => self.show_query_history(window, cx),
```

5. Replace `                Command::SqlDone { job, ok } => tracing::debug!(job, ok, "Database: statements ran"),` with:

```rust
                Command::SqlDone { job, ok } => self.database_sql_done(job, ok, cx),
```

6. At the end of `carry_out_db`, replace

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

In `fn database_tree_run`, replace

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

### 3f. The table editor's Import and Browse…

In `fn table_editor_event`, replace

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

### 3g. The dialog layer draws this module's dialogs too

In `pub(super) fn database_dialog_layer`, replace

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

## 4. `app-gpui/src/workbench/tab_views.rs`: mounting the console

In `pub(super) fn wrap_markdown`, replace

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

`wrap_markdown` runs at both sites that install an editor in a tab: the load in `mod.rs` and `restore_unsaved`. `wrap_sql_console` returns at once for an editor that is not SQL, is read-only, or already has a console.

## 5. Menus

Nothing changes in `menu_spec.rs`. The tree's context menu rows (`TreeAction::…`) already exist in `tree_view.rs` and go through `TreeEvent::Run` → `database_tree_run` → section 3e. Query History is reached from the console toolbar and the action.

## 6. Actions registered → end-to-end test → IPC

| action id                           | end-to-end test (in `workbench::database_extras::tests`)                                                                                                                        | IPC (adapter → backend fn)                                                                                        | parity.json |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ----------- |
| `db.ddlToClipboard`                 | `generate_ddl_copies_a_tables_definition_and_appends_it_to_the_console` (+ ⌘⌥⇧G in tree / not in editor), `generate_ddl_in_restricted_mode_is_refused_with_the_backends_reason` | `db_ddl` → `ops::db::ddl`; `db_object_ddl` → `ops::db::object_ddl`; `db_schemas`, `db_tables` for a source/schema | done        |
| `db.ddlToConsole`                   | same first test                                                                                                                                                                 | as above + `db_console_path` → `ops::db::console_path`                                                            | done        |
| `db.goToReferencedTable`            | `the_trees_navigation_actions_reach_the_referenced_table_and_the_console`                                                                                                       | `db_schemas` / `db_tables` (reveal loads)                                                                         | done        |
| `db.jumpToConsole`                  | same                                                                                                                                                                            | `db_console_path`                                                                                                 | done        |
| `db.separateProceduresAndFunctions` | same                                                                                                                                                                            | none                                                                                                              | done        |
| `db.modifyTable`                    | `modify_table_new_column_and_new_table_change_the_file_and_the_tree` (+ ⌘F6 in tree / not in editor)                                                                            | `db_table_details` → `ops::db::table_details`; `db_execute` → `ops::db::execute`                                  | done        |
| `db.newColumn`                      | same                                                                                                                                                                            | same                                                                                                              | done        |
| `db.newTable`                       | same                                                                                                                                                                            | `db_execute`                                                                                                      | done        |
| `db.truncate`                       | `truncate_asks_and_deletes_every_row_of_the_table`                                                                                                                              | `db_execute`                                                                                                      | done        |
| `db.importData`                     | `import_data_inserts_the_rows_of_a_csv_file_into_the_table`                                                                                                                     | `db_table_details`; `db_apply_edits` → `ops::db::apply_edits`                                                     | done        |
| `db.runSqlScript`                   | `run_sql_script_runs_a_picked_file_and_a_template_opens_in_the_console`                                                                                                         | `db_execute` (statements split by `ide_db::split::statements_in`, as `db_statements` does)                        | done        |
| `db.queryHistory`                   | `query_history_lists_the_statements_run_and_pastes_one_into_the_console`                                                                                                        | `db_history` → `ops::db::history`; `db_remove_history` → `ops::db::remove_history`                                | done        |
| `db.goToColumn`                     | `go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard` (+ ⌘F12 in the table editor / not in the tree)                                                           | none                                                                                                              | done        |

Rows that existing tests or the new ones prove further:

- `action:db.execute`: becomes done. `the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_and_executes_on_it` proves the chooser for a plain `.sql` file with two sources.
- `action:db.explainPlan` and `ipc:db_explain`: become done. `the_console_banner_and_the_query_plan_tab_follow_the_runs` proves the Query Plan tab is drawn, Explain Again works, and Execute switches back.
- `action:db.openConsole`: becomes done. The console's own toolbar is shown and tested by the console and manual-commit tests.
- `ipc:db_set_manual_commit` and `ipc:db_finish_transaction`: `manual_commit_keeps_changes_pending_until_commit_or_roll_back`. The rows are not in parity.json today; add them as done.
- `ipc:db_history` and `ipc:db_remove_history`: not in parity.json today; add them as done.
- `ipc:db_completion`: read through `ensure_console`. No asserted state can only come from it, so leave it as it is.
- `action:db.editData`: stays partial. The table editor is not an editor tab (§8).
- `point.registerOverlay:database.dialogs`: stays partial. Schema-object, import, truncate and history dialogs are built now. Rename and Drop of schema objects, Quick Documentation and Data Source from URL are not.
- `point.registerToolWindow:database`: stays partial. SSH configurations is still missing, and results and the table editor are not in an editor tab or Services.
- `unit:src/features/database/**`: no spec case of these files covers what was built, so the `unit:` rows keep their counts. The views' own `#[gpui_kit::test]`s are not named after spec sentences.

Not registered: `db.new`, `db.quickDoc`, `db.sqlScript`, `db.pluginConnect` (§8).

## 7. Assumptions (check first)

1. **Pinned connection.** `ops::db::set_manual_commit` makes the backend run a source's statements on one pinned connection with an open transaction. A second data source on the same SQLite file opens another connection that does not see uncommitted rows. The manual-commit test relies on both.
2. **History on every execute.** `ops::db::execute` records every statement in the history of the source it ran on, the `sqlite_source` setup statements included. `ide-db/src/lib.rs` says the records are written "off the async threads", so the history test waits for its own statement before opening the dialog.
3. **`Editor::selection()`.** It returns the primary selection as UTF-16 `Range<u32>`, as `run_sql_editor` already uses it.
4. **Keymap.** The keymap binds ⌘⌥⇧G (`db.ddlToClipboard`), ⌘F6 (`db.modifyTable`) and ⌘F12 (`db.goToColumn`) from `assets/gen/keymaps.json` by id. The `keyWhen` set on registration (`databaseViewFocus`) and the manifest `when` (`dbTableEditorFocus`) gate them. In the IntelliJ macOS preset ⌘F12 is also File Structure in editors. The test only asserts the tree and the table editor.
5. **Path dialogs in tests.** `simulate_path_prompt_response` answers `cx.prompt_for_paths` in the test platform (`diff_tools.rs` tests do the same).
6. **SQLite EXPLAIN.** `EXPLAIN QUERY PLAN` of `select name from users where id = 1` gives at least one node.

## 8. What the webview does that is not built here

- **Console and table editor as editor tabs, and query sessions in Services (design only).** Results and table editors stay in the lower half of the Database tool window. Moving them breaks compiled tests in the contended `mod.rs`: `edit_data_…` asserts `Lower::Table(id)`, and `execute_…` asserts `Lower::Results` and `database_table_editor()`. The design:
  - Add a `TabKind { id: "dbTable", restorable: true, title: "<table> [<source>]", build: database_extras::build_table_tab }` to `TAB_KINDS`. The input data is `{ target, schema, table }`, as `tableInput.ts` has it.
  - `build_table_tab` creates the `TableEditorView`, subscribes `table_editor_event`, and registers it in a new `ExtrasSlot.tables` map instead of `DatabasePanel.editors`.
  - `database_table_editor()` looks at the active tab view first.
  - `open_table_editor` calls `open_view_tab(input)`.
  - For Services: a `dbResults` service view whose body is the existing `ResultsView` entity, mounted through `services_window.rs`'s view host. `Command::ShowResults` then shows the service instead of the tool window's lower half.
  - Tests to change: `edit_data_…` (assert the active tab view is the table editor) and `execute_…` (assert the Services view shows results). Tests to add: a session restore of a table tab, and closing the tab releasing the editor.
- **Gutter marks, timing and statement highlights in the console editor.** These need `editor/`, which another agent owns. Only the error banner is shown.
- **Data type suggestions.** The type field of Modify Table is free text, without the webview's suggestion list.
- **The Query Plan tab's close button.** The tab is replaced by the next plan and goes when the store trims old results.
- **`db.quickDoc`.** The popup anchored under the row is not built.
- **`db.new`.** It needs the toolbar button's position for the New menu. The toolbar's own button already opens it.
- **`db.sqlScript`.** The hidden action's argument is a webview node object. The SQL Scripts menu rows work through the tree command, as the template test proves.
- **`db.pluginConnect`.** Plugin data sources cannot be run in a SQLite test.
- **Rename and Drop of schema objects** (`rename_sql`, `drop_sql` exist). Out of this assignment's list; `database.rs` still logs them.
- **SSH Configurations** (`SourceDialogEvent::EditSshConfigurations`). That is the Settings window, in `settings_view/`, which another agent owns.
- **Index and key dialogs.** The webview has none: Modify Table edits columns only.
- **`withConnection` around Generate DDL, the SQL script template and Import.** These go through `db_call` and do not ask for a password and retry. A source that asks for its password on connect reports the backend's error instead.
- **The plan tab's source label.** It comes from the active editor's console binding, not from the result's own target: the store does not hand that out.

## 9. Compile risks

- **Match guards in `carry_out_db`.** `self.is_query_plan(id, cx)` takes `&App`, and `cx` is `&mut Context<Workbench>` captured by shared reference in the guard. If the borrow checker objects, compute `let plan_id = self.database_plan_id(cx);` before the `for` loop. Do not do that blindly: the plan can be created inside the same loop. Instead, move the three arms into one `Command::Results(update) if matches!(…)` arm that calls `self.extras_results_update(update, …)`.
- **`database_tree_run`.** `node` is moved into `run_extra_tree_action` in the `other` arm. Every other arm only borrows it.
- **`picked_path<E>`** takes `Result<anyhow::Result<Option<Vec<PathBuf>>>, E>` with `E = oneshot::Canceled`. If inference fails, write `picked.await.ok().and_then(|r| …)` at the two call sites.
- **`#[expect(clippy::too_many_arguments)]` on `apply_import`** (8 parameters with `self`). If clippy counts 7, the expectation is unfulfilled under `-D warnings`. Remove it then.
- **`ExtrasSlot` derives `Default`.** It holds `HashMap`, `Option`s and an `Option<Option<String>>`.
- **`line_through()`** on `Div` in `modify_table_dialog.rs` is a `Styled` method (gpui `styled.rs:619`). It has no prior use in the crate.
- **`ClickEvent::click_count()`** is used as in `project_tree.rs`.

## 10. Tests most likely to fail on the first run

1. `manual_commit_keeps_changes_pending_until_commit_or_roll_back`: assumption 1 of §7. If the backend's manual mode keeps no transaction across `execute` calls, the uncommitted row is visible or the rollback does nothing.
2. `the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_and_executes_on_it`: clicks on the deferred chooser list (`console-option-DataSource-0`) inside the workbench. The list draws at priority 1 among the window's other deferred layers.
3. `the_console_banner_and_the_query_plan_tab_follow_the_runs`: the order `Plan` → `Select(Some(plan))` → `ShowResults` must leave `Lower::Plan` (section 3d.2).
4. `go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard`: assumption 4 of §7. If ⌘F12 is claimed by another action, the key part fails while the action part passes.
5. `generate_ddl_copies_…`: `readable_ddl` lays SQLite DDL out with the formatter. The test matches the lower-cased `create table users` only.
6. **Any test that opens a `.sql` file of a project.** The Database tools are now built and the data sources read when such a file opens, as the webview's console bar does on mount. In a project that cannot read its sources (Restricted Mode, if `db_sources` is gated), the store notifies "Cannot read data sources" (key `db-sources`). A test that asserts the exact notification list after opening a `.sql` file would see it.
