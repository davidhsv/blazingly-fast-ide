//! The editor features that read the document's syntax tree, run through the registry in a
//! project window without a language server (Light mode): the backend's tree-sitter structure
//! answers them.

use super::*;
use crate::editor::view::Editor;
use gpui_kit::Focusable as _;

/// A trusted project of one file `path` holding `text`, open in the editor with the keyboard.
struct Opened {
    _home: tempfile::TempDir,
    _folder: tempfile::TempDir,
    editor: Entity<Editor>,
}

fn open_one<'a>(
    cx: &'a mut TestAppContext,
    path: &str,
    text: &str,
) -> (Opened, Entity<Workbench>, &'a mut VisualTestContext) {
    let home = start(cx);
    let (folder, root, workbench, cx) = trusted_project(cx, &[(path, text)]);
    cx.update(|window, _| window.activate_window());
    let file = root.join(path);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    settle(cx, |cx| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    (Opened { _home: home, _folder: folder, editor }, workbench, cx)
}

impl Opened {
    fn text(&self, cx: &mut VisualTestContext) -> String {
        self.editor.read_with(cx, |editor, _| editor.text())
    }

    fn caret(&self, cx: &mut VisualTestContext) -> (u32, u32) {
        self.editor.read_with(cx, |editor, _| editor.caret_position())
    }

    fn select(&self, cx: &mut VisualTestContext, anchor: u32, head: u32) {
        self.editor.update(cx, |editor, cx| editor.select(anchor, head, cx));
        cx.run_until_parked();
    }

    fn caret_to(&self, cx: &mut VisualTestContext, line: u32, col: u32) {
        let offset = self.editor.read_with(cx, |editor, _| editor.offset_of(line, col));
        self.select(cx, offset, offset);
    }

    /// Waits until the structure commands waiting for the backend have run.
    #[track_caller]
    fn idle(&self, cx: &mut VisualTestContext) {
        wait_long(cx, "the structure commands", |cx| cx.update(|_, cx| crate::editor::structure::is_idle(cx)));
    }

    #[track_caller]
    fn wait_text(&self, cx: &mut VisualTestContext, text: &str) {
        wait_long(cx, &format!("the text {text:?}"), |cx| self.text(cx) == text);
    }

    /// Waits for the highlights of the current text, which mark its literals and escapes.
    #[track_caller]
    fn highlighted(&self, cx: &mut VisualTestContext) {
        wait_long(cx, "the highlights", |cx| self.editor.read_with(cx, |editor, _| !editor.code_tree().is_empty()));
    }
}

const STATEMENTS: &str = "class A {
  fun f() {
    val a = 1
    if (a > 0) {
      println(a)
    }
    val b = 2
  }
}
";

/// Move Statement Down moves the whole statement at the caret over the next one, however many
/// lines that one has, and does nothing at the last statement of its block instead of leaving
/// it; two quick presses of Move Statement Up move it twice. The statements come from the
/// backend's ranges (`syntax_structural_ranges`): moving lines would split the `if`.
#[gpui_kit::test]
async fn move_statement_moves_a_whole_statement_and_stays_inside_its_block(cx: &mut TestAppContext) {
    let (f, _workbench, cx) = open_one(cx, "src/A.kt", STATEMENTS);
    f.caret_to(cx, 2, 4);
    run_action(cx, "code.moveStatementDown", None);
    let over_if = STATEMENTS.replace(
        "    val a = 1\n    if (a > 0) {\n      println(a)\n    }\n",
        "    if (a > 0) {\n      println(a)\n    }\n    val a = 1\n",
    );
    f.wait_text(cx, &over_if);
    assert_eq!(f.caret(cx), (5, 4), "the caret moves with the statement");
    run_action(cx, "code.moveStatementDown", None);
    let last = over_if.replace("    val a = 1\n    val b = 2\n", "    val b = 2\n    val a = 1\n");
    f.wait_text(cx, &last);
    // The last statement of the block stays in it.
    run_action(cx, "code.moveStatementDown", None);
    f.idle(cx);
    assert_eq!(f.text(cx), last);
    assert_eq!(f.caret(cx), (6, 4));
    // Two presses before the backend answers: both run, in order.
    run_action(cx, "code.moveStatementUp", None);
    run_action(cx, "code.moveStatementUp", None);
    f.idle(cx);
    assert_eq!(f.text(cx), STATEMENTS);
    assert_eq!(f.caret(cx), (2, 4));
}

/// Move Statement on a blank line moves the line, as without a syntax tree.
#[gpui_kit::test]
async fn move_statement_on_a_blank_line_moves_the_line(cx: &mut TestAppContext) {
    let text = "fun f() {\n    val a = 1\n\n    val b = 2\n}\n";
    let (f, _workbench, cx) = open_one(cx, "src/B.kt", text);
    f.caret_to(cx, 2, 0);
    run_action(cx, "code.moveStatementUp", None);
    f.wait_text(cx, "fun f() {\n\n    val a = 1\n    val b = 2\n}\n");
    f.idle(cx);
}

/// Complete Current Statement closes the brackets the line left open where the backend's syntax
/// errors (`syntax_errors`) place the break, then finishes the line with the text rules: the
/// text rules alone would put the `;` inside the call. C#'s editing mode colours a token stream,
/// so the backend's tree is asked.
#[gpui_kit::test]
async fn complete_statement_closes_what_the_syntax_errors_say_is_left_open(cx: &mut TestAppContext) {
    let (f, _workbench, cx) = open_one(cx, "src/a.cs", "foo(1, bar(2\n");
    f.caret_to(cx, 0, 12);
    run_action(cx, "code.completeStatement", None);
    f.wait_text(cx, "foo(1, bar(2));\n\n");
    assert_eq!(f.caret(cx), (1, 0));
    f.idle(cx);
    // One undo step takes it back.
    run_action(cx, "edit.undo", None);
    assert_eq!(f.text(cx), "foo(1, bar(2\n");
}

/// In a language whose editing mode parses the document itself (JavaScript), Complete Current
/// Statement uses the text rules alone, as that editor does.
#[gpui_kit::test]
async fn complete_statement_uses_the_text_rules_where_the_editing_mode_parses_the_document(cx: &mut TestAppContext) {
    let (f, _workbench, cx) = open_one(cx, "src/a.js", "foo(1, bar(2\n");
    f.caret_to(cx, 0, 12);
    run_action(cx, "code.completeStatement", None);
    f.wait_text(cx, "foo(1, bar(2;\n\n");
    f.idle(cx);
}

/// Comment with Block Comment over a block comment that holds its own closing token takes it out
/// once the syntax tree (`syntax_node_at`) says it is one nested comment.
#[gpui_kit::test]
async fn a_block_comment_holding_its_closing_token_comes_out_when_the_tree_confirms_it(cx: &mut TestAppContext) {
    let (f, _workbench, cx) = open_one(cx, "src/main.rs", "/* a /* b */ c */\nfn main() {}\n");
    f.select(cx, 0, 17);
    run_action(cx, "code.commentBlock", None);
    f.wait_text(cx, "a /* b */ c\nfn main() {}\n");
    f.idle(cx);
}

/// Comment with Line Comment in a language without line comments takes out the nested block
/// comment of the caret line once the syntax tree confirms it is one comment.
#[gpui_kit::test]
async fn line_comment_without_line_comments_takes_out_a_nested_block_comment_the_tree_confirms(
    cx: &mut TestAppContext,
) {
    let (f, _workbench, cx) = open_one(cx, "src/main.ml", "(* a (* b *) c *)\nlet x = 1\n");
    f.caret_to(cx, 0, 0);
    run_action(cx, "code.commentLine", None);
    f.wait_text(cx, "a (* b *) c\nlet x = 1\n");
    f.idle(cx);
}

/// Toggle Case keeps the case of the escape sequences in strings: `\n` does not become `\N`.
#[gpui_kit::test]
async fn toggle_case_keeps_the_escapes_of_a_string(cx: &mut TestAppContext) {
    let text = "fn main() {\n    let s = \"a\\nb\";\n}\n";
    let (f, _workbench, cx) = open_one(cx, "src/main.rs", text);
    f.highlighted(cx);
    let (from, to) = f.editor.read_with(cx, |editor, _| (editor.offset_of(1, 4), editor.offset_of(1, 19)));
    f.select(cx, from, to);
    run_action(cx, "code.toggleCase", None);
    assert_eq!(f.text(cx), "fn main() {\n    LET S = \"A\\nB\";\n}\n");
}

/// Move Element finds the elements of a call whose string argument holds a closing bracket: the
/// string is not code, so the bracket does not end the list.
#[gpui_kit::test]
async fn move_element_skips_a_bracket_inside_a_string(cx: &mut TestAppContext) {
    let text = "fn main() {\n    f(\")\", b);\n}\n";
    let (f, _workbench, cx) = open_one(cx, "src/main.rs", text);
    f.highlighted(cx);
    f.caret_to(cx, 1, 11);
    run_action(cx, "code.moveElementLeft", None);
    assert_eq!(f.text(cx), "fn main() {\n    f(b, \")\");\n}\n");
    assert_eq!(f.caret(cx), (1, 6));
}

impl Opened {
    fn hint(&self, cx: &mut VisualTestContext) -> Option<String> {
        let intelligence = cx.update(|_, cx| crate::editor::intelligence::EditorIntelligence::of(&self.editor, cx));
        intelligence.and_then(|intelligence| intelligence.read_with(cx, |this, _| this.hint().map(str::to_owned)))
    }

    fn usages(&self, cx: &mut VisualTestContext) -> Vec<std::ops::Range<u32>> {
        let layer = crate::editor::marks::MarkLayer::Usages;
        self.editor.read_with(cx, |editor, _| editor.marks(layer).iter().map(|mark| mark.range.clone()).collect())
    }
}

const NESTED: &str = "fn a() {\n    if x {\n        if y {\n            z();\n        }\n    }\n}\n";

/// Context Info before the backend's fold ranges came (their request waits for a clock these waits
/// do not move): the header lines of the syntax tree's blocks around the caret, outermost first,
/// from the node at the line's start (`syntax_node_at`).
#[gpui_kit::test]
async fn context_info_without_fold_ranges_shows_the_header_lines_of_the_syntax_trees_blocks(cx: &mut TestAppContext) {
    let (f, _workbench, cx) = open_one(cx, "src/main.rs", NESTED);
    assert!(!f.editor.read_with(cx, |editor, _| editor.has_backend_folds()), "no fold ranges yet");
    f.caret_to(cx, 3, 12);
    run_action(cx, "editor.contextInfo", None);
    let headers = "fn a() {\n    if x {\n        if y {";
    settle(cx, |cx| f.hint(cx).as_deref() == Some(headers));
    assert!(!f.editor.read_with(cx, |editor, _| editor.has_backend_folds()), "the lines came from the tree");
    // On a line inside no block, it says so.
    f.caret_to(cx, 0, 0);
    run_action(cx, "editor.contextInfo", None);
    settle(cx, |cx| f.hint(cx).as_deref() == Some("No context information here"));
}

/// Highlight Usages in File without a language server: the occurrences of the local at the caret
/// in its scope, as the syntax tree resolves them (`syntax_locals`), are marked and counted in the
/// status bar, with the note that they are based on syntax.
#[gpui_kit::test]
async fn highlight_usages_without_a_server_marks_the_locals_occurrences_from_the_syntax_tree(cx: &mut TestAppContext) {
    let text = "function f() {\n  let count = 1;\n  return count;\n}\n";
    let (f, workbench, cx) = open_one(cx, "src/a.js", text);
    let usage = text.rfind("count").expect("the usage") as u32 + 2;
    f.select(cx, usage, usage);
    run_action(cx, "find.highlightUsages", None);
    wait_long(cx, "the usages", |cx| !f.usages(cx).is_empty());
    assert_eq!(f.usages(cx), [21..26, 41..46]);
    let status = workbench.read_with(cx, |w, _| w.usages_status().map(str::to_owned)).expect("the status text");
    assert!(status.starts_with("2 usages of 'count' found ("), "{status}");
    assert_eq!(f.hint(cx).as_deref(), Some(crate::editor_lsp::navigation::SYNTAX_NOTE));
    // Again on a usage: the highlighting goes.
    run_action(cx, "find.highlightUsages", None);
    assert!(f.usages(cx).is_empty());
    assert!(workbench.read_with(cx, |w, _| w.usages_status().is_none()));
}
