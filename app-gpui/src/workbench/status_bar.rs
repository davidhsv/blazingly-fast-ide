//! The status bar as a host of contributed items.
//!
//! Each item is registered with a [`StatusItemSpec`] (id, side, priority, the title it is listed
//! under in the bar's context menu, whether it shows before the user decides, whether a LightEdit
//! window shows it, an optional `when` clause) and a [`StatusContent`]: a view of its own, or a
//! [`StatusText`] the bar draws (text, icon, tooltip, what a click does), fixed or computed each
//! time the bar is drawn.
//!
//! [`StatusBarModel`] is the pure part: the order (a higher priority sits closer to the window
//! edge on both sides), the LightEdit filter, the visibility the user chose (stored through
//! [`status_bar_widgets`] under `appearance.statusBarWidgets`) and the context menu rows.
//! [`StatusBar`] draws the shown items, left group, spacer, right group, and reports clicks as
//! [`StatusBarEvent`]s; the window runs the actions, shows the menus and writes the settings.

use super::status_bar_widgets::{
    self, SetVisible, StatusItem, StatusMenuItem, WidgetOverrides, is_status_item_visible, set_status_widget_visible,
    status_bar_menu, toggle_status_widget,
};
use crate::platform::collate;
use crate::platform::context::{ContextKeys, evaluate_when, window_context};
use crate::platform::settings::SettingsWrite;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, EventEmitter, IntoElement, MouseButton, MouseDownEvent, Pixels,
    Point, Render, SharedString, Window, div, prelude::*, px, relative,
};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

/// The background tasks item: it gives way on narrow windows like the left group does.
pub const TASKS_ITEM: &str = "workbench.tasks";

/// The side of the status bar an item sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusAlign {
    Left,
    Right,
}

/// A status bar item as registered.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItemSpec {
    pub id: String,
    pub align: StatusAlign,
    /// Higher = further from the centre (closer to the window edge).
    pub priority: f64,
    /// Name in the status bar context menu, where the user can hide or show the item. Untitled
    /// items are always shown.
    pub title: Option<String>,
    /// Shown until the user decides otherwise (default true).
    pub default_visible: Option<bool>,
    /// Also shown in a LightEdit window, whose status bar has only such items.
    pub light_edit: bool,
    /// Shown only while this clause holds against the window's context keys.
    pub when: Option<String>,
}

impl StatusItemSpec {
    pub fn new(id: &str, align: StatusAlign, priority: f64) -> Self {
        Self { id: id.to_owned(), align, priority, title: None, default_visible: None, light_edit: false, when: None }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }

    /// Hidden until the user shows it from the context menu.
    pub fn hidden_by_default(mut self) -> Self {
        self.default_visible = Some(false);
        self
    }

    pub fn light_edit(mut self) -> Self {
        self.light_edit = true;
        self
    }

    pub fn when(mut self, when: &str) -> Self {
        self.when = Some(when.to_owned());
        self
    }

    /// The item as far as its visibility is concerned.
    fn widget(&self) -> StatusItem {
        StatusItem { id: self.id.clone(), title: self.title.clone(), default_visible: self.default_visible }
    }

    /// Whether the item gives way first on narrow windows: the left group and the tasks.
    pub fn shrinks(&self) -> bool {
        self.align == StatusAlign::Left || self.id == TASKS_ITEM
    }

    /// Whether the `when` clause holds; a clause that does not parse hides the item.
    fn when_holds(&self, ctx: &ContextKeys) -> bool {
        evaluate_when(self.when.as_deref(), ctx).unwrap_or_else(|error| {
            tracing::warn!("status item {} has an invalid when clause: {}", self.id, error.0);
            false
        })
    }
}

/// The items of each side in the order they are drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusOrder<'a> {
    pub left: Vec<&'a StatusItemSpec>,
    pub right: Vec<&'a StatusItemSpec>,
}

impl<'a> StatusOrder<'a> {
    /// The ids of each side.
    pub fn ids(&self) -> (Vec<String>, Vec<String>) {
        let ids = |specs: &[&StatusItemSpec]| specs.iter().map(|spec| spec.id.clone()).collect::<Vec<_>>();
        (ids(&self.left), ids(&self.right))
    }

    /// Left then right: the bar's order from its left edge.
    pub fn all(&self) -> impl Iterator<Item = &'a StatusItemSpec> {
        self.left.iter().chain(&self.right).copied()
    }
}

/// By priority, then by id. Priorities that do not order (NaN) count as equal.
fn by_priority(a: &StatusItemSpec, b: &StatusItemSpec) -> Ordering {
    match a.priority.partial_cmp(&b.priority) {
        Some(Ordering::Equal) | None => collate::compare(&a.id, &b.id),
        Some(order) => order,
    }
}

/// Orders status items: on both sides a higher priority sits closer to the window edge, so left
/// items sort descending and right items ascending; equal priorities by id.
pub fn order_status_items<'a>(items: impl IntoIterator<Item = &'a StatusItemSpec>) -> StatusOrder<'a> {
    let (mut left, mut right): (Vec<&StatusItemSpec>, Vec<&StatusItemSpec>) =
        items.into_iter().partition(|item| item.align == StatusAlign::Left);
    left.sort_by(|a, b| match a.priority.partial_cmp(&b.priority) {
        Some(Ordering::Equal) | None => collate::compare(&a.id, &b.id),
        Some(order) => order.reverse(),
    });
    right.sort_by(|a, b| by_priority(a, b));
    StatusOrder { left, right }
}

/// The registered items, the user's visibility choices and the kind of window: what the bar
/// shows, in which order, and what its context menu lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusBarModel {
    specs: Vec<StatusItemSpec>,
    overrides: Option<WidgetOverrides>,
    light_edit: bool,
}

impl StatusBarModel {
    /// Adds an item, or replaces the one registered under the same id. Returns whether anything
    /// changed.
    pub fn register(&mut self, spec: StatusItemSpec) -> bool {
        match self.specs.iter_mut().find(|known| known.id == spec.id) {
            Some(known) if *known == spec => false,
            Some(known) => {
                *known = spec;
                true
            }
            None => {
                self.specs.push(spec);
                true
            }
        }
    }

    pub fn unregister(&mut self, id: &str) -> bool {
        let before = self.specs.len();
        self.specs.retain(|spec| spec.id != id);
        self.specs.len() != before
    }

    /// Every registered item, in registration order.
    pub fn specs(&self) -> &[StatusItemSpec] {
        &self.specs
    }

    pub fn spec(&self, id: &str) -> Option<&StatusItemSpec> {
        self.specs.iter().find(|spec| spec.id == id)
    }

    /// The user's choices (`appearance.statusBarWidgets`).
    pub fn set_overrides(&mut self, overrides: Option<WidgetOverrides>) -> bool {
        if self.overrides == overrides {
            return false;
        }
        self.overrides = overrides;
        true
    }

    pub fn overrides(&self) -> Option<&WidgetOverrides> {
        self.overrides.as_ref()
    }

    /// A LightEdit window's bar has only the items registered for it.
    pub fn set_light_edit(&mut self, light_edit: bool) -> bool {
        if self.light_edit == light_edit {
            return false;
        }
        self.light_edit = light_edit;
        true
    }

    pub fn is_light_edit(&self) -> bool {
        self.light_edit
    }

    /// The items this window's bar has, in bar order, shown or not.
    pub fn order(&self) -> StatusOrder<'_> {
        order_status_items(self.specs.iter().filter(|spec| !self.light_edit || spec.light_edit))
    }

    /// Whether `spec` is shown: the user did not hide it and its `when` clause holds.
    pub fn is_shown(&self, spec: &StatusItemSpec, ctx: &ContextKeys) -> bool {
        is_status_item_visible(&spec.widget(), self.overrides.as_ref()) && spec.when_holds(ctx)
    }

    /// The items drawn against `ctx`, in bar order.
    pub fn shown(&self, ctx: &ContextKeys) -> StatusOrder<'_> {
        let StatusOrder { left, right } = self.order();
        StatusOrder {
            left: left.into_iter().filter(|spec| self.is_shown(spec, ctx)).collect(),
            right: right.into_iter().filter(|spec| self.is_shown(spec, ctx)).collect(),
        }
    }

    /// The context menu: every titled item of this bar with a check mark, in bar order.
    pub fn menu(&self) -> Vec<StatusMenuItem> {
        let widgets: Vec<StatusItem> = self.order().all().map(StatusItemSpec::widget).collect();
        status_bar_menu(&widgets, self.overrides.as_ref())
    }

    /// The write that shows or hides `id`.
    pub fn set_visible(&self, id: &str, visible: bool) -> SettingsWrite {
        set_status_widget_visible(self.overrides.as_ref(), id, visible)
    }

    /// The write that flips `id` (View › Appearance › Status Bar Widgets); `None` for untitled or
    /// unknown items, which stay as they are.
    pub fn toggle(&self, id: &str) -> Option<SettingsWrite> {
        let widgets: Vec<StatusItem> = self.specs.iter().map(StatusItemSpec::widget).collect();
        toggle_status_widget(&widgets, self.overrides.as_ref(), id)
    }
}

/// A transient determinate or indeterminate progress bar drawn after an item's text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusProgress {
    /// 0..1; `None` while indeterminate.
    pub fraction: Option<f64>,
}

/// What a click on a drawn item asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum StatusClick {
    /// Run this action.
    Action { id: String, arg: Option<Value> },
    /// The window opens what belongs to the item (its menu or popup) at the click.
    Open,
}

impl StatusClick {
    pub fn action(id: &str) -> Self {
        Self::Action { id: id.to_owned(), arg: None }
    }
}

/// An item the bar draws: a flat button with an optional icon, text, progress and suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusText {
    /// Shown as written: whoever builds the item translates it.
    pub text: String,
    pub icon: Option<&'static str>,
    /// Translated when shown.
    pub tooltip: Option<String>,
    /// The action whose shortcut the tooltip shows.
    pub shortcut_action: Option<&'static str>,
    pub click: Option<StatusClick>,
    /// A disabled item takes no clicks and does not highlight under the pointer.
    pub enabled: bool,
    /// Accent text (LightEdit's Open as Project).
    pub emphasis: bool,
    /// Code font (the Vim mode).
    pub mono: bool,
    /// Faint text (a feature that is off).
    pub dimmed: bool,
    pub progress: Option<StatusProgress>,
    /// Faint text after the rest (`+2` more tasks).
    pub suffix: Option<String>,
}

impl StatusText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            tooltip: None,
            shortcut_action: None,
            click: None,
            enabled: true,
            emphasis: false,
            mono: false,
            dimmed: false,
            progress: None,
            suffix: None,
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn shortcut_of(mut self, action: &'static str) -> Self {
        self.shortcut_action = Some(action);
        self
    }

    pub fn on_click(mut self, click: StatusClick) -> Self {
        self.click = Some(click);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn emphasis(mut self) -> Self {
        self.emphasis = true;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub fn progress(mut self, fraction: Option<f64>) -> Self {
        self.progress = Some(StatusProgress { fraction });
        self
    }

    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// The click the item takes, when it takes one.
    fn active_click(&self) -> Option<&StatusClick> {
        self.click.as_ref().filter(|_| self.enabled)
    }
}

/// Computes an item's text each time the bar is drawn; `None` while it has nothing to say.
pub type StatusProvider = Rc<dyn Fn(&Window, &App) -> Option<StatusText>>;

/// What an item draws.
#[derive(Clone)]
pub enum StatusContent {
    /// A view of its own (breadcrumbs, the language services popup…).
    View(AnyView),
    /// Fixed text the bar draws.
    Text(StatusText),
    /// Text the bar computes when it draws.
    Dynamic(StatusProvider),
}

impl StatusContent {
    pub fn dynamic(provider: impl Fn(&Window, &App) -> Option<StatusText> + 'static) -> Self {
        Self::Dynamic(Rc::new(provider))
    }
}

/// What the status bar asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum StatusBarEvent {
    /// Item `item` was clicked and runs this action.
    RunAction { item: String, id: String, arg: Option<Value> },
    /// Item `item` was clicked: open its menu or popup at `position`.
    OpenItem { item: String, position: Point<Pixels> },
    /// The bar was right-clicked: show [`StatusBar::context_menu`] at `position`.
    ContextMenu { position: Point<Pixels> },
}

/// The shown items of one side with what each draws, in bar order.
type DrawnSide = Vec<(StatusItemSpec, Drawn)>;

/// A shown item as drawn: a view, or the text it showed.
enum Drawn {
    View(AnyView),
    Text(StatusText),
}

/// The status bar of a window: its registered items drawn in order.
pub struct StatusBar {
    model: StatusBarModel,
    contents: HashMap<String, StatusContent>,
}

impl EventEmitter<StatusBarEvent> for StatusBar {}

impl StatusBar {
    pub fn new() -> Self {
        Self { model: StatusBarModel::default(), contents: HashMap::new() }
    }

    pub fn model(&self) -> &StatusBarModel {
        &self.model
    }

    /// Adds an item with what it draws, or replaces the one registered under the same id.
    pub fn register(&mut self, spec: StatusItemSpec, content: StatusContent, cx: &mut Context<Self>) {
        self.contents.insert(spec.id.clone(), content);
        self.model.register(spec);
        cx.notify();
    }

    /// Adds an item whose text its window feeds with [`Self::set_text`]; it draws nothing until then.
    pub fn register_fed(&mut self, spec: StatusItemSpec, cx: &mut Context<Self>) {
        if self.model.register(spec) {
            cx.notify();
        }
    }

    pub fn unregister(&mut self, id: &str, cx: &mut Context<Self>) {
        let removed = self.contents.remove(id).is_some();
        if self.model.unregister(id) || removed {
            cx.notify();
        }
    }

    /// Replaces the fixed text of item `id`; `None` draws nothing for it. Redraws only on a change.
    pub fn set_text(&mut self, id: &str, text: Option<StatusText>, cx: &mut Context<Self>) {
        let current = match self.contents.get(id) {
            Some(StatusContent::Text(text)) => Some(text),
            _ => None,
        };
        if current == text.as_ref() && (text.is_some() || !self.contents.contains_key(id)) {
            return;
        }
        match text {
            Some(text) => self.contents.insert(id.to_owned(), StatusContent::Text(text)),
            None => self.contents.remove(id),
        };
        cx.notify();
    }

    /// The user's choices, from `appearance.statusBarWidgets`.
    pub fn set_overrides(&mut self, overrides: Option<WidgetOverrides>, cx: &mut Context<Self>) {
        if self.model.set_overrides(overrides) {
            cx.notify();
        }
    }

    pub fn set_light_edit(&mut self, light_edit: bool, cx: &mut Context<Self>) {
        if self.model.set_light_edit(light_edit) {
            cx.notify();
        }
    }

    /// The content of each shown item that has something to draw, in bar order.
    fn drawn(&self, window: &Window, cx: &App) -> (DrawnSide, DrawnSide) {
        let ctx = window_context(window, cx);
        let order = self.model.shown(&ctx);
        let draw = |specs: &[&StatusItemSpec]| {
            specs
                .iter()
                .filter_map(|spec| {
                    let drawn = match self.contents.get(&spec.id)? {
                        StatusContent::View(view) => Drawn::View(view.clone()),
                        StatusContent::Text(text) => Drawn::Text(text.clone()),
                        StatusContent::Dynamic(provider) => Drawn::Text(provider(window, cx)?),
                    };
                    Some(((*spec).clone(), drawn))
                })
                .collect::<Vec<_>>()
        };
        (draw(&order.left), draw(&order.right))
    }

    /// The ids drawn on each side, in order: shown, and with something to draw.
    pub fn shown(&self, window: &Window, cx: &App) -> (Vec<String>, Vec<String>) {
        let (left, right) = self.drawn(window, cx);
        let ids = |items: Vec<(StatusItemSpec, Drawn)>| items.into_iter().map(|(spec, _)| spec.id).collect();
        (ids(left), ids(right))
    }

    /// The text of each drawn text item, left to right.
    pub fn texts(&self, window: &Window, cx: &App) -> Vec<String> {
        let (left, right) = self.drawn(window, cx);
        left.into_iter()
            .chain(right)
            .filter_map(|(_, drawn)| match drawn {
                Drawn::Text(text) => Some(text.text),
                Drawn::View(_) => None,
            })
            .collect()
    }

    /// Whether the bar draws anything.
    pub fn has_items(&self, window: &Window, cx: &App) -> bool {
        let (left, right) = self.drawn(window, cx);
        !left.is_empty() || !right.is_empty()
    }

    /// The context menu: every titled item with a check mark; choosing one flips it.
    pub fn context_menu(&self) -> Vec<MenuItem<SetVisible>> {
        self.model
            .menu()
            .into_iter()
            .map(|row| {
                let choice = row.run();
                let item = MenuItem::item(row.label, choice);
                if row.checked { item.icon("check") } else { item }
            })
            .collect()
    }

    /// The write that carries out a chosen context menu row.
    pub fn write_for(&self, choice: &SetVisible) -> SettingsWrite {
        self.model.set_visible(&choice.id, choice.visible)
    }

    fn render_text(&self, spec: &StatusItemSpec, text: StatusText, cx: &mut Context<Self>) -> AnyElement {
        let id = spec.id.clone();
        let selector = format!("status:{id}");
        let shrink = spec.shrinks();
        let clickable = text.active_click().cloned();
        let color = if text.emphasis {
            theme::color(cx, "accent")
        } else if text.dimmed || !text.enabled {
            theme::color(cx, "fg-faint")
        } else {
            theme::color(cx, "fg-muted")
        };
        let icon = text.icon.map(|icon| theme::icon(cx, icon, 14.0, color));
        let label = (!text.text.is_empty()).then(|| {
            div()
                .min_w_0()
                .when(shrink, |el| el.truncate())
                .when(text.mono, |el| el.font_family(font_family("font-mono", cx)))
                .child(SharedString::from(text.text.clone()))
        });
        let progress = text.progress.map(|progress| {
            let filled = progress.fraction.map_or(0.0, |fraction| fraction.clamp(0.0, 1.0) as f32);
            div()
                .w(px(96.0))
                .h(px(4.0))
                .flex_none()
                .rounded(px(2.0))
                .bg(theme::color(cx, "bg-progress-track"))
                .child(div().h_full().w(relative(filled)).rounded(px(2.0)).bg(theme::color(cx, "accent")))
        });
        let suffix = text
            .suffix
            .clone()
            .map(|suffix| div().flex_none().text_color(theme::color(cx, "fg-faint")).child(SharedString::from(suffix)));
        let element = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(20.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(4.0))
            .rounded(px(4.0))
            .text_color(color)
            .when(shrink, |el| el.flex_shrink(1.0).min_w_0().overflow_hidden())
            .when(!shrink, |el| el.flex_none())
            .when(text.progress.is_some(), |el| el.max_w(px(320.0)))
            .children(icon)
            .children(label)
            .children(progress)
            .children(suffix);
        let element = match (&text.tooltip, text.shortcut_action) {
            (Some(tooltip), Some(action)) => element.with_tooltip_shortcut(tooltip.clone(), action),
            (Some(tooltip), None) => element.with_tooltip(tooltip.clone()),
            (None, _) => element,
        };
        match clickable {
            Some(click) => element
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                    let item = id.clone();
                    cx.emit(match click.clone() {
                        StatusClick::Action { id, arg } => StatusBarEvent::RunAction { item, id, arg },
                        StatusClick::Open => StatusBarEvent::OpenItem { item, position: event.position() },
                    });
                }))
                .into_any_element(),
            None => element.into_any_element(),
        }
    }

    fn render_item(&self, spec: &StatusItemSpec, drawn: Drawn, cx: &mut Context<Self>) -> AnyElement {
        match drawn {
            Drawn::Text(text) => self.render_text(spec, text, cx),
            Drawn::View(view) => {
                let selector = format!("status:{}", spec.id);
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .items_center()
                    .px(px(4.0))
                    .when(spec.shrinks(), |el| el.flex_shrink(1.0).min_w_0().overflow_hidden())
                    .when(!spec.shrinks(), |el| el.flex_none())
                    .child(view)
                    .into_any_element()
            }
        }
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for StatusBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (left, right) = self.drawn(window, cx);
        let left: Vec<AnyElement> = left.into_iter().map(|(spec, drawn)| self.render_item(&spec, drawn, cx)).collect();
        let right: Vec<AnyElement> =
            right.into_iter().map(|(spec, drawn)| self.render_item(&spec, drawn, cx)).collect();
        div()
            .id("status-bar")
            .debug_selector(|| "status-bar".to_owned())
            .w_full()
            .h(theme::length(cx, "h-statusbar") + px(4.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(12.0))
            .pr(px(10.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(font_family("font-ui", cx))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg-muted"))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.emit(StatusBarEvent::ContextMenu { position: event.position });
                }),
            )
            .children(left)
            .child(div().flex_1().min_w(px(8.0)))
            .children(right)
    }
}

/// The first family of font token `name` the theme lists, the system font for its CSS aliases.
pub(super) fn font_family(name: &str, cx: &App) -> SharedString {
    let first = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families(name).into_iter().next())
        .filter(|family| !matches!(family.as_str(), "-apple-system" | "BlinkMacSystemFont" | "system-ui"));
    SharedString::from(first.unwrap_or_else(|| ".SystemUIFont".to_owned()))
}

/// The stored choices of the effective settings object, for [`StatusBar::set_overrides`].
pub fn stored_overrides(settings: &Value) -> Option<WidgetOverrides> {
    status_bar_widgets::status_widget_overrides(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::context::set_context;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;

    fn spec(id: &str, align: StatusAlign, priority: f64) -> StatusItemSpec {
        StatusItemSpec::new(id, align, priority)
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    fn model(specs: Vec<StatusItemSpec>) -> StatusBarModel {
        let mut model = StatusBarModel::default();
        for spec in specs {
            model.register(spec);
        }
        model
    }

    // src/workbench/Workbench.test.tsx › status bar ordering › puts higher priorities closer to the window edges
    #[test]
    fn puts_higher_priorities_closer_to_the_window_edges() {
        let specs = [
            spec("a", StatusAlign::Left, 1.0),
            spec("b", StatusAlign::Left, 5.0),
            spec("c", StatusAlign::Right, 5.0),
            spec("d", StatusAlign::Right, 1.0),
            spec("e", StatusAlign::Right, 3.0),
        ];
        let (left, right) = order_status_items(&specs).ids();
        assert_eq!(left, ids(&["b", "a"]));
        assert_eq!(right, ids(&["d", "e", "c"]));
    }

    #[test]
    fn equal_priorities_order_by_id_and_unordered_priorities_count_as_equal() {
        let specs = [
            spec("z", StatusAlign::Right, 1.0),
            spec("y", StatusAlign::Right, 1.0),
            spec("n", StatusAlign::Left, f64::NAN),
            spec("m", StatusAlign::Left, f64::NAN),
        ];
        let (left, right) = order_status_items(&specs).ids();
        assert_eq!((left, right), (ids(&["m", "n"]), ids(&["y", "z"])));
    }

    #[test]
    fn registering_an_id_again_replaces_its_item() {
        let mut model = model(vec![spec("a", StatusAlign::Left, 1.0)]);
        assert!(!model.register(spec("a", StatusAlign::Left, 1.0)));
        assert!(model.register(spec("a", StatusAlign::Right, 2.0)));
        assert_eq!(model.specs(), [spec("a", StatusAlign::Right, 2.0)]);
        assert!(model.unregister("a") && !model.unregister("a"));
        assert!(model.specs().is_empty());
    }

    #[test]
    fn a_light_edit_window_shows_only_the_items_registered_for_it() {
        let mut model = model(vec![
            spec("editor.caret", StatusAlign::Right, 20.0).title("Line:Column Number").light_edit(),
            spec("agents.status", StatusAlign::Right, 3.0).title("Agents"),
        ]);
        assert_eq!(model.order().ids().1, ids(&["agents.status", "editor.caret"]));
        model.set_light_edit(true);
        assert_eq!(model.order().ids().1, ids(&["editor.caret"]));
        let labels: Vec<String> = model.menu().into_iter().map(|row| row.label).collect();
        assert_eq!(labels, ["Line:Column Number"], "the menu lists this bar's items only");
    }

    #[test]
    fn an_item_shows_only_while_its_when_clause_holds() {
        let model = model(vec![
            spec("always", StatusAlign::Right, 0.0),
            spec("editor", StatusAlign::Right, 1.0).when("textEditorActive && !lightEdit"),
            spec("broken", StatusAlign::Right, 2.0).when("(("),
        ]);
        let mut ctx = ContextKeys::new();
        assert_eq!(model.shown(&ctx).ids().1, ids(&["always"]));
        ctx.set("textEditorActive", true);
        assert_eq!(model.shown(&ctx).ids().1, ids(&["always", "editor"]));
        ctx.set("lightEdit", true);
        assert_eq!(model.shown(&ctx).ids().1, ids(&["always"]));
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › applies defaults and overrides
    #[test]
    fn applies_defaults_and_overrides() {
        let mut model = model(vec![
            spec("perf.memory", StatusAlign::Right, 100.0).title("Memory Indicator").hidden_by_default(),
            spec("caret", StatusAlign::Right, 20.0).title("Line:Column"),
            spec("tasks", StatusAlign::Right, 0.0),
        ]);
        let ctx = ContextKeys::new();
        assert_eq!(model.shown(&ctx).ids().1, ids(&["tasks", "caret"]));
        model.set_overrides(Some(WidgetOverrides::from([
            ("perf.memory".to_owned(), true),
            ("caret".to_owned(), false),
            ("tasks".to_owned(), false),
        ])));
        // Untitled items cannot be hidden.
        assert_eq!(model.shown(&ctx).ids().1, ids(&["tasks", "perf.memory"]));
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › lists titled widgets with checkmarks
    #[test]
    fn lists_titled_widgets_with_checkmarks() {
        let mut model = model(vec![
            spec("caret", StatusAlign::Right, 0.0).title("Line:Column Number"),
            spec("tasks", StatusAlign::Right, 1.0),
            spec("mem", StatusAlign::Right, 2.0).title("Memory").hidden_by_default(),
        ]);
        model.set_overrides(Some(WidgetOverrides::from([("caret".to_owned(), true)])));
        let rows: Vec<(String, bool)> = model.menu().into_iter().map(|row| (row.label, row.checked)).collect();
        assert_eq!(rows, [("Line:Column Number".to_owned(), true), ("Memory".to_owned(), false)]);
        assert_eq!(model.menu()[1].run(), SetVisible { id: "mem".to_owned(), visible: true });
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › toggles one widget by id from View ›
    // Appearance › Status Bar Widgets
    #[test]
    fn toggles_one_widget_by_id_from_view_appearance_status_bar_widgets() {
        let mut model = model(vec![
            spec("w.caret", StatusAlign::Right, 0.0).title("Line:Column Number"),
            spec("w.memory", StatusAlign::Right, 0.0).title("Memory Indicator").hidden_by_default(),
            spec("w.tasks", StatusAlign::Right, 0.0),
        ]);
        model.set_overrides(Some(WidgetOverrides::new()));
        let write = model.toggle("w.memory").expect("a titled item");
        assert_eq!((write.key, &write.value), ("appearance.statusBarWidgets", &json!({ "w.memory": true })));
        model.set_overrides(stored_overrides(&json!({ "appearance": { "statusBarWidgets": write.value } })));
        let write = model.toggle("w.caret").expect("a titled item");
        assert_eq!(write.value, json!({ "w.memory": true, "w.caret": false }));
        // Untitled or unknown widgets cannot be hidden.
        assert_eq!(model.toggle("w.tasks"), None);
        assert_eq!(model.toggle("w.gone"), None);
    }

    /// An item view standing in for a feature's own.
    struct Stand(String);

    impl Render for Stand {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(SharedString::from(self.0.clone()))
        }
    }

    type Events = Rc<RefCell<Vec<StatusBarEvent>>>;

    /// A bar in a window of its own with `items` registered, each drawing a stand-in view, and
    /// what it has asked of the window so far.
    fn open(
        cx: &mut TestAppContext,
        items: Vec<(StatusItemSpec, Option<StatusText>)>,
    ) -> (Entity<StatusBar>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (bar, cx) = cx.add_window_view(move |_, cx| {
            let mut bar = StatusBar::new();
            for (spec, text) in items {
                let content = match text {
                    Some(text) => StatusContent::Text(text),
                    None => {
                        let name = spec.id.clone();
                        StatusContent::View(cx.new(|_| Stand(name)).into())
                    }
                };
                bar.register(spec, content, cx);
            }
            bar
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&bar, move |_, event: &StatusBarEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        (bar, events, cx)
    }

    fn shown(bar: &Entity<StatusBar>, cx: &mut VisualTestContext) -> (Vec<String>, Vec<String>) {
        cx.update(|window, cx| bar.read(cx).shown(window, cx))
    }

    fn left_edge(cx: &mut VisualTestContext, selector: &'static str) -> Pixels {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).left()
    }

    // src/workbench/Workbench.test.tsx › status bar ordering › renders registered items in order
    #[gpui_kit::test]
    async fn renders_registered_items_in_order(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(
            cx,
            vec![
                (spec("caret", StatusAlign::Right, 20.0), None),
                (spec("memory", StatusAlign::Right, 60.0), None),
                (spec("crumbs", StatusAlign::Left, 0.0), None),
                (spec("encoding", StatusAlign::Right, 40.0), None),
            ],
        );
        assert_eq!(shown(&bar, cx), (ids(&["crumbs"]), ids(&["caret", "encoding", "memory"])));
        let edges = [
            left_edge(cx, "status:crumbs"),
            left_edge(cx, "status:caret"),
            left_edge(cx, "status:encoding"),
            left_edge(cx, "status:memory"),
        ];
        assert!(edges.windows(2).all(|pair| pair[0] < pair[1]), "{edges:?}");
        // The right group sits against the right edge, the left one against the left.
        let bar_bounds = cx.debug_bounds("status-bar").expect("the bar is drawn");
        let memory = cx.debug_bounds("status:memory").expect("drawn");
        assert!(bar_bounds.right() - memory.right() < px(16.0), "{bar_bounds:?} {memory:?}");
        assert!(edges[0] - bar_bounds.left() < px(16.0));
    }

    // src/workbench/statusBarWidgets.test.tsx › status bar widgets › toggles a widget from the status bar
    // context menu and persists it
    #[gpui_kit::test]
    async fn toggles_a_widget_from_the_status_bar_context_menu_and_persists_it(cx: &mut TestAppContext) {
        let (bar, events, cx) = open(
            cx,
            vec![
                (spec("w.caret", StatusAlign::Right, 1.0).title("Line:Column Number"), None),
                (spec("w.memory", StatusAlign::Right, 2.0).title("Memory Indicator").hidden_by_default(), None),
            ],
        );
        assert!(cx.debug_bounds("status:w.memory").is_none());
        assert!(cx.debug_bounds("status:w.caret").is_some());

        let at = cx.debug_bounds("status-bar").expect("the bar is drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(StatusBarEvent::ContextMenu { .. })));

        let menu = bar.read_with(cx, |bar, _| bar.context_menu());
        let rows: Vec<(Option<&str>, bool)> = menu
            .iter()
            .map(|row| {
                let checked = matches!(row, MenuItem::Item(entry) if entry.icon == Some("check"));
                (row.label(), checked)
            })
            .collect();
        assert_eq!(rows, [(Some("Line:Column Number"), true), (Some("Memory Indicator"), false)]);
        let choice = menu[1].command().cloned().expect("a row of the bar's own");
        let write = bar.read_with(cx, |bar, _| bar.write_for(&choice));
        assert_eq!((write.key, &write.value), ("appearance.statusBarWidgets", &json!({ "w.memory": true })));

        // The window stores it and hands the new settings back.
        let stored = stored_overrides(&json!({ "appearance": { "statusBarWidgets": write.value } }));
        bar.update(cx, |bar, cx| bar.set_overrides(stored, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("status:w.memory").is_some());
        assert_eq!(shown(&bar, cx).1, ids(&["w.caret", "w.memory"]));
    }

    #[gpui_kit::test]
    async fn a_click_runs_the_items_action_or_opens_what_belongs_to_it(cx: &mut TestAppContext) {
        let (_, events, cx) = open(
            cx,
            vec![
                (
                    spec("editor.caret", StatusAlign::Right, 20.0),
                    Some(StatusText::new("3:7").on_click(StatusClick::action("navigate.gotoLine"))),
                ),
                (
                    spec("editor.encoding", StatusAlign::Right, 40.0),
                    Some(StatusText::new("UTF-8").on_click(StatusClick::Open)),
                ),
                (
                    spec("editor.lineEnding", StatusAlign::Right, 30.0),
                    Some(StatusText::new("LF").on_click(StatusClick::Open).enabled(false)),
                ),
                (spec("plain", StatusAlign::Left, 0.0), Some(StatusText::new("Indexing"))),
            ],
        );
        for selector in ["status:editor.caret", "status:editor.lineEnding", "status:editor.encoding", "status:plain"] {
            let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
            cx.simulate_click(at, Modifiers::default());
        }
        cx.run_until_parked();
        let events = events.borrow();
        assert_eq!(events.len(), 2, "the disabled and the plain items take no click: {events:?}");
        assert_eq!(
            events[0],
            StatusBarEvent::RunAction {
                item: "editor.caret".to_owned(),
                id: "navigate.gotoLine".to_owned(),
                arg: None
            }
        );
        let StatusBarEvent::OpenItem { item, position } = &events[1] else { panic!("{:?}", events[1]) };
        assert_eq!(item, "editor.encoding");
        let encoding = cx.debug_bounds("status:editor.encoding").expect("drawn");
        assert!(encoding.contains(position), "the menu opens at the click");
    }

    #[gpui_kit::test]
    async fn text_items_draw_their_text_and_items_with_nothing_to_say_are_left_out(cx: &mut TestAppContext) {
        let (bar, _, cx) = open(
            cx,
            vec![
                (spec("a", StatusAlign::Right, 1.0), Some(StatusText::new("first"))),
                (spec("b", StatusAlign::Right, 2.0), Some(StatusText::new("second").suffix("+2").progress(Some(0.5)))),
            ],
        );
        assert_eq!(cx.update(|window, cx| bar.read(cx).texts(window, cx)), ["first", "second"]);
        bar.update(cx, |bar, cx| {
            bar.set_text("a", None, cx);
            let dynamic = StatusContent::dynamic(|window, cx| {
                let ctx = window_context(window, cx);
                ctx.is_truthy("macroRecording").then(|| StatusText::new("Recording macro…"))
            });
            bar.register(spec("c", StatusAlign::Left, 4.0), dynamic, cx);
        });
        cx.run_until_parked();
        assert_eq!(shown(&bar, cx), (Vec::new(), ids(&["b"])));
        assert!(cx.debug_bounds("status:a").is_none());
        cx.update(|window, cx| set_context(window, cx, "macroRecording", true));
        cx.run_until_parked();
        assert_eq!(shown(&bar, cx), (ids(&["c"]), ids(&["b"])));
        assert_eq!(cx.update(|window, cx| bar.read(cx).texts(window, cx)), ["Recording macro…", "second"]);
        assert!(cx.update(|window, cx| bar.read(cx).has_items(window, cx)));
        bar.update(cx, |bar, cx| bar.unregister("b", cx));
        cx.update(|window, cx| set_context(window, cx, "macroRecording", false));
        assert!(!cx.update(|window, cx| bar.read(cx).has_items(window, cx)));
    }
}
