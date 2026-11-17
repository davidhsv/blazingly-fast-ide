//! Shell colours inside double-quoted strings: the delimiters of variables, command substitutions and
//! arithmetic are painted as decorations, the names as variables, the code inside as code.

use super::proofs_editor_highlight_support::{Expect, highlighted};
use super::*;

const DELIMITER: Option<&str> = Some("syn-decorator");
const VARIABLE: Option<&str> = Some("syn-variable");
const OPERATOR: Option<&str> = Some("syn-punct");

#[gpui_kit::test]
async fn separates_variables_command_substitutions_and_arithmetic_inside_strings(cx: &mut TestAppContext) {
    let (f, cx) = highlighted(cx, "script.sh", "echo \"$name ${count:-1} $(echo hi) $((count + 1))\"\n").await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for delimiter in ["«$»name", "${", "-1«}»", "$(", "hi«)»", "$((", "))"] {
            expect.painted(editor, delimiter, 0, DELIMITER);
        }
        expect.painted(editor, "$«name»", 0, VARIABLE);
        expect.painted(editor, "${«count»", 0, VARIABLE);
        expect.painted(editor, "+ «1»", 0, Some("syn-number"));
        expect.painted(editor, "+", 0, OPERATOR);
        expect.painted(editor, "echo", 0, Some("syn-builtin"));
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn highlights_parameter_operators_without_treating_length_expansion_as_a_comment(cx: &mut TestAppContext) {
    let (f, cx) = highlighted(cx, "script.sh", "echo \"${#name} ${name:-1}\"\n").await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "${«#»", 0, OPERATOR);
        expect.painted(editor, "#«name»", 0, VARIABLE);
        expect.painted(editor, ":-", 0, OPERATOR);
        expect.nowhere(editor, "syn-comment");
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn keeps_nested_command_substitutions_and_quoted_strings_independent(cx: &mut TestAppContext) {
    let (f, cx) = highlighted(cx, "script.sh", "echo \"$(echo \"$(echo hi)\") after\"\n").await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for nth in 0..2 {
            expect.painted(editor, "$(", nth, DELIMITER);
        }
        expect.painted(editor, "hi«)»", 0, DELIMITER);
        expect.painted(editor, "\"«)» after", 0, DELIMITER);
        expect.painted(editor, " after\"", 0, Some("syn-string"));
        expect.check(editor);
    });
    f.host.close_project("w1");
}

/// Single quotes, an escaped dollar, a comment and a quoted heredoc are literal text: nothing in them is
/// painted as an interpolation delimiter.
#[gpui_kit::test]
async fn leaves_single_quotes_escaped_dollars_comments_and_heredocs_intact(cx: &mut TestAppContext) {
    let text = "echo '$name ${count}'\necho \"\\$name\"\n# \"${count}\"\ncat <<'EOF'\n$name ${count}\nEOF\n";
    let (f, cx) = highlighted(cx, "script.sh", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.nowhere(editor, "syn-decorator");
        expect.check(editor);
    });
    f.host.close_project("w1");
}
