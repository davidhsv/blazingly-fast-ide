# Merge editor actions: draft tests

## Where the files go, and the `mod` lines

`app-gpui/src/workbench/conflicts.rs` has no `mod tests`. The helpers these tests need are private to `workbench::tests` in `app-gpui/src/workbench/mod.rs`: `conflicting_branches`, `merge_into_conflicts`, `merge_view_loaded`, `settle` and `context_is_on`, plus `git_project`, `init_repo`, `git_in`, `wait_long`, `wait_for_prompt` and `wait_for_notice`. So the only place these files compile is as children of that `mod tests`, under `app-gpui/src/workbench/tests/`:

- `proofs_merge_fixtures.rs` (repositories and helpers the others share, all `pub(super)`)
- `proofs_merge_apply.rs`
- `proofs_merge_change.rs`
- `proofs_merge_edit.rs`
- `proofs_merge_compare.rs`
- `proofs_merge_focus.rs`

In `app-gpui/src/workbench/mod.rs`, inside `mod tests {`, add these after the line `    mod proofs_editor_search_safe_delete;` (it keeps the list alphabetical, before `    mod proofs_services;`):

```rust
    mod proofs_merge_apply;
    mod proofs_merge_change;
    mod proofs_merge_compare;
    mod proofs_merge_edit;
    mod proofs_merge_fixtures;
    mod proofs_merge_focus;
```

Sibling files use `use super::proofs_merge_fixtures::*;` together with `use super::*;`.

## The rows that were missing or partial

From `docs/gpui-port/parity.json`, these `action:merge.*` rows were missing: accept, acceptTheirs, annotate, apply, applyLeft, applyRight, change, compareBaseResult, compareBaseTheirs, compareBaseYours, compareResultTheirs, compareYoursResult, copy, cut, ignore, paste, resolveAutomatically, resolveSimple, resolveUsingTheirs, resolveUsingYours, revert, selectAll. These were partial: compareYoursTheirs, settings, autoApply, reload. These were already done and are not reworked: applyAll, compare, cancel, acceptYours.

The manifest gives no key for any `merge.*` action, so there is no key to press. Every "does nothing where the `when` is false" check runs the action through the registry with `execute_action`, which is the `try_action` helper. The check asserts that it returns `false` and that nothing visible changed.

## Tests, the rows they prove, done or partial

### proofs_merge_apply.rs

- `applies_those_of_the_chosen_side_only` (mergeModel.test.ts case)
  - `action:merge.applyLeft` becomes **done**. It takes Yours' line 9 alone, the status becomes "2 changes left, 1 conflict", and the caret moves to the first change left. The key is then off, and running the action does nothing.
  - `action:merge.applyRight` becomes **done**. It takes Theirs' line 1, and the caret goes to the conflict at offset 10 (line 5). It is then off.
- `resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more` (mergeModel.test.ts case, and "sends the settled changes back to the base and leaves the pending ones")
  - `action:merge.resolveSimple` becomes **done**. It takes both non-conflicting changes and leaves the conflict of the same word. It is then off and does nothing.
  - `action:merge.revert` becomes **done**. It is off before anything is settled. Run, it brings back the base text and "3 changes left, 1 conflict", then it is off again.
- `resolve_simple_conflicts_then_apply_writes_both_sides_words_and_stages_the_file`
  - `action:merge.apply` becomes **done**, together with the next test. With nothing left it asks nothing. It writes `let x = 1 + 2;` to disk, `git ls-files -u` is empty, `git status --porcelain` shows `M  a.txt`, and the "a.txt: conflicts resolved" notice appears.
  - It also supports `action:merge.resolveSimple`, for a word-level conflict.
- `apply_merge_result_asks_about_unresolved_changes_and_writes_the_result`
  - `action:merge.apply`. While a change is unresolved, the "Unresolved Changes" question appears. Escape keeps the editor and leaves 3 unmerged stages. Enter writes the base `one\n` and stages it.
  - After the editor closes, `mergeEditorFocus` and `merge.apply` are off and the action does nothing. That is the OFF reading for `context:mergeEditorFocus`, which is already done.
- `accept_theirs_asks_once_the_result_was_changed_and_writes_their_version`
  - `action:merge.acceptTheirs` becomes **done**. After the result changes, the "Accept Theirs" question appears and No keeps the result. Discard Changes writes `feature\n` and stages it, and MERGE_HEAD stays. Once the editor is closed the action does nothing.

### proofs_merge_change.rs

- `settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it` (mergeModel.test.ts case)
  - `action:merge.ignore` becomes **done**, together with the result-pane case below. It runs from Theirs' line 2, leaves the text, and the conflict still counts. It is then off for that side.
  - `action:merge.accept` becomes **done**. It is off in the result and on an unchanged line. On Yours' line 2 it replaces the text and settles the conflict. On Theirs' line 4 it takes the one-sided change.
- `takes_the_text_and_leaves_a_conflict_open_for_the_other_side` (mergeModel.test.ts case, and "appends the other side after the first")
  - `action:merge.accept`. Yours' text goes in and the conflict stays open. Accept from Theirs then appends `R` below `L`.
- `settles_the_change_at_once_when_resolving_using_a_side` (mergeModel.test.ts case)
  - `action:merge.resolveUsingYours` becomes **done**, and so does `action:merge.resolveUsingTheirs`. Each settles the conflict at once with its side. Both are then off, and running Theirs does nothing.
- `merges_a_conflict_whose_sides_changed_different_words` (mergeModel.test.ts case, and "settles a conflict with the base when ignoring both sides")
  - `action:merge.resolveAutomatically` becomes **done**. In the result it gives `let x = 1 + 2;` and "All changes processed". It is then off and does nothing.
  - `action:merge.ignore`, in the result: both sides are settled and the base is kept. Apply then writes the base and stages it.
- `resolve_merge_change_does_what_its_argument_says_to_the_change`
  - `action:merge.change` becomes **done** once the fix below lands. It is EXPECTED TO FAIL until then. Its argument `{indexes, action}` applies accept/resolve, ignore of both sides, and auto to changes 0, 1 and 2. A second run on a settled change changes nothing.
- `annotate_with_git_blame_is_offered_in_the_side_panes_only`
  - `action:merge.annotate` stays **partial**. It is EXPECTED TO FAIL, as described under "Behaviours that differ". The test asserts the key is on in Yours' and Theirs' panes and off in the result. What is missing: no test shows the blame annotations, because the native side panes cannot draw them.

### proofs_merge_edit.rs

- `select_all_copy_cut_and_paste_edit_the_result`
  - `action:merge.selectAll`, `action:merge.copy`, `action:merge.cut` and `action:merge.paste` become **done**.
  - Copy and Cut are off without a selection, run does nothing, and the clipboard is unchanged.
  - Select All selects `0..14`. Copy puts the base on the clipboard. Cut empties the result.
  - Paste inserts the clipboard text at the caret, and the caret moves to `7..7`.
- `a_side_pane_copies_its_version_but_is_not_cut_or_pasted_into`
  - The same rows, in a side pane. Paste and Cut are off and do nothing. Select All selects Yours, and Copy puts Yours on the clipboard.

### proofs_merge_compare.rs

- `each_comparison_shows_its_two_versions_in_the_diff_window`
  - `action:merge.compareYoursResult`, `compareResultTheirs`, `compareBaseYours`, `compareBaseResult` and `compareBaseTheirs` become **done**, and `action:merge.compareYoursTheirs` goes from partial to **done**.
  - Each runs through the registry. The diff window's `DiffData.left` and `.right` are the exact `DiffSide::Text` of the two versions, with the titles from `view.titles()`, `Base` and `Result`. The Result is the text after Apply All.
  - Escape closes each one, and `mergeEditorFocus` is on again.
- `automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge`
  - `action:merge.autoApply` goes from partial to **done**. Its Merge Settings row shows checked or unchecked, and `action_toggled` reads false, then true, then false. The value is stored `"true"`, then `"false"`. Switching it on applies the changes at once. The next merge editor starts with them applied and `is_modified()` false.
  - `action:merge.settings` goes from partial to **done**, because the menu's rows are now asserted.
  - It also exercises `action:merge.cancel` (already done): Cancel asks after auto-apply, and does not ask in an editor where nothing was decided.
- `reload_from_disk_reads_the_versions_again`
  - `action:merge.reload` goes from partial to **done**. After Theirs' stage 3 in the index changes (`git update-index --index-info`), Reload shows the new Theirs text (`R2`). The decisions are dropped: the result is the base, the editor is not modified, and it is the same editor.

### proofs_merge_focus.rs

- `keeps_merge_commands_registered_and_disabled_outside_a_merge_editor` (mergeActions.test.ts case)
  - This strengthens `context:mergeEditorFocus` and `context:merge.`, which stay **done**.
  - It checks that 30 `merge.*` ids are registered. All of them are off and do nothing before the merge editor opens. Once it opens, `mergeEditorFocus` and `diffEditorFocus` are ON, and the commands that are always available are ON.
  - All of them are off and do nothing again while the text editor behind it has the keyboard: `mergeEditorFocus` and `diffEditorFocus` are OFF there, nothing is applied or closed, and nothing is asked. Once the merge editor has the keyboard back, `mergeEditorFocus` is ON and Apply All runs.
  - This also reads `context:diffEditorFocus` (already done) ON and OFF for the merge editor.
- `diff_settings_and_jump_to_source_act_on_the_merge_editor_that_has_the_keyboard`
  - `action:diff.showSettings` and `action:diff.jumpToSource` are already done; this adds the merge-editor case. It is EXPECTED TO FAIL until the fix below lands.
  - Diff Settings opens the merge settings menu.
  - Jump to Source opens `a.txt` in its editor, and that editor has the keyboard.

Count: 18 tests (5 apply, 6 change, 2 edit, 3 compare, 2 focus). 25 rows become done: 24 at once, and `merge.change` with fix 1. 1 row stays partial (`merge.annotate`). 4 rows that were already done are strengthened (`context:mergeEditorFocus`, `context:merge.`, `action:diff.showSettings`, `action:diff.jumpToSource`).

## Fixes the tests depend on (exact hunks)

### 1. `merge.change` runs with its argument

The webview's `runAction("change", arg)` applies `applyTo(indexes, action)`. Natively the argument is dropped, and `MergeView::run(Change)` does nothing.

`app-gpui/src/vcs/merge_view.rs`: add this after `pub fn apply_control`:

```rust
    /// Resolve Merge Change: `action` on the changes at `indexes`, as one step.
    pub fn apply_change(&mut self, indexes: &[usize], action: MergeAction, cx: &mut Context<Self>) {
        if !self.resolved {
            self.transact(|s| s.apply_to(indexes, action), false, cx);
        }
    }
```

`app-gpui/src/workbench/conflicts.rs`:

```diff
+use crate::vcs_diff::merge_gutters::MergeSide;
+use crate::vcs_more::merge_model::{AutoSide, MergeAction};
@@ register_actions
         let action = Action::new(meta, move |call, window, cx| {
-            on_workbench(window, cx, |workbench, _, cx| workbench.run_merge_action(&id, call.ctx, cx))
+            on_workbench(window, cx, |workbench, _, cx| workbench.run_merge_action(&id, call.ctx, call.arg, cx))
         });
@@ (a free function, next to `merge_versions`)
+/// The argument of Resolve Merge Change: `{ "indexes": [n…], "action": { "type": "accept" | "ignore" | "auto",
+/// "side": "left" | "right" | null, "resolve": bool } }`.
+fn merge_change(arg: &serde_json::Value) -> Option<(Vec<usize>, MergeAction)> {
+    let indexes = arg.get("indexes")?.as_array()?;
+    let indexes =
+        indexes.iter().map(|i| i.as_u64().and_then(|i| usize::try_from(i).ok())).collect::<Option<Vec<_>>>()?;
+    let action = arg.get("action")?;
+    let side = match action.get("side").and_then(serde_json::Value::as_str) {
+        Some("left") => Some(MergeSide::Left),
+        Some("right") => Some(MergeSide::Right),
+        _ => None,
+    };
+    let action = match action.get("type")?.as_str()? {
+        "accept" => MergeAction::Accept {
+            side: side?,
+            resolve: action.get("resolve").and_then(serde_json::Value::as_bool).unwrap_or(false),
+        },
+        "ignore" => MergeAction::Ignore { side },
+        "auto" => MergeAction::Auto { side: side.map_or(AutoSide::Base, AutoSide::Side) },
+        _ => return None,
+    };
+    Some((indexes, action))
+}
@@ impl Workbench
-    fn run_merge_action(&mut self, id: &str, keys: &ContextKeys, cx: &mut Context<Self>) {
+    fn run_merge_action(
+        &mut self,
+        id: &str,
+        keys: &ContextKeys,
+        arg: Option<&serde_json::Value>,
+        cx: &mut Context<Self>,
+    ) {
         let Some(view) = self.merge_view() else { return };
         let Some((&target, command)) = self.conflicts.focused.target(id, keys) else { return };
-        if target == view.entity_id() {
-            view.update(cx, |view, cx| view.run(command, cx));
-        }
+        if target != view.entity_id() {
+            return;
+        }
+        if command == MergeCommand::Change {
+            // Resolving one change takes the change and what to do with it.
+            if let Some((indexes, action)) = arg.and_then(merge_change) {
+                view.update(cx, |view, cx| view.apply_change(&indexes, action, cx));
+            }
+            return;
+        }
+        view.update(cx, |view, cx| view.run(command, cx));
     }
```

A unit test of `merge_change` (bad shapes give `None`) belongs with it. The integrator can put that test inline in conflicts.rs if it adds a `mod tests` there.

### 2. Diff Settings and Jump to Source in the merge editor

The webview's merge editor registers a `DiffNavigator` whose `showSettings` runs `merge.settings` and whose `jumpToSource` opens the file. Natively both actions go to the diff window only, and do nothing while the merge editor has the keyboard.

`app-gpui/src/workbench/diff_window.rs`:

```diff
             on_workbench(window, cx, |workbench, window, cx| {
                 // The merge editor that has the keyboard moves between its own changes.
-                let moved = merge.is_some_and(|direction| workbench.merge_go_to_change(direction, window, cx));
+                let moved = match merge {
+                    Some(direction) => workbench.merge_go_to_change(direction, window, cx),
+                    None => command == DiffCommand::JumpToSource && workbench.merge_jump_to_source(window, cx),
+                };
                 if !moved {
@@
     registry.handle(action::SHOW_SETTINGS, |_, window, cx| {
-        on_workbench(window, cx, |workbench, window, cx| workbench.show_diff_settings(window, cx))
+        on_workbench(window, cx, |workbench, window, cx| {
+            if !workbench.merge_show_settings(window, cx) {
+                workbench.show_diff_settings(window, cx);
+            }
+        })
     });
```

`app-gpui/src/workbench/conflicts.rs`, in `impl Workbench`, next to `merge_go_to_change`:

```rust
    /// Jump to Source while the merge editor has the keyboard: the file opens in its editor, in the merge
    /// editor's place.
    pub(super) fn merge_jump_to_source(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.merge_editor_focused(window, cx) {
            return false;
        }
        let Some(path) = self.merge_view().map(|view| PathBuf::from(view.read(cx).path())) else { return false };
        self.close_merge(false, window, cx);
        self.open_project_file(path, window, cx);
        true
    }

    /// Diff Settings while the merge editor has the keyboard: the merge editor's settings menu.
    pub(super) fn merge_show_settings(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if !self.merge_editor_focused(window, cx) {
            return false;
        }
        self.run_merge_command(MergeCommand::Settings, cx);
        true
    }
```

Product question for Jump to Source: in the webview the merge editor is a tab, so jumping keeps the merge work behind it. The native editor is an overlay, so the hunk closes it and drops undecided work. The alternative is to ask first, as Cancel does when `is_modified()`. The test does not depend on this choice: its merge editor is unmodified. Flag it `needs-decision` if it matters.

### 3. `merge.annotate` (no small hunk)

The webview enables Annotate whenever a side pane is active (`paneRef.current !== "result"`). Running it toggles git blame annotations in that pane, at `HEAD` or the other side's commit. Natively `MergeView::enabled(Annotate)` is always `false`, and `run` ignores it.

Making only the key true, with `MergeCommand::Annotate => self.active != MergePane::Result`, would make the test pass but give the user a command that does nothing. Do not do that alone. The real fix is blame in the side panes: per-line author and commit, read from `ide_host::ops::vcs` blame for the `HEAD` side and the other side's commit, and drawn in the gutter. That fix is out of scope for a draft hunk. Until it exists, keep the test `#[ignore]`d with that reason, or leave it red. The row stays partial.

## Behaviours that differ from the webview

1. `merge.change` drops its argument; see fix 1.
2. `merge.annotate` is never enabled; see fix 3. The existing assertion in `the_merge_actions_move_between_the_changes_and_apply_the_non_conflicting_ones` (`!enabled("merge.annotate")`, "blame is not shown in the side panes") holds there in the webview too, because the caret is in the result. It stays valid after a fix.
3. `diff.showSettings` and `diff.jumpToSource` do nothing in the merge editor; see fix 2.
4. Smaller, and not a bug: the check mark of `merge.autoApply` comes from the stored setting natively (`stored_auto_apply`) and from the focused editor's own state in the webview (`focused?.autoApply()`). Both always switch together, because the editor writes the setting on every switch, so no test separates them.

Requested items with no action in either UI, so no tests were written for them: "next/previous conflict" (only Previous and Next Change exist, which are `diff.prevDifference` and `diff.nextDifference` and are already done), "highlighting and whitespace policies", "show/hide details", and a separate "magic resolve". The last is `merge.resolveSimple` (toolbar wand) and `merge.resolveAutomatically` (wand and menu row), both covered above. Neither `MergeToolbar.tsx` nor `mergeMenus.ts` has any of these.

## Tests most likely to fail on first run

1. `resolve_merge_change_does_what_its_argument_says_to_the_change`: expected until fix 1 lands.
2. `annotate_with_git_blame_is_offered_in_the_side_panes_only`: expected; see fix 3.
3. `diff_settings_and_jump_to_source_act_on_the_merge_editor_that_has_the_keyboard`: expected until fix 2 lands. Once it lands, check that `open_project_file` really gives the file's editor the keyboard. If it doesn't, add `window.focus` of `self.editor()` in `merge_jump_to_source`.
4. Every test that uses `click_line` (the change, edit and annotate tests). The line's y is pane top + `lh-code` × (line + 0.5), as in `merge_view.rs`'s own `click_line`, and it relies on the panes not being scrolled. If the click lands on the wrong line, the side-pane commands stay off. Debug it with `view.selection(MergePane::Side(..))` right after the click.
5. `reload_from_disk_reads_the_versions_again`. It depends on two things: `git update-index --index-info` replacing only stage 3, and `ide_vcs::Repo::conflict_versions` (gix `:3`) reading the index afresh rather than a cached one. If gix caches the index, the wait times out; that would be a real bug for Reload too.
6. `keeps_merge_commands_registered_and_disabled_outside_a_merge_editor`. It needs a re-render after `window.focus` on the editor behind for `publish_merge_context` to blur; the test forces one with `cx.notify()`. Separately, the editor's `on_focus_in` (`editor_took_focus`) might also close the merge overlay or move it. If so, the assertion "nothing closed the merge editor" fails and is worth a look.
7. Exact-string status texts ("2 changes left", "All changes processed") come from `status_text` with `I18n::default()`. They hold as long as the default catalog is English.
8. `git status --porcelain` is asserted as `M  a.txt` where the result differs from HEAD. That assumes nothing else in the work tree shows up in status (`.openidea/` is excluded by `init_repo`, and `set_stage` writes its blob inside `.git/`).

## Spec files and cases covered through the app

- `src/features/vcs/__tests__/mergeActions.test.ts`: 2/3. Covered: "keeps merge commands registered and disabled outside a merge editor" and "shares changing enablement and toggle state with action menus" (the settings row's checked state, and keys going off as changes are taken). Not covered: "routes only to the focused editor and preserves it when another editor unmounts", because the window shows one merge editor at a time. The unit row is already done by `vcs_more::merge_actions::tests`.
- `src/features/vcs/__tests__/mergeModel.test.ts`: 12/32 through the app. Covered:
  - "starts from the base with every change pending"
  - "takes the text, and leaves a conflict open for the other side"
  - "appends the other side after the first, in either order" (one order only)
  - "settles the change at once when resolving using a side"
  - "settles one side of a conflict and leaves the text; the other side then replaces it"
  - "settles a conflict with the base when ignoring both sides"
  - "merges a conflict whose sides changed different words"
  - "resolves the simple conflicts and the non-conflicting changes together, and no more"
  - "applies those of the chosen side only"
  - "applies all of them from the base, and leaves conflicts" (already, and again in the compare test)
  - "sends the settled changes back to the base and leaves the pending ones"
  - "finds the changes in the selected lines of a side"

  The rest are model internals: CRLF, missing final newline, whole-file conflicts, history and undo, insertion points, and the summary table. They are proved by `vcs_more::merge_model::tests`, and that unit row is already done.

- `src/features/vcs/__tests__/GitMergeDiff.test.tsx`: 0/1. It covers the historical merge comparison, which is not the merge editor and is out of scope.
