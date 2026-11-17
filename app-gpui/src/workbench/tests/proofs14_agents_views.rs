//! The New Task page through the window: the commands the project has for its agent, the settings the agent
//! reported in its last session, and the agent menu.

use super::proofs13_agents_fixtures::*;
use super::*;
use crate::agents::new_task_view::NewTaskView;
use serde_json::json;

/// [`IMAGE_AGENT`], offering the command `/triage` in every session it opens.
fn commands_agent() -> String {
    let answer = r#"        answer(request, {"sessionId": "image-session", "configOptions": config})"#;
    assert!(IMAGE_AGENT.contains(answer), "the agent answers session/new on one line");
    let offer = r#"
        triage = {"name": "triage", "description": "One pass over open issues"}
        update = {"sessionUpdate": "available_commands_update", "availableCommands": [triage]}
        send({"jsonrpc": "2.0", "method": "session/update",
              "params": {"sessionId": "image-session", "update": update}})"#;
    IMAGE_AGENT.replace(answer, &format!("{answer}{offer}"))
}

/// Opens the New Task page with New Agent Task and gives its description the keyboard.
fn open_new_task(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<NewTaskView> {
    run_action(cx, "agents.newTask", None);
    wait_long(cx, "the New Task page", |cx| workbench.read_with(cx, |w, _| w.agents_home_page().is_some()));
    let page = workbench.read_with(cx, |w, _| w.agents_home_page()).expect("the New Task page shows");
    let focus = page.read_with(cx, |page, cx| page.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    page
}

/// After a task whose agent offered `/triage`, the New Task page completes it: typing `/tri` lists it under
/// Commands, and Enter puts it in the description.
#[gpui_kit::test]
async fn completes_the_commands_the_agent_offered_in_the_project(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = image_project(cx, home.path(), None);
    std::fs::write(home.path().join("fake_agent.py"), commands_agent()).expect("the agent script is written");
    let task = start_with(&workbench, &store, "Look", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let page = open_new_task(&workbench, cx);
    cx.simulate_input("/tri");
    let rows = |cx: &mut VisualTestContext| page.read_with(cx, |page, cx| page.command_rows(cx));
    wait_long(cx, "the agent's commands", |cx| rows(cx).is_some());
    assert_eq!(rows(cx), Some(vec!["Commands".to_owned(), "/triage · One pass over open issues *".to_owned()]));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(store.read_with(cx, |store, _| store.composer(crate::agents::store::HOME).text), "/triage ");
    assert_eq!(store.read_with(cx, |store, _| store.tasks().len()), 1, "Enter completed instead of starting");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// The settings the agent reported in its first task are chips on the New Task page; a model picked there is the
/// model of the next task's session.
#[gpui_kit::test]
async fn starts_the_next_task_with_the_model_picked_on_the_new_task_page(cx: &mut TestAppContext) {
    let home = start(cx);
    let model = json!({
        "id": "model",
        "name": "Model",
        "category": "model",
        "type": "select",
        "currentValue": "claude-sonnet-5.5",
        "options": [
            { "value": "claude-sonnet-5.5", "name": "Claude Sonnet 5.5" },
            { "value": "gpt-6-sol", "name": "GPT-6 Sol" },
        ],
    });
    let (folder, workbench, store, cx) = image_project(cx, home.path(), Some(json!([model])));
    let first = start_with(&workbench, &store, "Look", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &first, "agent: Answer 1", cx);
    let page = open_new_task(&workbench, cx);
    let chips = |cx: &mut VisualTestContext| page.read_with(cx, |page, cx| page.chips(cx));
    wait_long(cx, "the agent's settings", |cx| chips(cx) == ["Claude Sonnet 5.5"]);
    click_on(cx, "composer-config-0");
    let entries = page.read_with(cx, |page, _| page.picker_entries()).expect("the model list opens");
    assert_eq!(entries, ["Claude Sonnet 5.5 *", "GPT-6 Sol"]);
    click_on(cx, "composer-picker-1");
    assert_eq!(chips(cx), ["GPT-6 Sol"]);
    let focus = page.read_with(cx, |page, cx| page.focus_handle(cx));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_input("Second task");
    click_on(cx, "agents-home-start");
    wait_long(cx, "the second task", |cx| task_ids(&store, cx).len() == 2);
    wait_long(cx, "the picked model", |_| agent_settings(&folder).iter().any(|given| given == "model=gpt-6-sol"));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// The agent menu lists the installed agents before the ones that are not installed and ends with More agents…,
/// which opens Settings on the Agents page.
#[gpui_kit::test]
async fn lists_the_agents_and_more_agents_in_the_new_task_agent_menu(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, _store, cx) = image_project(cx, home.path(), None);
    open_new_task(&workbench, cx);
    let at = cx.debug_bounds("agents-home-agent").expect("the agent chooser is drawn").center();
    cx.simulate_mouse_down(at, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(at, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<String>()).expect("the agent menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines()).remove(0);
    let plain: Vec<&str> = lines.iter().map(|line| line.trim_start_matches("> ")).collect();
    // A built-in agent found on the machine's PATH is installed too, so the fake one is not always first.
    let fake = plain.iter().position(|line| *line == "Fake Agent").expect("the installed fake agent is listed");
    let missing = plain.iter().position(|line| *line == "Not installed").unwrap_or(plain.len());
    assert!(fake < missing, "the installed agents come first: {plain:?}");
    let more = plain.iter().position(|line| *line == "More agents…").expect("More agents… is listed");
    assert_eq!(more, plain.len() - 1, "{plain:?}");
    click_on(cx, leak(format!("menu-row-0-{more}")));
    wait_long(cx, "the Settings dialog", |cx| settings_dialog_of(&workbench, cx).is_some());
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("agents.list"));
    click_on(cx, "settings-cancel");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
