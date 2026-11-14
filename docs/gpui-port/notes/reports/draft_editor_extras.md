I drafted three new source files and the notes file. Nothing was compiled or run, so every behaviour and test result below comes from reading the source.

**Files written**

- `app-gpui/src/workbench/editor_extras.rs` (1190 lines): registration, handlers and 15 end-to-end tests.
- `app-gpui/src/editor/extras.rs` (113 lines): new editor methods for expand-to-level, the gutter fold chevrons, Context Info's header lines and custom regions.
- `app-gpui/src/editor_lsp/reformat_dialog.rs` (343 lines): the Reformat File… dialog (Whole file / Selected text, Optimize imports), with 2 view tests and 1 model test. As in the webview, the options are remembered for the rest of the run, not stored on disk.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/editor_extras_integration.md` (428 lines): every hunk, assumptions, compile risks, existing tests affected, the designs, and the parity table.

**Actions registered → test → IPC (done unless marked)**

| Action                                                                                                       | Test                                | IPC                                                      |
| ------------------------------------------------------------------------------------------------------------ | ----------------------------------- | -------------------------------------------------------- |
| `code.expandToLevel1..5`, `code.expandAllToLevel1..5`                                                        | `expand_to_level_…`                 | `doc_folding`                                            |
| `editor.contextInfo` (partial: no syntax-tree fallback)                                                      | `context_info_…`                    | `doc_folding`                                            |
| `navigate.customRegion`                                                                                      | `custom_folding_…`                  | none                                                     |
| `code.unwrap`                                                                                                | `unwrap_replaces_…`                 | `syntax_support`, `syntax_node_at`                       |
| `code.expressionTypeInfo` (partial: shown as a hint, not the doc popup)                                      | `expression_type_info_…`            | `lsp_hover`                                              |
| `code.reformatDialog`                                                                                        | `reformat_file_…`                   | `lsp_format`; `lsp_code_actions` when optimizing imports |
| `find.rerun`                                                                                                 | `rerun_search_…`                    | `search_start`                                           |
| `navigate.next/prevOccurrence` (Usages, Problems, TODO; partial)                                             | `next_occurrence_…`                 | none                                                     |
| `editor.toggleSemanticHighlighting`, `editor.toggleRainbowBrackets` (partial: the paint ignores the setting) | `the_color_scheme_switches_…`       | `settings_set`                                           |
| `editor.addOrRemoveCaret` (empty handler, as in the webview)                                                 | `the_editor_actions_need_an_editor` | none                                                     |
| `project.refresh`                                                                                            | `reload_all_from_disk_…`            | `fs_read_dir`                                            |

The key tests press the shortcut in the editor and again with the Project view or Find bar focused. Two gaps: the Next/Previous Occurrence keys have no "does nothing here" test, and no test covers the "N of M" status message.

**Defect fixes (hunks in the notes)**

1. **Highlighted-usage fallback:** fixed by registering `navigate.nextOccurrence` / `prevOccurrence` (webview `src/workbench/toolwindows/actions.ts:37`, `:217`).
2. **Usages text not drawn:** `chrome.rs` now feeds the usages item and a new transient status message (8 s). Webview `src/editor/lsp/index.ts:86`, `contributions.ts:1558`.
3. **Reload All:** registering `project.refresh` (webview `project-tree/actions.ts:515`) via the unused `ProjectTree::reload_all` fixes it with no change to `editor_core.rs`.
4. **Create New Test…:** the row is hidden while `templates.newFromTemplate` is unregistered. In the webview it opens the New File dialog from the test template (`gotoTest.ts:117`, `navigation/index.ts:140`, `templates/index.ts:233`). An empty chooser shows "No tests found", a new string I made up.
5. **`pasteHistoryListFocus`:** a `mod.rs` hunk in `publish_context`. Webview `PasteHistoryDialog.tsx:181`.
6. **Folds:** three `view.rs` hunks add open and closed chevrons and a 10 px click column. The webview does not save folds (`session.ts` keeps only caret and scroll), so there is nothing to restore.

**Hunks needed in compiled files**

- `mod` lines in `editor/mod.rs`, `editor_lsp/mod.rs`, `workbench/mod.rs`, plus in `mod.rs` the field, its initial value and the `register_actions` call.
- `core_actions.rs`: `run_fold` split so the new code can call `apply_fold_edit`.
- `folds.rs`: a `backend()` accessor.
- `editor_language.rs`: visibility for `FormatRun` and its fields, `REFORMAT`, `This`, `format_document`, `apply_silently`, plus the Go to Test change.
- `find/view.rs`: `FindPanel::rerun`.
- `prompt_dialogs.rs`: `checkbox` and `button` become `pub(crate)`.

**Designed but not coded**

- **Soft wrap:** every line↔row conversion in `folds.rs` / `core_actions.rs` changes, about 12 call sites, so it is a design only.
- **Inline-completion ghost text:** three paint sites. The delays come from `inlineCompletion.ts`: 75 ms while typing, `pauseMs` when "only on pause" is on, 30 s back-off.
- **Rainbow brackets / semantic highlighting paint:** one hunk each.
- **Recent Locations:** needs a new popup view plus location recording.
- **Pasted imports:** the whole paste flow isn't driven by the workbench.
- Not drafted: the `refactor.*` dialogs and `navigate.navigationBar`.

**Integrator: check first**

1. Whether `cmd-shift-* 1` reaches the `Meta+Shift+* 1` chord.
2. Whether `cmd-r` chooses `find.rerun` over the other actions on that key.
3. Whether `project_view_actions.rs` (being edited by the other agent) now registers `project.refresh`. If so, drop mine.
4. Whether the paste-history filter counts as a text field for key dispatch. If not, Backspace with text in the filter would delete an entry.
5. The expected text of the Rust unwrap test.
