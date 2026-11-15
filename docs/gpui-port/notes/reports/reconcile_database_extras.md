I wrote the reconciled notes to `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/database_extras_reconciled.md`. Nothing was compiled or run; the API checks come from reading the code.

**Hunks (37 sites in 4 files).** Every anchor counts exactly 1 at HEAD `a9da8e9f`, measured with `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dbx/anchors.py`. Another process was editing `workbench/mod.rs` and `tab_views.rs` during the session, so re-run that script just before applying.

- **Kept:** 34.
- **Re-anchored:** 1 — the console hunk in `pane_element` now sits on `(None, None, Some(editor)) => Some(editor.into_any_element()),`.
- **Rewritten:** 1 — the `db_expand` signature is wrapped, because the one-line form is 127 columns.
- **New:** 1 — `mod proofs6_database_console;` inside `mod tests`.
- **Dropped:** none. No newer code does what any hunk does.

**Draft edits** (copies in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dbx/edited/`, identical by `cmp`):

- **`database/dialog_parts.rs`:** now draws through the shared `app_views::{frame, title_row, button, checkbox, field_frame, tx}`, keeping its own public signatures.
- **Four dialog views:** their tooltips use the shared `.with_tooltip` with the English text. Before, the text was translated twice and the shared delay was skipped.
- **`history_view.rs`:** the hand-made search frame is now `field_frame`.
- **`database_extras.rs`:**
  - The import success notice gets the `tools` group, like its failure notice.
  - The history test waits for the first run to finish before the second Execute; Execute is disabled while a run is under way, so the old second click could do nothing.
  - The plan test waits for the Query Plan tab instead of asserting it at once.
  - New test `the_table_editors_import_and_export_browse_open_their_dialogs` covers the table editor's Import and Browse…, which nothing tested before.

**Checks.**

- No called API changed or disappeared.
- The labels, `when` conditions and shortcuts of all 13 action ids match the webview and `assets/gen/actions.json`.
- None of the 13 actions is in the application menu.
- No secret is drawn, logged or shown in a `Debug` output.
- Differences from the webview, listed in the notes: Browse Query History falls back to the tree's selection rather than the selected source, and Run SQL Script's file dialog has no `.sql` filter.

**Risks not settled by reading.**

- Hypothesis: the guards `if self.is_query_plan(id, cx)` in `carry_out_db` pass the borrow checker.
- Hypothesis: the error type of `picked_path` is inferred.
- Hypothesis: the borrows of separate fields in `sync_database_extras` are accepted.
- Hypothesis: SQLite's EXPLAIN gives at least two plan rows.
- Hypothesis: ⌘F12 in the tree does not reach Go to Column.
- Hypothesis: a successful `run_sql` refreshes the tree, so the new column and table show.
- Hypothesis: the history of a database node's target is the source's.
- Hypothesis: no backend error message contains a URL's username or password.
- Hypothesis: the chooser clicks in the two console tests hit the right deferred layer.

**Apply order.**

1. The six `pub mod` lines in `database/mod.rs` alone. Hypothesis: the six views compile and pass clippy on their own.
2. Everything else in one step, `database.rs` first. Any split leaves unresolved names or dead-code errors under `-D warnings`.
