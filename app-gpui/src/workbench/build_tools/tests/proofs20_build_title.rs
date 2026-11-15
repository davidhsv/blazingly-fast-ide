//! The title bar's Build Project hammer in a Maven project: drawn while nothing or a compiled
//! configuration is selected, hidden for a script, and a click builds the project with its Maven.

use super::*;
use crate::workbench::title_bar::BUILD_ITEM;

/// Whether the title bar lists the hammer and draws it.
fn hammer_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    cx.run_until_parked();
    let bar = workbench.read_with(cx, |w, _| w.title_bar().clone());
    let listed = cx.update(|window, cx| bar.read(cx).shown(window, cx).1.iter().any(|id| id == BUILD_ITEM));
    listed && cx.debug_bounds("build-title-item").is_some()
}

/// A Maven project, with the stand-in Maven of its settings and a script configuration `hello`, shows the
/// hammer with nothing selected and hides it while `hello` is; a click on it runs Build Project, which
/// starts the project's Maven.
#[gpui_kit::test]
async fn the_build_hammer_shows_for_a_maven_project_and_builds_it_with_the_projects_maven(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, tools) = build_project(cx, true, |root, tools| {
        maven_project(root, tools);
        let script = ide_run::RunConfig {
            command: "/bin/echo".to_owned(),
            args: vec!["hello".to_owned()],
            ..crate::run::configs::blank_config("hello", "hello", "program")
        };
        ide_run::save_user_configs(root, &[script]).expect("the configuration is saved");
    });
    assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_KEY)));
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    wait_long(cx, "the configurations", |cx| store.read_with(cx, |store, _| store.loaded));

    driver.update(cx, |driver, cx| driver.select_config(Some("hello"), cx));
    wait_long(cx, "the hammer to hide for a script", |cx| !hammer_shown(&workbench, cx));
    driver.update(cx, |driver, cx| driver.select_config(None, cx));
    wait_long(cx, "the hammer", |cx| hammer_shown(&workbench, cx));

    let ran = tools.join("ran");
    if ran.exists() {
        std::fs::remove_file(&ran).expect("the stand-in's mark is removed");
    }
    click_on(cx, "build-title-item");
    wait_long(cx, "the project's Maven to build", |_| ran.exists());
    close(cx);
}
