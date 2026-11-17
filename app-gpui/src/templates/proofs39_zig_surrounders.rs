//! The Zig surrounders.

use super::live_surrounders::{SurroundSelection, surrounders_for};

// surround with entries › wraps Zig in its parenthesised brace syntax
#[test]
fn wraps_zig_in_its_parenthesised_brace_syntax() {
    let descriptions = |lines: bool, inline: bool| -> Vec<String> {
        surrounders_for("zig", SurroundSelection { lines, inline }).iter().map(|s| s.description.clone()).collect()
    };
    let lines = descriptions(true, false);
    for wanted in ["if", "if / else", "while", "for", "defer", "test"] {
        assert!(lines.iter().any(|description| description == wanted), "{wanted} in {lines:?}");
    }
    let statements = surrounders_for("zig", SurroundSelection { lines: true, inline: false });
    assert!(statements[0].body.contains("if ($COND$)"), "{}", statements[0].body);
    assert_eq!(descriptions(false, true), ["(expr)", "!(expr)"]);
}

// Not from a spec: the Zig set in its order, numbered from `zig.0`.
#[test]
fn the_zig_surrounders_are_numbered_in_order() {
    let all = surrounders_for("zig", SurroundSelection { lines: true, inline: true });
    let descriptions: Vec<&str> = all.iter().map(|s| s.description.as_str()).collect();
    let expected = [
        "if",
        "if / else",
        "while",
        "for",
        "defer",
        "test",
        "{ }",
        "//region...//endregion Comments",
        "(expr)",
        "!(expr)",
    ];
    assert_eq!(descriptions, expected);
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    assert_eq!((ids.first().copied(), ids.last().copied()), (Some("zig.0"), Some("zig.9")));
    assert_eq!(all[3].body, "for ($ITER$) |$ITEM$| {\n\t$SELECTION$\n}$END$");
    assert_eq!(all[5].body, "test \"$NAME$\" {\n\t$SELECTION$\n}$END$");
}
