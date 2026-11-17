//! The details of the agent selected on the Coding Agents page: its icon, name and facts with the
//! header's buttons, an install's progress, what the registry says of it, and the Sign In, New Tasks
//! and Launch sections of an agent this machine knows.

use super::AgentListPageView;
use crate::agents::agent_list::{Action, AgentDetails, Dot, NewTaskOption, ProviderSettings};
use crate::agents::builtin_agents::HintPart;
use crate::settings_view::controls::{button, section, tx};
use crate::settings_view::pages::{
    code_family, drop_down, drop_down_row, hint_flow, icon_button, muted, row, row_label, select_box,
};
use crate::theme;
use crate::ui::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, IntoElement, MouseButton, SharedString, div, prelude::*, px, relative,
};

const EXECUTABLE_HINT: &str = "Leave empty to find the executable on PATH. Changes apply to new sessions.";
const ENVIRONMENT_HINT: &str =
    "Passed to the agent process, replacing variables of the same name. Changes apply to new sessions.";
const NEW_TASKS_HINT: &str =
    "What you pick on the New Task page wins; these apply until then and after switching agents.";
const NOT_KNOWN_YET: &str = "The agent's models and modes can be chosen here after its first session.";

/// The selector of a header button.
fn action_selector(action: Action) -> &'static str {
    match action {
        Action::Install => "settings-agents-action-install",
        Action::Update => "settings-agents-action-update",
        Action::Remove => "settings-agents-action-remove",
        Action::RemoveCustom => "settings-agents-action-remove-custom",
        Action::Cancel => "settings-agents-action-cancel",
    }
}

/// A small help text under a control.
fn hint(cx: &App, text: &str) -> Div {
    div()
        .pl(px(4.0))
        .pb(px(6.0))
        .text_size(theme::length(cx, "fs-small"))
        .text_color(theme::color(cx, "fg-subtle"))
        .child(SharedString::from(text.to_owned()))
}

/// The frame of a field that takes the width the details leave it.
fn field_box(cx: &App) -> Div {
    div()
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
        .font_family(code_family(cx))
}

impl AgentListPageView {
    /// The details of the agent selected; `None` before the agents are read or with none shown.
    pub(super) fn details_pane(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let details = self.details(cx)?;
        let mut body: Vec<AnyElement> = vec![self.header(&details, cx)];
        if let Some(progress) = &details.progress {
            let bar = div().h(px(4.0)).w_full().rounded_full().bg(theme::color(cx, "border")).child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(theme::color(cx, "accent"))
                    .w(relative(progress.fraction.unwrap_or(0.0).clamp(0.0, 1.0) as f32)),
            );
            body.push(
                div()
                    .debug_selector(|| "settings-agents-progress".to_owned())
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(bar)
                    .child(muted(cx, &progress.message))
                    .into_any_element(),
            );
        }
        let notice = |text: &str, color: &str, cx: &App| {
            div()
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, color))
                .child(SharedString::from(text.to_owned()))
                .into_any_element()
        };
        body.extend(details.update.as_deref().map(|text| notice(text, "accent", cx)));
        body.extend(details.error.as_deref().map(|text| {
            div()
                .debug_selector(|| "settings-agents-details-error".to_owned())
                .child(notice(text, "danger", cx))
                .into_any_element()
        }));
        body.extend(details.description.as_deref().map(|text| notice(text, "fg", cx)));
        body.extend(details.no_checksum.as_deref().map(|text| notice(text, "warning", cx)));
        if !details.links.is_empty() {
            let links: Vec<AnyElement> = details
                .links
                .iter()
                .map(|(label, url)| {
                    let selector: &'static str = if *label == "Repository" {
                        "settings-agents-link-repository"
                    } else {
                        "settings-agents-link-website"
                    };
                    let title = SharedString::from(url.clone());
                    let url = url.clone();
                    div()
                        .id(selector)
                        .debug_selector(move || selector.to_owned())
                        .text_color(theme::color(cx, "accent"))
                        .tooltip(move |window, cx| Tooltip::new(title.clone()).shown(window, cx))
                        .child(SharedString::from(tx(cx, label)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open_link(&url, cx)))
                        .into_any_element()
                })
                .collect();
            body.push(div().flex().gap(px(12.0)).children(links).into_any_element());
        }
        if let Some(settings) = &details.settings {
            body.extend(self.provider_sections(settings, cx));
        }
        body.extend(details.third_party.as_deref().map(|text| notice(text, "fg-subtle", cx)));
        Some(
            div()
                .id("settings-agents-details")
                .debug_selector(|| "settings-agents-details".to_owned())
                .flex_1()
                .min_w_0()
                .overflow_y_scroll()
                .p(px(16.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .children(body)
                .into_any_element(),
        )
    }

    /// The icon, the name, the dot and facts, and the header's buttons.
    fn header(&self, details: &AgentDetails, cx: &mut Context<Self>) -> AnyElement {
        let dot = details.dot.map(|dot| {
            let color = match dot {
                Dot::Custom => "accent",
                Dot::Found => "success",
                Dot::Missing => "fg-subtle",
            };
            div().flex_none().size(px(6.0)).rounded_full().bg(theme::color(cx, color))
        });
        let facts = div()
            .debug_selector(|| "settings-agents-facts".to_owned())
            .flex()
            .items_center()
            .gap(px(6.0))
            .min_w_0()
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-muted"))
            .children(dot)
            .child(SharedString::from(details.facts.clone()));
        let heading = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(
                div()
                    .text_size(theme::length(cx, "fs-input-large"))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(details.name.clone())),
            )
            .child(facts);
        let buttons: Vec<AnyElement> = details
            .actions
            .iter()
            .map(|action| {
                let selector = action_selector(action.action);
                let which = action.action;
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .child(button(cx, &action.label, action.primary, action.enabled))
                    .when(action.enabled, |el| {
                        el.on_click(cx.listener(move |view, _: &ClickEvent, window, cx| match which {
                            Action::Install | Action::Update => view.install(window, cx),
                            Action::Remove => view.remove(false, window, cx),
                            Action::RemoveCustom => view.remove(true, window, cx),
                            Action::Cancel => view.cancel_install(cx),
                        }))
                    })
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(theme::icon(cx, &details.icon, 24.0, theme::color(cx, "fg-icon")))
            .child(heading)
            .child(div().flex_none().flex().gap(px(6.0)).children(buttons))
            .into_any_element()
    }

    /// Sign In (when the agent says how), New Tasks and Launch.
    fn provider_sections(&self, settings: &ProviderSettings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.binding.query(cx);
        let mut out = Vec::new();
        if !settings.login.is_empty() {
            let parts: Vec<(bool, String)> = settings
                .login
                .iter()
                .map(|part| match part {
                    HintPart::Text(text) => (false, text.clone()),
                    HintPart::Code(code) => (true, code.clone()),
                })
                .collect();
            let login = div().debug_selector(|| "settings-agents-login".to_owned()).child(hint_flow(cx, &parts));
            out.push(section(cx, Some(&tx(cx, "Sign In")), &query, vec![login.into_any_element()]).into_any_element());
        }
        let new_tasks = self.new_tasks(&settings.new_tasks, &query, cx);
        out.push(section(cx, Some(&tx(cx, "New Tasks")), &query, new_tasks).into_any_element());
        let launch = self.launch(settings, &query, cx);
        out.push(section(cx, Some(&tx(cx, "Launch")), &query, launch).into_any_element());
        out
    }

    /// One choice per option, or why there are none yet.
    fn new_tasks(&self, options: &[NewTaskOption], query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if options.is_empty() {
            let text = div()
                .debug_selector(|| "settings-agents-defaults-unknown".to_owned())
                .child(muted(cx, &tx(cx, NOT_KNOWN_YET)));
            return vec![text.into_any_element()];
        }
        let mut out: Vec<AnyElement> = options
            .iter()
            .map(|option| {
                let id = option.id.clone();
                let selector = format!("settings-agents-default-{id}");
                let open = self.open_choice.as_deref() == Some(id.as_str());
                let toggle = id.clone();
                let boxed = select_box(cx, &option.shown)
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_choice(&toggle, cx)));
                let list = open.then(|| {
                    let rows: Vec<AnyElement> = option
                        .choices
                        .iter()
                        .enumerate()
                        .map(|(index, choice)| {
                            let (option_id, value) = (id.clone(), choice.value.clone());
                            let detail = choice.detail.as_ref().map(|detail| muted(cx, detail));
                            let selector = format!("settings-agents-default-{id}-{index}");
                            drop_down_row(cx, selector, &choice.label, choice.value == option.value)
                                .children(detail)
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.choose_default(&option_id, &value, cx);
                                }))
                                .into_any_element()
                        })
                        .collect();
                    drop_down(cx, rows)
                });
                // The option's name is the agent's own text: shown as the agent reports it.
                row(cx, false)
                    .child(row_label(cx, &option.name, query))
                    .child(div().relative().child(boxed).children(list))
                    .into_any_element()
            })
            .collect();
        out.push(hint(cx, &tx(cx, NEW_TASKS_HINT)).into_any_element());
        out
    }

    /// The command it launches, the executable (or why it is read-only) and the environment.
    fn launch(&self, settings: &ProviderSettings, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let command = SharedString::from(settings.launch.clone());
        let tooltip = command.clone();
        let code = div()
            .id("settings-agents-launch")
            .debug_selector(|| "settings-agents-launch".to_owned())
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .font_family(code_family(cx))
            .text_color(theme::color(cx, "fg-muted"))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).shown(window, cx))
            .child(command);
        out.push(row(cx, false).child(row_label(cx, &tx(cx, "Command"), query)).child(code).into_any_element());
        match &settings.executable {
            Some(value) => {
                let browse = icon_button(cx, "settings-agents-browse".to_owned(), "folder-open", "Browse…")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse(cx)));
                let reset = (!value.is_empty()).then(|| {
                    icon_button(cx, "settings-agents-reset".to_owned(), "undo", "Reset")
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.reset_executable(cx)))
                });
                let field = field_box(cx)
                    .id("settings-agents-executable")
                    .debug_selector(|| "settings-agents-executable".to_owned())
                    .child(self.executable.clone());
                let label = tx(cx, "Executable");
                out.push(
                    row(cx, self.binding.matches("Executable", cx))
                        .child(row_label(cx, &label, query))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .child(field)
                                .child(browse)
                                .children(reset),
                        )
                        .into_any_element(),
                );
                out.push(hint(cx, &tx(cx, EXECUTABLE_HINT)).into_any_element());
            }
            None => {
                let note = settings.note.clone().unwrap_or_default();
                out.push(hint(cx, &note).into_any_element());
            }
        }
        if settings.env.is_some() {
            let field = field_box(cx)
                .id("settings-agents-env")
                .debug_selector(|| "settings-agents-env".to_owned())
                .child(self.env.clone());
            let label = tx(cx, "Environment variables");
            out.push(
                row(cx, self.binding.matches("Environment variables", cx))
                    .child(row_label(cx, &label, query))
                    .child(field)
                    .into_any_element(),
            );
            out.push(hint(cx, &tx(cx, ENVIRONMENT_HINT)).into_any_element());
        }
        out
    }
}
