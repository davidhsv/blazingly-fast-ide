//! The dialogs of the project view's file operations.
//!
//! [`FileDialog`] is Rename (⇧F6), Move (F6, drag & drop, cut & paste) and Copy (F5, copy & paste)
//! of files and folders, for the [`FileRefactorRequest`] an operation opened it with. Rename and
//! Move are refactorings: they offer "Search for references" and "Search in comments and
//! strings", remembered per dialog, and say they are looking for references while the workbench
//! collects them. Move and Copy take the folder to go to as typed (absolute, or relative to the
//! project root) or picked with the browse button. Every dialog checks what is typed as it is
//! typed and shows the problem under the fields; Enter presses the default button when it can be
//! pressed, Escape cancels.
//!
//! [`DeleteDialog`] asks before Delete, with "Safe delete (with usage search)" as it was last
//! left. The questions of a safe delete ("Usages Detected", the progress of a long search) are
//! message boxes [`crate::vcs::prompt_dialogs::PromptDialog::ask`] shows; the functions here
//! build them and read their answers as the operation's [`Reply`].
//!
//! [`RefactoringPreview`] lists the references a move or rename changes in other files, each
//! occurrence on its line, and lets files be left out before Do Refactor.
//!
//! None of them runs anything: their events say what the workbench carries out.

use crate::editor_refactor::safe_delete::UsagesQuestion;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::misc::platform_info::Os;
use crate::project_tree_paths::{
    ValidateOptions, is_same_or_inside, join_path, name_of, parent_of, relative_to, split_extension, unique_name,
    validate_name,
};
use crate::project_view::operations::{
    DeleteQuestion, FileRefactorRequest, ProgressAnswer, RefactorLabel, Reply, UsagesAnswer,
};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::prompt_dialogs::DialogAnswer;
use crate::vcs_ops::dialogs::{AskButton, AskRequest};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_core::ipc_path::to_ipc;
use ide_core::{FileEdit, Location, TextEdit, WorkspaceEdit};
use ide_workspace::FileMove;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Where the options of the Rename and Move dialogs are kept, one key per dialog (see
/// [`refactor_options_key`]).
pub const REFACTOR_OPTIONS_KEY: &str = "openidea.projectTree.refactorOptions";
/// Where the Copy dialog's "Open copy in editor" is kept: `true` or `false`.
pub const COPY_OPEN_KEY: &str = "openidea.projectTree.copyOpenInEditor";
const BUTTON_HEIGHT: f32 = 28.0;
const FIELD_HEIGHT: f32 = 28.0;
/// A file of the preview lists at most this many of its occurrences.
const PREVIEW_OCCURRENCES: usize = 50;

// ---------------------------------------------------------------------------
// Remembered options

/// The options of the Rename and Move dialogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefactorOptions {
    /// Update references through the language servers.
    pub references: bool,
    /// Also replace the old name or path in comments, strings and text files.
    pub text_occurrences: bool,
    /// Move: open the moved files in editors.
    pub open_in_editor: bool,
}

impl Default for RefactorOptions {
    fn default() -> Self {
        Self { references: true, text_occurrences: true, open_in_editor: false }
    }
}

impl RefactorOptions {
    /// The options as they are stored.
    pub fn stored(&self) -> String {
        let value = json!({
            "references": self.references,
            "textOccurrences": self.text_occurrences,
            "openInEditor": self.open_in_editor,
        });
        value.to_string()
    }
}

/// The key the options of the dialog `label` are kept under.
pub fn refactor_options_key(label: RefactorLabel) -> String {
    let dialog = match label {
        RefactorLabel::Rename => "rename",
        RefactorLabel::Move => "move",
    };
    format!("{REFACTOR_OPTIONS_KEY}.{dialog}")
}

/// The options stored as `stored`: what is missing or unreadable takes its default.
pub fn load_refactor_options(stored: Option<&str>) -> RefactorOptions {
    let defaults = RefactorOptions::default();
    let Some(value) = stored.and_then(|stored| serde_json::from_str::<Value>(stored).ok()) else { return defaults };
    let flag = |key: &str, default: bool| value.get(key).and_then(Value::as_bool).unwrap_or(default);
    RefactorOptions {
        references: flag("references", defaults.references),
        text_occurrences: flag("textOccurrences", defaults.text_occurrences),
        open_in_editor: flag("openInEditor", defaults.open_in_editor),
    }
}

// ---------------------------------------------------------------------------
// Drawing

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// A check box with its label, greyed out when it cannot be changed.
fn checkbox(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    let face = div()
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
        .child(SharedString::from(label));
    if enabled { face } else { face.opacity(0.5) }
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
fn button(cx: &App, label: String, primary: bool, danger: bool, enabled: bool) -> Div {
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
fn field_frame(cx: &App, invalid: bool) -> Div {
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

/// The dialog's frame: a title over the body, the buttons in the footer with `footer_start`
/// before them.
fn dialog_frame(
    cx: &mut App,
    width: f32,
    title: String,
    body: Div,
    footer_start: Option<Div>,
    buttons: Vec<Div>,
) -> Div {
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
        .child(body)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(16.0))
                .py(px(12.0))
                .child(div().flex_1().min_w_0().children(footer_start))
                .children(buttons),
        )
}

fn small_text(cx: &App, color: &str, text: String) -> Div {
    div().text_size(theme::length(cx, "fs-small")).text_color(theme::color(cx, color)).child(SharedString::from(text))
}

/// The first font of the theme's `--font-mono` list that is installed: paths are code.
fn code_font(cx: &App) -> SharedString {
    crate::theme::installed_fonts::mono_family(cx)
}

// ---------------------------------------------------------------------------
// Rename, Move, Copy

/// What a [`FileDialog`] is opened with.
#[derive(Debug, Clone)]
pub struct FileDialogInput {
    pub request: FileRefactorRequest,
    /// The project root: a relative target folder starts there.
    pub root: String,
    /// The OS the project's files are on: whether names differ by case only, and the names
    /// Windows reserves.
    pub host_os: Os,
    /// Rename / Move: the options last used in that dialog ([`load_refactor_options`]).
    pub options: RefactorOptions,
    /// Copy: "Open copy in editor" as last chosen (what [`COPY_OPEN_KEY`] holds).
    pub open_copy: bool,
    /// The names in the folders the tree has read, by folder: the Copy dialog finds a free name
    /// in its folder and checks the name against the folder it copies into, when the tree has
    /// read that one (the copy itself reports a conflict otherwise).
    pub listings: BTreeMap<String, Vec<String>>,
}

/// What a [`FileDialog`] asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum FileDialogEvent {
    /// Cancel or Escape (stop a reference search under way), or Rename with the name unchanged:
    /// close the dialog.
    Cancelled,
    /// Rename or Move as a refactoring: store `options` under [`refactor_options_key`], collect
    /// the reference updates (call [`FileDialog::set_busy`] meanwhile), close the dialog once
    /// they are collected and carry out the moves.
    Refactor { label: RefactorLabel, moves: Vec<FileMove>, options: RefactorOptions },
    /// Copy `paths` into `target`, the single item as `name`: store `open_in_editor` under
    /// [`COPY_OPEN_KEY`] and copy; close the dialog when anything was copied.
    Copy { paths: Vec<String>, target: String, name: Option<String>, open_in_editor: bool },
    /// The browse button: pick a folder (starting at `current`) and hand it over with
    /// [`FileDialog::set_target`].
    Browse { current: String },
}

/// The folder `typed` (absolute, or relative to `root`), without trailing separators; the root
/// when nothing is typed.
pub fn resolve_target(typed: &str, root: &str) -> String {
    let trimmed = typed.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return root.to_owned();
    }
    let bytes = trimmed.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\');
    if trimmed.starts_with('/') || drive { trimmed.to_owned() } else { join_path(root, trimmed) }
}

pub struct FileDialog {
    input: FileDialogInput,
    options: RefactorOptions,
    open_copy: bool,
    name: Option<Entity<TextField>>,
    target: Option<Entity<TextField>>,
    /// Looking for references, or copying: nothing can be changed or pressed but Cancel.
    busy: bool,
    code_font: SharedString,
    key_claims: Vec<u64>,
    _field_events: Vec<Subscription>,
}

impl EventEmitter<FileDialogEvent> for FileDialog {}

fn fold(name: &str, case_insensitive: bool) -> String {
    if case_insensitive { name.to_lowercase() } else { name.to_owned() }
}

impl FileDialog {
    pub fn new(input: FileDialogInput, cx: &mut Context<Self>) -> Self {
        let case_insensitive = matches!(input.host_os, Os::Windows | Os::Macos);
        // A name field starts with the name's stem selected (a folder's whole name), ready to be retyped.
        let field = |text: &str, name_of_dir: Option<bool>, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut field = TextField::new(cx);
                field.set_text(text, cx);
                if let Some(dir) = name_of_dir {
                    let stem = if dir { text } else { split_extension(text).0 };
                    let end = if stem.is_empty() { text.len() } else { stem.len() };
                    field.select_range(0..end, cx);
                }
                field
            })
        };
        let (name, target) = match &input.request {
            FileRefactorRequest::Rename { path, dir, .. } => (Some(field(name_of(path), Some(*dir), cx)), None),
            FileRefactorRequest::Move { target, .. } => (None, Some(field(target, None, cx))),
            FileRefactorRequest::Copy { paths, target, dir } => {
                let name = match paths.as_slice() {
                    [only] => {
                        let listing = input.listings.get(target).map(Vec::as_slice).unwrap_or_default();
                        let taken: BTreeSet<String> = listing.iter().map(|name| fold(name, case_insensitive)).collect();
                        let is_taken = |name: &str| taken.contains(&fold(name, case_insensitive));
                        let free = unique_name(name_of(only), is_taken, *dir);
                        Some(field(&free, Some(*dir), cx))
                    }
                    _ => None,
                };
                (name, Some(field(target, None, cx)))
            }
        };
        let mut events = Vec::new();
        let mut key_claims = Vec::new();
        for entity in name.iter().chain(target.iter()) {
            events.push(cx.subscribe(entity, |dialog: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => dialog.changed(cx),
                TextFieldEvent::Submitted => dialog.submit(cx),
                TextFieldEvent::Cancelled => dialog.cancel(cx),
            }));
            let claimant = cx.weak_entity();
            key_claims.push(input::claim_keys(
                entity.focus_handle(cx),
                move |keystroke, _, cx| {
                    claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false)
                },
                cx,
            ));
        }
        cx.on_release(|dialog, cx| {
            for claim in &dialog.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        Self {
            options: input.options,
            open_copy: input.open_copy,
            input,
            name,
            target,
            busy: false,
            code_font: code_font(cx),
            key_claims,
            _field_events: events,
        }
    }

    pub fn request(&self) -> &FileRefactorRequest {
        &self.input.request
    }

    /// The dialog's title, English source text.
    pub fn title(&self) -> &'static str {
        match self.input.request {
            FileRefactorRequest::Rename { .. } => "Rename",
            FileRefactorRequest::Move { .. } => "Move",
            FileRefactorRequest::Copy { .. } => "Copy",
        }
    }

    /// The name field: Rename, and Copy of a single item.
    pub fn name_field(&self) -> Option<&Entity<TextField>> {
        self.name.as_ref()
    }

    /// The target folder field of Move and Copy.
    pub fn target_field(&self) -> Option<&Entity<TextField>> {
        self.target.as_ref()
    }

    pub fn options(&self) -> RefactorOptions {
        self.options
    }

    pub fn open_copy(&self) -> bool {
        self.open_copy
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn case_insensitive(&self) -> bool {
        matches!(self.input.host_os, Os::Windows | Os::Macos)
    }

    fn name_text(&self, cx: &App) -> String {
        self.name.as_ref().map(|field| field.read(cx).text().to_owned()).unwrap_or_default()
    }

    /// The folder the items go to, as typed.
    pub fn destination(&self, cx: &App) -> String {
        let typed = self.target.as_ref().map(|field| field.read(cx).text().to_owned()).unwrap_or_default();
        resolve_target(&typed, &self.input.root)
    }

    /// What the dialog is about, in the UI language: "Move file 'a.ts'", "Copy 3 items".
    pub fn subject(&self, i18n: &mut I18n) -> String {
        let (verb, paths, dir) = match &self.input.request {
            FileRefactorRequest::Rename { path, dir, .. } => {
                let current = [Param::from(name_of(path))];
                return if *dir {
                    i18n.tx_built(&["Rename directory '", "' and its usages to:"], &current)
                } else {
                    i18n.tx_built(&["Rename file '", "' and its usages to:"], &current)
                };
            }
            FileRefactorRequest::Move { paths, dir, .. } => ("Move", paths, *dir),
            FileRefactorRequest::Copy { paths, dir, .. } => ("Copy", paths, *dir),
        };
        match paths.as_slice() {
            [only] => {
                let kind = if dir { " directory '" } else { " file '" };
                i18n.tx_built(&[&format!("{verb}{kind}"), "'"], &[Param::from(name_of(only))])
            }
            _ => i18n.tx_built(&[&format!("{verb} "), " items"], &[Param::from(paths.len())]),
        }
    }

    /// The target folder as the hint under its field shows it: relative to the project root,
    /// "Project root" for the root itself.
    pub fn hint(&self, cx: &App) -> Option<String> {
        self.target.as_ref()?;
        let dest = self.destination(cx);
        let relative = relative_to(&self.input.root, &dest);
        Some(match relative {
            "" if dest == self.input.root => "Project root".to_owned(),
            "" => dest,
            relative => relative.to_owned(),
        })
    }

    /// The moves Rename or Move would carry out.
    fn moves(&self, cx: &App) -> Vec<FileMove> {
        let moved = |from: &str, to: String| FileMove { from: from.to_owned().into(), to: to.into() };
        match &self.input.request {
            FileRefactorRequest::Rename { path, .. } => {
                vec![moved(path, join_path(parent_of(path), &self.name_text(cx)))]
            }
            FileRefactorRequest::Move { paths, .. } => {
                let dest = self.destination(cx);
                let away = paths.iter().filter(|path| parent_of(path) != dest);
                away.map(|path| moved(path, join_path(&dest, name_of(path)))).collect()
            }
            FileRefactorRequest::Copy { .. } => Vec::new(),
        }
    }

    /// What stops the default button, English source text (a name's problem names the name).
    fn problem(&self, cx: &App) -> Option<String> {
        let case_insensitive = self.case_insensitive();
        let windows = self.input.host_os == Os::Windows;
        match &self.input.request {
            FileRefactorRequest::Rename { path, siblings, .. } => {
                let current = name_of(path);
                let name = self.name_text(cx);
                if name == current {
                    return None;
                }
                let siblings: Vec<&str> = siblings.iter().map(String::as_str).collect();
                let options = ValidateOptions {
                    siblings: &siblings,
                    current: Some(current),
                    case_insensitive,
                    windows,
                    ..ValidateOptions::default()
                };
                validate_name(&name, &options).error
            }
            FileRefactorRequest::Move { paths, .. } => {
                let dest = self.destination(cx);
                if paths.iter().any(|path| is_same_or_inside(&dest, path)) {
                    Some("Cannot move a folder into itself".to_owned())
                } else if self.moves(cx).is_empty() {
                    Some("Already in this directory".to_owned())
                } else {
                    None
                }
            }
            FileRefactorRequest::Copy { paths, .. } => {
                let dest = self.destination(cx);
                if paths.iter().any(|path| is_same_or_inside(&dest, path) && dest != parent_of(path)) {
                    return Some("Cannot copy a folder into itself".to_owned());
                }
                self.name.as_ref()?;
                let listing = self.input.listings.get(&dest).map(Vec::as_slice).unwrap_or_default();
                let siblings: Vec<&str> = listing.iter().map(String::as_str).collect();
                let options =
                    ValidateOptions { siblings: &siblings, case_insensitive, windows, ..ValidateOptions::default() };
                validate_name(&self.name_text(cx), &options).error
            }
        }
    }

    /// The problem shown under the fields, in the UI language.
    pub fn error(&self, cx: &mut App) -> Option<String> {
        let problem = self.problem(cx)?;
        Some(tx(cx, &problem))
    }

    /// Whether the default button can be pressed.
    pub fn can_submit(&self, cx: &App) -> bool {
        !self.busy && self.problem(cx).is_none()
    }

    /// The references are being collected (or the copy made): only Cancel can be pressed. A copy
    /// that copied nothing is reported with `false`, and the dialog can be used again.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        for field in self.name.iter().chain(self.target.iter()) {
            field.update(cx, |field, cx| field.set_read_only(busy, cx));
        }
        cx.notify();
    }

    /// The folder picked with the browse button.
    pub fn set_target(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(target) = &self.target {
            target.update(cx, |field, cx| field.set_text(path, cx));
            self.changed(cx);
        }
    }

    /// The names in `folder`, once the tree has read it.
    pub fn set_listing(&mut self, folder: &str, names: Vec<String>, cx: &mut Context<Self>) {
        self.input.listings.insert(folder.to_owned(), names);
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_submit(cx) {
            return;
        }
        let unchanged = |path: &str| self.name_text(cx) == name_of(path);
        let event = match &self.input.request {
            FileRefactorRequest::Rename { path, .. } if unchanged(path) => FileDialogEvent::Cancelled,
            FileRefactorRequest::Rename { .. } => {
                FileDialogEvent::Refactor { label: RefactorLabel::Rename, moves: self.moves(cx), options: self.options }
            }
            FileRefactorRequest::Move { .. } => {
                FileDialogEvent::Refactor { label: RefactorLabel::Move, moves: self.moves(cx), options: self.options }
            }
            FileRefactorRequest::Copy { paths, dir, .. } => {
                let name = self.name.as_ref().map(|_| self.name_text(cx));
                let open_in_editor = self.open_copy && name.is_some() && !dir;
                FileDialogEvent::Copy { paths: paths.clone(), target: self.destination(cx), name, open_in_editor }
            }
        };
        if matches!(event, FileDialogEvent::Copy { .. }) {
            self.busy = true;
        }
        cx.emit(event);
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(FileDialogEvent::Cancelled);
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            cx.emit(FileDialogEvent::Browse { current: self.destination(cx) });
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.submit(cx),
            "escape" => self.cancel(cx),
            _ => return false,
        }
        true
    }

    fn toggle(&mut self, option: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match option {
            "references" => self.options.references = !self.options.references,
            "text" => self.options.text_occurrences = !self.options.text_occurrences,
            "open" => self.options.open_in_editor = !self.options.open_in_editor,
            "open-copy" => self.open_copy = !self.open_copy,
            _ => return,
        }
        cx.notify();
    }

    /// The checkboxes the dialog offers: id, English label, state.
    pub fn checkboxes(&self) -> Vec<(&'static str, &'static str, bool)> {
        let refactor = [
            ("references", "Search for references", self.options.references),
            ("text", "Search in comments and strings", self.options.text_occurrences),
        ];
        match &self.input.request {
            FileRefactorRequest::Rename { .. } => refactor.to_vec(),
            FileRefactorRequest::Move { .. } => {
                let mut boxes = refactor.to_vec();
                boxes.push(("open", "Open moved files in editor", self.options.open_in_editor));
                boxes
            }
            FileRefactorRequest::Copy { dir, .. } if self.name.is_some() && !dir => {
                vec![("open-copy", "Open copy in editor", self.open_copy)]
            }
            FileRefactorRequest::Copy { .. } => Vec::new(),
        }
    }

    fn render_field(&self, cx: &mut App, label: &str, field: &Entity<TextField>, mono: bool, invalid: bool) -> Div {
        let label = tx(cx, label);
        let frame = field_frame(cx, invalid)
            .when(mono, |el| el.font_family(self.code_font.clone()))
            .child(div().flex_1().min_w_0().child(field.clone()));
        div().flex().flex_col().gap(px(6.0)).child(SharedString::from(label)).child(frame)
    }
}

impl Focusable for FileDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.name.as_ref().or(self.target.as_ref()) {
            Some(field) => field.focus_handle(cx),
            None => cx.focus_handle(),
        }
    }
}

impl Render for FileDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.error(cx);
        let can_submit = self.can_submit(cx);
        let subject = self.subject(&mut cx.default_global::<Localization>().0);
        let rename = matches!(self.input.request, FileRefactorRequest::Rename { .. });
        let mut body = div().px(px(16.0)).py(px(4.0)).flex().flex_col().gap(px(8.0));
        if rename {
            if let Some(name) = &self.name {
                let field = field_frame(cx, error.is_some()).child(div().flex_1().min_w_0().child(name.clone()));
                body = body.child(SharedString::from(subject)).child(field);
            }
        } else {
            body = body.child(div().text_color(theme::color(cx, "fg-emphasis")).child(SharedString::from(subject)));
            if let Some(name) = &self.name {
                body = body.child(self.render_field(cx, "New name:", name, false, error.is_some()));
            }
            if let Some(target) = &self.target {
                let field = self.render_field(cx, "To directory:", target, true, false);
                let browse = button(cx, String::new(), false, false, !self.busy)
                    .min_w(px(BUTTON_HEIGHT))
                    .px(px(0.0))
                    .child(theme::icon(cx, "folder", 14.0, theme::color(cx, "fg")))
                    .id("file-dialog-browse")
                    .debug_selector(|| "file-dialog-browse".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.browse(cx)));
                let row = div().flex().items_end().gap(px(6.0)).child(div().flex_1().min_w_0().child(field));
                body = body.child(row.child(browse));
                if let Some(hint) = self.hint(cx) {
                    let hint = tx(cx, &hint);
                    body = body.child(small_text(cx, "fg-subtle", hint));
                }
            }
        }
        if let Some(error) = error {
            body = body.child(small_text(cx, "danger", error));
        }
        let boxes: Vec<_> = self
            .checkboxes()
            .into_iter()
            .map(|(id, label, on)| {
                let label = tx(cx, label);
                checkbox(cx, label, on, !self.busy)
                    .id(SharedString::from(format!("file-dialog-{id}")))
                    .debug_selector(move || format!("file-dialog-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle(id, cx)))
            })
            .collect();
        body = body.child(div().mt(px(4.0)).flex().flex_col().gap(px(6.0)).children(boxes));
        let copy = matches!(self.input.request, FileRefactorRequest::Copy { .. });
        let ok = tx(cx, if copy { "OK" } else { "Refactor" });
        let cancel = tx(cx, "Cancel");
        let buttons = vec![
            button(cx, ok, true, false, can_submit)
                .id("file-dialog-ok")
                .debug_selector(|| "file-dialog-ok".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.submit(cx)))
                .into_any_element(),
            button(cx, cancel, false, false, true)
                .id("file-dialog-cancel")
                .debug_selector(|| "file-dialog-cancel".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.cancel(cx)))
                .into_any_element(),
        ];
        let busy = (self.busy && !copy).then(|| {
            let text = tx(cx, "Looking for references…");
            small_text(cx, "fg-muted", text)
        });
        let title = tx(cx, self.title());
        let width = if rename { 460.0 } else { 520.0 };
        dialog_frame(cx, width, title, body, busy, buttons.into_iter().map(|button| div().child(button)).collect())
    }
}

// ---------------------------------------------------------------------------
// Delete

/// What the Delete dialog answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDialogEvent {
    /// `None` when it was cancelled, else the state of "Safe delete": resume the flow with
    /// [`delete_reply`] and close the dialog.
    Answered(Option<bool>),
}

/// The operation's answer to the Delete dialog.
pub fn delete_reply(answer: Option<bool>) -> Reply {
    Reply::Confirmed(answer)
}

pub struct DeleteDialog {
    question: DeleteQuestion,
    checked: bool,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<DeleteDialogEvent> for DeleteDialog {}

impl DeleteDialog {
    /// The dialog for the question of [`crate::project_view::operations::Request::ConfirmDelete`].
    pub fn new(question: DeleteQuestion, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        let checked = question.checkbox.as_ref().is_some_and(|checkbox| checkbox.initial);
        Self { question, checked, focus, key_claim }
    }

    pub fn question(&self) -> &DeleteQuestion {
        &self.question
    }

    /// The state of "Safe delete", when it is offered.
    pub fn checked(&self) -> Option<bool> {
        self.question.checkbox.as_ref().map(|_| self.checked)
    }

    fn answer(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        cx.emit(DeleteDialogEvent::Answered(confirmed.then_some(self.checked && self.question.checkbox.is_some())));
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.question.checkbox.is_some() {
            self.checked = !self.checked;
            cx.notify();
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.answer(true, cx),
            "escape" => self.answer(false, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for DeleteDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DeleteDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let warning = theme::icon(cx, "warning", 16.0, theme::color(cx, "warning"));
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(SharedString::from(self.question.message.clone()))
            .child(SharedString::from(self.question.note.clone()));
        let mut body = div()
            .px(px(16.0))
            .py(px(4.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(div().flex().items_start().gap(px(8.0)).child(warning).child(text));
        if let Some(check) = &self.question.checkbox {
            let label = tx(cx, &check.label);
            body = body.child(
                checkbox(cx, label, self.checked, true)
                    .id("delete-safe")
                    .debug_selector(|| "delete-safe".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle(cx))),
            );
        }
        let ok = tx(cx, self.question.ok_label);
        let cancel = tx(cx, "Cancel");
        let buttons = vec![
            button(cx, ok, true, true, true)
                .id("delete-ok")
                .debug_selector(|| "delete-ok".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.answer(true, cx))),
            button(cx, cancel, false, false, true)
                .id("delete-cancel")
                .debug_selector(|| "delete-cancel".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.answer(false, cx))),
        ];
        let title = tx(cx, self.question.title);
        let buttons = buttons.into_iter().map(|button| div().child(button)).collect();
        dialog_frame(cx, 420.0, title, body, None, buttons).track_focus(&self.focus)
    }
}

// ---------------------------------------------------------------------------
// The questions of a safe delete

/// "Usages Detected" for [`crate::project_view::operations::Request::UsagesDetected`], to show
/// with [`crate::vcs::prompt_dialogs::PromptDialog::ask`]; its answer is [`usages_reply`].
pub fn usages_detected_request(i18n: &mut I18n, subject: &str, usages: &[Location], plural: bool) -> AskRequest {
    let question = UsagesQuestion::new(i18n, subject, usages, plural);
    AskRequest {
        title: question.title.to_owned(),
        body: question.body,
        warning: true,
        buttons: vec![
            AskButton { id: "view", label: question.view_label, primary: true },
            AskButton { id: "delete", label: question.delete_label, primary: false },
        ],
        ..AskRequest::default()
    }
}

/// The operation's answer to "Usages Detected": closing it any other way cancels.
pub fn usages_reply(answer: &DialogAnswer) -> Reply {
    let answer = match answer.button.as_str() {
        "view" => UsagesAnswer::View,
        "delete" => UsagesAnswer::Delete,
        _ => UsagesAnswer::Cancel,
    };
    Reply::UsagesAnswer(answer)
}

/// The operation's answer to [`crate::project_view::operations::search_progress_question`]:
/// "Delete Without Search", else cancel. When the workbench hides the question itself
/// ([`crate::project_view::operations::Effect::HideSearchProgress`]) nothing is answered.
pub fn progress_reply(answer: &DialogAnswer) -> Reply {
    let answer = if answer.button == "skip" { ProgressAnswer::Skip } else { ProgressAnswer::Cancel };
    Reply::SearchProgress(answer)
}

// ---------------------------------------------------------------------------
// Refactoring Preview

/// A file the preview lists: its path and its edits, ordered by position.
#[derive(Debug, Clone)]
struct PreviewFile {
    path: String,
    edits: Vec<TextEdit>,
}

/// What the preview is opened with.
#[derive(Debug, Clone)]
pub struct PreviewInput {
    /// What the refactoring is called, in the UI language: "Move 'util.ts' to 'b'".
    pub title: String,
    /// The reference updates collected for the moves.
    pub edit: WorkspaceEdit,
    /// The project root, which the files' folders are shown relative to.
    pub root: String,
}

/// What the preview answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefactoringPreviewEvent {
    /// Do Refactor: the edit without the files left out; carry out the moves with it.
    Confirmed(WorkspaceEdit),
    /// Cancel or Escape: nothing is moved.
    Cancelled,
}

/// The UTF-16 offset `units` of `text` as a byte offset, clamped to its end.
fn byte_at(text: &str, units: usize) -> usize {
    let mut counted = 0;
    for (byte, c) in text.char_indices() {
        if counted >= units {
            return byte;
        }
        counted += c.len_utf16();
    }
    text.len()
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// An occurrence on its line, trimmed: the text before it, the text it replaces, the new text and
/// the text after it. A range over several lines replaces the rest of its first line.
pub fn occurrence_parts(line: &str, edit: &TextEdit) -> (String, String, String, String) {
    let start = edit.range.start.col as usize;
    let end = if edit.range.end.line == edit.range.start.line { edit.range.end.col as usize } else { utf16_len(line) };
    let trimmed = line.trim();
    let lead = utf16_len(line) - utf16_len(line.trim_start());
    let from = start.saturating_sub(lead);
    let to = end.saturating_sub(lead).max(from);
    let (from, to) = (byte_at(trimmed, from), byte_at(trimmed, to));
    (trimmed[..from].to_owned(), trimmed[from..to].to_owned(), edit.new_text.clone(), trimmed[to..].to_owned())
}

pub struct RefactoringPreview {
    title: String,
    edit: WorkspaceEdit,
    root: String,
    files: Vec<PreviewFile>,
    excluded: BTreeSet<String>,
    /// The lines of the files, as far as they were read.
    lines: BTreeMap<String, Vec<String>>,
    code_font: SharedString,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<RefactoringPreviewEvent> for RefactoringPreview {}

impl RefactoringPreview {
    /// The preview of `input`. Its occurrences show their lines once [`Self::set_lines`] has them.
    pub fn new(input: PreviewInput, cx: &mut Context<Self>) -> Self {
        let files = input
            .edit
            .files
            .iter()
            .filter_map(|file| match file {
                FileEdit::Edit { path, edits, .. } => {
                    let mut edits = edits.clone();
                    edits.sort_by_key(|edit| (edit.range.start.line, edit.range.start.col));
                    Some(PreviewFile { path: to_ipc(path), edits })
                }
                FileEdit::Create { .. } | FileEdit::Rename { .. } | FileEdit::Delete { .. } => None,
            })
            .collect();
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| {
                claimant.update(cx, |preview, cx| preview.claim_key(keystroke, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|preview, cx| input::release_keys(preview.key_claim, cx)).detach();
        Self {
            title: input.title,
            edit: input.edit,
            root: input.root,
            files,
            excluded: BTreeSet::new(),
            lines: BTreeMap::new(),
            code_font: code_font(cx),
            focus,
            key_claim,
        }
    }

    /// The files whose references change, in the order of the edit.
    pub fn file_paths(&self) -> Vec<String> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    /// The text of `path` (read from its editor or from disk), for its occurrences' lines.
    pub fn set_lines(&mut self, path: &str, lines: Vec<String>, cx: &mut Context<Self>) {
        self.lines.insert(path.to_owned(), lines);
        cx.notify();
    }

    /// Leaves the file out of the refactoring, or takes it back in.
    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.excluded.remove(path) {
            self.excluded.insert(path.to_owned());
        }
        cx.notify();
    }

    pub fn is_included(&self, path: &str) -> bool {
        !self.excluded.contains(path)
    }

    /// The dialog's title, in the UI language.
    pub fn title(&self, i18n: &mut I18n) -> String {
        i18n.tx_built(&["Refactoring Preview: ", ""], &[Param::from(self.title.as_str())])
    }

    /// "References to update: 1 occurrence in 1 file", over the files kept.
    pub fn summary(&self, i18n: &mut I18n) -> String {
        let kept: Vec<&PreviewFile> = self.files.iter().filter(|file| self.is_included(&file.path)).collect();
        let occurrences: usize = kept.iter().map(|file| file.edits.len()).sum();
        let count = [Param::from(occurrences)];
        let occurrences = if occurrences == 1 {
            i18n.tx_built(&["", " occurrence"], &count)
        } else {
            i18n.tx_built(&["", " occurrences"], &count)
        };
        let count = [Param::from(kept.len())];
        let files = i18n.tx_built(&["", if kept.len() == 1 { " file" } else { " files" }], &count);
        i18n.tx_built(&["References to update: ", " in ", ""], &[Param::from(occurrences), Param::from(files)])
    }

    /// The edit to carry out: the text edits of the files kept, with the edit's preconditions.
    pub fn kept_edit(&self) -> WorkspaceEdit {
        let files = self.edit.files.iter().filter(|file| match file {
            FileEdit::Edit { path, .. } => self.is_included(&to_ipc(path)),
            FileEdit::Create { .. } | FileEdit::Rename { .. } | FileEdit::Delete { .. } => false,
        });
        WorkspaceEdit { files: files.cloned().collect(), preconditions: self.edit.preconditions.clone() }
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        cx.emit(RefactoringPreviewEvent::Confirmed(self.kept_edit()));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(RefactoringPreviewEvent::Cancelled);
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.confirm(cx),
            "escape" => self.cancel(cx),
            _ => return false,
        }
        true
    }

    fn render_file(&self, index: usize, file: &PreviewFile, cx: &mut Context<Self>) -> Div {
        let name = name_of(&file.path).to_owned();
        let relative = relative_to(&self.root, &file.path);
        let shown_path = if relative.is_empty() { file.path.clone() } else { relative.to_owned() };
        let count = [Param::from(file.edits.len())];
        let i18n = &mut cx.default_global::<Localization>().0;
        let occurrences = if file.edits.len() == 1 {
            i18n.tx_built(&["", " occurrence"], &count)
        } else {
            i18n.tx_built(&["", " occurrences"], &count)
        };
        let included = self.is_included(&file.path);
        let path = file.path.clone();
        let toggle = checkbox(cx, String::new(), included, true)
            .id(SharedString::from(format!("preview-include-{index}")))
            .debug_selector(move || format!("preview-include-{index}"))
            .on_click(cx.listener(move |preview, _: &ClickEvent, _, cx| preview.toggle(&path, cx)));
        let fg = theme::color(cx, "fg");
        let header = div()
            .h(theme::length(cx, "h-row"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(toggle)
            .child(theme::icon(cx, theme::icons::file_icon_name(&name), 14.0, fg))
            .child(div().text_color(theme::color(cx, "fg-emphasis")).child(SharedString::from(name)))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(shown_path)),
            )
            .child(div().flex_1())
            .child(small_text(cx, "fg-muted", occurrences));
        let lines = self.lines.get(&file.path);
        let small = theme::length(cx, "fs-small");
        let occurrences = file.edits.iter().take(PREVIEW_OCCURRENCES).map(|edit| {
            let line = lines.and_then(|lines| lines.get(edit.range.start.line as usize)).map_or("", String::as_str);
            let (before, old, new, after) = occurrence_parts(line, edit);
            let code = div()
                .min_w_0()
                .flex()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(self.code_font.clone())
                .text_size(small)
                .text_color(theme::color(cx, "fg-editor"))
                .child(SharedString::from(before))
                .child(div().text_color(theme::color(cx, "fg-muted")).line_through().child(SharedString::from(old)))
                .child(
                    div()
                        .bg(theme::color(cx, "bg-search-match"))
                        .text_color(theme::color(cx, "fg-search-match"))
                        .child(SharedString::from(new)),
                )
                .child(SharedString::from(after));
            div()
                .h(theme::length(cx, "h-row"))
                .pl(px(28.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(28.0))
                        .flex()
                        .justify_end()
                        .font_family(self.code_font.clone())
                        .text_size(small)
                        .text_color(theme::color(cx, "fg-faint"))
                        .child(SharedString::from((edit.range.start.line + 1).to_string())),
                )
                .child(code)
        });
        div().flex().flex_col().child(header).children(occurrences.collect::<Vec<_>>())
    }
}

impl Focusable for RefactoringPreview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RefactoringPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, summary) = {
            let i18n = &mut cx.default_global::<Localization>().0;
            (self.title(i18n), self.summary(i18n))
        };
        let files: Vec<Div> =
            self.files.iter().enumerate().map(|(index, file)| self.render_file(index, file, cx)).collect();
        let summary = div()
            .px(px(14.0))
            .pt(px(4.0))
            .pb(px(8.0))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(summary));
        let list = div().id("preview-files").max_h(px(420.0)).overflow_y_scroll().px(px(14.0)).pb(px(10.0));
        let body = div().flex().flex_col().child(summary).child(list.children(files));
        let ok = tx(cx, "Do Refactor");
        let cancel = tx(cx, "Cancel");
        let buttons = vec![
            div().child(
                button(cx, ok, true, false, true)
                    .id("preview-ok")
                    .debug_selector(|| "preview-ok".to_owned())
                    .on_click(cx.listener(|preview, _: &ClickEvent, _, cx| preview.confirm(cx))),
            ),
            div().child(
                button(cx, cancel, false, false, true)
                    .id("preview-cancel")
                    .debug_selector(|| "preview-cancel".to_owned())
                    .on_click(cx.listener(|preview, _: &ClickEvent, _, cx| preview.cancel(cx))),
            ),
        ];
        dialog_frame(cx, 680.0, title, body, None, buttons).track_focus(&self.focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_ops::dialogs::PromptCheckbox;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{Pos, Range};
    use std::cell::RefCell;
    use std::rc::Rc;

    const ROOT: &str = "/w/acme";

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The view `build` makes, with the keyboard, and the events it emits.
    fn open<V, E>(
        cx: &mut TestAppContext,
        build: impl FnOnce(&mut Context<V>) -> V + 'static,
    ) -> (Entity<V>, Rc<RefCell<Vec<E>>>, &mut VisualTestContext)
    where
        V: Render + Focusable + EventEmitter<E> + 'static,
        E: Clone + 'static,
    {
        init(cx);
        let (view, cx) = cx.add_window_view(move |_, cx| build(cx));
        let events: Rc<RefCell<Vec<E>>> = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &E, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn input(request: FileRefactorRequest) -> FileDialogInput {
        let listings = BTreeMap::from([
            (ROOT.to_owned(), vec!["README.md".to_owned(), "src".to_owned()]),
            (format!("{ROOT}/src"), vec!["main.ts".to_owned()]),
        ]);
        FileDialogInput {
            request,
            root: ROOT.to_owned(),
            host_os: Os::Linux,
            options: RefactorOptions::default(),
            open_copy: false,
            listings,
        }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    /// Replaces the text of `field` as typed, with the keyboard in it.
    fn retype(field: &Entity<TextField>, text: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = field.focus_handle(cx);
            window.focus(&focus, cx);
        });
        field.update(cx, |field, cx| field.select_all(cx));
        cx.simulate_input(text);
    }

    fn moved(from: &str, to: &str) -> FileMove {
        FileMove { from: from.to_owned().into(), to: to.to_owned().into() }
    }

    // src/features/project-tree/ProjectView.test.tsx › ProjectView › renames in IDEA's Rename dialog with Shift+F6,
    // validating conflicts, and undoes with ⌘Z
    #[gpui_kit::test]
    async fn renames_in_ideas_rename_dialog_with_shift_f6_validating_conflicts_and_undoes_with_cmd_z(
        cx: &mut TestAppContext,
    ) {
        let path = format!("{ROOT}/README.md");
        let siblings = vec!["README.md".to_owned(), "src".to_owned()];
        let request = FileRefactorRequest::Rename { path: path.clone(), dir: false, siblings };
        let (dialog, events, cx) = open::<_, FileDialogEvent>(cx, move |cx| FileDialog::new(input(request), cx));
        let subject = dialog.read_with(cx, |dialog, _| dialog.subject(&mut I18n::default()));
        assert_eq!(subject, "Rename file 'README.md' and its usages to:");
        let boxes = dialog.read_with(cx, |dialog, _| dialog.checkboxes());
        assert_eq!(
            boxes,
            [("references", "Search for references", true), ("text", "Search in comments and strings", true)]
        );
        let field = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field");
        // The name's stem is selected.
        assert_eq!(field.read_with(cx, |field, _| field.selected_text().to_owned()), "README");
        retype(&field, "src", cx);
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.error(cx)).as_deref(), Some("'src' already exists"));
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.can_submit(cx)));
        click(cx, "file-dialog-ok");
        assert!(events.borrow().is_empty(), "Refactor is disabled");
        retype(&field, "GUIDE.md", cx);
        click(cx, "file-dialog-ok");
        let expected = FileDialogEvent::Refactor {
            label: RefactorLabel::Rename,
            moves: vec![moved(&path, &format!("{ROOT}/GUIDE.md"))],
            options: RefactorOptions::default(),
        };
        assert_eq!(*events.borrow(), [expected]);
        // While the references are collected only Cancel does anything: the name cannot be changed.
        dialog.update(cx, |dialog, cx| dialog.set_busy(true, cx));
        cx.simulate_input("x");
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "GUIDE.md");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().len(), 1);
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&FileDialogEvent::Cancelled));
    }

    // src/features/project-tree/ProjectView.test.tsx › ProjectView › moves with F6 through the Move dialog
    #[gpui_kit::test]
    async fn moves_with_f6_through_the_move_dialog(cx: &mut TestAppContext) {
        let path = format!("{ROOT}/README.md");
        let paths = vec![path.clone()];
        let request = FileRefactorRequest::Move { paths, target: ROOT.to_owned(), dir: false, clipboard: None };
        let (dialog, events, cx) = open::<_, FileDialogEvent>(cx, move |cx| FileDialog::new(input(request), cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.subject(&mut I18n::default())), "Move file 'README.md'");
        let boxes = dialog.read_with(cx, |dialog, _| dialog.checkboxes());
        assert!(boxes.contains(&("open", "Open moved files in editor", false)));
        let target = dialog.read_with(cx, |dialog, _| dialog.target_field().cloned()).expect("the target field");
        assert_eq!(target.read_with(cx, |field, _| field.text().to_owned()), ROOT);
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.error(cx)).as_deref(), Some("Already in this directory"));
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)).as_deref(), Some("Project root"));
        retype(&target, "src", cx);
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.hint(cx)).as_deref(), Some("src"));
        click(cx, "file-dialog-open");
        click(cx, "file-dialog-ok");
        let options = RefactorOptions { open_in_editor: true, ..RefactorOptions::default() };
        let expected = FileDialogEvent::Refactor {
            label: RefactorLabel::Move,
            moves: vec![moved(&path, &format!("{ROOT}/src/README.md"))],
            options,
        };
        assert_eq!(*events.borrow(), [expected]);
        assert_eq!(load_refactor_options(Some(&options.stored())), options);
    }

    // Not from a spec: a folder cannot move into itself; the browse button asks for a folder and
    // the picked one becomes the target.
    #[gpui_kit::test]
    async fn a_folder_cannot_move_into_itself_and_browse_picks_the_target(cx: &mut TestAppContext) {
        let src = format!("{ROOT}/src");
        let request =
            FileRefactorRequest::Move { paths: vec![src.clone()], target: ROOT.to_owned(), dir: true, clipboard: None };
        let (dialog, events, cx) = open::<_, FileDialogEvent>(cx, move |cx| FileDialog::new(input(request), cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.subject(&mut I18n::default())), "Move directory 'src'");
        click(cx, "file-dialog-browse");
        assert_eq!(*events.borrow(), [FileDialogEvent::Browse { current: ROOT.to_owned() }]);
        let inside = format!("{src}/nested");
        dialog.update(cx, |dialog, cx| dialog.set_target(&inside, cx));
        let error = dialog.update(cx, |dialog, cx| dialog.error(cx));
        assert_eq!(error.as_deref(), Some("Cannot move a folder into itself"));
        dialog.update(cx, |dialog, cx| dialog.set_target("/elsewhere/", cx));
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.destination(cx)), "/elsewhere");
        assert!(dialog.read_with(cx, |dialog, cx| dialog.can_submit(cx)));
        assert_eq!(resolve_target("C:\\work", ROOT), "C:\\work");
        assert_eq!(resolve_target("  ", ROOT), ROOT);
    }

    // Not from a spec: Copy offers a free name in its folder, checks it against the folder it copies
    // into once that one's names are known, and answers with the copy to make.
    #[gpui_kit::test]
    async fn copy_offers_a_free_name_and_checks_it_against_the_target(cx: &mut TestAppContext) {
        let path = format!("{ROOT}/README.md");
        let request = FileRefactorRequest::Copy { paths: vec![path.clone()], target: ROOT.to_owned(), dir: false };
        let (dialog, events, cx) = open::<_, FileDialogEvent>(cx, move |cx| FileDialog::new(input(request), cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.subject(&mut I18n::default())), "Copy file 'README.md'");
        let name = dialog.read_with(cx, |dialog, _| dialog.name_field().cloned()).expect("the name field");
        assert_eq!(name.read_with(cx, |field, _| field.text().to_owned()), "README copy.md");
        let target = dialog.read_with(cx, |dialog, _| dialog.target_field().cloned()).expect("the target field");
        retype(&target, "docs", cx);
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.error(cx)), None, "a folder not read yet holds no conflict");
        let docs = format!("{ROOT}/docs");
        dialog.update(cx, |dialog, cx| dialog.set_listing(&docs, vec!["README copy.md".to_owned()], cx));
        let error = dialog.update(cx, |dialog, cx| dialog.error(cx));
        assert_eq!(error.as_deref(), Some("'README copy.md' already exists"));
        retype(&name, "NOTES.md", cx);
        click(cx, "file-dialog-open-copy");
        cx.simulate_keystrokes("enter");
        let expected = FileDialogEvent::Copy {
            paths: vec![path],
            target: docs,
            name: Some("NOTES.md".to_owned()),
            open_in_editor: true,
        };
        assert_eq!(*events.borrow(), [expected]);
        assert!(dialog.read_with(cx, |dialog, _| dialog.is_busy()), "the dialog waits for the copy");
    }

    // src/features/project-tree/ProjectView.test.tsx › ProjectView › deletes the selection after confirmation
    #[gpui_kit::test]
    async fn deletes_the_selection_after_confirmation(cx: &mut TestAppContext) {
        let question = DeleteQuestion {
            title: "Delete",
            ok_label: "Delete",
            message: "Delete 'README.md'?".to_owned(),
            note: "Items are moved to the Trash.".to_owned(),
            checkbox: Some(PromptCheckbox { label: "Safe delete (with usage search)".to_owned(), initial: true }),
        };
        let (dialog, events, cx) = open::<_, DeleteDialogEvent>(cx, move |cx| DeleteDialog::new(question, cx));
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.question().message.clone()), "Delete 'README.md'?");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(true), "safe delete starts as stored");
        click(cx, "delete-safe");
        click(cx, "delete-ok");
        assert_eq!(*events.borrow(), [DeleteDialogEvent::Answered(Some(false))]);
        assert!(matches!(delete_reply(Some(false)), Reply::Confirmed(Some(false))));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&DeleteDialogEvent::Answered(None)));
    }

    // Not from a spec: "Usages Detected" offers View Usages first and Delete Anyway; the progress
    // question's button deletes without waiting; anything else cancels.
    #[test]
    fn the_safe_delete_questions_answer_the_operation() {
        let usage = Location {
            path: "/p/b.ts".to_owned().into(),
            range: Range { start: Pos { line: 0, col: 0 }, end: Pos { line: 0, col: 1 } },
            uri: None,
        };
        let request = usages_detected_request(&mut I18n::default(), "File 'a.ts'", &[usage], false);
        assert_eq!(request.title, "Usages Detected");
        let labels: Vec<&str> = request.buttons.iter().map(|button| button.label).collect();
        assert_eq!(labels, ["View Usages", "Delete Anyway"]);
        let answer = |button: &str| DialogAnswer { button: button.to_owned(), ..DialogAnswer::default() };
        assert!(matches!(usages_reply(&answer("view")), Reply::UsagesAnswer(UsagesAnswer::View)));
        assert!(matches!(usages_reply(&answer("delete")), Reply::UsagesAnswer(UsagesAnswer::Delete)));
        assert!(matches!(usages_reply(&answer("cancel")), Reply::UsagesAnswer(UsagesAnswer::Cancel)));
        assert!(matches!(progress_reply(&answer("skip")), Reply::SearchProgress(ProgressAnswer::Skip)));
        assert!(matches!(progress_reply(&answer("cancel")), Reply::SearchProgress(ProgressAnswer::Cancel)));
    }

    fn text_edit(line: u32, from: u32, to: u32, text: &str) -> TextEdit {
        TextEdit {
            range: Range { start: Pos { line, col: from }, end: Pos { line, col: to } },
            new_text: text.to_owned(),
        }
    }

    fn preview_input() -> PreviewInput {
        let edit = WorkspaceEdit {
            files: vec![FileEdit::Edit {
                path: "/p/main.ts".to_owned().into(),
                edits: vec![text_edit(0, 20, 26, "./b/util")],
                version: None,
            }],
            preconditions: Vec::new(),
        };
        PreviewInput { title: "Move 'util.ts' to 'b'".to_owned(), edit, root: "/p".to_owned() }
    }

    // src/features/project-tree/fileRefactor.test.tsx › runFileRefactoring › previews edits in other files, applies
    // the move and offers Undo
    #[gpui_kit::test]
    async fn previews_edits_in_other_files_applies_the_move_and_offers_undo(cx: &mut TestAppContext) {
        let (preview, events, cx) =
            open::<_, RefactoringPreviewEvent>(cx, |cx| RefactoringPreview::new(preview_input(), cx));
        let mut i18n = I18n::default();
        let title = preview.read_with(cx, |preview, _| preview.title(&mut i18n));
        let summary = preview.read_with(cx, |preview, _| preview.summary(&mut i18n));
        assert_eq!(title, "Refactoring Preview: Move 'util.ts' to 'b'");
        assert_eq!(summary, "References to update: 1 occurrence in 1 file");
        assert_eq!(preview.read_with(cx, |preview, _| preview.file_paths()), ["/p/main.ts"]);
        preview.update(cx, |preview, cx| {
            preview.set_lines("/p/main.ts", vec!["import { x } from \"./a/util\";".to_owned()], cx);
        });
        click(cx, "preview-ok");
        assert_eq!(*events.borrow(), [RefactoringPreviewEvent::Confirmed(preview_input().edit)]);
    }

    // src/features/project-tree/fileRefactor.test.tsx › runFileRefactoring › excluded files are left out and Cancel
    // keeps everything
    #[gpui_kit::test]
    async fn excluded_files_are_left_out_and_cancel_keeps_everything(cx: &mut TestAppContext) {
        let (preview, events, cx) =
            open::<_, RefactoringPreviewEvent>(cx, |cx| RefactoringPreview::new(preview_input(), cx));
        click(cx, "preview-cancel");
        assert_eq!(*events.borrow(), [RefactoringPreviewEvent::Cancelled]);
        click(cx, "preview-include-0");
        let summary = preview.read_with(cx, |preview, _| preview.summary(&mut I18n::default()));
        assert_eq!(summary, "References to update: 0 occurrences in 0 files");
        cx.simulate_keystrokes("enter");
        let kept = WorkspaceEdit { files: Vec::new(), preconditions: Vec::new() };
        assert_eq!(events.borrow().last(), Some(&RefactoringPreviewEvent::Confirmed(kept)));
    }

    // Not from a spec: an occurrence shows on its trimmed line, the replaced text apart, in UTF-16
    // columns; a range over several lines takes the rest of its first line.
    #[test]
    fn an_occurrence_is_cut_out_of_its_trimmed_line() {
        let line = "    import { x } from \"./a/util\";";
        let parts = occurrence_parts(line, &text_edit(0, 25, 31, "b/util"));
        let expected = ["import { x } from \"./", "a/util", "b/util", "\";"].map(str::to_owned);
        assert_eq!(<[String; 4]>::from(parts), expected);
        let wide = occurrence_parts("é = 'a.ts'", &text_edit(0, 5, 9, "b"));
        assert_eq!(wide.1, "a.ts");
        let mut multi = text_edit(0, 4, 2, "x");
        multi.range.end.line = 1;
        assert_eq!(occurrence_parts("abc def", &multi).1, "def");
    }
}
