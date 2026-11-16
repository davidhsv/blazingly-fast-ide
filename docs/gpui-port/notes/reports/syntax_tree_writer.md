I wrote the editor-side structure service and the structural parts of every command in the brief, as new files only. Nothing was compiled or run: I ran no cargo, no git, edited no existing file and formatted nothing. Statements marked "Hypothesis:" were not checked against the code.

## Files written (exact `wc -l`)

All under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`:

- `editor/structure.rs` (355): the service. It holds a cache per document and version, a command queue per editor, the support/editing-mode check, the trees built from backend answers and from highlights, and the Context Info header lines.
- `editor/structure/commands.rs` (418): Move Statement, Complete Current Statement and Comment Line/Block. Backend questions run off the window's thread, presses run in order, and an answer that arrives after the text or selection moved is dropped.
- `editor/structure/proofs25_structure.rs` (260): plain unit tests.
- `workbench/editor_structure.rs` (118): the Context Info fallback and the Highlight Usages fallback.
- `editor_lsp/definition_source.rs` (147): Quick Definition reads the whole declaration and draws it with syntax colours.
- `workbench/tests/proofs25_syntax_tree.rs` (255): 10 end-to-end tests in Light mode, with no language server.
- `workbench/editor_language/tests/proofs25_quick_definition.rs` (43): uses the existing fake server for `.mock` files; the target is an `.ino` file, which has a grammar and no language server.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/syntax_tree_integration.md` (576). Every anchor in its 27 numbered hunks plus the 4 clean-up anchors was grep-checked to match exactly once. They are grouped by command, with the landing order; the test declarations 0.3 and 7.7 go last.

## Rows (expected after the hunks)

| Key                                                            | State              | Proving test                                                                                                                                                                                                                                           |
| -------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:code.commentBlock`                                     | done               | `workbench::tests::proofs25_syntax_tree::a_block_comment_holding_its_closing_token_comes_out_when_the_tree_confirms_it`                                                                                                                                |
| `action:code.commentLine`                                      | done               | `…::line_comment_without_line_comments_takes_out_a_nested_block_comment_the_tree_confirms` (OCaml)                                                                                                                                                     |
| `action:code.completeStatement`                                | done               | `…::complete_statement_closes_what_the_syntax_errors_say_is_left_open` (C#), `…::complete_statement_uses_the_text_rules_where_the_editing_mode_parses_the_document` (JS)                                                                               |
| `action:code.moveStatementUp`, `action:code.moveStatementDown` | done               | `…::move_statement_moves_a_whole_statement_and_stays_inside_its_block`, `…::move_statement_on_a_blank_line_moves_the_line`                                                                                                                             |
| `action:code.moveElementLeft`                                  | done               | `…::move_element_skips_a_bracket_inside_a_string`                                                                                                                                                                                                      |
| `action:code.moveElementRight`                                 | done               | existing `editor::command_actions::tests::caret_and_occurrence_commands_run_through_the_registry`, plus `editor::structure::proofs25_structure::the_code_tree_marks_literals_comments_and_regular_expressions_as_not_code`                             |
| `action:code.toggleCase`                                       | done               | `…::toggle_case_keeps_the_escapes_of_a_string`                                                                                                                                                                                                         |
| `action:code.quickDefinition`                                  | done               | `workbench::editor_language::tests::proofs25_quick_definition::quick_definition_shows_the_whole_declaration_from_the_syntax_tree_in_colour`                                                                                                            |
| `action:code.quickTypeDefinition`                              | partial            | It uses the same new code, but no test runs it on a file with a grammar.                                                                                                                                                                               |
| `action:editor.contextInfo`                                    | done               | `…::context_info_without_fold_ranges_shows_the_header_lines_of_the_syntax_trees_blocks`                                                                                                                                                                |
| `action:find.highlightUsages`                                  | done               | `…::highlight_usages_without_a_server_marks_the_locals_occurrences_from_the_syntax_tree`                                                                                                                                                               |
| `action:navigate.declaration`                                  | partial, unchanged | The webview's fallback for names that are not local (node role plus the project's symbol index, `syntaxFallback.ts`) is not built.                                                                                                                     |
| `unit:src/editor/core/cm/structural.test.ts`                   | done, 27 of 27     | The 23 existing, plus `editor::structure::proofs25_structure::{is_false_for_stream_modes_and_documents_without_a_language, is_true_for_a_lezer_grammar, asks_the_backend_once_per_position_and_document_version, answers_null_when_the_service_fails}` |
| `ipc:syntax_structural_ranges`                                 | done               | the Move Statement test and the Quick Definition test                                                                                                                                                                                                  |
| `ipc:syntax_errors`                                            | done               | the C# Complete Statement test                                                                                                                                                                                                                         |
| `ipc:syntax_expression_at`                                     | not proven         | Only `refactor_actions.rs` calls it, and every test there uses `.mock` files, which have no grammar.                                                                                                                                                   |

## Left out, and why

- **Move Statement differs from the webview on purpose.** For languages the webview parses itself (Rust, Java, JS, Python…) it uses its own planner over its own syntax tree. The native app has no such tree, and the backend returns node chains and ranges, never a whole tree. So every single-caret press in a language with a grammar uses the backend's ranges. Hypothesis: both planners move the same statement in ordinary code.
- **`ipc:syntax_expression_at`:** the brief lists this row but none of its commands calls it. I did not read the Introduce Variable fallback flow, so I wrote no test.
- **`navigate.declaration`:** the missing part needs the project's symbol index, not only the tree.
- **Highlight Usages read/write kinds:** native marks have one kind, so the webview's distinction between writes and plain occurrences is not drawn.

## Hypotheses (how to settle each is in the notes, section d)

- Hypothesis: it compiles after small fixes; the most likely trouble is borrows in `commands.rs`.
- Hypothesis: GPUI test timers do not fire without `advance_clock`. The Context Info test depends on this to have no fold ranges yet; it asserts that first, so a failure names the cause.
- Hypothesis: in Light mode, `lsp_document_highlights` returns an error rather than an empty list.
- Hypothesis: the tree-sitter grammars give the nodes and errors the tests expect: Kotlin statements, the C# error placement, the C++ function as the declaration, and `void` coloured differently from plain text. I did confirm by reading that the Rust and OCaml scanners nest block comments.
- Hypothesis: the existing case table in `command_actions` tests still passes once hunk 1.4 makes it wait for the service; that wait uses the existing 3-second helper.
- The list of editing modes that parse the document themselves was measured with a script over `node_modules/@codemirror/language-data`. Hypothesis: the missing ERB, JSP and Astro modes never matter, since those languages have no grammar.

## Kotlin finding (by reading only)

`highlight_query` in `crates/ide-language/src/syntax/mod.rs` uses the grammar spec's own query before the native one. Kotlin's spec query, `queries/kotlin/highlights.scm`, only covers declaration roles: it is meant to add to the webview's token-stream colouring. `native_highlights` has no Kotlin entry, and tree-sitter-kotlin-ng 1.1.0 ships no highlights query. So in `val s = "x"` only `@variable` matches, and `val` and the string get no colour.

Smallest fix (not applied):

- Add a native-only `queries/kotlin/highlights_native.scm` with keywords, strings, numbers and line comments.
- Add a `"kotlin"` entry in `native_highlights` that joins it before the existing query.
- Make `highlight_query` prefer the native query.

By reading, the last step changes no other language: none of the other specs with their own query (zig, elixir, hcl, nix, graphql, make, gdscript) has a native entry.

## What turned out wrong in the instructions or the existing code

- **Complete Statement:** the webview asks the backend first only for Java and for languages its editor does not parse itself. For JS, Rust and the others it uses the text rules alone (`actions.ts:340`). I gated the native command the same way; it reads the editing mode through `doc_ui_mode`.
- **Move Element row:** its proof text implies the webview finds elements from the tree. It doesn't: elements come from a text scan, and the tree only decides what is code. The native command already matched; the only gap was regular-expression literals, now covered by a one-line hunk.
- **Existing native test:** `editor_commands::structural::tests::drops_the_edit_when_the_document_changed_meanwhile_and_reports_fallback_without_ranges` does not test dropping the edit. Stale answers are now the service's job; only the cache side has a unit test (`an_answer_for_a_version_the_document_has_left_is_dropped`).
- **`editor/command_actions.rs` module comment:** it says the backend has no syntax tree of the document. Hunk 9.1 replaces it.
- **Spec case name:** `is_true_for_a_lezer_grammar` is kept as the brief requires, which conflicts with the rule against naming the other UI's libraries in test names.
- **Spec's `synced` count:** the 7 sync calls in the spec have no native counterpart. Native edits reach the backend document synchronously, so the editor's version is the backend's.
