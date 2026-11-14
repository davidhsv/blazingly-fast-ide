use super::*;

/// An item the server lists without its import, holding only the data to resolve it with; resolved, it carries
/// the import as an additional edit at the top of the file.
fn lazy_item_and_its_resolution() -> (Value, Value) {
    let mut lazy = item("fooBar", 6);
    lazy["data"] = json!({ "id": 7 });
    let start = json!({ "line": 0, "character": 0 });
    let mut resolved = lazy.clone();
    resolved["additionalTextEdits"] = json!([{ "range": { "start": start, "end": start }, "newText": "use foo;\n" }]);
    (lazy, resolved)
}

/// Accepting an item whose import the server computes only when the item is resolved puts the item in and the
/// import the resolve answer carries at the top of the file; the resolve request hands the server the item's
/// own data back.
#[gpui_kit::test]
async fn an_items_import_that_only_its_resolve_answer_carries_goes_in_with_the_accepted_item(cx: &mut TestAppContext) {
    let (lazy, resolved) = lazy_item_and_its_resolution();
    let answers = json!({
        "textDocument/completion": [lazy, item("other", 6)],
        "completionItem/resolve": resolved,
    });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", answers);

    cx.simulate_input("obj.");
    settle(cx, |cx| !f.rows(cx).is_empty());
    assert_eq!(f.rows(cx), ["fooBar", "other"]);
    assert_eq!(f.text(cx), "obj.", "the completion answer itself changes nothing");

    cx.simulate_keystrokes("enter");
    settle(cx, |cx| f.text(cx) == "use foo;\nobj.fooBar");
    assert!(f.rows(cx).is_empty(), "the lookup closed");
    let asked = f.requests("completionItem/resolve");
    assert!(!asked.is_empty(), "the item was resolved");
    assert_eq!(asked[0]["params"]["label"], json!("fooBar"));
    assert_eq!(asked[0]["params"]["data"], json!({ "id": 7 }));
    f.close(cx);
}
