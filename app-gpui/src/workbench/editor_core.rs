//! What the editor's actions ask of the window: Back / Forward and the edit locations across the
//! files of the window, Copy Path / Copy Path with Line Number, the editor hint, Reload from Disk,
//! the read-only attribute, Reset Font Size in every editor and Paste from History.
//!
//! The window records where the caret has been: every caret move of its active editor updates the
//! current place of [`NavigationHistory`], an editor taking the keyboard is a navigation (a file
//! switch leaves the previous place behind), and every edit is a place of [`EditHistory`].

use super::services_window::on_workbench;
use super::{Content, Workbench, Workbenches};
use crate::backend::Backend;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::nav_history::{EditHistory, NavigationHistory, Place};
use crate::editor::paste_history_view::{PasteHistoryDialog, PasteHistoryEvent};
use crate::editor::view::{EditOrigin, Editor, EditorClipboard, EditorEvent};
use crate::editor_commands::copy_reference::relative_path;
use crate::editor_commands::extras::{paste_chosen, paste_from_history_hint};
use crate::editor_lsp::hint::Tone;
use crate::editor_model::file_attributes::{ToggleTarget, confirmed, toggle_failure_title, toggle_read_only};
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::platform::i18n::Localization;
use crate::platform::local_time::now_ms;
use crate::project_view::copy_path_popup::{PATH_WITH_LINE, copied_hint};
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_ops::dialogs::ConfirmRequest;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{App, AppContext as _, ClipboardItem, Context, Entity, Focusable, Window};
use serde_json::Value;
use std::path::PathBuf;

/// The action that copies the edited file's absolute path.
const COPY_PATH: &str = "editor.copyPath";

/// The places the caret has been in the window's editors.
#[derive(Debug, Default)]
pub(super) struct EditorCore {
    nav: NavigationHistory,
    edits: EditHistory,
}

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("navigate.back", |_, window, cx| on_workbench(window, cx, Workbench::navigate_back));
    registry.handle("navigate.forward", |_, window, cx| on_workbench(window, cx, Workbench::navigate_forward));
    registry
        .handle("navigate.lastEditLocation", |_, window, cx| on_workbench(window, cx, Workbench::last_edit_location));
    registry
        .handle("navigate.nextEditLocation", |_, window, cx| on_workbench(window, cx, Workbench::next_edit_location));
    registry.handle("editor.showHint", |call, window, cx| {
        let text = call.arg.and_then(|arg| arg.get("text")).and_then(Value::as_str).unwrap_or_default().to_owned();
        let tone = match call.arg.and_then(|arg| arg.get("tone")).and_then(Value::as_str) {
            Some("warning") => Tone::Warning,
            Some("error") => Tone::Error,
            _ => Tone::Info,
        };
        on_workbench(window, cx, move |workbench, _, cx| workbench.show_hint_toned(text, tone, cx))
    });
    registry.handle(COPY_PATH, |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            if let Some(path) = workbench.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()) {
                workbench.copy_text(COPY_PATH, path, cx);
            }
        })
    });
    registry.handle(PATH_WITH_LINE, |call, window, cx| {
        let given = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |workbench, _, cx| {
            if let Some(text) = given.or_else(|| workbench.path_with_line(cx)) {
                workbench.copy_text(PATH_WITH_LINE, text, cx);
            }
        })
    });
    registry.handle("file.reloadFromDisk", |call, window, cx| {
        let paths: Vec<String> = match call.arg {
            Some(Value::String(path)) => vec![path.clone()],
            Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
            _ => Vec::new(),
        };
        on_workbench(window, cx, move |workbench, _, cx| {
            let paths = if paths.is_empty() { workbench.active_path(cx).into_iter().collect() } else { paths };
            workbench.reload_from_disk(&paths, cx);
        })
    });
    registry.handle("file.reloadAllFromDisk", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            let clean: Vec<String> = workbench
                .window_editors()
                .iter()
                .map(|editor| editor.read(cx))
                .filter(|editor| !editor.is_dirty())
                .map(|editor| editor.path().to_string_lossy().into_owned())
                .collect();
            workbench.reload_from_disk(&clean, cx);
        })?;
        // The project tree and the VCS status are read again too, where they are available.
        for id in ["project.refresh", "vcs.refresh"] {
            if cx.global::<ActionRegistry>().contains(id) {
                execute_action(id, None, window, cx);
            }
        }
        Ok(())
    });
    registry.handle("file.toggleReadOnly", |_, window, cx| on_workbench(window, cx, Workbench::toggle_read_only));
    registry.handle("editor.resetFontSize", |_, _, cx| {
        reset_font_size(cx);
        Ok(())
    });
    registry.handle("code.pasteFromHistory", |_, window, cx| on_workbench(window, cx, Workbench::show_paste_history));
    registry.handle("editor.pasteHistory.paste", |call, window, cx| {
        let only = call.arg.and_then(Value::as_str).map(str::to_owned);
        on_workbench(window, cx, move |workbench, _, cx| {
            if let Some(dialog) = workbench.paste_history_dialog() {
                dialog.update(cx, |dialog, cx| dialog.paste(only.as_deref(), cx));
            }
        })
    });
    registry.handle("editor.pasteHistory.remove", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            if let Some(dialog) = workbench.paste_history_dialog() {
                dialog.update(cx, PasteHistoryDialog::remove);
            }
        })
    });
}

/// Reset Font Size in All Editors: every editor of every window back to the configured size.
fn reset_font_size(cx: &mut App) {
    let workbenches: Vec<Entity<Workbench>> = cx
        .try_global::<Workbenches>()
        .map(|all| all.0.values().filter_map(gpui_kit::WeakEntity::upgrade).collect())
        .unwrap_or_default();
    for workbench in workbenches {
        let editors = workbench.read(cx).window_editors();
        for editor in editors {
            editor.update(cx, |editor, cx| editor.set_font_zoom(0, cx));
        }
    }
}

/// Where the caret of `editor` is, as the histories keep it.
fn place_of(editor: &Editor) -> Place {
    let (line, col) = editor.caret_position();
    Place { path: editor.path().to_string_lossy().into_owned(), line, col }
}

impl Workbench {
    /// Follows `editor` for the histories of the window: its caret moves, its edits and the keyboard
    /// coming to it.
    pub(super) fn install_editor_core(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        cx.subscribe_in(editor, window, |workbench, editor, event: &EditorEvent, _, cx| match event {
            EditorEvent::SelectionMoved => workbench.record_place(editor, false, cx),
            EditorEvent::Edited { origin, .. } => {
                if !matches!(origin, EditOrigin::External | EditOrigin::History) {
                    let place = place_of(editor.read(cx));
                    workbench.editor_core.edits.record(place);
                }
                workbench.record_place(editor, false, cx);
            }
            EditorEvent::Hovered { .. } | EditorEvent::GutterPressed { .. } => {}
        })
        .detach();
        let focused = editor.downgrade();
        cx.on_focus_in(&editor.focus_handle(cx), window, move |workbench, _, cx| {
            if let Some(editor) = focused.upgrade() {
                workbench.record_place(&editor, true, cx);
            }
        })
        .detach();
    }

    /// Updates the current place of Back / Forward from the window's active editor.
    fn record_place(&mut self, editor: &Entity<Editor>, navigation: bool, cx: &App) {
        if self.editor() != Some(editor) {
            return;
        }
        let place = place_of(editor.read(cx));
        self.editor_core.nav.update(place, navigation, now_ms() as u64);
    }

    /// The editors of this window: every tab's, or the single-file window's.
    pub(super) fn window_editors(&self) -> Vec<Entity<Editor>> {
        match &self.content {
            Content::Project { editors, .. } => editors.values().cloned().collect(),
            Content::Editor(editor) => vec![editor.clone()],
            _ => Vec::new(),
        }
    }

    fn active_path(&self, cx: &App) -> Option<String> {
        self.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned())
    }

    /// Opens `place`: its file's tab in a project, the caret on its line and column.
    fn go_to_place(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        match &self.content {
            Content::Project { .. } => {
                self.open_project_file_at(PathBuf::from(place.path), place.line, place.col, window, cx);
            }
            Content::Editor(editor) if editor.read(cx).path().to_string_lossy() == place.path => {
                let editor = editor.clone();
                editor.update(cx, |editor, cx| {
                    let offset = editor.offset_of(place.line, place.col);
                    editor.select(offset, offset, cx);
                });
                window.focus(&editor.focus_handle(cx), cx);
            }
            _ => {}
        }
    }

    /// Navigate › Back (⌘[): the place before the current one in the history.
    pub fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.editor().cloned() {
            self.record_place(&editor, false, cx);
        }
        let Some(location) = self.editor_core.nav.back().cloned() else { return };
        let place = Place { path: location.path, line: location.line, col: location.col };
        self.go_to_place(place, window, cx);
    }

    /// Navigate › Forward (⌘]): the place Back left.
    pub fn navigate_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(location) = self.editor_core.nav.forward().cloned() else { return };
        let place = Place { path: location.path, line: location.line, col: location.col };
        self.go_to_place(place, window, cx);
    }

    /// Navigate › Last Edit Location (⇧⌘⌫); repeated, it walks back through older edits.
    pub fn last_edit_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let caret = self.editor().map(|editor| place_of(editor.read(cx)));
        let Some(place) = self.editor_core.edits.back(caret.as_ref()).cloned() else { return };
        if let Some(editor) = self.editor().cloned() {
            self.record_place(&editor, false, cx);
        }
        self.go_to_place(place, window, cx);
    }

    /// Navigate › Next Edit Location: back towards the newest edit.
    pub fn next_edit_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(place) = self.editor_core.edits.forward().cloned() else { return };
        if let Some(editor) = self.editor().cloned() {
            self.record_place(&editor, false, cx);
        }
        self.go_to_place(place, window, cx);
    }

    /// The editor hint: a short message about what an editor command did or could not do, shown at the
    /// caret of the active editor until the next hint replaces it. Without an editor nothing shows.
    pub fn show_hint(&mut self, text: String, cx: &mut Context<Self>) {
        self.show_hint_toned(text, Tone::Info, cx);
    }

    /// [`Workbench::show_hint`] in `tone`.
    pub fn show_hint_toned(&mut self, text: String, tone: Tone, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned().filter(|_| !text.is_empty()) else { return };
        // Shown once the current effects are delivered: the caret move of the command would hide it.
        cx.defer(move |cx| {
            if let Some(intelligence) = EditorIntelligence::of(&editor, cx) {
                intelligence.update(cx, |this, cx| this.show_hint_at(&text, None, tone, None, cx));
            }
        });
    }

    /// Puts `text` on the clipboard and in the clipboard history, and says so in the hint.
    fn copy_text(&mut self, action: &str, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        cx.default_global::<EditorClipboard>().0.record(&text, false);
        let hint = copied_hint(&mut cx.default_global::<Localization>().0, action, &text);
        self.show_hint(hint, cx);
    }

    /// The edited file's path from the project's root with the caret's 1-based line: `src/a.rs:12`.
    fn path_with_line(&self, cx: &App) -> Option<String> {
        let editor = self.editor()?.read(cx);
        let path = editor.path().to_string_lossy().into_owned();
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        Some(format!("{}:{}", relative_path(&path, root.as_deref()), editor.caret_position().0 + 1))
    }

    /// Reload from Disk: the open documents of `paths` read their files again. A document with unsaved
    /// changes is left as it is, with a warning.
    fn reload_from_disk(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let editors = self.window_editors();
        for path in paths {
            let Some(editor) = editors.iter().find(|editor| editor.read(cx).path().to_string_lossy() == **path) else {
                continue;
            };
            if editor.read(cx).is_dirty() {
                let name = path.rsplit('/').next().unwrap_or(path);
                let request = NotificationRequest {
                    body: Some("Save or roll back the changes before reloading it from disk.".to_owned()),
                    ..NotificationRequest::new(NotificationLevel::Warning, format!("{name} has unsaved changes"))
                };
                self.notify(request, cx);
                continue;
            }
            let encoding = editor.read(cx).status().encoding;
            if let Err(error) = editor.update(cx, |editor, cx| editor.set_encoding(&encoding, true, cx)) {
                let request = NotificationRequest {
                    body: Some(error.message),
                    ..NotificationRequest::new(NotificationLevel::Error, "Cannot reload from disk")
                };
                self.notify(request, cx);
            }
        }
    }

    /// File › File Properties › Make File Read-only / Writable: asks, then sets the file's writable bit
    /// and whether the editor accepts edits.
    pub fn toggle_read_only(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let (path, read_only) = {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.is_read_only())
        };
        let target = ToggleTarget { path: &path, virtual_document: false, read_only, banner: None };
        let Some(question) = toggle_read_only(&target, &cx.default_global::<Localization>().0) else { return };
        let request = ConfirmRequest {
            title: question.title,
            message: question.message,
            ok_label: question.confirm,
            danger: false,
        };
        let answer = self.show_prompt_dialog(move |window, cx| PromptDialog::confirm(request, window, cx), window, cx);
        let label = self.label.clone();
        cx.spawn_in(window, async move |workbench, cx| {
            let Ok(answer) = answer.await else { return };
            let target = ToggleTarget { path: &path, virtual_document: false, read_only, banner: None };
            let Some(writable) = confirmed(&target, answer.confirmed()) else { return };
            let file = PathBuf::from(&path);
            let set = cx.update(|_, cx| {
                cx.global::<Backend>().with_host(move |host| async move {
                    ide_host::ops::fs::set_writable(&host, &label, file, writable)
                })
            });
            let Ok(set) = set else { return };
            let result = set.await;
            let _ = workbench.update(cx, |workbench, cx| match result {
                Ok(Ok(read_only)) => editor.update(cx, |editor, cx| editor.set_read_only(read_only, cx)),
                Ok(Err(error)) => {
                    let request = NotificationRequest {
                        body: Some(error.message),
                        ..NotificationRequest::new(NotificationLevel::Error, toggle_failure_title(&path, writable))
                    };
                    workbench.notify(request, cx);
                }
                Err(error) => {
                    let request = NotificationRequest {
                        body: Some(format!("{error:#}")),
                        ..NotificationRequest::new(NotificationLevel::Error, toggle_failure_title(&path, writable))
                    };
                    workbench.notify(request, cx);
                }
            });
        })
        .detach();
    }

    /// The Paste from History dialog, while it is open.
    pub fn paste_history_dialog(&self) -> Option<Entity<PasteHistoryDialog>> {
        self.vcs_dialog_view::<PasteHistoryDialog>()
    }

    /// Edit › Paste from History… (⇧⌘V): the dialog over the clipboard history; with an empty history
    /// the hint says so.
    pub fn show_paste_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let history = cx.default_global::<EditorClipboard>().0.clone();
        if let Some(hint) = paste_from_history_hint(&history) {
            let hint = cx.default_global::<Localization>().0.tx(hint);
            self.show_hint(hint, cx);
            return;
        }
        let read_only = editor.read(cx).is_read_only();
        let dialog = cx.new(|cx| PasteHistoryDialog::new(history.entries().to_vec(), read_only, cx));
        let target = editor.downgrade();
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &PasteHistoryEvent, window, cx| {
            match event {
                PasteHistoryEvent::Paste { text, linewise } => {
                    workbench.close_vcs_dialog(window, cx);
                    if let Some(editor) = target.upgrade() {
                        paste_into(&editor, text, *linewise, cx);
                    }
                }
                PasteHistoryEvent::Remove(texts) => {
                    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
                    cx.default_global::<EditorClipboard>().0.remove(&texts);
                }
                PasteHistoryEvent::Cancelled => workbench.close_vcs_dialog(window, cx),
            }
            cx.notify();
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }
}

/// Pastes the text chosen in Paste from History into `editor`: it goes on top of the clipboard
/// history and on the clipboard, and pastes as a paste of the clipboard does.
fn paste_into(editor: &Entity<Editor>, text: &str, linewise: bool, cx: &mut App) {
    let state = editor.read(cx).edit_state();
    let edit = paste_chosen(&state, &mut cx.default_global::<EditorClipboard>().0, text, linewise);
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    if let Some(edit) = edit {
        editor.update(cx, |editor, cx| {
            if !editor.apply_edit(&edit.changes, edit.selections, edit.main, cx) {
                tracing::warn!("Paste from History: the editor refused the edit");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::editor_hint;
    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::actions::execute_action_or_throw;
    use crate::platform::input;
    use crate::platform::storage::UiStorage;
    use crate::theme;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::path::Path;
    use std::time::Duration;

    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            super::super::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            super::super::menu::init(cx);
        });
        home
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    /// A project of `files` open in a window.
    fn project<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
            std::fs::write(path, text).expect("the file is written");
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        (folder, root, workbench, cx)
    }

    /// Opens `path` in its tab and waits for its editor to have the keyboard.
    fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
        let path = path.to_path_buf();
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        settle(cx, |cx| {
            cx.update(|window, cx| {
                workbench
                    .read(cx)
                    .editor()
                    .is_some_and(|e| e.read(cx).path() == path && e.focus_handle(cx).is_focused(window))
            })
        });
        workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
    }

    fn run(cx: &mut VisualTestContext, id: &str, arg: Option<Value>) {
        cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        cx.run_until_parked();
    }

    /// The active editor's file name and caret line and column.
    fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(String, u32, u32)> {
        workbench.read_with(cx, |w, cx| {
            let editor = w.editor()?.read(cx);
            let name = editor.path().file_name()?.to_string_lossy().into_owned();
            let (line, col) = editor.caret_position();
            Some((name, line, col))
        })
    }

    fn caret_to(editor: &Entity<Editor>, line: u32, col: u32, cx: &mut VisualTestContext) {
        editor.update(cx, |editor, cx| {
            let offset = editor.offset_of(line, col);
            editor.select(offset, offset, cx);
        });
        cx.run_until_parked();
    }

    fn titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        workbench.read_with(cx, |w, _| w.notifications().items().iter().map(|n| n.title.clone()).collect())
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn close(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    const FILES: &[(&str, &str)] = &[
        ("src/a.rs", "pub fn place() {}\n\nfn second() {}\nfn third() {}\n"),
        ("src/b.rs", "fn one() {}\nfn two() {}\nfn three() {}\n"),
    ];

    /// Back returns to the file and caret the editor showed before a file switch; Forward goes to where
    /// Back came from.
    #[gpui_kit::test]
    async fn back_and_forward_move_between_the_places_of_two_files(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, FILES);
        let a = open(&workbench, &root.join("src/a.rs"), cx);
        caret_to(&a, 2, 3, cx);
        let b = open(&workbench, &root.join("src/b.rs"), cx);
        caret_to(&b, 1, 2, cx);
        run(cx, "navigate.back", None);
        settle(cx, |cx| at(&workbench, cx) == Some(("a.rs".to_owned(), 2, 3)));
        run(cx, "navigate.forward", None);
        settle(cx, |cx| at(&workbench, cx) == Some(("b.rs".to_owned(), 1, 2)));
        // Nothing lies beyond the newest place.
        run(cx, "navigate.forward", None);
        assert_eq!(at(&workbench, cx), Some(("b.rs".to_owned(), 1, 2)));
        close(cx);
    }

    /// Last Edit Location goes to the newest edit, then to the one before it; Next Edit Location goes
    /// back towards the newest.
    #[gpui_kit::test]
    async fn last_and_next_edit_location_walk_through_the_edits(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, FILES);
        let a = open(&workbench, &root.join("src/a.rs"), cx);
        caret_to(&a, 3, 0, cx);
        cx.simulate_input("x");
        let b = open(&workbench, &root.join("src/b.rs"), cx);
        caret_to(&b, 0, 0, cx);
        cx.simulate_input("y");
        caret_to(&b, 2, 4, cx);
        // ⇧⌘⌫ in the editor.
        cx.simulate_keystrokes("cmd-shift-backspace");
        settle(cx, |cx| at(&workbench, cx) == Some(("b.rs".to_owned(), 0, 1)));
        run(cx, "navigate.lastEditLocation", None);
        settle(cx, |cx| at(&workbench, cx) == Some(("a.rs".to_owned(), 3, 1)));
        run(cx, "navigate.nextEditLocation", None);
        settle(cx, |cx| at(&workbench, cx) == Some(("b.rs".to_owned(), 0, 1)));
        close(cx);
    }

    /// Copy Path copies the absolute path and Copy Path with Line Number the path from the project
    /// root with the caret's line (or the text it is given); each says so in the hint and goes into
    /// the clipboard history.
    #[gpui_kit::test]
    async fn the_copy_path_actions_copy_the_forms_of_the_edited_file(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, FILES);
        let a = open(&workbench, &root.join("src/a.rs"), cx);
        caret_to(&a, 2, 3, cx);
        let absolute = root.join("src/a.rs").to_string_lossy().into_owned();
        run(cx, "editor.showHint", Some(serde_json::json!({ "text": "A hint" })));
        assert_eq!(editor_hint(&workbench, cx).as_deref(), Some("A hint"));
        assert!(!titles(&workbench, cx).contains(&"A hint".to_owned()), "the hint is not a notification");
        run(cx, "editor.copyPath", None);
        assert_eq!(clipboard(cx).as_deref(), Some(absolute.as_str()));
        assert_eq!(editor_hint(&workbench, cx), Some(format!("Copied {absolute}")));
        run(cx, "editor.copyPathWithLineNumber", None);
        assert_eq!(clipboard(cx).as_deref(), Some("src/a.rs:3"));
        run(cx, "editor.copyPathWithLineNumber", Some(Value::from("given:9")));
        assert_eq!(clipboard(cx).as_deref(), Some("given:9"));
        let history: Vec<String> = cx.update(|_, cx| {
            cx.default_global::<EditorClipboard>().0.entries().iter().map(|entry| entry.text.clone()).collect()
        });
        assert_eq!(history[..3], ["given:9".to_owned(), "src/a.rs:3".to_owned(), absolute]);
        close(cx);
    }

    /// Reload from Disk reads the file's new text into its editor; an editor with unsaved changes keeps
    /// its text and a warning says why.
    #[gpui_kit::test]
    async fn reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
        let file = root.join("notes.txt");
        let editor = open(&workbench, &file, cx);
        std::fs::write(&file, "two\n").expect("the file is written");
        run(cx, "file.reloadFromDisk", None);
        settle(cx, |cx| editor.read_with(cx, |editor, _| editor.text()) == "two\n");
        std::fs::write(&file, "three\n").expect("the file is written");
        run(cx, "file.reloadAllFromDisk", None);
        settle(cx, |cx| editor.read_with(cx, |editor, _| editor.text()) == "three\n");
        cx.simulate_input("x");
        std::fs::write(&file, "four\n").expect("the file is written");
        // ⌥⌘Y in the editor.
        cx.simulate_keystrokes("cmd-alt-y");
        assert!(titles(&workbench, cx).contains(&"notes.txt has unsaved changes".to_owned()));
        run(cx, "file.reloadFromDisk", Some(Value::from(file.to_string_lossy().into_owned())));
        assert!(titles(&workbench, cx).contains(&"notes.txt has unsaved changes".to_owned()));
        let kept = editor.read_with(cx, |editor, _| editor.text());
        assert!(kept.contains('x') && kept.contains("three"), "{kept:?}");
        close(cx);
    }

    /// Toggle Read-only Attribute asks first; confirmed, the file is read-only on disk and the editor
    /// refuses typing; toggled again both are writable.
    #[gpui_kit::test]
    async fn toggle_read_only_asks_then_locks_the_file_and_its_editor(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
        let file = root.join("notes.txt");
        let editor = open(&workbench, &file, cx);
        run(cx, "file.toggleReadOnly", None);
        let dialog = workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the question is asked");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Make File Read-only");
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| editor.read_with(cx, |editor, _| editor.is_read_only()));
        assert!(std::fs::metadata(&file).expect("the file").permissions().readonly());
        cx.simulate_input("x");
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "one\n");
        run(cx, "file.toggleReadOnly", None);
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.is_read_only()));
        assert!(!std::fs::metadata(&file).expect("the file").permissions().readonly());
        cx.simulate_input("x");
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "xone\n");
        close(cx);
    }

    /// Increase / Decrease Font Size step the active editor's font by one px, within the limits;
    /// Reset Font Size puts every editor back to the configured size.
    #[gpui_kit::test]
    async fn the_font_size_steps_in_the_active_editor_and_resets_in_every_editor(cx: &mut TestAppContext) {
        use crate::features_b::viewers::font_zoom::MIN_EDITOR_ZOOM;
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, FILES);
        let a = open(&workbench, &root.join("src/a.rs"), cx);
        let base = a.read_with(cx, |e, cx| e.font_size(cx));
        run(cx, "editor.increaseFontSize", None);
        // ⌥⇧. is Increase Font Size in a project window.
        cx.simulate_keystrokes("alt-shift-.");
        assert_eq!(a.read_with(cx, |e, cx| e.font_size(cx)), base + gpui_kit::px(2.0));
        run(cx, "editor.decreaseFontSize", None);
        assert_eq!(a.read_with(cx, |e, cx| e.font_size(cx)), base + gpui_kit::px(1.0));
        let b = open(&workbench, &root.join("src/b.rs"), cx);
        for _ in 0..60 {
            run(cx, "editor.decreaseFontSize", None);
        }
        let zooms =
            |cx: &mut VisualTestContext| [a.read_with(cx, |e, _| e.font_zoom()), b.read_with(cx, |e, _| e.font_zoom())];
        assert_eq!(zooms(cx), [1, MIN_EDITOR_ZOOM]);
        run(cx, "editor.resetFontSize", None);
        assert_eq!(zooms(cx), [0, 0]);
        assert_eq!(b.read_with(cx, |e, cx| e.font_size(cx)), base);
        close(cx);
    }

    /// Paste from History lists what was copied; the entry chosen pastes at the caret, Delete removes
    /// an entry from the history, and an empty history only says so.
    #[gpui_kit::test]
    async fn paste_from_history_pastes_the_entry_chosen_in_the_dialog(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "alpha beta\n")]);
        let editor = open(&workbench, &root.join("notes.txt"), cx);
        run(cx, "code.pasteFromHistory", None);
        assert!(workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
        assert_eq!(editor_hint(&workbench, cx).as_deref(), Some("Nothing to paste: the clipboard history is empty"));
        for (from, to) in [(0, 5), (6, 10)] {
            editor.update(cx, |editor, cx| editor.select(from, to, cx));
            run(cx, "edit.copy", None);
        }
        editor.update(cx, |editor, cx| editor.select(11, 11, cx));
        run(cx, "code.pasteFromHistory", None);
        let dialog = workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open");
        let rows: Vec<String> = dialog.read_with(cx, |d, _| d.rows().iter().map(|row| row.text.clone()).collect());
        assert_eq!(rows, ["beta", "alpha"]);
        cx.simulate_keystrokes("down");
        run(cx, "editor.pasteHistory.paste", None);
        assert!(workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "alpha beta\nalpha");
        assert_eq!(clipboard(cx).as_deref(), Some("alpha"));
        let history = |cx: &mut VisualTestContext| -> Vec<String> {
            cx.update(|_, cx| {
                cx.default_global::<EditorClipboard>().0.entries().iter().map(|entry| entry.text.clone()).collect()
            })
        };
        run(cx, "code.pasteFromHistory", None);
        // Backspace edits the filter while it has text: no entry goes.
        cx.simulate_input("be");
        cx.simulate_keystrokes("backspace");
        assert_eq!(history(cx), ["alpha", "beta"]);
        run(cx, "editor.pasteHistory.remove", None);
        assert_eq!(history(cx), ["alpha"]);
        cx.simulate_keystrokes("escape");
        assert!(workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
        close(cx);
    }

    /// Without a project the actions that need one do not run; without an editor tab the editor's
    /// actions do not run; without the dialog its actions do not run.
    #[gpui_kit::test]
    async fn the_actions_need_what_their_when_clauses_name(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx, FILES);
        let unavailable = |cx: &mut VisualTestContext, ids: &[&str]| {
            for id in ids {
                let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
                assert!(ran.is_err(), "{id} runs");
            }
        };
        // `textEditorActive`, `editorTabsOpen` and the editor's focus: no tab is open.
        let editor_actions =
            ["file.toggleReadOnly", "editor.copyPath", "code.foldAll", "code.autoIndentLines", "editor.scrollToCenter"];
        unavailable(cx, &editor_actions);
        open(&workbench, &root.join("src/a.rs"), cx);
        // `pasteHistoryOpen`: the dialog is not open.
        unavailable(cx, &["editor.pasteHistory.paste", "editor.pasteHistory.remove"]);
        close(cx);
        // `projectOpen`: a single-file window has no project.
        let loose = root.join("src/b.rs");
        let (light, cx) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
        light.update_in(cx, |w, window, cx| w.open_file(loose.clone(), window, cx));
        settle(cx, |cx| light.read_with(cx, |w, _| w.editor().is_some()));
        unavailable(
            cx,
            &[
                "navigate.lastEditLocation",
                "navigate.nextEditLocation",
                "file.reloadFromDisk",
                "file.reloadAllFromDisk",
                "editor.increaseFontSize",
                "editor.decreaseFontSize",
                "editor.resetFontSize",
            ],
        );
        // Their keys do nothing there either.
        let editor = light.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        cx.update(|window, cx| {
            window.activate_window();
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("alt-shift-. alt-shift-. alt-shift-,");
        assert_eq!(editor.read_with(cx, |e, _| e.font_zoom()), 0);
        // An edit, then the caret at the end: Last Edit Location (⇧⌘⌫) leaves it there, and Reload
        // from Disk (⌥⌘Y) neither reloads nor warns about the unsaved change.
        cx.simulate_input("z");
        cx.simulate_keystrokes("cmd-down");
        let end = editor.read_with(cx, |e, _| e.caret());
        cx.simulate_keystrokes("cmd-shift-backspace cmd-alt-y");
        assert_eq!(editor.read_with(cx, |e, _| e.caret()), end);
        assert!(editor.read_with(cx, |e, _| e.text().starts_with('z')));
        assert!(titles(&light, cx).iter().all(|title| !title.contains("unsaved changes")), "{:?}", titles(&light, cx));
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w2");
    }

    /// The editor's actions are confined as their `when` clauses say: with the Project view focused
    /// the folding commands and Copy Path with Line Number do not run, and the fold key folds nothing;
    /// in the editor the key folds.
    #[gpui_kit::test]
    async fn the_editor_actions_do_nothing_outside_the_editor(cx: &mut TestAppContext) {
        let _home = start(cx);
        let text = "fn a() {\n    if x {\n        y();\n    }\n}\n";
        let (_folder, root, workbench, cx) = project(cx, &[("src/main.rs", text)]);
        let editor = open(&workbench, &root.join("src/main.rs"), cx);
        settle(cx, |cx| editor.read_with(cx, |e, _| e.folds_are_current()));
        caret_to(&editor, 2, 8, cx);
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        for id in ["code.fold", "code.extendSelection", "editor.copyPathWithLineNumber", "code.pasteFromHistory"] {
            let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
            assert!(ran.is_err(), "{id} runs outside the editor");
        }
        cx.simulate_keystrokes("cmd--");
        assert_eq!(editor.read_with(cx, |e, _| e.shown_lines()), [0, 1, 2, 3, 4, 5]);
        // The other editor keys of the module leave the editor as it is: Extend / Shrink Selection,
        // Fold Selection, Fold Code Block, Collapse All, Collapse Recursively, Auto-Indent Lines,
        // Scroll to Center, Paste from History.
        let state = |cx: &mut VisualTestContext| {
            editor.read_with(cx, |e, _| (e.text(), e.selections().to_vec(), e.scroll_top(), e.shown_lines()))
        };
        let before = state(cx);
        cx.simulate_keystrokes("alt-up alt-down cmd-. cmd-shift-. cmd-shift-- cmd-alt-- ctrl-alt-i ctrl-l cmd-shift-v");
        assert_eq!(state(cx), before);
        assert!(workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
        let focus_editor = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let focus = editor.focus_handle(cx);
                window.focus(&focus, cx);
            });
            cx.run_until_parked();
        };
        focus_editor(cx);
        cx.simulate_keystrokes("cmd--");
        assert_eq!(editor.read_with(cx, |e, _| e.shown_lines()), [0, 1, 4, 5]);
        // Folded, with the Project view focused again: the expanding keys open nothing.
        cx.update(|window, cx| {
            let focus = tree.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("cmd-= cmd-shift-= cmd-alt-=");
        assert_eq!(editor.read_with(cx, |e, _| e.shown_lines()), [0, 1, 4, 5]);
        focus_editor(cx);
        cx.simulate_keystrokes("cmd-=");
        assert_eq!(editor.read_with(cx, |e, _| e.shown_lines()), [0, 1, 2, 3, 4, 5]);
        close(cx);
    }
}
