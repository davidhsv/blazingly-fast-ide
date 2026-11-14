use super::*;
use crate::workbench::agents::spawn_guard::AgentSpawnSeam;

/// The New Task page's Start needs a description; Enter starts the task once, a second Enter while it starts
/// starts nothing, and after a failed start the page says why and Start can be pressed again.
#[gpui_kit::test]
async fn disables_start_without_a_description_and_while_starting(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = agents_project(cx, home.path());
    // The seam answers every start with an error instead of starting the agent.
    let seam = AgentSpawnSeam::default();
    cx.update(|_, cx| cx.set_global(seam.clone()));
    let started = |seam: &AgentSpawnSeam| seam.started.lock().map(|started| started.clone()).unwrap_or_default();

    run_action(cx, "agents.newTask", None);
    wait_long(cx, "the New Task page", |cx| workbench.read_with(cx, |w, _| w.agents_home_page().is_some()));
    let page = workbench.read_with(cx, |w, _| w.agents_home_page()).expect("the New Task page shows");
    assert!(!page.read_with(cx, |page, cx| page.can_start(cx)), "no description");
    cx.update(|window, cx| window.focus(&page.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.simulate_input("Do the task");
    let typed = store.read_with(cx, |store, _| store.composer(crate::agents::store::HOME).text);
    assert_eq!(typed, "Do the task");
    assert!(page.read_with(cx, |page, cx| page.can_start(cx)));

    cx.simulate_keystrokes("enter enter");
    wait_long(cx, "the failed start", |cx| page.read_with(cx, |page, _| page.error().is_some()));
    assert_eq!(started(&seam), ["fake"], "one start for two presses");
    assert!(page.read_with(cx, |page, cx| page.can_start(cx)), "Start can be pressed again");
    assert!(cx.debug_bounds("agents-home-error").is_some(), "the page says why");
    assert!(store.read_with(cx, |store, _| store.tasks().is_empty()));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
