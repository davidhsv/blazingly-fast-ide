//! The small dialogs of the Git actions, and the Push dialog.
//!
//! [`PromptDialog`] is one form over the questions [`crate::vcs_ops::dialogs`] describes: a title,
//! a message, a text field with its problem under it, checkboxes and buttons. Its constructors
//! cover New Branch and Rename Branch (whose field cleans up a branch name as it is typed and is
//! checked by its [`BranchPrompt`]), Checkout Tag or Revision, the confirmations of destructive
//! operations and the message boxes with buttons of their own. Enter presses the default button
//! when it can be pressed, Escape cancels.
//!
//! [`PushDialog`] lists the repositories with something to push, each with its outgoing commits,
//! the remote and the target branch, and says which push each one gets.
//!
//! Neither runs anything: the answer goes to the workbench, which carries on with the operation
//! that asked.

use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_more::push_target::{PushChoice, push_operation, split_tracked};
use crate::vcs_ops::branch_ops::BranchPrompt;
use crate::vcs_ops::dialogs::{AskAnswer, AskRequest, ConfirmRequest, PromptRequest, PromptResult};
use crate::vcs_ops::remote_ui::{RepoPush, default_push_checked, is_protected_branch, push_candidates};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Keystroke, MouseButton, Render, SharedString, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::PushTags;
use std::collections::HashSet;
use std::ops::Range;

/// The id of the button that accepts a prompt or a confirmation.
pub const OK: &str = "ok";
/// What a dialog answers when it is closed without a button of its own.
pub const CANCEL: &str = "cancel";
const WIDTH: f32 = 420.0;
const PUSH_WIDTH: f32 = 680.0;
const BUTTON_HEIGHT: f32 = 28.0;
const FIELD_HEIGHT: f32 = 28.0;

/// What a prompt's text must be.
#[derive(Clone, Debug)]
pub enum FieldRule {
    /// Anything but blank; `message` says what is missing.
    NonEmpty(String),
    /// A branch name: cleaned up as it is typed and checked by its prompt, which may offer to
    /// overwrite an existing branch.
    Branch(Box<BranchPrompt>),
}

/// A button of a dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogButton {
    pub id: String,
    /// English source text.
    pub label: String,
    /// The default button: filled, and pressed by Enter.
    pub primary: bool,
    /// It destroys something.
    pub danger: bool,
}

impl DialogButton {
    fn new(id: &str, label: &str, primary: bool, danger: bool) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), primary, danger }
    }
}

/// How a dialog was closed: the button, the text of its field (trimmed), its checkbox and whether
/// the existing branch is to be overwritten.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogAnswer {
    pub button: String,
    pub value: String,
    pub checked: bool,
    pub overwrite: bool,
}

impl DialogAnswer {
    pub fn is_cancel(&self) -> bool {
        self.button == CANCEL
    }

    /// Whether a confirmation was accepted.
    pub fn confirmed(&self) -> bool {
        self.button == OK
    }

    /// What a prompt was closed with; `None` when it was cancelled.
    pub fn prompt_result(&self) -> Option<PromptResult> {
        self.confirmed().then(|| PromptResult {
            value: self.value.clone(),
            checked: self.checked,
            overwrite: self.overwrite,
        })
    }

    /// The answer of a message box.
    pub fn ask_answer(&self) -> AskAnswer {
        AskAnswer::new(&self.button, self.checked)
    }
}

/// What a dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDialogEvent {
    /// The dialog is done: close it and carry on with the answer.
    Closed(DialogAnswer),
}

/// The text field of a prompt.
struct Field {
    entity: Entity<TextField>,
    label: Option<String>,
    mono: bool,
    rule: FieldRule,
    /// The text as last reported by the field.
    value: String,
    /// The user edited the text, or its prefilled value is checked from the start: its problem
    /// shows.
    touched: bool,
}

pub struct PromptDialog {
    title: String,
    message: Option<String>,
    warning: bool,
    width: f32,
    field: Option<Field>,
    /// The checkbox's label and state.
    checkbox: Option<(String, bool)>,
    /// The user chose to overwrite the existing branch of the typed name.
    overwrite: bool,
    buttons: Vec<DialogButton>,
    focus: FocusHandle,
    code_font: SharedString,
    key_claim: u64,
    _field_events: Option<Subscription>,
}

impl EventEmitter<PromptDialogEvent> for PromptDialog {}

/// The first font of the theme's `--font-mono` list that is installed: names and revisions are code.
pub(super) fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// The smallest edit that turns `old` into `new`: the UTF-16 range of `old` it replaces and the text
/// that goes there. Replacing only that keeps the caret after the rewritten characters.
fn minimal_edit(old: &str, new: &str) -> (Range<usize>, String) {
    let prefix: usize = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix: usize =
        old_rest.chars().rev().zip(new_rest.chars().rev()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
    let removed = &old[prefix..old.len() - suffix];
    let inserted = &new[prefix..new.len() - suffix];
    let start = old[..prefix].encode_utf16().count();
    (start..start + removed.encode_utf16().count(), inserted.to_owned())
}

/// A check box with its label.
pub(super) fn checkbox(cx: &App, label: String, on: bool) -> Div {
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

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
pub(super) fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {
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
        let fill = theme::color(cx, if danger { "danger" } else { "bg-button-primary" });
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

/// The frame a text field is drawn in.
pub(super) fn field_frame(cx: &App, invalid: bool) -> Div {
    div()
        .h(px(FIELD_HEIGHT))
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if invalid { "danger" } else { "border" }))
        .bg(theme::color(cx, "bg-input"))
}

/// `text` in the active locale.
pub(super) fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The dialog a constructor describes.
/// A message box's texts, in the UI language, for [`PromptDialog::message`].
pub struct Message {
    pub title: String,
    pub body: String,
    pub warning: bool,
    /// The "Don't ask again" checkbox, unchecked.
    pub checkbox: Option<String>,
    pub buttons: Vec<DialogButton>,
}

struct Spec {
    title: String,
    message: Option<String>,
    warning: bool,
    width: f32,
    field: Option<(PromptRequest, FieldRule)>,
    checkbox: Option<(String, bool)>,
    buttons: Vec<DialogButton>,
}

impl PromptDialog {
    /// A one-line prompt for `request`, its text checked by `rule`.
    pub fn prompt(request: PromptRequest, rule: FieldRule, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ok = if request.ok_label.is_empty() { "OK".to_owned() } else { request.ok_label.clone() };
        let buttons = vec![DialogButton::new(OK, &ok, true, false), DialogButton::new(CANCEL, "Cancel", false, false)];
        let checkbox = request.checkbox.as_ref().map(|checkbox| (checkbox.label.clone(), checkbox.initial));
        let spec = Spec {
            title: request.title.clone(),
            message: None,
            warning: false,
            width: WIDTH,
            checkbox,
            field: Some((request, rule)),
            buttons,
        };
        Self::build(spec, window, cx)
    }

    /// A prompt for a branch name: New Branch ([`crate::vcs_ops::branch_ops::new_branch_prompt`],
    /// with Checkout branch, and Overwrite existing branch when the name is taken) or Rename Branch
    /// ([`crate::vcs_ops::branch_ops::rename_branch_prompt`]).
    pub fn branch_name(prompt: BranchPrompt, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request = prompt.request.clone();
        Self::prompt(request, FieldRule::Branch(Box::new(prompt)), window, cx)
    }

    /// New Branch, from [`crate::vcs_ops::branch_ops::new_branch_prompt`] or
    /// [`crate::vcs_ops::branch_ops::new_branch_in_repos_prompt`].
    pub fn new_branch(prompt: BranchPrompt, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::branch_name(prompt, window, cx)
    }

    /// Rename Branch, from [`crate::vcs_ops::branch_ops::rename_branch_prompt`].
    pub fn rename_branch(prompt: BranchPrompt, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::branch_name(prompt, window, cx)
    }

    /// Checkout Tag or Revision: a tag, a branch or a commit to check out.
    pub fn checkout_revision(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let request = PromptRequest {
            title: "Checkout Tag or Revision".to_owned(),
            label: Some("Enter a tag, branch or commit hash:".to_owned()),
            ok_label: "Checkout".to_owned(),
            mono: true,
            ..PromptRequest::default()
        };
        Self::prompt(request, FieldRule::NonEmpty("Enter a revision".to_owned()), window, cx)
    }

    /// A yes / no question: the confirmation of Delete Branch (a worktree holding it, or several
    /// repositories), Delete Remote Branch, and the other destructive operations.
    pub fn confirm(request: ConfirmRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spec = Spec {
            buttons: vec![
                DialogButton::new(OK, &request.ok_label, true, request.danger),
                DialogButton::new(CANCEL, "Cancel", false, false),
            ],
            title: request.title,
            message: Some(request.message),
            warning: request.danger,
            width: WIDTH,
            field: None,
            checkbox: None,
        };
        Self::build(spec, window, cx)
    }

    /// A message box with its own buttons; Escape answers [`CANCEL`].
    pub fn ask(request: AskRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let buttons: Vec<DialogButton> = request
            .buttons
            .iter()
            .map(|button| DialogButton::new(button.id, button.label, button.primary, false))
            .collect();
        let spec = Spec {
            buttons,
            title: request.title,
            message: Some(request.body),
            warning: request.warning,
            width: request.width.map_or(WIDTH, |width| width as f32),
            field: None,
            checkbox: request.checkbox.map(|label| (label, false)),
        };
        Self::build(spec, window, cx)
    }

    /// A message box whose title, message, checkbox and buttons are in the UI language already
    /// (the application shell's close and open questions); Escape answers [`CANCEL`].
    pub fn message(message: Message, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spec = Spec {
            title: message.title,
            message: Some(message.body),
            warning: message.warning,
            width: WIDTH,
            field: None,
            checkbox: message.checkbox.map(|label| (label, false)),
            buttons: message.buttons,
        };
        Self::build(spec, window, cx)
    }

    fn build(spec: Spec, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = spec.field.map(|(request, rule)| {
            let entity = cx.new(|cx| {
                let mut field = TextField::new(cx);
                field.set_text(&request.initial, cx);
                field
            });
            let touched = request.validate_initial && !request.initial.is_empty();
            Field { entity, label: request.label, mono: request.mono, rule, value: request.initial, touched }
        });
        let events = field.as_ref().map(|field| {
            cx.subscribe_in(&field.entity, window, |dialog: &mut Self, entity, event: &TextFieldEvent, window, cx| {
                match event {
                    TextFieldEvent::Changed => dialog.field_changed(entity, window, cx),
                    TextFieldEvent::Submitted => dialog.press_default(cx),
                    TextFieldEvent::Cancelled => dialog.close(CANCEL, cx),
                }
            })
        });
        let focus = match &field {
            Some(field) => field.entity.focus_handle(cx),
            None => cx.focus_handle(),
        };
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self {
            title: spec.title,
            message: spec.message,
            warning: spec.warning,
            width: spec.width,
            field,
            checkbox: spec.checkbox,
            overwrite: false,
            buttons: spec.buttons,
            focus,
            code_font: code_font(cx),
            key_claim,
            _field_events: events,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn buttons(&self) -> &[DialogButton] {
        &self.buttons
    }

    /// The text field, when the dialog has one.
    pub fn field(&self) -> Option<&Entity<TextField>> {
        self.field.as_ref().map(|field| &field.entity)
    }

    /// The checkbox's state.
    pub fn checked(&self) -> Option<bool> {
        self.checkbox.as_ref().map(|(_, on)| *on)
    }

    /// Whether Overwrite existing branch is offered: the typed name is that of an existing branch.
    pub fn offers_overwrite(&self) -> bool {
        match &self.field {
            Some(Field { rule: FieldRule::Branch(prompt), value, .. }) => prompt.overwrites_branch(value),
            _ => false,
        }
    }

    /// The problem of the field's text, in English, whether or not it shows yet.
    fn problem(&self, i18n: &mut I18n) -> Option<String> {
        let field = self.field.as_ref()?;
        match &field.rule {
            FieldRule::NonEmpty(message) => field.value.trim().is_empty().then(|| message.clone()),
            FieldRule::Branch(prompt) => prompt.validate(&field.value, self.offers_overwrite() && self.overwrite, i18n),
        }
    }

    /// The problems shown under the field, in the UI language: the field's, once it is edited (or
    /// its prefilled value is checked) and while it is not empty.
    pub fn errors(&self, cx: &mut App) -> Vec<String> {
        if !self.field.as_ref().is_some_and(|field| field.touched && !field.value.is_empty()) {
            return Vec::new();
        }
        let i18n = &mut cx.default_global::<Localization>().0;
        self.problem(i18n).map(|problem| i18n.tx(&problem)).into_iter().collect()
    }

    /// Whether the default button can be pressed: the field's text has no problem.
    pub fn can_confirm(&self, cx: &mut App) -> bool {
        self.problem(&mut cx.default_global::<Localization>().0).is_none()
    }

    /// The field's text changed: a branch name is cleaned up in place (the caret stays after what
    /// was rewritten), the choice to overwrite goes with the name it was made for.
    fn field_changed(&mut self, entity: &Entity<TextField>, window: &mut Window, cx: &mut Context<Self>) {
        let text = entity.read(cx).text().to_owned();
        let Some(field) = &mut self.field else { return };
        if let FieldRule::Branch(prompt) = &field.rule {
            let clean = prompt.sanitize(&text);
            if clean != text {
                let (range, inserted) = minimal_edit(&text, &clean);
                // The field reports the cleaned text in turn.
                entity.update(cx, |field, cx| field.replace_text_in_range(Some(range), &inserted, window, cx));
                return;
            }
        }
        if text.trim() != field.value.trim() {
            self.overwrite = false;
        }
        field.value = text;
        field.touched = true;
        cx.notify();
    }

    fn toggle_checkbox(&mut self, cx: &mut Context<Self>) {
        if let Some((_, on)) = &mut self.checkbox {
            *on = !*on;
            cx.notify();
        }
    }

    fn toggle_overwrite(&mut self, cx: &mut Context<Self>) {
        self.overwrite = !self.overwrite;
        cx.notify();
    }

    /// Enter: presses the default button, when it can be pressed; otherwise the problem shows.
    fn press_default(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.buttons.iter().find(|button| button.primary).map(|button| button.id.clone()) else {
            return;
        };
        self.press(&id, cx);
    }

    fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == OK && !self.can_confirm(cx) {
            if let Some(field) = &mut self.field {
                field.touched = true;
            }
            cx.notify();
            return;
        }
        self.close(id, cx);
    }

    fn close(&mut self, button: &str, cx: &mut Context<Self>) {
        let answer = DialogAnswer {
            button: button.to_owned(),
            value: self.field.as_ref().map(|field| field.value.trim().to_owned()).unwrap_or_default(),
            checked: self.checked() == Some(true),
            overwrite: self.offers_overwrite() && self.overwrite,
        };
        cx.emit(PromptDialogEvent::Closed(answer));
    }

    /// Enter and Escape are the dialog's own, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.press_default(cx),
            "escape" => self.close(CANCEL, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for PromptDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PromptDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let danger = theme::color(cx, "danger");
        let title = tx(cx, &self.title);
        let errors = self.errors(cx);
        let can_confirm = self.can_confirm(cx);
        let message = self.message.clone().map(|message| {
            let message = tx(cx, &message);
            let icon = self.warning.then(|| theme::icon(cx, "warning", 16.0, theme::color(cx, "warning")));
            div().flex().items_start().gap(px(8.0)).children(icon).child(
                div().flex_1().min_w_0().flex().flex_col().children(
                    message.lines().map(|line| div().min_h(px(16.0)).child(SharedString::from(line.to_owned()))),
                ),
            )
        });
        let field = self.field.as_ref().map(|field| {
            let label = field.label.as_ref().map(|label| {
                let label = cx.default_global::<Localization>().0.tx(label);
                div().text_color(muted).child(SharedString::from(label))
            });
            let frame = field_frame(cx, !errors.is_empty())
                .when(field.mono, |el| el.font_family(self.code_font.clone()))
                .child(div().flex_1().min_w_0().child(field.entity.clone()));
            let problems = errors.iter().map(|error| {
                let small = theme::length(cx, "fs-small");
                div().text_color(danger).text_size(small).child(SharedString::from(error.clone()))
            });
            div().flex().flex_col().gap(px(6.0)).children(label).child(frame).children(problems.collect::<Vec<_>>())
        });
        let check = self.checkbox.clone().map(|(label, on)| {
            let label = tx(cx, &label);
            checkbox(cx, label, on)
                .id("prompt-checkbox")
                .debug_selector(|| "prompt-checkbox".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_checkbox(cx)))
        });
        let overwrite = self.offers_overwrite().then(|| {
            let label = tx(cx, "Overwrite existing branch");
            checkbox(cx, label, self.overwrite)
                .id("prompt-overwrite")
                .debug_selector(|| "prompt-overwrite".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_overwrite(cx)))
        });
        let buttons: Vec<_> = self
            .buttons
            .clone()
            .into_iter()
            .map(|spec| {
                let enabled = spec.id != OK || can_confirm;
                let label = tx(cx, &spec.label);
                let id = spec.id.clone();
                button(cx, label, spec.primary, spec.danger, enabled)
                    .id(SharedString::from(format!("prompt-button-{}", spec.id)))
                    .debug_selector(|| format!("prompt-button-{}", spec.id))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(&id, cx)))
            })
            .collect();
        let has_field = self.field.is_some();
        let focus = self.focus.clone();
        div()
            .w(px(self.width))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .when(!has_field, |el| el.track_focus(&focus))
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .children(message)
                    .children(field)
                    .children(check)
                    .children(overwrite),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons))
    }
}

/// A repository offered by the Push dialog.
#[derive(Clone, Debug)]
pub struct PushRepo {
    pub push: RepoPush,
    /// Its name, as the version control views show it.
    pub name: String,
    /// The names of its remotes.
    pub remotes: Vec<String>,
}

/// What the Push dialog shows.
#[derive(Clone, Debug, Default)]
pub struct PushDialogInput {
    /// The project's repositories with their branch and outgoing commits; those with nothing to
    /// push are left out.
    pub repos: Vec<PushRepo>,
    /// The selected repository: a new branch is checked there only.
    pub selected: Option<String>,
    /// A repository has a pre-push hook: Run Git hooks is offered.
    pub has_hook: bool,
    /// Settings › Git › Protected branches: a force push to them is not made.
    pub protected_branches: String,
    /// Push All up to Here…: the commit the selected repository's branch is pushed up to.
    pub revision: Option<String>,
}

/// One push the dialog asks for.
#[derive(Clone)]
pub struct PushRequest {
    pub root: String,
    pub op: RemoteOp,
    /// What the Git console and the progress say of it.
    pub detail: String,
    /// The plain push to the upstream: a rejection is offered an update and a retry
    /// ([`crate::vcs_more::push_rejected::AutoUpdatePush`]); a push by name is run as it is.
    pub auto_update: bool,
}

/// What the Push dialog asks of the workbench.
#[derive(Clone)]
pub enum PushDialogEvent {
    /// Push these, in this order, each even when another fails; then report the ones that failed.
    Push(Vec<PushRequest>),
    Cancelled,
}

struct PushRow {
    repo: PushRepo,
    checked: bool,
    /// The remote picked in the dialog; `None` keeps the tracked one.
    remote: Option<String>,
    target: Entity<TextField>,
    /// The list of remotes is open.
    picking: bool,
}

impl PushRow {
    fn branch(&self) -> &ide_vcs::BranchInfo {
        &self.repo.push.branch
    }

    fn upstream(&self) -> Option<&str> {
        self.branch().upstream.as_deref().filter(|upstream| !upstream.is_empty())
    }

    fn head(&self) -> &str {
        self.branch().head.as_deref().unwrap_or_default()
    }

    /// The branch pushed to when the target field is left as it is.
    fn default_target(&self) -> String {
        self.upstream().map_or_else(|| self.head().to_owned(), |upstream| split_tracked(upstream).1.to_owned())
    }

    /// The remote shown: the one picked, the tracked one, else `origin` or the first.
    fn shown_remote(&self) -> Option<String> {
        let remotes = &self.repo.remotes;
        self.remote
            .clone()
            .or_else(|| self.upstream().map(|upstream| split_tracked(upstream).0.to_owned()))
            .or_else(|| remotes.iter().find(|remote| *remote == "origin").or(remotes.first()).cloned())
    }
}

pub struct PushDialog {
    rows: Vec<PushRow>,
    has_hook: bool,
    protected: String,
    /// The repository pushed only up to a commit, and that commit.
    revision: Option<(String, String)>,
    force: bool,
    run_hooks: bool,
    push_tags: bool,
    tags: PushTags,
    focus: FocusHandle,
    code_font: SharedString,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PushDialogEvent> for PushDialog {}

impl PushDialog {
    /// The dialog over `input`, with the keyboard in the first target branch field.
    pub fn new(input: PushDialogInput, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pushes: Vec<RepoPush> = input.repos.iter().map(|repo| repo.push.clone()).collect();
        let offered: HashSet<String> = push_candidates(&pushes).iter().map(|repo| repo.root.clone()).collect();
        let checked: HashSet<String> =
            default_push_checked(&pushes, input.selected.as_deref()).into_iter().map(str::to_owned).collect();
        let mut subscriptions = Vec::new();
        let rows: Vec<PushRow> =
            input
                .repos
                .into_iter()
                .filter(|repo| offered.contains(&repo.push.root))
                .map(|repo| {
                    let target = cx.new(TextField::new);
                    let row = PushRow {
                        checked: checked.contains(&repo.push.root),
                        repo,
                        remote: None,
                        target: target.clone(),
                        picking: false,
                    };
                    let initial = row.default_target();
                    target.update(cx, |field, cx| field.set_text(&initial, cx));
                    let events =
                        cx.subscribe_in(&target, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| {
                            match event {
                                TextFieldEvent::Changed => cx.notify(),
                                TextFieldEvent::Submitted => dialog.push(cx),
                                TextFieldEvent::Cancelled => cx.emit(PushDialogEvent::Cancelled),
                            }
                        });
                    subscriptions.push(events);
                    row
                })
                .collect();
        let focus = rows.first().map_or_else(|| cx.focus_handle(), |row| row.target.focus_handle(cx));
        let mut handles: Vec<FocusHandle> = rows.iter().map(|row| row.target.focus_handle(cx)).collect();
        if rows.is_empty() {
            handles.push(focus.clone());
        }
        let key_claims = handles
            .into_iter()
            .map(|handle| {
                let claimant = cx.weak_entity();
                input::claim_keys(
                    handle,
                    move |keystroke, _, cx| {
                        claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false)
                    },
                    cx,
                )
            })
            .collect();
        cx.on_release(|dialog, cx| {
            for claim in &dialog.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        Self {
            rows,
            has_hook: input.has_hook,
            protected: input.protected_branches,
            revision: input.selected.zip(input.revision),
            force: false,
            run_hooks: true,
            push_tags: false,
            tags: PushTags::All,
            focus,
            code_font: code_font(cx),
            key_claims,
            _subscriptions: subscriptions,
        }
    }

    /// The roots of the repositories listed, in order.
    pub fn repos(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.repo.push.root.clone()).collect()
    }

    /// The roots of the repositories checked to be pushed.
    pub fn checked(&self) -> Vec<String> {
        self.rows.iter().filter(|row| row.checked).map(|row| row.repo.push.root.clone()).collect()
    }

    /// The outgoing commits listed under the repository at `index`: short id and summary.
    pub fn commits(&self, index: usize) -> Vec<String> {
        let Some(row) = self.rows.get(index) else { return Vec::new() };
        row.repo.push.commits.iter().map(|commit| format!("{} {}", commit.short_id, commit.summary)).collect()
    }

    /// The target branch field of the repository listed at `index`.
    pub fn target_field(&self, index: usize) -> Option<&Entity<TextField>> {
        self.rows.get(index).map(|row| &row.target)
    }

    /// The remotes the repository at `index` offers to pick from: only one with several does.
    pub fn remote_choices(&self, index: usize) -> &[String] {
        self.rows.get(index).map(|row| row.repo.remotes.as_slice()).filter(|remotes| remotes.len() > 1).unwrap_or(&[])
    }

    /// The default button's label, in English.
    pub fn button_label(&self) -> &'static str {
        if self.force { "Force Push" } else { "Push" }
    }

    /// Whether anything is checked to be pushed.
    pub fn can_push(&self) -> bool {
        self.rows.iter().any(|row| row.checked)
    }

    /// Push tags is offered: no checked repository is pushed by name (which carries no tags).
    pub fn shows_push_tags(&self, cx: &App) -> bool {
        !self.retargeted(cx)
    }

    fn choice(&self, row: &PushRow, tags: Option<PushTags>, cx: &App) -> PushChoice {
        let target = row.target.read(cx).text().to_owned();
        let named = if target.trim().is_empty() { row.default_target() } else { target.trim().to_owned() };
        PushChoice {
            head: row.head().to_owned(),
            tracked: row.upstream().map(str::to_owned),
            remote: row.remote.clone(),
            target,
            explicit: false,
            revision: self.revision.as_ref().filter(|(root, _)| *root == row.repo.push.root).map(|(_, id)| id.clone()),
            force_with_lease: self.force && !is_protected_branch(&named, &self.protected),
            set_upstream: false,
            tags,
            no_verify: self.has_hook && !self.run_hooks,
        }
    }

    /// Whether a checked repository is pushed elsewhere than to its upstream.
    fn retargeted(&self, cx: &App) -> bool {
        self.rows
            .iter()
            .filter(|row| row.checked)
            .any(|row| matches!(push_operation(&self.choice(row, None, cx)), RemoteOp::PushBranch { .. }))
    }

    /// Push: the push of each checked repository, with tags when they go.
    fn push(&mut self, cx: &mut Context<Self>) {
        if !self.can_push() {
            return;
        }
        let tags = (self.push_tags && !self.retargeted(cx)).then_some(self.tags);
        let requests = self
            .rows
            .iter()
            .filter(|row| row.checked)
            .map(|row| {
                let op = push_operation(&self.choice(row, tags, cx));
                let (name, head) = (&row.repo.name, row.head());
                let auto_update = matches!(op, RemoteOp::Push { .. });
                let detail = if auto_update {
                    format!("{name}: {head} → {}", row.upstream().unwrap_or("new remote branch"))
                } else {
                    let remote = row.shown_remote().map(|remote| format!("{remote}/")).unwrap_or_default();
                    let target = row.target.read(cx).text().trim().to_owned();
                    let target = if target.is_empty() { row.default_target() } else { target };
                    format!("{name}: {head} → {remote}{target}")
                };
                PushRequest { root: row.repo.push.root.clone(), op, detail, auto_update }
            })
            .collect();
        cx.emit(PushDialogEvent::Push(requests));
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.push(cx),
            "escape" if self.rows.iter().any(|row| row.picking) => {
                self.rows.iter_mut().for_each(|row| row.picking = false);
                cx.notify();
            }
            "escape" => cx.emit(PushDialogEvent::Cancelled),
            _ => return false,
        }
        true
    }

    fn render_row(&self, index: usize, row: &PushRow, cx: &mut Context<Self>) -> Div {
        let (muted, subtle) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let font = self.code_font.clone();
        let changed = row.checked && matches!(push_operation(&self.choice(row, None, cx)), RemoteOp::PushBranch { .. });
        let check = checkbox(cx, row.repo.name.clone(), row.checked)
            .id(SharedString::from(format!("push-check-{index}")))
            .debug_selector(|| format!("push-check-{index}"))
            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                if let Some(row) = dialog.rows.get_mut(index) {
                    row.checked = !row.checked;
                    cx.notify();
                }
            }));
        let remote = row.shown_remote().map(|remote| {
            let label = div().text_color(muted).child(SharedString::from(remote.clone()));
            if row.repo.remotes.len() < 2 {
                return label.into_any_element();
            }
            let options: Vec<_> = row
                .repo
                .remotes
                .iter()
                .map(|name| {
                    let chosen = name.clone();
                    div()
                        .id(SharedString::from(format!("push-remote-{index}-{name}")))
                        .debug_selector(|| format!("push-remote-{index}-{name}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .text_color(theme::color(cx, "fg-strong"))
                        .when(*name == remote, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .child(SharedString::from(name.clone()))
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                            if let Some(row) = dialog.rows.get_mut(index) {
                                row.remote = Some(chosen.clone());
                                row.picking = false;
                                cx.notify();
                            }
                        }))
                })
                .collect();
            let list = row.picking.then(|| {
                deferred(
                    div()
                        .absolute()
                        .top(px(24.0))
                        .left(px(0.0))
                        .min_w(px(120.0))
                        .p(px(4.0))
                        .flex()
                        .flex_col()
                        .bg(theme::color(cx, "bg-popup"))
                        .border_1()
                        .border_color(theme::color(cx, "border-popup"))
                        .rounded(theme::length(cx, "radius-popup"))
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(options),
                )
                .with_priority(1)
            });
            div()
                .id(SharedString::from(format!("push-remote-{index}")))
                .debug_selector(|| format!("push-remote-{index}"))
                .relative()
                .flex()
                .items_center()
                .gap(px(2.0))
                .px(px(4.0))
                .rounded(px(4.0))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(label)
                .child(theme::icon(cx, "chevron-down", 12.0, subtle))
                .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                    if let Some(row) = dialog.rows.get_mut(index) {
                        row.picking = !row.picking;
                        cx.notify();
                    }
                }))
                .children(list)
                .into_any_element()
        });
        let has_remote = remote.is_some();
        let target = field_frame(cx, false)
            .w(px(180.0))
            .font_family(font.clone())
            .when(changed, |el| el.border_color(theme::color(cx, "accent")))
            .child(div().flex_1().min_w_0().child(row.target.clone()));
        let new_branch = row.upstream().is_none().then(|| {
            let label = tx(cx, "new remote branch");
            div().text_color(muted).child(SharedString::from(label))
        });
        let commits = row.repo.push.commits.iter().map(|commit| {
            div()
                .h(px(22.0))
                .pl(px(24.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .text_color(muted)
                        .font_family(font.clone())
                        .child(SharedString::from(commit.short_id.clone())),
                )
                .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(commit.summary.clone())))
                .child(div().flex_none().text_color(subtle).child(SharedString::from(commit.author.clone())))
        });
        let commits: Vec<_> = commits.collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(check)
                    .child(div().text_color(muted).child(SharedString::from(format!("{} →", row.head()))))
                    .children(remote)
                    .when(has_remote, |el| el.child(div().text_color(subtle).child("/")))
                    .child(target)
                    .children(new_branch),
            )
            .children(commits)
    }
}

impl Focusable for PushDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PushDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Push Commits");
        let rows: Vec<Div> = self.rows.iter().enumerate().map(|(index, row)| self.render_row(index, row, cx)).collect();
        let body = if rows.is_empty() {
            let text = tx(cx, "Everything is up to date");
            div()
                .h(px(72.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(text))
        } else {
            div().flex().flex_col().gap(px(4.0)).children(rows)
        };
        let toggle = |id: &'static str, label: &str, on: bool, cx: &mut Context<Self>| {
            let label = tx(cx, label);
            checkbox(cx, label, on).id(id).debug_selector(|| id.to_owned())
        };
        let force = toggle("push-force", "Force (with lease)", self.force, cx).on_click(cx.listener(
            |dialog, _: &ClickEvent, _, cx| {
                dialog.force = !dialog.force;
                cx.notify();
            },
        ));
        let hooks = self.has_hook.then(|| {
            toggle("push-hooks", "Run Git hooks", self.run_hooks, cx).on_click(cx.listener(
                |dialog, _: &ClickEvent, _, cx| {
                    dialog.run_hooks = !dialog.run_hooks;
                    cx.notify();
                },
            ))
        });
        let tags = self.shows_push_tags(cx).then(|| {
            let mode = match self.tags {
                PushTags::All => "All",
                PushTags::CurrentBranch => "Current Branch",
            };
            let mode = tx(cx, mode);
            let push_tags = self.push_tags;
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(toggle("push-tags", "Push tags:", self.push_tags, cx).on_click(cx.listener(
                    |dialog, _: &ClickEvent, _, cx| {
                        dialog.push_tags = !dialog.push_tags;
                        cx.notify();
                    },
                )))
                .child(
                    div()
                        .id("push-tags-mode")
                        .debug_selector(|| "push-tags-mode".to_owned())
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .text_color(theme::color(cx, if push_tags { "fg-strong" } else { "fg-faint" }))
                        .child(SharedString::from(mode))
                        .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| {
                            if dialog.push_tags {
                                dialog.tags = match dialog.tags {
                                    PushTags::All => PushTags::CurrentBranch,
                                    PushTags::CurrentBranch => PushTags::All,
                                };
                                cx.notify();
                            }
                        })),
                )
        });
        let can_push = self.can_push();
        let push_label = tx(cx, self.button_label());
        let push = button(cx, push_label, true, false, can_push)
            .id("push-button-push")
            .debug_selector(|| "push-button-push".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.push(cx)));
        let cancel_label = tx(cx, "Cancel");
        let cancel = button(cx, cancel_label, false, false, true)
            .id("push-button-cancel")
            .debug_selector(|| "push-button-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(PushDialogEvent::Cancelled)));
        let focus = self.focus.clone();
        div()
            .w(px(PUSH_WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .when(self.rows.is_empty(), |el| el.track_focus(&focus))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(div().px(px(16.0)).py(px(4.0)).child(body))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(12.0))
                    .child(force)
                    .children(hooks)
                    .children(tags)
                    .child(div().flex_1())
                    .child(push)
                    .child(cancel),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_ops::branch_ops::{new_branch_prompt, rename_branch_prompt};
    use crate::vcs_ops::dialogs::{AskButton, PromptCheckbox};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_vcs::{Branch, BranchInfo, Commit};
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<DialogAnswer>>>;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn branch(name: &str) -> Branch {
        let wire =
            json!({ "name": name, "remote": false, "current": false, "ahead": 0, "behind": 0, "lastCommitMs": 0 });
        serde_json::from_value(wire).expect("a branch in its wire form")
    }

    /// The dialog `build` makes, with the keyboard, and the answers it gives.
    fn open(
        cx: &mut TestAppContext,
        build: impl FnOnce(&mut Window, &mut Context<PromptDialog>) -> PromptDialog + 'static,
    ) -> (Entity<PromptDialog>, Answers, &mut VisualTestContext) {
        init(cx);
        let (dialog, cx) = cx.add_window_view(build);
        let answers: Answers = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &PromptDialogEvent, _| {
                let PromptDialogEvent::Closed(answer) = event;
                log.borrow_mut().push(answer.clone());
            })
            .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, answers, cx)
    }

    /// The branch name prompt of the spec: Create New Branch with no checkbox.
    fn branch_prompt(initial: &str) -> BranchPrompt {
        let mut prompt = new_branch_prompt(None, Some(initial), &[], &mut I18n::default());
        prompt.request.checkbox = None;
        prompt
    }

    fn text(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> String {
        let shown = dialog.read_with(cx, |dialog, cx| dialog.field().map(|field| field.read(cx).text().to_owned()));
        shown.unwrap_or_default()
    }

    fn errors(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.update(cx, |dialog, cx| dialog.errors(cx))
    }

    fn can_confirm(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> bool {
        dialog.update(cx, |dialog, cx| dialog.can_confirm(cx))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    // src/features/vcs/__tests__/promptDialog.test.tsx › prompt dialog with a sanitizer (branch name) › replaces a
    // typed space and keeps Create enabled
    #[gpui_kit::test]
    async fn replaces_a_typed_space_and_keeps_create_enabled(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, |window, cx| PromptDialog::new_branch(branch_prompt(""), window, cx));
        cx.simulate_input("fix login  redirect");
        assert_eq!(text(&dialog, cx), "fix-login-redirect");
        assert!(errors(&dialog, cx).is_empty());
        assert!(can_confirm(&dialog, cx));
        click(cx, "prompt-button-ok");
        let answer = answers.borrow().last().cloned().expect("the dialog answered");
        let expected = PromptResult { value: "fix-login-redirect".to_owned(), checked: false, overwrite: false };
        assert_eq!(answer.prompt_result(), Some(expected));
    }

    // src/features/vcs/__tests__/promptDialog.test.tsx › prompt dialog with a sanitizer (branch name) › converts spaces
    // in pasted text
    #[gpui_kit::test]
    async fn converts_spaces_in_pasted_text(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, |window, cx| PromptDialog::new_branch(branch_prompt(""), window, cx));
        let field = dialog.read_with(cx, |dialog, _| dialog.field().cloned()).expect("the prompt has a field");
        cx.update(|window, cx| {
            field.update(cx, |field, cx| field.replace_text_in_range(None, "feature/add new thing", window, cx));
        });
        cx.run_until_parked();
        assert_eq!(text(&dialog, cx), "feature/add-new-thing");
        assert!(can_confirm(&dialog, cx));
    }

    // src/features/vcs/__tests__/promptDialog.test.tsx › prompt dialog with a sanitizer (branch name) › keeps the caret
    // after the rewritten character when editing mid-text
    #[gpui_kit::test]
    async fn keeps_the_caret_after_the_rewritten_character_when_editing_mid_text(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, |window, cx| PromptDialog::new_branch(branch_prompt("ab"), window, cx));
        cx.simulate_keystrokes("left space");
        assert_eq!(text(&dialog, cx), "a-b");
        let field = dialog.read_with(cx, |dialog, _| dialog.field().cloned()).expect("the prompt has a field");
        assert_eq!(field.read_with(cx, |field, _| field.selection()), 2..2);
        cx.simulate_input("x");
        assert_eq!(text(&dialog, cx), "a-xb");
    }

    // Not from a spec: a taken name shows its problem from the start, Overwrite existing branch
    // lifts it, and the answer carries the checkbox and the overwrite.
    #[gpui_kit::test]
    async fn offers_to_overwrite_a_taken_branch_name(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, |window, cx| {
            let prompt = new_branch_prompt(None, Some("main"), &[branch("main")], &mut I18n::default());
            PromptDialog::new_branch(prompt, window, cx)
        });
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Create New Branch");
        assert_eq!(errors(&dialog, cx), ["Branch 'main' already exists"]);
        assert!(!can_confirm(&dialog, cx));
        assert!(dialog.read_with(cx, |dialog, _| dialog.offers_overwrite()));
        cx.simulate_keystrokes("enter");
        assert!(answers.borrow().is_empty(), "Enter does nothing while the name is taken");
        click(cx, "prompt-overwrite");
        assert!(errors(&dialog, cx).is_empty() && can_confirm(&dialog, cx));
        cx.simulate_keystrokes("enter");
        let answer = answers.borrow().last().cloned().expect("the dialog answered");
        let expected = DialogAnswer { button: OK.to_owned(), value: "main".to_owned(), checked: true, overwrite: true };
        assert_eq!(answer, expected);

        // Another name drops the overwrite; the checkbox toggles.
        let (dialog, answers, cx) = open(cx, |window, cx| {
            let prompt = new_branch_prompt(None, Some("main"), &[branch("main")], &mut I18n::default());
            PromptDialog::new_branch(prompt, window, cx)
        });
        click(cx, "prompt-overwrite");
        cx.simulate_input("2");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.offers_overwrite()));
        click(cx, "prompt-checkbox");
        cx.simulate_keystrokes("enter");
        let answer = answers.borrow().last().cloned().expect("the dialog answered");
        assert_eq!((answer.value.as_str(), answer.checked, answer.overwrite), ("main2", false, false));
    }

    // Not from a spec: Rename asks for a new name; Escape cancels.
    #[gpui_kit::test]
    async fn rename_asks_for_a_new_name(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, |window, cx| {
            let prompt = rename_branch_prompt("topic", &[branch("topic"), branch("main")], &mut I18n::default());
            PromptDialog::rename_branch(prompt, window, cx)
        });
        assert!(errors(&dialog, cx).is_empty(), "the unchanged name is not flagged before it is edited");
        assert!(!can_confirm(&dialog, cx));
        cx.simulate_keystrokes("backspace backspace backspace backspace backspace");
        cx.simulate_input("main");
        assert_eq!(errors(&dialog, cx), ["Branch 'main' already exists"]);
        cx.simulate_input("2");
        assert!(can_confirm(&dialog, cx));
        cx.simulate_keystrokes("escape");
        assert!(answers.borrow().last().is_some_and(DialogAnswer::is_cancel));
    }

    // Not from a spec: Checkout Tag or Revision needs a revision.
    #[gpui_kit::test]
    async fn checkout_revision_needs_a_revision(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, PromptDialog::checkout_revision);
        assert!(!can_confirm(&dialog, cx));
        cx.simulate_keystrokes("enter");
        assert!(answers.borrow().is_empty());
        cx.simulate_input("v1.0");
        cx.simulate_keystrokes("enter");
        let answer = answers.borrow().last().cloned().expect("the dialog answered");
        assert_eq!(answer.prompt_result().map(|result| result.value).as_deref(), Some("v1.0"));
    }

    // Not from a spec: a confirmation is accepted with Enter, its danger button labelled by the
    // request; a message box answers with its own buttons and Escape with cancel.
    #[gpui_kit::test]
    async fn confirmations_and_message_boxes_answer_with_their_buttons(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, |window, cx| {
            let request = ConfirmRequest::danger("Delete Remote Branch", "Delete branch 'x'?".to_owned(), "Delete");
            PromptDialog::confirm(request, window, cx)
        });
        let labels: Vec<String> =
            dialog.read_with(cx, |dialog, _| dialog.buttons().iter().map(|button| button.label.clone()).collect());
        assert_eq!(labels, ["Delete", "Cancel"]);
        cx.simulate_keystrokes("enter");
        assert!(answers.borrow().last().is_some_and(DialogAnswer::confirmed));

        let (_dialog, answers, cx) = open(cx, |window, cx| {
            let request = AskRequest {
                title: "Rollback".to_owned(),
                body: "Roll back?".to_owned(),
                checkbox: Some("Don't ask again".to_owned()),
                buttons: vec![
                    AskButton { id: "rollback", label: "Rollback", primary: true },
                    AskButton { id: "keep", label: "Don't Rollback", primary: false },
                ],
                ..AskRequest::default()
            };
            PromptDialog::ask(request, window, cx)
        });
        click(cx, "prompt-checkbox");
        click(cx, "prompt-button-keep");
        assert_eq!(answers.borrow().last().map(DialogAnswer::ask_answer), Some(AskAnswer::new("keep", true)));
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().last().map(DialogAnswer::ask_answer), Some(AskAnswer::new(CANCEL, true)));
    }

    // Not from a spec: a prompt's checkbox starts as the request says.
    #[gpui_kit::test]
    async fn a_prompt_checkbox_starts_as_requested(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, |window, cx| {
            let request = PromptRequest {
                title: "Name".to_owned(),
                checkbox: Some(PromptCheckbox { label: "Also this".to_owned(), initial: true }),
                ..PromptRequest::default()
            };
            PromptDialog::prompt(request, FieldRule::NonEmpty("Value must not be empty".to_owned()), window, cx)
        });
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(true));
        let labels: Vec<String> =
            dialog.read_with(cx, |dialog, _| dialog.buttons().iter().map(|button| button.label.clone()).collect());
        assert_eq!(labels, ["OK", "Cancel"]);
    }

    fn info(head: &str, upstream: &str) -> BranchInfo {
        let wire = json!({ "head": head, "headCommit": "c1", "upstream": upstream, "ahead": 1, "behind": 0 });
        serde_json::from_value(wire).expect("branch info in its wire form")
    }

    fn commit(id: &str, summary: &str) -> Commit {
        let wire = json!({
            "id": id,
            "shortId": id,
            "parents": ["c0"],
            "author": "me",
            "email": "me@example.com",
            "timeMs": 0,
            "summary": summary,
            "message": summary,
            "refs": [],
        });
        serde_json::from_value(wire).expect("a commit in its wire form")
    }

    fn push_repo(root: &str, remotes: &[&str]) -> PushRepo {
        PushRepo {
            push: RepoPush {
                root: root.to_owned(),
                branch: info("feat-x", "origin/feat-x"),
                commits: vec![commit(&format!("{root}-1"), "Work")],
            },
            name: root.trim_start_matches("/p/").to_owned(),
            remotes: remotes.iter().map(|remote| (*remote).to_owned()).collect(),
        }
    }

    // src/features/vcs/__tests__/multiPushTarget.test.tsx › pushes each repository to its own remote and target branch,
    // reporting the one that failed
    #[gpui_kit::test]
    async fn pushes_each_repository_to_its_own_remote_and_target_branch(cx: &mut TestAppContext) {
        init(cx);
        let input = PushDialogInput {
            repos: vec![
                push_repo("/p/a", &["origin"]),
                push_repo("/p/b", &["origin", "fork"]),
                push_repo("/p/c", &["origin"]),
            ],
            selected: Some("/p/a".to_owned()),
            ..PushDialogInput::default()
        };
        let (dialog, cx) = cx.add_window_view(|window, cx| PushDialog::new(input, window, cx));
        let pushed: Rc<RefCell<Vec<Vec<PushRequest>>>> = Rc::default();
        let log = Rc::clone(&pushed);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &PushDialogEvent, _| {
                if let PushDialogEvent::Push(requests) = event {
                    log.borrow_mut().push(requests.clone());
                }
            })
            .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), ["/p/a", "/p/b", "/p/c"]);
        let target = dialog.read_with(cx, |dialog, _| dialog.target_field(0).cloned()).expect("a's target field");
        assert_eq!(target.read_with(cx, |field, _| field.text().to_owned()), "feat-x");
        assert!(dialog.read_with(cx, |dialog, cx| dialog.shows_push_tags(cx)));
        target.update(cx, |field, cx| {
            field.set_text("release-2.1", cx);
            cx.emit(TextFieldEvent::Changed);
        });
        cx.run_until_parked();
        // A retargeted push carries no tags, as in the single-repository dialog.
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.shows_push_tags(cx)));

        // Only a repository with several remotes offers to pick one.
        assert!(dialog.read_with(cx, |dialog, _| dialog.remote_choices(0).is_empty()));
        assert!(cx.debug_bounds("push-remote-0").is_none());
        click(cx, "push-remote-1");
        click(cx, "push-remote-1-fork");

        click(cx, "push-button-push");
        let requests = pushed.borrow().last().cloned().expect("the dialog asked for the pushes");
        let ops: Vec<(String, serde_json::Value)> = requests
            .iter()
            .map(|request| (request.root.clone(), serde_json::to_value(&request.op).expect("a push serializes")))
            .collect();
        assert_eq!(
            ops,
            [
                (
                    "/p/a".to_owned(),
                    json!({
                        "op": "pushBranch",
                        "branch": "feat-x",
                        "revision": null,
                        "force_with_lease": false,
                        "target": "release-2.1",
                        "remote": null,
                        "force": false,
                        "set_upstream": false,
                        "no_verify": false,
                    })
                ),
                (
                    "/p/b".to_owned(),
                    json!({
                        "op": "pushBranch",
                        "branch": "feat-x",
                        "revision": null,
                        "force_with_lease": false,
                        "target": null,
                        "remote": "fork",
                        "force": false,
                        "set_upstream": false,
                        "no_verify": false,
                    })
                ),
                (
                    "/p/c".to_owned(),
                    json!({
                        "op": "push",
                        "force_with_lease": false,
                        "set_upstream": false,
                        "tags": null,
                        "force": false,
                        "no_verify": false,
                    })
                ),
            ]
        );
        let details: Vec<(&str, bool)> =
            requests.iter().map(|request| (request.detail.as_str(), request.auto_update)).collect();
        assert_eq!(
            details,
            [
                ("a: feat-x → origin/release-2.1", false),
                ("b: feat-x → fork/feat-x", false),
                ("c: feat-x → origin/feat-x", true),
            ]
        );
    }

    // Not from a spec: Force switches the button to Force Push, unchecking every repository leaves
    // nothing to push, and Escape cancels.
    #[gpui_kit::test]
    async fn force_and_the_checkboxes_change_what_is_pushed(cx: &mut TestAppContext) {
        init(cx);
        let input = PushDialogInput { repos: vec![push_repo("/p/a", &["origin"])], ..PushDialogInput::default() };
        let (dialog, cx) = cx.add_window_view(|window, cx| PushDialog::new(input, window, cx));
        let cancelled: Rc<RefCell<bool>> = Rc::default();
        let log = Rc::clone(&cancelled);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &PushDialogEvent, _| {
                if matches!(event, PushDialogEvent::Cancelled) {
                    *log.borrow_mut() = true;
                }
            })
            .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.button_label()), "Push");
        click(cx, "push-force");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.button_label()), "Force Push");
        click(cx, "push-check-0");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.can_push()));
        cx.simulate_keystrokes("escape");
        assert!(*cancelled.borrow());
    }

    // Not from a spec: the smallest edit keeps what is unchanged on both sides.
    #[test]
    fn the_smallest_edit_replaces_only_what_changed() {
        assert_eq!(minimal_edit("a b", "a-b"), (1..2, "-".to_owned()));
        assert_eq!(minimal_edit("fix ", "fix-"), (3..4, "-".to_owned()));
        assert_eq!(minimal_edit("ab~", "ab"), (2..3, String::new()));
        assert_eq!(minimal_edit("é x", "é-x"), (1..2, "-".to_owned()));
    }

    // Not from a spec: every design token the dialogs draw with exists.
    #[test]
    fn the_design_tokens_the_dialogs_use_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-faint",
            "fg-button-primary",
            "bg-popup",
            "bg-input",
            "bg-button",
            "bg-button-primary",
            "bg-checkbox",
            "bg-hover",
            "bg-tree-selected",
            "border",
            "border-popup",
            "border-button",
            "border-checkbox",
            "accent",
            "danger",
            "warning",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "fs-small", "radius-small", "radius-popup", "h-list-row"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
    }
}
