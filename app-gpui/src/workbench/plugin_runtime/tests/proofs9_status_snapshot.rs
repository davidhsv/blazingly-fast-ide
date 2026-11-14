use super::*;

/// A status item a plugin's code set while no other window was open shows in a window opened afterwards: that
/// window heard no status event for it, so the item comes from the plugin host's listing of the window's items.
#[gpui_kit::test]
async fn a_window_opened_after_a_plugin_set_its_status_item_shows_the_item(cx: &mut TestAppContext) {
    let home = start(cx);
    let folder = plugin_copy(home.path(), "api14", str::to_owned);
    let id = link(cx, &folder, &[], "global-status");
    let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    run_action(cx, &plugin_action_id(&id, "global-status"), Some(Value::from("set before")));
    wait(cx, "the item shows in the first window", |cx| {
        status_texts(&workbench, cx).iter().any(|text| text == "set before")
    });

    let other = tempfile::tempdir().expect("a temporary folder");
    let root = other.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (later, cx2) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
    cx2.update(|window, _| window.activate_window());
    later.update_in(cx2, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait(cx2, "the second project opens", |cx| later.read_with(cx, |w, _| w.project_tree().is_some()));
    wait(cx2, "the item shows in the later window", |cx| {
        status_texts(&later, cx).iter().any(|text| text == "set before")
    });
    cx2.update(|_, cx| cx.global::<Backend>().host.close_project("w2"));
}
