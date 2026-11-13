//! String templates in Kotlin, Scala, Dart and C#: the code inside a template is painted as code
//! (names as variables, operators, numbers, comments), the delimiters not as string, the rest as string;
//! escaped or doubled delimiters stay string.

use super::proofs_editor_highlight_support::{Expect, highlighted, highlighted_files};
use super::*;

const STRING: Option<&str> = Some("syn-string");
const VARIABLE: Option<&str> = Some("syn-variable");
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
async fn separates_delimiters_names_operators_and_numbers(cx: &mut TestAppContext) {
    // (language, expression, the two names, the opening and the closing delimiter of the second template)
    let cases = [
        ("Kotlin", "println(\"$i + 1 = ${i + 1}\")", ["$«i» +", "${«i» +"], "«${»i", "1«}»\")"),
        ("Scala", "println(s\"$i + 1 = ${i + 1}\")", ["$«i» +", "${«i» +"], "«${»i", "1«}»\")"),
        ("Dart", "print(\"$i + 1 = ${i + 1}\")", ["$«i» +", "${«i» +"], "«${»i", "1«}»\")"),
        ("C#", "Console.WriteLine($\"{i} + 1 = {i + 1}\")", ["{«i»}", "{«i» +"], "\"«{»i", "{i«}»"),
    ];
    let files: Vec<(String, String)> =
        cases.iter().enumerate().map(|(n, (language, source, ..))| file_of(n, language, source)).collect();
    let (f, editors, cx) = highlighted_files(cx, &files).await;
    let mut failed = Vec::new();
    for ((language, _, names, open, close), editor) in cases.iter().zip(&editors) {
        editor.read_with(cx, |editor, _| {
            let mut expect = Expect::default();
            for name in names {
                expect.painted(editor, name, 0, VARIABLE);
            }
            expect.painted(editor, "i «+» 1}", 0, Some("syn-punct"));
            expect.painted(editor, "+ «1»}", 0, NUMBER);
            expect.never(editor, open, 0, "syn-string");
            expect.never(editor, close, 0, "syn-string");
            failed.extend(expect.failures(language, editor));
        });
    }
    assert!(failed.is_empty(), "{failed:#?}");
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn keeps_nested_braces_strings_and_comments_inside_expressions(cx: &mut TestAppContext) {
    let languages = ["Kotlin", "Scala", "Dart", "C#"];
    let source = |language: &str| match language {
        "C#" => "$\"{call(new Foo { n = 2 }, \"}\") /* } */ + 1} tail\"".to_owned(),
        _ => {
            let interpolator = if language == "Scala" { "s" } else { "" };
            format!("{interpolator}\"${{call({{ n -> 2 }}, \"}}\") /* }} */ + 1}} tail\"")
        }
    };
    let files: Vec<(String, String)> =
        languages.iter().enumerate().map(|(n, &language)| file_of(n, language, &source(language))).collect();
    let (f, editors, cx) = highlighted_files(cx, &files).await;
    let mut failed = Vec::new();
    for (language, editor) in languages.iter().zip(&editors) {
        editor.read_with(cx, |editor, _| {
            let mut expect = Expect::default();
            expect.painted(editor, "+ «1»}", 0, NUMBER);
            expect.painted(editor, "tail", 0, STRING);
            expect.painted(editor, "/* } */", 0, Some("syn-comment"));
            failed.extend(expect.failures(language, editor));
        });
    }
    assert!(failed.is_empty(), "{failed:#?}");
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn handles_kotlin_multiline_raw_templates_and_nested_templates(cx: &mut TestAppContext) {
    let text = "val s = \"\"\"raw \\$name\n${list.map { \"${it + 1}\" }} tail\"\"\"\nval after = 3\n";
    let (f, cx) = highlighted(cx, "a.kt", text).await;
    f.editor.read_with(cx, |editor, _| {
        let mut expect = Expect::default();
        for name in ["\\$«name»", "${«it»", "val «after»"] {
            expect.painted(editor, name, 0, VARIABLE);
        }
        expect.painted(editor, "it + «1»", 0, NUMBER);
        expect.painted(editor, "= «3»", 0, NUMBER);
        expect.painted(editor, "raw", 0, STRING);
        expect.check(editor);
    });
    f.host.close_project("w1");
}

#[gpui_kit::test]
async fn preserves_literal_dollars_and_braces(cx: &mut TestAppContext) {
    // (language, expression, the part of it that is all string)
    let cases = [
        ("Kotlin", "\"\\$name \\${value}\"", "\"\\$name \\${value}\""),
        ("Scala", "\"$name ${value}\"", "\"$name ${value}\""),
        ("Scala", "s\"$$name\"", "s«\"$$name\"»"),
        ("Dart", "r\"$name ${value}\"", "r«\"$name ${value}\"»"),
        ("C#", "$\"{{value}}\"", "$\"{{value}}\""),
    ];
    let files: Vec<(String, String)> =
        cases.iter().enumerate().map(|(n, (language, source, _))| file_of(n, language, source)).collect();
    let (f, editors, cx) = highlighted_files(cx, &files).await;
    let mut failed = Vec::new();
    for ((language, source, literal), editor) in cases.iter().zip(&editors) {
        editor.read_with(cx, |editor, _| {
            let mut expect = Expect::default();
            expect.painted(editor, literal, 0, STRING);
            failed.extend(expect.failures(&format!("{language} {source}"), editor));
        });
    }
    assert!(failed.is_empty(), "{failed:#?}");
    f.host.close_project("w1");
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
