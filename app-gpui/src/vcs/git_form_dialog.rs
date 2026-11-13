//! The dialog that shows a form of [`crate::vcs::git_forms`]: Pull, Update Project, Merge, Rebase,
//! Git Reset, Tag, Stash, Unstash Changes, Rollback Changes, Git Remotes, Define / Edit Remote,
//! Worktrees and New Worktree.
//!
//! The form says what to draw ([`FormSpec`]); the dialog draws it, owns the text fields, feeds every
//! edit back to the form and draws it again. A select opens its options below itself. A button
//! that can be pressed emits the form's answer; the dialog stays open until the workbench closes
//! it, so a failure can be shown in it. Enter presses the default button when it can be pressed,
//! Escape closes an open select, else cancels.

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip;
use crate::vcs::git_forms::{
    ButtonPlace, CANCEL, Choice, FormAnswer, FormButton, FormInput, FormRow, FormSpec, GitForm, ListRow, can_press,
};
use crate::vcs::prompt_dialogs::{button, checkbox, code_font, field_frame};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    Keystroke, MouseButton, Render, SharedString, Subscription, Window, deferred, div, prelude::*, px,
};

const LIST_HEIGHT: f32 = 220.0;
const OPTIONS_HEIGHT: f32 = 240.0;

/// What the dialog asks of the workbench.
#[derive(Clone)]
pub enum GitFormEvent {
    /// A button was pressed: carry out its answer, then close the dialog or show a problem in it.
    Answer(FormAnswer),
    /// Cancel, Escape: close the dialog.
    Cancelled,
}

/// An answer may hold a remote's URL with its credentials: only the kind of event is shown.
impl std::fmt::Debug for GitFormEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Answer(_) => "Answer(..)",
            Self::Cancelled => "Cancelled",
        })
    }
}

/// A text field of the form, made for its row.
struct Field {
    id: &'static str,
    entity: Entity<TextField>,
    /// The placeholder it was made with.
    placeholder: String,
    key_claim: u64,
    _events: Subscription,
}

pub struct GitFormDialog {
    form: Box<dyn GitForm>,
    fields: Vec<Field>,
    /// The select whose options are shown.
    open_select: Option<&'static str>,
    focus: FocusHandle,
    /// The dialog had no text field when it opened: it takes the keyboard itself.
    own_focus: bool,
    code_font: SharedString,
    key_claim: u64,
}

impl EventEmitter<GitFormEvent> for GitFormDialog {}

fn text_rows(spec: &FormSpec) -> Vec<(&'static str, String, String)> {
    spec.rows
        .iter()
        .filter_map(|row| match row {
            FormRow::Text { id, value, placeholder, .. } => Some((*id, value.clone(), placeholder.clone())),
            _ => None,
        })
        .collect()
}

impl GitFormDialog {
    /// The dialog over `form`, with the keyboard in its first text field.
    pub fn new(form: Box<dyn GitForm>, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self {
            form,
            fields: Vec::new(),
            open_select: None,
            focus: cx.focus_handle(),
            own_focus: true,
            code_font: code_font(cx),
            key_claim: 0,
        };
        dialog.sync_fields(cx);
        dialog.own_focus = dialog.fields.is_empty();
        if let Some(first) = dialog.fields.first() {
            dialog.focus = first.entity.focus_handle(cx);
        } else {
            dialog.key_claim = dialog.claim(dialog.focus.clone(), cx);
        }
        cx.on_release(|dialog, cx| {
            input::release_keys(dialog.key_claim, cx);
            for field in &dialog.fields {
                input::release_keys(field.key_claim, cx);
            }
        })
        .detach();
        dialog
    }

    /// Enter and Escape are the dialog's own, ahead of the keymap, while `focus` has the keyboard.
    fn claim(&self, focus: FocusHandle, cx: &mut Context<Self>) -> u64 {
        let claimant = cx.weak_entity();
        input::claim_keys(
            focus,
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        )
    }

    /// What the form shows now, in the UI language.
    pub fn spec(&self, cx: &mut App) -> FormSpec {
        self.form.spec(&mut cx.default_global::<Localization>().0)
    }

    pub fn title(&self, cx: &mut App) -> String {
        self.spec(cx).title
    }

    /// The text field of row `id`, while the form shows it.
    pub fn field(&self, id: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|field| field.id == id).map(|field| &field.entity)
    }

    /// The select whose options are shown.
    pub fn open_select(&self) -> Option<&'static str> {
        self.open_select
    }

    /// Makes the fields the form shows, drops those it no longer shows and puts the form's text in
    /// a field that shows another (a branch name cleaned up as it is typed, a suggested folder).
    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        let rows = text_rows(&self.spec(cx));
        let shown: Vec<&'static str> = rows.iter().map(|(id, ..)| *id).collect();
        let gone: Vec<u64> =
            self.fields.iter().filter(|field| !shown.contains(&field.id)).map(|field| field.key_claim).collect();
        for claim in gone {
            input::release_keys(claim, cx);
        }
        self.fields.retain(|field| shown.contains(&field.id));
        for (id, value, placeholder) in rows {
            let at = self.fields.iter().position(|field| field.id == id);
            // A field's placeholder is set when it is made: an empty one is made again for another.
            let stale = at.is_some_and(|at| {
                let field = &self.fields[at];
                field.placeholder != placeholder && field.entity.read(cx).text().is_empty()
            });
            match at {
                Some(at) if !stale => {
                    let entity = self.fields[at].entity.clone();
                    if entity.read(cx).text() != value {
                        entity.update(cx, |field, cx| field.set_text(&value, cx));
                    }
                }
                _ => {
                    if let Some(at) = at {
                        let old = self.fields.remove(at);
                        input::release_keys(old.key_claim, cx);
                    }
                    let field = self.make_field(id, &value, &placeholder, cx);
                    self.fields.push(field);
                }
            }
        }
        // The fields keep the order of their rows.
        let order = |id: &'static str| shown.iter().position(|shown| *shown == id).unwrap_or(usize::MAX);
        self.fields.sort_by_key(|field| order(field.id));
    }

    fn make_field(&mut self, id: &'static str, value: &str, placeholder: &str, cx: &mut Context<Self>) -> Field {
        let shown = placeholder.to_owned();
        let entity = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(shown);
            field.set_text(value, cx);
            field
        });
        let events = cx.subscribe(&entity, move |dialog: &mut Self, entity, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = entity.read(cx).text().to_owned();
                dialog.edit(id, FormInput::Text(text), cx);
            }
            TextFieldEvent::Submitted => dialog.press_default(cx),
            TextFieldEvent::Cancelled => cx.emit(GitFormEvent::Cancelled),
        });
        let key_claim = self.claim(entity.focus_handle(cx), cx);
        Field { id, entity, placeholder: placeholder.to_owned(), key_claim, _events: events }
    }

    /// Control `id` was edited: the form takes it and is drawn again.
    pub fn edit(&mut self, id: &str, input: FormInput, cx: &mut Context<Self>) {
        self.form.edit(id, input);
        self.sync_fields(cx);
        cx.notify();
    }

    /// Types `text` into the field of row `id`, as the user would.
    pub fn set_text(&mut self, id: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(field) = self.field(id).cloned() {
            field.update(cx, |field, cx| field.set_text(text, cx));
        }
        self.edit(id, FormInput::Text(text.to_owned()), cx);
    }

    /// Chooses `value` in the select, radio group or list `id`.
    pub fn choose(&mut self, id: &str, value: &str, cx: &mut Context<Self>) {
        self.open_select = None;
        self.edit(id, FormInput::Choose(value.to_owned()), cx);
    }

    /// Checks or unchecks the checkbox `id`, when it can be changed.
    pub fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        let rows = self.spec(cx).rows;
        let enabled = rows.iter().any(|row| matches!(row, FormRow::Check { id: own, enabled: true, .. } if *own == id));
        if enabled {
            self.edit(id, FormInput::Toggle, cx);
        }
    }

    /// Checks or unchecks row `key` of the list `id`.
    pub fn toggle_row(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        self.edit(id, FormInput::ToggleRow(key.to_owned()), cx);
    }

    /// A problem the workbench met carrying out the answer, shown in the dialog.
    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.form.set_error(error);
        cx.notify();
    }

    /// Presses `button`: Cancel cancels; another one answers when it can be pressed.
    pub fn press(&mut self, button: &str, cx: &mut Context<Self>) {
        self.open_select = None;
        if button == CANCEL {
            cx.emit(GitFormEvent::Cancelled);
            return;
        }
        let spec = self.spec(cx);
        if !can_press(&spec, button) {
            cx.notify();
            return;
        }
        let answer = self.form.press(button, &mut cx.default_global::<Localization>().0);
        if let Some(answer) = answer {
            cx.emit(GitFormEvent::Answer(answer));
        }
        cx.notify();
    }

    /// Enter: the default button, when it can be pressed.
    fn press_default(&mut self, cx: &mut Context<Self>) {
        let spec = self.spec(cx);
        if let Some(id) = spec.buttons.iter().find(|button| button.primary).map(|button| button.id) {
            self.press(id, cx);
        }
    }

    fn toggle_select(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.open_select = if self.open_select == Some(id) { None } else { Some(id) };
        cx.notify();
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.press_default(cx),
            "escape" if self.open_select.is_some() => {
                self.open_select = None;
                cx.notify();
            }
            "escape" => cx.emit(GitFormEvent::Cancelled),
            _ => return false,
        }
        true
    }

    fn render_select(
        &self,
        id: &'static str,
        value: Option<&str>,
        placeholder: &str,
        options: &[Choice],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chosen = value.and_then(|value| options.iter().find(|option| option.value == value));
        let (text, color) = match chosen {
            Some(option) => (option.label.clone(), theme::color(cx, "fg-strong")),
            None => (placeholder.to_owned(), theme::color(cx, "fg-faint")),
        };
        let list = (self.open_select == Some(id)).then(|| {
            let rows: Vec<_> = options
                .iter()
                .enumerate()
                .map(|(index, option)| {
                    let picked = option.value.clone();
                    let detail = option
                        .detail
                        .clone()
                        .map(|detail| div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(detail)));
                    div()
                        .id(SharedString::from(format!("form-option-{id}-{index}")))
                        .debug_selector(|| format!("form-option-{id}-{index}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .when(Some(option.value.as_str()) == value, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(option.label.clone()))
                        .children(detail)
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.choose(id, &picked, cx)))
                })
                .collect();
            deferred(
                div()
                    .id(SharedString::from(format!("form-options-{id}")))
                    .absolute()
                    .top(px(30.0))
                    .left(px(0.0))
                    .min_w(px(220.0))
                    .max_h(px(OPTIONS_HEIGHT))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(2)
        });
        field_frame(cx, false)
            .id(SharedString::from(format!("form-select-{id}")))
            .debug_selector(|| format!("form-select-{id}"))
            .relative()
            .gap(px(6.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(color)
                    .child(SharedString::from(text)),
            )
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle_select(id, cx)))
            .children(list)
            .into_any_element()
    }

    fn render_radio(&self, id: &'static str, value: &str, options: &[Choice], cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<_> = options
            .iter()
            .map(|option| {
                let on = option.value == value;
                let picked = option.value.clone();
                let dot = on.then(|| div().size(px(8.0)).rounded_full().bg(theme::color(cx, "bg-button-primary")));
                let hint = option.detail.clone().map(|hint| {
                    let small = theme::length(cx, "fs-small");
                    div().text_color(theme::color(cx, "fg-muted")).text_size(small).child(SharedString::from(hint))
                });
                div()
                    .id(SharedString::from(format!("form-radio-{id}-{}", option.value)))
                    .debug_selector(|| format!("form-radio-{id}-{}", option.value))
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_none()
                            .mt(px(1.0))
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .border_1()
                            .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                            .bg(theme::color(cx, "bg-checkbox"))
                            .children(dot),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .text_color(theme::color(cx, "fg-strong"))
                            .child(SharedString::from(option.label.clone()))
                            .children(hint),
                    )
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.choose(id, &picked, cx)))
            })
            .collect();
        div().flex().flex_col().gap(px(8.0)).children(rows).into_any_element()
    }

    fn render_list(
        &self,
        id: &'static str,
        rows: &[ListRow],
        selected: Option<&str>,
        empty: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        let body: Vec<AnyElement> = if rows.is_empty() {
            vec![div().p(px(8.0)).text_color(muted).child(SharedString::from(empty.to_owned())).into_any_element()]
        } else {
            rows.iter()
                .enumerate()
                .map(|(index, row)| {
                    let key = row.key.clone();
                    let check = row.checked.map(|on| {
                        let toggled = row.key.clone();
                        checkbox(cx, String::new(), on)
                            .id(SharedString::from(format!("form-check-{id}-{index}")))
                            .debug_selector(|| format!("form-check-{id}-{index}"))
                            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                dialog.toggle_row(id, &toggled, cx);
                            }))
                    });
                    div()
                        .id(SharedString::from(format!("form-row-{id}-{index}")))
                        .debug_selector(|| format!("form-row-{id}-{index}"))
                        .h(theme::length(cx, "h-list-row"))
                        .pl(px(8.0 + 16.0 * row.depth as f32))
                        .pr(px(8.0))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(8.0))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .when(selected == Some(row.key.as_str()), |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .children(check)
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme::color(cx, "fg-strong"))
                                .child(SharedString::from(row.label.clone())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(muted)
                                .font_family(self.code_font.clone())
                                .child(SharedString::from(row.detail.clone())),
                        )
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.choose(id, &key, cx)))
                        .into_any_element()
                })
                .collect()
        };
        div()
            .id(SharedString::from(format!("form-list-{id}")))
            .max_h(px(LIST_HEIGHT))
            .min_h(px(72.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .py(px(2.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-input"))
            .children(body)
            .into_any_element()
    }

    fn render_row(&self, row: &FormRow, cx: &mut Context<Self>) -> AnyElement {
        let muted = theme::color(cx, "fg-muted");
        match row {
            FormRow::Hint(text) => div()
                .flex()
                .flex_col()
                .text_color(muted)
                .children(text.lines().map(|line| div().min_h(px(16.0)).child(SharedString::from(line.to_owned()))))
                .into_any_element(),
            FormRow::Label(text) => div().text_color(muted).child(SharedString::from(text.clone())).into_any_element(),
            FormRow::Text { id, mono, invalid, .. } => {
                let entity = self.field(id).cloned();
                field_frame(cx, *invalid)
                    .when(*mono, |el| el.font_family(self.code_font.clone()))
                    .child(div().flex_1().min_w_0().children(entity))
                    .into_any_element()
            }
            FormRow::Select { id, value, placeholder, options } => {
                self.render_select(id, value.as_deref(), placeholder, options, cx)
            }
            FormRow::Check { id, label, on, enabled } => {
                let id = *id;
                let check = checkbox(cx, label.clone(), *on)
                    .id(SharedString::from(format!("form-toggle-{id}")))
                    .debug_selector(|| format!("form-toggle-{id}"));
                if *enabled {
                    let toggle = cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle(id, cx));
                    check.on_click(toggle).into_any_element()
                } else {
                    check.opacity(0.5).into_any_element()
                }
            }
            FormRow::Radio { id, value, options } => self.render_radio(id, value, options, cx),
            FormRow::List { id, rows, selected, empty } => self.render_list(id, rows, selected.as_deref(), empty, cx),
            FormRow::Error(text) => div()
                .text_color(theme::color(cx, "danger"))
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(text.clone()))
                .into_any_element(),
        }
    }

    fn render_button(&self, spec: &FormButton, cx: &mut Context<Self>) -> AnyElement {
        let id = spec.id;
        let face = button(cx, spec.label.clone(), spec.primary, false, spec.enabled)
            .id(SharedString::from(format!("form-button-{id}")))
            .debug_selector(|| format!("form-button-{id}"))
            .with_tooltip(spec.tooltip.clone().unwrap_or_default());
        if spec.enabled {
            face.on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx))).into_any_element()
        } else {
            face.into_any_element()
        }
    }
}

impl Focusable for GitFormDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GitFormDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let spec = self.spec(cx);
        let rows: Vec<AnyElement> = spec.rows.iter().map(|row| self.render_row(row, cx)).collect();
        let placed = |place: ButtonPlace| spec.buttons.iter().filter(move |button| button.place == place);
        let toolbar: Vec<AnyElement> =
            placed(ButtonPlace::Toolbar).map(|button| self.render_button(button, cx)).collect();
        let start: Vec<AnyElement> =
            placed(ButtonPlace::FooterStart).map(|button| self.render_button(button, cx)).collect();
        let end: Vec<AnyElement> = placed(ButtonPlace::Footer).map(|button| self.render_button(button, cx)).collect();
        let toolbar = (!toolbar.is_empty()).then(|| div().flex().items_center().gap(px(6.0)).children(toolbar));
        let focus = self.focus.clone();
        div()
            .w(px(spec.width))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .when(self.own_focus, |el| el.track_focus(&focus))
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(spec.title.clone())),
            )
            .child(div().px(px(16.0)).py(px(4.0)).flex().flex_col().gap(px(8.0)).children(toolbar).children(rows))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .children(start)
                    .child(div().flex_1())
                    .children(end),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::git_forms::{OK, RemoteForm, RemotesCommand, RemotesForm, StashForm, UpdateForm};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_vcs::{Remote, RemoteEdit};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<GitFormEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        form: impl FnOnce() -> Box<dyn GitForm> + 'static,
    ) -> (Entity<GitFormDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|_, cx| GitFormDialog::new(form(), cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &GitFormEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn enabled(dialog: &Entity<GitFormDialog>, button: &str, cx: &mut VisualTestContext) -> bool {
        dialog.update(cx, |dialog, cx| can_press(&dialog.spec(cx), button))
    }

    fn remote(name: &str, url: &str) -> Remote {
        Remote { name: name.to_owned(), url: url.to_owned() }
    }

    // Not from a spec: typed text reaches the form, OK can only be pressed once the form allows it,
    // Enter presses it and the answer carries what was typed; Escape cancels.
    #[gpui_kit::test]
    async fn typing_reaches_the_form_and_enter_answers(cx: &mut TestAppContext) {
        let names = ["origin".to_owned()];
        let (dialog, events, cx) = open(cx, move || Box::new(RemoteForm::new(None, &names)));
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.title(cx)), "Define Remote");
        let name = dialog.read_with(cx, |dialog, _| dialog.field("name").cloned()).expect("the name field");
        assert_eq!(name.read_with(cx, |field, _| field.text().to_owned()), "", "origin is taken");
        assert!(!enabled(&dialog, OK, cx));
        cx.simulate_input("fork");
        let url = dialog.read_with(cx, |dialog, _| dialog.field("url").cloned()).expect("the URL field");
        cx.update(|window, cx| window.focus(&url.focus_handle(cx), cx));
        cx.simulate_input("/srv/fork.git");
        assert!(enabled(&dialog, OK, cx));
        cx.simulate_keystrokes("enter");
        let answer = events.borrow().last().cloned();
        let Some(GitFormEvent::Answer(FormAnswer::EditRemote(RemoteEdit::Add { name, url }))) = answer else {
            panic!("OK answers with the remote to add: {answer:?}");
        };
        assert_eq!((name.as_str(), url.as_str()), ("fork", "/srv/fork.git"));
        cx.simulate_keystrokes("escape");
        assert!(matches!(events.borrow().last(), Some(GitFormEvent::Cancelled)));
    }

    // Not from a spec: checkboxes and radio options answer the mouse, a disabled button does not.
    #[gpui_kit::test]
    async fn checkboxes_and_radio_options_answer_the_mouse(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, || Box::new(UpdateForm::new(None, true, false)));
        click(cx, "form-radio-type-rebase");
        click(cx, "form-toggle-hide");
        click(cx, "form-button-ok");
        let answer = events.borrow().last().cloned();
        assert!(matches!(answer, Some(GitFormEvent::Answer(FormAnswer::Update { rebase: true, hide: true }))));
        click(cx, "form-button-resetToRemote");
        assert!(matches!(events.borrow().last(), Some(GitFormEvent::Answer(FormAnswer::ResetToRemote))));
        click(cx, "form-button-cancel");
        assert!(matches!(events.borrow().last(), Some(GitFormEvent::Cancelled)));
        assert!(dialog.read_with(cx, |dialog, _| dialog.open_select().is_none()));

        let (dialog_b, events_b, cx) = open(cx, || Box::new(RemotesForm::new(Vec::new(), None, None)));
        assert!(!enabled(&dialog_b, "remove", cx), "nothing to remove");
        click(cx, "form-button-remove");
        assert!(events_b.borrow().is_empty(), "a disabled button answers nothing");
        click(cx, "form-button-add");
        let last = events_b.borrow().last().cloned();
        let added = matches!(last, Some(GitFormEvent::Answer(FormAnswer::Remotes(RemotesCommand::Add))));
        assert!(added, "Add Remote… asks for a remote to add");
    }

    // Not from a spec: a select shows its options below it, a click chooses one and closes them,
    // and a list's row is selected by a click.
    #[gpui_kit::test]
    async fn a_select_and_a_list_choose_with_the_mouse(cx: &mut TestAppContext) {
        let remotes = vec![remote("origin", "/o.git"), remote("fork", "/f.git")];
        let (dialog, events, cx) = open(cx, move || Box::new(RemotesForm::new(remotes, None, None)));
        click(cx, "form-row-remotes-1");
        click(cx, "form-button-edit");
        let answer = events.borrow().last().cloned();
        let Some(GitFormEvent::Answer(FormAnswer::Remotes(RemotesCommand::Edit(chosen)))) = answer else {
            panic!("Edit answers with the selected remote: {answer:?}");
        };
        assert_eq!(chosen.name, "fork");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.open_select()), None);

        let info: ide_vcs::BranchInfo = serde_json::from_value(serde_json::json!({
            "head": "main", "headCommit": "c1", "upstream": null, "ahead": 0, "behind": 0,
        }))
        .expect("branch info in its wire form");
        let remotes = vec![remote("origin", "/o.git"), remote("fork", "/f.git")];
        let (dialog, _, cx) =
            open(cx, move || Box::new(crate::vcs::git_forms::PullForm::new(Some(&info), remotes, Vec::new())));
        click(cx, "form-select-remote");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.open_select()), Some("remote"));
        click(cx, "form-option-remote-1");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.open_select()), None);
        let shown = dialog.update(cx, |dialog, cx| {
            dialog.spec(cx).rows.iter().find_map(|row| match row {
                FormRow::Select { id: "remote", value, .. } => value.clone(),
                _ => None,
            })
        });
        assert_eq!(shown.as_deref(), Some("fork"));
    }

    // Not from a spec: a problem the workbench reports shows in the dialog, which stays open.
    #[gpui_kit::test]
    async fn a_reported_problem_shows_in_the_dialog(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, || Box::new(RemotesForm::new(Vec::new(), None, None)));
        dialog.update(cx, |dialog, cx| dialog.set_error(Some("fatal: no such remote".to_owned()), cx));
        let rows = dialog.update(cx, |dialog, cx| dialog.spec(cx).rows);
        assert!(rows.contains(&FormRow::Error("fatal: no such remote".to_owned())));
        let (dialog, events, cx) = open(cx, || Box::new(StashForm::new(Some("main".to_owned()))));
        dialog.update(cx, |dialog, cx| dialog.set_text("message", "half done", cx));
        dialog.update(cx, |dialog, cx| dialog.toggle("keepIndex", cx));
        cx.simulate_keystrokes("enter");
        let answer = events.borrow().last().cloned();
        let Some(GitFormEvent::Answer(FormAnswer::Stash { message, keep_index, include_untracked })) = answer else {
            panic!("Create Stash answers: {answer:?}");
        };
        assert_eq!((message.as_deref(), keep_index, include_untracked), (Some("half done"), true, false));
    }
}
