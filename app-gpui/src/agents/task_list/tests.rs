use super::*;
use crate::agents::fixtures::{connected_store, local_task};
use crate::agents::store::Editors;

fn four() -> AgentsStore {
    connected_store(["one", "two", "three", "four"].into_iter().map(local_task).collect()).0
}

fn key(id: &str) -> RowKey {
    RowKey::Task(id.to_owned())
}

const PLAIN: Click = Click { shift: false, toggle: false, secondary: false };
const SHIFT: Click = Click { shift: true, toggle: false, secondary: false };
const TOGGLE: Click = Click { shift: false, toggle: true, secondary: false };

fn opened(actions: &[Option<PanelAction>]) -> Vec<&str> {
    actions
        .iter()
        .flatten()
        .filter_map(|a| match a {
            PanelAction::OpenTask(t) => Some(t.as_str()),
            PanelAction::DockTask(_) => None,
        })
        .collect()
}

#[test]
fn extends_and_shrinks_a_range_without_opening_its_lead_or_losing_selection_to_an_editor_echo() {
    let store = four();
    let mut panel = AgentsPanel::new(None);
    let mut actions = vec![panel.mouse_down(&store, &key("one"), PLAIN)];
    actions.push(panel.mouse_down(&store, &key("three"), SHIFT));
    assert_eq!(panel.list.selected_tasks(), ["one", "two", "three"]);
    assert_eq!(opened(&actions), ["one"]);
    panel.list.active_tab_changed(Some(&AgentTab::Session("one".to_owned())), store.tasks());
    assert_eq!(panel.list.selected_tasks(), ["one", "two", "three"]);
    actions.push(panel.mouse_down(&store, &key("two"), SHIFT));
    assert_eq!(panel.list.selected_tasks(), ["one", "two"]);
    panel.mouse_down(&store, &key("four"), PLAIN);
    assert_eq!(panel.list.selected_tasks(), ["four"]);
}

#[test]
fn keeps_the_initial_active_task_as_the_anchor_for_repeated_keyboard_extension() {
    let store = four();
    let mut panel = AgentsPanel::new(Some(&AgentTab::Session("one".to_owned())));
    let actions = [panel.key_down(&store, ListKey::Down, true), panel.key_down(&store, ListKey::Down, true)];
    assert_eq!(panel.list.selected_tasks(), ["one", "two", "three"]);
    assert!(opened(&actions).is_empty());
}

#[test]
fn toggles_individual_tasks_with_modifier() {
    // ⌘ on macOS and Ctrl elsewhere both arrive as a toggle.
    let store = four();
    let mut panel = AgentsPanel::new(None);
    let mut actions = vec![panel.mouse_down(&store, &key("one"), PLAIN)];
    actions.push(panel.mouse_down(&store, &key("three"), TOGGLE));
    assert_eq!(panel.list.selected_tasks(), ["one", "three"]);
    actions.push(panel.mouse_down(&store, &key("three"), TOGGLE));
    assert_eq!(panel.list.selected_tasks(), ["one"]);
    assert_eq!(opened(&actions), ["one"]);
}

fn docked(tasks: &[&str]) -> AgentsStore {
    let mut store = connected_store(tasks.iter().copied().map(local_task).collect()).0;
    store.docked = Some("one".to_owned());
    store
}

#[test]
fn opens_session_history_on_demand_and_closes_it_after_switching_tasks() {
    let i18n = I18n::default();
    let store = docked(&["one", "two", "three", "four"]);
    let mut panel = AgentsPanel::new(None);
    assert!(!panel.shows_task_list(&store));
    assert!(location_buttons(&store, &i18n).iter().any(|b| b.label == "Chat in Editor"));
    panel.toggle_history(&store);
    assert!(panel.shows_task_list(&store));
    assert_eq!(panel.mouse_down(&store, &key("two"), PLAIN), None);
    assert_eq!(panel.clicked(&store, "two", PLAIN), Some(PanelAction::OpenTask("two".to_owned())));
    assert!(!panel.history_open());
}

#[test]
fn lets_keyboard_users_browse_history_before_confirming_a_session() {
    let store = docked(&["one", "two", "three", "four"]);
    let mut panel = AgentsPanel::new(None);
    panel.toggle_history(&store);
    assert_eq!(panel.key_down(&store, ListKey::Down, false), None);
    assert!(panel.history_open());
    assert_eq!(panel.key_down(&store, ListKey::Enter, false), Some(PanelAction::OpenTask("two".to_owned())));
    assert!(!panel.history_open());
}

#[test]
fn includes_the_current_session_in_history_and_keeps_the_conversation_mounted() {
    let store = docked(&["one", "two", "three", "four"]);
    let mut panel = AgentsPanel::new(None);
    panel.toggle_history(&store);
    let rows: Vec<RowKey> = panel.list.rows(store.tasks()).iter().map(TaskRow::key).collect();
    assert!(rows.contains(&key("one")) && rows.contains(&key("two")));
    panel.close_history();
    assert!(!panel.shows_task_list(&store));
    assert_eq!(store.docked.as_deref(), Some("one"));
}

#[test]
fn gives_the_whole_tool_window_to_the_conversation_when_the_docked_task_is_the_only_one() {
    let store = docked(&["one"]);
    let panel = AgentsPanel::new(None);
    assert!(AgentsPanel::inline(&store));
    assert!(!panel.shows_task_list(&store));
}

#[test]
fn switches_placement_from_the_global_agents_header() {
    let i18n = I18n::default();
    let mut store = docked(&["one", "two"]);
    store.session_location = SessionLocation::ToolWindow;
    let buttons = location_buttons(&store, &i18n);
    assert!(!buttons.iter().any(|b| b.label == "Open in Editor"));
    assert_eq!(
        buttons.map(|b| (b.label, b.pressed)),
        [("Chat in Tool Window".to_owned(), true), ("Chat in Editor".to_owned(), false)]
    );
    store.set_session_location(buttons_location(&store, &i18n, "Chat in Editor"), &Editors::default());
    assert_eq!(store.session_location, SessionLocation::Editor);
    store.set_session_location(buttons_location(&store, &i18n, "Chat in Tool Window"), &Editors::default());
    assert_eq!(store.session_location, SessionLocation::ToolWindow);
}

fn buttons_location(store: &AgentsStore, i18n: &I18n, label: &str) -> SessionLocation {
    location_buttons(store, i18n).into_iter().find(|b| b.label == label).expect("the button").location
}

#[test]
fn opens_tabs_as_usual_without_a_docked_conversation() {
    let store = four();
    let mut panel = AgentsPanel::new(None);
    assert_eq!(panel.mouse_down(&store, &key("two"), PLAIN), Some(PanelAction::OpenTask("two".to_owned())));
}

#[test]
fn docks_a_session_editor_tab_dropped_on_the_tool_window_and_highlights_while_it_hovers() {
    let mut store = four();
    let mut panel = AgentsPanel::new(None);
    let tab = AgentTab::Session("one".to_owned());
    panel.tab_over(Some(&tab));
    assert!(panel.is_drop_target());
    let action = panel.tab_dropped(Some(&tab));
    assert_eq!(action, Some(PanelAction::DockTask("one".to_owned())));
    store.dock_task("one", &Editors::default());
    assert_eq!(store.docked.as_deref(), Some("one"));
    assert!(!panel.is_drop_target());
}

#[test]
fn ignores_editor_tab_drags_that_are_not_agent_sessions() {
    let mut panel = AgentsPanel::new(None);
    for tab in [None, Some(AgentTab::Other), Some(AgentTab::Home)] {
        panel.tab_over(tab.as_ref());
        assert!(!panel.is_drop_target());
        assert_eq!(panel.tab_dropped(tab.as_ref()), None);
    }
}
