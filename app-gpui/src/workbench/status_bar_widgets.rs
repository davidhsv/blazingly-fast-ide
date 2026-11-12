//! Which status bar widgets are shown: per-widget defaults, overridden in the global settings
//! under [`SETTINGS_KEY`].
//!
//! Nothing here does I/O: the overrides and the registered status items are arguments, and a
//! change returns the [`SettingsWrite`] its caller carries out.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The global settings key the overrides are stored under.
pub const SETTINGS_KEY: &str = "appearance.statusBarWidgets";

/// Widget id → shown, for the widgets the user decided about.
pub type WidgetOverrides = BTreeMap<String, bool>;

/// A status bar widget as contributed, as far as its visibility is concerned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusItem {
    pub id: String,
    /// Name in the status bar context menu, where the user can hide or show the widget.
    /// Untitled widgets are always shown.
    pub title: Option<String>,
    /// Shown until the user decides otherwise (default true).
    pub default_visible: Option<bool>,
}

impl StatusItem {
    /// The title, when the widget has one users can pick it by.
    fn menu_title(&self) -> Option<&str> {
        self.title.as_deref().filter(|title| !title.is_empty())
    }
}

/// A value to write to the global settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct SettingsWrite {
    pub key: &'static str,
    pub value: Value,
}

pub fn is_status_item_visible(item: &StatusItem, overrides: Option<&WidgetOverrides>) -> bool {
    if item.menu_title().is_none() {
        return true;
    }
    overrides.and_then(|overrides| overrides.get(&item.id).copied()).unwrap_or(item.default_visible != Some(false))
}

/// A widget users can switch, with its current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusWidgetRow {
    pub id: String,
    pub title: String,
    /// Plugin widgets are named by their plugin, not translated.
    pub verbatim: bool,
    pub visible: bool,
}

/// Every titled widget of `items` (in bar order) with its visibility: the rows users can switch.
pub fn status_widget_rows(items: &[StatusItem], overrides: Option<&WidgetOverrides>) -> Vec<StatusWidgetRow> {
    items
        .iter()
        .filter_map(|item| {
            Some(StatusWidgetRow {
                id: item.id.clone(),
                title: item.menu_title()?.to_owned(),
                verbatim: item.id.starts_with("plugin."),
                visible: is_status_item_visible(item, overrides),
            })
        })
        .collect()
}

/// What choosing a menu row asks for: the arguments of [`set_status_widget_visible`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetVisible {
    pub id: String,
    pub visible: bool,
}

/// A row of the status bar context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMenuItem {
    pub id: String,
    pub label: String,
    pub verbatim: bool,
    pub checked: bool,
}

impl StatusMenuItem {
    /// Choosing the row flips its widget.
    pub fn run(&self) -> SetVisible {
        SetVisible { id: self.id.clone(), visible: !self.checked }
    }
}

/// The status bar context menu: every titled widget with a checkmark, in bar order.
pub fn status_bar_menu(items: &[StatusItem], overrides: Option<&WidgetOverrides>) -> Vec<StatusMenuItem> {
    status_widget_rows(items, overrides)
        .into_iter()
        .map(|row| StatusMenuItem { id: row.id, label: row.title, verbatim: row.verbatim, checked: row.visible })
        .collect()
}

/// The write that persists the choice: the whole map, since widget ids contain dots.
pub fn set_status_widget_visible(overrides: Option<&WidgetOverrides>, id: &str, visible: bool) -> SettingsWrite {
    let mut next = overrides.cloned().unwrap_or_default();
    next.insert(id.to_owned(), visible);
    SettingsWrite {
        key: SETTINGS_KEY,
        value: Value::Object(next.into_iter().map(|(id, visible)| (id, Value::Bool(visible))).collect()),
    }
}

/// Flips one widget by id (View › Appearance › Status Bar Widgets); `None` for untitled or
/// unknown widgets, which stay as they are.
pub fn toggle_status_widget(
    items: &[StatusItem],
    overrides: Option<&WidgetOverrides>,
    id: &str,
) -> Option<SettingsWrite> {
    let item = items.iter().find(|item| item.id == id)?;
    item.menu_title()?;
    Some(set_status_widget_visible(overrides, id, !is_status_item_visible(item, overrides)))
}

/// The overrides stored in `settings` (the effective settings object); `None` when unset.
pub fn status_widget_overrides(settings: &Value) -> Option<WidgetOverrides> {
    let stored = settings.get("appearance")?.get("statusBarWidgets")?;
    Option::<WidgetOverrides>::deserialize(stored).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: &str, title: Option<&str>, default_visible: Option<bool>) -> StatusItem {
        StatusItem { id: id.to_owned(), title: title.map(str::to_owned), default_visible }
    }

    fn overrides(entries: &[(&str, bool)]) -> WidgetOverrides {
        entries.iter().map(|(id, visible)| ((*id).to_owned(), *visible)).collect()
    }

    /// Settings holding what `write` stored, as the backend hands them back.
    fn settings_after(write: &SettingsWrite) -> Value {
        assert_eq!(write.key, "appearance.statusBarWidgets");
        json!({ "appearance": { "statusBarWidgets": write.value } })
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › applies defaults and overrides
    #[test]
    fn applies_defaults_and_overrides() {
        let memory = item("perf.memory", Some("Memory Indicator"), Some(false));
        assert!(!is_status_item_visible(&memory, None));
        assert!(is_status_item_visible(&memory, Some(&overrides(&[("perf.memory", true)]))));
        let caret = item("caret", Some("Line:Column"), None);
        assert!(is_status_item_visible(&caret, Some(&overrides(&[]))));
        assert!(!is_status_item_visible(&caret, Some(&overrides(&[("caret", false)]))));
        // Untitled widgets cannot be hidden.
        assert!(is_status_item_visible(&item("tasks", None, None), Some(&overrides(&[("tasks", false)]))));
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › lists titled widgets with checkmarks
    #[test]
    fn lists_titled_widgets_with_checkmarks() {
        let menu = status_bar_menu(
            &[
                item("caret", Some("Line:Column Number"), None),
                item("tasks", None, None),
                item("mem", Some("Memory"), Some(false)),
            ],
            Some(&overrides(&[("caret", true)])),
        );
        let rows: Vec<(&str, bool)> = menu.iter().map(|row| (row.label.as_str(), row.checked)).collect();
        assert_eq!(rows, [("Line:Column Number", true), ("Memory", false)]);
        assert_eq!(menu[1].run(), SetVisible { id: "mem".to_owned(), visible: true });
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › toggles a widget from the status bar
    // context menu and persists it
    #[test]
    fn toggles_a_widget_from_the_context_menu_and_persists_it() {
        let items = [
            item("w.caret", Some("Line:Column Number"), None),
            item("w.memory", Some("Memory Indicator"), Some(false)),
        ];
        let stored = status_widget_overrides(&json!({ "appearance": { "statusBarWidgets": {} } }));
        assert_eq!(stored, Some(WidgetOverrides::new()));
        assert!(!is_status_item_visible(&items[1], stored.as_ref()));
        assert!(is_status_item_visible(&items[0], stored.as_ref()));

        let menu = status_bar_menu(&items, stored.as_ref());
        let entry = menu.iter().find(|row| row.label == "Memory Indicator").unwrap();
        assert!(!entry.checked);
        let SetVisible { id, visible } = entry.run();
        let write = set_status_widget_visible(stored.as_ref(), &id, visible);
        assert_eq!(write.value, json!({ "w.memory": true }));

        let stored = status_widget_overrides(&settings_after(&write));
        assert!(is_status_item_visible(&items[1], stored.as_ref()));
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › toggles one widget by id from
    // View › Appearance › Status Bar Widgets
    #[test]
    fn toggles_one_widget_by_id() {
        let items = [
            item("w.caret", Some("Line:Column Number"), None),
            item("w.memory", Some("Memory Indicator"), Some(false)),
            item("w.tasks", None, None),
        ];
        let mut stored = Some(WidgetOverrides::new());
        let write = toggle_status_widget(&items, stored.as_ref(), "w.memory").unwrap();
        assert_eq!(write.value, json!({ "w.memory": true }));
        stored = status_widget_overrides(&settings_after(&write));

        let write = toggle_status_widget(&items, stored.as_ref(), "w.caret").unwrap();
        stored = status_widget_overrides(&settings_after(&write));
        assert_eq!(stored, Some(overrides(&[("w.memory", true), ("w.caret", false)])));
        // Untitled or unknown widgets cannot be hidden.
        assert_eq!(toggle_status_widget(&items, stored.as_ref(), "w.tasks"), None);
        assert_eq!(toggle_status_widget(&items, stored.as_ref(), "w.gone"), None);
    }
}
