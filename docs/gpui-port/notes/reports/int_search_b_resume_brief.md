# Integrator brief: finish the Search Everywhere and Recent Files integration that the full disk interrupted

Read first: `$S/int_rules.md` (hard rules and report format: note its two new sections at the end, on disk space and on mutation checks), `$S/tests_brief.md`, `$S/view_rules.md`. Then the original brief `$S/int_search_b_brief.md` (its points to settle, its rows, its shots and its rules all still hold) and the first integrator's report `docs/gpui-port/notes/reports/int_search_b_part1.md` (what is applied, what was seen failing, what was not, what is left out). Your own scratch subfolder: `$S/dsbi2/`. The first integrator's logs and its switch script are in `$S/dsbi/` (read-only for you; do not use `switch.py`: it switches several files at once by hand, which is what left the tree broken).

## The state you start from (measured by the manager)

The first integrator applied the three pieces (the files it lists under "Files edited" are modified in the working tree, uncommitted; the draft test files are declared). Its 16 new tests passed once with the code on. Then the disk filled during its second fail-first build and it stopped with eight behaviours switched off in the tree; the manager restored them with its script (`grep -rn "OFF \*/" app-gpui/src` prints nothing now) and freed space (122 GiB free; the app crate's build output was cleaned, so your first build is a full one). Not done: formatting of the edited files (two tests added last are unformatted and one of them, `a_toggled_action_run_from_the_popup_remembers_the_query_while_it_stays_open`, has never compiled), the "after" filtered run, the fail-first checks of nine behaviours it lists as never seen failing, the gates, the shots in the real window, the rows and spec cases.

## What to do, in this order

1. `df` first (the rules' new section). Build; make the tree compile; run the new tests and the filtered set of the original brief (`everywhere search find_action go_to_file symbol recent switcher`): the same 464 that passed before the edits pass, plus the new ones; any expectation that changed is named with the webview's `file:line`.
2. Read the diff of the first integrator's edits (`git diff -U3 -- app-gpui/src`) against the original brief's points before trusting it: one place for the tab, scope, history and recent actions; nothing read or listed on the window's thread; a late answer never shown, also across the widening; history and recent actions kept where the webview keeps them (the first integrator read: app-wide, in the UI storage; settle whether two windows share them in the webview and assert the same natively).
3. The nine behaviours never seen failing (its list: history browsing with ⌥↑/⌥↓, ⌥↵ running `settings.assignShortcut`, recent actions listed first, the header wrapping, the 16 px icon slot, the late wide answer, remembering on activation, remembering on close, the shortcut drawn beside an action) and the clear-on-leave of the edited files: one mutation check each, by a script that restores the file in a trap (the rules' new section), one at a time, with the log kept. A behaviour whose test does not fail with the behaviour off has a test that proves nothing: fix the test.
4. Format the edited files; the gates of the original brief; the shots of the original brief (through `INSPECT_BASE=/private/tmp/openidea-inspect-<random> $S/insp8/run.sh …`); the rows and every spec case.

## Report

Your final message must be the full report (no short closing line after it), in the format the original brief asks for, covering the whole slice (what the first integrator measured counts only where you name its log and it is not contradicted by yours).

## Do not touch

As in the original brief. Commit nothing, push nothing, run no git command that changes the index, the tree or the stash; delete and rename no file.
