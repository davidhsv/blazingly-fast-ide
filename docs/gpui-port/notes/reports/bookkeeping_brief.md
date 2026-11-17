# Reading task: rows that are partial only because nobody finished reading them

You read and compare; you build nothing. You never run cargo, rustc, pnpm, either app, or the network; you never edit a tracked file and run no git command that changes anything. Another agent is editing `app-gpui/src` right now: read every file under `app-gpui/` with `git show HEAD:<path>` or `git grep … HEAD -- <path>`, never from the working tree. Use `git diff -U3` when you read a diff. Every temp file goes under your scratch subfolder, never `/tmp`; tell any helper you start the same. You read nothing under the user's home outside this worktree and the scratch folder.

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01` (branch `gpui`; pin your reads to the commit you start on and say which). Scratch: `S=/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`; your own subfolder `$S/dbook/` (create it).

## What this is

`app-gpui/` is a native Rust (GPUI) rewrite of an IDE's webview UI (`src/`). Parity is tracked row by row in `docs/gpui-port/parity.json` (key → `{status, proof}`; a proof cites native tests by full name in backticks, then after a semicolon says what works and, for partial, what is missing); `docs/gpui-port/INVENTORY.md` is generated from it (read its top: what a row at parity is). A `unit:<spec file>` row is at parity only when every case of the spec at HEAD has a native test that asserts the same thing. The native tests, one full name per line: `$S/rust_tests.txt`.

Some rows are partial not because behaviour is missing but because the manager recorded them without finishing the reading: their proofs say "not re-read", "not mapped", "not checked", "neither confirmed nor missing", "by the integrator's mapping", "do not assert the request id" or "not counted". Find them all by that grep over `parity.json` (about ten; among them `unit:src/features/templates/__tests__/files.test.ts` (32 cases), `templateCreate.test.ts` (6), `unit:src/features/run/remote.test.tsx` (20), `unit:src/features/run/javaModule.test.ts` (six cases that expect a request id with the start), `unit:src/features/project-tree/store.test.ts`, `unit:src/features/settings/__tests__/phpXdebug.test.tsx`, `action:navigate.gotoTest`, `module:src/editor/navigation/gotoTest.ts`, `action:run.stop`, `ipc:window_start_drag`, `module:src/workbench/toolwindows/ToolWindows.tsx`).

## For each row

Finish the reading the proof says was not done: for a spec row, every case at HEAD (count as the generator counts) with the native test that asserts the same thing: read the case's body and the native test's body for every case whose native test has another name, or whose proof says it was not read; for an action, module or backend-command row, the point its proof leaves unchecked, read on both sides. Then one verdict: `done` (give the full proof: the citations, then after a semicolon "N of N cases" and one sentence), or `partial` with an exact missing list (case names, or the unchecked point now settled as missing, each with the webview's `file:line`) in place of the vague words. Five earlier readers lowered 243 rows that claimed more than their tests prove (`docs/gpui-port/notes/RETRIAGE_UNIT_1.md`: read its top): a row goes to `done` only where you read both sides of every case; when in doubt, `partial` with the doubt stated exactly.

## Deliver

- `$S/dbook/BOOKKEEPING.md`: one section per row: what was unread, the table case → native test (or the point → what you found, with `file:line` on both sides), the verdict.
- `$S/dbook/updates.json`: an object key → `{ "status": "done" | "partial", "proof": "<the full new proof text>" }` for every row, and nothing else. Every citation must be a full test name present in `$S/rust_tests.txt` (check each by script and say so).

Markdown: one paragraph per line, no hard wrapping. Your final message (all the manager sees, and it must be the full report): per row the old state, the new state and why; the rows you are least sure of; anything in these instructions that turned out wrong. Mark every statement you did not measure as "Hypothesis:".
