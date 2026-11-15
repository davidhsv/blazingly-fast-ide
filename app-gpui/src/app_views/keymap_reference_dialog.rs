//! Help › Keymap Reference: every action the keymap binds, with its shortcuts, grouped by
//! category, under a filter field and a line naming the keymap and how many shortcuts are listed.
//! The dialog is handed the actions and the effective bindings when it opens; it reports only that
//! it wants to close.

use super::{field_frame, frame, t, title_row, tx};
use crate::app_models::shell::{KeymapGroup, ReferencedAction, keymap_reference};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use std::collections::HashMap;

const WIDTH: f32 = 720.0;
/// The share of the window's height the dialog's body may take before it scrolls.
const MAX_BODY_SHARE: f32 = 0.7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapReferenceEvent {
    Closed,
}

/// What the reference lists: the actions by id, the keymap's (binding, action id) pairs in
/// precedence order, the keymap's name, and whether shortcuts are written the macOS way.
pub struct KeymapReferenceInput {
    pub actions: HashMap<String, ReferencedAction>,
    pub bindings: Vec<(String, String)>,
    pub preset: String,
    pub mac: bool,
}

pub struct KeymapReferenceDialog {
    input: KeymapReferenceInput,
    filter: Entity<TextField>,
    /// The groups the filter keeps, in the order they are listed.
    groups: Vec<KeymapGroup>,
    /// The dialog's own focus: the keyboard is here after a click beside the field.
    focus: FocusHandle,
    key_claims: [u64; 2],
    _filter_events: Subscription,
    /// Like every popup near the top of the window, the reference closes once the window is left.
    _deactivation: Subscription,
}

impl EventEmitter<KeymapReferenceEvent> for KeymapReferenceDialog {}

impl KeymapReferenceDialog {
    pub fn new(input: KeymapReferenceInput, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(cx, "app.keymap.filter");
        let filter = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let filter_events =
            cx.subscribe_in(&filter, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => dialog.refilter(cx),
                TextFieldEvent::Submitted => {}
                TextFieldEvent::Cancelled => cx.emit(KeymapReferenceEvent::Closed),
            });
        let focus = cx.focus_handle();
        // Escape closes from the field and from the dialog around it, before any shortcut sees it.
        let key_claims = [focus.clone(), filter.focus_handle(cx)].map(|focus| {
            let claimant = cx.weak_entity();
            input::claim_keys(
                focus,
                move |keystroke, _, cx| {
                    claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false)
                },
                cx,
            )
        });
        cx.on_release(|dialog, cx| {
            for claim in dialog.key_claims {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        let deactivation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(KeymapReferenceEvent::Closed);
            }
        });
        let mut dialog = Self {
            input,
            filter,
            groups: Vec::new(),
            focus,
            key_claims,
            _filter_events: filter_events,
            _deactivation: deactivation,
        };
        dialog.groups = dialog.compute_groups(cx);
        dialog
    }

    /// The filter field, which has the keyboard when the dialog opens.
    pub fn filter_field(&self) -> &Entity<TextField> {
        &self.filter
    }

    /// The groups listed, each with its entries in title order.
    pub fn groups(&self) -> &[KeymapGroup] {
        &self.groups
    }

    /// The categories listed, in order.
    pub fn categories(&self) -> Vec<&str> {
        self.groups.iter().map(|group| group.category.as_str()).collect()
    }

    /// How many actions are listed.
    pub fn count(&self) -> usize {
        self.groups.iter().map(|group| group.entries.len()).sum()
    }

    /// The line beside the filter, in the UI language: `Keymap: <preset> · <n> shortcuts`.
    pub fn meta_line(&self, cx: &mut App) -> String {
        let i18n = &cx.default_global::<Localization>().0;
        let preset = i18n.t("app.keymap.preset", &[("preset", self.input.preset.as_str().into())]);
        let count = i18n.t("app.keymap.count", &[("count", self.count().into())]);
        format!("{preset} · {count}")
    }

    fn compute_groups(&self, cx: &mut App) -> Vec<KeymapGroup> {
        let other = t(cx, "app.keymap.other");
        let filter = self.filter.read(cx).text().to_owned();
        keymap_reference(&self.input.actions, &self.input.bindings, &filter, &other, self.input.mac)
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.groups = self.compute_groups(cx);
        cx.notify();
    }

    fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.update(cx, |field, cx| field.set_text("", cx));
        let focus = self.filter.focus_handle(cx);
        window.focus(&focus, cx);
        self.refilter(cx);
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() || keystroke.key != "escape" {
            return false;
        }
        cx.emit(KeymapReferenceEvent::Closed);
        true
    }

    /// A group: its category as a heading over its rows, each row an action's title and its
    /// shortcuts, the action's id as its tooltip.
    fn group_element(group: &KeymapGroup, cx: &App) -> Div {
        let subtle = theme::color(cx, "fg-subtle");
        let keys_font = theme::installed_fonts::theme_family(cx, "font-keys", ".SystemUIFont");
        let row_height = theme::length(cx, "h-row");
        let rows = group.entries.iter().map(|entry| {
            let keys = entry.shortcuts.iter().map(|shortcut| {
                div()
                    .font_family(keys_font.clone())
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-muted"))
                    .whitespace_nowrap()
                    .child(SharedString::from(shortcut.clone()))
            });
            let id = entry.id.clone();
            div()
                .id(SharedString::from(format!("keymap-row:{id}")))
                .flex()
                .items_center()
                .gap(px(12.0))
                .min_h(row_height)
                .px(px(4.0))
                .rounded(theme::length(cx, "radius-row"))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .with_tooltip(id)
                .child(div().flex_1().min_w_0().truncate().child(SharedString::from(entry.title.clone())))
                .child(div().flex_none().flex().gap(px(4.0)).children(keys))
        });
        let category = group.category.clone();
        div()
            .debug_selector(move || format!("keymap-group:{category}"))
            .pt(px(8.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(row_height)
                    .flex()
                    .items_center()
                    .text_color(subtle)
                    .text_size(theme::length(cx, "fs-small"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(group.category.to_uppercase())),
            )
            .children(rows)
    }
}

/// Where the groups are split into the two columns: the first `n` groups go on the left, chosen so
/// that the taller column is as short as can be (a heading counts as a row; a group is not split).
fn column_split(groups: &[KeymapGroup]) -> usize {
    let heights: Vec<usize> = groups.iter().map(|group| group.entries.len() + 1).collect();
    let total: usize = heights.iter().sum();
    let mut left = 0;
    let mut best = (usize::MAX, 0);
    for (index, height) in heights.iter().enumerate() {
        left += height;
        let taller = left.max(total - left);
        if taller < best.0 {
            best = (taller, index + 1);
        }
    }
    best.1
}

impl Focusable for KeymapReferenceDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.filter.focus_handle(cx)
    }
}

impl Render for KeymapReferenceDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = t(cx, "app.keymap.title");
        let meta = self.meta_line(cx);
        let clear_label = tx(cx, "Clear");
        let clear = (!self.filter.read(cx).text().is_empty()).then(|| {
            let label = clear_label;
            div()
                .id("keymap-filter-clear")
                .debug_selector(|| "keymap-filter-clear".to_owned())
                .flex_none()
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .with_tooltip(label)
                .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-muted")))
                .on_click(cx.listener(|dialog, _: &ClickEvent, window, cx| dialog.clear_filter(window, cx)))
        });
        let search = field_frame(cx, false)
            .flex_1()
            .min_w_0()
            .gap(px(6.0))
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-subtle")))
            .child(div().flex_1().min_w_0().child(self.filter.clone()))
            .children(clear);
        let bar = div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(12.0))
            .py(px(8.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .child(search)
            .child(
                div()
                    .flex_none()
                    .debug_selector(|| "keymap-meta".to_owned())
                    .text_color(theme::color(cx, "fg-subtle"))
                    .text_size(theme::length(cx, "fs-small"))
                    .child(SharedString::from(meta)),
            );
        let content = if self.groups.is_empty() {
            let empty = t(cx, "app.keymap.empty");
            div()
                .debug_selector(|| "keymap-empty".to_owned())
                .py(px(24.0))
                .flex()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(empty))
        } else {
            let split = column_split(&self.groups);
            let (left, right) = self.groups.split_at(split);
            let app: &App = cx;
            let column = |groups: &[KeymapGroup]| {
                div().flex_1().min_w_0().flex().flex_col().children(groups.iter().map(|g| Self::group_element(g, app)))
            };
            div().flex().gap(px(24.0)).child(column(left)).child(column(right))
        };
        let body = div()
            .id("keymap-body")
            .max_h(window.viewport_size().height * MAX_BODY_SHARE)
            .overflow_y_scroll()
            .px(px(12.0))
            .pt(px(4.0))
            .pb(px(12.0))
            .child(content);
        frame(cx, WIDTH)
            .debug_selector(|| "keymap-reference".to_owned())
            .track_focus(&self.focus)
            .child(title_row(cx, title))
            .child(bar)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<KeymapReferenceEvent>>>;

    fn input() -> KeymapReferenceInput {
        let action = |title: &str, category: Option<&str>, hidden: bool| ReferencedAction {
            title: title.to_owned(),
            category: category.map(str::to_owned),
            hidden,
        };
        let actions = HashMap::from([
            ("file.save".to_owned(), action("Save All", Some("File"), false)),
            ("hidden.one".to_owned(), action("Hidden", None, true)),
        ]);
        let bindings = [
            ("Meta+S", "file.save"),
            ("Meta+O", "navigate.gotoClass"),
            ("Meta+-", "code.foldToggle"),
            ("Meta+=", "code.foldToggle"),
            ("Meta+H", "hidden.one"),
            ("Alt+X", "zzz.custom"),
        ]
        .map(|(binding, id)| (binding.to_owned(), id.to_owned()))
        .to_vec();
        KeymapReferenceInput { actions, bindings, preset: "intellij-macos".to_owned(), mac: true }
    }

    fn open(cx: &mut TestAppContext) -> (Entity<KeymapReferenceDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|window, cx| KeymapReferenceDialog::new(input(), window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            window.activate_window();
            cx.subscribe(&dialog, move |_, event: &KeymapReferenceEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn ids(dialog: &Entity<KeymapReferenceDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.read_with(cx, |dialog, _| {
            dialog.groups().iter().flat_map(|group| group.entries.iter().map(|entry| entry.id.clone())).collect()
        })
    }

    // src/features/app/__tests__/shell.test.tsx › keymap reference › renders the overlay
    #[gpui_kit::test]
    async fn renders_the_overlay(cx: &mut TestAppContext) {
        let (dialog, _events, cx) = open(cx);
        let categories =
            dialog.read_with(cx, |dialog, _| dialog.categories().into_iter().map(str::to_owned).collect::<Vec<_>>());
        assert_eq!(categories, ["Code", "File", "Navigate", "Other"]);
        assert!(cx.debug_bounds("keymap-reference").is_some(), "the dialog is drawn");
        assert!(cx.debug_bounds("keymap-group:Navigate").is_some(), "the Navigate group is drawn");
        let meta = dialog.update(cx, |dialog, cx| dialog.meta_line(cx));
        assert_eq!(meta, "Keymap: intellij-macos · 4 shortcuts");
    }

    /// Typing in the field keeps the actions whose title, id or shortcut contains the text; the
    /// count follows, and Clear brings every action back.
    #[gpui_kit::test]
    async fn typing_filters_the_list_and_clear_restores_it(cx: &mut TestAppContext) {
        let (dialog, _events, cx) = open(cx);
        cx.simulate_input("save");
        assert_eq!(ids(&dialog, cx), ["file.save"]);
        let meta = dialog.update(cx, |dialog, cx| dialog.meta_line(cx));
        assert_eq!(meta, "Keymap: intellij-macos · 1 shortcut");
        let bounds = cx.debug_bounds("keymap-filter-clear").expect("Clear is drawn while there is text");
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(ids(&dialog, cx).len(), 4);
        assert!(cx.debug_bounds("keymap-filter-clear").is_none(), "Clear goes with the text");
    }

    /// A filter nothing matches says so instead of listing groups.
    #[gpui_kit::test]
    async fn a_filter_nothing_matches_says_there_is_no_matching_shortcut(cx: &mut TestAppContext) {
        let (dialog, _events, cx) = open(cx);
        cx.simulate_input("nothing like this");
        assert!(dialog.read_with(cx, |dialog, _| dialog.groups().is_empty()));
        assert!(cx.debug_bounds("keymap-empty").is_some(), "No matching shortcuts is drawn");
    }

    /// Escape in the field closes the dialog.
    #[gpui_kit::test]
    async fn escape_closes(cx: &mut TestAppContext) {
        let (_dialog, events, cx) = open(cx);
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [KeymapReferenceEvent::Closed]);
    }

    #[test]
    fn the_groups_are_split_into_two_columns_of_about_the_same_height() {
        let group = |rows: usize| KeymapGroup {
            category: format!("c{rows}"),
            entries: (0..rows)
                .map(|n| crate::app_models::shell::KeymapEntry {
                    id: n.to_string(),
                    title: n.to_string(),
                    shortcuts: Vec::new(),
                })
                .collect(),
        };
        assert_eq!(column_split(&[group(3), group(3)]), 1);
        assert_eq!(column_split(&[group(9), group(1), group(1)]), 1);
        assert_eq!(column_split(&[group(1), group(1), group(5)]), 2);
        assert_eq!(column_split(&[]), 0);
        assert_eq!(column_split(&[group(2)]), 1);
    }
}
