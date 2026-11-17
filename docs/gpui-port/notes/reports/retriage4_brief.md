# Reading task: re-verify the `action:` rows at parity whose registering file changed in the merge of upstream v0.1.46

You read and compare; you build nothing. You never run cargo, rustc, pnpm, either app, or the network; you never edit a tracked file and run no git command that changes anything. Another agent is editing `app-gpui/src` right now: read every file under `app-gpui/` with `git show HEAD:<path>` or `git grep … HEAD -- <path>`, never from the working tree. Use `git diff -U3` when you read a diff (this machine's git shows a thousand lines of context by default). Every temp file goes under your scratch subfolder, never `/tmp`; tell any helper you start the same.

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01` (branch `gpui`, HEAD `248ed4718`; the fork's base is upstream `37c2a604`, the merged tag `v0.1.46`; this branch's tree before the merge is `3662ca0ea`). Scratch: `S=/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`; your own subfolder `$S/dret4/` (create it).

## What this is

`app-gpui/` is a native rewrite of the IDE's webview UI (`src/`). Parity is tracked row by row in `docs/gpui-port/parity.json` (key → `{status, proof}`; a proof cites native tests by full name in backticks, then after a semicolon says what works and, for partial, what is missing). An `action:<id>` row is at parity when the native app has the action with the same title, the same conditions for being shown and enabled, the same key in the keymaps, the same places in menus, and does the same thing, proven by a native test that drives it. Read the top of `docs/gpui-port/INVENTORY.md` first. The native tests, one full name per line: `$S/rust_tests.txt`. The native action registry: `app-gpui/src/platform/actions.rs` and the modules that register actions (grep an id).

`docs/gpui-port/notes/RETRIAGE.md`, section "Actions", lists 454 done action rows whose registering file changed in the merge. The measure is coarse: any change in the file counts. Two earlier readers did the spec rows and the backend commands (`docs/gpui-port/notes/RETRIAGE_UNIT_1.md`, `RETRIAGE_IPC.md`: read their "what turned out wrong" notes; reuse their scripts under `$S/dret1/` and `$S/dret2/tools/` by copying). Leave out any row that is no longer done in `parity.json` at HEAD.

## Method

1. By script, for every row: the action's registration at `3662ca0ea` and at HEAD (the object literal or call that registers the id: title, `when` or enablement, key bindings in the keymap files under `src/platform/keymaps/`, menu placements: grep the id in menu definitions) and its handler (the function it runs, followed one level into the same file or the imported function it calls). Classify: `same` (registration and handler text equal after whitespace), `moved` (equal text in another file or line), `changed`.
2. For every `changed` row, read: what changed (one or two lines, with `file:line` at HEAD and the upstream commit's subject), then the native action (its registration and handler at HEAD) and the tests the proof cites: does the native action have the new title, condition, key, menu place and behaviour?
3. A verdict: `keep` (same or moved; or changed and the native action already matches, with the test that shows it), or `lower` to partial with the proof text to write: the citations that remain true, then after a semicolon what works and what is missing, each missing point with the webview's `file:line`. A handler that changed only inside the backend, or by a refactor with the same behaviour, is `keep` with one line saying so.

You never raise a row. When in doubt, lower it and say why. Start with the files that have the most rows (`src/editor/core/actions.ts` 76, `src/features/vcs/historyActions.tsx` 48, `src/features/project-tree/actions.ts` 44, `src/features/vcs/actions.tsx` 28, `src/features/debugger/actions.ts` 28, `src/editor/lsp/actions.ts` 21, then the rest); if you cannot finish all 454, finish whole files and say which files are not read.

## Deliver

- `$S/dret4/RETRIAGE_ACTIONS.md`: at the top the totals (rows same, moved, changed and kept, changed and lowered, not read) and the native differences found, grouped by native file, as work for an integrator (each with the native `file:line`, what the webview does, and the size of the fix as you judge it: one line, a few lines, a feature); then one section per registering file with a table of its changed rows (key, what changed, native state, verdict).
- `$S/dret4/lowerings.json`: an object key → `{ "status": "partial", "proof": "<the full new proof text>" }` for every row you lower, and nothing else. Every citation in a proof must be a full test name present in `$S/rust_tests.txt` (check each by script and say so).

Markdown: one paragraph per line, no hard wrapping. Your final message (all the manager sees): the totals; the native differences, largest first; the files not read; anything in these instructions or in `RETRIAGE.md` that turned out wrong. Mark every statement you did not measure as "Hypothesis:".
