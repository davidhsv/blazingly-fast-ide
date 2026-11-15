I've classified all 910 rows into `rows.json` and written `batches.md`, and the validation script passes with no errors. The work was read-only: I edited no repository files and ran no cargo, node or pnpm.

**Counts differ from your brief.** `parity.json` and `INVENTORY.md` were rewritten at 07:41, during the triage. That added three partial rows, now 482 instead of 479, and I classified those myself. Model-only is unchanged at 122, and there are 306 blank IPC rows.

**Something else is editing the worktree.** A `cargo clippy --all-targets` started at 08:01. Between 07:47 and 08:00 `lib.rs`, `workbench/mod.rs`, `workbench/build_tools.rs`, `agents_views.rs`, `new_task_view.rs`, `diff_tab.rs` and `vcs_diff/tab_view.rs` changed. Edits continued after my workers had finished, so another process made at least the later ones; the workers said they made no edits. Some file:line references in the output may already be off.

| Class               | Rows |
| ------------------- | ---- |
| small               | 279  |
| feature             | 238  |
| test-only           | 56   |
| blocked             | 16   |
| record-only         | 15   |
| IPC not-called      | 238  |
| IPC called-natively | 36   |
| IPC tauri-only      | 32   |

Seven not-called IPC rows already have a call drafted in a file no module declares, for example `refactor_actions.rs`, `vcs_text_actions.rs` and `languages_sdk.rs`. Those are close to small.

**Three densest test-only batches**, counting called-natively IPC rows since they only lack a test:

1. **App shell, 8 rows, high confidence:** `platform/actions.rs:358` (2), `popupOpen`, tool window store and layouts, `quickSwitch`, `tasks`, `TitleBar`. They are spread over 8 files.
2. **Plugin host functions, 7 `wit:` rows, low confidence:** the code is in `crates/ide-plugin/src/wit_host.rs`. Extend `proofs10_host_api`.
3. **GitHub, 7 rows:** 2 high (`github_part2.rs`) and 5 low `gh_pr_*` rows that need a fake `gh`.

The best fully high-confidence batches are debugger (6, `debugger/driver.rs`), database (6, `workbench/database.rs:173-192`) and run (6).

**Three densest small batches:**

1. **Editor paint, 14 rows:** wire unreached models into `editor/view.rs` (indent guides, selection marks, doc-comment tags, colour scheme, conflict markers, middle-button drag).
2. **Git log, 14 rows:** multi-selection, the collapsed graph, the filter-bar events the window ignores, and the `log_*` models.
3. **Terminal, 13 rows:** a tab strip and divider per split group, and wiring the terminal models into the view.

**Features that unlock the most rows:**

| Feature                            | Rows |
| ---------------------------------- | ---- |
| Deployment tool window             | 22   |
| Remote development                 | 18   |
| Document syntax tree in the editor | 17   |
| Docker services                    | 17   |
| Notebook editor                    | 14   |

**Proven but not recorded**, with the test bodies read:

- `action:run.run`, by `run_with_current_file_selected_runs_the_file_and_keeps_current_file_selected`.
- `results-view.test.tsx`, by `database::store::tests::records_the_selected_schema_or_connection_default`.
- `rainbowBrackets.test.ts`, by `editor::extras::tests::strings_and_comments_are_skipped_lexically`. This one is weaker: that test is already cited in the row's proof, yet the proof still calls the case missing.

Validation output:

```
rows: 910 by status: {'partial': 482, 'model': 122, 'blank-ipc': 306} expected from INVENTORY.md: {'partial': 482, 'model': 122, 'blank-ipc': 306}
partial in parity.json: 482
batches.md words: 1468
class counts: {'feature': 238, 'small': 279, 'blocked': 16, 'test-only': 56, 'record-only': 15, 'not-called': 238, 'called-natively': 36, 'tauri-only': 32}
OK: no errors
```

Beyond what you asked, the script also checks that each key appears once, that each class is allowed for its row's status, that every file:line exists, and that every named test is in `rust_tests.txt`.

Files are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/triage/`:

- rows.json
- batches.md
- validate.py
