use super::*;

/// The Zig tab comes after Go's and lists the Zig templates, each switched off and on again by its
/// box.
#[gpui_kit::test]
async fn the_zig_tab_lists_the_zig_templates(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    click(cx, "settings-postfix-group-zig");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[4], "JavaScript / TypeScript | Rust | Python | Go | [Zig] | Java | Kotlin");
    let zig = [
        "  [x] .if | if (expr)",
        "  [x] .not | !expr",
        "  [x] .const | const name = expr;",
        "  [x] .var | var name = expr;",
        "  [x] .return | return expr;",
        "  [x] .try | try expr;",
        "  [x] .defer | defer expr;",
        "  [x] .nn | if (expr) |item|",
        "  [x] .null | if (expr == null)",
        "  [x] .switch | switch (expr)",
        "  [x] .print | std.debug.print(\"{any}\\n\", .{expr})",
        "  [x] .par | (expr)",
    ];
    assert_eq!(lines[5..], zig);
    click(cx, "settings-postfix-template-zig.if");
    assert!(stored(cx).disabled.contains(&"zig.if".to_owned()));
    assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"  [ ] .if | if (expr)".to_owned()));
    click(cx, "settings-postfix-template-zig.if");
    assert!(!stored(cx).disabled.contains(&"zig.if".to_owned()));
}
