//! The action registry's error handling and listeners, `popupOpen`, every `toolwindow.<id>` action and the
//! title bar's project avatar, in a window.

use super::*;
use crate::platform::actions::{
    Action, ActionError, ActionMeta, ActionRegistry, ActionSource, execute_action, execute_action_from,
    execute_action_or_throw,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn context_on(cx: &mut VisualTestContext, key: &str) -> bool {
    cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
}

// src/platform/actions.test.ts › propagates host launch errors while retaining ordinary action error handling
/// A handler that fails hands its error to a caller that runs the action for its outcome, while the action run as
/// the user runs it counts as run (the failure is only logged). An action that is not registered is unavailable,
/// not failed.
#[gpui_kit::test]
async fn propagates_host_launch_errors_while_retaining_ordinary_action_error_handling(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    let runs = Rc::new(Cell::new(0_u32));
    let counted = runs.clone();
    cx.update(|_, cx| {
        let failing = Action::new(ActionMeta::new("test.hostLaunch", "Test"), move |_, _, _| {
            counted.set(counted.get() + 1);
            Err(anyhow::anyhow!("spawn failed"))
        });
        cx.global_mut::<ActionRegistry>().register(failing);
    });

    let thrown = cx.update(|window, cx| execute_action_or_throw("test.hostLaunch", None, window, cx));
    match thrown {
        Err(ActionError::Failed(error)) => assert_eq!(error.to_string(), "spawn failed"),
        other => panic!("the handler's error comes back: {other:?}"),
    }
    assert!(cx.update(|window, cx| execute_action("test.hostLaunch", None, window, cx)), "it counts as run");
    assert_eq!(runs.get(), 2, "the handler ran both times");

    let missing = cx.update(|window, cx| execute_action_or_throw("test.noSuchAction", None, window, cx));
    assert!(matches!(missing, Err(ActionError::Unavailable(_))), "{missing:?}");
    assert!(!cx.update(|window, cx| execute_action("test.noSuchAction", None, window, cx)));
}

/// The listeners of the registry hear the action the user runs, once and with how it was started; the action its
/// handler runs in turn is not told to them. Run by itself, that action is heard.
#[gpui_kit::test]
async fn listeners_hear_the_action_the_user_runs_and_not_the_ones_it_runs_itself(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    let inner_runs = Rc::new(Cell::new(0_u32));
    let counted = inner_runs.clone();
    cx.update(|_, cx| {
        let registry = cx.global_mut::<ActionRegistry>();
        registry.register(Action::new(ActionMeta::new("test.inner", "Inner"), move |_, _, _| {
            counted.set(counted.get() + 1);
            Ok(())
        }));
        registry.register(Action::new(ActionMeta::new("test.outer", "Outer"), |_, window, cx| {
            anyhow::ensure!(execute_action("test.inner", None, window, cx), "the inner action is available");
            Ok(())
        }));
    });
    let heard: Rc<RefCell<Vec<(String, ActionSource)>>> = Rc::default();
    let sink = heard.clone();
    let listener = cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>()
            .on_action_executed(move |id, _, source, _| sink.borrow_mut().push((id.to_owned(), source)))
    });

    assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "test.outer", None, window, cx)));
    assert_eq!(inner_runs.get(), 1, "the outer action ran the inner one");
    assert_eq!(heard.borrow().as_slice(), [("test.outer".to_owned(), ActionSource::Menu)]);

    assert!(cx.update(|window, cx| execute_action("test.inner", None, window, cx)));
    assert_eq!(inner_runs.get(), 2);
    assert_eq!(heard.borrow().last(), Some(&("test.inner".to_owned(), ActionSource::Other)));
    assert_eq!(heard.borrow().len(), 2);
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
}

/// A menu opened over the window (the status bar's line separator menu) sets `popupOpen`; Escape closes it and
/// clears the key.
#[gpui_kit::test]
async fn a_menu_over_the_window_sets_popup_open_until_escape_closes_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    assert!(!context_on(cx, "popupOpen"), "no popup is open yet");

    click_on(cx, "status:editor.lineEnding");
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<StatusCommand>()).is_some(), "the menu is open");
    assert!(context_on(cx, "popupOpen"));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<StatusCommand>()).is_none(), "Escape closes the menu");
    assert!(!context_on(cx, "popupOpen"));
    close_backend_project(cx);
}

// src/workbench/toolwindows/store.test.ts › registers toolwindow.<id> and generic actions
/// Every tool window the app draws has its `toolwindow.<id>` action, titled as the tool window is declared: run
/// through the registry it shows that window, and the window's context says it shows. Hide All Tool Windows hides
/// what shows.
#[gpui_kit::test]
async fn registers_toolwindow_id_and_generic_actions(cx: &mut TestAppContext) {
    let _home = start(cx);
    // Trusted: the Terminal it shows starts the isolated plain shell.
    let (_folder, _root, workbench, cx) = search_project_with(cx, true);
    let visible = |cx: &mut VisualTestContext, id: &str| workbench.read_with(cx, |w, _| w.is_tool_window_visible(id));
    // The Project view opens shown and focused, where its action would hide it: everything is hidden first.
    run_action(cx, "toolwindow.hideAll", None);
    assert!(!visible(cx, PROJECT_TOOL_WINDOW), "Hide All Tool Windows hides the Project view");

    for id in TOOL_WINDOWS {
        let action = format!("toolwindow.{id}");
        let declared = tool_window_declaration(id).unwrap_or_else(|| panic!("{id} is declared"));
        let title = cx.update(|_, cx| cx.global::<ActionRegistry>().get(&action).map(|a| a.meta.title.clone()));
        assert_eq!(title.as_deref(), Some(declared.title.as_str()), "{action} is registered");
        run_action(cx, &action, None);
        settle(cx, |cx| visible(cx, id));
        assert!(context_on(cx, &format!("toolWindow.{id}.visible")), "{id} is visible to the when clauses");
    }

    run_action(cx, "toolwindow.hideAll", None);
    for id in TOOL_WINDOWS {
        assert!(!visible(cx, id), "{id} is hidden");
    }
    close_backend_project(cx);
}

/// The title bar's project widget draws its avatar on the colours of the project's colour slot in the theme shown:
/// the slot of the project's own name, which the theme defines and which differs from another slot's.
#[gpui_kit::test]
async fn the_title_bars_avatar_is_drawn_on_the_colours_of_the_projects_slot(cx: &mut TestAppContext) {
    use crate::workbench::project_color::{avatar_fill, project_color_index, project_colors};
    let _home = start(cx);
    let first = "alpha";
    let second = ["beta", "gamma", "delta", "epsilon", "zeta", "theta", "kappa"]
        .into_iter()
        .find(|name| project_color_index(name) != project_color_index(first))
        .expect("a name of another colour slot");
    let parent = tempfile::tempdir().expect("a temporary folder");
    let parent_root = parent.path().canonicalize().expect("the folder exists");
    std::fs::create_dir(parent_root.join(first)).expect("the project folder is made");
    std::fs::write(parent_root.join(first).join("a.txt"), "a\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    let drawn =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.title_bar().read(cx).project_avatar_fill(cx));
    let slot = |name: &str, cx: &mut VisualTestContext| cx.update(|_, cx| avatar_fill(name, cx));

    workbench.update_in(cx, |w, window, cx| w.open_project(parent_root.join(first), window, cx));
    settle(cx, |cx| drawn(cx).is_some());
    let theme_has_slot = cx.update(|_, cx| project_colors(first, &cx.global::<theme::ActiveTheme>().theme).start);
    assert!(theme_has_slot.is_some(), "the theme defines the colour of the project's slot");
    assert_eq!(drawn(cx), Some(slot(first, cx)));
    assert_ne!(slot(first, cx), slot(second, cx), "the two slots have their own colours");
    assert!(cx.debug_bounds("title:project").is_some(), "the project widget is drawn");
    close_backend_project(cx);
}
