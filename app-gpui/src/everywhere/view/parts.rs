//! The parts of the Search Everywhere popup around its rows: the header with the tabs and the scope check
//! box, the hint in the query field, the footer that describes the selected row, and each result's line
//! (its icon, its name with the matched characters marked, an action's shortcut).

use super::SearchEverywhere;
use crate::app_views;
use crate::everywhere::model::{SeItem, SeRow, SeTab};
use crate::everywhere::recent_view::marked_text;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input::Keymap;
use crate::platform::keymap::{format_binding, host_binding, is_mac_host};
use crate::platform::virtual_docs::basename;
use crate::theme;
use gpui_kit::{AnyElement, App, Context, Div, Hsla, SharedString, Stateful, div, prelude::*, px};

/// The widest the popup is drawn.
pub const WIDTH: f32 = 760.0;
/// What a narrower window leaves free on each side of the popup.
pub const WINDOW_MARGIN: f32 = 24.0;
/// At most this wide, the header gives the tabs a line of their own and the query field drops its hint.
pub const NARROW_WIDTH: f32 = 680.0;
/// The height of the header on one line.
pub const HEADER_HEIGHT: f32 = 44.0;
/// The height of the footer.
pub const FOOTER_HEIGHT: f32 = 30.0;
/// The size of a row's icon, and of the empty place an action without one keeps.
const ICON: f32 = 16.0;

/// How wide the popup is in a window `viewport` wide.
pub fn popup_width(viewport: f32) -> f32 {
    WIDTH.min(viewport - 2.0 * WINDOW_MARGIN).max(0.0)
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The shortcut of `action` as menus show it.
fn shortcut(cx: &App, action: &str) -> Option<String> {
    cx.try_global::<Keymap>()?.0.shortcut_label(action, is_mac_host())
}

/// The label of the scope check box of `tab`: on Actions it shows the actions that cannot run here.
pub fn scope_label(tab: SeTab) -> &'static str {
    if tab == SeTab::Actions { "Include disabled actions" } else { "Include non-project items" }
}

/// What the query field says on its right on `tab`, if anything.
pub fn hint(tab: SeTab, cx: &mut App) -> Option<String> {
    match tab {
        SeTab::All => Some(tx(cx, "Type / to see commands")),
        SeTab::Text => {
            let keys = format_binding(&host_binding("Meta+Enter", is_mac_host()), is_mac_host());
            let i18n = &mut cx.default_global::<Localization>().0;
            Some(i18n.tx_built(&["", " to open in Find tool window"], &[Param::from(keys)]))
        }
        _ => None,
    }
}

/// What the footer says about `row`: an action's description (or category and title) and how to give it
/// a shortcut, a setting's place, a file's path, a symbol's or a text match's path and line.
pub fn describe(row: &SeRow, cx: &mut App) -> String {
    let SeRow::Item { item, .. } = row else { return String::new() };
    match item {
        SeItem::Action(action) => {
            let described = cx
                .try_global::<ActionRegistry>()
                .and_then(|registry| registry.get(&action.id))
                .and_then(|found| found.meta.description.clone());
            let text = described.unwrap_or_else(|| match &action.category {
                Some(category) => format!("{category} | {}", action.title),
                None => action.title.clone(),
            });
            let keys = format_binding("Alt+Enter", is_mac_host());
            let i18n = &mut cx.default_global::<Localization>().0;
            let assign = i18n.tx_built(&["", " to assign a shortcut"], &[Param::from(keys)]);
            format!("{text}  ·  {assign}")
        }
        SeItem::Setting(setting) => {
            let parts: Vec<String> = setting.path.iter().chain([&setting.title]).map(|part| tx(cx, part)).collect();
            parts.join(" | ")
        }
        SeItem::File(file) => file.rel.clone(),
        SeItem::Symbol(symbol) => format!("{}:{}", symbol.rel, symbol.line + 1),
        SeItem::Text(text) => format!("{}:{}", text.rel, text.line + 1),
    }
}

/// The fixed place of a row's icon, so that the names of every kind of row start at the same x.
fn icon_slot(icon: Option<AnyElement>) -> Div {
    div().flex_none().size(px(ICON)).flex().items_center().justify_center().children(icon)
}

/// The name of row `index`, with its matched characters marked.
fn name(index: usize, text: &str, indices: &[usize], color: Hsla, cx: &App) -> Stateful<Div> {
    div()
        .id(("search-everywhere-name", index))
        .debug_selector(move || format!("search-everywhere-name-{index}"))
        .flex_none()
        .min_w_0()
        .overflow_hidden()
        .text_color(color)
        .child(marked_text(cx, text, indices))
}

/// Text on the right of a row, or after its name.
fn detail(text: String, color: Hsla) -> Div {
    div().flex_none().min_w_0().overflow_hidden().text_color(color).child(SharedString::from(text))
}

/// The line of result `item`, row `index` of the list, drawn into `line`.
pub fn item_line(line: Stateful<Div>, index: usize, item: &SeItem, cx: &mut App) -> Stateful<Div> {
    let fg = theme::color(cx, "fg-strong");
    let (subtle, faint) = (theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
    let main = || div().flex_1().min_w_0().flex().items_center().gap(px(6.0)).overflow_hidden();
    match item {
        SeItem::File(file) => {
            let icon = theme::icon(cx, theme::icons::file_icon_name(&file.name), ICON, fg);
            let suffix = file.line.map(|line| detail(format!(":{}", line + 1), theme::color(cx, "fg-muted")));
            line.child(icon_slot(Some(icon))).child(
                main()
                    .child(name(index, &file.name, &file.name_indices, fg, cx))
                    .children(suffix)
                    .child(detail(file.dir.clone(), subtle)),
            )
        }
        SeItem::Symbol(symbol) => line
            .child(icon_slot(None))
            .child(
                main()
                    .child(name(index, &symbol.display, &symbol.indices, fg, cx))
                    .children(symbol.container.clone().map(|container| detail(container, subtle))),
            )
            .child(detail(symbol.rel.clone(), subtle)),
        SeItem::Action(action) => {
            // An action that cannot run here is listed on the faint colour.
            let (name_color, other) = if action.enabled { (fg, subtle) } else { (faint, faint) };
            let icon = action.icon.as_deref().map(|icon| theme::icon(cx, icon, ICON, other));
            let keys = shortcut(cx, &action.id).map(|keys| {
                let selector = format!("search-everywhere-shortcut-{index}");
                let font = theme::installed_fonts::theme_family(cx, "font-keys", ".SystemUIFont");
                detail(keys, other).debug_selector(move || selector).font_family(font)
            });
            line.child(icon_slot(icon))
                .child(main().child(name(index, &action.title, &action.indices, name_color, cx)).children(keys))
                .children(action.category.clone().map(|category| detail(category, other)))
        }
        SeItem::Text(text) => {
            let icon = theme::icon(cx, "text-match", ICON, subtle);
            let place = format!("{} {}", basename(&text.rel), text.line + 1);
            line.child(icon_slot(Some(icon)))
                .child(main().child(name(index, text.text.trim(), &text.indices, fg, cx)))
                .child(detail(place, subtle))
        }
        SeItem::Setting(setting) => {
            let icon = theme::icon(cx, "settings", ICON, subtle);
            let mut place = tx(cx, "Settings");
            if !setting.path.is_empty() {
                let path: Vec<String> = setting.path.iter().map(|part| tx(cx, part)).collect();
                place = format!("{place}: {}", path.join(" | "));
            }
            line.child(icon_slot(Some(icon)))
                .child(main().child(name(index, &setting.title, &setting.indices, fg, cx)))
                .child(detail(place, subtle))
        }
    }
}

impl SearchEverywhere {
    /// The header: the tabs, then the scope check box of the shown tab. In a narrow popup the tabs take a
    /// line of their own.
    pub(super) fn header(&self, tabs: Div, narrow: bool, cx: &mut Context<Self>) -> Div {
        let label = tx(cx, scope_label(self.tab));
        let on = self.memory.state.include_non_project;
        let scope = app_views::checkbox(cx, label, on)
            .id("search-everywhere-scope")
            .debug_selector(|| "search-everywhere-scope".to_owned())
            .cursor_pointer()
            .on_click(cx.listener(move |popup, _, window, cx| {
                popup.set_include_non_project(!on, cx);
                window.focus(&popup.field.focus_handle(cx), cx);
            }));
        div()
            .debug_selector(|| "search-everywhere-header".to_owned())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .bg(theme::color(cx, "bg-frame"))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .when(narrow, |el| el.flex_wrap().gap(px(6.0)).px(px(8.0)).py(px(6.0)))
            .when(!narrow, |el| el.h(px(HEADER_HEIGHT)).px(px(8.0)))
            .child(
                tabs.debug_selector(|| "search-everywhere-tabs".to_owned())
                    .when(narrow, |el| el.w_full())
                    .when(!narrow, |el| el.flex_1().min_w_0()),
            )
            .child(div().flex_none().flex().items_center().gap(px(8.0)).child(scope))
    }

    /// The footer under the rows: what the selected row is.
    pub(super) fn footer(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.rows.is_empty() {
            return None;
        }
        let text = self.footer_text(cx);
        Some(
            div()
                .debug_selector(|| "search-everywhere-footer".to_owned())
                .flex_none()
                .h(px(FOOTER_HEIGHT))
                .px(px(12.0))
                .flex()
                .items_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .bg(theme::color(cx, "bg-frame"))
                .border_t_1()
                .border_color(theme::color(cx, "border-popup"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(text))),
        )
    }

    /// What the footer says, for tests and the accessibility tree.
    pub fn footer_text(&self, cx: &mut App) -> String {
        self.active.and_then(|index| self.rows.get(index)).map(|row| describe(row, cx)).unwrap_or_default()
    }
}
