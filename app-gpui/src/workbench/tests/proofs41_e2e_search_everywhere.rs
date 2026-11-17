use super::*;
use crate::everywhere::store::{HISTORY_KEY, RECENT_ACTIONS_KEY, recent_actions, search_history};
use std::cell::RefCell;
use std::rc::Rc;

/// The value stored under `key`.
fn stored(cx: &mut VisualTestContext, key: &str) -> Option<String> {
    cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned))
}

/// The text of the Search Everywhere field.
fn query(everywhere: &Entity<SearchEverywhere>, cx: &mut VisualTestContext) -> String {
    everywhere.read_with(cx, |popup, cx| popup.field().read(cx).text().to_owned())
}

/// Selects the field's text, so that what is typed next replaces it.
fn select_query(everywhere: &Entity<SearchEverywhere>, cx: &mut VisualTestContext) {
    everywhere.update(cx, |popup, cx| popup.field().update(cx, |field, cx| field.select_all(cx)));
}

/// The arguments an action was run with, one entry per run.
type Calls = Rc<RefCell<Vec<Option<serde_json::Value>>>>;

/// Registers an action that only records the arguments it is run with.
fn recording_action(cx: &mut VisualTestContext, meta: ActionMeta) -> Calls {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&calls);
    cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().register(Action::new(meta, move |call, _, _| {
            seen.borrow_mut().push(call.arg.cloned());
            Ok(())
        }));
    });
    calls
}

/// ⌥↑ walks back through the queries typed in the tab, newest first, and stops at the oldest; ⌥↓ walks
/// forward and past the newest gives back what was typed. Without a selection the popup starts with the
/// tab's last query, and closing it remembers the query for the tab.
#[gpui_kit::test]
async fn alt_up_and_alt_down_browse_the_tab_s_search_history_and_restore_the_draft(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    let history = serde_json::json!([
        { "tab": "files", "query": "other tab" },
        { "tab": "all", "query": "newer" },
        { "tab": "all", "query": "older" },
    ]);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(HISTORY_KEY, history.to_string()));

    run_action(cx, "navigate.searchEverywhere", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(query(&everywhere, cx), "newer", "the tab's last query comes back, selected");
    type_query(cx, "draft");
    assert_eq!(query(&everywhere, cx), "draft");
    cx.simulate_keystrokes("alt-up");
    assert_eq!(query(&everywhere, cx), "newer");
    cx.simulate_keystrokes("alt-up alt-up");
    assert_eq!(query(&everywhere, cx), "older");
    cx.simulate_keystrokes("alt-down alt-down");
    assert_eq!(query(&everywhere, cx), "draft");
    let all = |cx: &mut VisualTestContext| search_history(stored(cx, HISTORY_KEY).as_deref(), SeTab::All);
    assert_eq!(all(cx), ["newer", "older"], "walking the history changes nothing in it");

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
    assert_eq!(all(cx), ["draft", "newer", "older"], "closing remembers the query of the tab");
    let files = search_history(stored(cx, HISTORY_KEY).as_deref(), SeTab::Files);
    assert_eq!(files, ["other tab"]);
    close_backend_project(cx);
}

/// Tab moves to Classes and Shift+Tab twice goes round to Text. Search Everywhere asked for on another
/// tab moves the popup to All and keeps the query, with project items only; asked for again on All, it
/// includes the non-project items.
#[gpui_kit::test]
async fn tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    run_action(cx, "navigate.searchEverywhere", None);
    let everywhere = popup(&workbench, cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Classes);
    cx.simulate_keystrokes("shift-tab shift-tab");
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Text);

    type_query(cx, "kept");
    run_action(cx, "navigate.searchEverywhere", None);
    assert_eq!(popup(&workbench, cx), everywhere);
    assert_eq!(everywhere.read_with(cx, |popup, _| (popup.tab(), popup.include_non_project())), (SeTab::All, false));
    assert_eq!(query(&everywhere, cx), "kept");
    run_action(cx, "navigate.searchEverywhere", None);
    assert!(everywhere.read_with(cx, |popup, _| popup.include_non_project()), "the shown tab's action widens it");
    assert_eq!(query(&everywhere, cx), "kept");
    assert!(cx.debug_bounds("search-everywhere-scope").is_some(), "the scope check box is drawn");
    run_action(cx, "navigate.searchEverywhere", None);
    assert!(!everywhere.read_with(cx, |popup, _| popup.include_non_project()), "and again narrows it");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// Find Action's check box is Include disabled actions. ⌥↵ on an action closes the popup and records a
/// shortcut for that action.
#[gpui_kit::test]
async fn labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    let pizza = recording_action(cx, ActionMeta::new("test.pizza", "Order Pizza").category("Test"));
    let hidden = ActionMeta { hidden: true, ..ActionMeta::new("settings.assignShortcut", "Assign") };
    let assign = recording_action(cx, hidden);

    run_action(cx, "navigate.findAction", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Actions);
    assert_eq!(crate::everywhere::view::scope_label(SeTab::Actions), "Include disabled actions");
    assert_eq!(crate::everywhere::view::scope_label(SeTab::All), "Include non-project items");
    type_query(cx, "order pizza");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_deref() == Some("Order Pizza")));
    cx.simulate_keystrokes("alt-enter");
    settle(cx, |_| !assign.borrow().is_empty());
    assert_eq!(*assign.borrow(), [Some(serde_json::Value::String("test.pizza".to_owned()))]);
    assert!(pizza.borrow().is_empty(), "the action itself does not run");
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "the popup closes");
    close_backend_project(cx);
}

/// A query with nothing in the project searches everywhere instead and says Nothing found; a query that no
/// longer contains it goes back to the project's items.
#[gpui_kit::test]
async fn searches_everywhere_when_nothing_is_found_in_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    run_action(cx, "navigate.gotoSymbol", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Symbols);
    type_query(cx, "zzqqxx");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.not_found() == Some("zzqqxx")));
    assert!(everywhere.read_with(cx, |popup, _| popup.include_non_project()));
    settle(cx, |cx| cx.debug_bounds("search-everywhere-empty:Nothing found").is_some());

    select_query(&everywhere, cx);
    type_query(cx, "other");
    let scope = everywhere.read_with(cx, |popup, _| (popup.include_non_project(), popup.not_found().is_none()));
    assert_eq!(scope, (false, true), "the new query is searched in the project again");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// An action run from Find Action is listed under Recent Actions the next time the query is empty, and the
/// footer describes the selected action and says how to give it a shortcut, which its row shows.
#[gpui_kit::test]
async fn actions_run_from_the_popup_are_recent_and_the_footer_describes_the_selection(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    let runs = recording_action(cx, ActionMeta::new("test.zymurgy", "Zymurgy Plain").category("Test"));

    run_action(cx, "navigate.findAction", None);
    let everywhere = popup(&workbench, cx);
    type_query(cx, "Zymurgy Plain");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_deref() == Some("Zymurgy Plain")));
    let footer = everywhere.update(cx, |popup, cx| popup.footer_text(cx));
    assert!(footer.starts_with("Test | Zymurgy Plain  ·  "), "{footer}");
    assert!(footer.ends_with(" to assign a shortcut"), "{footer}");
    cx.simulate_keystrokes("enter");
    settle(cx, |_| runs.borrow().len() == 1);
    assert_eq!(recent_actions(stored(cx, RECENT_ACTIONS_KEY).as_deref()), ["test.zymurgy"]);

    run_action(cx, "navigate.findAction", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(query(&everywhere, cx), "Zymurgy Plain", "the tab's last query comes back");
    cx.simulate_keystrokes("backspace");
    cx.executor().advance_clock(crate::everywhere::view::DEBOUNCE);
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines() == ["[Recent Actions]", "Zymurgy Plain"]));

    // An action with a shortcut shows it beside its name.
    select_query(&everywhere, cx);
    type_query(cx, "Go to File");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().contains(&"Go to File…".to_owned())));
    let row = everywhere.read_with(cx, |popup, _| popup.lines().iter().position(|line| line == "Go to File…"));
    let row = row.expect("listed");
    let selector: &'static str = Box::leak(format!("search-everywhere-shortcut-{row}").into_boxed_str());
    settle(cx, |cx| cx.debug_bounds(selector).is_some());
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

/// ⌘↵ on the Text tab hands the query to Find in Files and closes the popup.
#[gpui_kit::test]
async fn cmd_enter_on_text_opens_find_in_files_with_the_query(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    run_action(cx, "navigate.searchText", None);
    let everywhere = popup(&workbench, cx);
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::Text);
    type_query(cx, "needle");
    cx.simulate_keystrokes("cmd-enter");
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.find_popup().is_some()));
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()));
    let find = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
    assert_eq!(find.read_with(cx, |find, cx| find.pattern_field().read(cx).text().to_owned()), "needle");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}
