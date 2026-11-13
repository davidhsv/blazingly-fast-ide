//! Appearance & Behavior › Notifications: per notification group, how its notifications pop up
//! and whether the Notifications tool window lists them.

use super::{Binding, ControlEvent, drop_down, drop_down_row, row, row_label, select_box};
use crate::settings_view::controls::{check_box, search_label, tx};
use crate::theme;
use crate::workbench::notifications::{GROUPS_SETTINGS_KEY, NOTIFICATION_GROUPS};
use gpui_kit::{
    AnyElement, ClickEvent, Context, EventEmitter, IntoElement, MouseButton, Render, SharedString, Window, div,
    prelude::*, px,
};
use serde_json::{Map, Value, json};

pub const PAGE: &str = "notifications";

/// The popup types, as stored and as shown.
pub const DISPLAYS: [(&str, &str); 3] = [("balloon", "Balloon"), ("sticky", "Sticky balloon"), ("none", "No popup")];

/// One group as stored: its popup type and whether it is listed; a group without an entry is a
/// balloon and listed.
fn entry(groups: &Map<String, Value>, id: &str) -> (String, bool) {
    let stored = groups.get(id);
    let display = stored.and_then(|entry| entry.get("display")).and_then(Value::as_str).unwrap_or("balloon");
    let log = stored.and_then(|entry| entry.get("log")).and_then(Value::as_bool).unwrap_or(true);
    (display.to_owned(), log)
}

/// `groups` with group `id` set to `display` and `log`; a group back at the defaults is not stored.
pub fn with_group(groups: &Map<String, Value>, id: &str, display: &str, log: bool) -> Map<String, Value> {
    let mut groups = groups.clone();
    if display == "balloon" && log {
        groups.remove(id);
    } else {
        groups.insert(id.to_owned(), json!({ "display": display, "log": log }));
    }
    groups
}

pub struct NotificationGroupsView {
    binding: Binding,
    open: Option<&'static str>,
}

impl EventEmitter<ControlEvent> for NotificationGroupsView {}

impl NotificationGroupsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        cx.observe(&binding.model, |_, _, cx| cx.notify()).detach();
        Self { binding, open: None }
    }

    fn groups(&self, cx: &gpui_kit::App) -> Map<String, Value> {
        self.binding.value(GROUPS_SETTINGS_KEY, cx).and_then(|value| value.as_object().cloned()).unwrap_or_default()
    }

    fn set(&mut self, id: &str, display: Option<&str>, log: Option<bool>, cx: &mut Context<Self>) {
        let groups = self.groups(cx);
        let (now_display, now_log) = entry(&groups, id);
        let next = with_group(&groups, id, display.unwrap_or(&now_display), log.unwrap_or(now_log));
        self.open = None;
        self.binding.stage(GROUPS_SETTINGS_KEY, Value::Object(next), cx);
        cx.notify();
    }

    /// The table as text: `Title: Popup type [x]` per group.
    pub fn lines(&self, cx: &gpui_kit::App) -> Vec<String> {
        let groups = self.groups(cx);
        NOTIFICATION_GROUPS
            .iter()
            .map(|(id, title)| {
                let (display, log) = entry(&groups, id);
                let shown = DISPLAYS.iter().find(|(value, _)| *value == display).map_or("Balloon", |(_, label)| label);
                format!("{}: {} [{}]", tx(cx, title), tx(cx, shown), if log { "x" } else { " " })
            })
            .collect()
    }
}

impl Render for NotificationGroupsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let groups = self.groups(cx);
        let muted = theme::color(cx, "fg-subtle");
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(4.0))
            .text_color(muted)
            .child(div().w(px(super::LABEL_WIDTH)).child(SharedString::from(tx(cx, "Group"))))
            .child(div().w(px(180.0)).child(SharedString::from(tx(cx, "Popup type"))))
            .child(SharedString::from(tx(cx, "Show in tool window")));
        let rows: Vec<AnyElement> = NOTIFICATION_GROUPS
            .iter()
            .map(|&(id, title)| {
                let (display, log) = entry(&groups, id);
                let matched = self.binding.matches(title, cx);
                let shown = DISPLAYS.iter().find(|(value, _)| *value == display).map_or("Balloon", |(_, label)| label);
                let shown = tx(cx, shown);
                let boxed = select_box(cx, &shown)
                    .id(SharedString::from(format!("settings-notification-{id}-display")))
                    .debug_selector(move || format!("settings-notification-{id}-display"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.open = if view.open == Some(id) { None } else { Some(id) };
                        cx.notify();
                    }));
                let list = (self.open == Some(id)).then(|| {
                    let choices: Vec<AnyElement> = DISPLAYS
                        .iter()
                        .map(|&(value, label)| {
                            let text = tx(cx, label);
                            let selector = format!("settings-notification-{id}-display-{value}");
                            drop_down_row(cx, selector, &text, value == display)
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.set(id, Some(value), None, cx);
                                }))
                                .into_any_element()
                        })
                        .collect();
                    drop_down(cx, choices)
                });
                let check = div()
                    .id(SharedString::from(format!("settings-notification-{id}-log")))
                    .debug_selector(move || format!("settings-notification-{id}-log"))
                    .child(check_box(cx, log))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set(id, None, Some(!log), cx)));
                let title = tx(cx, title);
                row(cx, matched)
                    .child(row_label(cx, &title, &query))
                    .child(div().w(px(180.0)).relative().child(boxed).children(list))
                    .child(check)
                    .into_any_element()
            })
            .collect();
        let label = tx(cx, "Popup type");
        div()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(div().px(px(4.0)).text_color(muted).child(search_label(cx, &label, &query)))
            .child(header)
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // appearance & behavior settings › normalises notification groups (a group back at the
    // defaults is not stored)
    #[test]
    fn a_group_at_the_defaults_is_not_stored() {
        let groups = with_group(&Map::new(), "vcs", "none", true);
        assert_eq!(Value::Object(groups.clone()), json!({ "vcs": { "display": "none", "log": true } }));
        assert!(with_group(&groups, "vcs", "balloon", true).is_empty());
    }

    // Not from a spec: a group's popup type and its tool window checkbox stage the groups map.
    #[gpui_kit::test]
    async fn a_groups_popup_type_and_listing_stage_the_groups(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| NotificationGroupsView::new(binding, cx));
        click(cx, "settings-notification-vcs-display");
        click(cx, "settings-notification-vcs-display-none");
        click(cx, "settings-notification-run-log");
        let staged = staged(&model, cx);
        let expected =
            json!({ "vcs": { "display": "none", "log": true }, "run": { "display": "balloon", "log": false } });
        assert_eq!(staged, [("global:notifications.groups".to_owned(), expected.to_string())]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.contains(&"Version Control: No popup [x]".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Run and Debug: Balloon [ ]".to_owned()), "{lines:?}");
    }
}
