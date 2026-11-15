# Integrator brief: the diff viewer's soft wraps, moved code, and the diff as an editor tab (reconciled drafts)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/ddfg/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## The slice, in three parts landed one at a time

Reconciled notes (self-contained): `docs/gpui-port/notes/integration/diff_gaps_reconciled.md`: 81 hunks in three parts (soft wraps; moved code; the diff tab), the `mod` lines, the pending test files to rename (with plain `mv`: you run no git command that changes anything), the rows expected, hypotheses, risks. Scripts: `python3 -I $S/dfg/anchors.py <worktree>` re-checks every anchor (it must print `0 problem(s)` before you apply a part; re-anchor yourself from the quoted text where the tree moved), `$S/dfg/apply_check.py` applies the hunks in memory and parses the result. Draft files (committed, undeclared): `app-gpui/src/vcs_diff/wrap.rs`, `app-gpui/src/vcs_diff/tab_view.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/workbench/diff_tab.rs`, `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs`, and the pending test files the notes name.

After each part: its tests pass and the whole suite passes once, before the next part starts.

## Decisions already taken

- The webview opens a diff in an EDITOR TAB unless the modal commit interface is chosen (`vcs.commit.nonModal`, on by default: `src/features/vcs/` `workflowSettings.ts`, `diffInput.ts` `openDiff` / `openVcsDiff`); the native app has always opened a window. Part three makes the default follow the webview (the notes' last hunk, D31). It lands ONLY when, in a diff tab, the next and previous file (`diff.nextFile`, `diff.prevFile`: toolbar buttons and shortcuts), the diff's settings menu (`diff.showSettings`), Jump to Source and the close shortcut do what they do in the webview's tab, each proven by a test through the tab. The notes say the draft's tab does nothing for the first three: build that first. If it cannot be done in this integration, land parts one and two, leave the default as it is, and report exactly what the tab lacks.
- The tests that read the diff window after Show Diff (the notes list them: hunks D25–D30) are pinned to the modal interface BEFORE the default flips, without changing what they assert.
- Compare With… and Compare with Clipboard stay in a window in this slice (a tab's sides are not editable yet, and Accept Left/Right would be lost).
- `editor.toggleSoftWrap` has no native handler: if registering it for the editor and the diff is small (the webview: `src/editor/core/actions.ts:736`, its soft-wrap surfaces), do it with tests through the registry and read its `when` on and off; otherwise report it.

## Real window (paint and layout changed: required)

In your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; a temp Git project with a committed file that has long lines changed and a block of lines moved; launcher `$S/insp8/run.sh`, steps `$S/insp/USAGE.md`): the diff with soft wraps on (wrapped lines stay aligned between the two sides, line numbers on the first visual row only); moved-code marks and their links; the diff as a tab, maximized and restored; the diff window under the modal interface. Look at each: nothing cut or overlapping, both sides aligned.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: per part, what landed; one table of every `parity.json` key the notes name with the state it reaches, the proving tests by full name, for partial what is missing, and for `unit:`/`e2e:` rows "N of M cases"; whether the default flipped and the tests that prove the tab's navigation, settings, Jump to Source and close; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (`platform/error_log_sink.rs`, any draft another integration left undeclared) and every undeclared `proofs*` test file this brief does not hand you.
