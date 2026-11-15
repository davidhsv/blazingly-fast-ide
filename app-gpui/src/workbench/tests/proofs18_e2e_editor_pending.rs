//! Not declared: the flows below need steps the application does not have.
//!
//! - Breadcrumbs: no code breadcrumbs are drawn over the editor, and Navigation Bar (`navigate.navigationBar`,
//!   ⌘↑) is not registered (the Project view's menu and the empty editor's hints name it only). The status bar's
//!   `editor.breadcrumbs` item exists; the test needs `Workbench::code_breadcrumbs(cx) -> Vec<String>`,
//!   `Workbench::status_breadcrumbs(cx) -> Vec<String>` and `Workbench::navigation_bar()`.
//! - The modified mark: an edited editor tab draws no mark (`pane_element` in `workbench/mod.rs` draws the title
//!   and the close button only); the test needs it drawn with the selector `tab-modified:<id>`.

use super::*;

const BUILD: &str = concat!(
    "class OrderController(private val service: OrderService) {\n",
    "    fun list(status: Status) = service.find(status)\n",
    "\n",
    "    fun create(req: Req): OrderDto {\n",
    "        return order.toDto()\n",
    "    }\n",
    "}\n",
);

/// The caret on `service.find(status` names `list()` in the status bar's and the code's breadcrumbs, after
/// `OrderController`; typing in the method keeps it; on `return order.toDto()` they name `create()` instead;
/// ⌘↑ opens the navigation bar on `create()` and Escape closes it.
#[gpui_kit::test]
async fn breadcrumbs_follow_the_caret_into_the_symbol_under_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("build.gradle.kts", BUILD)]);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("build.gradle.kts"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let at = BUILD.find("service.find(status").expect("the line") as u32;
    editor.update(cx, |e, cx| e.select(at, at, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.code_breadcrumbs(cx)).contains(&"list()".to_owned()));
    assert!(workbench.read_with(cx, |w, cx| w.code_breadcrumbs(cx)).contains(&"OrderController".to_owned()));
    assert!(workbench.read_with(cx, |w, cx| w.status_breadcrumbs(cx)).contains(&"list()".to_owned()));
    cx.simulate_keystrokes("home");
    cx.simulate_input("checked ");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, cx| w.code_breadcrumbs(cx)).contains(&"list()".to_owned()));
    let at = editor.read_with(cx, |e, _| e.text()).find("return order").expect("the line") as u32;
    editor.update(cx, |e, cx| e.select(at, at, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.code_breadcrumbs(cx)).contains(&"create()".to_owned()));
    assert!(!workbench.read_with(cx, |w, cx| w.code_breadcrumbs(cx)).contains(&"list()".to_owned()));
    cx.simulate_keystrokes("cmd-up");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.navigation_bar().is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.navigation_bar().is_none()));
}

/// An edit marks the only tab modified and the file on disk is unchanged; ⌘S saves it and the mark goes.
#[gpui_kit::test]
async fn an_edit_marks_the_tab_modified_and_cmd_s_saves_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("build.gradle.kts", BUILD)]);
    let file = root.join("build.gradle.kts");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let id = workbench.read_with(cx, |w, _| w.tabs().and_then(|tabs| tabs.active_tab().map(str::to_owned)));
    let mark: &'static str = Box::leak(format!("tab-modified:{}", id.expect("a tab")).into_boxed_str());
    cx.run_until_parked();
    assert!(cx.debug_bounds(mark).is_none());
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let end = BUILD.find("toDto()").expect("the line") as u32 + 7;
    editor.update(cx, |e, cx| e.select(end, end, cx));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("// saved by e2e");
    cx.run_until_parked();
    assert!(cx.debug_bounds(mark).is_some(), "the tab is marked modified");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), BUILD);
    cx.simulate_keystrokes("cmd-s");
    settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text.contains("// saved by e2e")));
    cx.run_until_parked();
    assert!(cx.debug_bounds(mark).is_none(), "the mark goes");
}
