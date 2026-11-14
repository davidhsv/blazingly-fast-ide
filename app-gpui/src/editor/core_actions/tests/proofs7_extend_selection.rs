use super::*;

/// Extend Selection from inside an argument starts with the word and grows strictly at each step, and one step is
/// the whole call: a range only the backend's syntax tree has (no word, bracket or line of the text is the call).
/// Shrink Selection walks the same steps back to the caret.
#[gpui_kit::test]
async fn extend_selection_takes_the_call_around_its_arguments_from_the_syntax_tree(cx: &mut TestAppContext) {
    let text = "fn a() {\n    let value = call(one, two);\n}\n";
    let (f, cx) = open(cx, "main.rs", text).await;
    let caret = "fn a() {\n    let value = call(o|ne, two);\n}\n";
    let call = "fn a() {\n    let value = [call(one, two)];\n}\n";
    f.select(cx, caret);
    let mut seen: Vec<String> = Vec::new();
    while seen.last().is_none_or(|last| last != call) {
        assert!(seen.len() < 6, "no step selects the call: {seen:#?}");
        let before = f.shown(cx);
        run(cx, "code.extendSelection");
        settle(cx, |cx| f.shown(cx) != before);
        seen.push(f.shown(cx));
    }
    assert_eq!(seen[0], "fn a() {\n    let value = call([one], two);\n}\n");
    let sizes: Vec<u32> =
        seen.iter().map(|marked| state(marked).selections[0]).map(|(anchor, head)| anchor.abs_diff(head)).collect();
    assert!(sizes.windows(2).all(|pair| pair[0] < pair[1]), "every step selects more: {seen:#?}");
    for expected in seen.iter().rev().skip(1) {
        run(cx, "code.shrinkSelection");
        assert_eq!(&f.shown(cx), expected);
    }
    run(cx, "code.shrinkSelection");
    assert_eq!(f.shown(cx), caret);
    close(cx);
}
