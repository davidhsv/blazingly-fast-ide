# Writer brief: the editor commands that need the document's syntax tree, written without a compiler

You write a feature draft for a native Rust (GPUI) rewrite of an IDE's UI. Another agent is compiling right now: you NEVER run cargo, rustc, rustfmt, clippy or the binary, and you NEVER edit an existing file. You write NEW files only, plus notes that tell the integrator which hunks to apply to existing files.

Worktree (work only here): `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01` (branch `gpui`). Native app: `app-gpui/` (crate `openidea_gpui`). The app it must match 1:1: the webview UI under `src/` (React/TypeScript). Shared scratch: `S=/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`; your own subfolder `$S/dsyn/` (create it). Temp files go there, never `/tmp`.

Read first, whole: `$S/wiring_brief.md` (how features are wired: actions, backend calls), `$S/view_rules.md`, `$S/tests_brief.md` (how tests drive a project window and what they must not use).

## The feature

In the webview several editor commands ask the backend for the structure of the document (its syntax tree) and fall back to text rules without it. Natively the text rules exist and the structural part does not. The backend functions exist: `crates/ide-host/src/ops/syntax.rs` (`syntax_support`, `syntax_node_at`, `syntax_expression_at`, `syntax_errors`, `syntax_locals`, `syntax_identifiers`, `syntax_identifiers_in_files`, `syntax_structural_ranges`); the Tauri adapters that show how each is called: `src-tauri/src/commands/syntax.rs`. The webview side: `src/editor/core/cm/structural.ts` and its test, and every caller of the structure service (`grep -rn 'structur\|syntaxNodeAt\|syntaxExpressionAt\|syntaxErrors\|syntaxLocals\|syntaxStructuralRanges' src --include='*.ts' --include='*.tsx' -l | grep -v bindings`). Natively some of it is already called: read `app-gpui/src/editor/intelligence.rs`, `app-gpui/src/workbench/editor_extras.rs`, `app-gpui/src/workbench/refactor_actions.rs` (grep `ops::syntax::`) and the native model `app-gpui/src/editor_commands/structural.rs` with its tests, and build on what is there instead of beside it.

What "done" is, as inventory rows (`docs/gpui-port/parity.json`; each row's proof text says what is missing today, read it):

- `action:code.commentLine`, `action:code.commentBlock`: a block comment that contains its own closing token is handled as the webview does once the tree confirms the comment's span.
- `action:code.completeStatement`: the structural answers of the backend are asked, as the webview does, before the text rules.
- `action:code.moveStatementUp`, `action:code.moveStatementDown`: a whole statement moves and stays inside its block.
- `action:code.moveElementLeft`, `action:code.moveElementRight`: elements come from the syntax tree.
- `action:code.toggleCase`: escapes inside strings are protected.
- `action:code.quickDefinition`, `action:code.quickTypeDefinition`: the popup shows the declaration's whole span from the tree, with syntax colours (the Paste from History preview, `app-gpui/src/editor/paste_history_view/preview.rs`, shows how a read-only coloured preview in the editor's font is drawn).
- `action:editor.contextInfo`: without backend fold ranges, the fallback to the tree's blocks (`enclosingHeaders`).
- `action:find.highlightUsages`: without a language server, the fallback to the local's same-scope occurrences from the tree.
- `action:navigate.declaration`: read its row; build only what the tree gives without a server, if anything is still missing.
- `unit:src/editor/core/cm/structural.test.ts`: 23 of 27 cases are native; the 4 left (whether a document has a syntax tree, and the cache of structure queries) belong to the editor-side service you write. A unit row is done only when every case has a native test with the case's name (snake case) checking the same thing.
- `ipc:syntax_errors`, `ipc:syntax_expression_at`, `ipc:syntax_structural_ranges` (and any other `syntax_*` call still unrecorded: check `parity.json`): called through `ide_host::ops::syntax::*` with the same arguments as the Tauri adapter. An `ipc:` row is done only when a test asserts state that can only come from that backend function.

For every one of these, first read the webview's code path whole and write down, in the notes, exactly what it does with the tree and what it does without one (`file:line`); then write the native counterpart. Where the native text rule already gives the same result as the webview's structural path in every case you can construct, say so and add the test that proves it instead of new code.

Design constraints:

- One editor-side structure service, as the webview has one: it asks the backend off the window's thread, caches answers per document and version (the webview's cache rules are the 4 spec cases: follow them), drops a stale answer, and never blocks a keystroke. A command that needs the tree and does not have it yet does what the webview does in that moment (waits, or falls back): read it, do not guess.
- Nothing in paint asks the backend or walks the whole document.
- The syntax tree needs no language server: tests use real small files of languages whose grammar is bundled (read how `refactor_actions.rs`'s `finds_a_local_through_the_syntax_tree`-style tests and `editor_extras.rs` do it) and never start a language server of the machine, a toolchain, or anything under the user's home.
- While you are there, one measurement by reading only: a test of another slice found that the native highlighting of a one-line Kotlin file (`val s = "x"`) gives a single Variable span and no string or keyword span. Read where native highlights come from (`ide_language::Syntax::highlights`, `languages/kotlin.toml`, the Kotlin grammar's highlight query under `crates/`) and say in the notes why, and what the smallest fix is and where (do not edit `crates/` or `languages/`).

## What you deliver

1. New files only (leave them untracked; run no git command that changes anything): the service and the structural parts of the commands in new files (suggested: `app-gpui/src/editor/structure.rs` or a folder `app-gpui/src/editor/structure/`, and `app-gpui/src/workbench/editor_structure.rs` for the wiring), tests in new files (`proofs25_*.rs` beside the modules they test; end-to-end ones under `app-gpui/src/workbench/tests/`). Nothing declares them yet, so they are not compiled: write code that will compile, reading the real signature of everything you call (open the file and read the function; never guess an API of GPUI or of this crate). Keep each file under about 800 lines. Where a test waits on something not built, put it in a file named `*_pending.rs` and say so; never `#[ignore]`.
2. Notes at `$S/drafts/syntax_tree_integration.md`: (a) every hunk for existing files, in the order to apply them, each with the file, a quoted anchor of the lines just before it (copied exactly from the file), and the new lines; group the hunks by command so the integrator can land one command at a time; (b) the table of parity keys with the state you expect (done or partial), the proving test by full name, "N of M cases" for the unit row, and for partial what is missing; (c) per command: what the webview does with and without a tree (`file:line`) and what you built; (d) your hypotheses: every API or behaviour you assumed without being able to check by compiling, each with how the integrator settles it; (e) the tests most likely to fail and why; (f) the Kotlin finding.
3. Code comments say why the code is as it is: no history, no "port", "webview", "TypeScript", no task or ticket names. Match the neighbours' naming, error handling and test layout. No stubs, no TODO for core logic, no new dependencies, no `unsafe`.
4. Tests drive the app as a user does (an action through the registry, a key) in a project window on a temp project and assert the document's text, the selection or what the window shows; waits use the existing 30-second helpers, never a fixed sleep as proof.

Do not touch (other agents' work, some being compiled now): every existing file; `app-gpui/src/deployment/`, `app-gpui/src/http_client/`, `app-gpui/src/notebook/`, `docs/`, `src/`, `src-tauri/`, `crates/`, `languages/`, `e2e/`. Read them freely.

Your final message (all I will see): the list of files you wrote with line counts; the rows table; what you left out and why; your hypotheses; the Kotlin finding; anything in the existing native code or in these instructions that turned out wrong. Mark every statement you did not verify by reading the code as "Hypothesis:".
