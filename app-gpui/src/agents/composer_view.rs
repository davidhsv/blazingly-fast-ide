//! A task's message composer: the message (one line, its line breaks shown as marks: ⇧Enter types one),
//! the referenced files and images as chips that can be removed, the agent's settings as chips that open
//! their pickers (the model with its effort levels, the mode with Allow All, plain lists and switches), and
//! Send, or Queue and Stop while the agent works.
//!
//! [`ComposerView`] keeps the message in the store's composer of the task (so a quote or a reference added
//! elsewhere shows up here) and reports what needs the backend as [`ComposerEvent::Effects`]: the message
//! to send, a setting to change, the turn to stop. The answer of a send comes back through
//! [`ComposerView::call_answered`].

use crate::agents::composer::ComposerDraft;
use crate::agents::config_bar::{
    Chip, ListKey, OptionList, Picker, Values, config_bar, effort_slider, effort_step, mode_chip, model_chip,
    select_icon, switch_choice, value_of,
};
use crate::agents::config_pickers::switch_values;
use crate::agents::message_editor::submit_buttons;
use crate::agents::store::{AgentCall, AgentEffect, AgentsStore, Ticket};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement,
    KeyDownEvent, Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div,
    prelude::*, px,
};
use ide_agent::{AgentConfigOption, AgentTask};

/// How a line break of the message is shown, and typed, in the one-line field.
const LINE_BREAK_MARK: &str = "⏎";

fn marked(text: &str) -> String {
    text.replace("\r\n", "\n").replace(['\n', '\r'], LINE_BREAK_MARK)
}

fn unmarked(text: &str) -> String {
    text.replace(LINE_BREAK_MARK, "\n")
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// What the composer asks of its window.
#[derive(Clone, Debug)]
pub enum ComposerEvent {
    /// Carry out these effects; the answer of a send goes to [`AgentsStore::call_answered`] and then to
    /// [`ComposerView::call_answered`].
    Effects(Vec<AgentEffect>),
}

/// A chip of the settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfigChip {
    /// The option the chip opens (the model for the model and effort chip, the mode for the mode chip).
    option: String,
    label: String,
    detail: Option<String>,
    icon: Option<&'static str>,
    /// A switch flips on a click instead of opening a list.
    switch: Option<bool>,
}

impl ConfigChip {
    fn of(option: &str, chip: Chip) -> Self {
        Self { option: option.to_owned(), label: chip.value, detail: chip.detail, icon: chip.icon, switch: None }
    }
}

/// The open picker: the list of its option, and the extra row of a combined chip.
struct OpenPicker {
    chip: usize,
    list: OptionList,
    /// The effort option shown as level steps under the model list.
    effort: Option<String>,
    /// The Allow All switch under the mode list.
    allow_all: Option<String>,
}

fn find<'a>(config: &'a [AgentConfigOption], id: &str) -> Option<&'a AgentConfigOption> {
    config.iter().find(|o| o.id == id)
}

fn chips(config: &[AgentConfigOption], values: &Values, i18n: &I18n) -> Vec<ConfigChip> {
    let name_of = |option: &AgentConfigOption| {
        let value = value_of(option, values);
        option
            .values
            .iter()
            .find(|v| Some(v.value.as_str()) == value)
            .map_or_else(|| i18n.tx("Default"), |v| v.name.clone())
    };
    config_bar(config)
        .inline
        .into_iter()
        .map(|picker| match picker {
            Picker::ModelEffort { model, effort } => {
                let chip = model_chip(model, effort, values, i18n);
                ConfigChip::of(&model.id, chip)
            }
            Picker::ModeAllowAll { mode, allow_all } => {
                let chip = mode_chip(mode, allow_all, values, i18n);
                ConfigChip::of(&mode.id, chip)
            }
            Picker::Switch(option) => {
                let on = switch_values(option).is_some_and(|(on, _)| value_of(option, values) == Some(on));
                ConfigChip {
                    option: option.id.clone(),
                    label: i18n.tx(&option.name),
                    detail: None,
                    icon: None,
                    switch: Some(on),
                }
            }
            Picker::Select(option) => ConfigChip {
                option: option.id.clone(),
                label: name_of(option),
                detail: None,
                icon: value_of(option, values).and_then(|v| select_icon(option, v)),
                switch: None,
            },
        })
        .collect()
}

pub struct ComposerView {
    store: Entity<AgentsStore>,
    task: String,
    field: Entity<TextField>,
    /// The send on its way, by the ticket of its call.
    sending: Option<Ticket>,
    picker: Option<OpenPicker>,
    picker_focus: FocusHandle,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for ComposerView {}

impl ComposerView {
    /// The composer of `task`'s conversation.
    pub fn new(store: Entity<AgentsStore>, task: &str, cx: &mut Context<Self>) -> Self {
        let (text, placeholder) = {
            let store = store.read(cx);
            let draft = store.composer(task);
            let english = I18n::default();
            let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
            let placeholder = store.task(task).map(|t| submit_buttons(t, &draft, false, i18n).placeholder);
            (marked(&draft.text), placeholder.unwrap_or_default())
        };
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            field.set_text(&text, cx);
            field
        });
        let subscriptions = vec![
            cx.observe(&store, |view, _, cx| {
                view.sync_field(cx);
                cx.notify();
            }),
            cx.subscribe(&field, |view, field, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    let text = unmarked(field.read(cx).text());
                    let task = view.task.clone();
                    view.store.update(cx, |store, cx| {
                        store.update_composer(&task, |draft| draft.text = text);
                        cx.notify();
                    });
                }
            }),
        ];
        let weak = cx.weak_entity();
        let claim = input::claim_keys(
            field.read(cx).focus_handle(cx),
            move |keystroke, window, cx| {
                weak.update(cx, |view, cx| view.field_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view: &mut Self, cx| input::release_keys(view.claim, cx)).detach();
        Self {
            store,
            task: task.to_owned(),
            field,
            sending: None,
            picker: None,
            picker_focus: cx.focus_handle(),
            claim,
            _subscriptions: subscriptions,
        }
    }

    /// The message field, which takes the keyboard when the conversation asks the composer to.
    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The message as the store holds it.
    pub fn draft(&self, cx: &App) -> ComposerDraft {
        self.store.read(cx).composer(&self.task)
    }

    /// The store's message changed elsewhere (a quote, a reference, a sent message cleared): the field
    /// shows it.
    fn sync_field(&mut self, cx: &mut Context<Self>) {
        let text = self.store.read(cx).composer(&self.task).text;
        if unmarked(self.field.read(cx).text()) != text {
            let shown = marked(&text);
            self.field.update(cx, |field, cx| field.set_text(&shown, cx));
        }
    }

    /// The answer of a call: the end of a send this composer started.
    pub fn call_answered(&mut self, ticket: Ticket, cx: &mut Context<Self>) {
        if self.sending == Some(ticket) {
            self.sending = None;
            cx.notify();
        }
    }

    fn task_of<'a>(&self, store: &'a AgentsStore) -> Option<&'a AgentTask> {
        store.task(&self.task)
    }

    /// The labels of the submit row: `Stop` while the agent works, then `Send` or `Queue`, with
    /// ` (disabled)` when it cannot be pressed.
    pub fn buttons(&self, cx: &App) -> Vec<String> {
        let store = self.store.read(cx);
        let Some(task) = self.task_of(store) else { return Vec::new() };
        let english = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
        let buttons = submit_buttons(task, &store.composer(&self.task), self.sending.is_some(), i18n);
        let mut out = Vec::new();
        if buttons.stop {
            out.push(i18n.tx("Stop"));
        }
        out.push(if buttons.enabled { buttons.label } else { format!("{} (disabled)", buttons.label) });
        out
    }

    /// The settings chips, as `<label>` with ` · <detail>`, ` [on]` / ` [off]` for a switch.
    pub fn chips(&self, cx: &App) -> Vec<String> {
        self.config_chips(cx)
            .into_iter()
            .map(|chip| {
                let mut text = chip.label;
                if let Some(detail) = chip.detail {
                    text.push_str(&format!(" · {detail}"));
                }
                if let Some(on) = chip.switch {
                    text.push_str(if on { " [on]" } else { " [off]" });
                }
                text
            })
            .collect()
    }

    /// The entries of the open picker, as `<label>` with ` *` on the highlighted one.
    pub fn picker_entries(&self) -> Option<Vec<String>> {
        let picker = self.picker.as_ref()?;
        let active = picker.list.active();
        Some(
            picker
                .list
                .filtered()
                .iter()
                .enumerate()
                .map(|(i, entry)| if active == Some(i) { format!("{} *", entry.label) } else { entry.label.clone() })
                .collect(),
        )
    }

    fn config_chips(&self, cx: &App) -> Vec<ConfigChip> {
        let store = self.store.read(cx);
        let Some(task) = self.task_of(store) else { return Vec::new() };
        let english = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
        chips(&task.config, &Values::new(), i18n)
    }

    // ----- what keys and clicks do -----

    /// The keys of the message field, ahead of the field: ⇧Enter types a line break, Enter sends (⌘/Ctrl+Enter
    /// sends a queued message now while the agent works), and the open picker's keys.
    fn field_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let command = modifiers.platform || modifiers.control;
        match keystroke.key.as_str() {
            "enter" if modifiers.shift && !command => {
                self.field.update(cx, |field, cx| field.replace_text_in_range(None, LINE_BREAK_MARK, window, cx));
            }
            "enter" => self.send(command, cx),
            "escape" if self.picker.is_some() => self.close_picker(cx),
            _ => return false,
        }
        true
    }

    /// Send (or Queue while the agent works); `now` sends a queued message at once.
    pub fn send(&mut self, now: bool, cx: &mut Context<Self>) {
        if self.sending.is_some() {
            return;
        }
        let task = self.task.clone();
        let effect = self.store.update(cx, |store, _| {
            let open = store.task(&task).is_some_and(|t| t.status != ide_agent::AgentStatus::Archived);
            open.then(|| store.send_message(&task, now)).flatten()
        });
        if let Some(effect) = effect {
            if let AgentEffect::Call { ticket, .. } = &effect {
                self.sending = *ticket;
            }
            cx.emit(ComposerEvent::Effects(vec![effect]));
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let call = AgentCall::Cancel { task: self.task.clone(), subagent: None };
        cx.emit(ComposerEvent::Effects(vec![AgentEffect::Call { ticket: None, call }]));
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let task = self.task.clone();
        self.store.update(cx, |store, cx| {
            store.update_composer(&task, |draft| {
                if index < draft.files.len() {
                    draft.files.remove(index);
                }
            });
            cx.notify();
        });
    }

    fn remove_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let task = self.task.clone();
        self.store.update(cx, |store, cx| {
            store.update_composer(&task, |draft| {
                if index < draft.images.len() {
                    draft.images.remove(index);
                }
            });
            cx.notify();
        });
    }

    fn set_config(&mut self, option: String, value: String, cx: &mut Context<Self>) {
        let call = AgentCall::SetConfig { task: self.task.clone(), option, value };
        cx.emit(ComposerEvent::Effects(vec![AgentEffect::Call { ticket: None, call }]));
    }

    fn chip_clicked(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.as_ref().is_some_and(|p| p.chip == index) {
            self.close_picker(cx);
            return;
        }
        let Some(chip) = self.config_chips(cx).into_iter().nth(index) else { return };
        let store = self.store.read(cx);
        let Some(task) = self.task_of(store) else { return };
        let values = Values::new();
        let Some(option) = find(&task.config, &chip.option) else { return };
        if let Some(on) = chip.switch {
            let choice = switch_choice(option, !on);
            if let Some((option, value)) = choice {
                self.set_config(option, value, cx);
            }
            return;
        }
        let (mut effort, mut allow_all) = (None, None);
        for picker in config_bar(&task.config).inline {
            match picker {
                Picker::ModelEffort { model, effort: e } if model.id == option.id => effort = Some(e.id.clone()),
                Picker::ModeAllowAll { mode, allow_all: a } if mode.id == option.id => allow_all = Some(a.id.clone()),
                _ => {}
            }
        }
        self.picker = Some(OpenPicker { chip: index, list: OptionList::new(option, &values), effort, allow_all });
        window.focus(&self.picker_focus, cx);
        cx.notify();
    }

    fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }

    fn entry_clicked(&mut self, value: String, cx: &mut Context<Self>) {
        let choice = self.picker.as_ref().and_then(|p| p.list.click(&value));
        if let Some((option, value)) = choice {
            self.picker = None;
            self.set_config(option, value, cx);
        }
        cx.notify();
    }

    fn picker_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else { return };
        let key = match event.keystroke.key.as_str() {
            "up" => ListKey::Up,
            "down" => ListKey::Down,
            "enter" => ListKey::Enter,
            "escape" => {
                self.close_picker(cx);
                cx.stop_propagation();
                return;
            }
            _ => return,
        };
        if let Some((option, value)) = picker.list.key(key) {
            self.picker = None;
            self.set_config(option, value, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    // ----- drawing -----

    fn chip_button(
        &self,
        selector: String,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let id = selector.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector)
            .h(theme::length(cx, "h-control-small"))
            .px(px(6.0))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-control"))
            .text_color(theme::color(cx, "fg-muted"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .on_click(cx.listener(move |view, _, window, cx| on_click(view, window, cx)))
    }

    fn attachments(&self, draft: &ComposerDraft, cx: &mut Context<Self>) -> Option<Div> {
        if draft.files.is_empty() && draft.images.is_empty() {
            return None;
        }
        let muted = theme::color(cx, "fg-muted");
        let mut chips: Vec<AnyElement> = Vec::new();
        for (index, file) in draft.files.iter().enumerate() {
            let remove = self.chip_button(format!("composer-file-{index}-remove"), cx, move |view, _, cx| {
                view.remove_file(index, cx);
            });
            chips.push(
                div()
                    .debug_selector(move || format!("composer-file-{index}"))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .bg(theme::color(cx, "bg-hover"))
                    .child(theme::icon(cx, theme::icons::file_icon_name(&file.rel), 12.0, muted))
                    .child(SharedString::from(file.rel.clone()))
                    .child(remove.child(theme::icon(cx, "close", 12.0, muted)))
                    .into_any_element(),
            );
        }
        for (index, image) in draft.images.iter().enumerate() {
            let remove = self.chip_button(format!("composer-image-{index}-remove"), cx, move |view, _, cx| {
                view.remove_image(index, cx);
            });
            let name = image.name.clone().unwrap_or_else(|| tx(cx, "Image"));
            chips.push(
                div()
                    .debug_selector(move || format!("composer-image-{index}"))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .bg(theme::color(cx, "bg-hover"))
                    .child(theme::icon(cx, "file-image", 12.0, muted))
                    .child(SharedString::from(name))
                    .child(remove.child(theme::icon(cx, "close", 12.0, muted)))
                    .into_any_element(),
            );
        }
        Some(div().flex().flex_wrap().gap(px(4.0)).px(px(8.0)).pt(px(6.0)).children(chips))
    }

    fn picker_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let store = self.store.read(cx);
        let task = self.task_of(store)?;
        let values = Values::new();
        let effort = picker.effort.as_deref().and_then(|id| find(&task.config, id)).cloned();
        let allow_all = picker.allow_all.as_deref().and_then(|id| find(&task.config, id)).cloned();
        let active = picker.list.active();
        let entries: Vec<(String, String, bool, bool)> = picker
            .list
            .filtered()
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.value.clone(), entry.label.clone(), entry.section, active == Some(i)))
            .collect();
        let muted = theme::color(cx, "fg-muted");
        let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len());
        for (index, (value, label, section, highlighted)) in entries.into_iter().enumerate() {
            if section {
                let title = div().px(px(8.0)).pt(px(4.0)).text_color(muted).child(SharedString::from(label));
                rows.push(title.into_any_element());
                continue;
            }
            let selector = format!("composer-picker-{index}");
            let id = selector.clone();
            rows.push(
                div()
                    .id(SharedString::from(id))
                    .debug_selector(move || selector)
                    .px(px(8.0))
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .when(highlighted, |el| el.bg(theme::color(cx, "bg-list-selected")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |view, _, _, cx| view.entry_clicked(value.clone(), cx)))
                    .into_any_element(),
            );
        }
        if let Some(effort) = effort {
            let slider = effort_slider(&effort, &values);
            let mut steps = Vec::with_capacity(effort.values.len());
            for (index, level) in effort.values.iter().enumerate() {
                let choice = effort_step(&effort, &values, index);
                let selector = format!("composer-effort-{index}");
                let pressed = index == slider.index;
                steps.push(
                    self.chip_button(selector, cx, move |view, _, cx| {
                        if let Some((option, value)) = choice.clone() {
                            view.picker = None;
                            view.set_config(option, value, cx);
                        }
                    })
                    .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
                    .child(SharedString::from(level.name.clone())),
                );
            }
            let border = theme::color(cx, "border");
            let levels = div().flex().gap(px(2.0)).px(px(4.0)).pt(px(4.0)).border_t_1().border_color(border);
            rows.push(levels.children(steps).into_any_element());
        }
        if let Some(allow_all) = allow_all {
            let on = switch_values(&allow_all).is_some_and(|(on, _)| value_of(&allow_all, &values) == Some(on));
            let choice = switch_choice(&allow_all, !on);
            let label = format!("{}: {}", tx(cx, &allow_all.name), tx(cx, if on { "On" } else { "Off" }));
            rows.push(
                self.chip_button("composer-allow-all".to_owned(), cx, move |view, _, cx| {
                    if let Some((option, value)) = choice.clone() {
                        view.picker = None;
                        view.set_config(option, value, cx);
                    }
                })
                .child(SharedString::from(label))
                .into_any_element(),
            );
        }
        let popup = div()
            .debug_selector(|| "composer-picker".to_owned())
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(Self::picker_key))
            .absolute()
            .bottom(px(40.0))
            .left(px(8.0))
            .w(px(280.0))
            .max_h(px(320.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-popup"))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        Some(deferred(popup).with_priority(1).into_any_element())
    }
}

impl Focusable for ComposerView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let draft = store.composer(&self.task);
        let task = self.task_of(store).cloned();
        let buttons = task.as_ref().map(|task| {
            let english = I18n::default();
            let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
            submit_buttons(task, &draft, self.sending.is_some(), i18n)
        });
        let muted = theme::color(cx, "fg-muted");
        let mut chips: Vec<AnyElement> = Vec::new();
        for (index, chip) in self.config_chips(cx).into_iter().enumerate() {
            let selector = format!("composer-config-{index}");
            let mut button =
                self.chip_button(selector, cx, move |view, window, cx| view.chip_clicked(index, window, cx));
            if let Some(icon) = chip.icon {
                button = button.child(theme::icon(cx, icon, 13.0, muted));
            }
            button = button.child(SharedString::from(chip.label));
            if let Some(detail) = chip.detail {
                let faint = theme::color(cx, "fg-faint");
                button = button.child(div().text_color(faint).child(SharedString::from(detail)));
            }
            match chip.switch {
                Some(on) => button = button.child(theme::icon(cx, if on { "check" } else { "minus" }, 12.0, muted)),
                None => button = button.child(theme::icon(cx, "chevron-down", 12.0, muted)),
            }
            chips.push(button.into_any_element());
        }
        let mut submit: Vec<AnyElement> = Vec::new();
        if let Some(buttons) = buttons {
            if buttons.stop {
                submit.push(
                    self.chip_button("composer-stop".to_owned(), cx, |view, _, cx| view.stop(cx))
                        .child(theme::icon(cx, "stop", 14.0, theme::color(cx, "danger")))
                        .into_any_element(),
                );
            }
            let (enabled, primary) = (buttons.enabled, buttons.primary);
            let send = div()
                .id("composer-send")
                .debug_selector(|| "composer-send".to_owned())
                .size(px(26.0))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-control"))
                .when(primary && enabled, |el| el.bg(theme::color(cx, "accent")))
                .when(!enabled, |el| el.opacity(0.5))
                .child(theme::icon(
                    cx,
                    if primary { "send" } else { "arrow-up" },
                    14.0,
                    theme::color(cx, if primary && enabled { "fg-on-accent" } else { "fg-muted" }),
                ))
                .when(enabled, |el| el.on_click(cx.listener(|view, _, _, cx| view.send(false, cx))));
            submit.push(send.into_any_element());
        }
        let attachments = self.attachments(&draft, cx);
        div()
            .relative()
            .flex()
            .flex_col()
            .m(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-control"))
            .bg(theme::color(cx, "bg-field"))
            .text_size(theme::length(cx, "fs-ui"))
            .children(attachments)
            .child(div().px(px(8.0)).pt(px(6.0)).h(theme::length(cx, "h-control")).child(self.field.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(4.0))
                    .pb(px(4.0))
                    .child(div().flex().flex_wrap().gap(px(2.0)).children(chips))
                    .child(div().flex().items_center().gap(px(4.0)).children(submit)),
            )
            .children(self.picker_element(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::composer::FileRef;
    use crate::agents::fixtures::{calls, connected_store, local_task, option};
    use gpui_kit::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext};
    use ide_agent::AgentStatus;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ComposerEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        task: AgentTask,
    ) -> (Entity<ComposerView>, Entity<AgentsStore>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            if !cx.has_global::<input::Keymap>() {
                input::init(cx);
            }
        });
        let id = task.id.clone();
        let store = cx.new(|_| connected_store(vec![task]).0);
        let shared = store.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| ComposerView::new(shared, &id, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &ComposerEvent, _| seen.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, store, events, cx)
    }

    fn effect_calls(events: &Events) -> Vec<AgentCall> {
        events.borrow().iter().flat_map(|ComposerEvent::Effects(effects)| calls(effects)).collect()
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn draft_text(view: &Entity<ComposerView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| view.draft(cx).text)
    }

    fn prompts(events: &Events) -> Vec<(String, bool)> {
        effect_calls(events)
            .into_iter()
            .filter_map(|call| match call {
                AgentCall::Prompt { prompt, now, .. } => Some((prompt.text, now)),
                _ => None,
            })
            .collect()
    }

    #[gpui_kit::test]
    async fn keeps_an_accessible_icon_only_send_action_with_click_submission(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, local_task("A"));
        assert_eq!(view.read_with(cx, |view, cx| view.buttons(cx)), ["Send (disabled)"]);
        cx.simulate_input("hello");
        cx.run_until_parked();
        assert_eq!(draft_text(&view, cx), "hello");
        assert_eq!(view.read_with(cx, |view, cx| view.buttons(cx)), ["Send"]);
        click(cx, "composer-send");
        assert_eq!(prompts(&events), [("hello".to_owned(), false)]);
    }

    #[gpui_kit::test]
    async fn keeps_an_accessible_icon_only_send_action_with_enter_submission(cx: &mut TestAppContext) {
        let (_, _, events, cx) = open(cx, local_task("A"));
        cx.simulate_input("hello");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(prompts(&events), [("hello".to_owned(), false)]);
    }

    #[gpui_kit::test]
    async fn types_a_line_break_with_shift_enter(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, local_task("A"));
        cx.simulate_input("one");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("two");
        cx.run_until_parked();
        assert_eq!(draft_text(&view, cx), "one\ntwo");
        assert!(prompts(&events).is_empty());
    }

    #[gpui_kit::test]
    async fn keeps_queue_named_and_shows_icon_only_stop_actions_while_the_agent_works(cx: &mut TestAppContext) {
        let mut task = local_task("A");
        task.status = AgentStatus::Running;
        let (view, _, events, cx) = open(cx, task);
        cx.simulate_input("later");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.buttons(cx)), ["Stop", "Queue"]);
        click(cx, "composer-stop");
        assert_eq!(effect_calls(&events), [AgentCall::Cancel { task: "A".to_owned(), subagent: None }]);
        // ⌘Enter sends the message now instead of queueing it.
        cx.simulate_keystrokes("cmd-enter");
        cx.run_until_parked();
        assert_eq!(prompts(&events), [("later".to_owned(), true)]);
    }

    #[gpui_kit::test]
    async fn shows_a_message_put_in_the_composer_elsewhere(cx: &mut TestAppContext) {
        let (view, store, _, cx) = open(cx, local_task("A"));
        store.update(cx, |store, cx| {
            store.quote_in_composer("A", "Quoted\nreply");
            cx.notify();
        });
        cx.run_until_parked();
        let shown = view.read_with(cx, |view, cx| view.field().read(cx).text().to_owned());
        assert_eq!(unmarked(&shown), draft_text(&view, cx));
        assert!(shown.contains(LINE_BREAK_MARK));
    }

    #[gpui_kit::test]
    async fn lists_referenced_files_as_chips_that_can_be_removed(cx: &mut TestAppContext) {
        let (view, store, _, cx) = open(cx, local_task("A"));
        store.update(cx, |store, cx| {
            store.update_composer("A", |draft| {
                draft.files.push(FileRef { path: "/project/src/a.rs".to_owned(), rel: "src/a.rs".to_owned() });
            });
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("composer-file-0").is_some());
        click(cx, "composer-file-0-remove");
        assert!(view.read_with(cx, |view, cx| view.draft(cx).files.is_empty()));
        assert!(cx.debug_bounds("composer-file-0").is_none());
    }

    #[gpui_kit::test]
    async fn opens_a_setting_chip_and_sends_the_chosen_value(cx: &mut TestAppContext) {
        let mut task = local_task("A");
        task.config = vec![option("mode", &["ask", "code"], Some("ask"))];
        let (view, _, events, cx) = open(cx, task);
        assert_eq!(view.read_with(cx, |view, cx| view.chips(cx)), ["ask"]);
        click(cx, "composer-config-0");
        let entries = view.read_with(cx, |view, _| view.picker_entries());
        assert_eq!(entries, Some(vec!["ask *".to_owned(), "code".to_owned()]));
        click(cx, "composer-picker-1");
        assert_eq!(
            effect_calls(&events),
            [AgentCall::SetConfig { task: "A".to_owned(), option: "mode".to_owned(), value: "code".to_owned() }]
        );
        assert_eq!(view.read_with(cx, |view, _| view.picker_entries()), None);
    }

    #[test]
    fn line_breaks_are_marks_in_the_field() {
        assert_eq!(marked("a\r\nb"), "a⏎b");
        assert_eq!(unmarked("a⏎b"), "a\nb");
    }
}
