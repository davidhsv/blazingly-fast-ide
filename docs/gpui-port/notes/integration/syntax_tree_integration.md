# Syntax tree commands: integration notes

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. All paths below are relative to `app-gpui/src/` unless they start with `crates/`, `languages/` or `src/`.

Nothing here was compiled. Every claim about compile or run behaviour is a hypothesis unless it says it was read in the code.

## New files

| File                                                           | Lines | What                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------------------------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `editor/structure.rs`                                          | 355   | The editor-side structure service: `StructureCache` (answers per document and version, 64 positions, oldest evicted, failures kept as `None`), `StructureService` (GPUI global: caches per `DocId`, command queues per editor `EntityId`), `Lookup`, `Support`, `parses_itself` (`PARSED_MODES`), `has_syntax_tree`, `is_idle`, the trees `chain_tree` / `escape_tree` / `code_tree`, `enclosing_headers`, and `impl Editor { escape_tree, code_tree, has_backend_folds }`. Declares `pub mod commands;` and `#[cfg(test)] mod proofs25_structure;`. |
| `editor/structure/commands.rs`                                 | 418   | The queue (one per editor, commands run in press order), the backend calls (`syntax_support`, `syntax_structural_ranges`, `syntax_errors`, `syntax_node_at`, each with the adapter's arguments), and the entry points `move_statement`, `complete_statement`, `comment_block`, `comment_line`.                                                                                                                                                                                                                                                       |
| `editor/structure/proofs25_structure.rs`                       | 260   | Plain `#[test]`s: the 4 spec cases of `structural.test.ts` left, plus the trees and Context Info's headers.                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `workbench/editor_structure.rs`                                | 118   | `Workbench::context_info_from_tree` (Context Info without fold ranges) and `local_usages` (Highlight Usages without a server).                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `editor_lsp/definition_source.rs`                              | 147   | `DefinitionTarget::read_declared`, `read_definitions` (opens the target's document, asks `syntax_structural_ranges`, closes it), `line_colours`, `source_rows`.                                                                                                                                                                                                                                                                                                                                                                                      |
| `workbench/tests/proofs25_syntax_tree.rs`                      | 255   | End-to-end tests in a Light-mode project window (no language server).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `workbench/editor_language/tests/proofs25_quick_definition.rs` | 43    | The Quick Definition test (Full mode, fake server for `*.mock`, the target is an Arduino sketch: cpp grammar, no `[[language-servers]]` in `languages/arduino.toml`).                                                                                                                                                                                                                                                                                                                                                                                |

## (a) Hunks for existing files, by command, in the order to apply them

Order: 0.1 first. Then groups 1 to 4 in any order. Groups 5 and 6 land together with 0.2: `workbench/editor_structure.rs` imports `NO_CONTEXT_INFO` (5.1) and `call` (6.1), and its two functions are dead code under `-D warnings` until 5.2 and 6.3 call them. Group 9 only once groups 1, 2 and 3 are all in (each removes a user of `unparsed()`). 0.3 and 7.7 declare the end-to-end tests: apply them last, or the tests of a group not yet landed fail.

### Group 0: the service (needed by every other group)

**0.1 `editor/mod.rs`**: after

```rust
pub mod session;
```

add

```rust
pub mod structure;
```

**0.2 `workbench/mod.rs`**: after

```rust
mod editor_language;
```

add

```rust
mod editor_structure;
```

(Land 0.2 with groups 5 and 6, see the order above.)

**0.3 `workbench/mod.rs`, inside `mod tests`** (apply last): after

```rust
    mod proofs23_e2e_terminal_groups;
```

add

```rust
    mod proofs25_syntax_tree;
```

### Group 1: Move Statement Up / Down, Complete Current Statement (`code.moveStatementUp`, `code.moveStatementDown`, `code.completeStatement`, `ipc:syntax_structural_ranges`, `ipc:syntax_errors`)

**1.1 `editor/command_actions.rs`**: replace

```rust
    ("code.moveStatementUp", |editor, cx| move_statement(editor, true, cx)),
    ("code.moveStatementDown", |editor, cx| move_statement(editor, false, cx)),
```

by

```rust
    ("code.moveStatementUp", |editor, cx| super::structure::commands::move_statement(editor, true, cx)),
    ("code.moveStatementDown", |editor, cx| super::structure::commands::move_statement(editor, false, cx)),
```

**1.2 `editor/command_actions.rs`**: replace

```rust
    ("code.completeStatement", |editor, cx| {
        edit(editor, cx, |state, e| {
            let facts = &e.facts().language;
            editing::complete_statement(state, &facts.id, facts.comments.line.as_deref(), Some(&indent(e)))
        })
    }),
```

by

```rust
    ("code.completeStatement", super::structure::commands::complete_statement),
```

**1.3 `editor/command_actions.rs`**: delete

```rust
/// Move Statement: without a syntax tree, the lines move.
fn move_statement(editor: &mut Editor, up: bool, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, editor| editing::move_statement(state, &unparsed(), Some(&indent(editor)), up));
}

```

**1.4 `editor/command_actions.rs`, `mod tests`**: the commands of group 1 (and 2) now wait for the backend, so the case tables must wait for them. Replace

```rust
    fn run(cx: &mut VisualTestContext, id: &str) {
        cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        cx.run_until_parked();
    }
```

by

```rust
    fn run(cx: &mut VisualTestContext, id: &str) {
        cx.update(|window, cx| execute_action_or_throw(id, None, window, cx))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        cx.run_until_parked();
        // Move Statement and Complete Current Statement ask the backend's syntax tree first.
        settle(cx, |cx| cx.update(|_, cx| crate::editor::structure::is_idle(cx)));
    }
```

### Group 2: comments (`code.commentLine`, `code.commentBlock`; includes the import fix 9.4, which this group makes necessary)

**2.1 `editor/command_actions.rs`**: replace

```rust
    ("code.commentLine", comment_line),
    ("code.commentBlock", comment_block),
```

by

```rust
    ("code.commentLine", super::structure::commands::comment_line),
    ("code.commentBlock", super::structure::commands::comment_block),
```

**2.2 `editor/command_actions.rs`**: delete

```rust
fn comment_line(editor: &mut Editor, cx: &mut Context<Editor>) {
    let repeated = editor.memory.comment.as_ref() == Some(&editor.stamp());
    let done = editor.edit_with(EditKind::Other, cx, |editor| {
        let facts = editor.facts();
        let comments_at = |pos: u32| facts.comments_at(pos);
        comments::comment_line(&editor.edit_state(), &comments_at, facts.comment_style, &unparsed(), repeated)
    });
    if done {
        editor.memory.comment = Some(editor.stamp());
    }
}

fn comment_block(editor: &mut Editor, cx: &mut Context<Editor>) {
    edit(editor, cx, |state, editor| {
        let comments_at = |pos: u32| editor.facts().comments_at(pos);
        comments::comment_block(state, &comments_at, &unparsed())
    });
}

```

### Group 3: Toggle Case (`code.toggleCase`)

**3.1 `editor/command_actions.rs`**: replace

```rust
    ("code.toggleCase", |editor, cx| edit(editor, cx, |state, _| Some(editing::toggle_case(state, &unparsed())))),
```

by

```rust
    ("code.toggleCase", |editor, cx| edit(editor, cx, |state, e| Some(editing::toggle_case(state, &e.escape_tree())))),
```

### Group 4: Move Element (`code.moveElementLeft`, `code.moveElementRight`)

**4.1 `editor/command_actions.rs`**: replace

```rust
    ("code.moveElementLeft", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_left(state, &e.literal_tree()))
    }),
    ("code.moveElementRight", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_right(state, &e.literal_tree()))
    }),
```

by

```rust
    ("code.moveElementLeft", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_left(state, &e.code_tree()))
    }),
    ("code.moveElementRight", |editor, cx| {
        edit(editor, cx, |state, e| move_element::move_element_right(state, &e.code_tree()))
    }),
```

(`code_tree` is `literal_tree` plus the `Regexp` highlight spans, which `literal_tree` leaves out; for JavaScript the regex is `@string.special` and already in `literal_tree`, so this only matters for grammars capturing `@string.regex(p)`.)

### Group 5: Context Info (`editor.contextInfo`)

**5.1 `workbench/editor_extras.rs`**: replace

```rust
const NO_CONTEXT_INFO: &str = "No context information here";
```

by

```rust
pub(super) const NO_CONTEXT_INFO: &str = "No context information here";
```

**5.2 `workbench/editor_extras.rs`, `show_context_info`**: after

```rust
                .map(|line| editor.line_text(line).trim_end().to_owned())
                .collect();
            lines.join("\n")
        };
```

add

```rust
        // Without the backend's fold ranges the blocks come from the syntax tree.
        if text.is_empty() && !editor.read(cx).has_backend_folds() && self.context_info_from_tree(&editor, max, cx) {
            return;
        }
```

### Group 6: Highlight Usages in File (`find.highlightUsages`, `ipc:syntax_locals` already done)

**6.1 `workbench/editor_language.rs`**: replace

```rust
async fn call<T, F>(
```

by

```rust
pub(super) async fn call<T, F>(
```

**6.2 `workbench/editor_language.rs`**: replace

```rust
use crate::editor_lsp::navigation::usages_status_text;
```

by

```rust
use crate::editor_lsp::navigation::{SYNTAX_NOTE, SYNTAX_NOTE_TIMEOUT_MS, usages_status_text};
```

**6.3 `workbench/editor_language.rs`, `highlight_usages`**: replace

```rust
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_document_highlights(&host, &label, doc, pos).await
    })
    .await?;
    this.update(cx, |workbench, cx| {
```

by

```rust
    let answer = call(this, cx, move |host, label| async move {
        ide_host::ops::lsp::lsp_document_highlights(&host, &label, doc, pos).await
    })
    .await?;
    // Without a usable server, the occurrences of a certain local in its scope stand in.
    let (answer, from_syntax) = match answer {
        Err(error) if !is_cancelled(&error) => match super::editor_structure::local_usages(this, editor, doc, pos, cx).await? {
            Some(ranges) => (Ok(ranges), true),
            None => (Err(error), false),
        },
        answer => (answer.map(|highlights| highlights.into_iter().map(|highlight| highlight.range).collect()), false),
    };
    this.update(cx, |workbench, cx| {
```

(`rustfmt` will break the long `match` line.)

**6.4 `workbench/editor_language.rs`, same function**: replace

```rust
        let highlights = match answer {
            Ok(highlights) => highlights,
            Err(error) => {
                report(editor, &error, cx);
                return;
            }
        };
        let (marks, name) = {
            let rope = editor.read(cx).rope();
            let marks: Vec<Mark> = highlights
                .iter()
                .map(|highlight| range_to_offsets(rope, highlight.range))
```

by

```rust
        let ranges: Vec<ide_core::Range> = match answer {
            Ok(ranges) => ranges,
            Err(error) => {
                report(editor, &error, cx);
                return;
            }
        };
        if from_syntax {
            hint(editor, SYNTAX_NOTE, None, Tone::Info, Some(SYNTAX_NOTE_TIMEOUT_MS), cx);
        }
        let (marks, name) = {
            let rope = editor.read(cx).rope();
            let marks: Vec<Mark> = ranges
                .iter()
                .map(|range| range_to_offsets(rope, *range))
```

### Group 7: Quick Definition / Quick Type Definition (`code.quickDefinition`, `code.quickTypeDefinition`)

**7.1 `editor_lsp/mod.rs`**: before

```rust
pub mod definition_view;
```

add

```rust
pub mod definition_source;
```

**7.2 `editor_lsp/definition_view.rs`**: replace

```rust
use super::files::target_label;
```

by

```rust
use super::definition_source::source_rows;
use super::files::target_label;
use crate::editor::view::HighlightSpan;
```

**7.3 `editor_lsp/definition_view.rs`**: replace

```rust
    /// The 0-based line of the first line shown.
    pub first_line: u32,
    pub lines: Vec<String>,
}
```

by

```rust
    /// The 0-based line of the first line shown.
    pub first_line: u32,
    pub lines: Vec<String>,
    /// The highlight spans of the lines shown, joined by line breaks (UTF-16 offsets).
    pub spans: Vec<HighlightSpan>,
}
```

**7.4 `editor_lsp/definition_view.rs`**: replace

```rust
        Some(Self { location, name, first_line: first, lines: shown })
```

by

```rust
        Some(Self { location, name, first_line: first, lines: shown, spans: Vec::new() })
```

**7.5 `editor_lsp/definition_view.rs`, `render`**: replace

```rust
            .font_family(mono.family)
            .text_size(theme::length(cx, "fs-code"))
            .children(self.lines().into_iter().map(|line| div().whitespace_nowrap().child(SharedString::from(line))));
```

by

```rust
            .font_family(mono.family.clone())
            .text_size(theme::length(cx, "fs-code"))
            .children(self.targets.get(self.index).map(|target| source_rows(target, &mono, cx)).unwrap_or_default());
```

**7.6 `workbench/editor_language.rs`, `quick_definition`**: replace

```rust
    let read = call(this, cx, move |host, label| async move {
        let project = host.project(&label)?;
        Ok(locations
            .into_iter()
            .filter_map(|location| {
                let text = project.text_of(&location.path.0).ok()?;
                DefinitionTarget::read(location, &text)
            })
            .collect::<Vec<_>>())
    })
    .await?;
```

by

```rust
    let read = call(this, cx, move |host, label| async move {
        crate::editor_lsp::definition_source::read_definitions(&host, &label, locations).await
    })
    .await?;
```

**7.7 `workbench/editor_language.rs`, `mod tests`**: after

```rust
    mod proofs20_mcp_problems;
```

add

```rust
    mod proofs25_quick_definition;
```

### Group 9: clean-up of `editor/command_actions.rs` (after groups 1, 2 and 3)

**9.1** replace the module comment's second paragraph

```rust
//! The backend has no syntax tree of the document. The commands that read one get the tree of
//! the document's comments and string literals ([`Editor::literal_tree`]) where they only ask
//! whether a position is code, and the unparsed tree otherwise, which makes them take their
//! text-only path. The comment commands get the unparsed tree: a block comment holding its own
//! closing token takes a parser to confirm, and the highlights cannot tell two adjacent comments
//! from one.
```

by

```rust
//! The commands that only ask whether a position is code or an escape read the trees of the
//! current highlights ([`Editor::code_tree`], [`Editor::escape_tree`]). Move Statement, Complete
//! Current Statement and the comment commands over a block comment holding its own closing token
//! ask the backend's syntax tree ([`super::structure::commands`]).
```

**9.2** delete

```rust
/// The tree of a document the backend has not parsed: the commands take their text-only path.
fn unparsed() -> SyntaxTree {
    SyntaxTree::default()
}

```

**9.3** delete

```rust
use crate::editor_commands::tree::SyntaxTree;
```

**9.4** (with group 2: `comments` has no other user in this file) replace

```rust
    Edit, EditState, IndentContext, Selection, caret_commands, comments, lines, move_element,
```

by

```rust
    Edit, EditState, IndentContext, Selection, caret_commands, lines, move_element,
```

(Hypothesis: after 9.4 `rustfmt` joins the `use crate::editor_commands::{…}` list onto one line.)

## (b) Parity rows

| Key                                          | Expected                | Proving test (full name)                                                                                                                                                                                                                                                                  | Missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| -------------------------------------------- | ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:code.commentBlock`                   | done                    | `workbench::tests::proofs25_syntax_tree::a_block_comment_holding_its_closing_token_comes_out_when_the_tree_confirms_it`, `editor::structure::proofs25_structure::a_comment_holding_its_closing_token_comes_out_once_the_tree_confirms_its_span`                                           |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.commentLine`                    | done                    | `workbench::tests::proofs25_syntax_tree::line_comment_without_line_comments_takes_out_a_nested_block_comment_the_tree_confirms`, `editor::command_actions::tests::line_and_comment_commands_run_through_the_registry`                                                                     |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.completeStatement`              | done                    | `workbench::tests::proofs25_syntax_tree::complete_statement_closes_what_the_syntax_errors_say_is_left_open`, `workbench::tests::proofs25_syntax_tree::complete_statement_uses_the_text_rules_where_the_editing_mode_parses_the_document`                                                  | the lookup-acceptance step ("with the completion lookup open its item is inserted first", `src/editor/core/actions.ts:336`) was not read natively; Hypothesis: it is a separate row                                                                                                                                                                                                                                                                                                                                                                      |
| `action:code.moveStatementUp`                | done                    | `workbench::tests::proofs25_syntax_tree::move_statement_moves_a_whole_statement_and_stays_inside_its_block`, `workbench::tests::proofs25_syntax_tree::move_statement_on_a_blank_line_moves_the_line`                                                                                      |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.moveStatementDown`              | done                    | same two tests                                                                                                                                                                                                                                                                            |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.moveElementLeft`                | done                    | `workbench::tests::proofs25_syntax_tree::move_element_skips_a_bracket_inside_a_string`, `editor::structure::proofs25_structure::move_element_skips_the_brackets_of_a_string_the_code_tree_marks`                                                                                          |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.moveElementRight`               | done                    | `editor::command_actions::tests::caret_and_occurrence_commands_run_through_the_registry`, `editor::structure::proofs25_structure::the_code_tree_marks_literals_comments_and_regular_expressions_as_not_code`                                                                              | Right shares `element_swap` and the tree with Left; no Right-specific tree case was added                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `action:code.toggleCase`                     | done                    | `workbench::tests::proofs25_syntax_tree::toggle_case_keeps_the_escapes_of_a_string`, `editor::structure::proofs25_structure::toggle_case_keeps_the_escapes_the_highlights_mark`                                                                                                           |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.quickDefinition`                | done                    | `workbench::editor_language::tests::proofs25_quick_definition::quick_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour`                                                                                                                                               |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:code.quickTypeDefinition`            | partial                 | `workbench::editor_language::tests::proofs5_editor::quick_type_definition_shows_the_type_definitions_lines_and_f4_opens_it`                                                                                                                                                               | shares `read_definitions` with Quick Definition, but no test runs it on a target with a grammar                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:editor.contextInfo`                  | done                    | `workbench::tests::proofs25_syntax_tree::context_info_without_fold_ranges_shows_the_header_lines_of_the_syntax_trees_blocks`, `editor::structure::proofs25_structure::enclosing_headers_are_the_lines_of_the_blocks_around_the_line`                                                      |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:find.highlightUsages`                | done                    | `workbench::tests::proofs25_syntax_tree::highlight_usages_without_a_server_marks_the_locals_occurrences_from_the_syntax_tree`                                                                                                                                                             |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `action:navigate.declaration`                | partial (unchanged)     | existing proofs                                                                                                                                                                                                                                                                           | the webview's non-local fallback (`src/editor/lsp/syntaxFallback.ts`: `usageRole` from `syntax_node_at`, then the project index's symbols filtered by role and file family) is not built: it needs the symbol index, not only the tree                                                                                                                                                                                                                                                                                                                   |
| `unit:src/editor/core/cm/structural.test.ts` | done, 27 of 27          | the 23 in `editor_commands::structural::tests` plus `editor::structure::proofs25_structure::{is_false_for_stream_modes_and_documents_without_a_language, is_true_for_a_lezer_grammar, asks_the_backend_once_per_position_and_document_version, answers_null_when_the_service_fails}`      |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `ipc:syntax_structural_ranges`               | done                    | `workbench::tests::proofs25_syntax_tree::move_statement_moves_a_whole_statement_and_stays_inside_its_block` (a three-line `if` moves as one, only the statement ranges say so), `…proofs25_quick_definition::quick_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour` |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `ipc:syntax_errors`                          | done                    | `workbench::tests::proofs25_syntax_tree::complete_statement_closes_what_the_syntax_errors_say_is_left_open` (the text rules alone give `foo(1, bar(2;`)                                                                                                                                   |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `ipc:syntax_node_at`                         | done (already)          | adds `…a_block_comment_holding_its_closing_token_comes_out_when_the_tree_confirms_it`, `…context_info_without_fold_ranges_…`                                                                                                                                                              |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `ipc:syntax_expression_at`                   | not proven by this work |                                                                                                                                                                                                                                                                                           | called natively by `workbench/refactor_actions.rs` (`MenuStep::ExpressionAt`, `FactRequest::ExpressionAt`); read: every Refactor This test there runs on `*.mock` files with the fake server, a language without a grammar, so none reaches it. None of the commands of this brief calls it in the webview. Hypothesis: a Light-mode `refactor.introduceVariable` test on a JavaScript expression proves it; not written, because the Introduce fallback's flow (`editor_refactor/refactor_this.rs`, `editor_refactor/server_introduce.rs`) was not read |

## (c) Per command: the webview with and without a tree, and what was built

- **Move Statement** (`src/editor/core/actions.ts:315-327`). With a Lezer tree, a multi-range or a non-empty selection: the Lezer planner (`editing.ts` `moveStatementUp/Down`). Otherwise queued (`queueEdit`, `:303-311`): `structure.support(language).tree` (`:294-300`), then `moveStatementStructural` (`structural.ts:427-449`: ranges at the caret line's start, errors only when a node `hasError`, `planMoveStatement`, stale → nothing); `fallback` or no tree → `moveLineUp/Down`. Native: there is no Lezer tree (the commands' tree model `editor_commands/tree.rs` has no producer but the highlights, and the backend gives node chains and ranges, never a whole tree), so the Lezer planner `editing::move_statement` cannot run on one; every single-caret press in a language with a grammar takes the tree-sitter path, also in the languages where the webview uses its Lezer planner (Rust, Java, JavaScript, Python, Go…). This is a deliberate difference: Hypothesis: both planners move the same statement in ordinary code; they can differ at the edges (Lezer's guard and leading-comment rules in `editing.ts` versus `lineSpan` / `reliableBounds` in `structural.ts`). Multi-range or selection keeps `editing::move_statement` with the empty tree (= line move, read in `editing.rs:802-811`). Built: the queue, the cache, `move_turn` using the existing planner `editor_commands::structural::plan_move_statement`.
- **Complete Current Statement** (`actions.ts:335-352`). A document whose mode has a Lezer tree, unless it is Java, or several ranges: text rules only. Otherwise queued: `structure.support(language).tree`, then `completeStatementStructural` (`structural.ts:451-462`: ranges and errors at the caret, both null → fallback; `planCompleteStatement`), else text rules; read-only → nothing. Native: several carets → text rules at once; otherwise queued, and the first turn asks `syntax_support` and `doc_ui_mode` (the same IPC commands; the mode decides `parses_itself`, the native counterpart of `hasLezerTree`): a mode in `PARSED_MODES` and not Java → text rules; no grammar → text rules; else `complete_statement_structural`. `PARSED_MODES` is the list of `@codemirror/language-data` entries that load no legacy (stream) mode, measured with a script over `node_modules/@codemirror/language-data/dist/index.js`, plus `Angular TypeScript` (`src/editor/core/cm/angular/inline.ts` configures the Lezer TypeScript grammar). `src/editor/core/cm/languages.ts:78-160` turns Kotlin, Scala, Dart, Groovy, C#, Shell, CMake and GoMod into stream modes; they are not in the list.
- **Comment with Block / Line Comment** (`comments.ts:300-340`). The Lezer tree confirms a comment holding its closing token (`resolveInner(comment.open, 1)`, climbs to a `/comment/i` node, compares its span); without a tree it is refused. Native: the text path runs first; only when it answers "blocked" (no change, same selection) and the language has a grammar is `syntax_node_at` asked at the first code character of each range, the chains merged into a `SyntaxTree` (`chain_tree`), and the command run again with it.
- **Toggle Case** (`editing.ts:754-797`). Escapes are Lezer nodes named `/Escape/`; without a tree none. Native: the escapes are the `Escape` highlight spans of the current text (`escape_tree`; the Rust highlights query has `(escape_sequence) @escape`, read in tree-sitter-rust 0.24.2 `queries/highlights.scm:154`); stale highlights → none, as an unparsed tree.
- **Move Element** (`moveElement.ts:16-19`). The tree only decides what is code (`String|Comment|Regexp|Char|Template`); the elements come from a text scan. Native already used `literal_tree` the same way; the one difference is `Regexp` highlight spans, now in `code_tree`. Test only plus a one-line hunk.
- **Quick Definition** (`hover.ts:301-345`). `declarationRange` opens the target's document (`withDocument`, `files.ts:45-54`), asks `structuralRanges(doc, loc.range.start).declaration`, `definitionSpan` shows it whole (≤ 400 lines), the popup colours the text by its language. Without a tree: a window around the name. Native: `read_definitions` does the same off the window's thread (open, `syntax_structural_ranges`, close on every path), the spans of the shown text come from `Host::syntax().highlights`, `source_rows` draws them in the code font.
- **Context Info** (`actions.ts:358-364`, `stickyScroll.ts:20-49`). Backend folds of kind region around the line; without folds, the Lezer ancestors at the line start with `foldNodeProp` or a name matching `Body|Block|ClassBody|Object|Array`. Native: folds as before; without them, `syntax_node_at` at `(line, 0)` and the ancestors whose kind matches the same pattern or which their own brackets delimit (tree-sitter has no `foldNodeProp`; the backend folds every bracket-delimited node, `crates/ide-language/src/syntax/folding.rs:1-2`).
- **Highlight Usages** (`navigation.ts:802-863`). On any non-cancelled server error: `structure.support(language).locals`, then `structure.locals` at the caret; a certain local → definition and references marked, hint `SYNTAX_NOTE` for 2.5 s; else the error. Native: the same in `local_usages` plus hunk 6.3/6.4.
- **Go to Declaration**: see the row; nothing built.

## (d) Hypotheses (not settled by reading), and how to settle them

1. Hypothesis: the code compiles after small fixes. Settle: `cargo build -p openidea_gpui`, then clippy.
2. Hypothesis: `apply(editor, version, …)` and the other helpers in `commands.rs` borrow-check as written (a `&mut Context<Editor>` global borrow ends before `edit_with`). Settle: compile.
3. Hypothesis: GPUI's test executor does not fire `background_executor().timer(…)` timers during `run_until_parked` without `advance_clock`, so in `workbench/tests` the `settle` helper (which does not move the clock) leaves the fold request (`editor/core_actions.rs` `FOLD_DELAY`) unanswered. The Context Info test relies on it and asserts `!has_backend_folds()` first. Settle: run the test; if folds arrive, the first assertion names it.
4. Hypothesis: `ide_host::ops::lsp::lsp_document_highlights` in Light mode answers `Err` (kind `Unavailable`), not `Ok([])`. Settle: run the Highlight Usages test.
5. Hypothesis: tree-sitter-kotlin-ng gives the `if (…) {…}` inside a block as a direct child of `block` (a "statement" for `structural_ranges`). Settle: run the Move Statement test; or a `crates/ide-syntax` probe like `structural_ranges_statements_and_declarations`.
6. Hypothesis: tree-sitter-c-sharp reports an error touching line 0 for a top-level `foo(1, bar(2` (ERROR or two MISSING `)`). Either gives `foo(1, bar(2));` through `unclosed_brackets`. Settle: run the test.
7. Read, not run: tree-sitter-rust 0.24.2's scanner tracks nested block comment depth (`src/scanner.c` `process_block_comment`) and tree-sitter-ocaml 0.26.0's `scan_comment` recurses. Hypothesis: `syntax_node_at` at offset 0 of `/* a /* b */ c */` returns the `block_comment` (tree-sitter OCaml: `comment`). Settle: run the two comment tests.
8. Hypothesis: `structural_ranges` gives a C++ `function_definition` at file level as `declaration` for the Arduino sketch. Settle: run the Quick Definition test.
9. Hypothesis: the C++ highlights colour `void` (`primitive_type` → `@type.builtin` → `syn-builtin`) differently from `fg-editor`. Settle: run the test.
10. Hypothesis: `execute_action_or_throw` checks the actions' `when` (editor focus); the tests focus the editor in an active window first.
11. The comment commands enter the queue only when the text path is blocked; a blocked text answer is recognised as "an edit without changes whose selection is the current one" (read in `comments.rs`: `Uncomment::Blocked` is the only such answer). Hypothesis: `comment_line`'s line path never answers that way. Settle: `line_and_comment_commands_run_through_the_registry`.
12. Hypothesis: after hunk 1.4 the existing `LINE_CASES` still pass on `main.rs`: `ab\ncd` gives no usable statement (fallback = line move) and `let a = 1` gets a `MISSING ;` (same result as the text rules). Settle: run `editor::command_actions::tests::line_and_comment_commands_run_through_the_registry`.
13. Hypothesis: the spec's `synced` count (`structural.test.ts:124`, 7 calls) has no native counterpart: native edits reach the backend document synchronously (`Editor::edit_with` → `send`, read in `editor/view.rs:1022-1055`), so the editor's version is the backend's and the cache key is that version.
14. A failed `syntax_support` is not cached (the press goes on without a tree, the next one asks again), as `src/editor/structure/index.ts:69-80` deletes a failed support promise. The webview caches support per language for the process; natively per document. All structure queries pass `request: None`, as the webview's `structure.*` do without a `signal` (read: `index.ts:57-67`; `structural.ts` and `navigation.ts` pass none).
15. Hypothesis: the OWN_MODES ERB, JSP and Astro (mixed Lezer parsers) are missing from `PARSED_MODES`; their languages have no tree-sitter grammar, so the gate never matters for them.
16. The test name `is_true_for_a_lezer_grammar` keeps the spec's sentence as the brief requires, which conflicts with the rule against naming the other UI's libraries in test names; rename only if the parity checker allows.

## (e) Tests most likely to fail first

- `context_info_without_fold_ranges_…`: depends on hypothesis 3 (timers without clock).
- `move_statement_moves_a_whole_statement_and_stays_inside_its_block`: Kotlin node shapes (hypothesis 5) and the caret columns after each move.
- `complete_statement_closes_what_the_syntax_errors_say_is_left_open`: error placement in tree-sitter-c-sharp for a top-level `foo(1, bar(2` (hypothesis 6, now for C#).
- `quick_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour`: the cpp declaration kind and the colour of `void` (hypotheses 8, 9).
- `editor::command_actions::tests::line_and_comment_commands_run_through_the_registry`: now waits on the backend for the statement cases (hypothesis 12); its `settle` is the 3-second one.

## (f) Kotlin highlighting: why `val s = "x"` gives one Variable span

Read, not run:

- `crates/ide-language/src/syntax/mod.rs` `highlight_query`: the query is `self.spec.highlights.as_ref().or(native.as_ref())`: the grammar spec's own query wins over the native one.
- `crates/ide-language/src/syntax/grammars.rs`: the Kotlin spec has `highlights: Some(Highlights { query: highlights!("kotlin"), … })`, and `native_highlights` has no `"kotlin"` arm (it returns `None`).
- `crates/ide-language/queries/kotlin/highlights.scm` captures only declaration roles (types, function definitions, parameters, `(property_declaration (variable_declaration (identifier) @variable))`, properties, calls, `class_modifier`, block comments, escapes): it is written to supplement the webview's stream lexer (`languages/kotlin.toml` `ui-mode = "Kotlin"`), which colours keywords, strings, numbers and line comments. The native editor has no stream lexer.
- tree-sitter-kotlin-ng 1.1.0 ships no highlights query (`bindings/rust/lib.rs` exports only `LANGUAGE` and `NODE_TYPES`; no `queries/` folder).

So for `val s = "x"` only the `@variable` pattern matches: one Variable span, no keyword (`val`), no string.

Smallest fix (in `crates/ide-language`, not applied): a native base query for Kotlin, used only by the native UI so the webview keeps its stream lexer plus the supplement:

1. a new `crates/ide-language/queries/kotlin/highlights_native.scm` with the keywords as anonymous tokens (`["val" "var" "fun" "class" "object" "interface" "if" "else" "when" "for" "while" "do" "return" "throw" "try" "catch" "finally" "import" "package" "typealias" "in" "is" "as" "this" "super" "by" "where" "init" "constructor" "companion" "override" "private" "protected" "public" "internal" "abstract" "open" "final" "sealed" "data" "enum" "inner" "inline" "suspend" "lateinit" "const" "vararg"] @keyword`), `(string_literal) @string`, `(multiline_string_literal) @string`, `(character_literal) @character`, `(number_literal) @number`, `(float_literal) @number`, `(line_comment) @comment` (node names read in tree-sitter-kotlin-ng 1.1.0 `src/node-types.json`);
2. in `grammars.rs` `native_highlights`, an arm `"kotlin" => joined(&KOTLIN, &[include_str!("../../queries/kotlin/highlights_native.scm"), highlights!("kotlin")])` (base first: later patterns win, so the supplement's roles override);
3. in `mod.rs` `highlight_query`, prefer the native query when there is one: `native.as_ref().or(self.spec.highlights.as_ref())`.

Step 3 changes no other language: read in `grammars.rs`, the specs with their own query are kotlin, zig, elixir, hcl, nix, graphql, make and gdscript, and `native_highlights` has an arm for none of them (only Kotlin after step 2). Hypothesis: adding these captures to the shared `kotlin/highlights.scm` instead would double-colour the webview (stream lexer plus query); keeping them in a native-only file avoids that. Settle: the webview test `src/editor/core/cm/syntaxHighlight.test.ts` and `crates/ide-language` `kotlin_supplements_stream_highlighting_with_declaration_roles`.
