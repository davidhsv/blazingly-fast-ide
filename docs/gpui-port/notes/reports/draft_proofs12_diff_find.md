I wrote 43 tests in five new files, but none of them has been compiled or run: I had no compiler. Eleven of them are meant to fail until the native code gains the missing behaviour, and one file needs a small accessor (H1) before it compiles. Every expected failure goes through API that exists today (a missing on-screen element, a stored setting, a menu label), so the files compile.

## Files

| File                                                              | Lines | Tests        |
| ----------------------------------------------------------------- | ----- | ------------ |
| `app-gpui/src/workbench/tests/proofs12_find_popup.rs`             | 551   | 17           |
| `app-gpui/src/workbench/tests/proofs12_diff_window.rs`            | 367   | 11           |
| `app-gpui/src/workbench/tests/proofs12_diff_soft_wraps.rs`        | 153   | 3 (needs H1) |
| `app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view.rs`    | 53    | 2            |
| `app-gpui/src/vcs_diff/diff_options/tests/proofs12_moved_code.rs` | 180   | 10           |

The notes file lists, for each spec, every case with the Rust test that proves it (existing or new) or why it is not written. It also gives the exact `mod` lines and where they go: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs12_diff_find.md`

## Rows

All four rows stay **partial**:

- **diffToolbar:** 16 of 25 cases are proven; 7 are expected to fail; 2 cannot be written.
- **movedCode:** 13 of 15 are proven, 2 of them only weakly; 2 are expected to fail.
- **diffDialog:** 5 of 8 are proven; 3 are expected to fail.
- **findPopup:** 19 of 25 are proven; 4 of those are only partly proven; 1 is expected to fail and 1 cannot be written.

## Hunk H1

One accessor, `DiffView::soft_wraps()`, is needed only by `proofs12_diff_soft_wraps.rs`. Leave that file's `mod` line out until the accessor is added. Its body returns today's stored choice, so the editor-following assertions still fail.

## Behaviour that differs from the webview

- **Revision-to-revision diffs have no Jump to Source** (diffToolbar 9, 22, 23, 24). The native viewer only offers it when a side is a file on disk, and the jump itself returns early. The webview checks that the working file exists and maps the line through to it.
- **The non-modal commit setting is ignored** (diffDialog 8). `Workbench::show_diff` always passes `false`, even though the setting defaults to on. The native app also has no diff editor tabs at all, which is why diffToolbar 15 and 17 can't be written.
- **Soft wraps** (diffToolbar 11–13): the setting is stored but nothing reads it. The settings menu has no "Use Soft Wraps" item.
- **Moved code**: only the counter knows about moves. There are no move links, no moved-line marks and no "(edited)" label (movedCode 11, 13).
- **Diff window** (diffDialog 5, 7): its size is fixed, so it can't be resized or remembered. Only Escape closes it; ⌘W closes the editor tab behind it instead.
- **Find in Files popup**:
  - Its size is fixed (case 21).
  - The browse dialog can't start at the current folder, because GPUI's path prompt has no starting directory (case 7).
  - Fields hold one line, so a multi-line query and its key hint are impossible (case 17).
  - Scope-tab letters are never underlined (case 18).
  - Query validation is synchronous, so only the "while the query is resting" part of cases 9 and 10 can be tested.

The notes file says where each fix would go and which element name each test expects.

## Corrections to the earlier list in `proofs11_windows.md`

- findPopup cases 3, 4 and 22 were already covered by existing tests.
- The diffToolbar "Unified Revert" cases were already cited by existing tests.
- Several existing citations are weak:
  - The existing "embedded comparison" test never exercises a diff that asks for a viewer kind against a stored preference, so I added a window version.
  - The "Highlight lines" and "Do not highlight" cases (movedCode 12, 14) never use moved code.
  - The CRLF case passes the same raw text on both sides. Whether the real working-copy path keeps CRLF is an unchecked guess.

## Left out

- The "footer" variant of diffDialog 7: the native diff window has no footer.
- The part of the spec that holds validation and the first new file open separately: there is no native counterpart.

The tests most at risk on first run are listed at the end of the notes. The main ones are the right-click menu and double-click selectors in the Commit window, and the moved-code model cases where native move detection may differ from the webview's.
