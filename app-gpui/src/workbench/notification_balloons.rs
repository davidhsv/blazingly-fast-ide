//! The notification balloons: the stack at the bottom right of the window, the newest at the bottom.
//!
//! [`NotificationBalloons`] draws the balloons it is given with [`NotificationBalloons::set`] and
//! reports what the user does to them as [`BalloonEvent`]s. It keeps no notification of its own and
//! runs no clock: the window owns the [`Notifications`] and their fade-out clocks, applies each
//! event to them and passes the new balloons in.
//!
//! A balloon and an entry of the Notifications tool window are the same [`toast`]: a level icon,
//! a title, a body that scrolls when it is long, the actions as links and a close button.

use super::notifications::{
    DONT_SHOW_AGAIN, IdeNotification, NotificationAction, NotificationLevel, NotificationSettings, Notifications,
    ShownAction, notification_actions,
};
use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{
    App, BoxShadow, Context, Deferred, Div, Entity, EventEmitter, FontWeight, HoverListenerMode, IntoElement, Render,
    Rgba, SharedString, Stateful, Window, deferred, div, prelude::*, px,
};
use std::rc::Rc;

/// How wide a balloon is.
const BALLOON_WIDTH: f32 = 340.0;
/// The room between two balloons.
const STACK_GAP: f32 = 8.0;
/// How far the stack stays from the window's right edge: inside the editor island's bottom right
/// corner, clear of the right stripe.
const STACK_RIGHT: f32 = 78.0;
/// How far the stack stays above the status bar.
const STACK_ABOVE_STATUS_BAR: f32 = 16.0;
/// A balloon never grows to within this much of the window's height: its close button stays
/// reachable.
const WINDOW_MARGIN: f32 = 48.0;
/// The share of the window's height a body takes before it scrolls: a huge message (a stack
/// trace, build output) scrolls inside its toast instead of growing past the window.
const BODY_MAX_HEIGHT: f32 = 0.4;

/// A button of a shown notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAction {
    /// One of the notification's own: the window runs its command.
    Own(NotificationAction),
    /// "Don't Show Again": the window turns the balloons of `group` off.
    DontShowAgain { group: String },
}

impl EntryAction {
    /// The label as it was given, in English.
    pub fn label(&self) -> &str {
        match self {
            Self::Own(action) => &action.label,
            Self::DontShowAgain { .. } => DONT_SHOW_AGAIN,
        }
    }
}

/// A notification as a view draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationEntry {
    pub id: u64,
    pub level: NotificationLevel,
    pub title: String,
    pub body: Option<String>,
    /// The buttons, in the order they show.
    pub actions: Vec<EntryAction>,
    /// Title, body and action labels come from outside the IDE and are shown as written.
    pub verbatim: bool,
    /// When it was raised, in milliseconds since the epoch.
    pub time: f64,
}

impl NotificationEntry {
    /// `notification` with the buttons the Notifications settings give it: its own, then "Don't
    /// Show Again" while its group can still be silenced.
    pub fn new(notification: &IdeNotification, settings: Option<&NotificationSettings>) -> Self {
        let actions = notification_actions(notification, settings)
            .into_iter()
            .map(|action| match action {
                ShownAction::Own(own) => EntryAction::Own(own.clone()),
                ShownAction::DontShowAgain => EntryAction::DontShowAgain { group: notification.group.clone() },
            })
            .collect();
        Self {
            id: notification.id,
            level: notification.level,
            title: notification.title.clone(),
            body: notification.body.clone(),
            actions,
            verbatim: notification.verbatim,
            time: notification.time,
        }
    }
}

/// The balloons of `notifications` as [`NotificationBalloons::set`] takes them: top to bottom,
/// and none while the Notifications tool window is open.
pub fn balloon_entries(
    notifications: &Notifications,
    settings: Option<&NotificationSettings>,
    list_open: bool,
) -> Vec<NotificationEntry> {
    notifications
        .balloons(list_open)
        .into_iter()
        .map(|notification| NotificationEntry::new(notification, settings))
        .collect()
}

/// The icon of a level's notifications.
pub fn level_icon(level: NotificationLevel) -> &'static str {
    match level {
        NotificationLevel::Info => "info",
        NotificationLevel::Warning => "warning",
        NotificationLevel::Error => "error",
    }
}

/// The colour token a level's icon is drawn in.
fn level_color(level: NotificationLevel) -> &'static str {
    match level {
        NotificationLevel::Info => "info",
        NotificationLevel::Warning => "warning",
        NotificationLevel::Error => "danger",
    }
}

/// English UI text in the UI language.
pub(super) fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// What an entry says, in the UI language.
pub(super) struct ShownText {
    pub title: String,
    pub body: Option<String>,
    /// A label per action of the entry.
    pub actions: Vec<String>,
}

/// The text of `entry` as it shows. The title and the entry's own action labels may be built from
/// values (`"{name} failed"`), so they are matched against the catalog's templates; text from
/// outside the IDE stays as written, and "Don't Show Again" is the IDE's own in either case. Line
/// breaks in a body flow as spaces.
pub(super) fn shown_text(entry: &NotificationEntry, cx: &App) -> ShownText {
    let i18n = cx.try_global::<Localization>().map(|localization| &localization.0);
    let template = |text: &str| i18n.map_or_else(|| text.to_owned(), |i18n| i18n.tx_template(text));
    let given = |text: &str| if entry.verbatim { text.to_owned() } else { template(text) };
    let body = entry.body.as_deref().map(|body| {
        let body = if entry.verbatim { body.to_owned() } else { tx(cx, body) };
        body.split_whitespace().collect::<Vec<_>>().join(" ")
    });
    ShownText {
        title: given(&entry.title),
        body: body.filter(|body| !body.is_empty()),
        actions: entry
            .actions
            .iter()
            .map(|action| match action {
                EntryAction::Own(own) => given(&own.label),
                EntryAction::DontShowAgain { .. } => template(DONT_SHOW_AGAIN),
            })
            .collect(),
    }
}

/// An entry on one line, as it reads: `error: Build failed — error: E0425 [Show log]`.
pub(super) fn entry_line(entry: &NotificationEntry, cx: &App) -> String {
    let text = shown_text(entry, cx);
    let level = match entry.level {
        NotificationLevel::Info => "info",
        NotificationLevel::Warning => "warning",
        NotificationLevel::Error => "error",
    };
    let mut line = format!("{level}: {}", text.title);
    if let Some(body) = &text.body {
        line.push_str(" — ");
        line.push_str(body);
    }
    for label in &text.actions {
        line.push_str(&format!(" [{label}]"));
    }
    line
}

/// What a click on a toast asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToastClick {
    Close,
    /// The action at this index of the entry's actions.
    Action(usize),
}

/// What a toast calls with each click on its close button or on one of its actions.
pub(super) type ToastHandler = Rc<dyn Fn(&ToastClick, &mut Window, &mut App)>;

/// A notification, drawn: the level's icon, the title with `meta` beside it, the body, the
/// actions as links and a close button in the top right corner. The caller gives it its frame
/// (width, background, border). `selector` names it and its parts for click tests: the toast
/// itself, `<selector>-body`, `<selector>-action-<index>` and `<selector>-close`.
pub(super) fn toast(
    entry: &NotificationEntry,
    selector: &str,
    meta: Option<String>,
    on_click: ToastHandler,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    let text = shown_text(entry, cx);
    let (link, link_hover) = (theme::color(cx, "link"), theme::color(cx, "link-hover"));
    let hover_bg = theme::color(cx, "bg-hover");
    let head = div()
        .flex()
        .items_baseline()
        .gap(px(8.0))
        .min_h(px(16.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(theme::color(cx, "fg-strong"))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(text.title)),
        )
        .children(meta.map(|meta| {
            div()
                .flex_none()
                .text_color(theme::color(cx, "fg-subtle"))
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(meta))
        }));
    let body = text.body.map(|body| {
        div()
            .id("body")
            .debug_selector(|| format!("{selector}-body"))
            .max_h(window.viewport_size().height * BODY_MAX_HEIGHT)
            .overflow_y_scroll()
            .text_color(theme::color(cx, "fg-secondary"))
            .child(SharedString::from(body))
    });
    let actions = (!text.actions.is_empty()).then(|| {
        div().flex().flex_wrap().gap(px(14.0)).mt(px(4.0)).children(text.actions.into_iter().enumerate().map(
            |(index, label)| {
                let on_click = Rc::clone(&on_click);
                div()
                    .id(("action", index))
                    .debug_selector(|| format!("{selector}-action-{index}"))
                    .text_color(link)
                    .cursor_pointer()
                    .hover(|el| el.text_color(link_hover))
                    .child(SharedString::from(label))
                    .on_click(move |_, window, cx| on_click(&ToastClick::Action(index), window, cx))
            },
        ))
    });
    let close = div()
        .id("close")
        .debug_selector(|| format!("{selector}-close"))
        .absolute()
        .top(px(8.0))
        .right(px(8.0))
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .hover(|el| el.bg(hover_bg))
        .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-muted")))
        .on_click(move |_, window, cx| on_click(&ToastClick::Close, window, cx));
    div()
        .id(("notification", entry.id as usize))
        .debug_selector(|| selector.to_owned())
        .flex()
        .items_start()
        .gap(px(10.0))
        .py(px(14.0))
        .pl(px(14.0))
        // Room for the close button.
        .pr(px(34.0))
        .text_color(theme::color(cx, "fg"))
        .child(theme::icon(cx, level_icon(entry.level), 16.0, theme::color(cx, level_color(entry.level))))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(4.0)).child(head).children(body).children(actions))
        .child(close)
}

/// A drop shadow as a `--shadow-*` token writes it: `<x> <y> <blur> <colour>`.
fn parse_shadow(value: &str) -> Option<BoxShadow> {
    let mut parts = value.split_whitespace();
    let mut lengths = [0.0_f32; 3];
    for length in &mut lengths {
        *length = parts.next()?.trim_end_matches("px").parse().ok()?;
    }
    let color = theme::tokens::parse_color(&parts.collect::<Vec<_>>().join(" "))?;
    let color = Rgba { r: color.r, g: color.g, b: color.b, a: color.a };
    Some(BoxShadow::new(px(lengths[0]), px(lengths[1]), color.into()).blur_radius(px(lengths[2])))
}

/// The shadow under a balloon; none when the theme does not define one.
fn balloon_shadow(cx: &App) -> Vec<BoxShadow> {
    cx.try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.resolved("shadow-balloon"))
        .and_then(|value| parse_shadow(&value))
        .into_iter()
        .collect()
}

/// What the balloons ask of their window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalloonEvent {
    /// The close button of this notification's balloon.
    Dismissed(u64),
    /// An action of this notification's balloon. The balloon goes away and the action runs.
    ActionChosen { id: u64, action: EntryAction },
    /// The balloon under the pointer is now this one, or none: a balloon does not fade out while
    /// the pointer is over it.
    HoverChanged(Option<u64>),
}

/// The balloon stack.
#[derive(Default)]
pub struct NotificationBalloons {
    /// The balloons on screen, top to bottom.
    balloons: Vec<NotificationEntry>,
    /// The balloon under the pointer.
    hovered: Option<u64>,
}

impl EventEmitter<BalloonEvent> for NotificationBalloons {}

impl NotificationBalloons {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows `balloons`, top to bottom. A balloon that goes away under the pointer is no longer
    /// the hovered one, and the window is told so.
    pub fn set(&mut self, balloons: Vec<NotificationEntry>, cx: &mut Context<Self>) {
        if self.balloons == balloons {
            return;
        }
        self.balloons = balloons;
        if self.hovered.is_some_and(|hovered| !self.balloons.iter().any(|balloon| balloon.id == hovered)) {
            self.set_hovered(None, cx);
        }
        cx.notify();
    }

    /// The balloons on screen, top to bottom.
    pub fn shown(&self) -> &[NotificationEntry] {
        &self.balloons
    }

    /// What each balloon says, top to bottom: `<level>: <title> — <body> [<action>]…`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        self.balloons.iter().map(|balloon| entry_line(balloon, cx)).collect()
    }

    /// The balloon under the pointer.
    pub fn hovered(&self) -> Option<u64> {
        self.hovered
    }

    fn set_hovered(&mut self, hovered: Option<u64>, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.emit(BalloonEvent::HoverChanged(hovered));
        }
    }

    /// The pointer came over the balloon of `id`, or left it. Leaving one balloon for another
    /// reports the two in either order: leaving the old one does not unset the new one.
    fn hover_changed(&mut self, id: u64, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered { Some(id) } else { self.hovered.filter(|current| *current != id) };
        self.set_hovered(next, cx);
    }

    fn clicked(&mut self, id: u64, click: ToastClick, cx: &mut Context<Self>) {
        let Some(balloon) = self.balloons.iter().find(|balloon| balloon.id == id) else { return };
        match click {
            ToastClick::Close => cx.emit(BalloonEvent::Dismissed(id)),
            ToastClick::Action(index) => {
                if let Some(action) = balloon.actions.get(index) {
                    cx.emit(BalloonEvent::ActionChosen { id, action: action.clone() });
                }
            }
        }
    }
}

impl Render for NotificationBalloons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let max_height = (window.viewport_size().height - px(WINDOW_MARGIN)).max(px(0.0));
        let shadow = balloon_shadow(cx);
        let (background, border) = (theme::color(cx, "bg-popup"), theme::color(cx, "border-popup"));
        let radius = theme::length(cx, "radius-popup-large");
        let balloons: Vec<Stateful<Div>> = self
            .balloons
            .iter()
            .map(|balloon| {
                let id = balloon.id;
                let on_click: ToastHandler = Rc::new(
                    cx.listener(move |stack: &mut Self, click: &ToastClick, _, cx| stack.clicked(id, *click, cx)),
                );
                toast(balloon, &format!("balloon-{id}"), None, on_click, window, cx)
                    .w(px(BALLOON_WIDTH))
                    .max_h(max_height)
                    .bg(background)
                    .border_1()
                    .border_color(border)
                    .rounded(radius)
                    .shadow(shadow.clone())
                    // A click on a balloon is not a click on what it covers.
                    .occlude()
                    // The pointer resting on a balloon keeps it while the user types.
                    .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
                    .on_hover(cx.listener(move |stack: &mut Self, hovered: &bool, _, cx| {
                        stack.hover_changed(id, *hovered, cx);
                    }))
            })
            .collect();
        // The gaps between the balloons let the pointer through to what is under the stack.
        div().flex().flex_col().items_end().gap(px(STACK_GAP)).children(balloons)
    }
}

/// The stack where a window shows it: over the window's content, at its bottom right corner above
/// the status bar. It is a child of the element that fills the window; popups drawn with a
/// higher priority cover it.
pub fn layer(balloons: &Entity<NotificationBalloons>, cx: &App) -> Deferred {
    deferred(
        div()
            .absolute()
            .right(px(STACK_RIGHT))
            .bottom(theme::length(cx, "h-statusbar") + px(STACK_ABOVE_STATUS_BAR))
            .child(balloons.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::super::notifications::NotificationLevel::{Error, Info, Warning};
    use super::super::notifications::{
        BALLOON_TIMEOUT, BalloonClocks, BalloonView, MAX_BALLOONS, NotificationRequest, STICKY_BALLOON_TIMEOUT,
        notification_settings,
    };
    use super::*;
    use gpui_kit::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, point};
    use serde_json::json;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BalloonEvent>>>;

    /// A window that shows the stack where a workbench window does.
    struct Host {
        stack: Entity<NotificationBalloons>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(layer(&self.stack, cx))
        }
    }

    /// The stack in a window of its own, and what it has asked of the window so far.
    fn open(cx: &mut TestAppContext) -> (Entity<NotificationBalloons>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (host, cx) = cx.add_window_view(|_, cx| Host { stack: cx.new(|_| NotificationBalloons::new()) });
        let stack = host.read_with(cx, |host, _| host.stack.clone());
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&stack, move |_, event: &BalloonEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (stack, events, cx)
    }

    /// A window's notifications and their fade-out clocks, kept in step with the stack as the
    /// window keeps them.
    #[derive(Default)]
    struct Bench {
        store: Notifications,
        clocks: BalloonClocks,
        view: BalloonView,
        now: f64,
    }

    impl Bench {
        /// Brings the clocks and the stack up to the notifications.
        fn sync(&mut self, stack: &Entity<NotificationBalloons>, cx: &mut VisualTestContext) {
            self.clocks.sync(&self.store, self.view, self.now);
            let balloons = balloon_entries(&self.store, None, self.view.list_open);
            stack.update(cx, |stack, cx| stack.set(balloons, cx));
            cx.run_until_parked();
        }

        fn notify(
            &mut self,
            request: NotificationRequest,
            stack: &Entity<NotificationBalloons>,
            cx: &mut VisualTestContext,
        ) -> u64 {
            let id = self.store.notify(request, None, self.now).id;
            self.sync(stack, cx);
            id
        }

        /// Lets `ms` pass, fading balloons out as their time comes.
        fn advance(&mut self, ms: f64, stack: &Entity<NotificationBalloons>, cx: &mut VisualTestContext) {
            let end = self.now + ms;
            while let Some(due) = self.clocks.next_deadline().filter(|due| *due <= end) {
                self.now = due;
                self.clocks.fade_out(&mut self.store, self.view, self.now);
            }
            self.now = end;
            self.sync(stack, cx);
        }

        /// Does what the stack asked for, as its window does.
        fn apply_all(&mut self, events: &Events, stack: &Entity<NotificationBalloons>, cx: &mut VisualTestContext) {
            let asked: Vec<BalloonEvent> = events.borrow_mut().drain(..).collect();
            for event in asked {
                match event {
                    BalloonEvent::Dismissed(id) => self.store.dismiss_balloon(id),
                    BalloonEvent::ActionChosen { id, action } => {
                        self.store.dismiss_balloon(id);
                        if let EntryAction::DontShowAgain { group } = action {
                            self.store.hide_group_balloons(&group);
                        }
                    }
                    BalloonEvent::HoverChanged(hovered) => self.view.hovered = hovered,
                }
            }
            self.sync(stack, cx);
        }
    }

    fn request(level: NotificationLevel, title: &str) -> NotificationRequest {
        NotificationRequest::new(level, title)
    }

    fn grouped(level: NotificationLevel, title: &str, group: &str) -> NotificationRequest {
        NotificationRequest { group: Some(group.to_owned()), ..request(level, title) }
    }

    fn action(label: &str) -> NotificationAction {
        NotificationAction { label: label.to_owned(), command: format!("test.{label}") }
    }

    fn lines(stack: &Entity<NotificationBalloons>, cx: &mut VisualTestContext) -> Vec<String> {
        stack.read_with(cx, |stack, cx| stack.lines(cx))
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = bounds_of(cx, selector).center();
        cx.simulate_click(at, Modifiers::default());
    }

    /// What the stack asked for, leaving out where the pointer is.
    fn chosen(events: &Events) -> Vec<BalloonEvent> {
        events.borrow().iter().filter(|event| !matches!(event, BalloonEvent::HoverChanged(_))).cloned().collect()
    }

    fn close_to(a: Pixels, b: Pixels) -> bool {
        (a - b).abs() < px(1.0)
    }

    // src/workbench/Workbench.test.tsx › notifications › shows balloons; info auto-dismisses, errors stay
    #[gpui_kit::test]
    async fn shows_balloons_info_auto_dismisses_errors_stay(cx: &mut TestAppContext) {
        let (stack, events, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(request(Info, "Saved"), &stack, cx);
        let failed = bench.notify(
            NotificationRequest { actions: vec![action("Show log")], ..request(Error, "Build failed") },
            &stack,
            cx,
        );
        assert_eq!(lines(&stack, cx), ["info: Saved [Don't Show Again]", "error: Build failed [Show log]"]);
        bench.advance(BALLOON_TIMEOUT + 10.0, &stack, cx);
        assert_eq!(lines(&stack, cx), ["error: Build failed [Show log]"]);
        // Running an action of a balloon dismisses it; the notification stays in the history.
        click(cx, "balloon-2-action-0");
        assert_eq!(
            chosen(&events),
            [BalloonEvent::ActionChosen { id: failed, action: EntryAction::Own(action("Show log")) }]
        );
        bench.apply_all(&events, &stack, cx);
        assert!(lines(&stack, cx).is_empty());
        assert_eq!(bench.store.items().len(), 2);
    }

    // src/workbench/Workbench.test.tsx › notifications › explicitly non-sticky errors fade after the ordinary
    // balloon timeout
    #[gpui_kit::test]
    async fn explicitly_non_sticky_errors_fade_after_the_ordinary_balloon_timeout(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(NotificationRequest { sticky: Some(false), ..request(Error, "Formatting failed") }, &stack, cx);
        assert_eq!(lines(&stack, cx), ["error: Formatting failed"]);
        bench.advance(BALLOON_TIMEOUT + 10.0, &stack, cx);
        assert!(lines(&stack, cx).is_empty());
        assert!(cx.debug_bounds("balloon-1").is_none());
    }

    // src/workbench/Workbench.test.tsx › notifications › sticky balloons fade into the tool window after
    // 5 minutes, staying unread
    #[gpui_kit::test]
    async fn sticky_balloons_fade_into_the_tool_window_after_5_minutes_staying_unread(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(request(Error, "Sync failed"), &stack, cx);
        bench.advance(STICKY_BALLOON_TIMEOUT - 1000.0, &stack, cx);
        assert_eq!(lines(&stack, cx), ["error: Sync failed"]);
        bench.advance(1100.0, &stack, cx);
        assert!(lines(&stack, cx).is_empty());
        assert_eq!(bench.store.unread_count(), 1);
    }

    // src/workbench/Workbench.test.tsx › notifications › pauses the balloon timeout while the app window is
    // inactive
    #[gpui_kit::test]
    async fn pauses_the_balloon_timeout_while_the_app_window_is_inactive(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(request(Info, "Indexed"), &stack, cx);
        bench.advance(BALLOON_TIMEOUT - 1000.0, &stack, cx);
        bench.view.window_active = false;
        bench.sync(&stack, cx);
        bench.advance(60_000.0, &stack, cx);
        assert_eq!(lines(&stack, cx), ["info: Indexed [Don't Show Again]"]);
        bench.view.window_active = true;
        bench.sync(&stack, cx);
        bench.advance(1100.0, &stack, cx);
        assert!(lines(&stack, cx).is_empty());
    }

    // src/workbench/Workbench.test.tsx › notifications › the Notifications tool window lists history, marks
    // it read and hides balloons
    #[gpui_kit::test]
    async fn the_notifications_tool_window_lists_history_marks_it_read_and_hides_balloons(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(request(Info, "Hello"), &stack, cx);
        assert_eq!(lines(&stack, cx), ["info: Hello [Don't Show Again]"]);
        // The tool window opens: no balloon shows over it, and what it lists is seen.
        bench.view.list_open = true;
        bench.sync(&stack, cx);
        assert!(lines(&stack, cx).is_empty());
        bench.store.mark_all_read();
        bench.view.list_open = false;
        bench.sync(&stack, cx);
        assert!(lines(&stack, cx).is_empty(), "a seen notification has no balloon to come back to");
        assert_eq!(bench.store.logged().count(), 1);
    }

    // src/workbench/Workbench.test.tsx › Don't Show Again › is offered for non-error notifications of groups
    // still shown as balloons
    #[gpui_kit::test]
    async fn dont_show_again_is_offered_for_non_error_notifications_of_groups_still_shown_as_balloons(
        cx: &mut TestAppContext,
    ) {
        let (stack, _, cx) = open(cx);
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
        let error = IdeNotification { id: 2, level: Error, actions: Vec::new(), ..base.clone() };
        let settings =
            notification_settings(&json!({ "notifications": { "groups": { "vcs": { "display": "none" } } } }));
        let silenced = IdeNotification { id: 3, level: Warning, ..base.clone() };
        let other = IdeNotification { id: 4, level: Warning, group: "run".to_owned(), ..base.clone() };
        let balloons = vec![
            NotificationEntry::new(&base, None),
            NotificationEntry::new(&error, None),
            NotificationEntry::new(&silenced, settings.as_ref()),
            NotificationEntry::new(&other, settings.as_ref()),
        ];
        assert_eq!(
            balloons[0].actions,
            [EntryAction::Own(action("Open")), EntryAction::DontShowAgain { group: "vcs".to_owned() }]
        );
        stack.update(cx, |stack, cx| stack.set(balloons, cx));
        assert_eq!(
            lines(&stack, cx),
            [
                "info: x [Open] [Don't Show Again]",
                "error: x",
                "warning: x [Open]",
                "warning: x [Open] [Don't Show Again]"
            ]
        );
    }

    #[gpui_kit::test]
    async fn dont_show_again_is_a_link_that_names_the_group_to_silence(cx: &mut TestAppContext) {
        let (stack, events, cx) = open(cx);
        let mut bench = Bench::default();
        bench.notify(grouped(Info, "Fetched", "vcs"), &stack, cx);
        bench.notify(grouped(Info, "Started", "run"), &stack, cx);
        let pushed = bench.notify(grouped(Warning, "Push rejected", "vcs"), &stack, cx);
        click(cx, "balloon-3-action-0");
        assert_eq!(
            chosen(&events),
            [BalloonEvent::ActionChosen { id: pushed, action: EntryAction::DontShowAgain { group: "vcs".to_owned() } }]
        );
        // The group's balloons go away; its notifications stay in the history.
        bench.apply_all(&events, &stack, cx);
        assert_eq!(lines(&stack, cx), ["info: Started [Don't Show Again]"]);
        assert_eq!(bench.store.logged().count(), 3);
    }

    // src/editor/lsp/formatErrors.test.tsx › keeps stderr untranslated in a Chinese notification and replaces
    // failures per file, including on save
    #[gpui_kit::test]
    async fn keeps_stderr_untranslated_in_a_chinese_notification(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        cx.update(|_, cx| {
            cx.default_global::<Localization>().0.set_locale(Some("zh"), "en");
        });
        let entry = |id: u64, verbatim: bool| NotificationEntry {
            id,
            level: Warning,
            title: "Build failed".to_owned(),
            body: Some("Formatting timed out".to_owned()),
            actions: vec![EntryAction::Own(action("Open")), EntryAction::DontShowAgain { group: "tools".to_owned() }],
            verbatim,
            time: 0.0,
        };
        stack.update(cx, |stack, cx| stack.set(vec![entry(1, true), entry(2, false)], cx));
        // Text from outside the IDE shows as written; the IDE's own button is in the UI language.
        assert_eq!(
            lines(&stack, cx),
            [
                "warning: Build failed — Formatting timed out [Open] [不再显示]",
                "warning: 构建失败 — 格式化超时 [打开] [不再显示]"
            ]
        );
    }

    #[gpui_kit::test]
    async fn a_title_built_from_a_value_is_translated_and_a_body_flows_on_one_paragraph(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let entry = NotificationEntry {
            id: 1,
            level: Error,
            title: "ruff is not installed".to_owned(),
            body: Some("error: E0425\n    at main.rs:3\n".to_owned()),
            actions: Vec::new(),
            verbatim: false,
            time: 0.0,
        };
        stack.update(cx, |stack, cx| stack.set(vec![entry.clone()], cx));
        assert_eq!(lines(&stack, cx), ["error: ruff is not installed — error: E0425 at main.rs:3"]);
        assert_eq!(stack.read_with(cx, |stack, _| stack.shown().to_vec()), std::slice::from_ref(&entry));
        cx.update(|_, cx| {
            cx.default_global::<Localization>().0.set_locale(Some("zh"), "en");
        });
        assert_eq!(lines(&stack, cx), ["error: ruff 未安装 — error: E0425 at main.rs:3"]);
        // An empty body is no body.
        let blank = NotificationEntry { body: Some(" \n".to_owned()), ..entry };
        stack.update(cx, |stack, cx| stack.set(vec![blank], cx));
        assert_eq!(lines(&stack, cx), ["error: ruff 未安装"]);
        assert!(cx.debug_bounds("balloon-1-body").is_none());
    }

    #[gpui_kit::test]
    async fn the_close_button_dismisses_its_balloon(cx: &mut TestAppContext) {
        let (stack, events, cx) = open(cx);
        let mut bench = Bench::default();
        let first = bench.notify(request(Error, "Build failed"), &stack, cx);
        bench.notify(request(Error, "Tests failed"), &stack, cx);
        click(cx, "balloon-1-close");
        assert_eq!(chosen(&events), [BalloonEvent::Dismissed(first)]);
        bench.apply_all(&events, &stack, cx);
        assert_eq!(lines(&stack, cx), ["error: Tests failed"]);
        // The notification is still listed, unread.
        assert_eq!((bench.store.items().len(), bench.store.unread_count()), (2, 2));
    }

    #[gpui_kit::test]
    async fn the_stack_shows_the_newest_balloons_at_the_bottom_right_above_the_status_bar(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        for title in ["One", "Two", "Three", "Four"] {
            bench.notify(request(Error, title), &stack, cx);
        }
        // No more than three show; the oldest waits for room.
        assert_eq!(lines(&stack, cx), ["error: Two", "error: Three", "error: Four"]);
        assert_eq!(lines(&stack, cx).len(), MAX_BALLOONS);
        assert!(cx.debug_bounds("balloon-1").is_none());
        let (two, three, four) = (bounds_of(cx, "balloon-2"), bounds_of(cx, "balloon-3"), bounds_of(cx, "balloon-4"));
        assert!(close_to(two.bottom() + px(STACK_GAP), three.top()), "{two:?} {three:?}");
        assert!(close_to(three.bottom() + px(STACK_GAP), four.top()), "{three:?} {four:?}");
        let viewport = cx.update(|window, _| window.viewport_size());
        let status_bar = cx.update(|_, cx| theme::length(cx, "h-statusbar"));
        for balloon in [two, three, four] {
            assert!(close_to(balloon.size.width, px(BALLOON_WIDTH)), "{balloon:?}");
            assert!(close_to(balloon.right(), viewport.width - px(STACK_RIGHT)), "{balloon:?}");
        }
        assert!(close_to(four.bottom(), viewport.height - status_bar - px(STACK_ABOVE_STATUS_BAR)), "{four:?}");
        // The close button sits in the balloon's top right corner.
        let close = bounds_of(cx, "balloon-4-close");
        assert!(close.right() < four.right() && close.right() > four.right() - px(12.0), "{close:?}");
        assert!(close.top() > four.top() && close.top() < four.top() + px(12.0), "{close:?}");
        // When a balloon goes, the one that waited comes back on top.
        bench.store.dismiss_balloon(4);
        bench.sync(&stack, cx);
        assert_eq!(lines(&stack, cx), ["error: One", "error: Two", "error: Three"]);
    }

    #[gpui_kit::test]
    async fn a_long_body_scrolls_inside_its_balloon(cx: &mut TestAppContext) {
        let (stack, _, cx) = open(cx);
        let mut bench = Bench::default();
        let body = "error[E0425]: cannot find value `total` in this scope\n".repeat(400);
        bench.notify(NotificationRequest { body: Some(body), ..request(Error, "Build failed") }, &stack, cx);
        let viewport = cx.update(|window, _| window.viewport_size());
        let (balloon, body) = (bounds_of(cx, "balloon-1"), bounds_of(cx, "balloon-1-body"));
        assert!(body.size.height <= viewport.height * BODY_MAX_HEIGHT + px(1.0), "{body:?}");
        assert!(balloon.size.height <= viewport.height - px(WINDOW_MARGIN) + px(1.0), "{balloon:?}");
        assert!(balloon.top() >= px(0.0), "the balloon starts inside the window: {balloon:?}");
        let close = bounds_of(cx, "balloon-1-close");
        assert!(close.top() >= px(0.0) && close.bottom() <= viewport.height, "{close:?}");
    }

    #[gpui_kit::test]
    async fn the_pointer_over_a_balloon_pauses_its_fade_out(cx: &mut TestAppContext) {
        let (stack, events, cx) = open(cx);
        let mut bench = Bench::default();
        let id = bench.notify(request(Info, "Indexed"), &stack, cx);
        bench.advance(BALLOON_TIMEOUT - 1000.0, &stack, cx);
        assert!(events.borrow().is_empty(), "the pointer is elsewhere");
        let over = bounds_of(cx, "balloon-1").center();
        cx.simulate_mouse_move(over, None, Modifiers::default());
        assert_eq!(*events.borrow(), [BalloonEvent::HoverChanged(Some(id))]);
        assert_eq!(stack.read_with(cx, |stack, _| stack.hovered()), Some(id));
        bench.apply_all(&events, &stack, cx);
        bench.advance(60_000.0, &stack, cx);
        assert_eq!(lines(&stack, cx), ["info: Indexed [Don't Show Again]"]);
        // The pointer leaves: the balloon has the second it had left.
        cx.simulate_mouse_move(point(px(1.0), px(1.0)), None, Modifiers::default());
        assert_eq!(*events.borrow(), [BalloonEvent::HoverChanged(None)]);
        bench.apply_all(&events, &stack, cx);
        bench.advance(900.0, &stack, cx);
        assert_eq!(lines(&stack, cx).len(), 1);
        bench.advance(200.0, &stack, cx);
        assert!(lines(&stack, cx).is_empty());
    }

    #[gpui_kit::test]
    async fn a_balloon_that_goes_away_under_the_pointer_is_no_longer_hovered(cx: &mut TestAppContext) {
        let (stack, events, cx) = open(cx);
        let mut bench = Bench::default();
        let id = bench.notify(request(Error, "Build failed"), &stack, cx);
        let over = bounds_of(cx, "balloon-1-close").center();
        cx.simulate_mouse_move(over, None, Modifiers::default());
        cx.simulate_click(over, Modifiers::default());
        assert_eq!(*events.borrow(), [BalloonEvent::HoverChanged(Some(id)), BalloonEvent::Dismissed(id)]);
        bench.apply_all(&events, &stack, cx);
        assert_eq!(*events.borrow(), [BalloonEvent::HoverChanged(None)]);
        assert_eq!(stack.read_with(cx, |stack, _| stack.hovered()), None);
        bench.apply_all(&events, &stack, cx);
        assert_eq!(bench.view.hovered, None);
    }

    #[test]
    fn each_level_has_its_icon_and_colour() {
        assert_eq!([Info, Warning, Error].map(level_icon), ["info", "warning", "error"]);
        assert_eq!([Info, Warning, Error].map(level_color), ["info", "warning", "danger"]);
    }

    #[gpui_kit::test]
    async fn the_shadow_is_the_themes(cx: &mut TestAppContext) {
        let shadow = parse_shadow("0 8px 24px rgba(0, 0, 0, 0.45)").expect("a shadow");
        assert_eq!((shadow.offset, shadow.blur_radius), (point(px(0.0), px(8.0)), px(24.0)));
        assert!((shadow.color.a - 0.45).abs() < 0.001);
        assert_eq!(parse_shadow("none"), None);
        assert_eq!(parse_shadow("0 8px"), None);
        cx.update(|cx| {
            assert!(balloon_shadow(cx).is_empty(), "no theme, no shadow");
            cx.set_global(theme::ActiveTheme::default());
            assert_eq!(balloon_shadow(cx).len(), 1);
        });
    }
}
