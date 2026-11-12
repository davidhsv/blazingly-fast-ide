//! Notification centre: balloons + history.
//!
//! [`Notifications`] holds what was raised, newest first. It reads no settings and no clock: the
//! Notifications settings ([`NotificationSettings`], the `notifications` object of the effective
//! settings) and the time are arguments. A notification's actions are data
//! ([`NotificationAction`]); the caller runs them.
//!
//! Balloons fade out on [`BalloonClocks`]: deadlines the caller checks with
//! [`BalloonClocks::fade_out`], running only while the balloon is on screen, the window active
//! and the pointer elsewhere.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

/// A button of a notification. `command` tells the caller what to run; it means nothing here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAction {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdeNotification {
    pub id: u64,
    pub key: Option<String>,
    pub level: NotificationLevel,
    pub title: String,
    pub body: Option<String>,
    pub actions: Vec<NotificationAction>,
    /// When it was raised, in milliseconds since the epoch.
    pub time: f64,
    pub read: bool,
    /// Title, body and action labels come from outside the IDE (a plugin) and are shown as written.
    pub verbatim: bool,
    /// Currently shown as a balloon.
    pub balloon: bool,
    pub sticky: bool,
    pub group: String,
    /// Listed in the Notifications tool window (the group's "Show in tool window").
    pub logged: bool,
}

/// Notification groups shown in Settings › Appearance & Behavior › Notifications: (id, title).
pub const NOTIFICATION_GROUPS: [(&str, &str); 9] = [
    ("system", "System Messages"),
    ("lsp", "Language Servers"),
    ("vcs", "Version Control"),
    ("run", "Run and Debug"),
    ("tools", "External Tools"),
    ("search", "Find in Files"),
    ("plugins", "Plugins"),
    ("updates", "IDE Updates"),
    ("distractionFree", "Distraction Free Mode"),
];

/// The group of app-wide notifications, which outlive the project they were raised in.
const UPDATES_GROUP: &str = "updates";

/// The global settings key the per-group settings are stored under.
pub const GROUPS_SETTINGS_KEY: &str = "notifications.groups";

/// How a group's notifications pop up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationDisplay {
    Balloon,
    Sticky,
    /// "No popup", stored as `none`.
    NoPopup,
}

impl NotificationDisplay {
    /// The name the display type is stored by.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balloon => "balloon",
            Self::Sticky => "sticky",
            Self::NoPopup => "none",
        }
    }
}

/// Settings › Notifications for one group, as stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationGroupSettings {
    /// `balloon` | `sticky` | `none`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// Also list it in the Notifications tool window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<bool>,
}

/// Settings › Notifications, as stored: global switches and the per-group display type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balloons: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<bool>,
    /// Group id → display; groups not listed use their default.
    #[serde(default)]
    pub groups: BTreeMap<String, NotificationGroupSettings>,
}

/// The Notifications settings in `settings` (the effective settings object); `None` when unset.
pub fn notification_settings(settings: &Value) -> Option<NotificationSettings> {
    Option::<NotificationSettings>::deserialize(settings.get("notifications")?).ok().flatten()
}

/// What Settings › Notifications says for one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationPrefs {
    pub balloons: bool,
    pub sound: bool,
    pub display: NotificationDisplay,
    pub log: bool,
}

/// Settings › Notifications for one group, defaults applied.
pub fn notification_prefs(settings: Option<&NotificationSettings>, group: &str) -> NotificationPrefs {
    let entry = settings.and_then(|settings| settings.groups.get(group));
    let display = match entry.and_then(|entry| entry.display.as_deref()) {
        Some("sticky") => NotificationDisplay::Sticky,
        Some("none") => NotificationDisplay::NoPopup,
        _ => NotificationDisplay::Balloon,
    };
    NotificationPrefs {
        balloons: settings.and_then(|settings| settings.balloons) != Some(false),
        sound: settings.and_then(|settings| settings.sound) == Some(true),
        display,
        log: entry.and_then(|entry| entry.log) != Some(false),
    }
}

/// Errors always show; other notifications offer "Don't Show Again" for their group.
pub fn can_suppress(settings: Option<&NotificationSettings>, level: NotificationLevel, group: &str) -> bool {
    level != NotificationLevel::Error && notification_prefs(settings, group).display != NotificationDisplay::NoPopup
}

/// A value to write to the global settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct SettingsWrite {
    pub key: &'static str,
    pub value: Value,
}

/// "Don't Show Again": the write that turns the group's balloons off (Settings › Notifications ›
/// "No popup"); its notifications are still logged. Once it is stored, the caller takes the
/// group's balloons away with [`Notifications::hide_group_balloons`].
pub fn suppress_group(settings: Option<&NotificationSettings>, group: &str) -> SettingsWrite {
    let mut groups = settings.map(|settings| settings.groups.clone()).unwrap_or_default();
    let entry = groups.entry(group.to_owned()).or_default();
    entry.display = Some(NotificationDisplay::NoPopup.as_str().to_owned());
    entry.log = Some(entry.log.unwrap_or(true));
    SettingsWrite { key: GROUPS_SETTINGS_KEY, value: json!(groups) }
}

pub const DONT_SHOW_AGAIN: &str = "Don't Show Again";

/// A button shown on a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShownAction<'a> {
    /// One of the notification's own.
    Own(&'a NotificationAction),
    /// [`suppress_group`] for the notification's group.
    DontShowAgain,
}

impl ShownAction<'_> {
    pub fn label(&self) -> &str {
        match self {
            Self::Own(action) => &action.label,
            Self::DontShowAgain => DONT_SHOW_AGAIN,
        }
    }
}

/// The notification's own actions, then "Don't Show Again" for its group (IDEA).
pub fn notification_actions<'a>(
    notification: &'a IdeNotification,
    settings: Option<&NotificationSettings>,
) -> Vec<ShownAction<'a>> {
    let suppress =
        can_suppress(settings, notification.level, &notification.group).then_some(ShownAction::DontShowAgain);
    notification.actions.iter().map(ShownAction::Own).chain(suppress).collect()
}

/// IDEA fades balloons out after 10 s.
pub const BALLOON_TIMEOUT: f64 = 10_000.0;

/// The fade-out delay of a level's balloons; `None` for errors, which are sticky balloons (as
/// IDEA's error groups).
pub fn level_balloon_timeout(level: NotificationLevel) -> Option<f64> {
    match level {
        NotificationLevel::Info | NotificationLevel::Warning => Some(BALLOON_TIMEOUT),
        NotificationLevel::Error => None,
    }
}

/// Sticky balloons fade too, after 5 minutes of active window time, as in IDEA: the notification
/// stays unread in the Notifications tool window. One that the tool window does not list stays
/// until dismissed.
pub const STICKY_BALLOON_TIMEOUT: f64 = 5.0 * 60.0 * 1000.0;

/// Fade-out delay of a balloon in milliseconds, or `None` when it stays until dismissed.
pub fn balloon_timeout(notification: &IdeNotification) -> Option<f64> {
    if notification.sticky {
        return notification.logged.then_some(STICKY_BALLOON_TIMEOUT);
    }
    Some(level_balloon_timeout(notification.level).unwrap_or(BALLOON_TIMEOUT))
}

/// How many notifications are kept.
const MAX_HISTORY: usize = 200;
/// How many balloons show at a time.
pub const MAX_BALLOONS: usize = 3;

/// A notification to raise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRequest {
    pub level: NotificationLevel,
    pub title: String,
    pub body: Option<String>,
    pub actions: Vec<NotificationAction>,
    /// Keep the balloon until dismissed (errors are sticky by default).
    pub sticky: Option<bool>,
    /// Replaces an earlier notification with the same key.
    pub key: Option<String>,
    /// Title, body and action labels come from outside the IDE (a plugin) and are shown as written.
    pub verbatim: bool,
    /// Notification group (Settings › Notifications): `system` (default), `lsp`, `vcs`, `run`,
    /// `tools`, `plugins`, `search`, `updates`.
    pub group: Option<String>,
    /// The notification answers a command the user just ran (Check for Updates and the install
    /// it offers). It pops up even when balloon notifications are turned off or its group's popup
    /// type is "No popup": those settings silence what the IDE reports on its own, not the answer
    /// to a request.
    pub user_initiated: bool,
}

impl NotificationRequest {
    pub fn new(level: NotificationLevel, title: impl Into<String>) -> Self {
        Self {
            level,
            title: title.into(),
            body: None,
            actions: Vec::new(),
            sticky: None,
            key: None,
            verbatim: false,
            group: None,
            user_initiated: false,
        }
    }

    /// The group the notification belongs to: the one it names, else by its key.
    fn group_id(&self) -> &str {
        if let Some(group) = self.group.as_deref().filter(|group| !group.is_empty()) {
            return group;
        }
        let key = self.key.as_deref().unwrap_or_default();
        if key.starts_with("app.update") {
            UPDATES_GROUP
        } else if key.starts_with("lsp") {
            "lsp"
        } else {
            "system"
        }
    }
}

/// What raising a notification came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notified {
    /// The notification's id, also when the settings had it dropped.
    pub id: u64,
    /// Play the notification chime ("Play sound").
    pub play_sound: bool,
}

/// The notifications raised in this window, newest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notifications {
    items: Vec<IdeNotification>,
    /// The id of the last notification raised.
    seq: u64,
}

impl Notifications {
    /// Every notification kept, newest first.
    pub fn items(&self) -> &[IdeNotification] {
        &self.items
    }

    /// Raises a notification as the settings of its group say: as a balloon, in the tool window's
    /// list, both, or not at all.
    pub fn notify(
        &mut self,
        request: NotificationRequest,
        settings: Option<&NotificationSettings>,
        now_ms: f64,
    ) -> Notified {
        self.seq += 1;
        let id = self.seq;
        let group = request.group_id().to_owned();
        let prefs = notification_prefs(settings, &group);
        // The answer to a command the user ran is not a notification they opted out of.
        let balloon = request.user_initiated || (prefs.balloons && prefs.display != NotificationDisplay::NoPopup);
        let sticky = prefs.display == NotificationDisplay::Sticky
            || request.sticky.unwrap_or(level_balloon_timeout(request.level).is_none());
        // Neither shown nor listed: dropped, as IDEA's "No popup" + no log.
        if !balloon && !prefs.log {
            return Notified { id, play_sound: false };
        }
        if let Some(key) = request.key.as_deref().filter(|key| !key.is_empty()) {
            self.items.retain(|item| item.key.as_deref() != Some(key));
        }
        self.items.insert(
            0,
            IdeNotification {
                id,
                key: request.key,
                level: request.level,
                title: request.title,
                body: request.body,
                actions: request.actions,
                time: now_ms,
                read: false,
                verbatim: request.verbatim,
                balloon,
                sticky,
                group,
                logged: prefs.log,
            },
        );
        self.items.truncate(MAX_HISTORY);
        Notified { id, play_sound: balloon && prefs.sound }
    }

    /// Takes the balloon of each matching notification away. One that is not logged only existed
    /// for its balloon and goes with it.
    fn hide_balloons(&mut self, matches: impl Fn(&IdeNotification) -> bool) {
        self.items.retain_mut(|item| {
            if !matches(item) {
                return true;
            }
            item.balloon = false;
            item.logged
        });
    }

    pub fn dismiss_balloon(&mut self, id: u64) {
        self.hide_balloons(|item| item.id == id);
    }

    /// Hides the balloon for `key`: the condition it reported no longer holds.
    pub fn expire_notification(&mut self, key: &str) {
        self.hide_balloons(|item| item.balloon && item.key.as_deref() == Some(key));
    }

    /// Hides the balloons of `group`, once [`suppress_group`] is stored.
    pub fn hide_group_balloons(&mut self, group: &str) {
        self.hide_balloons(|item| item.balloon && item.group == group);
    }

    pub fn remove_notification(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// A project switch drops the previous project's notifications; app-wide ones (updates) stay.
    pub fn clear_project_notifications(&mut self) {
        self.items.retain(|item| item.group == UPDATES_GROUP);
    }

    /// The list was seen: everything is read and no longer shown as a balloon.
    pub fn mark_all_read(&mut self) {
        self.items.retain_mut(|item| {
            item.read = true;
            item.balloon = false;
            item.logged
        });
    }

    /// The balloon stack, top to bottom (newest at the bottom): at most [`MAX_BALLOONS`], and
    /// none while the Notifications tool window is open.
    pub fn balloons(&self, list_open: bool) -> Vec<&IdeNotification> {
        if list_open {
            return Vec::new();
        }
        let mut balloons: Vec<&IdeNotification> =
            self.items.iter().filter(|item| item.balloon).take(MAX_BALLOONS).collect();
        balloons.reverse();
        balloons
    }

    /// The history the Notifications tool window lists, newest first.
    pub fn logged(&self) -> impl Iterator<Item = &IdeNotification> {
        self.items.iter().filter(|item| item.logged)
    }

    /// Unread count on the Notifications stripe button.
    pub fn unread_count(&self) -> usize {
        self.logged().filter(|item| !item.read).count()
    }

    /// Whether showing the list changes anything: the open tool window calls
    /// [`Self::mark_all_read`] while this holds.
    pub fn has_unseen(&self) -> bool {
        self.items.iter().any(|item| !item.read || item.balloon)
    }
}

/// What the balloon clocks depend on besides the notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BalloonView {
    /// The Notifications tool window is open (it hides the balloons).
    pub list_open: bool,
    /// The app window is active (focused).
    pub window_active: bool,
    /// The balloon under the pointer.
    pub hovered: Option<u64>,
}

impl Default for BalloonView {
    fn default() -> Self {
        Self { list_open: false, window_active: true, hovered: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BalloonClock {
    id: u64,
    /// What is left of the balloon's time once the clock (re)starts.
    remaining_ms: f64,
    /// Since when the clock has been running; `None` while paused.
    running_since: Option<f64>,
}

impl BalloonClock {
    fn deadline(&self) -> Option<f64> {
        self.running_since.map(|since| since + self.remaining_ms)
    }
}

/// The fade-out clocks of the balloons on screen. IDEA's smart fade-out: a clock runs only while
/// the window is active and its balloon not hovered, and a balloon that leaves the screen (pushed
/// out by newer ones, or hidden by the tool window) starts over when it comes back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BalloonClocks {
    clocks: Vec<BalloonClock>,
}

impl BalloonClocks {
    /// Starts, pauses and drops clocks to match what is on screen. Call it after every change to
    /// the notifications or to `view`.
    pub fn sync(&mut self, notifications: &Notifications, view: BalloonView, now_ms: f64) {
        let timed: Vec<(u64, f64)> = notifications
            .balloons(view.list_open)
            .into_iter()
            .filter_map(|notification| Some((notification.id, balloon_timeout(notification)?)))
            .collect();
        self.clocks.retain(|clock| timed.iter().any(|(id, _)| *id == clock.id));
        for (id, timeout) in timed {
            if !self.clocks.iter().any(|clock| clock.id == id) {
                self.clocks.push(BalloonClock { id, remaining_ms: timeout, running_since: None });
            }
            let Some(clock) = self.clocks.iter_mut().find(|clock| clock.id == id) else { continue };
            let running = view.window_active && view.hovered != Some(id);
            match (running, clock.running_since) {
                (true, None) => clock.running_since = Some(now_ms),
                (false, Some(since)) => {
                    clock.remaining_ms = (clock.remaining_ms - (now_ms - since)).max(1.0);
                    clock.running_since = None;
                }
                (true, Some(_)) | (false, None) => {}
            }
        }
    }

    /// When [`Self::fade_out`] next has something to do, if the clocks keep running as they are.
    pub fn next_deadline(&self) -> Option<f64> {
        self.clocks.iter().filter_map(BalloonClock::deadline).min_by(|a, b| a.total_cmp(b))
    }

    /// Dismisses the balloons whose time is up and returns their ids. They stay unread in the
    /// Notifications tool window.
    pub fn fade_out(&mut self, notifications: &mut Notifications, view: BalloonView, now_ms: f64) -> Vec<u64> {
        let due: Vec<u64> = self
            .clocks
            .iter()
            .filter(|clock| clock.deadline().is_some_and(|deadline| deadline <= now_ms))
            .map(|clock| clock.id)
            .collect();
        for id in &due {
            notifications.dismiss_balloon(*id);
        }
        self.sync(notifications, view, now_ms);
        due
    }
}

#[cfg(test)]
mod tests {
    use super::NotificationLevel::{Error, Info, Warning};
    use super::*;

    /// The notifications of a window with their balloon clocks, and a clock to advance.
    #[derive(Default)]
    struct Harness {
        store: Notifications,
        clocks: BalloonClocks,
        view: BalloonView,
        settings: Option<NotificationSettings>,
        now: f64,
    }

    impl Harness {
        fn with_settings(settings: Value) -> Self {
            Self { settings: notification_settings(&json!({ "notifications": settings })), ..Self::default() }
        }

        fn notify(&mut self, request: NotificationRequest) -> Notified {
            let notified = self.store.notify(request, self.settings.as_ref(), self.now);
            self.sync();
            notified
        }

        fn sync(&mut self) {
            self.clocks.sync(&self.store, self.view, self.now);
        }

        fn set_view(&mut self, view: BalloonView) {
            self.view = view;
            self.sync();
        }

        /// Lets `ms` pass, fading balloons out as their time comes.
        fn advance(&mut self, ms: f64) {
            let end = self.now + ms;
            while let Some(due) = self.clocks.next_deadline().filter(|due| *due <= end) {
                self.now = due;
                self.clocks.fade_out(&mut self.store, self.view, self.now);
            }
            self.now = end;
        }

        fn dismiss(&mut self, id: u64) {
            self.store.dismiss_balloon(id);
            self.sync();
        }

        fn titles(&self) -> Vec<&str> {
            self.store.items().iter().map(|item| item.title.as_str()).collect()
        }

        /// The titles of the balloons on screen.
        fn balloons(&self) -> Vec<&str> {
            self.store.balloons(self.view.list_open).into_iter().map(|item| item.title.as_str()).collect()
        }

        fn find(&self, title: &str) -> Option<&IdeNotification> {
            self.store.items().iter().find(|item| item.title == title)
        }

        fn keyed(&self, key: &str) -> Option<&IdeNotification> {
            self.store.items().iter().find(|item| item.key.as_deref() == Some(key))
        }
    }

    fn request(level: NotificationLevel, title: &str) -> NotificationRequest {
        NotificationRequest::new(level, title)
    }

    fn keyed(level: NotificationLevel, title: &str, key: &str) -> NotificationRequest {
        NotificationRequest { key: Some(key.to_owned()), ..request(level, title) }
    }

    fn action(label: &str) -> NotificationAction {
        NotificationAction { label: label.to_owned(), command: label.to_owned() }
    }

    fn labels<'a>(actions: &'a [ShownAction<'a>]) -> Vec<&'a str> {
        actions.iter().map(ShownAction::label).collect()
    }

    // src/workbench/projectNotifications.test.ts › notifications across project changes › keeps app-level
    // update notifications and drops project ones
    #[test]
    fn keeps_app_level_update_notifications_and_drops_project_ones() {
        let mut harness = Harness::default();
        harness.notify(NotificationRequest {
            sticky: Some(true),
            ..keyed(Info, "Downloading OpenIDEA 0.1.35", "app.update")
        });
        harness.notify(NotificationRequest { group: Some("vcs".to_owned()), ..request(Info, "Git refresh failed") });
        harness.store.clear_project_notifications();
        assert_eq!(harness.titles(), ["Downloading OpenIDEA 0.1.35"]);
    }

    // src/workbench/Workbench.test.tsx › notifications › shows balloons; info auto-dismisses, errors stay
    #[test]
    fn shows_balloons_info_auto_dismisses_errors_stay() {
        let mut harness = Harness::default();
        harness.notify(request(Info, "Saved"));
        let failed =
            harness.notify(NotificationRequest { actions: vec![action("Show log")], ..request(Error, "Build failed") });
        assert_eq!(harness.balloons(), ["Saved", "Build failed"]);
        harness.advance(BALLOON_TIMEOUT + 10.0);
        assert_eq!(harness.balloons(), ["Build failed"]);
        // Running an action of a balloon dismisses it.
        let shown = notification_actions(&harness.store.items()[0], None);
        assert_eq!(labels(&shown), ["Show log"]);
        harness.dismiss(failed.id);
        assert!(harness.balloons().is_empty());
        assert_eq!(harness.store.items().len(), 2);
    }

    // src/workbench/Workbench.test.tsx › notifications › explicitly non-sticky errors fade after the ordinary
    // balloon timeout
    #[test]
    fn explicitly_non_sticky_errors_fade_after_the_ordinary_balloon_timeout() {
        let mut harness = Harness::default();
        harness.notify(NotificationRequest { sticky: Some(false), ..request(Error, "Formatting failed") });
        assert_eq!(harness.balloons(), ["Formatting failed"]);
        assert_eq!(balloon_timeout(&harness.store.items()[0]), Some(BALLOON_TIMEOUT));
        harness.advance(BALLOON_TIMEOUT + 10.0);
        assert!(harness.balloons().is_empty());
        let item = &harness.store.items()[0];
        assert!(!item.sticky && !item.balloon);
    }

    // src/workbench/Workbench.test.tsx › notifications › sticky balloons fade into the tool window after
    // 5 minutes, staying unread
    #[test]
    fn sticky_balloons_fade_into_the_tool_window_after_5_minutes_staying_unread() {
        let mut harness = Harness::default();
        harness.notify(request(Error, "Sync failed"));
        harness.advance(STICKY_BALLOON_TIMEOUT - 1000.0);
        assert_eq!(harness.balloons(), ["Sync failed"]);
        harness.advance(1100.0);
        assert!(harness.balloons().is_empty());
        let item = &harness.store.items()[0];
        assert_eq!((item.title.as_str(), item.read, item.balloon), ("Sync failed", false, false));
        assert_eq!(harness.store.unread_count(), 1);
    }

    // src/workbench/Workbench.test.tsx › notifications › pauses the balloon timeout while the app window is
    // inactive
    #[test]
    fn pauses_the_balloon_timeout_while_the_app_window_is_inactive() {
        let mut harness = Harness::default();
        harness.notify(request(Info, "Indexed"));
        harness.advance(BALLOON_TIMEOUT - 1000.0);
        harness.set_view(BalloonView { window_active: false, ..BalloonView::default() });
        harness.advance(60_000.0);
        assert_eq!(harness.balloons(), ["Indexed"]);
        harness.set_view(BalloonView::default());
        harness.advance(1100.0);
        assert!(harness.balloons().is_empty());
    }

    // src/workbench/Workbench.test.tsx › notifications › replaces notifications with the same key and
    // subscribes to backend notify events
    #[test]
    fn replaces_notifications_with_the_same_key() {
        let mut harness = Harness::default();
        harness.notify(keyed(Warning, "LSP missing", "lsp"));
        harness.notify(keyed(Warning, "LSP still missing", "lsp"));
        // What a backend `notify` event raises: its level and title.
        harness.notify(request(Info, "From backend"));
        assert_eq!(harness.titles(), ["From backend", "LSP still missing"]);
        assert_eq!(harness.store.items()[1].group, "lsp");
    }

    // src/workbench/Workbench.test.tsx › notifications › the Notifications tool window lists history, marks
    // it read and hides balloons
    #[test]
    fn the_notifications_tool_window_lists_history_marks_it_read_and_hides_balloons() {
        let mut harness = Harness::default();
        harness.notify(request(Info, "Hello"));
        assert_eq!(harness.balloons(), ["Hello"]);
        assert_eq!(harness.store.logged().count(), 1);
        assert!(!harness.store.items().iter().all(|item| item.read));
        // The tool window opens: it shows the list and marks what it shows as seen.
        harness.set_view(BalloonView { list_open: true, ..BalloonView::default() });
        assert!(harness.store.has_unseen());
        harness.store.mark_all_read();
        harness.sync();
        assert!(harness.balloons().is_empty());
        assert_eq!(harness.store.logged().count(), 1);
        assert!(harness.store.items().iter().all(|item| item.read && !item.balloon));
        assert!(!harness.store.has_unseen());
        assert_eq!(harness.clocks.next_deadline(), None);
    }

    // src/workbench/Workbench.test.tsx › notifications › pops up the answer to a command the user ran even
    // when balloons are off for its group
    #[test]
    fn pops_up_the_answer_to_a_command_the_user_ran_even_when_balloons_are_off_for_its_group() {
        let mut harness = Harness::with_settings(
            json!({ "balloons": false, "groups": { "updates": { "display": "none", "log": true } } }),
        );
        harness
            .notify(NotificationRequest {
                user_initiated: true, ..keyed(Info, "Checking for updates…", "app.update")
            });
        harness.notify(keyed(Info, "Update available", "app.update.background"));
        assert_eq!(harness.find("Checking for updates…").map(|item| item.balloon), Some(true));
        // What the IDE raises on its own still follows the settings.
        assert_eq!(harness.find("Update available").map(|item| item.balloon), Some(false));
    }

    // src/workbench/Workbench.test.tsx › Don't Show Again › is offered for non-error notifications of groups
    // still shown as balloons
    #[test]
    fn dont_show_again_is_offered_for_non_error_notifications_of_groups_still_shown_as_balloons() {
        let base = IdeNotification {
            id: 1,
            key: None,
            level: Info,
            title: "x".to_owned(),
            body: None,
            actions: vec![action("Open")],
            time: 0.0,
            read: false,
            verbatim: false,
            balloon: true,
            sticky: false,
            group: "vcs".to_owned(),
            logged: true,
        };
        assert_eq!(labels(&notification_actions(&base, None)), ["Open", "Don't Show Again"]);
        let error = IdeNotification { level: Error, actions: Vec::new(), ..base };
        assert!(notification_actions(&error, None).is_empty());
        let settings =
            notification_settings(&json!({ "notifications": { "groups": { "vcs": { "display": "none" } } } }));
        assert!(!can_suppress(settings.as_ref(), Warning, "vcs"));
        assert!(can_suppress(settings.as_ref(), Warning, "run"));
    }

    // src/workbench/distractionFree.test.tsx › distraction free mode › exit hint › tells how to leave the
    // mode on entering, with an Exit action, and expires on exit
    #[test]
    fn a_keyed_hint_shows_as_a_balloon_and_expires_with_its_cause() {
        let mut harness = Harness::default();
        let hint = NotificationRequest {
            group: Some("distractionFree".to_owned()),
            body: Some("To exit: View | Appearance | Exit Distraction Free Mode".to_owned()),
            actions: vec![action("Exit Distraction Free Mode")],
            ..keyed(Info, "Distraction Free Mode", "distractionFree.hint")
        };
        harness.notify(hint.clone());
        let shown = harness.keyed("distractionFree.hint").unwrap();
        assert!(shown.balloon);
        assert_eq!(shown.body, hint.body);
        assert_eq!(shown.actions, hint.actions);
        harness.store.expire_notification("distractionFree.hint");
        assert_eq!(harness.keyed("distractionFree.hint").map(|item| item.balloon), Some(false));
    }

    // src/workbench/distractionFree.test.tsx › distraction free mode › exit hint › offers Don't Show Again
    // once, scoped to its own notification group
    #[test]
    fn a_hint_offers_dont_show_again_once_scoped_to_its_own_notification_group() {
        let mut harness = Harness::default();
        harness.notify(NotificationRequest {
            group: Some("distractionFree".to_owned()),
            actions: vec![action("Exit Distraction Free Mode")],
            ..keyed(Info, "Distraction Free Mode", "distractionFree.hint")
        });
        let hint = harness.keyed("distractionFree.hint").unwrap();
        let shown = notification_actions(hint, None);
        assert_eq!(labels(&shown).into_iter().filter(|label| *label == "Don't Show Again").count(), 1);
        assert_eq!(hint.group, "distractionFree");
    }

    // src/workbench/distractionFree.test.tsx › distraction free mode › exit hint › stays off once its group
    // shows no popup
    #[test]
    fn a_hint_stays_off_once_its_group_shows_no_popup() {
        let mut harness = Harness::with_settings(json!({ "groups": { "distractionFree": { "display": "none" } } }));
        harness.notify(NotificationRequest {
            group: Some("distractionFree".to_owned()),
            ..keyed(Info, "Distraction Free Mode", "distractionFree.hint")
        });
        assert_eq!(harness.keyed("distractionFree.hint").map(|item| item.balloon), Some(false));
        assert!(harness.balloons().is_empty());
    }

    // src/features/app/__tests__/updates.test.ts › Check for Updates › still pops up when IDE Updates is
    // set to "No popup" (no other surface on the welcome screen)
    #[test]
    fn a_manual_update_check_still_pops_up_when_ide_updates_is_set_to_no_popup() {
        // What Settings › Notifications › IDE Updates stores for "No popup" (or "Don't Show Again").
        let mut harness = Harness::with_settings(
            json!({ "balloons": true, "groups": { "updates": { "display": "none", "log": true } } }),
        );
        let checking =
            NotificationRequest { user_initiated: true, ..keyed(Info, "Checking for updates…", "app.update") };
        harness.notify(checking);
        let first = &harness.store.items()[0];
        assert_eq!(
            (first.key.as_deref(), first.balloon, first.title.as_str()),
            (Some("app.update"), true, "Checking for updates…")
        );
        let done =
            NotificationRequest { user_initiated: true, ..keyed(Info, "OpenIDEA 0.1.21 is up to date", "app.update") };
        harness.notify(done);
        assert_eq!(harness.titles(), ["OpenIDEA 0.1.21 is up to date"]);
        assert!(harness.store.items()[0].balloon);
    }

    #[test]
    fn dont_show_again_stores_no_popup_for_the_group_and_hides_its_balloons() {
        let mut harness = Harness::with_settings(json!({ "groups": { "run": { "display": "sticky", "log": false } } }));
        let vcs = harness.notify(NotificationRequest { group: Some("vcs".to_owned()), ..request(Info, "Fetched") });
        let run = harness.notify(NotificationRequest { group: Some("run".to_owned()), ..request(Info, "Started") });
        // Sticky by its group, and unlisted: it stays until dismissed.
        let started = harness.find("Started").unwrap();
        assert!(started.sticky && !started.logged);
        assert_eq!(balloon_timeout(started), None);

        let write = suppress_group(harness.settings.as_ref(), "vcs");
        assert_eq!(write.key, "notifications.groups");
        assert_eq!(
            write.value,
            json!({ "run": { "display": "sticky", "log": false }, "vcs": { "display": "none", "log": true } })
        );
        harness.store.hide_group_balloons("vcs");
        assert_eq!(harness.find("Fetched").map(|item| (item.id, item.balloon)), Some((vcs.id, false)));

        // An unlisted notification only existed for its balloon.
        let write = suppress_group(harness.settings.as_ref(), "run");
        assert_eq!(write.value["run"], json!({ "display": "none", "log": false }));
        harness.store.hide_group_balloons("run");
        assert_eq!(harness.find("Started"), None);
        assert_eq!(run.id, vcs.id + 1);
    }

    #[test]
    fn drops_what_is_neither_shown_nor_listed_and_plays_the_sound_only_for_balloons() {
        let mut harness = Harness::with_settings(json!({
            "sound": true,
            "groups": { "search": { "display": "none", "log": false }, "vcs": { "display": "none" } },
        }));
        let dropped =
            harness.notify(NotificationRequest { group: Some("search".to_owned()), ..request(Info, "Found") });
        assert_eq!(dropped, Notified { id: 1, play_sound: false });
        assert!(harness.store.items().is_empty());
        let listed = harness.notify(NotificationRequest { group: Some("vcs".to_owned()), ..request(Info, "Fetched") });
        assert_eq!(listed, Notified { id: 2, play_sound: false });
        assert_eq!(harness.notify(request(Info, "Saved")), Notified { id: 3, play_sound: true });
        assert_eq!(harness.store.unread_count(), 2);
    }

    #[test]
    fn a_balloon_pushed_off_screen_starts_its_time_over_and_hovering_pauses_it() {
        let mut harness = Harness::default();
        let first = harness.notify(request(Info, "One"));
        harness.advance(9000.0);
        for title in ["Two", "Three", "Four"] {
            harness.notify(request(Info, title));
        }
        assert_eq!(harness.balloons(), ["Two", "Three", "Four"]);
        harness.advance(4000.0);
        harness.set_view(BalloonView { hovered: Some(first.id + 3), ..BalloonView::default() });
        harness.advance(6000.0);
        // "Two" and "Three" faded; "One" came back on screen then and has its whole time again.
        assert_eq!(harness.balloons(), ["One", "Four"]);
        harness.set_view(BalloonView::default());
        // "Four" has the 6 s left that it had when the pointer came over it.
        harness.advance(6000.0);
        assert_eq!(harness.balloons(), ["One"]);
        harness.advance(BALLOON_TIMEOUT - 6000.0 - 1.0);
        assert_eq!(harness.balloons(), ["One"]);
        harness.advance(1.0);
        assert!(harness.balloons().is_empty());
    }
}
