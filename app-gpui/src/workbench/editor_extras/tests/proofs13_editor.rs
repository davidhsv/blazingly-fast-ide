//! Rainbow brackets through the editor's paint (the palette cycling, unmatched and mismatched
//! brackets, strings, comments and regular expressions, the depth far into a document after an
//! edit) and the text width of Distraction Free Mode.

use super::*;
use ide_workspace::settings::SettingsScope;

/// The colour the editor last painted the character at `line`, `col` in, after a fresh frame.
fn painted(editor: &Entity<Editor>, line: u32, col: u32, cx: &mut VisualTestContext) -> Option<gpui_kit::Hsla> {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    editor.read_with(cx, |e, _| e.painted_color_at(line, col))
}

/// The three bracket colours of the scheme and its error colour.
fn palette(cx: &mut VisualTestContext) -> [gpui_kit::Hsla; 4] {
    cx.update(|_, cx| {
        ["syn-bracket-1", "syn-bracket-2", "syn-bracket-3", "syn-invalid"].map(|token| theme::color(cx, token))
    })
}

/// Turns Rainbow Brackets on through its action.
fn rainbow_on(f: &Fixture, cx: &mut VisualTestContext) {
    f.run(cx, "editor.toggleRainbowBrackets");
    assert!(cx.update(|_, cx| crate::platform::settings::global(cx).editor.rainbow_brackets));
}

/// Opens `file` of the fixture's project in its editor and waits for its text.
fn show(f: &Fixture, file: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = f.root.join(file);
    let wanted = path.clone();
    f.workbench.update_in(cx, |w, window, cx| w.open_project_file(path, window, cx));
    settle(cx, |cx| f.editor(cx).is_some_and(|e| e.read_with(cx, |e, _| e.path() == wanted)));
    f.active(cx)
}

/// Brackets nested four deep take the scheme's three colours by depth and start again with the
/// first at the fourth level.
#[gpui_kit::test]
async fn colours_brackets_by_nesting_depth_cycling_through_the_palette(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("src/main.rs", "f(a[b{c(d)}])\n")], "src/main.rs", None);
    let editor = f.active(cx);
    rainbow_on(&f, cx);
    let [first, second, third, _] = palette(cx);
    settle(cx, |cx| painted(&editor, 0, 3, cx) == Some(second));
    let opening = [(1, first), (3, second), (5, third), (7, first)];
    let closing = [(9, first), (10, third), (11, second), (12, first)];
    for (col, colour) in opening.into_iter().chain(closing) {
        assert_eq!(painted(&editor, 0, col, cx), Some(colour), "column {col}");
    }
    f.close(cx);
}

/// A closer without an opener, an opener closed by the wrong bracket, and an opener never closed are
/// painted in the error colour; a matched pair inside an unclosed opener keeps its depth's colour.
#[gpui_kit::test]
async fn marks_unmatched_and_mismatched_brackets(cx: &mut TestAppContext) {
    let files = [("a.rs", "a)b\n"), ("b.rs", "(]\n"), ("c.rs", "x(y\n"), ("d.rs", "((a)\n")];
    let (f, cx) = open(cx, &files, "a.rs", None);
    rainbow_on(&f, cx);
    let [_, second, _, invalid] = palette(cx);
    let cases: [(&str, &[(u32, gpui_kit::Hsla)]); 4] = [
        ("a.rs", &[(1, invalid)]),
        ("b.rs", &[(0, invalid), (1, invalid)]),
        ("c.rs", &[(1, invalid)]),
        ("d.rs", &[(0, invalid), (1, second), (3, second)]),
    ];
    for (file, marks) in cases {
        let editor = show(&f, file, cx);
        let (col, colour) = marks[0];
        settle(cx, |cx| painted(&editor, 0, col, cx) == Some(colour));
        for &(col, colour) in marks {
            assert_eq!(painted(&editor, 0, col, cx), Some(colour), "{file} column {col}");
        }
    }
    f.close(cx);
}

/// With the syntax tree of a JavaScript file, brackets in strings, template strings, regular
/// expressions and comments are not coloured; the others are, by depth.
#[gpui_kit::test]
async fn skips_strings_comments_and_regexps_with_the_syntax_tree(cx: &mut TestAppContext) {
    let text = "f(\"(\", /[(]/, `{`) // )\n/* ] */ g([1, {a: 2}])\n";
    let (f, cx) = open(cx, &[("a.js", text)], "a.js", None);
    let editor = f.active(cx);
    settle(cx, |cx| editor.read_with(cx, |e, _| !e.highlights().is_empty()));
    rainbow_on(&f, cx);
    let [first, second, third, invalid] = palette(cx);
    settle(cx, |cx| painted(&editor, 1, 10, cx) == Some(second));
    let coloured = [(0, 1, first), (0, 17, first), (1, 9, first), (1, 10, second), (1, 14, third)];
    for (line, col, colour) in coloured.into_iter().chain([(1, 19, third), (1, 20, second), (1, 21, first)]) {
        assert_eq!(painted(&editor, line, col, cx), Some(colour), "{line}:{col}");
    }
    for (line, col) in [(0, 3), (0, 8), (0, 9), (0, 10), (0, 15), (0, 22), (1, 3)] {
        let drawn = painted(&editor, line, col, cx);
        assert!(![first, second, third, invalid].map(Some).contains(&drawn), "{line}:{col} is {drawn:?}");
    }
    f.close(cx);
}

/// Far into a long document the brackets shown take the depth of everything before them; an
/// opener typed near the top deepens them, and leaves the last closer unmatched.
#[gpui_kit::test]
async fn knows_the_depth_at_a_viewport_far_into_the_document_and_after_edits(cx: &mut TestAppContext) {
    let text = format!("{{\n{}[z]\n}}\n", "x(y)\n".repeat(5000));
    let (f, cx) = open(cx, &[("src/main.rs", text.as_str())], "src/main.rs", None);
    let editor = f.active(cx);
    rainbow_on(&f, cx);
    let [first, second, third, invalid] = palette(cx);
    let (opener, closer) = (5001, 5002);
    editor.update(cx, |e, cx| e.scroll_to_line(4990, cx));
    settle(cx, |cx| painted(&editor, opener, 0, cx) == Some(second));
    assert_eq!(painted(&editor, opener, 2, cx), Some(second), "]");
    assert_eq!(painted(&editor, closer, 0, cx), Some(first), "}}");

    // An opener at the start of line 20 (its auto-inserted closer, if any, is deleted again).
    editor.update(cx, |e, cx| {
        let at = e.offset_of(20, 0);
        e.select(at, at, cx);
    });
    cx.simulate_input("(");
    if editor.read_with(cx, |e, _| e.text().lines().nth(20).is_some_and(|line| line.starts_with("()"))) {
        cx.simulate_keystrokes("delete");
    }
    assert!(editor.read_with(cx, |e, _| e.text().lines().nth(20).is_some_and(|line| line.starts_with('('))));
    editor.update(cx, |e, cx| e.scroll_to_line(4990, cx));
    settle(cx, |cx| painted(&editor, opener, 0, cx) == Some(third));
    assert_eq!(painted(&editor, opener, 2, cx), Some(third), "]");
    assert_eq!(painted(&editor, closer, 0, cx), Some(invalid), "}} is unmatched");
    f.close(cx);
}

/// Distraction Free Mode centres the text at 80 columns by default, and at the width of Settings ›
/// Editor › Distraction free text width once that is set.
#[gpui_kit::test]
async fn defaults_the_text_width_to_80_columns_and_centres_at_the_configured_width(cx: &mut TestAppContext) {
    let line = "x".repeat(150);
    let (f, cx) = open(cx, &[("notes.txt", line.as_str())], "notes.txt", None);
    let editor = f.active(cx);
    assert_eq!(cx.update(|_, cx| crate::platform::settings::global(cx).editor.appearance.distraction_free_width), 80);
    // How far the text starts from the editor's left edge, the width of `columns` columns, the
    // editor's width.
    let geometry = |cx: &mut VisualTestContext, columns: u32| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            let bounds = e.painted_bounds().expect("painted");
            let start = e.position_of(e.offset_of(0, 0)).expect("painted").x;
            let end = e.position_of(e.offset_of(0, columns)).expect("painted").x;
            (start - bounds.left(), end - start, bounds.size.width)
        })
    };
    let centred_at = |cx: &mut VisualTestContext, columns: u32| {
        let (left, text, width) = geometry(cx, columns);
        ((width - (text + px(22.0))) / 2.0 - left).abs() < px(1.0)
    };

    f.run(cx, "editor.toggleZen");
    assert!(centred_at(cx, 80), "{:?}", geometry(cx, 80));
    assert!(!centred_at(cx, 100), "not at 100 columns by default");

    cx.update(|_, cx| {
        let (key, width) = ("editor.appearance.distractionFreeWidth", serde_json::Value::from(100));
        crate::platform::settings::set("w1", SettingsScope::Global, key, &width, cx)
    })
    .expect("the width is stored");
    settle(cx, |cx| centred_at(cx, 100));
    assert!(!centred_at(cx, 80));
    f.run(cx, "editor.toggleZen");
    f.close(cx);
}
