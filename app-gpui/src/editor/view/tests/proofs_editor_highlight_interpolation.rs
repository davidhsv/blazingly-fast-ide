//! String templates in Kotlin, Scala, Dart and C#: the code inside a template is painted as code
//! (names as variables, operators, numbers, comments), the delimiters not as string, the rest as string;
//! escaped or doubled delimiters stay string.

use super::proofs_editor_highlight_support::{Expect, highlighted};
use super::*;

const STRING: Option<&str> = Some("syn-string");
const NUMBER: Option<&str> = Some("syn-number");

/// The file `case<n>` of `language` holding the expression `source` where an expression may stand.
fn file_of(n: usize, language: &str, source: &str) -> (String, String) {
    let (extension, text) = match language {
        "Kotlin" => ("kt", format!("fun main() {{\n    val x = {source}\n}}\n")),
        "Scala" => ("scala", format!("object A {{\n  val x = {source}\n}}\n")),
        "Dart" => ("dart", format!("void main() {{\n  var x = {source};\n}}\n")),
        "C#" => ("cs", format!("class A {{\n  void M() {{\n    var x = {source};\n  }}\n}}\n")),
        _ => unreachable!("no grammar for {language}"),
    };
    (format!("case{n}.{extension}"), text)
}

#[gpui_kit::test]
async fn handles_csharp_verbatim_interpolation_and_doubled_quotes(cx: &mut TestAppContext) {
    let (name, text) = file_of(0, "C#", "$@\"say \"\"hello\"\" {i + 1}\nend\"");
    let (f, cx) = highlighted(cx, &name, &text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "+ «1»}", 0, NUMBER);
        expect.painted(editor, "say \"\"hello\"\"", 0, STRING);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn handles_csharp_raw_interpolation_with_multiple_dollar_delimiters(cx: &mut TestAppContext) {
    let (name, text) = file_of(0, "C#", "$$\"\"\"literal {brace} {{i + 1}} tail\"\"\"");
    let (f, cx) = highlighted(cx, &name, &text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        expect.painted(editor, "+ «1»}}", 0, NUMBER);
        expect.never(editor, "{{", 0, "syn-string");
        expect.never(editor, "1«}}»", 0, "syn-string");
        expect.painted(editor, "{brace}", 0, STRING);
        expect.check(editor);
    });
    f.host.close_project("w1");
}
