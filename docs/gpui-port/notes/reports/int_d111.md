# Report: D111 integration (Part A: tooltip, Settings labels, flaky quick-fix test; Part B: the database extras)

Every gate passed: two full suites, the 20× loops beside the second, `web_view`, clippy, and a build with 0 warnings. Two things to know first:

- **Quick-fix cause is a hypothesis.** The failure was reproduced (1 in 880 runs before the fix, 0 in 550 after). That truncation caused that one failure was not isolated.
- **The Query Plan's Params value is cut** at the docked Database window width (≈290 pt). It is the only cut text left in the shots.

## 1. Gates (the last code edit came before all of these)

| Gate                                        | Result                                                                                               |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cargo test --lib`, run 1                   | `test result: ok. 7297 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 163.27s` |
| `cargo test --lib`, run 2                   | `test result: ok. 7297 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 214.43s` |
| Changed tests, 20× by filter during run 2   | 20 of 20 iterations `ok. 82 passed; 0 failed`                                                        |
| Quick-fix test, 300× `--exact` during run 2 | `pass=300 fail=0`                                                                                    |
| `cargo test --test web_view`                | `ok. 1 passed; 0 failed`                                                                             |
| `cargo clippy --all-targets -- -D warnings` | Finished, no warning or error                                                                        |
| `cargo build`                               | Finished, 0 warnings                                                                                 |

- **Loop filter:** both tooltip window tests, `ui::tooltip::tests`, the Appearance test, `settings_view::controls::tests::a_`, `settings_view::pages::font_picker`, the six `database::{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}`, `workbench::database_extras::tests`, `workbench::tests::proofs6_database_console`, and `editor::intelligence::tests` (22 tests, all on the changed fake server).
- **How the runs were started:**
  - Suites, `web_view`, clippy and build were Bash tool commands, foreground or `run_in_background`. None was started with `&` in a script, so none ran with SIGINT ignored.
  - The loops are zsh scripts in `d111/` that run the test binary in sequence, with no `&`.
  - The app was always launched by my copy of `run.sh`, which starts the binary with `&` inside `sh`, so it ran with SIGINT ignored (the app installs its own handler). It ran with fresh temp `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR` and was stopped by PID. No `openidea-gpui` process is left.
- **Disclosures:**
  - An earlier full gate round (7297 passed twice, plus 250/250 quick-fix runs) was made obsolete by the later edits.
  - One launch (`t4`) had no script file. It idled until the 150 s limit and was killed by PID.
  - Load runs: one suite of the pre-fix copied test binary (`ok. 7257 passed`), run as load beside the pre-fix loops.
- **Failures seen during the work, all fixed:**
  - `database::modify_table_dialog::tests::new_column_adds_one_and_selects_it`: the draft expected `alter table users …` for Postgres with schema `main`. The model correctly writes `main.users`, as the webview's `qualifiedName` does (`src/features/database/objects.ts:39`). The test now uses `public` and expects `public.users`.
  - `workbench::tests::proofs6_database_console::a_sqlite_console_runs_in_the_default_schema_its_completion_model_names`: "the default schema did not happen … reached later: None". An app gap, fixed in Part B.
  - A compile error: `AppContext` was not imported in `database_extras.rs`.
  - `git diff` also lists `app-gpui/src/build_tools/{execute,view}.rs` and `app-gpui/src/workbench/build_tools.rs`. Those are the reconciler's edits, not mine; I did not touch or format them.

## 2. Parts

### A.1 Tooltip over the popup its target opened

- **Reproduction:** the new test failed before the fix ("no tooltip of title:project covers its menu"). The flow is: pointer arrives, press at once, more than the delay passes on the test clock.
  - Cause found in the test: the press is exactly one delay old when the delay ends, and the check `now - press < TOOLTIP_DELAY` lets it through.
  - Hypothesis: in the real window, the timer fires at 600 ms plus jitter after the move and the press comes a few ms after it, so the same check passes. Confirm by logging `now - press` in `pressed_during_delay` while running `2026-11-15-b/j-menus.script` on the old code.
  - I did not reproduce it in a pre-fix window. The pre-fix evidence is the earlier shots `2026-11-15-b/03` and `05`.
- **Fix:** in `app-gpui/src/ui/tooltip.rs`, a press now holds tooltips while the pointer rests where the press was made, as well as during the delay.
  - A new `press_watcher()` is drawn by the window root. It records presses and moves through window-level listeners, so they are seen whatever is under the pointer.
  - It replaces the root's `note_press` call in `workbench/mod.rs`.
  - Webview: `src/ui/Tooltip.tsx:92` (pointerdown hides) and `:68` (a tooltip comes back only on a new pointerover).
- **Tests:** `workbench::tests::a_clicked_widgets_tooltip_stays_off_its_menu_while_the_pointer_rests_on_it` covers the project widget and the gear. It checks the rest, a 2 pt move on the target with the menu open, and that the tooltip shows again after the pointer leaves and comes back. The existing `ui::tooltip::tests` and `a_pressed_widgets_tooltip_stays_off_the_menu_it_opened` pass.
- **Real window (measured):** `t1-01`, `t1-02` (click, wait 1200) and `t7-01`, `t7-02` (hover, click, move 2 pt, wait 1200) all show the menu with no tooltip.

### A.2 Settings → Appearance labels

- **Fix:** `settings_view/controls.rs` `form_label` no longer adds a colon to a label that already ends in `:` or `：` (the catalog's "Size:", "Taille :", "大小："). This covers every page, including "Distraction free mode text width:" and the custom rows' "Show tabs in:" fallback.
- **Alignment:** `settings_view/pages/font_picker.rs` puts the label on the field's line, not centred on the field and its status line.
- **Webview:** `src/features/settings/pages/behavior.tsx:250` and `:263`; `Behavior.module.css:54`.
- **Tests:**
  - `settings_view::controls::tests::a_label_ending_in_a_colon_gets_no_second_one`
  - `settings_view::controls::tests::a_drawn_label_ends_in_one_colon`
  - `workbench::tests::the_appearance_pages_font_labels_end_in_one_colon_on_their_fields_line`
- **Shot `t1-03`:** "UI font:", "Fallback font:", "Size:", each on its field's line.
- **Not changed:** the webview draws no visible "UI font" label; the picker sits beside the check box. The native keeps a "UI font:" row.

### A.3 The quick-fix test whose problems never came

- **Measured before the fix:** 880 runs across four loops (200, 300, at least 80, 300), some beside a suite, gave 1 failure. It was run 80 of the third loop, with the same message: "the problems did not happen within thirty seconds … reached later: None".
- **Measured mechanism:** the fake server re-reads `answers.json` when it publishes a `didOpen`. `FakeServer::answer` rewrote the file with `std::fs::write`, which truncates and then writes. A Python reader racing that rewrite saw no `diagnostics` in 5173 of 24202 reads. With write-then-rename it saw 0 in 112182.
- **Hypothesis: this is the cause of the failure.** The test rewrites the answers right after `open_with` sees `didOpen` logged, while the server's publish may be reading them, and nothing publishes again. Confirm by running `$S/d111/old_lib_tests` (the pre-fix binary) against the current binary in several parallel loops of about 800 each, beside a suite.
- **Fix:** `editor/intelligence.rs` `FakeServer::answer` writes beside the file and renames over it. This covers every test on this fake server (22 tests).
- **After the fix:** 250/250 and 300/300 runs beside suites. At a 1-in-880 base rate this is not a strong discriminator.

### B. Database extras

- **Applied in the notes' order.**
  - Step 1: the six `pub mod` lines of `database/mod.rs`. It compiled with no warning; 313/314 database tests passed, the one failure being the wrong draft expectation above.
  - Step 2: all of §3 and §2, §4, then §2f, by `$S/d111/apply_b.py`. All 35 anchors counted 1.
  - Files: `workbench/{database.rs,mod.rs,tab_views.rs}`. Drafts handed to me and now declared: `workbench/database_extras.rs`, `database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `workbench/tests/proofs6_database_console.rs`.
- **Hypotheses from the notes, now settled by measurement:**
  - Borrow checks and the match guards compile.
  - SQLite EXPLAIN gives at least 2 rows (`the_console_banner_and_the_query_plan_tab_follow_the_runs` passes).
  - The tree shows a new column after a successful `run_sql` (`modify_table_new_column_and_new_table_change_the_file_and_the_tree`).
  - The chooser clicks work (`the_console_toolbar_chooses…`, `manual_commit…`). All pass 20× beside a suite.
- **App fixes I made:**
  1. **A console bound by the sources' arrival never loaded its completion model or default schema.** Now, when a console becomes bound, `ensure_console` runs (`sync_database_extras`). Webview: `src/features/database/console.tsx:291` (`useEffect(() => { if (bound) loadCompletion(bound) }, [bound])`). Test: `workbench::tests::proofs6_database_console::a_sqlite_console_runs_in_the_default_schema_its_completion_model_names`.
  2. **Import Data had no keys.** Escape now cancels and Enter imports, through a key claim as `truncate_dialog.rs` uses. Webview: `src/ui/Dialog.tsx:288` and `:294`, used by `DataDialogs.tsx:195`. Tests: `database::import_dialog::tests::enter_imports_and_escape_cancels`, plus `workbench::database_extras::tests::the_table_editors_import_and_export_browse_open_their_dialogs` (Escape closes it).
  3. **The Query Plan tab drew its columns over each other at the docked width** (shot `t3-05`). Now:
     - The operation column is at least 160 wide.
     - The properties go under the tree when the pane is too narrow and sit beside it otherwise (flex-wrap).
     - The property names are not cut.
     - Webview: `src/features/database/Database.module.css:1038` and `:1048`.
     - Test: `database::plan_view::tests::a_narrow_plan_puts_the_properties_under_the_tree_and_a_wide_one_beside_it` (300 pt and 900 pt).
- **The notes' two differences:**
  - **Browse Query History's fallback:** not a difference. The webview's `selectedSource` is `nodeTarget(lead)` (`src/features/database/DatabaseView.tsx:515`, `nodes.ts:57`, `index.ts:252`), which is `selected_target()`. History is keyed by `split_target(id).0` (`crates/ide-db/src/lib.rs:1091`, `:1096`). Read, not measured.
  - **Run SQL Script's file filter:** left as is. The webview filters `sql`/`ddl` (`src/features/database/treeActions.ts:156`). GPUI's `PathPromptOptions` has no filter field (read).
- **Secrets:** grep of the eight files finds `Debug` only on events and requests carrying SQL and table names, and `tracing` only for file-dialog errors and an action's title. My additions log nothing.
- **Not built** (as in notes §8): the console's gutter marks, data-type suggestions, the plan tab's close button, `withConnection` around DDL, templates and Import, and editor-tab consoles and tables.

## 3. Inventory rows (`parity.json` keys)

| Key                                        | State                                           | Proving tests (`workbench::database_extras::tests::` unless named)                                                                                                                            | Missing                                      |
| ------------------------------------------ | ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `action:db.ddlToClipboard`                 | partial                                         | `generate_ddl_copies_a_tables_definition_and_appends_it_to_the_console`, `generate_ddl_in_restricted_mode_is_refused_with_the_backends_reason`                                                | no password ask and retry (`withConnection`) |
| `action:db.ddlToConsole`                   | partial                                         | `generate_ddl_copies_…`                                                                                                                                                                       | same                                         |
| `action:db.goToReferencedTable`            | done                                            | `the_trees_navigation_actions_reach_the_referenced_table_and_the_console`                                                                                                                     |                                              |
| `action:db.jumpToConsole`                  | done                                            | same                                                                                                                                                                                          |                                              |
| `action:db.separateProceduresAndFunctions` | done                                            | same                                                                                                                                                                                          |                                              |
| `action:db.modifyTable`                    | partial                                         | `modify_table_new_column_and_new_table_change_the_file_and_the_tree`                                                                                                                          | data-type suggestions                        |
| `action:db.newColumn`                      | partial                                         | same                                                                                                                                                                                          | same                                         |
| `action:db.newTable`                       | partial                                         | same                                                                                                                                                                                          | same                                         |
| `action:db.truncate`                       | done                                            | `truncate_asks_and_deletes_every_row_of_the_table`                                                                                                                                            |                                              |
| `action:db.importData`                     | partial                                         | `import_data_inserts_the_rows_of_a_csv_file_into_the_table`, `the_table_editors_import_and_export_browse_open_their_dialogs`                                                                  | `withConnection`                             |
| `action:db.runSqlScript`                   | partial                                         | `run_sql_script_runs_a_picked_file_and_a_template_opens_in_the_console`                                                                                                                       | `.sql`/`.ddl` filter (GPUI has none)         |
| `action:db.queryHistory`                   | done                                            | `query_history_lists_the_statements_run_and_pastes_one_into_the_console`, `workbench::tests::proofs6_database_console::delete_in_query_history_removes_the_statement_from_the_stored_history` |                                              |
| `action:db.goToColumn`                     | done                                            | `go_to_column_lists_the_columns_of_the_table_editor_with_the_keyboard`                                                                                                                        |                                              |
| `action:db.explainPlan`                    | done (was partial: "Query Plan tab not built")  | `the_console_banner_and_the_query_plan_tab_follow_the_runs`, `database::plan_view::tests::*`                                                                                                  |                                              |
| `action:db.openConsole`                    | done (was partial: "console toolbar not shown") | `the_console_toolbar_chooses…`, proofs6 default-schema test                                                                                                                                   |                                              |
| `action:db.execute`                        | done (was partial: two sources not choosable)   | `the_console_toolbar_chooses_the_data_source_of_a_plain_sql_file_and_executes_on_it`                                                                                                          |                                              |
| `point.registerOverlay:database.dialogs`   | partial                                         | the dialog tests above                                                                                                                                                                        | index and key dialogs, SSH configurations    |
| `point.registerToolWindow:database`        | partial                                         | as above                                                                                                                                                                                      | SSH configurations                           |
| `point.registerServiceView:dbResults`      | unchanged                                       |                                                                                                                                                                                               | results and plan stay in the tool window     |
| `unit:` rows                               | none change                                     |                                                                                                                                                                                               | no webview spec covers these dialogs         |

Each IPC row's adapter is in `src-tauri/src/commands/db.rs`; each calls the same `ide_host::ops::db::` function with the same arguments as the native call (read).

| Key                         | Adapter → op                          | Native call                                                             | State and tests                                                       |
| --------------------------- | ------------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `ipc:db_ddl`                | `:142` → `ops::db::ddl`               | `database_extras.rs:195`, `:208`                                        | done (already) + DDL tests                                            |
| `ipc:db_object_ddl`         | `:165` → `ops::db::object_ddl`        | `database_extras.rs:193`                                                | unchanged; no routine DDL test                                        |
| `ipc:db_schemas`            | `:113` → `ops::db::schemas`           | `database_extras.rs:200`                                                | done (already)                                                        |
| `ipc:db_tables`             | `:119` → `ops::db::tables`            | `database_extras.rs:204`                                                | done (already)                                                        |
| `ipc:db_table_details`      | `:130` → `ops::db::table_details`     | `database_extras.rs:684`, `:770`, `:862`                                | modify, import, template tests                                        |
| `ipc:db_execute`            | `:249` → `ops::db::execute`           | `database.rs:210` via `run_sql`                                         | modify and truncate tests                                             |
| `ipc:db_apply_edits`        | `:295` → `ops::db::apply_edits`       | `database_extras.rs:933`                                                | import test                                                           |
| `ipc:db_console_path`       | `:308` → `ops::db::console_path`      | `database.rs:220`                                                       | done (already) + jump test                                            |
| `ipc:db_history`            | `:191` → `ops::db::history`           | `database.rs:216`                                                       | done: query-history test                                              |
| `ipc:db_remove_history`     | `:201` → `ops::db::remove_history`    | `database.rs:218`                                                       | done: proofs6 delete test, history test                               |
| `ipc:db_explain`            | `:178` → `ops::db::explain`           | `database.rs:214`                                                       | done: banner/plan test                                                |
| `ipc:db_set_manual_commit`  | `:79` → `ops::db::set_manual_commit`  | `database.rs:183`                                                       | done: `manual_commit_keeps_changes_pending_until_commit_or_roll_back` |
| `ipc:db_finish_transaction` | `:90` → `ops::db::finish_transaction` | `database.rs:186`                                                       | done: same                                                            |
| `ipc:db_completion`         | `:225` → `ops::db::completion`        | `database.rs:206`                                                       | done: proofs6 default-schema test                                     |
| `ipc:db_statements`         | `:235` → `ops::db::statements`        | the native splits with `ide_db::split::statements_in` (`store.rs:2502`) | partial: not the same op                                              |

## 4. Screenshots

All are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d111/insp/shots/`. The current binary's Database set is `t8-*`.

- **`t8-01`:** the tree app › main › tables (orders, users).
- **`t8-02`:** Modify Table users.
- **`t8-03`:** Import Data into main.users.
- **`t8-04`:** a result ("ann").
- **`t8-05`:** the Query Plan, with Select › Index Scan and the properties under the tree.
- **`t8-06`:** app Query History, with the statement listed and Paste enabled.
- **Earlier binary:** `t1-01`, `t1-02`, `t7-01`, `t7-02` (tooltips) and `t1-03` (Settings), as in Part A.
- The dialogs use the shared frame and sit inside the window, with no cut text.
- **Still cut (measured):**
  - The Query Plan's Params value ("table: users; USIN…") runs off the pane's edge. The webview ellipsizes it.
  - The Database tool window's own "Filter objects" field is clipped at the right edge. That predates this work.

## 5. False or stale in notes and briefs

- The brief's hypothesis for A.1 (the target counted as left and entered again) was not the cause the test reproduced; that was the press-at-arrival boundary.
- The reconciled notes name no `point.*` or `unit:` keys. The superseded notes name `point.registerOverlay:database.dialogs` and `point.registerToolWindow:database`.
- §6a's "Browse Query History differs" is not a difference (read, see Part B).
- §10's "the views compile on their own" holds, but one draft test expected the wrong SQL.
- The notes missed the `AppContext` import, the completion load on binding, the Import dialog's keys, and the plan pane's layout.
