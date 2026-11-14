use super::*;

/// Whether the Services toolbar's button `id` is greyed out, while the toolbar has it.
fn toolbar_disabled(view: &Entity<ServicesView>, id: &str, cx: &mut VisualTestContext) -> Option<bool> {
    let actions = view.read_with(cx, |view, cx| view.toolbar_actions(cx));
    actions.into_iter().find(|action| action.id == id).map(|action| action.disabled)
}

/// Selects the row `key` of the Services tree with a click.
fn select_row(key: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("services-row:{key}").into_boxed_str());
    click_on(cx, selector);
}

/// Move Up is greyed out on the first configuration of a type group and Move Down on the last; the order Move Down
/// gives is the one a Services window opened later on the project lists.
#[gpui_kit::test]
async fn move_up_and_down_are_greyed_out_at_the_ends_of_a_group_and_a_later_window_keeps_the_order(
    cx: &mut TestAppContext,
) {
    let configs = vec![program("one", "/bin/echo", "1"), program("two", "/bin/echo", "2")];
    let (workbench, cx, _folders, root) = services_project(cx, configs);
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = services_view(&workbench, cx);
    wait_long(cx, "both configurations to show", |cx| config_rows(&view, cx).len() == 2);
    let before = config_rows(&view, cx);
    let (first, second) = (before[0].clone(), before[1].clone());

    select_row(&first, cx);
    assert_eq!(toolbar_disabled(&view, "moveUp", cx), Some(true), "nothing is above the first row");
    assert_eq!(toolbar_disabled(&view, "moveDown", cx), Some(false));
    select_row(&second, cx);
    assert_eq!(toolbar_disabled(&view, "moveUp", cx), Some(false));
    assert_eq!(toolbar_disabled(&view, "moveDown", cx), Some(true), "nothing is below the last row");

    select_row(&first, cx);
    click_on(cx, "services-action:moveDown");
    wait_long(cx, "the rows to swap", |cx| config_rows(&view, cx) == [second.clone(), first.clone()]);
    close_project(cx);

    let (later, cx2) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
    cx2.update(|window, _| window.activate_window());
    later.update_in(cx2, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx2, "the project to open again", |cx| later.read_with(cx, |w, _| w.project_tree().is_some()));
    let driver = later.read_with(cx2, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx2, |driver, _| driver.store().clone());
    wait_long(cx2, "listing the configurations", |cx| store.read_with(cx, |store, _| store.loaded));
    run_action(cx2, "services.show", None);
    let reopened = services_view(&later, cx2);
    wait_long(cx2, "the stored order", |cx| config_rows(&reopened, cx) == [second.clone(), first.clone()]);
    cx2.update(|_, cx| cx.global::<Backend>().host.close_project("w2"));
}
