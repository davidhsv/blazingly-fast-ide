I reconciled the diff gaps draft and wrote the notes; nothing has been compiled. `rustfmt --check` parses every edited draft and pending file with no errors. A script applies all 81 hunks in memory, in order, and the patched files parse after each of the three parts.

**When the webview uses a tab and when a dialog.** The deciding setting is `vcs.commit.nonModal`, which defaults to on (`workflowSettings.ts:207`, `workflow_settings.rs:123`). There are two entry points:

- **`openDiff` always opens a tab** (`diffInput.ts:133-135`). It is used by `vcs.openDiff` (`compare.tsx:149`), by chain navigation inside a tab (`DiffEditor.tsx:686-696`, as a preview tab), and by `e2e/diff-navigation.spec.ts` itself.
- **`openVcsDiff` opens the dialog only under the modal interface** (`diffInput.ts:142, 151-155`), otherwise it calls `openDiff`. It is used by Show Diff (`actions.tsx:367-412`), the Commit window (`CommitView.tsx:204-210`) and Compare With… (`compare.tsx:241`).

So with default settings both e2e specs expect tabs.

**Hunks.** The original notes had 43 hunks. All 63 anchor fragments still occur exactly once, because only `mod.rs` and `tab_views.rs` changed after the draft commit.

- **Kept: 36**, of which 8 are split across the three parts.
- **Re-anchored: 1.** The `all_hidden` hunk's end marker was not unique; it now anchors on the end of `hide_all`.
- **Rewritten: 6.**
  - The `mod` lines.
  - `RowPaint.row` is dropped, because it would become a field never read under `-D warnings`.
  - `wrap_to` now re-measures after a zoom.
  - `position_at` no longer uses a bare `?;` statement.
  - The moved-code test module declaration.
  - The Show Diff routing hunk (now D31) moves to the very end.
- **Dropped: 0.**

New hunks: test pins to the modal interface (D25-D30), `find_file` made visible, a call at the end of `with_layout` so the Maximize button follows the layout, and moving two existing moved-code tests onto moved code.

**Draft edits.**

- `tab_view.rs`: no Maximize button on an embedded diff (`diff-editor-tab.tsx:10`), a `set_maximized` method, and chain neighbours passed to the viewer.
- `diff_tab.rs`: missing imports fixed. I added chain navigation into the preview tab and `sync_diff_tabs_maximized`; neither was in the original draft. Without them Show Diff would now open tabs that cannot move to the next file. The tests cover both rows of the spec's maximize case (nothing maximized, bottom maximized), the remount case, and Hide All Tool Windows.
- `diff_tab/tests/proofs18_e2e_diff_navigation.rs`: rewritten to the spec's three flows.
- `wrap.rs`, `moved.rs`: no change needed; every API they call exists with that signature.

**Pending tests.** These are moved by `git mv` (listed in the notes) rather than pasted into draft modules, because part 1 has no workbench draft module to hold them.

- Soft wraps → `proofs12_diff_soft_wraps.rs`, now pinned to the modal interface.
- Moved code → `proofs12_moved_code_view.rs`, with two more spec cases asserted on moved code.
- Diff tabs → `proofs12_diff_tabs.rs`.
- The diff-navigation pending test is rewritten into the draft file; its pending file is `git rm`'d.

Nothing is left waiting.

**Expected rows.**

- To done: `unit:…/diffToolbar.test.tsx` (25/25), `unit:…/movedCode.test.tsx` (15/15), `unit:…/diffDialog.test.tsx` (8/8), `e2e:e2e/diff-navigation.spec.ts` (3/3).
- To partial: `point.registerEditorKind:diff`, because diff tabs are not restored with the session.
- Stays partial: `e2e:e2e/compareWith.spec.ts`. Routing the compare actions into tabs would lose Accept Left/Right Side, since tab sides are not editable yet.
- Stays 0/1: `point.registerSoftWrapSurface`, because `editor.toggleSoftWrap` has no native handler at all.

**Top five risks.**

1. Show Diff opens tabs by default once D31 lands. The 9 existing tests that read the diff window after Show Diff fail unless D25-D30 land first. I traced all 22 tests that read `diff_dialog()`, plus every `"vcs.showDiff"` literal and the `cmd-d`/`ctrl-d` shortcut tests.
2. This is a regression on the default path: in a tab, `diff.nextFile`, `diff.prevFile` and `diff.showSettings` do nothing. Their done rows hold on the modal path only.
3. Keyboard focus may not follow the new preview tab after each chain step. The third F7 in the binary-skip test is pressed without refocusing, so that test shows it first.
4. Whether moved-code marks and links get clicks without blocking the pane depends on GPUI hit-testing, and the width assertion in the maximize test depends on the default test window size.
5. The soft-wrap tests rely on the settings listener firing for both `editor.softWrap` and `editor.general.softWrapFiles`.

**Anchors script output** (run last, after re-counting `mod.rs`): `81 hunks, 0 problem(s)`, every line `ok`.

Files are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`:

- `drafts/diff_gaps_reconciled.md`
- `dfg/anchors.py`
- `dfg/hunks.py`
- `dfg/gen_notes.py`
- `dfg/apply_check.py`
