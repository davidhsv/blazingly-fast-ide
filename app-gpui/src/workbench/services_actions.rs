//! End-to-end proofs of the Services tool window's row actions that `services_window.rs` carries
//! out: Move Up and Move Down of a run configuration within its type group, which reorder the
//! rows and store the project's order, and the Docker view options, which only change what the
//! Docker tree leaves out.
//!
//! The tests give the Docker store no connection (`docker.json` in the temporary data folder), so
//! no Docker call is made, whether or not a `docker` program is installed.

#[cfg(test)]
mod tests {
    use super::super::tests::{click_on, run_action, start, wait_long};
    use super::super::{SERVICES_TOOL_WINDOW, Workbench};
    use crate::backend::Backend;
    use crate::platform::storage::UiStorage;
    use crate::run_testing::services::order_key;
    use crate::services::view::ServicesView;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::path::PathBuf;

    /// A program configuration that runs `command arg`.
    fn program(id: &str, command: &str, arg: &str) -> ide_run::RunConfig {
        ide_run::RunConfig {
            command: command.to_owned(),
            args: vec![arg.to_owned()],
            ..crate::run::configs::blank_config(id, id, "program")
        }
    }

    /// A trusted project with the saved configurations `configs`, open in an active window whose run
    /// configurations are listed, with no Docker connection: the window, the home and project
    /// folders (kept while they live), and the project's root.
    fn services_project(
        cx: &mut TestAppContext,
        configs: Vec<ide_run::RunConfig>,
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
        let home = start(cx);
        let data = cx.update(|cx| cx.global::<Backend>().dirs.data.clone());
        std::fs::create_dir_all(&data).expect("the data folder is made");
        std::fs::write(data.join("docker.json"), r#"{ "connections": [], "registries": [] }"#)
            .expect("the Docker settings are written");
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        ide_run::save_user_configs(&root, &configs).expect("the configurations are saved");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        wait_long(cx, "the project to open", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        wait_long(cx, "listing the configurations", |cx| store.read_with(cx, |store, _| store.loaded));
        (workbench, cx, [home, folder], root)
    }

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

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// Move Down on a configuration of a type group puts it after the next one, Move Up back before
    /// it; the order is stored for the project, so the next Services window lists them so.
    #[gpui_kit::test]
    async fn move_up_and_move_down_reorder_a_configuration_within_its_type_and_store_the_order(
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
        let rows = view.read_with(cx, |view, _| view.rows());
        assert!(rows.iter().all(|row| !row.key.starts_with("docker")), "no Docker connection is listed");

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
        close_project(cx);
    }

    /// Show Stopped Containers and Show Untagged Images, run as actions, flip what the Docker tree
    /// leaves out and store it. No Docker call is made.
    #[gpui_kit::test]
    async fn the_docker_view_options_flip_and_store_what_the_tree_leaves_out(cx: &mut TestAppContext) {
        use crate::services::docker::{FILTERS_KEY, read_filters};
        let (_workbench, cx, _folders, _root) = services_project(cx, Vec::new());
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
        close_project(cx);
    }
}
