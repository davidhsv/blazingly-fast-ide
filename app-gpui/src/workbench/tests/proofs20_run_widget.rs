//! The title bar's run widget while its configuration runs: Rerun starts it again, Stop ends it, and
//! More Actions lists the run actions and runs the one chosen. Run itself is offered only while a
//! project is open.

use super::*;
use crate::platform::actions::{ActionError, ActionRegistry, execute_action_or_throw, is_action_enabled};
use crate::platform::context::window_context;
use crate::run::widget_view::RunWidget;

/// The widget's buttons as drawn: (debug selector, title, enabled).
fn widget_buttons(widget: &Entity<RunWidget>, cx: &mut VisualTestContext) -> Vec<(&'static str, String, bool)> {
    let buttons = widget.read_with(cx, |widget, cx| widget.buttons(cx));
    buttons.into_iter().map(|button| (button.id, button.title, button.enabled)).collect()
}

/// The live sessions of configuration `name`, by when they started.
fn live_starts(store: &Entity<RunStore>, name: &str, cx: &mut VisualTestContext) -> Vec<f64> {
    store.read_with(cx, |store, _| {
        store.sessions.iter().filter(|s| s.config.name == name && s.is_live()).map(|s| s.started_at).collect()
    })
}

/// Whether the condition of the action `id` holds in the window.
fn action_enabled(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, cx| {
        let action = cx.global::<ActionRegistry>().get(id).unwrap_or_else(|| panic!("{id} is registered"));
        is_action_enabled(&action.meta, &window_context(window, cx))
    })
}

/// While the selected configuration runs, the widget offers Rerun and Stop for it: Rerun ends the live
/// process and starts the configuration again, Stop ends it and the widget offers Run again. More
/// Actions lists Run and Debug Context Configuration, Edit Configurations… and Reload Run
/// Configurations, and its Edit Configurations… row opens Edit Configurations.
#[gpui_kit::test]
async fn the_run_widgets_rerun_stop_and_more_actions_restart_stop_and_offer_the_run_actions(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("loop", "/bin/sleep", "30")]);
    let widget = workbench.read_with(cx, |w, _| w.run_widget().cloned()).expect("the title bar has the widget");
    assert_eq!(store.read_with(cx, |store, _| store.selected_id.clone()).as_deref(), Some("loop"));

    click_on(cx, "run-widget-run");
    let running = |cx: &mut VisualTestContext| {
        live_starts(&store, "loop", cx).len() == 1
            && widget_buttons(&widget, cx).iter().any(|(id, _, enabled)| *id == "run-widget-rerun" && *enabled)
    };
    wait_long(cx, "the run and its Rerun", running);
    let buttons = widget_buttons(&widget, cx);
    let titles: Vec<&str> = buttons.iter().map(|(_, title, _)| title.as_str()).collect();
    assert_eq!(titles, ["Rerun 'loop'", "Stop 'loop'", "More Actions"], "{buttons:?}");
    let first = live_starts(&store, "loop", cx)[0];

    click_on(cx, "run-widget-rerun");
    wait_long(cx, "the second run", |cx| {
        let starts = live_starts(&store, "loop", cx);
        starts.len() == 1 && starts[0] > first
    });

    click_on(cx, "run-widget-stop");
    wait_long(cx, "the end of the run", |cx| live_starts(&store, "loop", cx).is_empty());
    wait_long(cx, "Run offered again", |cx| {
        let buttons = widget_buttons(&widget, cx);
        buttons.iter().any(|(id, _, enabled)| *id == "run-widget-run" && *enabled)
            && !buttons.iter().any(|(id, _, _)| *id == "run-widget-stop" || *id == "run-widget-rerun")
    });

    click_on(cx, "run-widget-more");
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<std::convert::Infallible>()).expect("the menu is open");
    let rows: Vec<String> = menu.read_with(cx, |menu, _| menu.lines())[0]
        .iter()
        .map(|line| line.trim_start_matches("> ").to_owned())
        .collect();
    let expected = [
        "Run Context Configuration",
        "Debug Context Configuration",
        "-",
        "Edit Configurations…",
        "Reload Run Configurations",
    ];
    assert_eq!(rows, expected);
    let index = rows.iter().position(|row| row == "Edit Configurations…").expect("the row is listed");
    click_on(cx, Box::leak(format!("menu-row-0-{index}").into_boxed_str()));
    wait_long(cx, "Edit Configurations", |cx| workbench.read_with(cx, |w, _| w.edit_configs_dialog().is_some()));
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<std::convert::Infallible>()).is_none(), "the menu closed");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Run is offered while a project is open and runs the selected configuration; once the project is
/// closed it is not, and running it is refused.
#[gpui_kit::test]
async fn run_runs_the_selection_with_a_project_open_and_is_unavailable_without_one(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("hello", "/bin/echo", "hello")]);
    assert!(context_is_on(cx, "projectOpen"));
    assert!(action_enabled(cx, "run.run"));
    run_action(cx, "run.run", None);
    let ended = |cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| {
            store.sessions.iter().find(|s| s.config.name == "hello").map(|s| (s.is_live(), s.exit_code))
        })
    };
    wait_long(cx, "the exit of echo", |cx| ended(cx).is_some_and(|(live, _)| !live));
    assert_eq!(ended(cx), Some((false, Some(0))));

    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    wait_long(cx, "the project to close", |cx| !context_is_on(cx, "projectOpen"));
    assert!(!action_enabled(cx, "run.run"), "nothing to run without a project");
    let ran = cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx));
    assert!(matches!(ran, Err(ActionError::Unavailable(_))), "Run is refused without a project");
}
