//! The New File dialog of the file templates (IDEA's New Class dialog), in two steps.
//!
//! Without a template it first lists the enabled templates under the folder the file goes into
//! ("New"): the arrows move round the list, Enter or a click picks one. Then it asks for the name
//! ("New <template>"): the kinds of the template are listed under the field and the arrows choose
//! one, the line under them says where the file goes (and its package or namespace), or what is
//! wrong with the name. A template with an exact file name asks for no name; one with variables
//! asks for each of them, with Create and Cancel buttons. Enter (not while an input method is
//! composing) creates, Escape closes.
//!
//! The dialog creates nothing itself: it reports the file to create, and the folder whose package
//! the hint needs; the workbench answers with [`NewFileDialog::set_package`] and, when the creation
//! fails before the file exists, [`NewFileDialog::creation_failed`].

use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::virtual_docs::relative_path;
use crate::templates::files_create::package_style_for;
use crate::templates::files_model::{
    FileTemplate, FileTemplateKind, PackageStyle, PlannedFile, TemplatePrompt, plan_file, with_kind,
};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::prompt_dialogs::button;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Keystroke, MouseButton, MouseDownEvent, Render, SharedString, Subscription, Window, div, prelude::*,
    px,
};
use std::collections::BTreeMap;

const CHOOSER_WIDTH: f32 = 320.0;
const NAME_WIDTH: f32 = 380.0;
const TEMPLATE_ROW_HEIGHT: f32 = 24.0;
const TEMPLATE_LIST_MAX_HEIGHT: f32 = 360.0;
const FIELD_HEIGHT: f32 = 28.0;
/// How far Page Up / Page Down move in the template list.
const PAGE: usize = 10;

/// What the dialog is opened with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewFileDialogInput {
    /// The folder the file goes into.
    pub directory: String,
    /// The project's root ("" without a project): paths are shown relative to it.
    pub project_root: String,
    /// The enabled templates the first step lists.
    pub templates: Vec<FileTemplate>,
    /// The template to name a file of; without one the dialog starts with the list.
    pub template: Option<FileTemplate>,
    /// The name the field starts with (else the template's default name).
    pub name: Option<String>,
    /// The id of the kind chosen at first.
    pub kind_id: Option<String>,
}

/// A file the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFileCreate {
    pub template: FileTemplate,
    pub kind: FileTemplateKind,
    pub directory: String,
    /// The name as typed (a template's exact file name when it has one).
    pub input: String,
    /// The template's variables, by name.
    pub variables: BTreeMap<String, String>,
}

/// The package the hint shows is that of a file of `template` (its kind applied) in `directory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageQuery {
    pub template: FileTemplate,
    pub directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewFileDialogEvent {
    /// Create this file; the dialog stays, busy, until the workbench closes it or reports a failure.
    Create(NewFileCreate),
    /// The hint needs the package of this query: answer with [`NewFileDialog::set_package`].
    PackageWanted(PackageQuery),
    /// Escape or Cancel: close the dialog, nothing is created.
    Closed,
}

/// The second step: the name, the variables and the kind of a template.
struct NameStep {
    template: FileTemplate,
    variables: Vec<(TemplatePrompt, Entity<TextField>)>,
    kind: usize,
    /// Shown instead of the hint, in the UI language (or as the backend wrote it).
    error: Option<String>,
    busy: bool,
    package: String,
    package_query: Option<PackageQuery>,
}

enum Step {
    Choose { active: usize },
    Name(NameStep),
}

pub struct NewFileDialog {
    directory: String,
    project_root: String,
    templates: Vec<FileTemplate>,
    initial_name: Option<String>,
    initial_kind: Option<String>,
    step: Step,
    name: Entity<TextField>,
    focus: FocusHandle,
    claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewFileDialogEvent> for NewFileDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn tx_built(cx: &mut App, strings: &[&str], value: &str) -> String {
    cx.default_global::<Localization>().0.tx_built(strings, &[Param::from(value)])
}

impl NewFileDialog {
    /// The dialog for `input`; the owner gives it the keyboard ([`Focusable`]) and then [`Self::start`]s it.
    pub fn new(input: NewFileDialogInput, cx: &mut Context<Self>) -> Self {
        let name = cx.new(TextField::new);
        let focus = cx.focus_handle();
        let changed = cx.subscribe(&name, |dialog: &mut Self, _, event: &TextFieldEvent, cx| {
            // Enter and Escape are claimed before the field sees them.
            if *event == TextFieldEvent::Changed {
                dialog.name_changed(cx);
            }
        });
        let mut dialog = Self {
            directory: input.directory,
            project_root: input.project_root,
            templates: input.templates,
            initial_name: input.name,
            initial_kind: input.kind_id,
            step: Step::Choose { active: 0 },
            name,
            focus,
            claims: Vec::new(),
            _subscriptions: vec![changed],
        };
        let own = dialog.focus.clone();
        let field = dialog.name.focus_handle(cx);
        dialog.claim(own, cx);
        dialog.claim(field, cx);
        cx.on_release(|dialog, cx| {
            for claim in dialog.claims.drain(..) {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        if let Some(template) = input.template {
            dialog.enter_name_step(template, cx);
        }
        dialog
    }

    /// Has the dialog see the keys pressed while `focus` is focused, ahead of the keymap.
    fn claim(&mut self, focus: FocusHandle, cx: &mut Context<Self>) {
        let claimant = cx.weak_entity();
        let id = input::claim_keys(
            focus,
            move |keystroke, window, cx| {
                claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        self.claims.push(id);
    }

    /// Starts the dialog once its owner listens: the hint asks for its package, and a template with
    /// an exact file name and nothing to ask creates its file at once.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Step::Name(step) = &self.step else { return };
        let creates_at_once = step.template.file_name.is_some() && step.variables.is_empty();
        self.refresh_package(cx);
        if creates_at_once {
            self.submit(cx);
        }
    }

    /// Asks for the name of a `template` file; returns what is to get the keyboard.
    fn enter_name_step(&mut self, template: FileTemplate, cx: &mut Context<Self>) -> FocusHandle {
        let initial =
            template.file_name.clone().or_else(|| self.initial_name.clone()).or(template.default_name.clone());
        self.name.update(cx, |field, cx| field.set_text(initial.as_deref().unwrap_or_default(), cx));
        let mut variables = Vec::new();
        for prompt in &template.variables {
            let field = cx.new(TextField::new);
            let changed = cx.subscribe(&field, |dialog: &mut Self, _, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed {
                    dialog.variable_changed(cx);
                }
            });
            self._subscriptions.push(changed);
            self.claim(field.focus_handle(cx), cx);
            variables.push((prompt.clone(), field));
        }
        let kind = self
            .initial_kind
            .as_deref()
            .and_then(|id| template.kinds.iter().position(|kind| kind.id == id))
            .unwrap_or(0);
        let target = if template.file_name.is_none() {
            Some(self.name.focus_handle(cx))
        } else {
            variables.first().map(|(_, field)| field.focus_handle(cx))
        };
        self.step = Step::Name(NameStep {
            template,
            variables,
            kind,
            error: None,
            busy: false,
            package: String::new(),
            package_query: None,
        });
        self.update_placeholder(cx);
        cx.notify();
        target.unwrap_or_else(|| self.focus.clone())
    }

    // ---------------------------------------------------------------------------------------------
    // What the dialog shows

    /// The dialog's title, in the UI language.
    pub fn title(&self, cx: &mut App) -> String {
        match &self.step {
            Step::Choose { .. } => tx(cx, "New"),
            Step::Name(step) => tx_built(cx, &["New ", ""], &step.template.name),
        }
    }

    /// Whether the dialog asks for the name (else it lists the templates).
    pub fn is_naming(&self) -> bool {
        matches!(self.step, Step::Name(_))
    }

    /// The names of the templates the first step lists.
    pub fn templates(&self) -> Vec<String> {
        self.templates.iter().map(|template| template.name.clone()).collect()
    }

    /// The selected row of the template list.
    pub fn active_template(&self) -> Option<usize> {
        match self.step {
            Step::Choose { active } => Some(active).filter(|&at| at < self.templates.len()),
            Step::Name(_) => None,
        }
    }

    /// The folder line above the template list.
    pub fn target(&self) -> String {
        let shown = relative_path(&self.project_root, &self.directory).into_owned();
        if shown.is_empty() { "/".to_owned() } else { shown }
    }

    /// The name field, while the dialog asks for a name.
    pub fn name_field(&self) -> Option<&Entity<TextField>> {
        match &self.step {
            Step::Name(step) if step.template.file_name.is_none() => Some(&self.name),
            _ => None,
        }
    }

    /// The variable fields with their prompts, as written by the template.
    pub fn variable_fields(&self) -> Vec<(String, Entity<TextField>)> {
        match &self.step {
            Step::Name(step) => {
                step.variables.iter().map(|(prompt, field)| (prompt.prompt.clone(), field.clone())).collect()
            }
            Step::Choose { .. } => Vec::new(),
        }
    }

    /// The labels of the kinds listed under the field (none for a template of one kind).
    pub fn kinds(&self) -> Vec<String> {
        match &self.step {
            Step::Name(step) if step.template.kinds.len() > 1 => {
                step.template.kinds.iter().map(|kind| kind.label.clone()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// The index of the chosen kind.
    pub fn kind(&self) -> Option<usize> {
        match &self.step {
            Step::Name(step) => Some(step.kind),
            Step::Choose { .. } => None,
        }
    }

    /// The name field's placeholder.
    pub fn placeholder(&self, cx: &App) -> String {
        self.name.read(cx).placeholder().to_owned()
    }

    /// The problem shown under the field, when there is one.
    pub fn error(&self) -> Option<&str> {
        match &self.step {
            Step::Name(step) => step.error.as_deref(),
            Step::Choose { .. } => None,
        }
    }

    /// Whether a creation is under way: the fields and Create are disabled.
    pub fn is_busy(&self) -> bool {
        matches!(&self.step, Step::Name(step) if step.busy)
    }

    /// The buttons (label in the UI language, enabled), shown for a template with variables.
    pub fn buttons(&self, cx: &mut App) -> Vec<(String, bool)> {
        match &self.step {
            Step::Name(step) if !step.variables.is_empty() => {
                let busy = step.busy;
                vec![(tx(cx, "Cancel"), true), (tx(cx, "Create"), !busy)]
            }
            _ => Vec::new(),
        }
    }

    /// The template as the chosen kind makes it, and where the typed name puts the file.
    fn effective(&self, cx: &App) -> Option<(FileTemplate, Option<PlannedFile>)> {
        let Step::Name(step) = &self.step else { return None };
        let effective = with_kind(&step.template, step.template.kinds.get(step.kind)).into_owned();
        let plan = plan_file(&effective, &self.directory, self.name.read(cx).text());
        Some((effective, plan))
    }

    /// The line under the field: the problem, else where the file goes and its package.
    pub fn hint(&self, cx: &App) -> String {
        let Step::Name(step) = &self.step else { return String::new() };
        if let Some(error) = &step.error {
            return error.clone();
        }
        let Some((effective, plan)) = self.effective(cx) else { return String::new() };
        let place = match plan {
            Some(plan) => relative_path(&self.project_root, &plan.path).into_owned(),
            None => self.target(),
        };
        let package = match package_style_for(&effective) {
            Some(_) if step.package.is_empty() => String::new(),
            Some(PackageStyle::Php) => format!(" · namespace {}", step.package),
            Some(_) => format!(" · package {}", step.package),
            None => String::new(),
        };
        format!("{place}{package}")
    }

    // ---------------------------------------------------------------------------------------------
    // What the workbench tells the dialog

    /// The package `query` found; an answer to an earlier query is dropped.
    pub fn set_package(&mut self, query: &PackageQuery, package: String, cx: &mut Context<Self>) {
        if let Step::Name(step) = &mut self.step
            && step.package_query.as_ref() == Some(query)
        {
            step.package = package;
            cx.notify();
        }
    }

    /// The file could not be created: `message` shows under the field and the dialog takes input
    /// again.
    pub fn creation_failed(&mut self, message: String, cx: &mut Context<Self>) {
        if let Step::Name(step) = &mut self.step {
            step.error = Some(message);
            step.busy = false;
        }
        self.set_fields_read_only(false, cx);
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Input

    fn name_changed(&mut self, cx: &mut Context<Self>) {
        if let Step::Name(step) = &mut self.step {
            step.error = None;
        }
        self.refresh_package(cx);
        cx.notify();
    }

    fn variable_changed(&mut self, cx: &mut Context<Self>) {
        if let Step::Name(step) = &mut self.step {
            step.error = None;
        }
        cx.notify();
    }

    /// Asks for the package of the file as it is planned now, when the template has one and the
    /// folder or the kind changed.
    fn refresh_package(&mut self, cx: &mut Context<Self>) {
        let Some((effective, plan)) = self.effective(cx) else { return };
        if package_style_for(&effective).is_none() {
            return;
        }
        let directory = plan.map_or_else(|| self.directory.clone(), |plan| plan.directory);
        let query = PackageQuery { template: effective, directory };
        let Step::Name(step) = &mut self.step else { return };
        if step.package_query.as_ref() != Some(&query) {
            step.package_query = Some(query.clone());
            cx.emit(NewFileDialogEvent::PackageWanted(query));
        }
    }

    fn update_placeholder(&mut self, cx: &mut Context<Self>) {
        let Some((effective, _)) = self.effective(cx) else { return };
        let placeholder = if effective.extension.is_empty() {
            tx(cx, "File name with extension")
        } else {
            tx_built(cx, &["Name (.", ")"], &effective.extension)
        };
        self.name.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
    }

    fn set_kind(&mut self, kind: usize, cx: &mut Context<Self>) {
        let Step::Name(step) = &mut self.step else { return };
        if kind >= step.template.kinds.len() || step.busy {
            return;
        }
        step.kind = kind;
        self.update_placeholder(cx);
        self.refresh_package(cx);
        cx.notify();
    }

    fn set_fields_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        let Step::Name(step) = &self.step else { return };
        let fields: Vec<Entity<TextField>> =
            std::iter::once(self.name.clone()).chain(step.variables.iter().map(|(_, field)| field.clone())).collect();
        for field in fields {
            field.update(cx, |field, cx| field.set_read_only(read_only, cx));
        }
    }

    /// Whether an input method is composing text in one of the fields: Enter confirms that text.
    fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let variables: Vec<Entity<TextField>> = match &self.step {
            Step::Name(step) => step.variables.iter().map(|(_, field)| field.clone()).collect(),
            Step::Choose { .. } => Vec::new(),
        };
        std::iter::once(self.name.clone())
            .chain(variables)
            .any(|field| field.update(cx, |field, cx| field.marked_text_range(window, cx)).is_some())
    }

    /// Enter: creates the file, once the name and every variable are given.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some((_, plan)) = self.effective(cx) else { return };
        let name = self.name.read(cx).text().to_owned();
        let Step::Name(step) = &self.step else { return };
        if step.busy {
            return;
        }
        if plan.is_none() {
            let message = tx(cx, if name.trim().is_empty() { "Enter a name" } else { "Invalid name" });
            self.show_error(message, cx);
            return;
        }
        let variables: BTreeMap<String, String> = step
            .variables
            .iter()
            .map(|(prompt, field)| (prompt.name.clone(), field.read(cx).text().to_owned()))
            .collect();
        if variables.values().any(|value| value.trim().is_empty()) {
            let message = tx(cx, "Enter a value for each variable");
            self.show_error(message, cx);
            return;
        }
        let Some(kind) = step.template.kinds.get(step.kind).or(step.template.kinds.first()).cloned() else { return };
        let create = NewFileCreate {
            template: step.template.clone(),
            kind,
            directory: self.directory.clone(),
            input: name,
            variables,
        };
        if let Step::Name(step) = &mut self.step {
            step.busy = true;
        }
        self.set_fields_read_only(true, cx);
        cx.emit(NewFileDialogEvent::Create(create));
        cx.notify();
    }

    fn show_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Step::Name(step) = &mut self.step {
            step.error = Some(message);
        }
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(NewFileDialogEvent::Closed);
    }

    /// Picks the template at `index` of the list: the dialog asks for its name.
    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = self.templates.get(index).cloned() else { return };
        let target = self.enter_name_step(template, cx);
        window.focus(&target, cx);
        self.start(cx);
    }

    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let key = keystroke.key.as_str();
        if let Step::Choose { active } = self.step {
            let last = self.templates.len().saturating_sub(1);
            let next = match key {
                "escape" => {
                    self.cancel(cx);
                    return true;
                }
                "enter" => {
                    self.pick(active, window, cx);
                    return true;
                }
                _ if self.templates.is_empty() => return false,
                "down" => {
                    if active >= last {
                        0
                    } else {
                        active + 1
                    }
                }
                "up" => {
                    if active == 0 {
                        last
                    } else {
                        active - 1
                    }
                }
                "home" => 0,
                "end" => last,
                "pagedown" => (active + PAGE).min(last),
                "pageup" => active.saturating_sub(PAGE),
                _ => return false,
            };
            self.step = Step::Choose { active: next };
            cx.notify();
            return true;
        }
        if self.composing(window, cx) {
            return false;
        }
        match key {
            "escape" => self.cancel(cx),
            "enter" => self.submit(cx),
            "up" | "down" => {
                let Step::Name(step) = &self.step else { return false };
                let count = step.template.kinds.len();
                if count < 2 {
                    return false;
                }
                let next = if key == "down" { (step.kind + 1) % count } else { (step.kind + count - 1) % count };
                self.set_kind(next, cx);
            }
            _ => return false,
        }
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn field_frame(cx: &App, invalid: bool, field: Entity<TextField>) -> Div {
        div()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if invalid { "danger" } else { "border" }))
            .bg(theme::color(cx, "bg-input"))
            .child(div().flex_1().min_w_0().child(field))
    }

    fn render_templates(&self, active: usize, cx: &mut Context<Self>) -> Div {
        let rows: Vec<_> = self
            .templates
            .iter()
            .enumerate()
            .map(|(index, template)| {
                let selected = index == active;
                let icon = theme::icon(cx, &template.icon, 16.0, theme::color(cx, "fg-muted"));
                div()
                    .id(index)
                    .debug_selector(move || format!("new-file-template-{index}"))
                    .h(px(TEMPLATE_ROW_HEIGHT))
                    .mx(px(4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(theme::length(cx, "radius-row"))
                    .text_color(theme::color(cx, if selected { "fg-strong" } else { "fg" }))
                    .when(selected, |row| row.bg(theme::color(cx, "bg-selected")))
                    .child(div().flex_none().child(icon))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(template.name.clone())),
                    )
                    .on_hover(cx.listener(move |dialog: &mut Self, hovered: &bool, _, cx| {
                        if *hovered && matches!(dialog.step, Step::Choose { active } if active != index) {
                            dialog.step = Step::Choose { active: index };
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| dialog.pick(index, window, cx)))
            })
            .collect();
        let target = div()
            .px(px(10.0))
            .pb(px(6.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-subtle"))
            .overflow_hidden()
            .whitespace_nowrap()
            .child(SharedString::from(self.target()));
        div().flex().flex_col().track_focus(&self.focus).child(target).child(
            div().id("new-file-templates").max_h(px(TEMPLATE_LIST_MAX_HEIGHT)).overflow_y_scroll().children(rows),
        )
    }

    fn render_name(&self, step: &NameStep, cx: &mut Context<Self>) -> Div {
        let invalid = step.error.is_some();
        let label_color = theme::color(cx, "fg");
        let hover = theme::color(cx, "bg-hover");
        let name =
            step.template.file_name.is_none().then(|| {
                Self::field_frame(cx, invalid, self.name.clone()).debug_selector(|| "new-file-name".to_owned())
            });
        // The prompts come from the template's author and are shown as written.
        let variables: Vec<Div> = step
            .variables
            .iter()
            .enumerate()
            .map(|(index, (prompt, field))| {
                div()
                    .flex()
                    .flex_col()
                    .gap(theme::length(cx, "space-2"))
                    .text_color(label_color)
                    .child(SharedString::from(prompt.prompt.clone()))
                    .child(
                        Self::field_frame(cx, false, field.clone())
                            .debug_selector(move || format!("new-file-variable-{index}")),
                    )
            })
            .collect();
        let kinds = (step.template.kinds.len() > 1).then(|| {
            let rows: Vec<_> = step
                .template
                .kinds
                .iter()
                .enumerate()
                .map(|(index, kind)| {
                    let chosen = index == step.kind;
                    let icon = theme::icon(cx, &step.template.icon, 14.0, theme::color(cx, "fg-muted"));
                    div()
                        .id(SharedString::from(format!("new-file-kind-{index}")))
                        .debug_selector(move || format!("new-file-kind-{index}"))
                        .h(theme::length(cx, "h-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(theme::length(cx, "radius-row"))
                        .text_color(theme::color(cx, if chosen { "fg-strong" } else { "fg" }))
                        .when(chosen, |row| row.bg(theme::color(cx, "bg-selected")))
                        .when(!chosen, |row| row.hover(move |row| row.bg(hover)))
                        .child(icon)
                        .child(SharedString::from(kind.label.clone()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |dialog, event: &MouseDownEvent, window, cx| {
                                // The name field keeps the keyboard.
                                window.prevent_default();
                                cx.stop_propagation();
                                dialog.set_kind(index, cx);
                                if event.click_count >= 2 {
                                    dialog.submit(cx);
                                }
                            }),
                        )
                })
                .collect();
            div().flex().flex_col().py(px(2.0)).children(rows)
        });
        let hint = div()
            .min_h(px(16.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, if invalid { "danger" } else { "fg-subtle" }))
            .overflow_hidden()
            .whitespace_nowrap()
            .debug_selector(|| "new-file-hint".to_owned())
            .child(SharedString::from(self.hint(cx)));
        let gap = if step.variables.is_empty() { px(6.0) } else { theme::length(cx, "space-3") };
        div()
            .flex()
            .flex_col()
            .gap(gap)
            .when(name.is_none() && step.variables.is_empty(), |form| form.track_focus(&self.focus))
            .children(name)
            .children(variables)
            .children(kinds)
            .child(hint)
    }

    fn render_buttons(&self, cx: &mut Context<Self>) -> Option<Div> {
        let buttons = self.buttons(cx);
        if buttons.is_empty() {
            return None;
        }
        let [(cancel, _), (create, enabled)] = <[(String, bool); 2]>::try_from(buttons).ok()?;
        let cancel = button(cx, cancel, false, false, true)
            .id("new-file-button-cancel")
            .debug_selector(|| "new-file-button-cancel".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.cancel(cx)));
        let create = button(cx, create, true, false, enabled)
            .id("new-file-button-create")
            .debug_selector(|| "new-file-button-create".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.submit(cx)));
        Some(div().flex().justify_end().gap(px(8.0)).pt(px(12.0)).child(cancel).child(create))
    }
}

impl Focusable for NewFileDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.step {
            Step::Name(step) if step.template.file_name.is_none() => self.name.focus_handle(cx),
            Step::Name(step) => step.variables.first().map_or_else(|| self.focus.clone(), |(_, f)| f.focus_handle(cx)),
            Step::Choose { .. } => self.focus.clone(),
        }
    }
}

impl Render for NewFileDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        let (width, body, buttons, inset) = match &self.step {
            Step::Choose { active } => (CHOOSER_WIDTH, self.render_templates(*active, cx), None, px(0.0)),
            Step::Name(step) => {
                // A form of variables sits further in from the dialog's edges.
                let inset = if step.variables.is_empty() {
                    px(16.0)
                } else {
                    (theme::length(cx, "space-3") + theme::length(cx, "space-4")) * 0.5 + px(16.0)
                };
                (NAME_WIDTH, self.render_name(step, cx), self.render_buttons(cx), inset)
            }
        };
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            // A click on the dialog is not a click beside it.
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
            .child(div().px(inset).pb(px(12.0)).child(body).children(buttons))
    }
}
