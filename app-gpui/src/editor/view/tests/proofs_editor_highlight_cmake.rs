//! CMake colours, each stated as the colour token a CMake editor paints: commands as built-ins, argument
//! keywords as keywords, predefined variables as built-ins, variable expansions as constants.

use super::proofs_editor_highlight_support::{Expect, highlighted};
use super::*;

const COMMAND: Option<&str> = Some("syn-builtin");

#[gpui_kit::test]
async fn keeps_multiline_bracket_comments_and_literal_bracket_strings_opaque(cx: &mut TestAppContext) {
    let text = "#[=[\nset(CMAKE_FAKE ON)\n]=]\nset(TEXT [==[\n${LITERAL} # raw\n]==])\n# line comment\nmessage(hello)";
    let (f, cx) = highlighted(cx, "CMakeLists.txt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "set", 1, COMMAND);
        expect.partly(editor, "CMAKE_FAKE", 0, "syn-comment");
        expect.partly(editor, "${LITERAL}", 0, "syn-string");
        expect.nowhere(editor, "syn-constant");
        expect.painted(editor, "message", 0, COMMAND);
        expect.check(editor);
    });
    f.host.close_project("w1");
}
