use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// A go.mod requirement that is both unused and upgradable: two problems over the same text.
const REQUIREMENT: &str = " github.com/sergi/go-diff v1.3.1\n";

fn not_used() -> Value {
    json!({
        "range": range(0, 1, 25),
        "severity": 2,
        "message": "github.com/sergi/go-diff is not used in this module",
        "source": "go mod tidy",
    })
}

fn upgradable() -> Value {
    json!({
        "range": range(0, 1, 25),
        "severity": 3,
        "message": "github.com/sergi/go-diff can be upgraded",
        "source": "upgrade available",
    })
}

/// The server's two fixes of the line, each naming the problem it fixes; removing the requirement
/// is the preferred one.
fn fixes(file: &Path, line_end: u32) -> Value {
    json!([
        {
            "title": "Remove dependency: github.com/sergi/go-diff",
            "kind": "quickfix",
            "isPreferred": true,
            "diagnostics": [not_used()],
            "edit": edit(file, vec![text_edit(range(0, 0, line_end), "")]),
        },
        {
            "title": "Upgrade to v1.4.0",
            "kind": "quickfix",
            "isPreferred": false,
            "diagnostics": [upgradable()],
            "edit": edit(file, vec![text_edit(range(0, 26, 32), "v1.4.0")]),
        },
    ])
}

/// Apply Quick Fix applies the fix that names the problem at the caret, even when the fix of
/// another problem over the same text is the preferred one: on the upgradable requirement it
/// upgrades, on the unused one it removes the requirement.
#[gpui_kit::test]
async fn uses_the_matching_fix_for_apply_quick_fix(cx: &mut TestAppContext) {
    let answers = json!({ "diagnostics": [upgradable()] });
    let (f, cx) = open(cx, &[("go.mock", REQUIREMENT)], "go.mock", Some(answers), &[]);
    let file = f.root.join("go.mock");
    let editor = f.active(cx);
    let messages = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| problems(&editor, cx).into_iter().map(|problem| problem.message).collect::<Vec<_>>())
    };
    settle(cx, "the upgrade problem", |cx| messages(cx) == ["github.com/sergi/go-diff can be upgraded"]);
    f.answer(json!({ "diagnostics": [upgradable()], "textDocument/codeAction": fixes(&file, 32) }));
    f.put_caret(cx, 3);
    f.run(cx, "code.applyQuickFix");
    settle(cx, "the upgraded requirement", |cx| f.text(cx) == " github.com/sergi/go-diff v1.4.0\n");

    // The same line, now reported unused only: its own fix applies.
    f.answer(json!({ "diagnostics": [not_used()], "textDocument/codeAction": fixes(&file, 32) }));
    editor.update(cx, |e, cx| e.insert(Some(32..32), " ", cx));
    settle(cx, "the unused problem", |cx| messages(cx) == ["github.com/sergi/go-diff is not used in this module"]);
    f.put_caret(cx, 3);
    f.run(cx, "code.applyQuickFix");
    settle(cx, "the removed requirement", |cx| f.text(cx) == " \n");
    f.close(cx);
}

/// Quick Definition drops an answer that arrives after the text it was asked for changed: the
/// server answers the first request late, the user edits meanwhile and asks again, and only the
/// second answer's definition is ever shown.
#[gpui_kit::test]
async fn quick_definition_drops_an_answer_that_arrives_after_the_text_changed(cx: &mut TestAppContext) {
    let files = [("a.mock", "call target here\n"), ("b.mock", "zero\none\n  target\nthree\n")];
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
    let b = f.root.join("b.mock");
    let location = |line: u32, character: u32| {
        let at = json!({ "line": line, "character": character });
        json!([{ "uri": uri(&b), "range": { "start": at, "end": at } }])
    };
    f.answer(json!({
        "delays": { "textDocument/definition": [2] },
        "textDocument/definition": { "by_call": [location(2, 2), location(0, 0)] },
    }));
    let shown = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let _recorder = cx.update(|_, cx| {
        let shown = shown.clone();
        cx.observe(&f.workbench, move |workbench, cx| {
            if let Some(popup) = workbench.read(cx).language_popup::<DefinitionPopup>() {
                shown.borrow_mut().push(popup.read(cx).header());
            }
        })
    });
    f.put_caret(cx, 7);
    f.run(cx, "code.quickDefinition");
    settle(cx, "the server has the first request", |_| f.requests("textDocument/definition").len() == 1);
    let editor = f.active(cx);
    editor.update(cx, |e, cx| e.insert(Some(16..16), " ", cx));
    f.run(cx, "code.quickDefinition");
    settle(cx, "the second answer's popup", |cx| {
        f.workbench.read_with(cx, |w, cx| {
            w.language_popup::<DefinitionPopup>().is_some_and(|popup| popup.read(cx).header()[1] == "b.mock:1")
        })
    });
    assert_eq!(f.requests("textDocument/definition").len(), 2);
    let shown = shown.borrow();
    assert!(shown.iter().all(|header| header[1] == "b.mock:1"), "the late answer was shown: {shown:?}");
    f.close(cx);
}
