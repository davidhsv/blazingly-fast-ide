use super::*;

/// The Java tab describes `.var` as the declaration Introduce Variable makes.
#[gpui_kit::test]
async fn the_java_tab_describes_var_as_introduce_variable_does(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx);
    click(cx, "settings-postfix-group-java");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines[4].contains("[Java]"), "{}", lines[4]);
    assert!(lines.contains(&"  [x] .var | T name = expr;".to_owned()), "{lines:?}");
}
