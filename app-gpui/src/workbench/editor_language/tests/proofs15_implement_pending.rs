//! The lines an applied implement action inserts follow the file's indentation.

use super::*;

/// A server that generates its members at column 0 right under the class's `{`: the inserted
/// lines move one indentation level in, keeping their own nesting, and the rest of the file stays.
#[gpui_kit::test]
async fn implement_methods_indents_the_inserted_members_under_the_brace(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.mock", "class A {\n}\n")], "a.mock", Some(json!({})), &[]);
    let a = f.root.join("a.mock");
    let edit = edit(&a, vec![text_edit(range(1, 0, 0), "run() {\n    go()\n}\n")]);
    f.answer(json!({ "textDocument/codeAction": [
        { "title": "Implement missing members", "kind": "quickfix", "edit": edit },
    ] }));
    f.put_caret(cx, 6);
    f.run(cx, "code.implementMethods");
    settle(cx, |cx| f.text(cx) == "class A {\n    run() {\n        go()\n    }\n}\n");
    f.close(cx);
}
