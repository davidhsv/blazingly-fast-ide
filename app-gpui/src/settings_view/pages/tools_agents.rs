//! Tools › Agents, its Built-in tab: the built-in agents (Claude Code, Codex, Gemini CLI…) in a list,
//! and the one selected with what it launches, its executable (empty: the one found on `PATH`, with
//! Browse… and Reset), the environment variables it starts with and how to sign in.
//!
//! The values are not settings: each lives in the agent's file of the user's configuration, so
//! the page stages them in the dialog's model as values kept outside the settings
//! ([`BuiltinAgents::set_command`], [`BuiltinAgents::set_env`]), each agent's on its own, and Apply
//! or OK writes them (`agent_set_command`, `agent_set_env`) and hands the page the agents as they
//! read afterwards ([`AgentsPageView::agents_loaded`]). Reset and Cancel discard them with the rest
//! of the dialog's edits, and the fields always show what is staged for the agent selected.
//!
//! The agents are read when the dialog shows the page, through an [`AgentLister`]: the backend's
//! `agent_providers`, which reads the agent files of the configuration folder and looks for each
//! executable on `PATH` (it starts nothing). The environment can hold API keys: it is shown in its
//! field only, never in [`AgentsPageView::lines`], an event or a log line.

#[cfg(test)]
mod proofs36_agents_view;

use super::{Binding, ControlEvent, Then, code_family, hint_flow, icon_button, new_field};
use crate::agents::builtin_agents::{AgentRow, BuiltinAgents, Details, HintPart};
use crate::agents::model::{REGISTRY_PAGE, provider_icon};
use crate::backend::Backend;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::settings_view::controls::{search_label, tx};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, BorrowAppContext, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Keystroke, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_agent::AgentProvider;
use std::path::PathBuf;
use std::rc::Rc;

pub const PAGE: &str = REGISTRY_PAGE;

const BUILT_IN: &str = "Built-in";
const EXECUTABLE: &str = "Executable";
const EXECUTABLE_HINT: &str = "Leave empty to find the executable on PATH. Changes apply to new sessions.";
const ENVIRONMENT: &str = "Environment variables";
const ENVIRONMENT_HINT: &str =
    "Passed to the agent process, replacing variables of the same name. Changes apply to new sessions.";
const ENVIRONMENT_PLACEHOLDER: &str = "NAME=value; OTHER=value";
const SIGN_IN: &str = "Sign in";
const LAUNCH_COMMAND: &str = "Launch command";
/// The width of the agent list, as the page's split has it.
const LIST_WIDTH: f32 = 300.0;
/// The height of a row of the agent list.
const AGENT_ROW_HEIGHT: f32 = 32.0;

/// Reads the window's agents (`agent_providers`) and hands `then` the list, or why it could not be
/// read (no trusted project).
pub type AgentLister = Rc<dyn Fn(Then<Result<Vec<AgentProvider>, String>>, &mut App)>;

/// The lister a test sets as a global, in place of the backend's.
pub struct AgentListing(pub AgentLister);

impl gpui_kit::Global for AgentListing {}

/// The lister set as [`AgentListing`], else the backend's for window `window_label`. The backend's
/// reads only the configuration folder's agent files and the `PATH` directories, and starts no
/// program, so test builds use it too: their configuration folder is a temporary one.
pub fn lister(window_label: &str, cx: &App) -> AgentLister {
    match cx.try_global::<AgentListing>() {
        Some(set) => set.0.clone(),
        None => backend_lister(window_label),
    }
}

/// `agent_providers` for window `window_label`, run on the backend's runtime as its command runs it.
pub fn backend_lister(window_label: &str) -> AgentLister {
    let label: Rc<str> = Rc::from(window_label);
    Rc::new(move |then: Then<Result<Vec<AgentProvider>, String>>, cx: &mut App| {
        let Some(backend) = cx.try_global::<Backend>() else {
            then(Err("The backend is not running".to_owned()), cx);
            return;
        };
        let label = label.to_string();
        let work = backend.with_host(move |host| async move {
            ide_host::ops::agent::providers(&host, &label).map_err(|error| error.message)
        });
        cx.spawn(async move |cx| {
            let answer = work.await.unwrap_or_else(|error| Err(error.to_string()));
            cx.update(|cx| then(answer, cx));
        })
        .detach();
    })
}

/// The page's tabs. The ACP Registry tab, which lists, installs and removes agents of the network's
/// registry, is not drawn by this page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsTab {
    BuiltIn,
}

const TABS: [(AgentsTab, &str, &str); 1] = [(AgentsTab::BuiltIn, "builtin", BUILT_IN)];

pub struct AgentsPageView {
    binding: Binding,
    lister: AgentLister,
    agents: BuiltinAgents,
    tab: AgentsTab,
    /// The agents were read (or failed to be) since the dialog showed the page.
    loaded: bool,
    /// Counts the listings: an answer to an earlier one is dropped.
    asked: u64,
    /// The agent the fields were filled for.
    form: Option<String>,
    executable: Entity<TextField>,
    env: Entity<TextField>,
    list_focus: FocusHandle,
    /// Whether the dialog shows the page now.
    page_shown: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for AgentsPageView {}

impl AgentsPageView {
    pub fn new(binding: Binding, lister: AgentLister, cx: &mut Context<Self>) -> Self {
        let executable = new_field("", None, cx);
        let env = new_field("", Some(ENVIRONMENT_PLACEHOLDER.to_owned()), cx);
        for field in [&executable, &env] {
            field.update(cx, |field, cx| field.set_code_font(true, cx));
        }
        let subscriptions = vec![
            cx.subscribe(&executable, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.set_command(&text, cx);
                }
                TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            }),
            cx.subscribe(&env, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.set_env(&text, cx);
                }
                TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            }),
            cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)),
        ];
        let list_focus = cx.focus_handle();
        let host = cx.weak_entity();
        let claim = input::claim_keys(
            list_focus.clone(),
            move |keystroke, _, cx| host.update(cx, |view, cx| view.list_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(move |_: &mut Self, cx: &mut App| input::release_keys(claim, cx)).detach();
        let mut view = Self {
            binding,
            lister,
            agents: BuiltinAgents::default(),
            tab: AgentsTab::BuiltIn,
            loaded: false,
            asked: 0,
            form: None,
            executable,
            env,
            list_focus,
            page_shown: false,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn tab(&self) -> AgentsTab {
        self.tab
    }

    pub fn select_tab(&mut self, tab: AgentsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    /// The agents as the page read them last, those with a default executable.
    pub fn agents(&self) -> &[AgentProvider] {
        self.agents.agents()
    }

    /// The rows of the agent list, in the UI language: each agent's name and tag.
    pub fn rows(&self, cx: &App) -> Vec<AgentRow> {
        match cx.try_global::<Localization>() {
            Some(localization) => self.agents.rows(&localization.0),
            None => self.agents.rows(&I18n::default()),
        }
    }

    /// Whether the agents were read since the dialog showed the page.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Why the agents could not be read.
    pub fn error(&self) -> Option<&str> {
        self.agents.error()
    }

    pub fn executable_field(&self) -> &Entity<TextField> {
        &self.executable
    }

    pub fn env_field(&self) -> &Entity<TextField> {
        &self.env
    }

    /// The agents as read again after a write (or after any other change): the page shows them, and
    /// a listing asked for earlier is dropped when it answers.
    pub fn agents_loaded(&mut self, agents: Result<Vec<AgentProvider>, String>, cx: &mut Context<Self>) {
        self.asked += 1;
        self.show_agents(agents, cx);
    }

    fn show_agents(&mut self, agents: Result<Vec<AgentProvider>, String>, cx: &mut Context<Self>) {
        self.agents.loaded(agents);
        self.loaded = true;
        self.sync(cx);
    }

    /// Reads the agents; the answer to the latest listing is the one shown.
    fn list(&mut self, cx: &mut Context<Self>) {
        self.asked += 1;
        let asked = self.asked;
        let view = cx.weak_entity();
        let then: Then<Result<Vec<AgentProvider>, String>> =
            Rc::new(move |answer: Result<Vec<AgentProvider>, String>, cx: &mut App| {
                view.update(cx, |view, cx| {
                    if view.asked == asked {
                        view.show_agents(answer, cx);
                    }
                })
                .ok();
            });
        let lister = self.lister.clone();
        // After this turn: a lister that answers at once finds the view free to update.
        cx.defer(move |cx| lister(then, cx));
    }

    /// The page starts afresh each time the dialog shows it, as a page of its own: the Built-in tab,
    /// the first agent selected, the agents read again. What is staged stays in the dialog.
    fn follow_page(&mut self, cx: &mut Context<Self>) {
        let shown = self.binding.model.read(cx).page_id == self.binding.page;
        if shown == self.page_shown {
            return;
        }
        self.page_shown = shown;
        if shown {
            self.tab = AgentsTab::BuiltIn;
            self.agents = BuiltinAgents::default();
            self.loaded = false;
            self.form = None;
            self.list(cx);
        }
    }

    /// What the page shows of the agent selected, in the UI language.
    fn details(&self, cx: &mut App) -> Option<Details> {
        let model = self.binding.model.clone();
        cx.update_default_global::<Localization, _>(|localization, cx| {
            self.agents.details(model.read(cx), &mut localization.0)
        })
    }

    /// The fields show what is staged (else stored) for the agent selected; a field already showing
    /// it is left alone, so typing keeps its caret. While an Apply runs they cannot be changed.
    fn sync(&mut self, cx: &mut Context<Self>) {
        self.follow_page(cx);
        self.form = self.agents.selected().map(|agent| agent.id.clone());
        let details = self.details(cx);
        let saving = self.binding.model.read(cx).saving;
        let command = details.as_ref().and_then(|details| details.executable.clone()).unwrap_or_default();
        let env = details.as_ref().and_then(|details| details.env.clone()).unwrap_or_default();
        let placeholder = details.as_ref().map(|details| details.placeholder.clone()).unwrap_or_default();
        self.executable.update(cx, |field, cx| {
            if field.text() != command {
                field.set_text(&command, cx);
            }
            if field.placeholder() != placeholder {
                field.set_placeholder(placeholder, cx);
            }
            if field.is_read_only() != saving {
                field.set_read_only(saving, cx);
            }
        });
        self.env.update(cx, |field, cx| {
            if field.text() != env {
                field.set_text(&env, cx);
            }
            if field.is_read_only() != saving {
                field.set_read_only(saving, cx);
            }
        });
        cx.notify();
    }

    /// The executable field changed (typed, cleared, reset or browsed).
    fn set_command(&mut self, text: &str, cx: &mut Context<Self>) {
        let model = self.binding.model.clone();
        model.update(cx, |dialog, cx| {
            self.agents.set_command(dialog, text);
            cx.notify();
        });
    }

    fn set_env(&mut self, text: &str, cx: &mut Context<Self>) {
        let model = self.binding.model.clone();
        model.update(cx, |dialog, cx| {
            self.agents.set_env(dialog, text);
            cx.notify();
        });
    }

    /// Selects the agent at `index` of the list.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.agents.agents().len() {
            self.agents.select(index);
            self.sync(cx);
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.agents.selected()?;
        self.agents.agents().iter().position(|agent| agent.id == selected.id)
    }

    /// Up, Down, Home and End move the selection while the list has the keyboard.
    fn list_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let len = self.agents.agents().len();
        if keystroke.modifiers.modified() || len == 0 {
            return false;
        }
        let at = self.selected_index().unwrap_or_default();
        let next = match keystroke.key.as_str() {
            "up" => at.saturating_sub(1),
            "down" => (at + 1).min(len - 1),
            "home" => 0,
            "end" => len - 1,
            "escape" => {
                cx.emit(ControlEvent::Cancel);
                return true;
            }
            _ => return false,
        };
        self.select(next, cx);
        true
    }

    /// Reset beside the executable: automatic detection is staged.
    pub fn reset_executable(&mut self, cx: &mut Context<Self>) {
        self.set_command("", cx);
    }

    /// Browse…: the system's chooser for a file; the file chosen is staged as the executable of the
    /// agent it was asked for. A read-only agent has no button.
    pub fn browse(&mut self, cx: &mut Context<Self>) {
        let model = self.binding.model.clone();
        let asked = cx.update_default_global::<Localization, _>(|localization, cx| {
            self.agents.browse(model.read(cx), &mut localization.0)
        });
        let (Some(_), Some(agent)) = (asked, self.form.clone()) else { return };
        let view = cx.weak_entity();
        let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
            let path = path.to_string_lossy().into_owned();
            view.update(cx, |view, cx| view.chosen(&agent, &path, cx)).ok();
        });
        cx.emit(ControlEvent::Browse { directories: false, then });
    }

    fn chosen(&mut self, agent: &str, path: &str, cx: &mut Context<Self>) {
        // The chooser was answered after another agent was selected: its file is not that agent's.
        if self.form.as_deref() != Some(agent) {
            return;
        }
        let model = self.binding.model.clone();
        model.update(cx, |dialog, cx| {
            self.agents.chosen(dialog, Some(path));
            cx.notify();
        });
    }

    /// The page as text: the tabs (`[x]` the one shown), then the error, `…` while the agents are
    /// read, or the agents (`>` the selected one, with its tag) and the details of the selected one.
    /// The environment's value is never listed.
    pub fn lines(&self, cx: &mut App) -> Vec<String> {
        let tabs: Vec<String> = TABS
            .iter()
            .map(|(tab, _, title)| {
                let title = tx(cx, title);
                if *tab == self.tab { format!("[{title}]") } else { title }
            })
            .collect();
        let mut out = vec![tabs.join(" ")];
        if let Some(error) = self.agents.error() {
            out.push(format!("! {error}"));
            return out;
        }
        if !self.loaded {
            out.push("…".to_owned());
            return out;
        }
        let selected = self.agents.selected().map(|agent| agent.id.clone());
        for row in self.rows(cx) {
            let mark = if selected.as_deref() == Some(row.id.as_str()) { ">" } else { " " };
            let tag = row.tag.map(|tag| format!(" ({tag})")).unwrap_or_default();
            out.push(format!("{mark} {}{tag}", row.name));
        }
        if let (Some(details), Some(agent)) = (self.details(cx), self.agents.selected()) {
            out.push(format!("# {}", agent.name));
            out.push(format!("{} · {}", details.status, details.launch));
            match &details.executable {
                Some(value) => out.push(format!("{}: [{value}]", tx(cx, EXECUTABLE))),
                None => out.extend(details.note.clone()),
            }
            if details.env.is_some() {
                out.push(tx(cx, ENVIRONMENT));
            }
            if !details.login.is_empty() {
                let hint: String = details
                    .login
                    .iter()
                    .map(|part| match part {
                        HintPart::Text(text) => text.clone(),
                        HintPart::Code(code) => format!("`{code}`"),
                    })
                    .collect();
                out.push(format!("{}: {hint}", tx(cx, SIGN_IN)));
            }
        }
        out
    }

    fn tab_button(&self, tab: AgentsTab, name: &str, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("settings-agents-tab-{name}");
        let active = tab == self.tab;
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .h(theme::length(cx, "h-control-small"))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .text_color(theme::color(cx, if active { "fg-strong" } else { "fg-muted" }))
            .when(active, |el| el.bg(theme::color(cx, "bg-selected")))
            .when(!active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .child(SharedString::from(tx(cx, title)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(tab, cx)))
            .into_any_element()
    }

    /// The list of agents: the logo, the name (muted when it is neither found nor overridden) and
    /// its tag.
    fn agent_list(&self, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let rows = self.rows(cx);
        let selected = self.selected_index();
        let focused = self.list_focus.clone();
        let drawn: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let selector = format!("settings-agents-row-{index}");
                let custom = self.agents.agents().get(index).is_some_and(|agent| agent.command_overridden);
                let name_color = theme::color(cx, if row.missing { "fg-muted" } else { "fg" });
                let tag = row.tag.map(|tag| {
                    div()
                        .flex_none()
                        .text_size(theme::length(cx, "fs-small"))
                        .text_color(theme::color(cx, if custom { "accent" } else { "fg-subtle" }))
                        .child(SharedString::from(tag))
                });
                let logo = div().flex_none().when(row.missing, |el| el.opacity(0.6)).child(theme::icon(
                    cx,
                    &provider_icon(&row.id),
                    16.0,
                    theme::color(cx, "fg-icon"),
                ));
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(AGENT_ROW_HEIGHT))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .when(selected == Some(index), |el| el.bg(theme::color(cx, "bg-selected")))
                    .when(selected != Some(index), |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(logo)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(name_color)
                            .child(SharedString::from(row.name)),
                    )
                    .children(tag)
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        window.focus(&view.list_focus, cx);
                        view.select(index, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .id("settings-agents-list")
            .track_focus(&focused)
            .flex_none()
            .w(px(LIST_WIDTH))
            .p(px(6.0))
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .children(drawn)
    }

    /// A small help text under a field.
    fn hint(cx: &App, text: &str) -> Div {
        div()
            .mt(px(8.0))
            .mb(px(16.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(tx(cx, text)))
    }

    /// A label above its field, marked by the Settings search.
    fn field_label(cx: &App, text: &str, query: &str) -> Div {
        div().mb(px(6.0)).child(search_label(cx, &tx(cx, text), query))
    }

    /// The agent selected: its name, where its executable comes from with its launch command, the
    /// executable field (or why there is none), the environment field and how to sign in.
    fn details_pane(&self, cx: &mut Context<Self>) -> Option<Div> {
        let agent = self.agents.selected()?.clone();
        let details = self.details(cx)?;
        let query = self.binding.query(cx);
        let dot = if agent.command_overridden {
            "accent"
        } else if agent.installed {
            "success"
        } else {
            "fg-subtle"
        };
        let title = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_color(theme::color(cx, "fg-strong"))
            .child(theme::icon(cx, &provider_icon(&agent.id), 20.0, theme::color(cx, "fg-icon")))
            .child(
                div()
                    .text_size(theme::length(cx, "fs-input-large"))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(SharedString::from(agent.name.clone())),
            );
        let subtitle = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .min_w_0()
            .mt(px(6.0))
            .mb(px(20.0))
            .ml(px(28.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-muted"))
            .child(div().flex_none().size(px(6.0)).rounded_full().bg(theme::color(cx, dot)))
            .child(SharedString::from(details.status.clone()))
            .child("·")
            .child(
                div()
                    .id("settings-agents-launch")
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(code_family(cx))
                    .tooltip(crate::ui::tooltip::tooltip(LAUNCH_COMMAND))
                    .child(SharedString::from(details.launch.clone())),
            );
        let mut body: Vec<AnyElement> = vec![title.into_any_element(), subtitle.into_any_element()];
        match &details.executable {
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
                    .font_family(code_family(cx))
                    .child(self.executable.clone());
                body.push(Self::field_label(cx, EXECUTABLE, &query).into_any_element());
                body.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(field)
                        .child(browse)
                        .children(reset)
                        .into_any_element(),
                );
                body.push(Self::hint(cx, EXECUTABLE_HINT).into_any_element());
            }
            None => {
                let note = details.note.clone().unwrap_or_default();
                body.push(Self::hint(cx, &note).into_any_element());
            }
        }
        if details.env.is_some() {
            let field = field_box(cx)
                .id("settings-agents-env")
                .debug_selector(|| "settings-agents-env".to_owned())
                .font_family(code_family(cx))
                .child(self.env.clone());
            body.push(Self::field_label(cx, ENVIRONMENT, &query).into_any_element());
            body.push(div().flex().items_center().child(field).into_any_element());
            body.push(Self::hint(cx, ENVIRONMENT_HINT).into_any_element());
        }
        if !details.login.is_empty() {
            let parts: Vec<(bool, String)> = details
                .login
                .iter()
                .map(|part| match part {
                    HintPart::Text(text) => (false, text.clone()),
                    HintPart::Code(code) => (true, code.clone()),
                })
                .collect();
            body.push(
                div()
                    .mt(px(20.0))
                    .mb(px(6.0))
                    .text_size(theme::length(cx, "fs-ui"))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(search_label(cx, &tx(cx, SIGN_IN), &query))
                    .into_any_element(),
            );
            let login = div().debug_selector(|| "settings-agents-login".to_owned()).child(hint_flow(cx, &parts));
            body.push(login.into_any_element());
        }
        Some(div().flex_1().min_w_0().p(px(20.0)).flex().flex_col().children(body))
    }

    /// The Built-in tab: the agents and the selected one's details in a bordered split, why they
    /// could not be read, or a mark while they are read.
    fn built_in(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = self.agents.error() {
            return div()
                .debug_selector(|| "settings-agents-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error.to_owned()))
                .into_any_element();
        }
        if !self.loaded {
            return div()
                .debug_selector(|| "settings-agents-loading".to_owned())
                .text_color(theme::color(cx, "fg-subtle"))
                .child("…")
                .into_any_element();
        }
        let list = self.agent_list(cx);
        let details = self.details_pane(cx);
        div()
            .flex()
            .flex_none()
            .min_w(px(640.0))
            .min_h(px(320.0))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .rounded(theme::length(cx, "radius-control"))
            .overflow_hidden()
            .child(list)
            .children(details)
            .into_any_element()
    }
}

/// The frame of a field that takes the width the details leave it, as [`super::frame`] draws one.
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
}

impl Focusable for AgentsPageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.list_focus.clone()
    }
}

impl Render for AgentsPageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<AnyElement> =
            TABS.iter().map(|&(tab, name, title)| self.tab_button(tab, name, title, cx)).collect();
        let body = match self.tab {
            AgentsTab::BuiltIn => self.built_in(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_color(theme::color(cx, "fg"))
            .child(div().flex().flex_wrap().gap(px(2.0)).children(tabs))
            .child(body)
    }
}
