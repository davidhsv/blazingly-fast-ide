//! IDEA's empty editor: what a pane without an open tab shows, a few navigation hints with their
//! shortcuts. A hint whose action exists runs it when clicked.

use gpui_kit::{App, Div, SharedString, Stateful, Window, div, prelude::*, px};

use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use crate::theme;

/// A hint: its text, the action it stands for and the shortcut shown when the keymap binds none.
struct Hint {
    label: &'static str,
    action: Option<&'static str>,
    fallback: Option<&'static str>,
}

const HINTS: [Hint; 5] = [
    Hint { label: "Search Everywhere", action: Some("navigate.searchEverywhere"), fallback: Some("Double ⇧") },
    Hint { label: "Go to File", action: Some("navigate.gotoFile"), fallback: None },
    Hint { label: "Recent Files", action: Some("navigate.recentFiles"), fallback: None },
    Hint { label: "Navigation Bar", action: Some("navigate.navigationBar"), fallback: Some("⌥Home") },
    Hint { label: "Drop files here to open them", action: None, fallback: None },
];

/// A hint as drawn: its text in the active language, its shortcut, and the action a click runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintRow {
    pub label: String,
    pub keys: Option<String>,
    pub runs: Option<&'static str>,
}

/// The hints with their shortcuts as the keymap labels them, in order.
pub fn hint_rows(cx: &App) -> Vec<HintRow> {
    let keymap = cx.try_global::<Keymap>();
    let registry = cx.try_global::<ActionRegistry>();
    HINTS
        .iter()
        .map(|hint| {
            let label = cx
                .try_global::<Localization>()
                .map_or_else(|| hint.label.to_owned(), |localization| localization.0.tx(hint.label));
            let keys = hint.action.and_then(|id| {
                keymap
                    .and_then(|keymap| keymap.0.shortcut_label(id, is_mac_host()))
                    .or_else(|| hint.fallback.map(str::to_owned))
            });
            let runs = hint.action.filter(|id| registry.is_some_and(|registry| registry.contains(id)));
            HintRow { label, keys, runs }
        })
        .collect()
}

/// The empty editor, filling its pane, the hints in the middle.
pub fn empty_editor(cx: &App) -> Stateful<Div> {
    let mut rows = hint_rows(cx);
    // The last hint is a remark under the others, across both columns.
    let remark = rows.pop();
    let (labels, keys): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .map(|row| {
            let label = match row.runs {
                Some(id) => div()
                    .id(SharedString::from(format!("empty-editor:{id}")))
                    .debug_selector(move || format!("empty-editor:{id}"))
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.text_color(theme::color(cx, "fg-secondary")))
                    .cursor_pointer()
                    .on_click(move |_, window: &mut Window, cx: &mut App| {
                        // The click arrives inside the window's update: the action runs after it.
                        window.defer(cx, move |window, cx| {
                            execute_action(id, None, window, cx);
                        });
                    })
                    .child(SharedString::from(row.label))
                    .into_any_element(),
                None => div().child(SharedString::from(row.label)).into_any_element(),
            };
            let keys = div()
                .font_family(super::status_bar::font_family("font-keys", cx))
                .text_color(theme::color(cx, "link"))
                // A hint without a shortcut keeps its line, so the columns stay in step.
                .child(SharedString::from(row.keys.unwrap_or_else(|| " ".to_owned())));
            (label, keys)
        })
        .unzip();
    let column = || div().flex().flex_col().gap(px(8.0));
    div()
        .id("empty-editor")
        .debug_selector(|| "empty-editor".to_owned())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .overflow_hidden()
        .bg(theme::color(cx, "bg-island"))
        .text_color(theme::color(cx, "fg-muted"))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .flex()
                        .gap(px(16.0))
                        .child(column().items_end().children(labels))
                        .child(column().children(keys)),
                )
                .children(remark.map(|remark| {
                    div().mt(px(14.0)).text_color(theme::color(cx, "fg-faint")).child(SharedString::from(remark.label))
                })),
        )
}
