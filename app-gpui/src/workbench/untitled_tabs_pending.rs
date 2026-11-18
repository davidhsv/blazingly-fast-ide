//! Untitled buffers in the window's tabs: File › New › Untitled File opens one (a window showing
//! nothing turns into LightEdit first), Save All and the close questions give each one with text a
//! file through Save As, one dialog at a time, and closing the last tab of one with text always
//! asks, since autosave has no file to write it to.
//!
//! Every editor of an untitled buffer holds one backend reference, as every editor does: the editor
//! of a new tab is built from the reference minting the buffer gave, and a second tab of the buffer
//! (a split, Reopen Closed Tab) opens it again by its key, which counts one more.

use super::branches::This;
use super::light_edit_tabs;
use super::services_window::on_workbench;
use super::{Content, Workbench};
use crate::backend::{AppDirs, Backend};
use crate::editor::tabs::{OpenTabOptions, TabsState};
use crate::editor::view::Editor;
use crate::editor_model::untitled::{retarget_tabs, save_as_start, untitled_input, untitled_name};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{App, AppContext as _, AsyncWindowContext, Context, Entity, Focusable, Window};
use ide_workspace::DocumentInfo;
use std::path::PathBuf;

/// The title of the notification of a Save As the backend refused.
const SAVE_AS_FAILED: &str = "Cannot save file";

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("file.newUntitled", |_, window, cx| on_workbench(window, cx, Workbench::new_untitled));
}

/// The folder that stands for the user's home: LightEdit without a file is rooted there and Save As
/// opens there when nothing nearer is known. Under a home of the app's own (tests, measurements) it
/// is that home, so that nothing reads or writes the user's.
pub(super) fn user_home(dirs: &AppDirs) -> PathBuf {
    if dirs.isolated {
        return dirs.data.parent().unwrap_or(&dirs.data).to_path_buf();
    }
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

impl Workbench {
    /// File › New › Untitled File: an empty untitled buffer opens in a tab of the active pane and
    /// takes the keyboard. A window that shows nothing becomes a LightEdit window without a file
    /// first; a window opening something or showing a loose file's viewer has no tabs for it.
    pub(super) fn new_untitled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.content {
            Content::Project { .. } | Content::Editor { .. } => {}
            Content::Empty | Content::Failed(_) => return self.new_untitled_in_light_edit(window, cx),
            Content::Opening(_) | Content::Viewer(_) => return,
        }
        let label = self.label.clone();
        let opened = cx
            .global::<Backend>()
            .with_host(move |host| async move { ide_host::ops::doc::open_untitled(&host, &label, None).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let opened = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| match flatten(opened) {
                Ok(info) => workbench.untitled_opened(info, window, cx),
                Err(message) => tracing::error!("cannot open an untitled buffer: {message}"),
            });
        })
        .detach();
    }

    /// The backend minted the buffer `info`: its tab opens with an editor built from the reference
    /// the minting gave. An answer for a project the window no longer shows finds no document in
    /// the current one and is dropped; an editor built for a window that lost its tabs meanwhile is
    /// dropped too, which releases the buffer.
    fn untitled_opened(&mut self, info: DocumentInfo, window: &mut Window, cx: &mut Context<Self>) {
        let host = cx.global::<Backend>().host.clone();
        let label = self.label.clone();
        let Ok(project) = host.project(&label) else { return };
        let Ok((version, rope)) = project.documents.rope_snapshot(info.id) else { return };
        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
        let key = info.path.to_string_lossy().into_owned();
        let (Content::Project { tabs, .. } | Content::Editor { tabs, .. }) = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let Some(tab_id) = tabs.open_tab(&pane, untitled_input(&key), OpenTabOptions::default()) else { return };
        self.install_tab_editor(&tab_id, editor, true, window, cx);
        self.tabs_changed(true, window, cx);
    }

    /// Gives tab `tab_id` the editor of its document and wires it into the window: the editor
    /// services, the place the file was left at, the focus that makes its pane the active one, the
    /// Markdown layouts, a pending reveal and the gutter's feeds. `still_open` false: the tab closed
    /// while its document was opening, and the editor goes with this call, releasing the document.
    pub(super) fn install_tab_editor(
        &mut self,
        tab_id: &str,
        editor: Entity<Editor>,
        still_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if still_open {
            self.install_intelligence(&editor, window, cx);
        }
        if let (
            true,
            Content::Project { editors, restored_views, .. } | Content::Editor { editors, restored_views, .. },
        ) = (still_open, &mut self.content)
        {
            // The file is where it was left when the project was last open.
            let path = editor.read(cx).path().to_string_lossy().into_owned();
            if let Some(view) = restored_views.remove(&path) {
                editor.update(cx, |editor, cx| {
                    editor.select(view.anchor, view.head, cx);
                    if let Some(line) = view.top_line {
                        editor.scroll_to_line(line, cx);
                    }
                });
            }
            let focused = tab_id.to_owned();
            let took_focus = cx.on_focus_in(&editor.focus_handle(cx), window, move |workbench, window, cx| {
                workbench.editor_took_focus(&focused, window, cx);
            });
            editors.insert(tab_id.to_owned(), editor.clone());
            if let Content::Project { editor_focus, .. } | Content::Editor { editor_focus, .. } = &mut self.content {
                editor_focus.insert(tab_id.to_owned(), took_focus);
            }
            self.wrap_markdown(tab_id, &editor, window, cx);
        }
        self.serve_reveal(cx);
        if still_open {
            self.watch_debug_editor(&editor, window, cx);
            self.watch_plugin_gutter(&editor, window, cx);
            self.http_editor_opened(&editor, window, cx);
            self.editor_gutter_opened(&editor, window, cx);
        }
    }

    /// File › New › Untitled File in a window that shows nothing: the window becomes a LightEdit
    /// window without a file, rooted at the user's home and watching nothing until a buffer is saved
    /// into a folder, with the new buffer as its first tab among the tabs of the last LightEdit
    /// session.
    fn new_untitled_in_light_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = user_home(&cx.global::<Backend>().dirs);
        self.content = Content::Opening(dir.clone());
        self.light_pending = None;
        self.sync_welcome(window, cx);
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let (blocking_host, blocking_label) = (host.clone(), label.clone());
            tokio::task::spawn_blocking(move || blocking_host.open_light_empty_in(&blocking_label, dir).map(|_| ()))
                .await
                .map_err(|error| ide_core::IdeError::internal(error.to_string()))??;
            ide_host::ops::doc::open_untitled(&host, &label, None).await
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let opened = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                // The window went on to something else meanwhile (a project, a file): the answer is
                // dropped, and with it the buffer's only reference.
                if !matches!(workbench.content, Content::Opening(_)) {
                    return;
                }
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                let text = flatten(opened).and_then(|info| {
                    let project = host.project(&label).map_err(|error| error.message)?;
                    let (version, rope) = project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                    Ok((info, version, rope))
                });
                workbench.content = match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        workbench.install_intelligence(&editor, window, cx);
                        let focus = editor.focus_handle(cx);
                        window.focus(&focus, cx);
                        workbench.observe_editor(&editor, cx);
                        workbench.light_edit_content(editor, window, cx)
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                if let Some(editor) = workbench.editor().cloned() {
                    workbench.editor_gutter_opened(&editor, window, cx);
                }
                workbench.sync_welcome(window, cx);
                workbench.light_edit_opened(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Where Save As opens for the untitled buffer of `editor`, and the name it suggests.
    fn untitled_save_start(&self, editor: &Entity<Editor>, cx: &App) -> (PathBuf, String) {
        let key = editor.read(cx).path().to_string_lossy().into_owned();
        let home = user_home(&cx.global::<Backend>().dirs);
        let none = TabsState::new();
        let tabs = self.tabs().unwrap_or(&none);
        save_as_start(self.project_root(cx).as_deref(), tabs, &home, &key)
    }

    /// Whether `editor` still shows an untitled buffer of this window's project: one that closed with
    /// its project, or became a file meanwhile, is not saved again.
    fn holds_untitled(&self, editor: &Entity<Editor>, cx: &App) -> bool {
        let editor = editor.read(cx);
        editor.is_untitled()
            && cx
                .global::<Backend>()
                .host
                .project(&self.label)
                .and_then(|project| project.documents.is_untitled(editor.doc()))
                .unwrap_or(false)
    }

    /// One editor of each untitled buffer that holds text not on disk, in the order the window
    /// lists its documents.
    fn unsaved_untitled(&self, cx: &App) -> Vec<Entity<Editor>> {
        self.document_editors(cx)
            .into_iter()
            .filter(|editor| {
                let editor = editor.read(cx);
                editor.is_untitled() && editor.is_dirty()
            })
            .collect()
    }

    /// Save As made the untitled buffer `old_key` the file `info` describes: its editors take the
    /// file, every tab of it becomes the file's tab, and the window takes the change in as any
    /// change of its tabs (the session, Open Files, the plugins).
    fn untitled_saved(&mut self, old_key: &str, info: &DocumentInfo, window: &mut Window, cx: &mut Context<Self>) {
        Editor::retarget_document(info.id, info, cx);
        let path = info.path.to_string_lossy().into_owned();
        if let Content::Project { tabs, .. } | Content::Editor { tabs, .. } = &mut self.content {
            retarget_tabs(tabs, old_key, &path);
        }
        self.tabs_changed(false, window, cx);
    }

    fn report_save_as_failure(&mut self, message: String, cx: &mut Context<Self>) {
        let mut request = NotificationRequest::new(NotificationLevel::Error, SAVE_AS_FAILED);
        request.body = Some(message);
        self.notify(request, cx);
    }
}

/// Asks where to save the untitled buffer `editor` shows and saves it there: the folder and name of
/// [`save_as_start`], the save cleanup, then the backend's Save As. Resolves to whether the buffer is
/// a file now; false when the dialog was cancelled, the buffer went away meanwhile, or the backend
/// refused (a file open in another tab, a folder that cannot be made), which is told as "Cannot save
/// file" with the reason while the buffer keeps its text.
pub(super) async fn save_untitled(
    this: &This,
    editor: &Entity<Editor>,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<bool> {
    let picked = this.update(cx, |workbench, cx| {
        let (dir, name) = workbench.untitled_save_start(editor, cx);
        cx.prompt_for_new_path(&dir, Some(name.as_str()))
    })?;
    let path = match picked.await {
        Ok(Ok(Some(path))) => path,
        Ok(Ok(None)) | Err(_) => return Ok(false),
        Ok(Err(error)) => {
            this.update(cx, |workbench, cx| workbench.report_save_as_failure(error.to_string(), cx))?;
            return Ok(false);
        }
    };
    let current = this.read_with(cx, |workbench, cx| workbench.holds_untitled(editor, cx))?;
    if !current {
        return Ok(false);
    }
    let (old_key, saving) = this.update(cx, |_, cx| {
        let old_key = editor.read(cx).path().to_string_lossy().into_owned();
        (old_key, Editor::save_document_as(editor, path, cx))
    })?;
    let saved = saving.await;
    this.update_in(cx, |workbench, window, cx| match saved {
        Ok(Ok(info)) => {
            workbench.untitled_saved(&old_key, &info, window, cx);
            true
        }
        Ok(Err(error)) => {
            workbench.report_save_as_failure(error.message, cx);
            false
        }
        Err(error) => {
            workbench.report_save_as_failure(error.to_string(), cx);
            false
        }
    })
}

/// Gives every untitled buffer of the window that holds text a file, one Save As dialog after the
/// other (the platform shows one save sheet at a time). Resolves to whether none is left unsaved.
pub(super) async fn save_untitled_buffers(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<bool> {
    let buffers = this.read_with(cx, |workbench, cx| workbench.unsaved_untitled(cx))?;
    let mut all_saved = true;
    for editor in buffers {
        // An earlier answer may have settled this one already (its tab closed with the project).
        let pending = this.read_with(cx, |_, cx| {
            let editor = editor.read(cx);
            editor.is_untitled() && editor.is_dirty()
        })?;
        if pending {
            all_saved &= save_untitled(this, &editor, cx).await?;
        }
    }
    Ok(all_saved)
}

/// The last tab of an untitled buffer with text closed: the user is asked, whatever autosave says,
/// since there is no file to write it to. Save runs Save As and the tab stays when that does not end
/// in a file; Don't Save lets the text go; Cancel, or the question going away, brings the tab back.
pub(super) fn ask_closing_untitled(editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Workbench>) {
    let name = untitled_name(&editor.read(cx).path().to_string_lossy()).to_owned();
    let i18n = &cx.default_global::<Localization>().0;
    let (title, body) = (i18n.t("app.unsaved.title", &[]), i18n.t("app.unsaved.fileBody", &[("name", name.into())]));
    let answers =
        [i18n.t("app.unsaved.saveFile", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])];
    let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
    let decision = window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(&body), &answers, cx);
    cx.spawn_in(window, async move |workbench, cx| {
        let kept = match decision.await.ok() {
            Some(0) => !matches!(save_untitled(&workbench, &editor, cx).await, Ok(true)),
            // Don't Save.
            Some(1) => false,
            _ => true,
        };
        if kept {
            let _ =
                workbench.update_in(cx, |workbench, window, cx| workbench.restore_unsaved(editor, None, window, cx));
        } else {
            light_edit_tabs::closed_tab_settled(&workbench, cx);
        }
    })
    .detach();
}

/// A backend answer as the window takes it: the value, or the message.
fn flatten<T>(result: anyhow::Result<ide_core::IdeResult<T>>) -> Result<T, String> {
    result.map_err(|error| error.to_string()).and_then(|answer| answer.map_err(|error| error.message))
}
