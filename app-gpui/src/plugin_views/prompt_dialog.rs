//! What a plugin asks the user while one of its commands waits (`ui.input`, `ui.input-multiline`,
//! `ui.pick`, `ui.pick-many`, `ui.confirm`): a dialog over the window whose answer goes back to the
//! plugin.
//!
//! The plugin's title, prompt and item labels are shown as written. Typed text comes back as typed
//! (spaces kept, nothing refused); a one-line field confirms on Enter, a multi-line one on
//! Cmd/Ctrl+Enter, Enter adding a line. A pick list moves with Up, Down, Home, End, Page Up and
//! Page Down; Enter picks the row of a single pick and ticks or unticks it in a multiple pick,
//! whose OK returns the ticked keys in list order. Escape and Cancel answer "dismissed".
//!
//! [`PluginPrompt`] calls nothing: its one [`PluginPromptEvent::Answered`] carries what the
//! workbench reports to the plugin host.

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*,
    px, uniform_list,
};
use ide_plugin::PickItem;
use std::collections::BTreeSet;
use std::ops::Range;

const WIDTH: f32 = 420.0;
const BUTTON_HEIGHT: f32 = 28.0;
const FIELD_HEIGHT: f32 = 28.0;
/// Rows of a pick list without descriptions, and with them.
const ROW_HEIGHT: f32 = 24.0;
const TALL_ROW_HEIGHT: f32 = 36.0;
/// How many rows a pick list shows before it scrolls; Page Up and Page Down move one less.
const VISIBLE_ROWS: usize = 10;
/// Rows of the multi-line field.
const AREA_ROWS: usize = 5;

/// What the prompt tells its window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginPromptEvent {
    /// The user is done: the answer for the plugin, `None` when the prompt was dismissed. Input
    /// answers `[text]`, a pick the chosen keys, a confirmation an empty list.
    Answered(Option<Vec<String>>),
}

/// The field of a text prompt.
enum Field {
    Line(Entity<TextField>),
    Area(Entity<TextArea>),
}

impl Field {
    fn text(&self, cx: &App) -> String {
        match self {
            Self::Line(field) => field.read(cx).text().to_owned(),
            Self::Area(area) => area.read(cx).text().to_owned(),
        }
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Line(field) => field.focus_handle(cx),
            Self::Area(area) => area.focus_handle(cx),
        }
    }
}

/// What the prompt asks.
enum Body {
    Input { field: Field },
    Pick { items: Vec<PickItem>, multiple: bool, active: Option<usize>, checked: BTreeSet<String> },
    Confirm { message: String },
}

pub struct PluginPrompt {
    title: String,
    body: Body,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    key_claim: u64,
    /// The prompt has answered: later presses change nothing.
    answered: bool,
    _field_events: Option<Subscription>,
}

impl EventEmitter<PluginPromptEvent> for PluginPrompt {}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// A check box with its label.
fn checkbox(cx: &App, label: SharedString, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().child(label))
}

/// A dialog button: filled when `primary`, greyed out when it cannot be pressed.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .min_w(px(72.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

impl PluginPrompt {
    /// A text prompt (`ui.input`, `ui.input-multiline`): `prompt` is its title, `value` the text it
    /// starts with.
    pub fn input(
        prompt: &str,
        value: Option<&str>,
        multiline: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = value.unwrap_or_default();
        let (field, events) = if multiline {
            let area = cx.new(|cx| {
                let mut area = TextArea::new(cx);
                area.set_text(initial, cx);
                area.set_rows(AREA_ROWS, AREA_ROWS, cx);
                area
            });
            let events =
                cx.subscribe_in(&area, window, |prompt: &mut Self, _, event: &TextAreaEvent, _, cx| match event {
                    TextAreaEvent::Submitted => prompt.confirm(cx),
                    TextAreaEvent::Cancelled => prompt.answer(None, cx),
                    TextAreaEvent::Changed | TextAreaEvent::Blurred => {}
                });
            (Field::Area(area), events)
        } else {
            let field = cx.new(|cx| {
                let mut field = TextField::new(cx);
                field.set_text(initial, cx);
                field
            });
            let events =
                cx.subscribe_in(&field, window, |prompt: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                    TextFieldEvent::Submitted => prompt.confirm(cx),
                    TextFieldEvent::Cancelled => prompt.answer(None, cx),
                    TextFieldEvent::Changed => cx.notify(),
                });
            (Field::Line(field), events)
        };
        let focus = field.focus_handle(cx);
        Self::build(prompt.to_owned(), Body::Input { field }, focus, Some(events), cx)
    }

    /// A list to pick one entry from (`ui.pick`) or several (`ui.pick-many`). The first row starts
    /// active; nothing starts ticked.
    pub fn pick(title: &str, items: Vec<PickItem>, multiple: bool, cx: &mut Context<Self>) -> Self {
        let active = (!items.is_empty()).then_some(0);
        let body = Body::Pick { items, multiple, active, checked: BTreeSet::new() };
        let focus = cx.focus_handle();
        Self::build(title.to_owned(), body, focus, None, cx)
    }

    /// A yes / no question (`ui.confirm`).
    pub fn confirm_question(title: &str, message: &str, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        Self::build(title.to_owned(), Body::Confirm { message: message.to_owned() }, focus, None, cx)
    }

    fn build(
        title: String,
        body: Body,
        focus: FocusHandle,
        field_events: Option<Subscription>,
        cx: &mut Context<Self>,
    ) -> Self {
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |prompt, cx| prompt.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|prompt, cx| input::release_keys(prompt.key_claim, cx)).detach();
        Self {
            title,
            body,
            focus,
            scroll: UniformListScrollHandle::new(),
            key_claim,
            answered: false,
            _field_events: field_events,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The text typed so far, for a text prompt.
    pub fn text(&self, cx: &App) -> Option<String> {
        match &self.body {
            Body::Input { field } => Some(field.text(cx)),
            _ => None,
        }
    }

    /// Whether the text prompt takes several lines.
    pub fn is_multiline(&self) -> bool {
        matches!(&self.body, Body::Input { field: Field::Area(_) })
    }

    /// The one-line field of a text prompt.
    pub fn field(&self) -> Option<&Entity<TextField>> {
        match &self.body {
            Body::Input { field: Field::Line(field) } => Some(field),
            _ => None,
        }
    }

    /// The multi-line field of a text prompt.
    pub fn area(&self) -> Option<&Entity<TextArea>> {
        match &self.body {
            Body::Input { field: Field::Area(area) } => Some(area),
            _ => None,
        }
    }

    /// The rows of a pick list: the label, and the description after a tab when there is one.
    pub fn rows(&self) -> Vec<String> {
        let Body::Pick { items, .. } = &self.body else { return Vec::new() };
        items
            .iter()
            .map(|item| match &item.description {
                Some(description) => format!("{}\t{description}", item.label),
                None => item.label.clone(),
            })
            .collect()
    }

    /// The key of the active row of a pick list.
    pub fn active(&self) -> Option<&str> {
        let Body::Pick { items, active, .. } = &self.body else { return None };
        active.and_then(|index| items.get(index)).map(|item| item.key.as_str())
    }

    /// The ticked keys of a multiple pick, in list order.
    pub fn checked(&self) -> Vec<String> {
        let Body::Pick { items, checked, .. } = &self.body else { return Vec::new() };
        items.iter().filter(|item| checked.contains(&item.key)).map(|item| item.key.clone()).collect()
    }

    /// The message of a confirmation.
    pub fn message(&self) -> Option<&str> {
        match &self.body {
            Body::Confirm { message } => Some(message),
            _ => None,
        }
    }

    /// Whether OK can be pressed: a single pick needs an active row; everything else always can.
    pub fn can_confirm(&self) -> bool {
        match &self.body {
            Body::Pick { multiple: false, active, .. } => active.is_some(),
            _ => true,
        }
    }

    /// OK: the text as typed, the active key of a single pick, the ticked keys of a multiple one, an
    /// empty list for a confirmation.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        let answer = match &self.body {
            Body::Input { field } => vec![field.text(cx)],
            Body::Pick { multiple: true, .. } => self.checked(),
            Body::Pick { multiple: false, .. } => match self.active() {
                Some(key) => vec![key.to_owned()],
                None => return,
            },
            Body::Confirm { .. } => Vec::new(),
        };
        self.answer(Some(answer), cx);
    }

    /// Cancel or Escape: the plugin hears that the prompt was dismissed.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.answer(None, cx);
    }

    fn answer(&mut self, answer: Option<Vec<String>>, cx: &mut Context<Self>) {
        if self.answered {
            return;
        }
        self.answered = true;
        cx.emit(PluginPromptEvent::Answered(answer));
    }

    /// Ticks or unticks the row `index` of a multiple pick.
    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        let Body::Pick { items, multiple: true, active, checked } = &mut self.body else { return };
        let Some(item) = items.get(index) else { return };
        if !checked.remove(&item.key) {
            checked.insert(item.key.clone());
        }
        *active = Some(index);
        cx.notify();
    }

    /// A click on row `index`: a single pick answers with it, a multiple one ticks it.
    fn click_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if matches!(self.body, Body::Pick { multiple: true, .. }) {
            self.toggle(index, cx);
            return;
        }
        let Body::Pick { items, active, .. } = &mut self.body else { return };
        *active = Some(index);
        let picked = items.get(index).map(|item| item.key.clone());
        if let Some(key) = picked {
            self.answer(Some(vec![key]), cx);
        }
    }

    /// Enter on the list: picks the active row of a single pick, ticks it in a multiple one.
    fn activate(&mut self, cx: &mut Context<Self>) {
        let Body::Pick { multiple, active: Some(index), .. } = &self.body else { return };
        let (multiple, index) = (*multiple, *index);
        if multiple { self.toggle(index, cx) } else { self.confirm(cx) }
    }

    /// The row a list key moves to, as a list without wrapping does: `None` for other keys.
    fn navigate(key: &str, active: Option<usize>, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let last = len - 1;
        let page = VISIBLE_ROWS - 1;
        let next = match (key, active) {
            ("down", Some(at)) => (at + 1).min(last),
            ("down", None) | ("home", _) => 0,
            ("up", Some(at)) => at.saturating_sub(1),
            ("up", None) | ("end", _) => last,
            ("pagedown", Some(at)) => (at + page).min(last),
            ("pagedown", None) => page.min(last),
            ("pageup", Some(at)) => at.saturating_sub(page),
            ("pageup", None) => 0,
            _ => return None,
        };
        Some(next)
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let key = keystroke.key.as_str();
        let modifiers = &keystroke.modifiers;
        if let Body::Input { field } = &self.body {
            // Enter and Escape of a one-line field arrive as the field's own events; Cmd/Ctrl+Enter
            // confirms a multi-line one, which gets every other key.
            let Field::Area(area) = field else { return false };
            let submit =
                key == "enter" && (modifiers.platform || modifiers.control) && !modifiers.shift && !modifiers.alt;
            if submit {
                let area = area.clone();
                area.update(cx, |area, cx| area.submit(cx));
            }
            return submit;
        }
        if modifiers.modified() {
            return false;
        }
        match key {
            "enter" if matches!(self.body, Body::Pick { .. }) => self.activate(cx),
            "enter" => self.confirm(cx),
            "escape" => self.dismiss(cx),
            _ => {
                let Body::Pick { items, active, .. } = &mut self.body else { return false };
                let Some(next) = Self::navigate(key, *active, items.len()) else { return false };
                *active = Some(next);
                self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
                cx.notify();
            }
        }
        true
    }

    fn render_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        match &self.body {
            Body::Input { field: Field::Line(field) } => div()
                .h(px(FIELD_HEIGHT))
                .flex()
                .items_center()
                .px(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .bg(theme::color(cx, "bg-input"))
                .debug_selector(|| "plugin-prompt-field".to_owned())
                .child(div().flex_1().min_w_0().child(field.clone()))
                .into_any_element(),
            Body::Input { field: Field::Area(area) } => div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .bg(theme::color(cx, "bg-input"))
                .debug_selector(|| "plugin-prompt-area".to_owned())
                .child(area.clone())
                .into_any_element(),
            Body::Confirm { message } => div()
                .flex()
                .flex_col()
                .text_color(fg)
                .children(message.lines().map(|line| div().min_h(px(16.0)).child(SharedString::from(line.to_owned()))))
                .into_any_element(),
            Body::Pick { items, .. } if items.is_empty() => {
                let text = tx(cx, "Nothing to choose from");
                div().py(px(8.0)).text_color(muted).child(SharedString::from(text)).into_any_element()
            }
            Body::Pick { items, multiple, active, checked } => {
                let tall = items.iter().any(|item| item.description.is_some());
                let height = if tall { TALL_ROW_HEIGHT } else { ROW_HEIGHT };
                let rows: Vec<(SharedString, Option<SharedString>, bool, bool)> = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        (
                            SharedString::from(item.label.clone()),
                            item.description.clone().map(SharedString::from),
                            *active == Some(index),
                            checked.contains(&item.key),
                        )
                    })
                    .collect();
                let multiple = *multiple;
                let shown = rows.len().min(VISIBLE_ROWS) as f32 * height;
                let list = uniform_list(
                    "plugin-prompt-items",
                    rows.len(),
                    cx.processor(move |_, range: Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let (label, description, active, on) = rows[index].clone();
                                let selected = theme::color(cx, "bg-list-selected");
                                let label = if multiple {
                                    checkbox(cx, label, on).into_any_element()
                                } else {
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(label)
                                        .into_any_element()
                                };
                                let hint = description.map(|description| {
                                    div()
                                        .text_color(theme::color(cx, "fg-muted"))
                                        .text_size(theme::length(cx, "fs-small"))
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .child(description)
                                });
                                div()
                                    .id(index)
                                    .debug_selector(move || format!("plugin-prompt-row-{index}"))
                                    .h(px(height))
                                    .px(px(8.0))
                                    .flex()
                                    .flex_col()
                                    .justify_center()
                                    .rounded(theme::length(cx, "radius-small"))
                                    .when(active, |el| el.bg(selected))
                                    .child(label)
                                    .children(hint)
                                    .on_click(
                                        cx.listener(move |prompt, _: &ClickEvent, _, cx| prompt.click_row(index, cx)),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .h(px(shown));
                div()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .p(px(2.0))
                    .child(list)
                    .into_any_element()
            }
        }
    }
}

impl Focusable for PluginPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PluginPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let body = self.render_body(cx);
        let (ok, cancel) = (tx(cx, "OK"), cx.default_global::<Localization>().0.t("common.cancel", &[]));
        let can_confirm = self.can_confirm();
        let buttons = [
            button(cx, ok, true, can_confirm)
                .id("plugin-prompt-ok")
                .debug_selector(|| "plugin-prompt-ok".to_owned())
                .on_click(cx.listener(|prompt, _: &ClickEvent, _, cx| {
                    if prompt.can_confirm() {
                        prompt.confirm(cx);
                    }
                })),
            button(cx, cancel, false, true)
                .id("plugin-prompt-cancel")
                .debug_selector(|| "plugin-prompt-cancel".to_owned())
                .on_click(cx.listener(|prompt, _: &ClickEvent, _, cx| prompt.dismiss(cx))),
        ];
        let holds_focus = !matches!(self.body, Body::Input { .. });
        let focus = self.focus.clone();
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .debug_selector(|| "plugin-prompt".to_owned())
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .when(holds_focus, |el| el.track_focus(&focus))
            // A press on the dialog is not a press beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(self.title.clone())),
            )
            .child(div().px(px(16.0)).py(px(4.0)).child(body))
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<Vec<String>>>>>;

    fn item(key: &str, label: &str, description: Option<&str>) -> PickItem {
        PickItem { key: key.to_owned(), label: label.to_owned(), description: description.map(str::to_owned) }
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The prompt in a window of its own, with the keyboard, and the answers it gave.
    fn open(
        cx: &mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<PluginPrompt>) -> PluginPrompt + 'static,
    ) -> (Entity<PluginPrompt>, Answers, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(build);
        let answers: Answers = Rc::default();
        let log = answers.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &PluginPromptEvent, _| {
                let PluginPromptEvent::Answered(answer) = event;
                log.borrow_mut().push(answer.clone());
            })
            .detach();
            window.activate_window();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, answers, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }

    fn targets() -> Vec<PickItem> {
        vec![item("a", "Alpha", None), item("b", "Beta", Some("second"))]
    }

    // src/features/plugins/__tests__/plugins.test.tsx › plugin prompts
    #[gpui_kit::test]
    async fn shows_a_plugins_question_and_reports_the_answer_or_the_dismissal(cx: &mut TestAppContext) {
        init(cx);
        let (view, answers, cx) = open(cx, |_, cx| PluginPrompt::pick("Pick targets", targets(), true, cx));
        assert_eq!(view.read_with(cx, |view, _| view.rows()), ["Alpha", "Beta\tsecond"]);
        assert_eq!(view.read_with(cx, |view, _| view.title().to_owned()), "Pick targets");
        click(cx, "plugin-prompt-row-1");
        assert_eq!(view.read_with(cx, |view, _| view.checked()), ["b"]);
        click(cx, "plugin-prompt-ok");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["b".to_owned()])]);

        let (view, answers, cx) =
            open(&mut cx.cx, |window, cx| PluginPrompt::input("Name", Some("Main"), false, window, cx));
        assert_eq!(view.read_with(cx, |view, cx| view.text(cx)), Some("Main".to_owned()));
        click(cx, "plugin-prompt-cancel");
        assert_eq!(answers.borrow().as_slice(), [None]);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › plugin prompts
    #[gpui_kit::test]
    async fn asks_multi_line_text_and_confirms_on_ctrl_enter(cx: &mut TestAppContext) {
        init(cx);
        let (view, answers, cx) = open(cx, |window, cx| PluginPrompt::input("Notes", Some("one"), true, window, cx));
        assert!(view.read_with(cx, |view, _| view.is_multiline()));
        let area = view.read_with(cx, |view, _| view.area().cloned()).expect("a multi-line field");
        cx.update(|window, cx| window.focus(&area.focus_handle(cx), cx));
        area.update(cx, |area, cx| area.set_text("", cx));
        cx.simulate_input("a");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("b");
        assert!(answers.borrow().is_empty(), "Enter adds a line");
        cx.simulate_keystrokes("ctrl-enter");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["a\nb".to_owned()])]);
    }

    // src/features/plugins/__tests__/plugins.test.tsx › plugin prompts
    #[gpui_kit::test]
    async fn returns_the_typed_text_untrimmed(cx: &mut TestAppContext) {
        init(cx);
        let (_view, answers, cx) =
            open(cx, |window, cx| PluginPrompt::input("Name", Some("  Main  "), false, window, cx));
        click(cx, "plugin-prompt-ok");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["  Main  ".to_owned()])]);
    }

    /// A blank text is an answer too: the plugin decides what it means.
    #[gpui_kit::test]
    async fn an_empty_text_is_answered_with_enter(cx: &mut TestAppContext) {
        init(cx);
        let (view, answers, cx) = open(cx, |window, cx| PluginPrompt::input("Name", None, false, window, cx));
        let field = view.read_with(cx, |view, _| view.field().cloned()).expect("a one-line field");
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_keystrokes("enter");
        assert_eq!(answers.borrow().as_slice(), [Some(vec![String::new()])]);
    }

    /// The keyboard of a single pick: the arrows, Home and End move without wrapping, Enter picks
    /// the active row, Escape dismisses.
    #[gpui_kit::test]
    async fn a_single_pick_moves_with_the_keys_and_enter_picks_the_active_row(cx: &mut TestAppContext) {
        init(cx);
        let items: Vec<PickItem> = (0..12).map(|i| item(&format!("k{i}"), &format!("Item {i}"), None)).collect();
        let (view, answers, cx) = open(cx, move |_, cx| PluginPrompt::pick("Pick", items, false, cx));
        let active = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.active().map(str::to_owned));
        assert_eq!(active(cx).as_deref(), Some("k0"));
        cx.simulate_keystrokes("up");
        assert_eq!(active(cx).as_deref(), Some("k0"), "no wrap at the top");
        cx.simulate_keystrokes("down down");
        assert_eq!(active(cx).as_deref(), Some("k2"));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(active(cx).as_deref(), Some("k11"));
        cx.simulate_keystrokes("home");
        assert_eq!(active(cx).as_deref(), Some("k0"));
        cx.simulate_keystrokes("end up enter");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["k10".to_owned()])]);
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().len(), 1, "a prompt answers once");
    }

    /// Enter ticks the active row of a multiple pick; OK answers the ticked keys in list order,
    /// with nothing ticked an empty list; Escape dismisses.
    #[gpui_kit::test]
    async fn enter_ticks_rows_of_a_multiple_pick_and_ok_returns_them_in_list_order(cx: &mut TestAppContext) {
        init(cx);
        let items = vec![item("a", "Alpha", None), item("b", "Beta", None), item("c", "Gamma", None)];
        let (view, answers, cx) = open(cx, move |_, cx| PluginPrompt::pick("Pick", items, true, cx));
        cx.simulate_keystrokes("end enter home enter");
        assert_eq!(view.read_with(cx, |view, _| view.checked()), ["a", "c"]);
        cx.simulate_keystrokes("enter");
        assert_eq!(view.read_with(cx, |view, _| view.checked()), ["c"], "Enter unticks a ticked row");
        click(cx, "plugin-prompt-ok");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["c".to_owned()])]);

        let (_view, answers, cx) = open(&mut cx.cx, |_, cx| PluginPrompt::pick("Pick", targets(), true, cx));
        click(cx, "plugin-prompt-ok");
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().as_slice(), [Some(Vec::new())]);
    }

    /// A pick without items cannot be confirmed; a confirmation answers an empty list on Enter
    /// and is dismissed with Escape.
    #[gpui_kit::test]
    async fn an_empty_pick_cannot_be_confirmed_and_a_confirmation_answers_on_enter(cx: &mut TestAppContext) {
        init(cx);
        let (view, answers, cx) = open(cx, |_, cx| PluginPrompt::pick("Pick", Vec::new(), false, cx));
        assert!(!view.read_with(cx, |view, _| view.can_confirm()));
        cx.simulate_keystrokes("enter");
        assert!(answers.borrow().is_empty());
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().as_slice(), [None]);

        let (view, answers, cx) = open(&mut cx.cx, |_, cx| PluginPrompt::confirm_question("Delete", "Really?", cx));
        assert_eq!(view.read_with(cx, |view, _| view.message().map(str::to_owned)).as_deref(), Some("Really?"));
        cx.simulate_keystrokes("enter");
        assert_eq!(answers.borrow().as_slice(), [Some(Vec::new())]);
    }

    /// The prompt's keys are its own only while it has the keyboard: with the focus elsewhere in the
    /// window, Enter, Escape and the arrows change nothing; given the keyboard back, Enter answers.
    #[gpui_kit::test]
    async fn the_prompt_keys_do_nothing_while_the_prompt_has_no_keyboard(cx: &mut TestAppContext) {
        init(cx);
        let (view, answers, cx) = open(cx, |_, cx| PluginPrompt::pick("Pick", targets(), false, cx));
        cx.update(|window, cx| {
            let elsewhere = cx.focus_handle();
            window.focus(&elsewhere, cx);
        });
        cx.simulate_keystrokes("down enter escape");
        assert!(answers.borrow().is_empty(), "no answer without the keyboard");
        assert_eq!(view.read_with(cx, |view, _| view.active().map(str::to_owned)).as_deref(), Some("a"));
        cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
        cx.simulate_keystrokes("enter");
        assert_eq!(answers.borrow().as_slice(), [Some(vec!["a".to_owned()])]);
    }
}
