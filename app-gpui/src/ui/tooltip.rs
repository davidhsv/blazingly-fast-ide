//! Tooltips: the bubble GPUI shows when the pointer rests on an element, with the element's
//! text, the shortcut of its action dimmed after it, and an optional line of detail below.
//!
//! GPUI shows a stateful element's tooltip after the pointer rests on it and hides it when the
//! pointer leaves, a button is pressed or the wheel turns; a bubble shown through [`Tooltip::shown`]
//! also hides on a key press and when its window stops being the active one, until the pointer
//! comes back to the element. A view adds one in a line: `.with_tooltip("Close Tab")` on an
//! element with an id, or `.tooltip(tooltip("…"))` with GPUI's own method.

use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use gpui_kit::{
    AnyView, App, BoxShadow, Context, IntoElement, Render, Rgba, SharedString, StatefulInteractiveElement,
    Subscription, Window, div, prelude::*, px,
};
use std::time::Duration;

/// How long the pointer rests on an element before its tooltip shows (GPUI's own default is
/// 500ms; [`WithTooltip`] uses this one).
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// The widest a tooltip grows before its text wraps.
const MAX_WIDTH: f32 = 720.0;

/// The bubble: a title, the shortcut that runs the same action, a line of detail.
pub struct Tooltip {
    title: SharedString,
    shortcut: Option<SharedString>,
    detail: Option<SharedString>,
    /// A key was pressed or the window lost the focus: the bubble draws nothing until GPUI drops it
    /// when the pointer leaves, and a new one is built when the pointer rests on the element again.
    dismissed: bool,
    _dismissal: Vec<Subscription>,
}

impl Tooltip {
    /// A tooltip showing `title` as written.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self { title: title.into(), shortcut: None, detail: None, dismissed: false, _dismissal: Vec::new() }
    }

    /// The bubble as GPUI's `.tooltip(…)` builders return it, hidden by a key pressed in `window`
    /// and by `window` stopping being the active one.
    pub fn shown(self, window: &mut Window, cx: &mut App) -> AnyView {
        let here = window.window_handle().window_id();
        cx.new(|cx| {
            let weak: gpui_kit::WeakEntity<Self> = cx.weak_entity();
            let keys = cx.intercept_keystrokes(move |_, window, cx| {
                if window.window_handle().window_id() == here {
                    // A bubble already gone has nothing to hide.
                    let _ = weak.update(cx, |tooltip, cx| tooltip.dismiss(cx));
                }
            });
            let focus = cx.observe_window_activation(window, |tooltip, window, cx| {
                if !window.is_window_active() {
                    tooltip.dismiss(cx);
                }
            });
            Self { _dismissal: vec![keys, focus], ..self }
        })
        .into()
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.dismissed {
            self.dismissed = true;
            cx.notify();
        }
    }

    /// The shortcut label shown dimmed after the title.
    pub fn with_shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into()).filter(|shortcut: &SharedString| !shortcut.is_empty());
        self
    }

    /// The shortcut the keymap binds to `action_id`, if it binds one.
    pub fn with_action_shortcut(mut self, action_id: &str, cx: &App) -> Self {
        self.shortcut = shortcut_label(action_id, cx).map(SharedString::from);
        self
    }

    /// A second line under the title, dimmed.
    pub fn with_detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into()).filter(|detail: &SharedString| !detail.is_empty());
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn shortcut(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The text as it reads, one entry per line: the title with the shortcut after it (two
    /// spaces apart), then the detail.
    pub fn lines(&self) -> Vec<String> {
        let first = match &self.shortcut {
            Some(shortcut) => format!("{}  {shortcut}", self.title),
            None => self.title.to_string(),
        };
        std::iter::once(first).chain(self.detail.as_ref().map(ToString::to_string)).collect()
    }
}

/// The keymap's label for the shortcut of `action_id` (`⌘S`, `Ctrl+S`).
fn shortcut_label(action_id: &str, cx: &App) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(action_id, is_mac_host()))
}

/// `text` in the active language; text the catalog does not know (a path, a name) as it is.
fn translate(text: &str, cx: &App) -> SharedString {
    match cx.try_global::<Localization>() {
        Some(localization) => localization.0.tx(text).into(),
        None => SharedString::from(text.to_owned()),
    }
}

/// The first family of font token `name` that GPUI can name; the system font for the CSS
/// aliases of it.
fn font_family(name: &str, cx: &App) -> SharedString {
    let first = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families(name).into_iter().next())
        .filter(|family| !matches!(family.as_str(), "-apple-system" | "BlinkMacSystemFont" | "system-ui"));
    SharedString::from(first.unwrap_or_else(|| ".SystemUIFont".to_owned()))
}

/// The tooltip's drop shadow from `--shadow-tooltip` (`<x> <y> <blur> <colour>`); none when the
/// theme does not define one.
fn shadow(cx: &App) -> Vec<BoxShadow> {
    let value = cx.try_global::<theme::ActiveTheme>().and_then(|active| active.theme.resolved("shadow-tooltip"));
    let Some(value) = value else { return Vec::new() };
    let mut parts = value.split_whitespace();
    let mut lengths = [0.0_f32; 3];
    for length in &mut lengths {
        match parts.next().and_then(|part| part.trim_end_matches("px").parse().ok()) {
            Some(parsed) => *length = parsed,
            None => return Vec::new(),
        }
    }
    let Some(color) = theme::tokens::parse_color(&parts.collect::<Vec<_>>().join(" ")) else { return Vec::new() };
    let color = Rgba { r: color.r, g: color.g, b: color.b, a: color.a };
    vec![BoxShadow::new(px(lengths[0]), px(lengths[1]), color.into()).blur_radius(px(lengths[2]))]
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dismissed {
            return div().into_any_element();
        }
        let fg = theme::color(cx, "fg-tooltip");
        let dimmed = fg.opacity(0.62);
        let max_width = (window.viewport_size().width - px(32.0)).clamp(px(0.0), px(MAX_WIDTH));
        let title = div().min_w_0().child(self.title.clone());
        let keys_font = font_family("font-keys", cx);
        let shortcut = self.shortcut.clone().map(|shortcut| {
            div().flex_none().whitespace_nowrap().font_family(keys_font).text_color(dimmed).child(shortcut)
        });
        let detail = self.detail.clone().map(|detail| div().text_color(dimmed).child(detail));
        // Tests read what the bubble says by its first line.
        let first = self.lines().into_iter().next().unwrap_or_default();
        // GPUI places the tooltip at the pointer: the margin keeps the bubble clear of it.
        div()
            .pl(px(8.0))
            .pt(px(10.0))
            .child(
                div()
                    .debug_selector(|| "tooltip".to_owned())
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .max_w(max_width)
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(theme::length(cx, "radius-tooltip"))
                    .border_1()
                    .border_color(theme::color(cx, "border-tooltip"))
                    .bg(theme::color(cx, "bg-tooltip"))
                    .shadow(shadow(cx))
                    .font_family(font_family("font-ui", cx))
                    .text_size(theme::length(cx, "fs-ui"))
                    .line_height(px(16.0))
                    .text_color(fg)
                    .child(
                        div()
                            .debug_selector(move || format!("tooltip-text:{first}"))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(title)
                            .children(shortcut),
                    )
                    .children(detail),
            )
            .into_any_element()
    }
}

/// What `.tooltip(…)` takes: a tooltip showing `text`, translated.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |window: &mut Window, cx: &mut App| {
        let title = translate(&text, cx);
        Tooltip::new(title).shown(window, cx)
    }
}

/// What `.tooltip(…)` takes: a tooltip showing `text`, translated, with the shortcut the keymap
/// binds to `action_id` at the time it shows.
pub fn tooltip_with_shortcut(
    text: impl Into<SharedString>,
    action_id: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let (text, action_id) = (text.into(), action_id.into());
    move |window: &mut Window, cx: &mut App| {
        Tooltip::new(translate(&text, cx)).with_action_shortcut(&action_id, cx).shown(window, cx)
    }
}

/// What `.tooltip(…)` takes: a tooltip showing `text`, translated, with `keys` as its shortcut: for a
/// key the view handles itself, which no action binds.
pub fn tooltip_with_keys(
    text: impl Into<SharedString>,
    keys: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let (text, keys) = (text.into(), keys.into());
    move |window: &mut Window, cx: &mut App| {
        Tooltip::new(translate(&text, cx)).with_shortcut(keys.clone()).shown(window, cx)
    }
}

/// What `.tooltip(…)` takes: a tooltip of two lines, `title` and a dimmed `detail`, translated.
pub fn tooltip_lines(
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let (title, detail) = (title.into(), detail.into());
    move |window: &mut Window, cx: &mut App| {
        Tooltip::new(translate(&title, cx)).with_detail(translate(&detail, cx)).shown(window, cx)
    }
}

/// Tooltips on any element with an id, shown after [`TOOLTIP_DELAY`]. Empty text adds none.
pub trait WithTooltip: StatefulInteractiveElement + Sized {
    fn with_tooltip(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        if text.is_empty() {
            return self;
        }
        self.tooltip(tooltip(text)).tooltip_show_delay(TOOLTIP_DELAY)
    }

    fn with_tooltip_shortcut(self, text: impl Into<SharedString>, action_id: impl Into<SharedString>) -> Self {
        let text = text.into();
        if text.is_empty() {
            return self;
        }
        self.tooltip(tooltip_with_shortcut(text, action_id)).tooltip_show_delay(TOOLTIP_DELAY)
    }

    fn with_tooltip_keys(self, text: impl Into<SharedString>, keys: impl Into<SharedString>) -> Self {
        let text = text.into();
        if text.is_empty() {
            return self;
        }
        self.tooltip(tooltip_with_keys(text, keys)).tooltip_show_delay(TOOLTIP_DELAY)
    }

    fn with_tooltip_lines(self, title: impl Into<SharedString>, detail: impl Into<SharedString>) -> Self {
        let title = title.into();
        if title.is_empty() {
            return self;
        }
        self.tooltip(tooltip_lines(title, detail)).tooltip_show_delay(TOOLTIP_DELAY)
    }
}

impl<E: StatefulInteractiveElement> WithTooltip for E {}

/// Rests the pointer on the element drawn as `selector` until its tooltip is due: whether the
/// tooltip then shown reads `text` on its first line, followed by the shortcut the keymap binds to
/// `action` when it binds one. The pointer moves off first, so a tooltip shown before goes.
#[cfg(test)]
pub fn hovered_tooltip(
    cx: &mut gpui_kit::VisualTestContext,
    selector: &'static str,
    text: &str,
    action: Option<&str>,
) -> bool {
    use gpui_kit::{Modifiers, point};
    let shortcut = action.and_then(|action| cx.update(|_, cx| shortcut_label(action, cx)));
    let expected = match shortcut {
        Some(shortcut) => format!("{text}  {shortcut}"),
        None => text.to_owned(),
    };
    cx.simulate_mouse_move(point(px(1.0), px(1.0)), None, Modifiers::default());
    cx.run_until_parked();
    let over = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    cx.simulate_mouse_move(over, None, Modifiers::default());
    cx.executor().advance_clock(TOOLTIP_DELAY + Duration::from_millis(10));
    cx.run_until_parked();
    let line: &'static str = Box::leak(format!("tooltip-text:{expected}").into_boxed_str());
    cx.debug_bounds(line).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use crate::platform::keymap::{self, KeymapState};
    use gpui_kit::{
        Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, point,
    };

    mod proofs4_tooltip;

    const ACTION: &str = "navigate.searchEverywhere";

    /// A button with a tooltip, in a window with room around it.
    struct Host {
        text: &'static str,
        shortcut: bool,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let button = div().id("button").debug_selector(|| "button".to_owned()).w(px(80.0)).h(px(24.0));
            let button = if self.shortcut {
                button.with_tooltip_shortcut(self.text, ACTION)
            } else {
                button.with_tooltip(self.text)
            };
            div().size_full().p(px(40.0)).child(button)
        }
    }

    fn setup(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            let mut state = KeymapState::new("intellij-macos");
            state.set_overrides(keymap::Keymap::from([(ACTION.to_owned(), vec!["Meta+P".to_owned()])]));
            cx.set_global(Keymap(state));
        });
    }

    fn host<'a>(cx: &'a mut TestAppContext, text: &'static str, shortcut: bool) -> &'a mut VisualTestContext {
        setup(cx);
        let (_, cx) = cx.add_window_view(|_, _| Host { text, shortcut });
        cx
    }

    fn button(cx: &mut VisualTestContext) -> Point<Pixels> {
        cx.debug_bounds("button").expect("the button is drawn").center()
    }

    fn shown(cx: &mut VisualTestContext) -> bool {
        cx.debug_bounds("tooltip").is_some()
    }

    /// Rests the pointer on the button until its tooltip is due.
    fn hover(cx: &mut VisualTestContext) {
        let over = button(cx);
        cx.simulate_mouse_move(over, None, Modifiers::default());
        cx.executor().advance_clock(TOOLTIP_DELAY + Duration::from_millis(10));
        cx.run_until_parked();
    }

    fn expected_shortcut(cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| shortcut_label(ACTION, cx))
    }

    #[gpui_kit::test]
    async fn the_view_shows_its_text_shortcut_and_detail_lines(cx: &mut TestAppContext) {
        setup(cx);
        let plain = Tooltip::new("Close Tab");
        assert_eq!((plain.title(), plain.shortcut(), plain.detail()), ("Close Tab", None, None));
        assert_eq!(plain.lines(), ["Close Tab"]);
        let both = Tooltip::new("Save All").with_shortcut("Ctrl+S").with_detail("3 files");
        assert_eq!(both.lines(), ["Save All  Ctrl+S", "3 files"]);
        assert_eq!(Tooltip::new("Save All").with_shortcut("").shortcut(), None);

        let (_, cx) = cx.add_window_view(|_, _| Host { text: "Search", shortcut: false });
        let label = expected_shortcut(cx);
        assert!(label.is_some());
        let (with_shortcut, with_detail) = cx.update(|window, cx| {
            let lines = |view: AnyView, cx: &mut App| {
                view.downcast::<Tooltip>().map(|tooltip| tooltip.read(cx).lines()).unwrap_or_default()
            };
            let with_shortcut = tooltip_with_shortcut("Search Everywhere", ACTION)(window, cx);
            let with_detail = tooltip_lines("main.rs", "src/main.rs")(window, cx);
            (lines(with_shortcut, cx), lines(with_detail, cx))
        });
        assert_eq!(with_shortcut, [format!("Search Everywhere  {}", label.unwrap_or_default())]);
        assert_eq!(with_detail, ["main.rs", "src/main.rs"]);
        // An action without a binding shows no shortcut.
        let unbound = cx.update(|_, cx| Tooltip::new("Nothing").with_action_shortcut("t.unbound", cx).lines());
        assert_eq!(unbound, ["Nothing"]);
    }

    #[gpui_kit::test]
    async fn shows_tooltip_text_and_shortcut_after_delay(cx: &mut TestAppContext) {
        let cx = host(cx, "d:/project/src/main.rs", true);
        let over = button(cx);
        cx.simulate_mouse_move(over, None, Modifiers::default());
        assert!(!shown(cx));
        cx.executor().advance_clock(TOOLTIP_DELAY - Duration::from_millis(50));
        cx.run_until_parked();
        assert!(!shown(cx), "the tooltip waits for the pointer to rest");
        cx.executor().advance_clock(Duration::from_millis(60));
        cx.run_until_parked();
        assert!(shown(cx));
        let line = format!("d:/project/src/main.rs  {}", expected_shortcut(cx).unwrap_or_default());
        let selector: &'static str = Box::leak(format!("tooltip-text:{line}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "the bubble reads {line}");
        // The bubble opens below and right of the pointer.
        let bubble = cx.debug_bounds("tooltip").expect("the tooltip is drawn");
        assert!(bubble.top() > over.y && bubble.left() > over.x);
        // Pointer out hides the tooltip.
        cx.simulate_mouse_move(point(px(400.0), px(400.0)), None, Modifiers::default());
        assert!(!shown(cx));
    }

    #[gpui_kit::test]
    async fn keeps_dismissal_behavior_for_pointer_down_and_wheel(cx: &mut TestAppContext) {
        let cx = host(cx, "Project", true);
        hover(cx);
        assert!(shown(cx));
        let over = button(cx);
        cx.simulate_mouse_down(over, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(over, MouseButton::Left, Modifiers::default());
        assert!(!shown(cx), "pointer down hides the tooltip");

        cx.simulate_mouse_move(point(px(400.0), px(400.0)), None, Modifiers::default());
        hover(cx);
        assert!(shown(cx));
        cx.simulate_event(ScrollEvent {
            position: over,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-20.0))),
            ..Default::default()
        });
        assert!(!shown(cx), "the wheel hides the tooltip");
    }

    #[gpui_kit::test]
    async fn returns_no_tooltip_when_text_is_missing(cx: &mut TestAppContext) {
        let cx = host(cx, "", false);
        hover(cx);
        assert!(!shown(cx));
    }
}
