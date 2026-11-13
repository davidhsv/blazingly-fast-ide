I wrote 18 end-to-end tests for the merge editor's actions in six new files; nothing has been compiled or run. They would prove 24 of the 26 missing or partial `merge.*` rows (counted against `docs/gpui-port/parity.json`). `merge.change` needs fix 1 below first, and `merge.annotate` stays partial.

**Files** (all new, under `app-gpui/src/workbench/tests/`). `conflicts.rs` has no `mod tests`, and the helpers the tests need are private to `workbench::tests`, so these files are children of that module:

| File                       | Lines | Tests                           |
| -------------------------- | ----- | ------------------------------- |
| `proofs_merge_fixtures.rs` | 126   | shared repositories and helpers |
| `proofs_merge_apply.rs`    | 143   | 5                               |
| `proofs_merge_change.rs`   | 159   | 6                               |
| `proofs_merge_edit.rs`     | 63    | 2                               |
| `proofs_merge_compare.rs`  | 143   | 3                               |
| `proofs_merge_focus.rs`    | 101   | 2                               |

The `mod` lines go inside `mod tests` in `app-gpui/src/workbench/mod.rs`, after the line `    mod proofs_editor_search_safe_delete;`.

**What they cover:** every missing or partial id except annotate: apply left/right/all, resolve simple, revert, accept, ignore, resolve using yours/theirs, resolve automatically, apply, accept theirs, cut/copy/paste/select all, the six comparisons, settings, auto apply and reload. Each test runs on a real temporary repository stopped on a conflict, and checks the result text, the toolbar's status text, where the caret lands, the file on disk, `git ls-files -u` and `git status --porcelain`.

The manifest has no key for any `merge.*` action, so each "condition is false" check runs the action through the registry and asserts it does nothing. One test does this for all 30 ids, before the merge editor opens and again while the editor behind it has the keyboard. It reads `mergeEditorFocus` and `diffEditorFocus` both on and off.

**Behaviours that differ from the webview.** These come from reading the code; the test named for each will confirm or refute it:

1. **`merge.change` ignores its argument.** The webview applies `{indexes, action}` to the given changes; natively the argument is dropped and the command does nothing. Test: `resolve_merge_change_does_what_its_argument_says_to_the_change`. Fix 1 changes `conflicts.rs` and adds `MergeView::apply_change`.
2. **`merge.annotate` is never enabled.** The webview enables it in a side pane and shows git blame there. Test: `annotate_with_git_blame_is_offered_in_the_side_panes_only`. The real fix is drawing blame in the side panes, which is not a small change, so the row stays partial.
3. **Diff Settings and Jump to Source do nothing in the merge editor.** In the webview they open the merge settings menu and the file; natively they only act on the diff window. Test: `diff_settings_and_jump_to_source_act_on_the_merge_editor_that_has_the_keyboard`. Fix 2 changes `diff_window.rs` and adds two methods to `conflicts.rs`.

**Decision for you:** in the webview the merge editor is a tab, so Jump to Source keeps the merge work behind it. Natively it is an overlay, so my hunk closes it and drops any undecided work. Asking first, the way Cancel does, is the alternative. The test passes either way.

**Most likely to fail first**, besides the three above:

- **Clicking a line in a side pane:** the click position is computed, not measured.
- **Reload:** it changes the index stage with `git update-index --index-info` and relies on the backend re-reading the index rather than a cached copy.
- **Focus test:** moving the keyboard to the editor behind might close the merge editor.

**Left out:** "next/previous conflict", "highlighting and whitespace policies" and "show/hide details" have no action in either UI. "Magic resolve" is covered by resolve simple and resolve automatically. Only one merge editor is shown at a time natively, so the spec case "routes only to the focused editor" can't run through the app. Spec cases covered: `mergeActions.test.ts` 2/3, `mergeModel.test.ts` 12/32; the rest of `mergeModel.test.ts` is model internals, already covered by its unit tests.

The notes file has the exact hunks, the parity key per test and the full failure risks: /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs_merge.md
