use super::*;
use crate::platform::i18n::Localization;
use std::path::Path;

/// The platform's primary modifier with `key`: ⌘ on macOS, Ctrl elsewhere.
fn primary(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
}

/// The UTF-16 offset just before `word` on the line holding `text` of `document`.
fn offset_before(document: &str, text: &str, word: &str) -> u32 {
    let line = document.find(text).unwrap_or_else(|| panic!("{text} is in the document"));
    let at = line + document[line..].find(word).unwrap_or_else(|| panic!("{word} is on the line"));
    document[..at].encode_utf16().count() as u32
}

fn location_of(path: &Path, line: u32, from: u32, to: u32) -> Value {
    json!({ "uri": format!("file://{}", path.display()), "range": range(line, from, to) })
}

/// The labels of the lookup's rows, each drawn as `label — detail`.
fn labels(f: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    f.rows(cx).iter().map(|row| row.split(" — ").next().unwrap_or_default().to_owned()).collect()
}

/// The text of the line the active editor's caret is on.
fn caret_line(f: &Fixture, cx: &mut VisualTestContext) -> String {
    let editor = f.editor(cx).expect("an editor");
    let (line, _) = editor.read_with(cx, |editor, _| editor.caret_position());
    editor.read_with(cx, |editor, _| editor.text()).lines().nth(line as usize).unwrap_or_default().to_owned()
}

const BUILD: &str = "fun list(status: Status) {\n    service.find(status)\n}\n\nfun create(req: Req) {\n    \
                     val order: Order = service.create(req)\n    return order.toDto()\n}\n";
const SETTINGS: &str = "fun other() {\n    service.find(status)\n    val order: Order = service.create(req)\n}\n";

/// After `return order.toDto()`, Enter and `order.` then ⌃Space list the server's items `toDto` and
/// `total`; `toD` typed narrows them to `toDto`, and Enter puts it in: the line reads `order.toDto()`.
#[gpui_kit::test]
async fn completion_lists_the_servers_items_and_inserts_the_chosen_one(cx: &mut TestAppContext) {
    let items = json!([
        { "label": "toDto", "kind": 2, "detail": "OrderDto", "insertText": "toDto()" },
        { "label": "total", "kind": 10, "detail": "BigDecimal", "insertText": "total" },
    ]);
    let (f, cx) = open(cx, &[("build.mock", BUILD)], "build.mock", json!({ "textDocument/completion": items }));
    let end = offset_before(BUILD, "return order.toDto()", "toDto()") + "toDto()".len() as u32;
    f.put_caret(cx, end);
    cx.simulate_keystrokes("enter");
    cx.simulate_input("order.");
    cx.simulate_keystrokes("ctrl-space");
    settle(cx, "the lookup's rows", |cx| f.rows(cx).len() == 2);
    let rows = labels(&f, cx);
    assert!(rows.contains(&"total".to_owned()) && rows.contains(&"toDto".to_owned()), "{rows:?}");
    cx.simulate_input("toD");
    cx.run_until_parked();
    assert_eq!(labels(&f, cx), ["toDto"], "total is hidden");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(f.rows(cx).is_empty(), "the lookup closed");
    assert_eq!(caret_line(&f, cx).trim(), "order.toDto()");
    assert!(!f.requests("textDocument/completion").is_empty());
    f.close(cx);
}

/// ⌘B on `service` asks the server for the definition there and opens the other file it names with the
/// caret on the declaration's line.
#[gpui_kit::test]
async fn cmd_b_goes_to_the_declaration_in_another_file(cx: &mut TestAppContext) {
    let files = [("build.mock", BUILD), ("settings.mock", SETTINGS)];
    let (f, cx) = open(cx, &files, "build.mock", json!({}));
    let settings = f.root.join("settings.mock");
    f.server.answer(json!({ "textDocument/definition": [location_of(&settings, 2, 8, 13)] }));
    let at = offset_before(BUILD, "val order: Order", "service");
    f.put_caret(cx, at);
    cx.simulate_keystrokes(&primary("b"));
    settle(cx, "the editor of settings.mock", |cx| {
        f.editor(cx).is_some_and(|editor| editor.read_with(cx, |e, _| e.path() == settings.as_path()))
    });
    let opened = f.editor(cx).expect("the target's editor");
    settle(cx, "the caret on the declaration", |cx| opened.read_with(cx, |e, _| e.caret_position()).0 == 2);
    assert!(caret_line(&f, cx).contains("val order: Order = service.create(req)"));
    let asked = f.requests("textDocument/definition");
    assert_eq!(asked[0]["params"]["position"], json!({ "line": 5, "character": 23 }));
    f.close(cx);
}

/// ⌥F7 on `service` lists the server's three references under "Found usages 3 results", by file with
/// each usage's line; a double click on the usage in the other file opens that file.
#[gpui_kit::test]
async fn find_usages_lists_every_usage_by_file_and_opens_one(cx: &mut TestAppContext) {
    let files = [("build.mock", BUILD), ("settings.mock", SETTINGS)];
    let capabilities = json!({ "referencesProvider": true });
    let (f, cx) = open(cx, &files, "build.mock", json!({ "capabilities": capabilities.clone() }));
    let (build, settings) = (f.root.join("build.mock"), f.root.join("settings.mock"));
    let references =
        json!([location_of(&build, 1, 4, 11), location_of(&build, 5, 23, 30), location_of(&settings, 1, 4, 11)]);
    f.server.answer(json!({ "capabilities": capabilities, "textDocument/references": references }));
    f.put_caret(cx, offset_before(BUILD, "val order: Order", "service"));
    cx.simulate_keystrokes("alt-f7");
    settle(cx, "the Find Usages window", |cx| f.workbench.read_with(cx, |w, _| w.usages_view().is_some()));
    let view = f.workbench.read_with(cx, |w, _| w.usages_view()).expect("the Find Usages window is shown");
    settle(cx, "the search results", |cx| {
        view.read_with(cx, |view, _| view.search().is_some_and(|search| !search.loading))
    });
    let rows = |cx: &mut VisualTestContext| -> Vec<String> {
        view.update(cx, |view, cx| {
            view.rows(&cx.default_global::<Localization>().0).into_iter().map(|row| row.text).collect()
        })
    };
    let shown = rows(cx);
    let has = |prefix: &str| shown.iter().any(|row| row.starts_with(prefix));
    assert!(shown.iter().any(|row| row == "Found usages 3 results"), "{shown:?}");
    assert!(has("build.mock") && shown.iter().any(|row| row.starts_with("build.mock") && row.ends_with("2 usages")));
    assert!(has("2 service.find(status)") && has("6 val order: Order = service.create"), "{shown:?}");
    assert!(shown.iter().any(|row| row.starts_with("settings.mock") && row.ends_with("1 usage")), "{shown:?}");

    // The last usage is the one in settings.mock.
    let index = shown.iter().rposition(|row| row.starts_with("2 ")).expect("the other file's usage");
    cx.run_until_parked();
    let selector: &'static str = Box::leak(format!("usages-row-{index}").into_boxed_str());
    let position = cx.debug_bounds(selector).expect("the usage is drawn").center();
    let (modifiers, button) = (Modifiers::default(), gpui_kit::MouseButton::Left);
    cx.simulate_event(gpui_kit::MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position, modifiers, button, click_count: 2 });
    settle(cx, "the editor of settings.mock", |cx| {
        f.editor(cx).is_some_and(|editor| editor.read_with(cx, |e, _| e.path() == settings.as_path()))
    });
    f.close(cx);
}

/// The server's problem on `Order` is underlined there; ⌥⏎ on it lists both quick fixes, and a click
/// on the preferred import applies its edit.
#[gpui_kit::test]
async fn diagnostics_underline_the_range_and_alt_enter_applies_the_preferred_quick_fix(cx: &mut TestAppContext) {
    let bad = range(5, 15, 20);
    let problem = json!({ "range": bad, "message": "Unresolved reference: Order", "severity": 1, "source": "mock" });
    let (f, cx) = open(cx, &[("build.mock", BUILD)], "build.mock", json!({ "diagnostics": [problem.clone()] }));
    let build = f.root.join("build.mock");
    let import = json!({
        "title": "Import 'com.acme.orders.model.Order'",
        "kind": "quickfix",
        "isPreferred": true,
        "edit": { "changes": { format!("file://{}", build.display()): [
            { "range": range(4, 0, 0), "newText": "import com.acme.orders.model.Order\n" },
        ] } },
    });
    let create = json!({ "title": "Create class 'Order'", "kind": "quickfix", "isPreferred": false });
    f.server.answer(json!({ "diagnostics": [problem], "textDocument/codeAction": [import, create] }));
    let editor = f.editor(cx).expect("an editor");
    let order = offset_before(BUILD, "val order: Order", "Order ");
    let marks = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.marks(MarkLayer::Diagnostics).to_vec());
    settle(cx, "the underline", |cx| !marks(cx).is_empty());
    assert_eq!(marks(cx), [Mark { range: order..order + 5, kind: MarkKind::ErrorProblem }], "the squiggle is Order");

    f.put_caret(cx, order);
    cx.simulate_keystrokes("alt-enter");
    let intelligence = f.intelligence(cx);
    settle(cx, "the intentions list", |cx| intelligence.read_with(cx, |this, _| this.intentions().is_some()));
    let list = intelligence.read_with(cx, |this, _| this.intentions()).expect("the intentions");
    settle(cx, "both fixes", |cx| list.read_with(cx, |list, _| list.lines().len() >= 2));
    let lines = list.read_with(cx, |list, _| list.lines());
    assert!(lines.iter().any(|line| line.contains("Create class 'Order'")), "{lines:?}");
    let at = lines.iter().position(|line| line.contains("Import 'com.acme.orders.model.Order'")).expect("the fix");
    cx.run_until_parked();
    let selector: &'static str = Box::leak(format!("list-row-{at}").into_boxed_str());
    let position = cx.debug_bounds(selector).expect("the fix is drawn").center();
    cx.simulate_click(position, Modifiers::default());
    settle(cx, "the import is applied", |cx| f.text(cx).contains("import com.acme.orders.model.Order\n"));
    assert_eq!(f.text(cx).lines().nth(4), Some("import com.acme.orders.model.Order"));
    assert!(intelligence.read_with(cx, |this, _| this.intentions().is_none()));
    f.close(cx);
}
