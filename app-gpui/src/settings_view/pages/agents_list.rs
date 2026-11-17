//! Agents › Coding Agents: every agent in one searchable list, those this machine can run under
//! "Installed" and those it can install under "Available", and the selected agent's details: its
//! facts and actions (Install, Update, Remove, Cancel), Sign In, the options its New Tasks start with,
//! and how it is launched (command, executable with Browse… and Reset, environment variables). Below
//! the list: Add Custom Agent…, Import acp.json, and the state of the ACP Registry.
//!
//! The launch values are not settings: each lives in the agent's file of the user's configuration, so
//! the page stages them in the dialog's model as values kept outside the settings, and Apply or OK
//! writes them (`agent_set_command`, `agent_set_env`) and hands the page the agents as they read
//! afterwards ([`AgentListPageView::agents_loaded`]). New Tasks stages the ordinary setting
//! `agents.defaults`. Installs, removals and custom agents act at once, after asking where the page
//! asks, and an install keeps running after the page closes.
//!
//! The page starts afresh each time the dialog shows it: the agents are read again and the registry
//! is listed. Everything that reaches the backend goes through the page's [`AgentListBackend`].
//! An agent's environment values may be API keys: they show in the environment field only, never in
//! [`AgentListPageView::lines`], a tooltip, an event or a log line.

mod custom_agent;
mod details;
mod master;
#[cfg(test)]
mod proofs38_agents_list_view;

pub use custom_agent::{CustomAgentEvent, CustomAgentModal};

use super::{Binding, ControlEvent, Then, new_field};
use crate::agents::agent_list::{AgentList, DEFAULTS_KEY, ListEffect, ListRow, RegistryState, defaults_with};
use crate::agents::builtin_agents::HintPart;
use crate::agents::model::REGISTRY_PAGE;
use crate::agents::registry::{Failure, RegistryEffect};
use crate::backend::Backend;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, BorrowAppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use ide_agent::registry::AgentRegistry;
use ide_agent::{AcpImportOutcome, AgentProvider, CustomAgentInput};
use ide_core::RequestId;
use ide_tools::InstallEvent;
use master::overlay;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub const PAGE: &str = REGISTRY_PAGE;

/// The master list's width, as the page's split gives it at most.
const LIST_WIDTH: f32 = 240.0;
const LIST_MIN_WIDTH: f32 = 150.0;
const ROW_HEIGHT: f32 = 28.0;
const SECTION_HEIGHT: f32 = 26.0;

/// Reads the window's agents (`agent_providers`) and hands `then` the list, or why it could not be read.
pub type Lister = Rc<dyn Fn(Then<Result<Vec<AgentProvider>, String>>, &mut App)>;

/// `agent_registry_list` with the request id it can be cancelled by.
pub type RegistryList = Rc<dyn Fn(RequestId, Then<Result<AgentRegistry, Failure>>, &mut App)>;
/// `agent_registry_install` of an agent: its progress to the first callback, its end to the second.
pub type RegistryInstall = Rc<dyn Fn(String, RequestId, Then<InstallEvent>, Then<Result<(), Failure>>, &mut App)>;
/// Reads an `acp.json` file and imports its agents.
pub type AcpImport = Rc<dyn Fn(PathBuf, Then<Result<Vec<AcpImportOutcome>, String>>, &mut App)>;

/// What the page asks of the ACP Registry. Each call answers on the window's thread.
#[derive(Clone)]
pub struct RegistryCalls {
    pub list: RegistryList,
    pub install: RegistryInstall,
    /// `agent_registry_remove`.
    pub remove: Rc<dyn Fn(String, Then<Result<(), String>>, &mut App)>,
    /// `cancel_request`.
    pub cancel: Rc<dyn Fn(RequestId, &mut App)>,
    /// Opens a registry agent's web page in the browser.
    pub open_url: Rc<dyn Fn(String, &mut App)>,
}

/// What the page asks for custom agents. Each call answers on the window's thread.
#[derive(Clone)]
pub struct CustomCalls {
    /// `agent_custom_save`.
    pub save: Rc<dyn Fn(CustomAgentInput, Then<Result<(), String>>, &mut App)>,
    /// `agent_custom_remove`.
    pub remove: Rc<dyn Fn(String, Then<Result<(), String>>, &mut App)>,
    /// Reads an `acp.json` file (`fs_read_text`), then `agent_custom_import` of its text.
    pub import: AcpImport,
    /// The system's chooser for a file; `None` when it is dismissed.
    pub pick_file: Rc<dyn Fn(Then<Option<PathBuf>>, &mut App)>,
}

/// Everything the page reaches the backend through. Without `registry` the page lists no registry
/// agents and draws no registry line; without `custom` it draws no custom-agent buttons.
#[derive(Clone)]
pub struct AgentListBackend {
    pub providers: Lister,
    pub registry: Option<RegistryCalls>,
    pub custom: Option<CustomCalls>,
}

/// Runs `f` with the UI language.
fn with_i18n<R>(cx: &mut App, f: impl FnOnce(&mut I18n) -> R) -> R {
    f(&mut cx.default_global::<Localization>().0)
}

pub struct AgentListPageView {
    binding: Binding,
    backend: AgentListBackend,
    list: AgentList,
    /// Counts the listings of the agents: an answer to an earlier one is dropped.
    asked: u64,
    /// The agent the fields were filled for.
    form: Option<String>,
    search: Entity<TextField>,
    executable: Entity<TextField>,
    env: Entity<TextField>,
    list_focus: FocusHandle,
    /// Whether the dialog shows the page now.
    page_shown: bool,
    /// The New Tasks choice whose list is open.
    open_choice: Option<String>,
    modal: Option<(Entity<CustomAgentModal>, Subscription)>,
    /// Lists the registry again every 1.5 s while installs started elsewhere run.
    poll: Option<Task<()>>,
    polling: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for AgentListPageView {}

impl AgentListPageView {
    pub fn new(binding: Binding, backend: AgentListBackend, cx: &mut Context<Self>) -> Self {
        let search = new_field("", Some(tx(cx, "Search agents")), cx);
        let executable = new_field("", None, cx);
        let env = new_field("", Some("NAME=value; OTHER=value".to_owned()), cx);
        for field in [&executable, &env] {
            field.update(cx, |field, cx| field.set_code_font(true, cx));
        }
        let subscriptions = vec![
            cx.subscribe(&search, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    view.list.query = field.read(cx).text().to_owned();
                    view.sync(cx);
                }
                TextFieldEvent::Submitted => view.enter(cx),
                TextFieldEvent::Cancelled => view.escape(cx),
            }),
            cx.subscribe(&executable, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.set_command(&text, cx);
                }
                TextFieldEvent::Submitted => view.enter(cx),
                TextFieldEvent::Cancelled => view.escape(cx),
            }),
            cx.subscribe(&env, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.set_env(&text, cx);
                }
                TextFieldEvent::Submitted => view.enter(cx),
                TextFieldEvent::Cancelled => view.escape(cx),
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
        cx.on_release(move |view: &mut Self, cx: &mut App| {
            input::release_keys(claim, cx);
            // The dialog closed: the page's registry listings are cancelled; an install keeps running.
            if let Some(calls) = view.backend.registry.clone() {
                for effect in view.list.close() {
                    if let ListEffect::Registry(RegistryEffect::Cancel(request)) = effect {
                        (calls.cancel)(request, cx);
                    }
                }
            }
        })
        .detach();
        let mut view = Self {
            binding,
            backend,
            list: AgentList::default(),
            asked: 0,
            form: None,
            search,
            executable,
            env,
            list_focus,
            page_shown: false,
            open_choice: None,
            modal: None,
            poll: None,
            polling: false,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn model(&self) -> &AgentList {
        &self.list
    }

    /// Whether the agents were read (or failed to be) since the dialog showed the page.
    pub fn is_loaded(&self) -> bool {
        self.list.is_loaded()
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search
    }

    pub fn executable_field(&self) -> &Entity<TextField> {
        &self.executable
    }

    pub fn env_field(&self) -> &Entity<TextField> {
        &self.env
    }

    /// The Add Custom Agent dialog, while it is open.
    pub fn custom_agent(&self) -> Option<&Entity<CustomAgentModal>> {
        self.modal.as_ref().map(|(modal, _)| modal)
    }

    /// The agents as read again after a write: the page shows them, and a listing asked for earlier
    /// is dropped when it answers.
    pub fn agents_loaded(&mut self, agents: Result<Vec<AgentProvider>, String>, cx: &mut Context<Self>) {
        self.asked += 1;
        self.list.loaded(agents);
        self.sync(cx);
    }

    /// Reads the agents; the answer to the latest listing is the one shown.
    fn list_providers(&mut self, cx: &mut Context<Self>) {
        self.asked += 1;
        let asked = self.asked;
        let view = cx.weak_entity();
        let then: Then<Result<Vec<AgentProvider>, String>> =
            Rc::new(move |answer: Result<Vec<AgentProvider>, String>, cx: &mut App| {
                view.update(cx, |view, cx| {
                    if view.asked == asked {
                        view.list.loaded(answer);
                        view.sync(cx);
                    }
                })
                .ok();
            });
        let lister = self.backend.providers.clone();
        // After this turn: a lister that answers at once finds the view free to update.
        cx.defer(move |cx| lister(then, cx));
    }

    /// The page starts afresh each time the dialog shows it; once hidden, its registry listings are
    /// cancelled (an install keeps running) and it stops following background installs.
    fn follow_page(&mut self, cx: &mut Context<Self>) {
        let shown = self.binding.model.read(cx).page_id == self.binding.page;
        if shown == self.page_shown {
            return;
        }
        self.page_shown = shown;
        if !shown {
            let effects = self.list.close();
            self.stop_polling();
            self.modal = None;
            self.run(effects, cx);
            return;
        }
        let request = self.backend.registry.is_some().then(Backend::next_request_id);
        let (list, effects) = AgentList::open(request);
        self.list = list;
        self.form = None;
        self.open_choice = None;
        self.search.update(cx, |field, cx| field.set_text("", cx));
        self.list_providers(cx);
        self.run(effects, cx);
    }

    /// The fields show what is staged (else stored) for the agent selected; a field already showing it
    /// is left alone, so typing keeps its caret. While an Apply runs they cannot be changed.
    fn sync(&mut self, cx: &mut Context<Self>) {
        self.follow_page(cx);
        let settings = self.details(cx).and_then(|details| details.settings);
        self.form = self.list.selected_id();
        let saving = self.binding.model.read(cx).saving;
        let command = settings.as_ref().and_then(|s| s.executable.clone()).unwrap_or_default();
        let env = settings.as_ref().and_then(|s| s.env.clone()).unwrap_or_default();
        let placeholder = settings.as_ref().map(|s| s.placeholder.clone()).unwrap_or_default();
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

    /// What the page shows of the agent selected, in the UI language.
    fn details(&self, cx: &mut App) -> Option<crate::agents::agent_list::AgentDetails> {
        let model = self.binding.model.clone();
        let defaults = self.binding.value(DEFAULTS_KEY, cx);
        cx.update_default_global::<Localization, _>(|localization, cx| {
            self.list.details(model.read(cx), defaults.as_ref(), &mut localization.0)
        })
    }

    /// The rows of the list, in the UI language.
    pub fn rows(&self, cx: &App) -> Vec<ListRow> {
        match cx.try_global::<Localization>() {
            Some(localization) => self.list.rows(&localization.0),
            None => self.list.rows(&I18n::default()),
        }
    }

    fn set_command(&mut self, text: &str, cx: &mut Context<Self>) {
        let model = self.binding.model.clone();
        model.update(cx, |dialog, cx| {
            self.list.set_command(dialog, text);
            cx.notify();
        });
    }

    fn set_env(&mut self, text: &str, cx: &mut Context<Self>) {
        let model = self.binding.model.clone();
        model.update(cx, |dialog, cx| {
            self.list.set_env(dialog, text);
            cx.notify();
        });
    }

    /// Selects agent `id`.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.list.select(id);
        self.open_choice = None;
        self.sync(cx);
    }

    /// Up, Down, Home and End move the selection over the agents (headers skipped) while the list has
    /// the keyboard.
    fn list_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() || self.modal.is_some() {
            return false;
        }
        // While the page asks, Enter and Escape answer it and the agent behind it stays selected.
        if self.list.pending().is_some() {
            match keystroke.key.as_str() {
                "enter" => self.answer(true, cx),
                "escape" => self.answer(false, cx),
                _ => {}
            }
            return true;
        }
        match keystroke.key.as_str() {
            "up" => self.list.select_next(-1),
            "down" => self.list.select_next(1),
            "home" => self.list.select_edge(false),
            "end" => self.list.select_edge(true),
            "escape" => {
                cx.emit(ControlEvent::Cancel);
                return true;
            }
            _ => return false,
        }
        self.open_choice = None;
        self.sync(cx);
        true
    }

    /// Reset beside the executable: automatic detection is staged.
    pub fn reset_executable(&mut self, cx: &mut Context<Self>) {
        self.set_command("", cx);
    }

    /// Browse… beside the executable: the system's chooser for a file; the file chosen is staged as
    /// the executable of the agent it was asked for. It is stored as text and never run here.
    pub fn browse(&mut self, cx: &mut Context<Self>) {
        let Some((agent, _title)) = with_i18n(cx, |i18n| self.list.browse(i18n)) else { return };
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
        self.set_command(path, cx);
    }

    /// A New Tasks choice: `value` empty for the agent's default.
    pub fn choose_default(&mut self, option: &str, value: &str, cx: &mut Context<Self>) {
        let Some(agent) = self.list.selected_id() else { return };
        let all = self.binding.value(DEFAULTS_KEY, cx);
        self.binding.stage(DEFAULTS_KEY, defaults_with(all.as_ref(), &agent, option, value), cx);
        self.open_choice = None;
        self.sync(cx);
    }

    fn toggle_choice(&mut self, option: &str, cx: &mut Context<Self>) {
        self.open_choice = if self.open_choice.as_deref() == Some(option) { None } else { Some(option.to_owned()) };
        cx.notify();
    }

    // ----- acting on agents -----

    /// Install or Update of the agent selected; a question it asks takes the keyboard.
    pub fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.list.selected_id() else { return };
        let request = Backend::next_request_id();
        let effects = with_i18n(cx, |i18n| self.list.install(&id, request, i18n));
        self.focus_question(window, cx);
        self.run(effects, cx);
    }

    /// Remove of the agent selected (a registry install, or a custom agent and its file), after asking.
    pub fn remove(&mut self, custom: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.list.selected_id() else { return };
        with_i18n(cx, |i18n| self.list.ask_remove(&id, custom, i18n));
        self.focus_question(window, cx);
        self.sync(cx);
    }

    /// The question asked now takes the keyboard (the list's keys answer it).
    fn focus_question(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list.pending().is_some() {
            window.focus(&self.list_focus, cx);
        }
    }

    /// Enter in a field: the answer to the question asked, else the dialog's OK.
    fn enter(&mut self, cx: &mut Context<Self>) {
        if self.list.pending().is_some() {
            self.answer(true, cx);
        } else {
            cx.emit(ControlEvent::Submit);
        }
    }

    /// Escape in a field: no to the question asked, else the dialog's Cancel.
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.list.pending().is_some() {
            self.answer(false, cx);
        } else {
            cx.emit(ControlEvent::Cancel);
        }
    }

    /// The answer to the question the page asks.
    pub fn answer(&mut self, ok: bool, cx: &mut Context<Self>) {
        let request = Backend::next_request_id();
        let effects = with_i18n(cx, |i18n| self.list.answer(ok, request, i18n));
        self.run(effects, cx);
    }

    /// Cancel of the install the page started.
    pub fn cancel_install(&mut self, cx: &mut Context<Self>) {
        let effects: Vec<ListEffect> = self.list.cancel_install().into_iter().collect();
        self.run(effects, cx);
    }

    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if let Some(calls) = &self.backend.registry {
            let open = calls.open_url.clone();
            let url = url.to_owned();
            cx.defer(move |cx| open(url, cx));
        }
    }

    /// Carries out what the model asks, then shows the state it leaves.
    fn run(&mut self, effects: Vec<ListEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                ListEffect::RemoveCustom(id) => self.remove_custom(id, cx),
                ListEffect::Registry(effect) => self.run_registry(effect, cx),
            }
        }
        self.sync(cx);
    }

    fn run_registry(&mut self, effect: RegistryEffect, cx: &mut Context<Self>) {
        let Some(calls) = self.backend.registry.clone() else { return };
        let view = cx.weak_entity();
        match effect {
            RegistryEffect::List(request) => {
                let then: Then<Result<AgentRegistry, Failure>> =
                    Rc::new(move |answer: Result<AgentRegistry, Failure>, cx: &mut App| {
                        view.update(cx, |view, cx| {
                            let effects = with_i18n(cx, |i18n| view.list.registry_answered(request, answer, i18n));
                            view.run(effects, cx);
                        })
                        .ok();
                    });
                cx.defer(move |cx| (calls.list)(request, then, cx));
            }
            RegistryEffect::Install { id, request } => {
                let events = view.clone();
                let on_event: Then<InstallEvent> = Rc::new(move |event: InstallEvent, cx: &mut App| {
                    events
                        .update(cx, |view, cx| {
                            with_i18n(cx, |i18n| view.list.install_event(request, event, i18n));
                            view.sync(cx);
                        })
                        .ok();
                });
                let done: Then<Result<(), Failure>> = Rc::new(move |result: Result<(), Failure>, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        let effects = with_i18n(cx, |i18n| view.list.installed(request, result, i18n));
                        view.run(effects, cx);
                    })
                    .ok();
                });
                cx.defer(move |cx| (calls.install)(id, request, on_event, done, cx));
            }
            RegistryEffect::Remove { id } => {
                let removed = id.clone();
                let then: Then<Result<(), String>> = Rc::new(move |result: Result<(), String>, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        let effects = with_i18n(cx, |i18n| view.list.removed(&removed, result, i18n));
                        view.run(effects, cx);
                    })
                    .ok();
                });
                cx.defer(move |cx| (calls.remove)(id, then, cx));
            }
            RegistryEffect::Cancel(request) => cx.defer(move |cx| (calls.cancel)(request, cx)),
            RegistryEffect::PollEvery { ms } => self.start_polling(ms, cx),
            RegistryEffect::Changed => self.list_providers(cx),
            // The model asks its questions itself (`AgentList::pending`).
            RegistryEffect::Confirm { .. } => {}
        }
    }

    fn start_polling(&mut self, ms: f64, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        let every = Duration::from_millis(ms.max(0.0) as u64);
        self.poll = Some(cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor().timer(every).await;
                let go = view.update(cx, |view, cx| view.poll_tick(cx));
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn stop_polling(&mut self) {
        self.polling = false;
        self.poll = None;
    }

    /// A tick of the poll: lists the registry again unless a listing is in flight; `false` once
    /// nothing runs in the background any more.
    fn poll_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.list.following_background() {
            self.polling = false;
            return false;
        }
        if let Some(effect) = self.list.poll(Backend::next_request_id()) {
            self.run(vec![effect], cx);
        }
        true
    }

    fn remove_custom(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(calls) = self.backend.custom.clone() else { return };
        let view = cx.weak_entity();
        let removed = id.clone();
        let then: Then<Result<(), String>> = Rc::new(move |result: Result<(), String>, cx: &mut App| {
            view.update(cx, |view, cx| {
                if view.list.custom_removed(&removed, result) {
                    view.list_providers(cx);
                }
                view.sync(cx);
            })
            .ok();
        });
        cx.defer(move |cx| (calls.remove)(id, then, cx));
    }

    /// Import acp.json: the file chosen is read and its agents saved; the page says which were saved
    /// and which skipped.
    pub fn import(&mut self, cx: &mut Context<Self>) {
        let Some(calls) = self.backend.custom.clone() else { return };
        if !self.list.import_started() {
            return;
        }
        let view = cx.weak_entity();
        let import = calls.import.clone();
        let picked: Then<Option<PathBuf>> = Rc::new(move |path: Option<PathBuf>, cx: &mut App| {
            let view = view.clone();
            let finish = move |result: Option<Result<Vec<AcpImportOutcome>, String>>, cx: &mut App| {
                view.update(cx, |view, cx| {
                    if with_i18n(cx, |i18n| view.list.import_finished(result, i18n)) {
                        view.list_providers(cx);
                    }
                    view.sync(cx);
                })
                .ok();
            };
            match path {
                None => finish(None, cx),
                Some(path) => {
                    let then: Then<Result<Vec<AcpImportOutcome>, String>> =
                        Rc::new(move |result: Result<Vec<AcpImportOutcome>, String>, cx: &mut App| {
                            finish(Some(result), cx)
                        });
                    import(path, then, cx);
                }
            }
        });
        cx.defer(move |cx| (calls.pick_file)(picked, cx));
        self.sync(cx);
    }

    /// Add Custom Agent…: the dialog over the page, its Name field with the keyboard.
    pub fn add_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(calls) = self.backend.custom.clone() else { return };
        if self.list.busy() || self.modal.is_some() {
            return;
        }
        let modal = cx.new(|cx| CustomAgentModal::new(calls, cx));
        let subscription = cx.subscribe(&modal, |view: &mut Self, _, event: &CustomAgentEvent, cx| {
            view.modal = None;
            if *event == CustomAgentEvent::Saved {
                view.list_providers(cx);
            }
            cx.notify();
        });
        let focus = modal.read(cx).name_field().focus_handle(cx);
        window.focus(&focus, cx);
        self.modal = Some((modal, subscription));
        cx.notify();
    }

    // ----- what the page shows -----

    /// The page as text: `! error`, `…` while the agents are read, else the search, the rows (`#` a
    /// header, `>` the selected agent, `~` a muted one, its tag in parentheses, `*` while it installs),
    /// `No agents match`, the buttons, the notice and the registry's line, then the details of the agent
    /// selected and the question asked. The environment's value is never listed.
    pub fn lines(&self, cx: &mut App) -> Vec<String> {
        if let Some(error) = self.list.error() {
            return vec![format!("! {error}")];
        }
        if !self.list.is_loaded() {
            return vec!["…".to_owned()];
        }
        let mut out = vec![format!("search: [{}]", self.list.query)];
        let selected = self.list.selected_id();
        for row in self.rows(cx) {
            match row {
                ListRow::Section(title) => out.push(format!("# {}", tx(cx, title))),
                ListRow::Agent(item) => {
                    let mark = if selected.as_deref() == Some(item.id.as_str()) { ">" } else { " " };
                    let muted = if item.muted { "~" } else { "" };
                    let tag = if item.working {
                        " *".to_owned()
                    } else {
                        item.tag.map(|tag| format!(" ({})", tag.label)).unwrap_or_default()
                    };
                    out.push(format!("{mark} {muted}{}{tag}", item.name));
                }
            }
        }
        if self.list.nothing_matches() {
            out.push(tx(cx, "No agents match"));
        }
        if self.backend.custom.is_some() {
            let import = if self.list.importing() { "Importing…" } else { "Import acp.json" };
            out.push(format!("[{}] [{}]", tx(cx, "Add Custom Agent…"), tx(cx, import)));
        }
        out.extend(self.list.notice().map(|notice| format!("notice: {notice}")));
        out.extend(self.registry_line(cx).map(|line| format!("registry: {line}")));
        if let Some(details) = self.details(cx) {
            out.push(format!("## {}", details.name));
            out.push(details.facts.clone());
            if !details.actions.is_empty() {
                let buttons: Vec<String> = details
                    .actions
                    .iter()
                    .map(|b| if b.enabled { format!("[{}]", b.label) } else { format!("[{}]-", b.label) })
                    .collect();
                out.push(buttons.join(" "));
            }
            if let Some(progress) = &details.progress {
                let percent = progress.fraction.map(|f| format!("{:.0}% ", f * 100.0)).unwrap_or_default();
                out.push(format!("progress: {percent}{}", progress.message));
            }
            out.extend(details.update.clone());
            out.extend(details.error.as_ref().map(|error| format!("! {error}")));
            out.extend(details.description.clone());
            out.extend(details.no_checksum.clone());
            for (label, url) in &details.links {
                out.push(format!("{}: {url}", tx(cx, label)));
            }
            if let Some(settings) = &details.settings {
                if !settings.login.is_empty() {
                    let hint: String = settings
                        .login
                        .iter()
                        .map(|part| match part {
                            HintPart::Text(text) => text.clone(),
                            HintPart::Code(code) => format!("`{code}`"),
                        })
                        .collect();
                    out.push(format!("{}: {hint}", tx(cx, "Sign In")));
                }
                out.push(format!("{}:", tx(cx, "New Tasks")));
                if settings.new_tasks.is_empty() {
                    out.push(tx(cx, "The agent's models and modes can be chosen here after its first session."));
                }
                for option in &settings.new_tasks {
                    out.push(format!("{}: [{}]", option.name, option.shown));
                }
                out.push(format!("{}: {}", tx(cx, "Command"), settings.launch));
                match &settings.executable {
                    Some(value) => out.push(format!("{}: [{value}]", tx(cx, "Executable"))),
                    None => out.extend(settings.note.clone()),
                }
                if settings.env.is_some() {
                    out.push(tx(cx, "Environment variables"));
                }
            }
            out.extend(details.third_party.clone());
        }
        if let Some(pending) = self.list.pending() {
            let request = pending.request();
            out.push(format!("? {}: {} [{}] [{}]", request.title, request.message, request.ok, tx(cx, "Cancel")));
        }
        out
    }

    /// The registry's state line, in the UI language.
    fn registry_line(&self, cx: &mut App) -> Option<String> {
        match with_i18n(cx, |i18n| self.list.registry_state(i18n))? {
            RegistryState::Loading => Some(tx(cx, "Loading the ACP Registry…")),
            RegistryState::Warning(text) | RegistryState::Error(text) => Some(text),
        }
    }
}

impl Focusable for AgentListPageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.list_focus.clone()
    }
}

impl Render for AgentListPageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if let Some(error) = self.list.error() {
            div()
                .debug_selector(|| "settings-agents-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error.to_owned()))
                .into_any_element()
        } else if !self.list.is_loaded() {
            div()
                .debug_selector(|| "settings-agents-loading".to_owned())
                .text_color(theme::color(cx, "fg-subtle"))
                .child("…")
                .into_any_element()
        } else {
            let master = self.master(cx);
            let details = self.details_pane(cx);
            div()
                .flex()
                .flex_1()
                .min_h(px(420.0))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .rounded(theme::length(cx, "radius-control"))
                .overflow_hidden()
                .child(master)
                .children(details)
                .into_any_element()
        };
        let confirm = self.confirm_panel(cx);
        let modal =
            self.modal.as_ref().map(|(modal, _)| overlay("settings-agents-custom", modal.clone().into_any_element()));
        div()
            .id("settings-agents-page")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(theme::color(cx, "fg"))
            .child(body)
            .children(modal)
            .children(confirm)
    }
}
