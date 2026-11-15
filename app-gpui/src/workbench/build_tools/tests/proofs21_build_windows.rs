//! The Maven tool window coming and going with the project's POM, Reload of one chosen Gradle build, and a build
//! file picked for Link Gradle Projects after the window moved on to another project.

use super::*;
use crate::workbench::tool_windows::{Anchor, Stripe};

/// Tells the window that `paths` of its project were removed, as the file watcher does.
pub(super) fn removed(paths: Vec<PathBuf>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let removed = paths.into_iter().map(Into::into).collect();
        let event = IdeEvent::FsChanged { created: Vec::new(), modified: Vec::new(), removed };
        cx.global::<Backend>().emit(Some("w1"), event);
    });
}

/// A project with no POM has no Maven tool window. A `pom.xml` written brings it, as declared: "Maven", its icon,
/// docked on the right with its stripe button drawn there, and `build.maven` on. The POM removed while the window
/// shows, the window, its stripe button and its view go, and `build.maven` is off again.
#[gpui_kit::test]
async fn registers_the_maven_tool_window_only_while_the_project_has_a_pom(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_settings);
    assert!(!tool_window_offered(&workbench, MAVEN_TOOL_WINDOW, cx), "no POM, no Maven window");
    assert!(!key_on(cx, MAVEN_KEY));
    assert!(cx.debug_bounds("stripe:maven").is_none());

    write(&root.join("pom.xml"), &pom("solo", "<packaging>jar</packaging><name>Solo</name>"));
    created(vec![root.join("pom.xml")], cx);
    wait_long(cx, "the Maven model", |cx| key_on(cx, MAVEN_KEY));
    let declared = workbench
        .read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.window(MAVEN_TOOL_WINDOW).cloned()))
        .expect("the Maven window is registered");
    assert_eq!(
        (declared.title.as_str(), declared.icon.as_str(), declared.anchor),
        ("Maven", "toolwindow-maven", Anchor::Right)
    );
    assert_eq!(declared.stripe, Stripe::Shown, "its stripe button is there before it is first opened");
    let docked =
        workbench.read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.anchor_of(MAVEN_TOOL_WINDOW)));
    assert_eq!(docked, Some(Anchor::Right));
    wait_long(cx, "its stripe button", |cx| cx.debug_bounds("stripe:maven").is_some());
    let button = cx.debug_bounds("stripe:maven").expect("the stripe button is drawn");
    let width = cx.update(|window, _| window.viewport_size().width);
    assert!(f32::from(button.center().x) > f32::from(width) / 2.0, "the button is on the right stripe");
    assert!(!tool_window_offered(&workbench, GRADLE_TOOL_WINDOW, cx), "a POM brings no Gradle window");
    show(&workbench, BuildTool::Maven, cx);

    std::fs::remove_file(root.join("pom.xml")).expect("the POM is removed");
    removed(vec![root.join("pom.xml")], cx);
    wait_long(cx, "the Maven model to go", |cx| !key_on(cx, MAVEN_KEY));
    cx.run_until_parked();
    assert!(!tool_window_offered(&workbench, MAVEN_TOOL_WINDOW, cx), "the Maven window goes with the POM");
    assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(MAVEN_TOOL_WINDOW)));
    assert!(cx.debug_bounds("stripe:maven").is_none(), "its stripe button goes");
    assert!(workbench.read_with(cx, |w, _| w.build_tool_view(BuildTool::Maven).is_none()), "its view goes");
    close(cx);
}

/// A Gradle project with a second build `other` (with a subproject `app` as the first has) linked next to it.
fn two_builds(root: &Path, tools: &Path) {
    gradle_project(root, tools);
    write(&root.join("libs/other/settings.gradle"), "rootProject.name = 'other'\ninclude 'app'\n");
    write(&root.join("libs/other/build.gradle"), "// the other root build\n");
    write(&root.join("libs/other/app/build.gradle"), "plugins { id 'java' }\n");
    // Its own wrapper: without one, its launcher would be looked for in the user's settings.
    executable(&root.join("libs/other/gradlew"), FAKE_GRADLEW);
}

/// Links `libs/other` through Link Gradle Projects and waits for the tree to show it.
fn link_other(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Entity<BuildToolView> {
    run_action(cx, "gradle.linkProjects", None);
    wait_long(cx, "the build file is asked for", |cx| cx.did_prompt_for_paths());
    let picked = root.join("libs/other/settings.gradle");
    cx.simulate_path_prompt_response(move |_| Some(vec![picked.clone()]));
    let view = workbench.read_with(cx, |w, _| w.build_tool_view(BuildTool::Gradle)).expect("the Gradle window");
    wait_long(cx, "the linked build", |cx| labels(&view, cx).contains(&"other".to_owned()));
    view
}

/// Each build of the model, by folder, with whether Gradle has reported it.
fn synced(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench.read_with(cx, |w, _| {
        w.build_model()
            .map(|m| m.gradle.iter().map(|g| (g.dir.clone(), g.synced_at.is_some())).collect())
            .unwrap_or_default()
    })
}

/// Reload Gradle Project on a linked build's root reloads that build only: Gradle reports its tasks, and the
/// project's own build is left as its scripts describe it.
#[gpui_kit::test]
async fn routes_reload_to_the_chosen_independent_build(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, two_builds);
    let view = link_other(&workbench, &root, cx);
    let other = rows(&view, cx).into_iter().find(|row| row.label == "other").expect("the linked build's root");
    let NodeKind::GradleProject { build, .. } = &other.node.kind else { panic!("a Gradle project") };
    let build = build.clone();
    let menu = view.read_with(cx, |view, cx| view.context_menu(&other.node.key, cx));
    let reload = menu.iter().find(|item| item.action_id() == Some("gradle.reload")).expect("the root's menu reloads");
    assert_eq!(reload.label(), Some("Reload Gradle Project"));
    let arg = reload.arg().cloned().expect("the reload names its build");
    assert_eq!(arg, json!({ "buildDir": build }));
    let before = synced(&workbench, cx);
    assert_eq!(before.len(), 2, "{before:?}");
    assert!(before.iter().all(|(_, reported)| !reported), "nothing is reported before a sync: {before:?}");

    run_action(cx, "gradle.reload", Some(arg));
    wait_long(cx, "the chosen build's sync", |cx| {
        !workbench.read_with(cx, |w, _| w.build_sync_running(BuildTool::Gradle))
            && synced(&workbench, cx).iter().any(|(dir, reported)| *dir == build && *reported)
    });
    let after = synced(&workbench, cx);
    assert!(after.iter().filter(|(dir, _)| *dir != build).all(|(_, reported)| !reported), "{after:?}");
    let titles = notification_titles(&workbench, cx);
    assert!(!titles.iter().any(|t| t.contains("Gradle")), "nothing went wrong: {titles:?}");
    close(cx);
}

/// Link Gradle Projects asks for a build file; the window moves on to another project before one is picked. The
/// pick then links nothing: the second project's settings and builds do not get the first project's build, and
/// linking a build in the second project afterwards adds only that build.
#[gpui_kit::test]
async fn a_build_picked_after_the_window_changed_projects_is_linked_nowhere(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, first, tools) = build_project(cx, true, two_builds);
    run_action(cx, "gradle.linkProjects", None);
    wait_long(cx, "the build file is asked for", |cx| cx.did_prompt_for_paths());

    let second_folder = tempfile::tempdir().expect("a temporary folder");
    let second = second_folder.path().canonicalize().expect("the folder exists");
    project_settings(&second, &tools, "");
    let extern_folder = tempfile::tempdir().expect("a temporary folder");
    let external = extern_folder.path().canonicalize().expect("the folder exists");
    write(&external.join("settings.gradle"), "rootProject.name = 'external'\n");
    write(&external.join("build.gradle"), "plugins { id 'java' }\n");
    executable(&external.join("gradlew"), FAKE_GRADLEW);
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&second, true, false)).expect("the project is trusted");
    workbench.update_in(cx, |w, window, cx| w.open_project(second.clone(), window, cx));
    wait_long(cx, "the second project", |cx| {
        workbench
            .read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(second.as_path()) && w.build_model_answered())
    });

    let stale = first.join("libs/other/settings.gradle");
    cx.simulate_path_prompt_response(move |_| Some(vec![stale.clone()]));
    cx.run_until_parked();

    // The user links a build in the second project: its answer is the end of every link asked for so far.
    run_action(cx, "gradle.linkProjects", None);
    wait_long(cx, "the build file is asked for again", |cx| cx.did_prompt_for_paths());
    let picked = external.join("settings.gradle");
    cx.simulate_path_prompt_response(move |_| Some(vec![picked.clone()]));
    wait_long(cx, "the external build", |cx| {
        workbench.read_with(cx, |w, _| {
            w.build_model().is_some_and(|m| m.gradle.iter().any(|g| Path::new(&g.dir) == external))
        })
    });
    let first_text = first.to_string_lossy().into_owned();
    let dirs: Vec<String> = workbench
        .read_with(cx, |w, _| w.build_model().map(|m| m.gradle.iter().map(|g| g.dir.clone()).collect()))
        .unwrap_or_default();
    assert!(!dirs.iter().any(|dir| dir.contains(&first_text)), "the first project's build is not linked: {dirs:?}");
    let settings = std::fs::read_to_string(second.join(".openidea/settings.toml")).unwrap_or_default();
    assert!(!settings.contains(&first_text), "the second project's settings name no build of the first: {settings}");
    let titles = notification_titles(&workbench, cx);
    assert!(!titles.iter().any(|t| t.contains("Cannot link")), "{titles:?}");
    close(cx);
}
