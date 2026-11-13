//! End-to-end proofs of the Services tool window's row actions that `services_window.rs` carries
//! out: Move Up and Move Down of a run configuration within its type group, which reorder the
//! rows and store the project's order.
//!
//! The Docker actions are not carried out in this window: see the integration notes.

#[cfg(test)]
mod tests {
    use super::super::tests::{click_on, program, run_action, run_project, wait_long};
    use super::super::{SERVICES_TOOL_WINDOW, Workbench};
    use crate::platform::storage::UiStorage;
    use crate::run_testing::services::order_key;
    use crate::services::view::ServicesView;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    fn services_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ServicesView> {
        workbench
            .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
            .and_then(|view| view.downcast::<ServicesView>().ok())
            .expect("the Services tool window holds its view")
    }

    /// The run configuration rows, top to bottom.
    fn config_rows(view: &Entity<ServicesView>, cx: &mut VisualTestContext) -> Vec<String> {
        let rows = view.read_with(cx, |view, _| view.rows());
        rows.into_iter().map(|row| row.key).filter(|key| key.starts_with("run:config:")).collect()
    }

    /// Move Down on a configuration of a type group puts it after the next one, Move Up back before
    /// it; the order is stored for the project, so the next Services window lists them so.
    #[gpui_kit::test]
    async fn move_up_and_move_down_reorder_a_configuration_within_its_type_and_store_the_order(
        cx: &mut TestAppContext,
    ) {
        let configs = vec![program("one", "/bin/echo", "1"), program("two", "/bin/echo", "2")];
        let (workbench, cx, _folders, root, _store) = run_project(cx, configs);
        run_action(cx, "services.show", None);
        run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
        let view = services_view(&workbench, cx);
        wait_long(cx, "both configurations to show", |cx| config_rows(&view, cx).len() == 2);
        let before = config_rows(&view, cx);
        let (first, second) = (before[0].clone(), before[1].clone());

        let selector: &'static str = Box::leak(format!("services-row:{first}").into_boxed_str());
        click_on(cx, selector);
        click_on(cx, "services-action:moveDown");
        wait_long(cx, "the rows to swap", |cx| config_rows(&view, cx) == [second.clone(), first.clone()]);
        let key = order_key(&root.to_string_lossy());
        let stored =
            cx.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned)).expect("the order is stored");
        let id = |key: &str| format!("\"{}\"", key.trim_start_matches("run:config:"));
        let at = |key: &str| stored.find(&id(key)).unwrap_or_else(|| panic!("{key} in {stored}"));
        assert!(at(&second) < at(&first), "{stored}");

        click_on(cx, "services-action:moveUp");
        wait_long(cx, "the rows to swap back", |cx| config_rows(&view, cx) == [first.clone(), second.clone()]);
    }

    /// Show Stopped Containers and Show Untagged Images, run as actions, flip what the Docker tree
    /// leaves out and store it; the view options menu shows the state. No Docker call is made.
    #[gpui_kit::test]
    async fn the_docker_view_options_flip_and_store_what_the_tree_leaves_out(cx: &mut TestAppContext) {
        use crate::services::docker::{FILTERS_KEY, read_filters};
        let (_workbench, cx, _folders, _root, _store) = run_project(cx, Vec::new());
        let filters =
            |cx: &mut VisualTestContext| cx.update(|_, cx| read_filters(cx.global::<UiStorage>().get(FILTERS_KEY)));
        assert!(filters(cx).show_stopped_containers && filters(cx).show_untagged_images, "both show at first");
        run_action(cx, "docker.toggleStoppedContainers", None);
        assert!(!filters(cx).show_stopped_containers);
        assert!(filters(cx).show_untagged_images);
        run_action(cx, "docker.toggleUntaggedImages", None);
        assert!(!filters(cx).show_untagged_images);
        run_action(cx, "docker.toggleStoppedContainers", None);
        assert!(filters(cx).show_stopped_containers);
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(FILTERS_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some(r#"{"showStoppedContainers":true,"showUntaggedImages":false}"#));
    }
}
