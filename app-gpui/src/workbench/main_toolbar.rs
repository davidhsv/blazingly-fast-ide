//! Customize Main Toolbar: which widgets (project, VCS, run…) and actions the title bar shows on
//! each side, in which order. Stored in the global settings under [`SETTINGS_KEY`]; unset = the
//! default toolbar.
//!
//! Nothing here does I/O: the saved value and the registered title items are arguments, and
//! [`save_toolbar`] returns the [`SettingsWrite`] its caller carries out.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;

pub const PROJECT_WIDGET: &str = "project";
/// Shown as "Version Control ▾" while the project has no repository.
pub const VCS_WIDGET: &str = "vcs.branch";
pub const ACTION_PREFIX: &str = "action:";
pub const SEARCH_ENTRY: &str = "action:navigate.searchEverywhere";
/// The global settings key the toolbar is stored under.
pub const SETTINGS_KEY: &str = "appearance.mainToolbar";

/// The side of the title bar a widget sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A title bar widget (VCS branch, run configuration, run/debug buttons…) as contributed.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleItem {
    pub id: String,
    pub align: Side,
    /// Higher = further from the centre (closer to the window edge).
    pub priority: f64,
    /// Name in Customize Main Toolbar (defaults to the id).
    pub title: Option<String>,
}

/// The stored toolbar (`appearance.mainToolbar`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainToolbarSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Vec<String>>,
    /// Widgets the user removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<Vec<String>>,
}

/// The full editable layout: the entries of each side in order, and the removed widgets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarLayout {
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub hidden: Vec<String>,
}

impl From<&ToolbarLayout> for MainToolbarSettings {
    /// What the settings hold once `layout` is saved.
    fn from(layout: &ToolbarLayout) -> Self {
        Self { left: Some(layout.left.clone()), right: Some(layout.right.clone()), hidden: Some(layout.hidden.clone()) }
    }
}

/// The entries shown on each side, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolbarSides {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

/// A value to write to the global settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct SettingsWrite {
    pub key: &'static str,
    pub value: Value,
}

pub fn is_action_entry(id: &str) -> bool {
    id.starts_with(ACTION_PREFIX)
}

/// The action id of an action entry.
pub fn entry_action(id: &str) -> &str {
    id.strip_prefix(ACTION_PREFIX).unwrap_or(id)
}

/// Alphabetical regardless of case; of two ids differing only by case, the lower-case one first.
fn compare_ids(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// By priority, then by id. Priorities that do not order (NaN) count as equal.
fn by_priority(a: &TitleItem, b: &TitleItem) -> Ordering {
    match a.priority.partial_cmp(&b.priority) {
        Some(Ordering::Equal) | None => compare_ids(&a.id, &b.id),
        Some(order) => order,
    }
}

/// Widgets in their default place (built-ins included): (left, right).
fn default_widgets(items: &[TitleItem]) -> (Vec<String>, Vec<String>) {
    let side = |align: Side| {
        let mut widgets: Vec<&TitleItem> = items.iter().filter(|item| item.align == align).collect();
        widgets.sort_by(|a, b| by_priority(a, b));
        widgets.into_iter().map(|item| item.id.clone()).collect::<Vec<_>>()
    };
    let contributed = side(Side::Left);
    let mut left = vec![PROJECT_WIDGET.to_owned()];
    if !contributed.iter().any(|id| id == VCS_WIDGET) {
        left.push(VCS_WIDGET.to_owned());
    }
    left.extend(contributed);
    (left, side(Side::Right))
}

/// The toolbar of a user who customized nothing: the project and VCS widgets, the contributed
/// widgets by priority, then Search Everywhere.
pub fn default_toolbar(items: &[TitleItem]) -> ToolbarLayout {
    let (left, mut right) = default_widgets(items);
    right.push(SEARCH_ENTRY.to_owned());
    ToolbarLayout { left, right, hidden: Vec::new() }
}

/// The entries to show per side: the saved order, plus widgets registered since (not listed, not
/// removed) at their default side's end.
pub fn resolve_toolbar(saved: Option<&MainToolbarSettings>, items: &[TitleItem]) -> ToolbarSides {
    let Some(saved) = saved else {
        let ToolbarLayout { left, right, .. } = default_toolbar(items);
        return ToolbarSides { left, right };
    };
    let mut left = saved.left.clone().unwrap_or_default();
    let mut right = saved.right.clone().unwrap_or_default();
    let (default_left, default_right) = default_widgets(items);
    let is_known = |id: &String| {
        left.contains(id) || right.contains(id) || saved.hidden.as_ref().is_some_and(|hidden| hidden.contains(id))
    };
    let new_left: Vec<String> = default_left.into_iter().filter(|id| !is_known(id)).collect();
    let new_right: Vec<String> = default_right.into_iter().filter(|id| !is_known(id)).collect();
    left.extend(new_left);
    right.extend(new_right);
    ToolbarSides { left, right }
}

/// The full editable layout (Customize Main Toolbar dialog).
pub fn editable_toolbar(saved: Option<&MainToolbarSettings>, items: &[TitleItem]) -> ToolbarLayout {
    let ToolbarSides { left, right } = resolve_toolbar(saved, items);
    ToolbarLayout { left, right, hidden: saved.and_then(|saved| saved.hidden.clone()).unwrap_or_default() }
}

/// The name of an entry in Customize Main Toolbar. `action_title` looks up a registered
/// action's title by its id.
pub fn widget_title(id: &str, items: &[TitleItem], action_title: impl Fn(&str) -> Option<String>) -> String {
    if id == PROJECT_WIDGET {
        return "Project Widget".to_owned();
    }
    let item = items.iter().find(|item| item.id == id);
    if id == VCS_WIDGET && item.is_none() {
        return "VCS Widget".to_owned();
    }
    if is_action_entry(id) {
        let action = entry_action(id);
        return action_title(action).unwrap_or_else(|| action.to_owned());
    }
    item.and_then(|item| item.title.clone()).unwrap_or_else(|| id.to_owned())
}

/// Shows or removes a widget, keeping the rest of the layout. A removed widget is remembered as
/// hidden; a removed action is simply dropped.
pub fn toggle_widget(layout: &ToolbarLayout, id: &str, default_side: Side) -> ToolbarLayout {
    let without = |ids: &[String]| ids.iter().filter(|entry| entry.as_str() != id).cloned().collect::<Vec<_>>();
    let shown = layout.left.iter().chain(&layout.right).any(|entry| entry == id);
    let mut next =
        ToolbarLayout { left: without(&layout.left), right: without(&layout.right), hidden: without(&layout.hidden) };
    if !shown {
        match default_side {
            Side::Left => next.left.push(id.to_owned()),
            Side::Right => next.right.push(id.to_owned()),
        }
    } else if !is_action_entry(id) {
        next.hidden.push(id.to_owned());
    }
    next
}

/// The toolbar stored in `settings` (the effective settings object); `None` when unset.
pub fn saved_toolbar(settings: &Value) -> Option<MainToolbarSettings> {
    let saved = settings.get("appearance")?.get("mainToolbar")?;
    Option::<MainToolbarSettings>::deserialize(saved).ok().flatten()
}

/// The write that saves `layout`; `None` restores the default toolbar.
pub fn save_toolbar(layout: Option<&ToolbarLayout>) -> SettingsWrite {
    SettingsWrite { key: SETTINGS_KEY, value: json!(layout) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, align: Side, priority: f64) -> TitleItem {
        TitleItem { id: id.to_owned(), align, priority, title: None }
    }

    fn items() -> Vec<TitleItem> {
        vec![
            item("run.widget", Side::Right, 10.0),
            item("build.project", Side::Right, 0.0),
            item(VCS_WIDGET, Side::Left, 0.0),
        ]
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    // src/workbench/mainToolbar.test.ts › main toolbar layout › defaults to the project and VCS widgets,
    // contributed widgets by priority, then Search Everywhere
    #[test]
    fn defaults_to_the_project_and_vcs_widgets_then_contributed_widgets_then_search_everywhere() {
        assert_eq!(
            default_toolbar(&items()),
            ToolbarLayout {
                left: ids(&["project", VCS_WIDGET]),
                right: ids(&["build.project", "run.widget", SEARCH_ENTRY]),
                hidden: Vec::new(),
            }
        );
        // Without a VCS widget the left side keeps its "Version Control" fallback.
        assert_eq!(default_toolbar(&[]).left, ids(&["project", VCS_WIDGET]));
    }

    // src/workbench/mainToolbar.test.ts › main toolbar layout › keeps the saved order and appends widgets
    // registered since
    #[test]
    fn keeps_the_saved_order_and_appends_widgets_registered_since() {
        let saved = MainToolbarSettings {
            left: Some(ids(&["project"])),
            right: Some(ids(&[SEARCH_ENTRY, "run.widget"])),
            hidden: Some(ids(&[VCS_WIDGET])),
        };
        assert_eq!(
            resolve_toolbar(Some(&saved), &items()),
            ToolbarSides { left: ids(&["project"]), right: ids(&[SEARCH_ENTRY, "run.widget", "build.project"]) }
        );
    }

    // src/workbench/mainToolbar.test.ts › main toolbar layout › hides and re-adds widgets; removed actions
    // are dropped, not hidden
    #[test]
    fn hides_and_re_adds_widgets_and_drops_removed_actions() {
        let layout = editable_toolbar(None, &items());
        let hidden = toggle_widget(&layout, "run.widget", Side::Right);
        assert_eq!(hidden.right, ids(&["build.project", SEARCH_ENTRY]));
        assert_eq!(hidden.hidden, ids(&["run.widget"]));
        let shown = toggle_widget(&hidden, "run.widget", Side::Right);
        assert_eq!(shown.right, ids(&["build.project", SEARCH_ENTRY, "run.widget"]));
        assert!(shown.hidden.is_empty());
        assert!(toggle_widget(&layout, SEARCH_ENTRY, Side::Right).hidden.is_empty());
        // A hidden widget stays hidden after reload.
        let reloaded = resolve_toolbar(Some(&MainToolbarSettings::from(&hidden)), &items());
        assert!(!reloaded.right.iter().any(|id| id == "run.widget"));
    }

    #[test]
    fn saves_the_layout_in_the_shape_the_settings_store() {
        let layout = toggle_widget(&editable_toolbar(None, &items()), "run.widget", Side::Right);
        let write = save_toolbar(Some(&layout));
        assert_eq!(write.key, "appearance.mainToolbar");
        assert_eq!(
            write.value,
            json!({
                "left": ["project", "vcs.branch"],
                "right": ["build.project", "action:navigate.searchEverywhere"],
                "hidden": ["run.widget"],
            })
        );
        let settings = json!({ "appearance": { "mainToolbar": write.value } });
        assert_eq!(saved_toolbar(&settings), Some(MainToolbarSettings::from(&layout)));
        assert_eq!(save_toolbar(None).value, Value::Null);
        assert_eq!(saved_toolbar(&json!({ "appearance": { "mainToolbar": null } })), None);
        assert_eq!(saved_toolbar(&json!({})), None);
    }

    #[test]
    fn names_entries_by_widget_action_or_id() {
        let mut registered = items();
        registered[0].title = Some("Run Widget".to_owned());
        let action_title = |id: &str| (id == "navigate.searchEverywhere").then(|| "Search Everywhere".to_owned());
        assert_eq!(widget_title(PROJECT_WIDGET, &registered, action_title), "Project Widget");
        assert_eq!(widget_title(VCS_WIDGET, &[], action_title), "VCS Widget");
        assert_eq!(widget_title(VCS_WIDGET, &registered, action_title), VCS_WIDGET);
        assert_eq!(widget_title(SEARCH_ENTRY, &registered, action_title), "Search Everywhere");
        assert_eq!(widget_title("action:gone", &registered, action_title), "gone");
        assert_eq!(widget_title("run.widget", &registered, action_title), "Run Widget");
        assert_eq!(widget_title("build.project", &registered, action_title), "build.project");
    }
}
