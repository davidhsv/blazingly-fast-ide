use super::*;
use std::cell::Cell;
use std::rc::Rc;

/// Search Everywhere on All with nothing typed lists the recently opened files under their
/// header; once a query is typed the results are one flat list, without section headers.
#[gpui_kit::test]
async fn shows_recent_files_for_an_empty_query_then_flat_results(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let alpha = root.join("src/alpha.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == alpha.as_path())));

    run_action(cx, "navigate.searchEverywhere", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::All);
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| !popup.is_loading() && !popup.lines().is_empty()));
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.lines()), ["[Recent Files]", "src/alpha.rs"]);

    type_query(cx, "alph");
    settle(cx, |cx| {
        everywhere.read_with(cx, |popup, _| !popup.is_loading() && popup.lines().contains(&"src/alpha.rs".to_owned()))
    });
    let lines = everywhere.read_with(cx, |popup, _| popup.lines());
    assert!(lines.iter().all(|line| !line.starts_with('[')), "All mixes its results without headers: {lines:?}");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Tab moves from All to Classes and Shift+Tab goes back round to Text. Search Everywhere asked
/// for again while the popup is on another tab moves it to All and keeps what was typed.
#[gpui_kit::test]
async fn tab_cycles_scopes_and_repeating_the_scope_action_moves_to_all_keeping_the_query(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    run_action(cx, "navigate.searchEverywhere", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::All);
    cx.simulate_keystrokes("tab");
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Classes);
    cx.simulate_keystrokes("shift-tab shift-tab");
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Text);

    type_query(cx, "kept");
    run_action(cx, "navigate.searchEverywhere", None);
    assert_eq!(popup(&workbench, cx), everywhere, "the open popup is the one that moved");
    assert_eq!(
        everywhere.read_with(cx, |popup, cx| (popup.tab(), popup.field().read(cx).text().to_owned())),
        (SeTab::All, "kept".to_owned())
    );
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Only an action registered with a state reports it: the toggle reads off, runs from Find
/// Action, reads on and leaves the popup open where its new state shows; the plain action
/// reports no state and closes the popup when it runs.
#[gpui_kit::test]
async fn reports_state_only_for_actions_with_toggled(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    let on = Rc::new(Cell::new(false));
    let plain_runs = Rc::new(Cell::new(0_u32));
    cx.update(|_, cx| {
        let (read, flip, count) = (on.clone(), on.clone(), plain_runs.clone());
        let registry = cx.global_mut::<ActionRegistry>();
        registry.register(
            Action::new(ActionMeta::new("test.zymurgyToggle", "Zymurgy Switch"), move |_, _, _| {
                flip.set(!flip.get());
                Ok(())
            })
            .toggled(move |_, _| read.get()),
        );
        registry.register(Action::new(ActionMeta::new("test.zymurgyPlain", "Zymurgy Plain"), move |_, _, _| {
            count.set(count.get() + 1);
            Ok(())
        }));
    });
    let state = |cx: &mut VisualTestContext, id: &str| {
        cx.update(|window, cx| {
            let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
            crate::platform::actions::action_toggled(&action, window, cx)
        })
    };
    assert_eq!(state(cx, "test.zymurgyToggle"), Some(false));
    assert_eq!(state(cx, "test.zymurgyPlain"), None);

    run_action(cx, "navigate.findAction", None);
    let everywhere = popup(&workbench, cx);
    type_query(cx, "Zymurgy Switch");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_deref() == Some("Zymurgy Switch")));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| state(cx, "test.zymurgyToggle") == Some(true));
    assert!(on.get());
    assert_eq!(workbench.read_with(cx, |w, _| w.search_everywhere()), Some(everywhere.clone()));

    everywhere.update(cx, |popup, cx| popup.field().update(cx, |field, cx| field.select_all(cx)));
    type_query(cx, "Zymurgy Plain");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_deref() == Some("Zymurgy Plain")));
    cx.simulate_keystrokes("enter");
    settle(cx, |_| plain_runs.get() == 1);
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "a plain action closes the popup");
    assert_eq!(state(cx, "test.zymurgyPlain"), None);
    assert_eq!(state(cx, "test.zymurgyToggle"), Some(true));
    close_backend_project(cx);
}
