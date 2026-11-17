//! Add Custom Agent: a dialog over the Coding Agents page for an ACP agent of the user's own: its name,
//! the id of its agent file (suggested from the name until the id is edited), the command with
//! Browse…, its arguments and environment variables. Add saves it (`agent_custom_save`); the backend
//! refuses a built-in's or a registry install's id and one already taken, and the dialog shows why.
//!
//! The command and arguments are data: they are handed to the backend as typed and never run here,
//! and nothing builds a shell command from them. The environment field shows its values as they are
//! typed; they leave the dialog only in the input saved.

use super::CustomCalls;
use crate::agents::agent_list::{parse_args, parse_env, slugify_id};
use crate::platform::i18n::Localization;
use crate::settings_view::controls::{button, tx};
use crate::settings_view::pages::{Then, icon_button, new_field};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use ide_agent::CustomAgentInput;
use std::path::PathBuf;
use std::rc::Rc;

/// The dialog's fields, in their order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Id,
    Command,
    Args,
    Env,
}

const FIELDS: [(Field, &str, &str); 5] = [
    (Field::Name, "name", "Name"),
    (Field::Id, "id", "ID"),
    (Field::Command, "command", "Command"),
    (Field::Args, "args", "Arguments"),
    (Field::Env, "env", "Environment variables"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomAgentEvent {
    /// The agent was saved: the list changed.
    Saved,
    /// Cancel or Escape.
    Closed,
}

pub struct CustomAgentModal {
    calls: CustomCalls,
    fields: [Entity<TextField>; 5],
    /// The id was typed: the name no longer suggests it.
    id_touched: bool,
    saving: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CustomAgentEvent> for CustomAgentModal {}

impl CustomAgentModal {
    pub fn new(calls: CustomCalls, cx: &mut Context<Self>) -> Self {
        let placeholders = [
            Some(tx(cx, "My agent")),
            Some("my-agent".to_owned()),
            Some(tx(cx, "/path/to/agent-acp")),
            Some("--acp".to_owned()),
            Some("NAME=value; OTHER=value".to_owned()),
        ];
        let fields = placeholders.map(|placeholder| new_field("", placeholder, cx));
        let mut subscriptions = Vec::new();
        for (at, field) in fields.iter().enumerate() {
            if at > 0 {
                field.update(cx, |field, cx| field.set_code_font(true, cx));
            }
            let which = FIELDS[at].0;
            subscriptions.push(cx.subscribe(
                field,
                move |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
                    TextFieldEvent::Changed => view.changed(which, cx),
                    TextFieldEvent::Submitted => view.save(cx),
                    TextFieldEvent::Cancelled => cx.emit(CustomAgentEvent::Closed),
                },
            ));
        }
        Self { calls, fields, id_touched: false, saving: false, error: None, _subscriptions: subscriptions }
    }

    fn entity(&self, which: Field) -> &Entity<TextField> {
        &self.fields[which as usize]
    }

    pub fn field(&self, which: Field) -> &Entity<TextField> {
        self.entity(which)
    }

    pub fn name_field(&self) -> &Entity<TextField> {
        self.entity(Field::Name)
    }

    fn text(&self, which: Field, cx: &App) -> String {
        self.entity(which).read(cx).text().to_owned()
    }

    /// Why the last Add failed (or Browse…).
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Add can be pressed: a name, an id and a command, and no save under way.
    pub fn can_save(&self, cx: &App) -> bool {
        let filled = |which| !self.text(which, cx).trim().is_empty();
        filled(Field::Name) && filled(Field::Id) && filled(Field::Command) && !self.saving
    }

    fn changed(&mut self, which: Field, cx: &mut Context<Self>) {
        match which {
            // Text set by the dialog itself does not come back as a change.
            Field::Id => self.id_touched = true,
            Field::Name if !self.id_touched => {
                let id = slugify_id(&self.text(Field::Name, cx));
                self.entity(Field::Id).update(cx, |field, cx| field.set_text(&id, cx));
            }
            _ => {}
        }
        cx.notify();
    }

    /// Browse… beside the command: the file chosen becomes the command, as text.
    pub fn browse(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let view = cx.weak_entity();
        let then: Then<Option<PathBuf>> = Rc::new(move |path: Option<PathBuf>, cx: &mut App| {
            view.update(cx, |view, cx| {
                if let Some(path) = path {
                    let path = path.to_string_lossy().into_owned();
                    view.entity(Field::Command).update(cx, |field, cx| field.set_text(&path, cx));
                }
                view.error = None;
                cx.notify();
            })
            .ok();
        });
        let pick = self.calls.pick_file.clone();
        cx.defer(move |cx| pick(then, cx));
    }

    /// Add: the agent as typed, its arguments split as a shell would split them (without running one)
    /// and its environment read as `NAME=value` pairs.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        let env = self.text(Field::Env, cx);
        let parsed = parse_env(&env, &mut cx.default_global::<Localization>().0);
        let env = match parsed {
            Ok(env) => env,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let input = CustomAgentInput {
            id: self.text(Field::Id, cx).trim().to_owned(),
            name: self.text(Field::Name, cx).trim().to_owned(),
            command: self.text(Field::Command, cx).trim().to_owned(),
            args: parse_args(&self.text(Field::Args, cx)),
            env,
        };
        self.saving = true;
        self.error = None;
        let view = cx.weak_entity();
        let then: Then<Result<(), String>> = Rc::new(move |result: Result<(), String>, cx: &mut App| {
            view.update(cx, |view, cx| {
                view.saving = false;
                match result {
                    Ok(()) => cx.emit(CustomAgentEvent::Saved),
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        });
        let save = self.calls.save.clone();
        cx.defer(move |cx| save(input, then, cx));
        cx.notify();
    }

    /// The dialog as text: each field's label and text (the environment's only as whether it is
    /// empty), the error, and the Add button's state.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![tx(cx, "Add Custom Agent")];
        for (which, _, label) in FIELDS {
            let text = self.text(which, cx);
            let shown = if which == Field::Env { if text.is_empty() { "" } else { "…" }.to_owned() } else { text };
            out.push(format!("{}: [{shown}]", tx(cx, label)));
        }
        out.extend(self.error.as_ref().map(|error| format!("! {error}")));
        out.push(if self.can_save(cx) { "[Add]".to_owned() } else { "[Add]-".to_owned() });
        out
    }

    fn field_row(&self, which: Field, name: &str, label: &str, cx: &mut Context<Self>) -> Div {
        let selector = format!("settings-agents-custom-{name}");
        let field = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .flex_1()
            .min_w_0()
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-input"))
            .child(self.entity(which).clone());
        let browse = (which == Field::Command).then(|| {
            icon_button(cx, "settings-agents-custom-browse".to_owned(), "folder-open", "Browse…")
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse(cx)))
        });
        let hint = match which {
            Field::Id => Some("Letters, digits, dashes and underscores only; names the agent file."),
            Field::Args => Some("Separate arguments with spaces; quote ones containing spaces."),
            _ => None,
        }
        .map(|hint| {
            div()
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(tx(cx, hint)))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_none().w(px(150.0)).child(SharedString::from(tx(cx, label))))
                    .child(field)
                    .children(browse),
            )
            .children(hint)
    }
}

impl Render for CustomAgentModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = FIELDS
            .iter()
            .map(|&(which, name, label)| self.field_row(which, name, label, cx).into_any_element())
            .collect();
        let error = self.error.as_ref().map(|error| {
            div()
                .debug_selector(|| "settings-agents-custom-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error.clone()))
        });
        let can_save = self.can_save(cx);
        let add = div()
            .id("settings-agents-custom-add")
            .debug_selector(|| "settings-agents-custom-add".to_owned())
            .child(button(cx, &tx(cx, "Add"), true, can_save))
            .when(can_save, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save(cx))));
        let cancel = div()
            .id("settings-agents-custom-cancel")
            .debug_selector(|| "settings-agents-custom-cancel".to_owned())
            .child(button(cx, &tx(cx, "Cancel"), false, true))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(CustomAgentEvent::Closed)));
        div()
            .w(px(560.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(16.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-dialog"))
            .text_color(theme::color(cx, "fg"))
            .child(
                div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(tx(cx, "Add Custom Agent"))),
            )
            .children(error)
            .children(rows)
            .child(div().flex().justify_end().gap(px(8.0)).child(add).child(cancel))
    }
}
