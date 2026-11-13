//! The commit message area of the Commit tool window and the controls around it: Amend, the
//! commit message history, the message field with its inspections, the failed before-commit
//! checks, the Commit and Commit and Push… (or Commit and Rebase) buttons and the Commit Options
//! popup (author, sign-off, Git hooks and the before-commit checks).
//!
//! The draft lives in the [`CommitState`] the box shares with the changes tree; the box writes the
//! message, Amend and the options into it and reads the failed checks, the Fixup… target and the
//! focus and history requests from it. Running the commit, reading the last commit's message and
//! changing settings are the window's: the box asks for them with [`CommitBoxEvent`].

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::commit_checks::CommitProblem;
use crate::vcs_ops::commit_store::{CommitState, pending_fixup};
use crate::vcs_ops::commit_view::{commit_and_push_label, commit_label, message_problems, wrap_at_margin};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_core::settings::CommitSettings;
use ide_vcs::{Commit, CommitOptions};

/// How many characters of a message's first line the history popup shows.
const HISTORY_LABEL: usize = 80;
/// Height of the bar above the message and of the row of buttons below it.
const BAR_HEIGHT: f32 = 28.0;
const ACTIONS_HEIGHT: f32 = 40.0;
const MENU_WIDTH: f32 = 360.0;
const OPTIONS_WIDTH: f32 = 300.0;

/// A before-commit check a plugin contributes, listed under Advanced Commit Checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCheck {
    /// `<plugin id>/<check id>`, as `vcs.commit.disabledPluginChecks` lists it.
    pub key: String,
    pub title: String,
}

/// What the box shows besides the draft.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommitBoxInput {
    /// How many changes the inclusion checkboxes include.
    pub included: usize,
    /// The repository is stopped in a merge: committing it needs no included change.
    pub merging: bool,
    /// A fetch, pull or push is running: Commit and Push… waits for it.
    pub remote_busy: bool,
    /// A commit, its before-commit checks or the push after it are running.
    pub busy: bool,
    /// What the running before-commit check is doing ("Analyzing code…"), shown while it runs.
    pub progress: Option<String>,
    /// The commit messages used before, newest first.
    pub history: Vec<String>,
    /// Settings › Version Control › Commit.
    pub settings: CommitSettings,
    /// Before-commit checks contributed by plugins.
    pub plugin_checks: Vec<PluginCheck>,
}

/// What the Commit buttons ask for.
#[derive(Debug, Clone)]
pub struct CommitRequest {
    pub message: String,
    /// Amend, sign-off, author and hooks; the files to commit are the window's to fill in from the
    /// inclusion (`commit_selection`).
    pub options: CommitOptions,
    /// Commit and Push…: the push follows the commit.
    pub push: bool,
    /// Commit and Rebase: the Fixup… / Squash Into… target the commit is squashed into afterwards.
    pub rebase_onto: Option<Commit>,
    /// The failed checks were shown: Commit Anyway commits without running them again.
    pub anyway: bool,
}

/// A setting of Settings › Version Control › Commit the options popup edits in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitCheck {
    Reformat,
    OptimizeImports,
    Cleanup,
    AnalyzeCode,
    CheckTodo,
}

impl CommitCheck {
    /// The settings key the check is stored under (`vcs.commit.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Reformat => "reformat",
            Self::OptimizeImports => "optimizeImports",
            Self::Cleanup => "cleanup",
            Self::AnalyzeCode => "analyzeCode",
            Self::CheckTodo => "checkTodo",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Reformat => "Reformat code",
            Self::OptimizeImports => "Optimize imports",
            Self::Cleanup => "Cleanup",
            Self::AnalyzeCode => "Analyze code",
            Self::CheckTodo => "Check TODO",
        }
    }

    fn on(self, settings: &CommitSettings) -> bool {
        match self {
            Self::Reformat => settings.reformat,
            Self::OptimizeImports => settings.optimize_imports,
            Self::Cleanup => settings.cleanup,
            Self::AnalyzeCode => settings.analyze_code,
            Self::CheckTodo => settings.check_todo,
        }
    }
}

/// What the box asks of its window.
#[derive(Debug, Clone)]
pub enum CommitBoxEvent {
    /// Commit, Commit and Push… or Commit and Rebase was asked for, with a message (or Amend).
    Commit(Box<CommitRequest>),
    /// Amend was checked: read the last commit's message and pass it to
    /// [`CommitBox::amend_message_loaded`].
    LoadAmendMessage,
    /// Keep this message in the commit message history (the draft Amend replaced).
    RememberMessage(String),
    /// Set `vcs.commit.<check>` to `on`.
    SetCheck { check: CommitCheck, on: bool },
    /// Turn the plugin check `key` on or off (`vcs.commit.disabledPluginChecks`).
    SetPluginCheck { key: String, on: bool },
    /// Configure… of Check TODO: open Settings › Version Control › Commit (`file.settings` with
    /// `vcs.commit`).
    Configure,
    /// The details link of the failed check at this index of `check_problems` was clicked.
    ProblemDetails(usize),
}

/// A button as it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonState {
    pub label: String,
    pub enabled: bool,
}

/// The popup open over the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popup {
    /// The commit message history, with the row the keyboard is on.
    History {
        active: usize,
    },
    Options,
}

pub struct CommitBox {
    focus: FocusHandle,
    state: Entity<CommitState>,
    area: Entity<TextArea>,
    author: Entity<TextField>,
    input: CommitBoxInput,
    popup: Option<Popup>,
    /// Commit was asked for without a message: said at the field until it is typed in.
    empty_error: bool,
    /// The requests of the shared state already answered.
    seen_focus: u64,
    seen_history: u64,
    key_claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommitBoxEvent> for CommitBox {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The first line of a message, as the history popup lists it.
fn history_label(message: &str) -> String {
    message.split('\n').next().unwrap_or_default().chars().take(HISTORY_LABEL).collect()
}

/// A square button showing `icon`, lit while `pressed`, faint while it cannot be used.
fn icon_button(cx: &App, icon: &str, pressed: bool, enabled: bool) -> Div {
    let color = theme::color(
        cx,
        if !enabled {
            "fg-faint"
        } else if pressed {
            "fg-strong"
        } else {
            "fg-muted"
        },
    );
    div()
        .flex_none()
        .size(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A button with a text label. The primary one is filled; one that cannot be used is drawn faint.
fn button(cx: &App, label: &str, primary: bool, enabled: bool) -> Div {
    let (bg, fg, border) = if primary && enabled {
        ("bg-button-primary", "fg-button-primary", "bg-button-primary")
    } else {
        ("bg-button", if enabled { "fg" } else { "fg-faint" }, "border-button")
    };
    div()
        .flex_none()
        .h(theme::length(cx, "h-button"))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, border))
        .bg(theme::color(cx, bg))
        .text_color(theme::color(cx, fg))
        .whitespace_nowrap()
        .child(SharedString::from(label.to_owned()))
}

/// A check box with its label.
fn checkbox(cx: &App, label: String, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
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
        .child(SharedString::from(label))
}

impl CommitBox {
    pub fn new(state: Entity<CommitState>, input: CommitBoxInput, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Commit Message");
        let area = cx.new(|cx| {
            let mut area = TextArea::new(cx).with_placeholder(placeholder);
            area.set_rows(3, 12, cx);
            area
        });
        let author = cx.new(TextField::new);
        let (message, draft_author, focus_tick, history_tick) = {
            let draft = state.read(cx);
            (draft.message.clone(), draft.author.clone(), draft.focus_tick, draft.history_tick)
        };
        area.update(cx, |area, cx| area.set_text(&message, cx));
        author.update(cx, |field, cx| field.set_text(&draft_author, cx));
        let subscriptions = vec![
            cx.subscribe_in(&area, window, |this: &mut Self, _, event: &TextAreaEvent, window, cx| {
                this.area_event(event, window, cx);
            }),
            cx.subscribe(&author, |this: &mut Self, field, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    let text = field.read(cx).text().to_owned();
                    this.state.update(cx, |state, cx| {
                        state.author = text;
                        cx.notify();
                    });
                }
            }),
            cx.observe_in(&state, window, |this: &mut Self, _, window, cx| this.state_changed(window, cx)),
        ];
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            area.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |this, cx| this.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|this, cx| input::release_keys(this.key_claim, cx)).detach();
        Self {
            focus: cx.focus_handle(),
            state,
            area,
            author,
            input,
            popup: None,
            empty_error: false,
            seen_focus: focus_tick,
            seen_history: history_tick,
            key_claim,
            _subscriptions: subscriptions,
        }
    }

    /// Shows what the window knows now. The failed checks concern what was about to be committed:
    /// another number of included changes dismisses them.
    pub fn set_input(&mut self, input: CommitBoxInput, cx: &mut Context<Self>) {
        if input.included != self.input.included {
            self.clear_problems(cx);
        }
        if input.history.is_empty() && matches!(self.popup, Some(Popup::History { .. })) {
            self.popup = None;
        }
        self.input = input;
        cx.notify();
    }

    pub fn input(&self) -> &CommitBoxInput {
        &self.input
    }

    /// The message field.
    pub fn message_area(&self) -> &Entity<TextArea> {
        &self.area
    }

    /// The Author field of the options popup.
    pub fn author_field(&self) -> &Entity<TextField> {
        &self.author
    }

    pub fn message(&self, cx: &App) -> String {
        self.state.read(cx).message.clone()
    }

    /// Commit was asked for without a message: "Specify commit message" shows at the field.
    pub fn empty_error(&self) -> bool {
        self.empty_error
    }

    /// The Commit buttons can be used: something is included (or Amend, or a merge is committed)
    /// and no commit is running. An empty message does not disable them: it is reported at the
    /// field.
    fn can_commit(&self, cx: &App) -> bool {
        let amend = self.state.read(cx).amend;
        (self.input.included > 0 || amend || self.input.merging) && !self.input.busy
    }

    /// The Fixup… / Squash Into… target while the message is still the one prepared for it.
    fn fixup(&self, cx: &App) -> Option<Commit> {
        let state = self.state.read(cx);
        pending_fixup(&state.message, state.fixup_target.as_ref()).cloned()
    }

    /// The Commit button and the one beside it (Commit and Push…, or Commit and Rebase after
    /// Fixup… / Squash Into…), as they are drawn.
    pub fn buttons(&self, cx: &mut App) -> [ButtonState; 2] {
        let can = self.can_commit(cx);
        let (amend, problems) = {
            let state = self.state.read(cx);
            (state.amend, !state.check_problems.is_empty())
        };
        let commit = ButtonState { label: tx(cx, commit_label(amend, problems)), enabled: can };
        let second = if self.fixup(cx).is_some() {
            ButtonState { label: tx(cx, "Commit and Rebase"), enabled: can }
        } else {
            ButtonState { label: tx(cx, commit_and_push_label(amend)), enabled: can && !self.input.remote_busy }
        };
        [commit, second]
    }

    /// The inspections of the message shown below it; the subject's length shows in the bar.
    pub fn inspections(&self, cx: &App) -> Vec<String> {
        let message = &self.state.read(cx).message;
        message_problems(message, &self.input.settings)
            .into_iter()
            .filter(|problem| !problem.starts_with("Subject line"))
            .collect()
    }

    /// `length/limit` of a subject line longer than the limit.
    pub fn subject_length(&self, cx: &App) -> Option<String> {
        let settings = &self.input.settings;
        let message = &self.state.read(cx).message;
        let length = message.split('\n').next().unwrap_or_default().encode_utf16().count();
        (settings.limit_subject && length > settings.subject_limit as usize)
            .then(|| format!("{length}/{}", settings.subject_limit))
    }

    /// The failed before-commit checks, as `title: text` with the details link's label.
    pub fn problems(&self, cx: &mut App) -> Vec<String> {
        let problems: Vec<CommitProblem> = self.state.read(cx).check_problems.clone();
        problems
            .iter()
            .map(|problem| {
                let line = format!("{}: {}", tx(cx, &problem.title), tx(cx, &problem.text));
                match &problem.details {
                    Some(details) => format!("{line} {}", tx(cx, details)),
                    None => line,
                }
            })
            .collect()
    }

    /// The rows of the open commit message history, `None` while it is closed.
    pub fn history_lines(&self) -> Option<Vec<String>> {
        matches!(self.popup, Some(Popup::History { .. }))
            .then(|| self.input.history.iter().map(|message| history_label(message)).collect())
    }

    /// The history row the keyboard is on.
    pub fn history_active(&self) -> Option<usize> {
        match self.popup {
            Some(Popup::History { active }) => Some(active),
            _ => None,
        }
    }

    pub fn options_open(&self) -> bool {
        self.popup == Some(Popup::Options)
    }

    /// The options popup's rows with their state, as `label: on`.
    pub fn option_rows(&self, cx: &mut App) -> Vec<(String, bool)> {
        let (sign_off, run_hooks) = {
            let state = self.state.read(cx);
            (state.sign_off, state.run_hooks)
        };
        let settings = self.input.settings.clone();
        let mut rows = vec![(tx(cx, "Sign-off commit"), sign_off)];
        for check in [CommitCheck::Reformat, CommitCheck::OptimizeImports, CommitCheck::Cleanup] {
            rows.push((tx(cx, check.label()), check.on(&settings)));
        }
        rows.push((tx(cx, "Run Git hooks"), run_hooks));
        for check in [CommitCheck::AnalyzeCode, CommitCheck::CheckTodo] {
            rows.push((tx(cx, check.label()), check.on(&settings)));
        }
        for check in &self.input.plugin_checks {
            rows.push((check.title.clone(), !settings.disabled_plugin_checks.contains(&check.key)));
        }
        rows
    }

    /// Puts the keyboard in the message field.
    pub fn focus_message(&self, window: &mut Window, cx: &mut App) {
        let focus = self.area.focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// The last commit's message arrived after Amend was checked (`None`: there are no commits):
    /// it replaces the draft, which is kept in the history.
    pub fn amend_message_loaded(&mut self, last: Option<&str>, cx: &mut Context<Self>) {
        let replaced = self.state.update(cx, |state, cx| {
            let replaced = state.amend_loaded(last);
            cx.notify();
            replaced
        });
        if let Some(draft) = replaced.filter(|draft| !draft.trim().is_empty()) {
            cx.emit(CommitBoxEvent::RememberMessage(draft));
        }
    }

    /// Checks or unchecks Amend. Checking asks for the last commit's message; unchecking restores
    /// the draft it replaced, unless the message was edited since.
    pub fn toggle_amend(&mut self, cx: &mut Context<Self>) {
        let load = self.state.update(cx, |state, cx| {
            let on = !state.amend;
            let load = state.set_amend(on);
            state.check_problems.clear();
            cx.notify();
            load
        });
        if load {
            cx.emit(CommitBoxEvent::LoadAmendMessage);
        }
        cx.notify();
    }

    /// Commit (`push`: Commit and Push…; `rebase`: Commit and Rebase). Without a message (and
    /// without Amend) nothing is committed: the field says so and takes the keyboard.
    pub fn commit(&mut self, push: bool, rebase: bool, cx: &mut Context<Self>) {
        if !self.can_commit(cx) || (push && !rebase && self.input.remote_busy) {
            return;
        }
        let (message, amend, sign_off, author, run_hooks, anyway) = {
            let state = self.state.read(cx);
            let author = Some(state.author.trim().to_owned()).filter(|author| !author.is_empty());
            let anyway = !state.check_problems.is_empty();
            (state.message.clone(), state.amend, state.sign_off, author, state.run_hooks, anyway)
        };
        if message.trim().is_empty() && !amend {
            self.empty_error = true;
            self.state.update(cx, |state, cx| {
                state.focus_tick += 1;
                cx.notify();
            });
            cx.notify();
            return;
        }
        let rebase_onto = if rebase { self.fixup(cx) } else { None };
        let options = CommitOptions { amend, sign_off, author, run_hooks, ..CommitOptions::default() };
        // The window clears it with the next input once the commit has ended.
        self.input.busy = true;
        self.popup = None;
        cx.emit(CommitBoxEvent::Commit(Box::new(CommitRequest {
            message,
            options,
            push: push && rebase_onto.is_none(),
            rebase_onto,
            anyway,
        })));
        cx.notify();
    }

    fn clear_problems(&mut self, cx: &mut Context<Self>) {
        if self.state.read(cx).check_problems.is_empty() {
            return;
        }
        self.state.update(cx, |state, cx| {
            state.check_problems.clear();
            cx.notify();
        });
    }

    fn area_event(&mut self, event: &TextAreaEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            TextAreaEvent::Changed => {
                let (mut text, caret) = {
                    let area = self.area.read(cx);
                    (area.text().to_owned(), area.selection().end)
                };
                let settings = &self.input.settings;
                // The area has no caret to restore after a rewrite: a line is wrapped while it is
                // typed at the end of the message, which is where the caret stays.
                if settings.limit_body && settings.wrap_at_margin && caret == text.len() {
                    let chars = text.chars().count();
                    if let Some(wrapped) = wrap_at_margin(&text, chars, settings.body_limit as usize) {
                        text = wrapped;
                        self.area.update(cx, |area, cx| area.set_text(&text, cx));
                    }
                }
                self.empty_error = false;
                self.state.update(cx, |state, cx| {
                    state.message = text;
                    state.check_problems.clear();
                    cx.notify();
                });
                cx.notify();
            }
            TextAreaEvent::Cancelled => {
                if self.popup.take().is_none() {
                    window.focus(&self.focus, cx);
                }
                cx.notify();
            }
            TextAreaEvent::Submitted | TextAreaEvent::Blurred => {}
        }
    }

    /// Follows the shared draft: a message set elsewhere (Amend, a template, a commit) shows in the
    /// field, and the requests to focus the field or open the history are answered.
    fn state_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (message, author, focus_tick, history_tick) = {
            let state = self.state.read(cx);
            (state.message.clone(), state.author.clone(), state.focus_tick, state.history_tick)
        };
        if self.area.read(cx).text() != message {
            self.area.update(cx, |area, cx| area.set_text(&message, cx));
        }
        if self.author.read(cx).text() != author {
            self.author.update(cx, |field, cx| field.set_text(&author, cx));
        }
        if focus_tick != self.seen_focus {
            self.seen_focus = focus_tick;
            self.focus_message(window, cx);
        }
        if history_tick != self.seen_history {
            self.seen_history = history_tick;
            if !self.input.history.is_empty() {
                self.popup = Some(Popup::History { active: 0 });
            }
        }
        cx.notify();
    }

    /// Opens or closes the commit message history.
    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.popup = match self.popup {
            Some(Popup::History { .. }) => None,
            _ if self.input.history.is_empty() => None,
            _ => Some(Popup::History { active: 0 }),
        };
        cx.notify();
    }

    pub fn toggle_options(&mut self, cx: &mut Context<Self>) {
        self.popup = if self.popup == Some(Popup::Options) { None } else { Some(Popup::Options) };
        cx.notify();
    }

    /// Puts the history's message at `index` in the field and the keyboard there.
    pub fn choose_history(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(message) = self.input.history.get(index).cloned() else { return };
        self.popup = None;
        self.empty_error = false;
        self.state.update(cx, |state, cx| {
            state.message = message;
            state.check_problems.clear();
            cx.notify();
        });
        self.focus_message(window, cx);
        cx.notify();
    }

    fn toggle_sign_off(&mut self, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.sign_off = !state.sign_off;
            cx.notify();
        });
    }

    fn toggle_hooks(&mut self, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.run_hooks = !state.run_hooks;
            cx.notify();
        });
    }

    fn toggle_check(&mut self, check: CommitCheck, cx: &mut Context<Self>) {
        cx.emit(CommitBoxEvent::SetCheck { check, on: !check.on(&self.input.settings) });
    }

    fn toggle_plugin_check(&mut self, key: &str, cx: &mut Context<Self>) {
        let on = self.input.settings.disabled_plugin_checks.iter().any(|known| known == key);
        cx.emit(CommitBoxEvent::SetPluginCheck { key: key.to_owned(), on });
    }

    /// Keys of the message field it does not take itself: ⌘↩ (Ctrl+Enter) commits, with ⌥ (Alt)
    /// Commit and Push…; the arrows, Enter and Escape move in an open history.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let primary = modifiers.platform || modifiers.control;
        if self.popup.is_some() && key == "escape" {
            self.popup = None;
            cx.notify();
            return true;
        }
        if let Some(Popup::History { active }) = self.popup {
            let last = self.input.history.len().saturating_sub(1);
            match key {
                "down" => self.popup = Some(Popup::History { active: (active + 1).min(last) }),
                "up" => self.popup = Some(Popup::History { active: active.saturating_sub(1) }),
                "enter" if !primary => self.choose_history(active, window, cx),
                _ => return false,
            }
            cx.notify();
            return true;
        }
        if key == "enter" && primary && !modifiers.shift {
            self.commit(modifiers.alt, false, cx);
            return true;
        }
        false
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.popup.is_some() && self.claim_key(&event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    fn render_bar(&self, cx: &mut Context<Self>) -> Div {
        let amend = self.state.read(cx).amend;
        let has_history = !self.input.history.is_empty();
        let history_open = matches!(self.popup, Some(Popup::History { .. }));
        let label = tx(cx, "Amend");
        let length = self.subject_length(cx);
        div()
            .flex_none()
            .h(px(BAR_HEIGHT))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .child(
                checkbox(cx, label, amend)
                    .id("commit-amend")
                    .debug_selector(|| "commit-amend".to_owned())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_amend(cx))),
            )
            .child(
                icon_button(cx, "history", history_open, has_history)
                    .id("commit-history")
                    .debug_selector(|| "commit-history".to_owned())
                    .when(has_history, |el| {
                        el.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_history(cx)))
                    }),
            )
            .child(div().flex_1())
            .children(
                length.map(|length| div().text_color(theme::color(cx, "warning")).child(SharedString::from(length))),
            )
    }

    fn render_problems(&self, cx: &mut Context<Self>) -> Option<Div> {
        let problems: Vec<CommitProblem> = self.state.read(cx).check_problems.clone();
        if problems.is_empty() {
            return None;
        }
        let (fg, accent) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-link"));
        let rows: Vec<AnyElement> = problems
            .iter()
            .enumerate()
            .map(|(index, problem)| {
                let line = format!("{}: {}", tx(cx, &problem.title), tx(cx, &problem.text));
                let link = problem.details.as_ref().map(|details| {
                    div()
                        .id(("commit-problem", index))
                        .debug_selector(move || format!("commit-problem-{index}"))
                        .text_color(accent)
                        .cursor_pointer()
                        .child(SharedString::from(tx(cx, details)))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(CommitBoxEvent::ProblemDetails(index));
                        }))
                });
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.0))
                    .text_color(fg)
                    .child(SharedString::from(line))
                    .children(link)
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex_none()
                .flex()
                .gap(px(6.0))
                .mx(px(8.0))
                .p(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "bg-banner-warning"))
                .child(theme::icon(cx, "warning", 14.0, theme::color(cx, "warning")))
                .child(div().flex_1().flex().flex_col().gap(px(2.0)).children(rows)),
        )
    }

    fn render_actions(&self, cx: &mut Context<Self>) -> Div {
        let [commit, second] = self.buttons(cx);
        let fixup = self.fixup(cx).is_some();
        let options_open = self.popup == Some(Popup::Options);
        div()
            .flex_none()
            .h(px(ACTIONS_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .child(
                button(cx, &commit.label, true, commit.enabled)
                    .id("commit-commit")
                    .debug_selector(|| "commit-commit".to_owned())
                    .when(commit.enabled, |el| {
                        el.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.commit(false, false, cx)))
                    }),
            )
            .child(
                // In a narrow window this label gives way, so the options button stays in view.
                button(cx, &second.label, false, second.enabled)
                    .flex_shrink(1.0)
                    .min_w_0()
                    .overflow_hidden()
                    .id("commit-second")
                    .debug_selector(|| "commit-second".to_owned())
                    .when(second.enabled, |el| {
                        el.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.commit(!fixup, fixup, cx)))
                    }),
            )
            .children(self.input.progress.clone().map(|progress| {
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(progress))
            }))
            .child(div().flex_1())
            .child(
                icon_button(cx, "settings", options_open, true)
                    .id("commit-options")
                    .debug_selector(|| "commit-options".to_owned())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_options(cx))),
            )
    }

    /// The commit message history, below the bar on the right. A click beside it closes it.
    fn render_history(&self, active: usize, cx: &mut Context<Self>) -> AnyElement {
        let (fg, selected_bg, hover_bg) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let rows: Vec<AnyElement> = self
            .input
            .history
            .iter()
            .enumerate()
            .map(|(index, message)| {
                div()
                    .id(("commit-history-row", index))
                    .debug_selector(move || format!("commit-history-{index}"))
                    .flex_none()
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(fg)
                    .when(active == index, |el| el.bg(selected_bg))
                    .when(active != index, |el| el.hover(|el| el.bg(hover_bg)))
                    .child(SharedString::from(history_label(message)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.choose_history(index, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let list = self.popup_frame(cx).w(px(MENU_WIDTH)).max_h(px(320.0)).overflow_hidden().children(rows);
        self.backdrop("commit-history-backdrop", cx)
            .pt(px(BAR_HEIGHT))
            .child(div().flex_none().flex().justify_end().child(list))
            .into_any_element()
    }

    /// The Commit Options popup, above the gear.
    fn render_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let (sign_off, run_hooks) = {
            let state = self.state.read(cx);
            (state.sign_off, state.run_hooks)
        };
        let settings = self.input.settings.clone();
        let muted = theme::color(cx, "fg-muted");
        let section = |title: String| div().pt(px(6.0)).text_color(muted).child(SharedString::from(title));
        let row = |el: Div, id: &'static str| el.id(id).debug_selector(move || id.to_owned());
        let check = |check: CommitCheck, id: &'static str, cx: &mut Context<Self>| {
            let label = tx(cx, check.label());
            row(checkbox(cx, label, check.on(&settings)), id)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_check(check, cx)))
        };
        let git = tx(cx, "Git");
        let author_label = tx(cx, "Author:");
        let sign_label = tx(cx, "Sign-off commit");
        let checks_title = tx(cx, "Commit Checks");
        let hooks_label = tx(cx, "Run Git hooks");
        let advanced = tx(cx, "Advanced Commit Checks");
        let configure = tx(cx, "Configure");
        let field_border = theme::color(cx, "border-strong");
        let plugin_rows: Vec<AnyElement> = self
            .input
            .plugin_checks
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let on = !settings.disabled_plugin_checks.contains(&plugin.key);
                let key = plugin.key.clone();
                checkbox(cx, plugin.title.clone(), on)
                    .id(("commit-plugin-check", index))
                    .debug_selector(move || format!("commit-plugin-check-{index}"))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_plugin_check(&key, cx)))
                    .into_any_element()
            })
            .collect();
        let panel = self
            .popup_frame(cx)
            .w(px(OPTIONS_WIDTH))
            .gap(px(6.0))
            .p(px(10.0))
            .child(section(git))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().flex_none().child(SharedString::from(author_label)))
                    .child(
                        div()
                            .flex_1()
                            .px(px(6.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .border_1()
                            .border_color(field_border)
                            .debug_selector(|| "commit-author".to_owned())
                            .child(self.author.clone()),
                    ),
            )
            .child(
                row(checkbox(cx, sign_label, sign_off), "commit-sign-off")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_sign_off(cx))),
            )
            .child(section(checks_title))
            .child(check(CommitCheck::Reformat, "commit-check-reformat", cx))
            .child(check(CommitCheck::OptimizeImports, "commit-check-optimize-imports", cx))
            .child(check(CommitCheck::Cleanup, "commit-check-cleanup", cx))
            .child(
                row(checkbox(cx, hooks_label, run_hooks), "commit-run-hooks")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_hooks(cx))),
            )
            .child(section(advanced))
            .child(check(CommitCheck::AnalyzeCode, "commit-check-analyze", cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(check(CommitCheck::CheckTodo, "commit-check-todo", cx))
                    .child(
                        div()
                            .id("commit-configure")
                            .debug_selector(|| "commit-configure".to_owned())
                            .text_color(theme::color(cx, "fg-link"))
                            .cursor_pointer()
                            .child(SharedString::from(configure))
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(CommitBoxEvent::Configure))),
                    ),
            )
            .children(plugin_rows);
        self.backdrop("commit-options-backdrop", cx)
            .justify_end()
            .pb(px(ACTIONS_HEIGHT))
            .child(div().flex_none().flex().justify_end().child(panel))
            .into_any_element()
    }

    fn popup_frame(&self, cx: &App) -> Div {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    /// The layer a popup sits on: it covers the box, and a press on it closes the popup.
    fn backdrop(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        div().id(id).absolute().inset_0().occlude().flex().flex_col().px(px(8.0)).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.popup = None;
                cx.notify();
            }),
        )
    }
}

impl Focusable for CommitBox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CommitBox {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = self.render_bar(cx);
        let inspections = self.inspections(cx);
        let problems = self.render_problems(cx);
        let actions = self.render_actions(cx);
        let popup = match self.popup {
            Some(Popup::History { active }) => Some(self.render_history(active, cx)),
            Some(Popup::Options) => Some(self.render_options(cx)),
            None => None,
        };
        let border = theme::color(cx, if self.empty_error { "danger" } else { "border-strong" });
        let empty = self.empty_error.then(|| {
            let text = tx(cx, "Specify commit message");
            div().flex_none().px(px(8.0)).text_color(theme::color(cx, "danger")).child(SharedString::from(text))
        });
        let muted = theme::color(cx, "fg-muted");
        div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .child(bar)
            .child(
                div()
                    .flex_none()
                    .mx(px(8.0))
                    .px(px(6.0))
                    .py(px(4.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(border)
                    .bg(theme::color(cx, "bg-field"))
                    .debug_selector(|| "commit-message".to_owned())
                    .child(self.area.clone()),
            )
            .children(empty)
            .when(!inspections.is_empty(), |el| {
                let text = SharedString::from(inspections.join(" · "));
                el.child(div().flex_none().px(px(8.0)).text_color(muted).child(text))
            })
            .children(problems)
            .child(actions)
            .children(popup.map(|popup| deferred(popup).with_priority(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<CommitBoxEvent>>>;

    fn input(included: usize) -> CommitBoxInput {
        CommitBoxInput { included, settings: CommitSettings::default(), ..CommitBoxInput::default() }
    }

    fn open(
        cx: &mut TestAppContext,
        state: CommitState,
        box_input: CommitBoxInput,
    ) -> (Entity<CommitBox>, Entity<CommitState>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let state = cx.update(|cx| cx.new(|_| state));
        let shared = state.clone();
        let (view, cx) = cx.add_window_view(|window, cx| CommitBox::new(shared, box_input, window, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &CommitBoxEvent, _| log.borrow_mut().push(event.clone())).detach();
            let area = view.read(cx).message_area().clone();
            let focus = area.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, state, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn buttons(view: &Entity<CommitBox>, cx: &mut VisualTestContext) -> [ButtonState; 2] {
        view.update(cx, |view, cx| view.buttons(cx))
    }

    fn commits(events: &Events) -> Vec<CommitRequest> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                CommitBoxEvent::Commit(request) => Some((**request).clone()),
                _ => None,
            })
            .collect()
    }

    fn message(state: &Entity<CommitState>, cx: &mut VisualTestContext) -> String {
        state.read_with(cx, |state, _| state.message.clone())
    }

    fn area_text(view: &Entity<CommitBox>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| view.message_area().read(cx).text().to_owned())
    }

    // Not from a spec: the buttons follow what is included, Amend, a merge and running work.
    #[gpui_kit::test]
    async fn enables_the_commit_buttons_for_what_can_be_committed(cx: &mut TestAppContext) {
        let (view, state, _, cx) = open(cx, CommitState::default(), input(0));
        let [commit, push] = buttons(&view, cx);
        assert_eq!((commit.label.as_str(), commit.enabled), ("Commit", false));
        assert_eq!((push.label.as_str(), push.enabled), ("Commit and Push…", false));

        view.update(cx, |view, cx| view.set_input(input(2), cx));
        assert!(buttons(&view, cx).iter().all(|button| button.enabled));
        view.update(cx, |view, cx| view.set_input(CommitBoxInput { remote_busy: true, ..input(2) }, cx));
        assert_eq!(buttons(&view, cx).map(|button| button.enabled), [true, false]);
        view.update(cx, |view, cx| view.set_input(CommitBoxInput { busy: true, ..input(2) }, cx));
        assert_eq!(buttons(&view, cx).map(|button| button.enabled), [false, false]);
        // A merge is committed with nothing included.
        view.update(cx, |view, cx| view.set_input(CommitBoxInput { merging: true, ..input(0) }, cx));
        assert!(buttons(&view, cx)[0].enabled);

        view.update(cx, |view, cx| view.set_input(input(0), cx));
        state.update(cx, |state, _| state.amend = true);
        let [commit, push] = buttons(&view, cx);
        assert_eq!((commit.label.as_str(), commit.enabled), ("Amend Commit", true));
        assert_eq!(push.label, "Amend and Push…");
    }

    // Not from a spec: Commit without a message says so at the field instead of committing, and
    // typing takes the error away.
    #[gpui_kit::test]
    async fn reports_a_missing_message_at_the_field(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, CommitState::default(), input(1));
        click(cx, "commit-commit");
        assert!(commits(&events).is_empty());
        assert!(view.read_with(cx, |view, _| view.empty_error()));
        cx.simulate_input("x");
        assert!(!view.read_with(cx, |view, _| view.empty_error()));
    }

    // Not from a spec: ⌘↩ commits the typed message with the options, ⌥⌘↩ commits and pushes.
    #[gpui_kit::test]
    async fn commits_the_typed_message_from_the_keyboard(cx: &mut TestAppContext) {
        let (view, state, events, cx) = open(cx, CommitState::default(), input(1));
        cx.simulate_input("Fix the parser");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("Body");
        assert_eq!(message(&state, cx), "Fix the parser\nBody");
        state.update(cx, |state, _| state.author = "Ada <ada@example.test>".to_owned());
        cx.simulate_keystrokes("cmd-enter");
        let sent = commits(&events);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].message, "Fix the parser\nBody");
        assert!(!sent[0].push && sent[0].rebase_onto.is_none() && !sent[0].anyway);
        assert_eq!(sent[0].options.author.as_deref(), Some("Ada <ada@example.test>"));
        assert!(sent[0].options.run_hooks && !sent[0].options.amend);
        // A running commit is not started twice.
        cx.simulate_keystrokes("cmd-enter");
        assert_eq!(commits(&events).len(), 1);

        view.update(cx, |view, cx| view.set_input(input(1), cx));
        cx.simulate_keystrokes("cmd-alt-enter");
        assert!(commits(&events)[1].push);
    }

    // Not from a spec: Amend puts the last commit's message in the field and gives the draft back.
    #[gpui_kit::test]
    async fn amend_swaps_the_draft_for_the_last_commits_message(cx: &mut TestAppContext) {
        let (view, state, events, cx) = open(cx, CommitState::default(), input(0));
        cx.simulate_input("draft");
        click(cx, "commit-amend");
        assert!(matches!(events.borrow().last(), Some(CommitBoxEvent::LoadAmendMessage)));
        view.update(cx, |view, cx| view.amend_message_loaded(Some("Last commit\n"), cx));
        cx.run_until_parked();
        assert_eq!(area_text(&view, cx), "Last commit");
        assert!(matches!(events.borrow().last(), Some(CommitBoxEvent::RememberMessage(draft)) if draft == "draft"));
        assert!(buttons(&view, cx)[0].enabled, "an amend needs nothing included");
        click(cx, "commit-amend");
        assert_eq!(area_text(&view, cx), "draft");
        assert!(!state.read_with(cx, |state, _| state.amend));
    }

    // Not from a spec: the history lists earlier messages by their first line; choosing one puts it
    // in the field, and Show Commit Message History opens it through the shared state.
    #[gpui_kit::test]
    async fn picks_a_message_from_the_history(cx: &mut TestAppContext) {
        let history = vec!["Older".to_owned(), "Newest subject\n\nwith a body".to_owned()];
        let (view, state, _, cx) = open(cx, CommitState::default(), CommitBoxInput { history, ..input(1) });
        click(cx, "commit-history");
        let lines = view.read_with(cx, |view, _| view.history_lines());
        assert_eq!(lines, Some(vec!["Older".to_owned(), "Newest subject".to_owned()]));
        cx.simulate_keystrokes("down enter");
        assert_eq!(message(&state, cx), "Newest subject\n\nwith a body");
        assert_eq!(area_text(&view, cx), "Newest subject\n\nwith a body");
        assert_eq!(view.read_with(cx, |view, _| view.history_lines()), None);

        state.update(cx, |state, cx| {
            state.history_tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.history_active()), Some(0));
        click(cx, "commit-history-0");
        assert_eq!(message(&state, cx), "Older");
    }

    // Not from a spec: the subject's length shows in the bar, the other inspections below the field.
    #[gpui_kit::test]
    async fn inspects_the_message_as_it_is_typed(cx: &mut TestAppContext) {
        let settings = CommitSettings { subject_limit: 10, ..CommitSettings::default() };
        let (view, _, _, cx) = open(cx, CommitState::default(), CommitBoxInput { settings, ..input(1) });
        cx.simulate_input("A long subject");
        assert_eq!(view.read_with(cx, |view, cx| view.subject_length(cx)).as_deref(), Some("14/10"));
        cx.simulate_keystrokes("enter");
        cx.simulate_input("body");
        let inspections = view.read_with(cx, |view, cx| view.inspections(cx));
        assert_eq!(inspections, ["Missing blank line between subject and body"]);
    }

    // Not from a spec: failed checks turn Commit into Commit Anyway until the draft changes, and
    // their link asks for the details.
    #[gpui_kit::test]
    async fn failed_checks_turn_commit_into_commit_anyway(cx: &mut TestAppContext) {
        let problem = CommitProblem {
            title: "Code analysis".to_owned(),
            text: "2 errors".to_owned(),
            details: Some("Review".to_owned()),
        };
        let mut draft = CommitState::default();
        draft.message = "Fix".to_owned();
        draft.check_problems = vec![problem];
        let (view, state, events, cx) = open(cx, draft, input(1));
        assert_eq!(buttons(&view, cx)[0].label, "Commit Anyway");
        assert_eq!(view.update(cx, |view, cx| view.problems(cx)), ["Code analysis: 2 errors Review"]);
        click(cx, "commit-problem-0");
        assert!(matches!(events.borrow().last(), Some(CommitBoxEvent::ProblemDetails(0))));
        click(cx, "commit-commit");
        assert!(commits(&events)[0].anyway);

        view.update(cx, |view, cx| view.set_input(input(2), cx));
        assert!(state.read_with(cx, |state, _| state.check_problems.is_empty()));
        assert_eq!(buttons(&view, cx)[0].label, "Commit");
    }

    // Not from a spec: after Fixup… the second button commits and rebases onto the target.
    #[gpui_kit::test]
    async fn commits_and_rebases_a_prepared_fixup(cx: &mut TestAppContext) {
        let target: Commit = serde_json::from_value(serde_json::json!({
            "id": "a".repeat(40), "shortId": "aaaaaaaa", "parents": [], "author": "Ada", "email": "ada@example.test",
            "timeMs": 0.0, "summary": "Add parser", "message": "Add parser", "refs": [],
        }))
        .expect("a commit");
        let mut draft = CommitState::default();
        draft.message = "fixup! Add parser".to_owned();
        draft.fixup_target = Some(target);
        let (view, _, events, cx) = open(cx, draft, input(1));
        assert_eq!(buttons(&view, cx)[1].label, "Commit and Rebase");
        click(cx, "commit-second");
        let sent = commits(&events);
        assert_eq!(sent[0].rebase_onto.as_ref().map(|commit| commit.summary.as_str()), Some("Add parser"));
        assert!(!sent[0].push);
    }

    // Not from a spec: the options popup edits the draft's options and asks for the settings.
    #[gpui_kit::test]
    async fn edits_the_commit_options(cx: &mut TestAppContext) {
        let plugin_checks = vec![PluginCheck { key: "p/lint".to_owned(), title: "Lint".to_owned() }];
        let (view, state, events, cx) = open(cx, CommitState::default(), CommitBoxInput { plugin_checks, ..input(1) });
        click(cx, "commit-options");
        assert!(view.read_with(cx, |view, _| view.options_open()));
        click(cx, "commit-sign-off");
        click(cx, "commit-run-hooks");
        assert!(state.read_with(cx, |state, _| state.sign_off && !state.run_hooks));
        click(cx, "commit-check-reformat");
        assert!(matches!(
            events.borrow().last(),
            Some(CommitBoxEvent::SetCheck { check: CommitCheck::Reformat, on: true })
        ));
        click(cx, "commit-plugin-check-0");
        let toggled = events.borrow().last().cloned();
        assert!(matches!(toggled, Some(CommitBoxEvent::SetPluginCheck { key, on: false }) if key == "p/lint"));
        click(cx, "commit-configure");
        assert!(matches!(events.borrow().last(), Some(CommitBoxEvent::Configure)));
        let rows = view.update(cx, |view, cx| view.option_rows(cx));
        assert_eq!(rows[0], ("Sign-off commit".to_owned(), true));
        assert_eq!(rows.last(), Some(&("Lint".to_owned(), true)));
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.options_open()));
    }
}
