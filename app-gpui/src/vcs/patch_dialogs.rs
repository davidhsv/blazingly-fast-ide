//! Create Patch and Apply Patch.
//!
//! [`CreatePatchDialog`] asks which changed files go into the patch (or names the shelf or the
//! commits it is made of), whether it is saved to a file or copied to the clipboard, and whether it
//! is reversed. [`ApplyPatchDialog`] lists the files of a patch as the backend previews them, each
//! with its line counts and whether it applies cleanly or a 3-way merge will be tried.
//!
//! Neither runs anything: the workbench reads, writes and applies the patch, and feeds the preview.

use super::change_dialogs::{
    ChecklistFile, button_row, check_row, checklist, claim_dialog_keys, dialog_body, dialog_button, dialog_frame,
    muted_line, radio,
};
use super::changes::{EffectiveKind, base_name, dir_name};
use super::commit_view::kind_color;
use super::prompt_dialogs::{CANCEL, OK, button, field_frame, tx};
use crate::platform::i18n::{Localization, Param};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_vcs::{ChangeKind, PatchFile};
use std::collections::HashSet;

/// The id of Create Patch's Browse… button.
pub const BROWSE: &str = "browse";

/// What a patch is created from, as the dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatePatchSource {
    /// Local changes, to check in a list.
    Changes(Vec<ChecklistFile>),
    /// A shelf (its name, or the shelved file) or commits, named by this line.
    Described(String),
}

/// How Create Patch was answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatePatchAnswer {
    /// The checked files (absolute paths); empty unless the patch is made of local changes.
    pub paths: Vec<String>,
    /// Save to the file `file`; otherwise copy to the clipboard.
    pub to_file: bool,
    pub file: String,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatePatchDialogEvent {
    Create(CreatePatchAnswer),
    /// Browse… was pressed with this file in the field: ask where to save the patch.
    Browse(String),
    Cancelled,
}

/// Create Patch: the changes to include, Save to file (with Browse…) or Copy to clipboard, and
/// Reverse patch (not for a shelf).
pub struct CreatePatchDialog {
    source: CreatePatchSource,
    checked: HashSet<String>,
    to_file: bool,
    file: Entity<TextField>,
    reverse: Option<bool>,
    focus: FocusHandle,
    _file_events: Subscription,
}

impl EventEmitter<CreatePatchDialogEvent> for CreatePatchDialog {}

impl CreatePatchDialog {
    /// `file` is the path suggested for the patch; `reversible` offers Reverse patch.
    pub fn new(
        source: CreatePatchSource,
        file: &str,
        reversible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let checked = match &source {
            CreatePatchSource::Changes(files) => files.iter().map(|file| file.path.clone()).collect(),
            CreatePatchSource::Described(_) => HashSet::new(),
        };
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(file, cx);
            field
        });
        let events =
            cx.subscribe_in(&field, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submitted => dialog.press(OK, cx),
                TextFieldEvent::Cancelled => dialog.press(CANCEL, cx),
            });
        let focus = cx.focus_handle();
        claim_dialog_keys(&[field.focus_handle(cx), focus.clone()], Self::press, cx);
        Self {
            source,
            checked,
            to_file: true,
            file: field,
            reverse: reversible.then_some(false),
            focus,
            _file_events: events,
        }
    }

    pub fn source(&self) -> &CreatePatchSource {
        &self.source
    }

    /// The checked files' paths, in the list's order.
    pub fn checked(&self) -> Vec<String> {
        match &self.source {
            CreatePatchSource::Changes(files) => {
                files.iter().filter(|file| self.checked.contains(&file.path)).map(|file| file.path.clone()).collect()
            }
            CreatePatchSource::Described(_) => Vec::new(),
        }
    }

    pub fn file_field(&self) -> &Entity<TextField> {
        &self.file
    }

    pub fn file(&self, cx: &App) -> String {
        self.file.read(cx).text().trim().to_owned()
    }

    pub fn to_file(&self) -> bool {
        self.to_file
    }

    /// Reverse patch's state; `None` when it is not offered.
    pub fn reverse(&self) -> Option<bool> {
        self.reverse
    }

    /// Create can be pressed: a change is checked (for local changes) and, to save, a file is named.
    pub fn can_create(&self, cx: &App) -> bool {
        let empty = matches!(self.source, CreatePatchSource::Changes(_)) && self.checked.is_empty();
        !empty && (!self.to_file || !self.file(cx).is_empty())
    }

    /// The path chosen with Browse….
    pub fn set_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.file.update(cx, |field, cx| field.set_text(path, cx));
        cx.notify();
    }

    /// Save to file (`true`) or Copy to clipboard.
    pub fn set_to_file(&mut self, to_file: bool, cx: &mut Context<Self>) {
        self.to_file = to_file;
        cx.notify();
    }

    pub fn toggle_reverse(&mut self, cx: &mut Context<Self>) {
        if let Some(reverse) = self.reverse.as_mut() {
            *reverse = !*reverse;
            cx.notify();
        }
    }

    pub fn toggle_path(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.checked.remove(path) {
            self.checked.insert(path.to_owned());
        }
        cx.notify();
    }

    /// Presses button `id`: [`OK`] (Create), [`BROWSE`] or [`CANCEL`].
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            CANCEL => cx.emit(CreatePatchDialogEvent::Cancelled),
            BROWSE if self.to_file => cx.emit(CreatePatchDialogEvent::Browse(self.file(cx))),
            OK if self.can_create(cx) => {
                let answer = CreatePatchAnswer {
                    paths: self.checked(),
                    to_file: self.to_file,
                    file: self.file(cx),
                    reverse: self.reverse == Some(true),
                };
                cx.emit(CreatePatchDialogEvent::Create(answer));
            }
            _ => {}
        }
    }
}

impl Focusable for CreatePatchDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CreatePatchDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Create Patch");
        let can = self.can_create(cx);
        let source = match &self.source {
            CreatePatchSource::Changes(files) => {
                let heading = tx(cx, "Changes to include:");
                let files = files.clone();
                div().flex().flex_col().gap(px(6.0)).child(SharedString::from(heading)).child(checklist(
                    cx,
                    "patch",
                    &files,
                    &self.checked,
                    Self::toggle_path,
                ))
            }
            CreatePatchSource::Described(line) => div().child(SharedString::from(line.clone())),
        };
        let to_file_label = tx(cx, "Save to file:");
        let clipboard_label = tx(cx, "Copy to clipboard");
        let to_file = radio(cx, to_file_label, self.to_file, true)
            .id("patch-to-file")
            .debug_selector(|| "patch-to-file".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.set_to_file(true, cx)));
        let clipboard = radio(cx, clipboard_label, !self.to_file, true)
            .id("patch-to-clipboard")
            .debug_selector(|| "patch-to-clipboard".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.set_to_file(false, cx)));
        let browse_label = tx(cx, "Browse…");
        let browse = button(cx, browse_label, false, false, self.to_file)
            .id("patch-button-browse")
            .debug_selector(|| "patch-button-browse".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press(BROWSE, cx)));
        let field = field_frame(cx, false)
            .flex_1()
            .min_w_0()
            .when(!self.to_file, |el| el.opacity(0.5))
            .child(div().flex_1().min_w_0().child(self.file.clone()));
        let reverse = self
            .reverse
            .map(|on| check_row(cx, "patch-reverse".to_owned(), "Reverse patch", on, true, Self::toggle_reverse));
        let buttons = vec![
            dialog_button(cx, "patch", OK, "Create", true, can, Self::press),
            dialog_button(cx, "patch", CANCEL, "Cancel", false, true, Self::press),
        ];
        dialog_frame(cx, 560.0, title)
            .track_focus(&self.focus)
            .child(
                dialog_body()
                    .child(source)
                    .child(to_file)
                    .child(div().flex().items_center().gap(px(8.0)).child(field).child(browse))
                    .child(clipboard)
                    .children(reverse),
            )
            .child(button_row(buttons))
    }
}

// -------------------------------------------------------------------------------------------------
// Apply Patch

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyPatchDialogEvent {
    Apply,
    Cancelled,
}

/// Apply Patch: `source` (the file's name, or Clipboard) in the title, the patch's files as the
/// backend previews them, and how many will be merged.
pub struct ApplyPatchDialog {
    source: String,
    /// `None` while the preview loads.
    preview: Option<Result<Vec<PatchFile>, String>>,
    focus: FocusHandle,
}

impl EventEmitter<ApplyPatchDialogEvent> for ApplyPatchDialog {}

/// The colour a patch file's name is drawn in.
fn patch_kind(kind: ChangeKind) -> EffectiveKind {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => EffectiveKind::Added,
        ChangeKind::Deleted => EffectiveKind::Deleted,
        ChangeKind::Renamed => EffectiveKind::Renamed,
        ChangeKind::Conflicted => EffectiveKind::Conflicted,
        ChangeKind::Ignored => EffectiveKind::Ignored,
        ChangeKind::Modified => EffectiveKind::Modified,
    }
}

impl ApplyPatchDialog {
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        claim_dialog_keys(std::slice::from_ref(&focus), Self::press, cx);
        Self { source: source.to_owned(), preview: None, focus }
    }

    /// The title, "Apply Patch: <source>", in the UI language.
    pub fn title(&self, cx: &mut App) -> String {
        cx.default_global::<Localization>().0.tx_built(&["Apply Patch: ", ""], &[Param::from(self.source.as_str())])
    }

    /// What the backend's preview found: the files, or why the patch cannot be read.
    pub fn set_preview(&mut self, preview: Result<Vec<PatchFile>, String>, cx: &mut Context<Self>) {
        self.preview = Some(preview);
        cx.notify();
    }

    pub fn is_loading(&self) -> bool {
        self.preview.is_none()
    }

    /// Why the patch cannot be applied, as the backend said.
    pub fn error(&self) -> Option<&str> {
        match &self.preview {
            Some(Err(error)) => Some(error),
            _ => None,
        }
    }

    fn files(&self) -> &[PatchFile] {
        match &self.preview {
            Some(Ok(files)) => files,
            _ => &[],
        }
    }

    /// "2 files in the patch", once previewed.
    pub fn summary(&self) -> Option<String> {
        let Some(Ok(files)) = &self.preview else { return None };
        Some(format!("{} file{} in the patch", files.len(), if files.len() == 1 { "" } else { "s" }))
    }

    /// One line per file: its name, where it comes from or its folder, the line counts (or
    /// "binary"), and "!" when it does not apply cleanly.
    pub fn rows(&self, cx: &mut App) -> Vec<String> {
        let i18n = &mut cx.default_global::<Localization>().0;
        self.files()
            .iter()
            .map(|file| {
                let place = match &file.old_path {
                    Some(old) => i18n.tx_built(&["from ", ""], &[Param::from(old.as_str())]),
                    None => dir_name(&file.path).to_owned(),
                };
                let counts =
                    if file.binary { i18n.tx("binary") } else { format!("+{} −{}", file.added, file.removed) };
                let mark = if file.applies { "" } else { "!" };
                let parts = [base_name(&file.path), place.as_str(), counts.as_str(), mark];
                parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
            })
            .collect()
    }

    /// "1 file will be merged; conflicts open in the merge tool", when files do not apply cleanly.
    pub fn hint(&self, cx: &mut App) -> Option<String> {
        let failing = self.files().iter().filter(|file| !file.applies).count();
        if failing == 0 {
            return None;
        }
        let unit = if failing == 1 {
            " file will be merged; conflicts open in the merge tool"
        } else {
            " files will be merged; conflicts open in the merge tool"
        };
        Some(cx.default_global::<Localization>().0.tx_built(&["", unit], &[Param::from(failing)]))
    }

    /// OK can be pressed: the preview lists files.
    pub fn can_apply(&self) -> bool {
        !self.files().is_empty()
    }

    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == CANCEL {
            cx.emit(ApplyPatchDialogEvent::Cancelled);
        } else if self.can_apply() {
            cx.emit(ApplyPatchDialogEvent::Apply);
        }
    }
}

impl Focusable for ApplyPatchDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ApplyPatchDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        let (muted, danger) = (theme::color(cx, "fg-muted"), theme::color(cx, "danger"));
        let content: AnyElement = match &self.preview {
            None => {
                let loading = tx(cx, "Loading…");
                muted_line(cx, loading).into_any_element()
            }
            Some(Err(error)) => div().text_color(danger).child(SharedString::from(error.clone())).into_any_element(),
            Some(Ok(files)) if files.is_empty() => {
                let empty = tx(cx, "No changes");
                muted_line(cx, empty).into_any_element()
            }
            Some(Ok(files)) => {
                let i18n = &mut cx.default_global::<Localization>().0;
                let (binary, clean, merged) =
                    (i18n.tx("binary"), i18n.tx("Applies cleanly"), i18n.tx("Does not apply cleanly"));
                let rows: Vec<AnyElement> = files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        let color = theme::color(cx, kind_color(patch_kind(file.kind)));
                        let name = base_name(&file.path).to_owned();
                        let place = match &file.old_path {
                            Some(old) => cx
                                .default_global::<Localization>()
                                .0
                                .tx_built(&["from ", ""], &[Param::from(old.as_str())]),
                            None => dir_name(&file.path).to_owned(),
                        };
                        let counts =
                            if file.binary { binary.clone() } else { format!("+{} −{}", file.added, file.removed) };
                        let state = if file.applies { clean.clone() } else { merged.clone() };
                        div()
                            .id(("patch-file", index))
                            .h(px(22.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .whitespace_nowrap()
                            .child(theme::icon(cx, theme::icons::file_icon_name(&name), 14.0, color))
                            .child(div().text_color(color).child(SharedString::from(name)))
                            .child(div().text_color(muted).child(SharedString::from(place)))
                            .child(div().flex_1())
                            .child(div().text_color(muted).child(SharedString::from(counts)))
                            .when(!file.applies, |el| {
                                el.child(theme::icon(cx, "warning", 14.0, theme::color(cx, "warning")))
                            })
                            .child(
                                div()
                                    .text_color(muted)
                                    .text_size(theme::length(cx, "fs-small"))
                                    .child(SharedString::from(state)),
                            )
                            .into_any_element()
                    })
                    .collect();
                div()
                    .id("patch-files")
                    .h(px(280.0))
                    .overflow_y_scroll()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .bg(theme::color(cx, "bg-input"))
                    .children(rows)
                    .into_any_element()
            }
        };
        let summary = self.summary().unwrap_or_default();
        let hint = self.hint(cx);
        let can = self.can_apply();
        let buttons = vec![
            dialog_button(cx, "apply-patch", OK, "OK", true, can, Self::press),
            dialog_button(cx, "apply-patch", CANCEL, "Cancel", false, true, Self::press),
        ];
        let footer = div()
            .flex()
            .items_center()
            .child(div().flex_1().pl(px(16.0)).children(hint.map(|hint| muted_line(cx, hint))))
            .child(button_row(buttons));
        dialog_frame(cx, 600.0, title)
            .track_focus(&self.focus)
            .child(dialog_body().child(SharedString::from(summary)).child(content))
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        cx.run_until_parked();
    }

    fn patch_file(path: &str, applies: bool) -> PatchFile {
        PatchFile {
            path: path.to_owned(),
            old_path: None,
            kind: ChangeKind::Modified,
            added: 2,
            removed: 1,
            binary: false,
            applies,
        }
    }

    /// Create Patch for local changes starts with every file checked, saving to the suggested file;
    /// Copy to clipboard and Reverse patch go into the answer, Browse… asks with the file named, and
    /// nothing checked disables Create.
    #[gpui_kit::test]
    async fn create_patch_answers_with_the_checked_files_and_where_the_patch_goes(cx: &mut TestAppContext) {
        init(cx);
        let files = vec![
            ChecklistFile {
                path: "/p/a.txt".into(),
                name: "a.txt".into(),
                dir: String::new(),
                kind: EffectiveKind::Modified,
            },
            ChecklistFile {
                path: "/p/b.txt".into(),
                name: "b.txt".into(),
                dir: String::new(),
                kind: EffectiveKind::Added,
            },
        ];
        let (dialog, cx) = cx.add_window_view(|window, cx| {
            CreatePatchDialog::new(CreatePatchSource::Changes(files), "/p/Changes.patch", true, window, cx)
        });
        let events: Rc<RefCell<Vec<CreatePatchDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &CreatePatchDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        click(cx, "patch-button-browse");
        assert_eq!(*events.borrow(), [CreatePatchDialogEvent::Browse("/p/Changes.patch".to_owned())]);
        dialog.update(cx, |dialog, cx| dialog.set_file("/elsewhere/x.patch", cx));
        click(cx, "patch-file-b.txt");
        click(cx, "patch-reverse");
        click(cx, "patch-button-ok");
        let saved = CreatePatchAnswer {
            paths: vec!["/p/a.txt".to_owned()],
            to_file: true,
            file: "/elsewhere/x.patch".to_owned(),
            reverse: true,
        };
        assert_eq!(events.borrow().last(), Some(&CreatePatchDialogEvent::Create(saved)));
        click(cx, "patch-to-clipboard");
        assert!(!dialog.read_with(cx, |d, _| d.to_file()));
        click(cx, "patch-file-a.txt");
        assert!(!dialog.read_with(cx, |d, cx| d.can_create(cx)), "nothing is checked");
    }

    /// A shelf's patch names the shelf and offers no Reverse patch.
    #[gpui_kit::test]
    async fn a_shelf_patch_names_the_shelf_without_reverse(cx: &mut TestAppContext) {
        init(cx);
        let (dialog, cx) = cx.add_window_view(|window, cx| {
            CreatePatchDialog::new(CreatePatchSource::Described("Work".to_owned()), "/p/Work.patch", false, window, cx)
        });
        assert_eq!(dialog.read_with(cx, |d, _| d.reverse()), None);
        assert!(dialog.read_with(cx, |d, cx| d.can_create(cx)));
        assert!(cx.debug_bounds("patch-reverse").is_none());
    }

    /// Apply Patch shows the files the preview found with their counts, says how many will be
    /// merged, and OK applies; a preview error shows instead and OK is disabled.
    #[gpui_kit::test]
    async fn apply_patch_lists_the_previewed_files_and_refuses_an_unreadable_patch(cx: &mut TestAppContext) {
        init(cx);
        let (dialog, cx) = cx.add_window_view(|_, cx| ApplyPatchDialog::new("fix.patch", cx));
        let events: Rc<RefCell<Vec<ApplyPatchDialogEvent>>> = Rc::default();
        let log = events.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &ApplyPatchDialogEvent, _| log.borrow_mut().push(event.clone()))
        });
        assert_eq!(dialog.update(cx, |d, cx| d.title(cx)), "Apply Patch: fix.patch");
        assert!(dialog.read_with(cx, |d, _| d.is_loading() && !d.can_apply()));
        dialog
            .update(cx, |d, cx| d.set_preview(Ok(vec![patch_file("src/a.txt", true), patch_file("b.txt", false)]), cx));
        assert_eq!(dialog.read_with(cx, |d, _| d.summary()).as_deref(), Some("2 files in the patch"));
        assert_eq!(dialog.update(cx, |d, cx| d.rows(cx)), ["a.txt src +2 −1", "b.txt +2 −1 !"]);
        assert_eq!(
            dialog.update(cx, |d, cx| d.hint(cx)).as_deref(),
            Some("1 file will be merged; conflicts open in the merge tool")
        );
        click(cx, "apply-patch-button-ok");
        assert_eq!(*events.borrow(), [ApplyPatchDialogEvent::Apply]);

        dialog.update(cx, |d, cx| d.set_preview(Err("no file changes found in the patch".to_owned()), cx));
        assert_eq!(
            dialog.read_with(cx, |d, _| d.error().map(str::to_owned)).as_deref(),
            Some("no file changes found in the patch")
        );
        click(cx, "apply-patch-button-ok");
        assert_eq!(events.borrow().len(), 1, "OK is disabled");
    }
}
