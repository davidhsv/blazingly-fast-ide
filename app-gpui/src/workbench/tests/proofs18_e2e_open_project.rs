use super::proofs18_e2e_fixtures::{click_at, double_shift};
use super::*;

/// The trust store's answer for `path`: (decided, trusted).
fn trust_of(path: &std::path::Path, cx: &mut VisualTestContext) -> (bool, bool) {
    let status = cx.update(|_, cx| cx.global::<Backend>().host.trust.status(path)).expect("the status reads");
    (status.decided, status.trusted)
}

/// The folder the window's trust question asks about, while one is shown.
fn asked_about(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let dialog = workbench.read_with(cx, |w, _| w.trust_dialog())?;
    Some(dialog.read_with(cx, |dialog, _| dialog.path().to_owned()))
}

/// The names the Welcome screen lists, while it shows.
fn welcome_names(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    let welcome = workbench.read_with(cx, |w, _| w.welcome().cloned())?;
    Some(welcome.read_with(cx, |view, _| view.shown_projects().into_iter().map(|project| project.name).collect()))
}

/// Close Project found with double Shift in Search Everywhere closes the project: the Project view is
/// gone and the Welcome screen lists the recent projects. A click on `billing`, which nobody decided
/// about, asks "Trust and Open Project?" and opens nothing yet; Trust Project trusts that folder alone
/// and opens it, and its Project view lists its files.
#[gpui_kit::test]
async fn close_project_shows_the_welcome_screen_a_recent_project_opens_from_it_once_trusted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let projects = tempfile::tempdir().expect("a temporary folder");
    let parent = projects.path().canonicalize().expect("the folder exists");
    let (acme, billing) = (parent.join("acme-api"), parent.join("billing"));
    std::fs::create_dir_all(acme.join("src")).expect("the folder is made");
    std::fs::write(acme.join("build.gradle.kts"), "plugins {}\n").expect("the file is written");
    std::fs::create_dir_all(billing.join("src")).expect("the folder is made");
    std::fs::write(billing.join("package.json"), "{}\n").expect("the file is written");
    std::fs::write(billing.join("README.md"), "# Billing\n").expect("the file is written");
    let config = cx.update(|cx| cx.global::<Backend>().dirs.config.clone());
    ide_workspace::RecentProjects::load(&config).touch(&billing, Some("main".to_owned())).expect("billing is recent");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&acme, true, false)).expect("acme-api is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(acme.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));

    double_shift(cx);
    let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("double Shift opens the popup");
    let close = action_title(cx, "file.closeProject");
    type_query(cx, "Close Project");
    // All lists its results under section titles: Enter takes the selected one.
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_ref() == Some(&close)));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| welcome_names(&workbench, cx).is_some());
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_none()), "the Project view is gone");
    assert!(cx.update(|_, cx| cx.global::<Backend>().host.project("w1").is_none()), "the backend closed it");
    let names = welcome_names(&workbench, cx).expect("the Welcome screen shows");
    assert!(names.contains(&"billing".to_owned()) && names.contains(&"acme-api".to_owned()), "{names:?}");

    // A project not yet trusted asks first.
    let index = names.iter().position(|name| name == "billing").expect("billing is listed");
    click_at(cx, format!("welcome-row-{index}"));
    wait_long(cx, "the question is shown", |cx| asked_about(&workbench, cx).is_some());
    assert_eq!(asked_about(&workbench, cx).as_deref(), Some(&*billing.to_string_lossy()));
    assert!(workbench.read_with(cx, |w, cx| w.project_root(cx)).is_none(), "nothing opens before the answer");
    assert!(cx.update(|_, cx| cx.global::<Backend>().host.project("w1").is_none()));

    click_on(cx, "trust-button-project.trust");
    wait_long(cx, "billing opens", |cx| {
        workbench.read_with(cx, |w, cx| w.project_root(cx)).as_deref() == Some(&*billing)
    });
    assert!(asked_about(&workbench, cx).is_none(), "the question is closed");
    assert_eq!(trust_of(&billing, cx), (true, true));
    let decisions = cx.update(|_, cx| cx.global::<Backend>().host.trust.decisions()).expect("the decisions read");
    assert!(!decisions.locations.contains(&parent), "the parent folder is not trusted: {:?}", decisions.locations);
    let tree = project_tree_of(&workbench, cx);
    settle(cx, |cx| tree.read_with(cx, |tree, _| tree.is_loaded(tree.root())));
    let root_label = tree.read_with(cx, |tree, _| tree.rows().first().map(|row| row.node.label.clone()));
    assert!(root_label.is_some_and(|label| label.starts_with("billing")), "the tree shows billing");
    settle(cx, |cx| tree_labels(&tree, cx).contains(&"package.json".to_owned()));
    close_backend_project(cx);
}
