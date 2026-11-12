//! The Notifications tool window: the history of what was raised, the newest first, each entry
//! with the time it was raised, its actions and a button that removes it; above the list, a
//! button that clears it.
//!
//! [`NotificationsView`] draws the entries it is given with [`NotificationsView::set`] and reports
//! what the user does to them as [`NotificationsEvent`]s. The window owns the [`Notifications`],
//! applies each event to them and passes the new entries in.

use super::notification_balloons::{EntryAction, NotificationEntry, ToastClick, ToastHandler, entry_line, toast, tx};
use super::notifications::{NotificationSettings, Notifications};
use crate::platform::date_format::CivilDateTime;
use crate::platform::local_time::utc_offset_minutes;
use crate::theme;
use gpui_kit::{
    App, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use std::rc::Rc;

/// The entries of `notifications` as [`NotificationsView::set`] takes them: what the tool window
/// lists, the newest first.
pub fn logged_entries(
    notifications: &Notifications,
    settings: Option<&NotificationSettings>,
) -> Vec<NotificationEntry> {
    notifications.logged().map(|notification| NotificationEntry::new(notification, settings)).collect()
}

/// The time of day a clock `offset_minutes` east of UTC shows at the Unix time `ms`, as
/// `HH:MM`.
pub fn time_label(ms: f64, offset_minutes: i32) -> String {
    let moment = CivilDateTime::from_unix_ms(ms, offset_minutes);
    format!("{:02}:{:02}", moment.hour, moment.minute)
}

/// The time of day the user's clock showed at the Unix time `ms`.
pub fn local_time_label(ms: f64) -> String {
    time_label(ms, utc_offset_minutes(ms))
}

/// What the Notifications tool window asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationsEvent {
    /// The close button of this notification's entry: it leaves the history.
    Removed(u64),
    /// An action of this notification's entry. The entry stays.
    ActionChosen { id: u64, action: EntryAction },
    /// The Clear All button: the history is emptied.
    ClearAll,
}

pub struct NotificationsView {
    focus: FocusHandle,
    /// The history, the newest first.
    entries: Vec<NotificationEntry>,
}

impl EventEmitter<NotificationsEvent> for NotificationsView {}

impl NotificationsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self { focus: cx.focus_handle(), entries: Vec::new() }
    }

    /// Lists `entries`, the newest first.
    pub fn set(&mut self, entries: Vec<NotificationEntry>, cx: &mut Context<Self>) {
        if self.entries != entries {
            self.entries = entries;
            cx.notify();
        }
    }

    /// The history listed, the newest first.
    pub fn entries(&self) -> &[NotificationEntry] {
        &self.entries
    }

    /// What each entry says, top to bottom: `<HH:MM> <level>: <title> — <body> [<action>]…`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        self.entries.iter().map(|entry| format!("{} {}", local_time_label(entry.time), entry_line(entry, cx))).collect()
    }

    /// What the tool window says instead of a list while it has nothing to list.
    pub fn empty_text(&self, cx: &App) -> Option<String> {
        self.entries.is_empty().then(|| tx(cx, "No notifications"))
    }

    /// Whether Clear All has something to clear.
    pub fn can_clear(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Clear All: asks the window to empty the history, when there is one.
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.can_clear() {
            cx.emit(NotificationsEvent::ClearAll);
        }
    }

    fn clicked(&mut self, id: u64, click: ToastClick, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.iter().find(|entry| entry.id == id) else { return };
        match click {
            ToastClick::Close => cx.emit(NotificationsEvent::Removed(id)),
            ToastClick::Action(index) => {
                if let Some(action) = entry.actions.get(index) {
                    cx.emit(NotificationsEvent::ActionChosen { id, action: action.clone() });
                }
            }
        }
    }
}

impl Focusable for NotificationsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotificationsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = theme::color(cx, "border");
        let can_clear = self.can_clear();
        let clear_all = div()
            .id("clear-all")
            .debug_selector(|| "notifications-clear-all".to_owned())
            .size(px(20.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .child(theme::icon(cx, "trash", 16.0, theme::color(cx, "fg-muted")))
            .when(!can_clear, |el| el.opacity(0.4))
            .when(can_clear, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(|view: &mut Self, _, _, cx| view.clear_all(cx)))
            });
        let content = match self.empty_text(cx) {
            // Centred grey text, no illustration.
            Some(text) => div()
                .debug_selector(|| "notifications-empty".to_owned())
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(text))
                .into_any_element(),
            None => {
                let last = self.entries.len() - 1;
                let entries: Vec<Stateful<Div>> = self
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let id = entry.id;
                        let on_click: ToastHandler =
                            Rc::new(cx.listener(move |view: &mut Self, click: &ToastClick, _, cx| {
                                view.clicked(id, *click, cx);
                            }));
                        let time = Some(local_time_label(entry.time));
                        toast(entry, &format!("notification-{id}"), time, on_click, window, cx)
                            .w_full()
                            .flex_none()
                            // A line between two entries.
                            .when(index < last, |el| el.border_b_1().border_color(border))
                    })
                    .collect();
                div()
                    .id("notifications-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(entries)
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .h(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px(px(6.0))
                    .border_b_1()
                    .border_color(border)
                    .child(clear_all),
            )
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::super::notification_balloons::balloon_entries;
    use super::super::notifications::NotificationLevel::{Error, Info, Warning};
    use super::super::notifications::{
        NotificationAction, NotificationLevel, NotificationRequest, notification_settings,
    };
    use super::*;
    use crate::platform::i18n::Localization;
    use gpui_kit::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<NotificationsEvent>>>;

    /// The view in a window of its own, and what it has asked of the window so far.
    fn open(cx: &mut TestAppContext) -> (Entity<NotificationsView>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(|_, cx| NotificationsView::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &NotificationsEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (view, events, cx)
    }

    /// Lists what `store` holds, as the window does after every change.
    fn show(store: &Notifications, view: &Entity<NotificationsView>, cx: &mut VisualTestContext) {
        let entries = logged_entries(store, None);
        view.update(cx, |view, cx| view.set(entries, cx));
        cx.run_until_parked();
    }

    /// Does what the view asked for, as its window does, and lists the outcome.
    fn apply_all(
        store: &mut Notifications,
        events: &Events,
        view: &Entity<NotificationsView>,
        cx: &mut VisualTestContext,
    ) {
        let asked: Vec<NotificationsEvent> = events.borrow_mut().drain(..).collect();
        for event in asked {
            match event {
                NotificationsEvent::Removed(id) => store.remove_notification(id),
                NotificationsEvent::ClearAll => store.clear(),
                // The window runs the action; the history stays as it is.
                NotificationsEvent::ActionChosen { .. } => {}
            }
        }
        show(store, view, cx);
    }

    fn request(level: NotificationLevel, title: &str) -> NotificationRequest {
        NotificationRequest::new(level, title)
    }

    fn action(label: &str) -> NotificationAction {
        NotificationAction { label: label.to_owned(), command: format!("test.{label}") }
    }

    fn lines(view: &Entity<NotificationsView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.lines(cx))
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
    }

    /// 2026-03-01 at this time of day, UTC, in milliseconds since the epoch.
    fn at(hour: f64, minute: f64) -> f64 {
        1_772_323_200_000.0 + (hour * 60.0 + minute) * 60_000.0
    }

    // src/workbench/Workbench.test.tsx › notifications › the Notifications tool window lists history, marks
    // it read and hides balloons
    #[gpui_kit::test]
    async fn the_notifications_tool_window_lists_history_marks_it_read_and_hides_balloons(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let mut store = Notifications::default();
        store.notify(request(Info, "Hello"), None, at(9.0, 5.0));
        show(&store, &view, cx);
        let hello = format!("{} info: Hello [Don't Show Again]", local_time_label(at(9.0, 5.0)));
        assert_eq!(lines(&view, cx), std::slice::from_ref(&hello));
        assert_eq!(balloon_entries(&store, None, false).len(), 1);
        assert!(store.has_unseen());
        // The tool window is on screen: what it lists is seen, and no balloon shows any more.
        store.mark_all_read();
        show(&store, &view, cx);
        assert_eq!(lines(&view, cx), [hello]);
        assert!(balloon_entries(&store, None, false).is_empty());
        assert!(store.items().iter().all(|item| item.read && !item.balloon));
        assert_eq!(store.unread_count(), 0);
    }

    #[gpui_kit::test]
    async fn lists_the_history_newest_first_each_entry_with_its_time(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let mut store = Notifications::default();
        store.notify(request(Info, "Indexed"), None, at(9.0, 5.0));
        let warned = NotificationRequest {
            body: Some("origin/main has 2 new commits".to_owned()),
            actions: vec![action("Update")],
            group: Some("vcs".to_owned()),
            ..request(Warning, "Push rejected")
        };
        store.notify(warned, None, at(13.0, 40.0));
        store.notify(request(Error, "Build failed"), None, at(23.0, 59.0));
        show(&store, &view, cx);
        assert_eq!(
            lines(&view, cx),
            [
                format!("{} error: Build failed", local_time_label(at(23.0, 59.0))),
                format!(
                    "{} warning: Push rejected — origin/main has 2 new commits [Update] [Don't Show Again]",
                    local_time_label(at(13.0, 40.0))
                ),
                format!("{} info: Indexed [Don't Show Again]", local_time_label(at(9.0, 5.0))),
            ]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)), None);
        // The entries span the tool window, one under the other, below the Clear All button.
        let viewport = cx.update(|window, _| window.viewport_size());
        let (newest, middle, oldest) =
            (bounds_of(cx, "notification-3"), bounds_of(cx, "notification-2"), bounds_of(cx, "notification-1"));
        assert!(bounds_of(cx, "notifications-clear-all").bottom() <= newest.top());
        assert!(newest.bottom() <= middle.top() && middle.bottom() <= oldest.top(), "{newest:?} {middle:?} {oldest:?}");
        for entry in [newest, middle, oldest] {
            assert!((entry.size.width - viewport.width).abs() < px(1.0), "{entry:?}");
        }
        assert!(cx.debug_bounds("notifications-empty").is_none());
    }

    #[gpui_kit::test]
    async fn lists_only_what_its_group_logs(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let settings = notification_settings(&json!({ "notifications": { "groups": { "run": { "log": false } } } }));
        let mut store = Notifications::default();
        store.notify(request(Info, "Saved"), settings.as_ref(), at(8.0, 0.0));
        let started = NotificationRequest { group: Some("run".to_owned()), ..request(Info, "Started") };
        store.notify(started, settings.as_ref(), at(8.0, 1.0));
        assert_eq!(store.items().len(), 2);
        let entries = logged_entries(&store, settings.as_ref());
        view.update(cx, |view, cx| view.set(entries, cx));
        assert_eq!(lines(&view, cx), [format!("{} info: Saved [Don't Show Again]", local_time_label(at(8.0, 0.0)))]);
    }

    #[gpui_kit::test]
    async fn says_so_when_there_is_nothing_to_list_and_clear_all_is_off(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert!(lines(&view, cx).is_empty());
        assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("No notifications"));
        assert!(cx.debug_bounds("notifications-empty").is_some());
        assert!(!view.read_with(cx, |view, _| view.can_clear()));
        click(cx, "notifications-clear-all");
        view.update(cx, |view, cx| view.clear_all(cx));
        cx.run_until_parked();
        assert!(events.borrow().is_empty(), "there is nothing to clear");
        cx.update(|_, cx| {
            cx.default_global::<Localization>().0.set_locale(Some("zh"), "en");
        });
        assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("没有通知"));
    }

    #[gpui_kit::test]
    async fn clear_all_empties_the_history(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut store = Notifications::default();
        store.notify(request(Info, "Saved"), None, at(8.0, 0.0));
        store.notify(request(Error, "Build failed"), None, at(8.0, 1.0));
        show(&store, &view, cx);
        assert!(view.read_with(cx, |view, _| view.can_clear()));
        click(cx, "notifications-clear-all");
        assert_eq!(*events.borrow(), [NotificationsEvent::ClearAll]);
        apply_all(&mut store, &events, &view, cx);
        assert!(lines(&view, cx).is_empty() && store.items().is_empty());
        assert!(cx.debug_bounds("notifications-empty").is_some());
        assert!(cx.debug_bounds("notification-1").is_none());
    }

    #[gpui_kit::test]
    async fn the_close_button_removes_its_entry(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut store = Notifications::default();
        let saved = store.notify(request(Info, "Saved"), None, at(8.0, 0.0)).id;
        store.notify(request(Error, "Build failed"), None, at(8.0, 1.0));
        show(&store, &view, cx);
        click(cx, "notification-1-close");
        assert_eq!(*events.borrow(), [NotificationsEvent::Removed(saved)]);
        apply_all(&mut store, &events, &view, cx);
        assert_eq!(lines(&view, cx), [format!("{} error: Build failed", local_time_label(at(8.0, 1.0)))]);
        assert_eq!(view.read_with(cx, |view, _| view.entries().len()), 1);
    }

    #[gpui_kit::test]
    async fn an_action_is_a_link_that_leaves_its_entry_listed(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut store = Notifications::default();
        let rejected = NotificationRequest {
            actions: vec![action("Update"), action("Force Push")],
            group: Some("vcs".to_owned()),
            ..request(Warning, "Push rejected")
        };
        let id = store.notify(rejected, None, at(8.0, 0.0)).id;
        show(&store, &view, cx);
        click(cx, "notification-1-action-1");
        click(cx, "notification-1-action-2");
        assert_eq!(
            *events.borrow(),
            [
                NotificationsEvent::ActionChosen { id, action: EntryAction::Own(action("Force Push")) },
                NotificationsEvent::ActionChosen { id, action: EntryAction::DontShowAgain { group: "vcs".to_owned() } },
            ]
        );
        apply_all(&mut store, &events, &view, cx);
        assert_eq!(
            lines(&view, cx),
            [format!(
                "{} warning: Push rejected [Update] [Force Push] [Don't Show Again]",
                local_time_label(at(8.0, 0.0))
            )]
        );
    }

    #[gpui_kit::test]
    async fn takes_the_keyboard_when_its_tool_window_is_given_it(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
    }

    #[test]
    fn the_time_is_the_clocks_hour_and_minute() {
        assert_eq!(time_label(0.0, 0), "00:00");
        assert_eq!(time_label(at(9.0, 5.0), 0), "09:05");
        assert_eq!(time_label(at(23.0, 59.0) + 59_999.0, 0), "23:59");
        // East and west of UTC, across midnight.
        assert_eq!(time_label(at(23.0, 30.0), 90), "01:00");
        assert_eq!(time_label(at(0.0, 15.0), -30), "23:45");
        assert_eq!(time_label(at(13.0, 40.0), 5 * 60 + 30), "19:10");
    }
}
