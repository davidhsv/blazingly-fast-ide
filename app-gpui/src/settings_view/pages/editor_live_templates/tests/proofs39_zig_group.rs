use super::*;

/// The built-in groups in the order the page lists them: Zig's comes after Go's.
#[test]
fn the_zig_group_follows_the_go_group() {
    let groups = group_templates(builtin_templates(), "");
    let names: Vec<&str> = groups.iter().map(|(group, _)| group.as_str()).collect();
    let go = names.iter().position(|&name| name == "Go").expect("a Go group");
    assert_eq!(names.get(go + 1).copied(), Some("Zig"), "{names:?}");
    assert_eq!(names.get(go + 2).copied(), Some("Java"), "{names:?}");
}

/// Filtering the page by "zig" shows the Zig group with its nineteen templates, and a click on one
/// shows its text.
#[gpui_kit::test]
async fn the_page_lists_the_zig_templates(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    filter(&view, "zig", cx);
    let tree = view.read_with(cx, |view, cx| view.tree_lines(cx));
    let expected = [
        "Zig",
        "  [x] fn | Function",
        "  [x] pfn | Public function",
        "  [x] main | main function",
        "  [x] test | Test block",
        "  [x] st | Struct type",
        "  [x] en | Enum type",
        "  [x] un | Tagged union type",
        "  [x] if | if statement",
        "  [x] ife | if / else",
        "  [x] iferr | if result / error",
        "  [x] while | while loop",
        "  [x] for | for loop",
        "  [x] defer | defer block",
        "  [x] const | const declaration",
        "  [x] var | var declaration",
        "  [x] try | try expression",
        "  [x] catch | catch block",
        "  [x] sw | switch expression",
        "  [x] p | std.debug.print",
    ];
    assert_eq!(tree, expected);
    click(cx, "settings-live-item-zig.main");
    assert_eq!(template(&view, "zig.main", cx).body, "pub fn main() !void {\n\t$END$\n}");
    assert_eq!(view.read_with(cx, |view, cx| view.tree_lines(cx))[3], "> [x] main | main function");
}
