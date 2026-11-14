I've drafted the rest of the Database tool window: 13 actions wired, the console toolbar mounted over every SQL editor, Query History, the Query Plan tab, and the table and import dialogs. None of it has been compiled or run, so every test is unproven. It compiles only after the hunks the notes list in `mod.rs`, `database.rs` and `tab_views.rs`. The final review call to the advisor timed out, so nobody else has checked the draft.

**Files written (line counts)**

- `app-gpui/src/workbench/database_extras.rs` (1596): wiring, handlers and 12 end-to-end tests.
- `app-gpui/src/database/`:
  - `dialog_parts.rs` (166): shared dialog pieces.
  - `history_view.rs` (404), `plan_view.rs` (477), `modify_table_dialog.rs` (710), `truncate_dialog.rs` (227), `import_dialog.rs` (385): one view each, each with its own GPUI tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/database_extras_integration.md`

**Actions registered → test → IPC**

| Action                                                                                                   | Test                                                                                    | IPC                                                                           |
| -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `db.ddlToClipboard`, `db.ddlToConsole` (⌘⌥⇧G tested in the tree, and in an editor where it does nothing) | `generate_ddl_copies_…`; Restricted Mode refusal in `generate_ddl_in_restricted_mode_…` | `db_ddl` / `db_object_ddl` / `db_schemas` / `db_tables` (+ `db_console_path`) |
| `db.goToReferencedTable`, `db.jumpToConsole`, `db.separateProceduresAndFunctions`                        | `the_trees_navigation_actions_…`                                                        | reveal loads, `db_console_path`                                               |
| `db.modifyTable` (⌘F6 tested in tree and in editor), `db.newColumn`, `db.newTable`                       | `modify_table_new_column_and_new_table_…`                                               | `db_table_details`, `db_execute`                                              |
| `db.truncate`                                                                                            | `truncate_asks_…`                                                                       | `db_execute`                                                                  |
| `db.importData`                                                                                          | `import_data_inserts_…`                                                                 | `db_table_details`, `db_apply_edits`                                          |
| `db.runSqlScript` (plus the SQL Scripts menu templates)                                                  | `run_sql_script_…`                                                                      | `db_execute`                                                                  |
| `db.queryHistory`                                                                                        | `query_history_lists_…`                                                                 | `db_history`, `db_remove_history`                                             |
| `db.goToColumn` (⌘F12 tested in the table editor and in the tree)                                        | `go_to_column_…`                                                                        | none                                                                          |

Two more tests cover the toolbar:

- `the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_…`: with two sources, choosing one binds the plain `.sql` file and Execute runs on it.
- `manual_commit_keeps_changes_pending_…`: Tx: Manual, Commit and Roll Back, checked through a second source on the same SQLite file.

The plan test covers the error banner, the Query Plan tab, Explain Again, and switching back to results. If they pass, `action:db.execute`, `db.explainPlan`, `db.openConsole` and `ipc:db_explain` go to done, and `ipc:db_set_manual_commit`, `ipc:db_finish_transaction`, `ipc:db_history` and `ipc:db_remove_history` are new rows to add as done. No spec cases of the webview's `src/features/database/**` test files cover the new behaviour, so the `unit:` counts don't change.

**Designed but not coded:** the table editor and console as a new editor tab kind (`dbTable`), and results in the Services view. Moving them would break compiled tests in the shared `mod.rs` that assert `Lower::Table`, `Lower::Results` and `database_table_editor()`. The notes give the design, the call sites and the tests to change.

**Left out**

- Not registered: `db.new` (needs the toolbar button's position), `db.quickDoc`, `db.sqlScript` (its argument is a webview node object; the menu rows work anyway), `db.pluginConnect` (can't be run on SQLite).
- SSH Configurations: it lives in the settings window another agent is editing.
- Gutter marks and timings in the console editor: they need the editor, which another agent owns. Only the error banner is shown.
- Rename and Drop of schema objects: not in my list.
- Index and key dialogs: the webview has none.
- Data-type suggestions in Modify Table, and a close button on the plan tab.
- Generate DDL, script templates and import don't ask for a password and retry; a source that asks for one reports the backend's error instead.

**Hunks in compiled files** (exact text in the notes, each anchor checked to occur once)

- `database/mod.rs`: six `pub mod` lines.
- `workbench/mod.rs`:
  - The `mod database_extras;` and `register_actions` lines.
  - The pane body so an SQL editor is drawn inside its console.
  - Seven test helpers made `pub(super)`.
- `workbench/database.rs`:
  - `pub(super)` on four functions and two `DatabaseSlot` fields, plus the new `extras` field.
  - The Query Plan tab in the lower half.
  - New `carry_out_db` arms (Plan, Running, Close, Select, ShowResults, MarkOutcomes, ShowHistory, SqlDone) and a sync call at its end.
  - The `other` arm of `database_tree_run`, the table editor's Import and Browse…, and the dialog layer.
- `workbench/tab_views.rs`: one line in `wrap_markdown` that mounts the console.

**Check these first**

1. Manual commit must keep one open transaction per source across executes, and a second connection to the same file must not see uncommitted rows.
2. History is recorded off-thread and includes the setup statements, so the test waits for its own statement.
3. The keymap must bind ⌘⌥⇧G, ⌘F6 and ⌘F12 by id within their focus conditions.
4. The Select / ShowResults ordering must leave the plan tab shown after Explain Plan.
5. Opening any project `.sql` file now builds the Database tools and reads the sources, as the webview's console does. In a project that can't read them, a "Cannot read data sources" notice appears.
