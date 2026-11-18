//! Creating a file from a template in the window: the New File dialog over the workbench, the
//! package its hint shows, and the creation itself.
//!
//! A file is created as New › <template> creates it: the name is planned in the folder New was used
//! in (a plan that would leave that folder is refused), the package is found (a sibling Go file or a
//! composer.json may be read), the body is rendered, and only then is the empty file created through
//! the backend's file creation, so Local History, the file watcher and version control see it as any
//! file the IDE creates. A name that is taken, a folder that cannot be written and any other failure
//! before the file exists show under the name and the dialog takes input again. Once the file exists
//! the dialog closes, the file opens, and an empty document is filled with the body, the caret at
//! `$END$` (kept there through Reformat Code when the template asks for it) and saved.

use super::Content;
use super::Workbench;
use super::editor_language::{FormatRun, This, call, format_document};
use super::notifications::NotificationRequest;
use crate::editor::buffer;
use crate::editor::view::Editor;
use crate::platform::actions::execute_action;
use crate::platform::i18n::Localization;
use crate::platform::project_roots::{FolderRole, RootFolder};
use crate::platform::storage::UiStorage;
use crate::templates::files_create::{
    ComposerStep, CreateEffect, Creation, CsprojStep, INVALID_FILE_NAME, PackageLookup, ProjectInfo, SiblingFile,
    caret_after_fill_reformat, package_for, plan_creation,
};
use crate::templates::files_model::{FileTemplate, Includes, PlannedFile, TemplateFragment};
use crate::templates::files_store::FileTemplates;
use crate::templates::new_file_dialog::{
    NewFileCreate, NewFileDialog, NewFileDialogEvent, NewFileDialogInput, PackageQuery,
};
use gpui_kit::{App, AppContext as _, AsyncWindowContext, Context, Entity, Focusable as _, WeakEntity, Window};
use ide_core::{IdeError, IdeResult, TextChange};
use ide_host::Host;
use ide_vfs::EntryKind;
use ide_workspace::RootKind;
use std::path::PathBuf;
use std::time::Duration;

/// How often, and how long, the new file's editor is waited for before its content is given up.
pub(super) const OPEN_POLL: Duration = Duration::from_millis(30);
pub(super) const OPEN_POLLS: u32 = 134;

/// The file templates, includes and code templates in effect.
#[derive(Debug, Clone, PartialEq)]
pub struct FileTemplateSet {
    pub templates: Vec<FileTemplate>,
    pub includes: Vec<TemplateFragment>,
    pub code_templates: Vec<TemplateFragment>,
}

impl FileTemplateSet {
    /// The templates New offers: every one the user did not turn off.
    pub fn enabled(&self) -> Vec<FileTemplate> {
        self.templates.iter().filter(|template| template.enabled != Some(false)).cloned().collect()
    }
}

/// The file templates in effect for the project at `root` (none: the Default scheme): the built-ins
/// with the customisations of the scheme the project chose, as File and Code Templates stores them.
pub fn templates_in_effect(root: Option<&str>, cx: &App) -> FileTemplateSet {
    let storage = cx.try_global::<UiStorage>();
    let loaded = FileTemplates::load(root, &|key| storage.and_then(|storage| storage.get(key)).map(str::to_owned));
    FileTemplateSet {
        templates: loaded.templates().to_vec(),
        includes: loaded.includes().to_vec(),
        code_templates: loaded.code_templates().to_vec(),
    }
}

/// What New › <template> and New from Template… open the dialog with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewFileRequest {
    /// The folder the file goes into.
    pub directory: String,
    /// The template to name a file of, by id; without one (or an unknown id) the dialog lists them.
    pub template_id: Option<String>,
    /// A template of another feature's own (New › HTTP Request), not one of the templates in effect.
    pub template: Option<FileTemplate>,
    /// The name the field starts with.
    pub name: Option<String>,
    /// The kind chosen at first.
    pub kind_id: Option<String>,
}

/// Whether `plan` puts the file inside `directory`: none of the path below it is empty, `.` or
/// `..`, whatever the name, the template's file name or its extension said.
fn stays_in(plan: &PlannedFile, directory: &str) -> bool {
    let base = directory.trim_end_matches('/');
    let Some(rest) = plan.path.strip_prefix(base).and_then(|rest| rest.strip_prefix('/')) else { return false };
    rest.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// The project's name, as the tree shows it: its folder's name.
fn project_name(root: &str) -> String {
    std::path::Path::new(root).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn folder_role(kind: RootKind) -> FolderRole {
    match kind {
        RootKind::Source => FolderRole::Source,
        RootKind::Test => FolderRole::Test,
        RootKind::Resources => FolderRole::Resources,
        RootKind::TestResources => FolderRole::TestResources,
        RootKind::Excluded => FolderRole::Excluded,
    }
}

/// The project's effective source and test roots; none without a project.
fn project_roots(host: &Host, label: &str) -> Vec<RootFolder> {
    let roots = ide_host::ops::project::roots(host, label).map(|roots| roots.roots).unwrap_or_default();
    roots
        .into_iter()
        .map(|root| RootFolder { path: root.path, kind: folder_role(root.kind), detected_from: root.detected_from })
        .collect()
}

/// The files of `directory`, as names and paths; none when it cannot be listed.
async fn listed_files(host: &Host, label: &str, directory: &str) -> Option<Vec<(String, String)>> {
    let listed = ide_host::ops::fs::read_dir(host, label, PathBuf::from(directory)).await.ok()?;
    let files = listed
        .into_iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
        .map(|entry| (entry.name, entry.path.0.to_string_lossy().into_owned()))
        .collect();
    Some(files)
}

/// A package a lookup found; `block_namespace`: the project's C# has no file-scoped namespaces, so the
/// body's `namespace $PACKAGE$;` is written as a block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FoundPackage {
    name: String,
    block_namespace: bool,
}

/// Carries out a package lookup, reading what it asks for. A file that cannot be read counts as one
/// that is not there.
async fn finish_lookup(host: &Host, label: &str, lookup: PackageLookup) -> FoundPackage {
    let name = match lookup {
        PackageLookup::Known(package) => package,
        PackageLookup::Csharp(mut lookup) => loop {
            let files = listed_files(host, label, lookup.folder()).await;
            let siblings: Option<Vec<SiblingFile<'_>>> = files.as_ref().map(|files| {
                files.iter().map(|(name, path)| SiblingFile { name: name.as_str(), path: path.as_str() }).collect()
            });
            match lookup.clone().listed(siblings.as_deref()) {
                CsprojStep::Read { path, name } => {
                    let text = ide_host::ops::fs::read_text(host, label, PathBuf::from(path)).await.unwrap_or_default();
                    let found = lookup.read(&name, &text);
                    let block_namespace = !found.name.is_empty() && !found.file_scoped;
                    return FoundPackage { name: found.name, block_namespace };
                }
                CsprojStep::Next(next) => lookup = next,
                CsprojStep::Done(none) => break none.name,
            }
        },
        PackageLookup::Go(lookup) => {
            let files = listed_files(host, label, &lookup.directory).await.unwrap_or_default();
            let siblings: Vec<SiblingFile<'_>> =
                files.iter().map(|(name, path)| SiblingFile { name: name.as_str(), path: path.as_str() }).collect();
            let text = match lookup.pick(&siblings) {
                Some(path) => ide_host::ops::fs::read_text(host, label, PathBuf::from(path)).await.ok(),
                None => None,
            };
            lookup.finish(text.as_deref())
        }
        PackageLookup::Php(mut lookup) => loop {
            let text = ide_host::ops::fs::read_text(host, label, PathBuf::from(lookup.path())).await.ok();
            match lookup.answer(text.as_deref()) {
                ComposerStep::Done(namespace) => break namespace,
                ComposerStep::Next(next) => lookup = next,
            }
        },
    };
    FoundPackage { name, block_namespace: false }
}

/// Runs `f` on a blocking thread of the backend's runtime.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> IdeResult<T> + Send + 'static) -> IdeResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|error| IdeError::internal(error.to_string()))?
}

/// The end of a creation's background work: it only fails when the window or the dialog went away.
fn finished(result: anyhow::Result<()>) {
    if let Err(error) = result {
        tracing::debug!("a file creation from a template ended with its window: {error:#}");
    }
}

/// Shows `message` in the dialog, which takes input again.
fn creation_failed(
    dialog: &WeakEntity<NewFileDialog>,
    cx: &mut AsyncWindowContext,
    message: String,
) -> anyhow::Result<()> {
    dialog.update(cx, |dialog, cx| dialog.creation_failed(message, cx))
}

impl Workbench {
    /// The New File dialog, while it is open.
    pub fn new_file_dialog(&self) -> Option<Entity<NewFileDialog>> {
        self.vcs_dialog_view::<NewFileDialog>()
    }

    /// Opens the New File dialog for `request`, near the top of the window.
    pub fn open_new_file_dialog(&mut self, request: NewFileRequest, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let set = templates_in_effect(root.as_deref(), cx);
        let template = request.template.or_else(|| {
            let id = request.template_id.as_deref()?;
            set.templates.iter().find(|template| template.id == id).cloned()
        });
        let input = NewFileDialogInput {
            directory: request.directory,
            project_root: root.unwrap_or_default(),
            templates: set.enabled(),
            template,
            name: request.name,
            kind_id: request.kind_id,
        };
        let dialog = cx.new(|cx| NewFileDialog::new(input, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, dialog, event: &NewFileDialogEvent, window, cx| {
            workbench.new_file_dialog_event(dialog, event.clone(), window, cx);
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.clone().into(), focus, events, window, cx);
        self.place_vcs_dialog_at_top(cx);
        dialog.update(cx, |dialog, cx| dialog.start(cx));
    }

    fn is_new_file_dialog(&self, dialog: &WeakEntity<NewFileDialog>) -> bool {
        self.new_file_dialog().is_some_and(|shown| Some(shown) == dialog.upgrade())
    }

    fn new_file_dialog_event(
        &mut self,
        dialog: &Entity<NewFileDialog>,
        event: NewFileDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            NewFileDialogEvent::Closed => {
                if self.is_new_file_dialog(&dialog.downgrade()) {
                    self.close_vcs_dialog(window, cx);
                }
            }
            NewFileDialogEvent::PackageWanted(query) => self.find_dialog_package(dialog.downgrade(), query, window, cx),
            NewFileDialogEvent::Create(create) => self.create_from_template(dialog.downgrade(), create, window, cx),
        }
    }

    /// Finds the package the dialog's hint shows for `query`, off the main thread.
    fn find_dialog_package(
        &mut self,
        dialog: WeakEntity<NewFileDialog>,
        query: PackageQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        cx.spawn_in(window, async move |this, cx| {
            let asked = query.clone();
            let found = call(&this, cx, move |host, label| async move {
                let roots = project_roots(&host, &label);
                let lookup = package_for(&asked.template, &asked.directory, root.as_deref(), &roots);
                Ok(finish_lookup(&host, &label, lookup).await)
            })
            .await;
            let result = match found {
                Ok(Ok(package)) => dialog.update(cx, |dialog, cx| dialog.set_package(&query, package.name, cx)),
                Ok(Err(error)) => {
                    tracing::debug!("no package for the New File dialog: {}", error.message);
                    Ok(())
                }
                Err(error) => Err(error),
            };
            finished(result);
        })
        .detach();
    }

    /// Plans `create`, then creates and fills the file in the background; a name that cannot be
    /// planned in the folder is reported at once.
    fn create_from_template(
        &mut self,
        dialog: WeakEntity<NewFileDialog>,
        create: NewFileCreate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let NewFileCreate { template, kind, directory, input, variables } = create;
        let planned = plan_creation(&template, &kind, &directory, &input, variables)
            .ok()
            .filter(|creation| stays_in(&creation.plan, &directory));
        let Some(creation) = planned else {
            let message = cx.default_global::<Localization>().0.tx(INVALID_FILE_NAME);
            if dialog.update(cx, |dialog, cx| dialog.creation_failed(message, cx)).is_err() {
                tracing::debug!("the New File dialog closed before its name was refused");
            }
            return;
        };
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let set = templates_in_effect(root.as_deref(), cx);
        cx.spawn_in(window, async move |this, cx| {
            finished(create_and_fill(this, cx, dialog, creation, root, set).await);
        })
        .detach();
    }

    /// The editor showing the file at `path`, in any pane.
    fn new_file_editor(&self, path: &str, cx: &App) -> Option<Entity<Editor>> {
        let (Content::Project { editors, .. } | Content::Editor { editors, .. }) = &self.content else { return None };
        editors.values().find(|editor| editor.read(cx).path().to_string_lossy() == path).cloned()
    }
}

/// Waits for the editor of `path`, which was just opened; `None` once it did not come in time.
async fn wait_for_editor(
    this: &This,
    cx: &mut AsyncWindowContext,
    path: &str,
) -> anyhow::Result<Option<Entity<Editor>>> {
    for _ in 0..OPEN_POLLS {
        let found = this.update(cx, |workbench, cx| workbench.new_file_editor(path, cx))?;
        if found.is_some() {
            return Ok(found);
        }
        cx.background_executor().timer(OPEN_POLL).await;
    }
    Ok(None)
}

/// The steps after the plan: package, render, create, open, fill, reformat, save.
async fn create_and_fill(
    this: This,
    cx: &mut AsyncWindowContext,
    dialog: WeakEntity<NewFileDialog>,
    creation: Creation,
    root: Option<String>,
    set: FileTemplateSet,
) -> anyhow::Result<()> {
    let project_root = root.clone();
    let found = call(&this, cx, move |host, label| async move {
        let roots = project_roots(&host, &label);
        let lookup = creation.package_lookup(project_root.as_deref(), &roots);
        let package = finish_lookup(&host, &label, lookup).await;
        Ok((creation, package))
    })
    .await?;
    let (mut creation, package) = match found {
        Ok(found) => found,
        Err(error) => return creation_failed(&dialog, cx, error.message),
    };
    if package.block_namespace {
        creation.use_block_namespace();
    }
    let name = root.as_deref().map(project_name).unwrap_or_default();
    let lists: [&[TemplateFragment]; 2] = [&set.includes, &set.code_templates];
    let project = root.as_deref().map(|root| ProjectInfo { name: &name, root });
    let prepared = creation.render(&package.name, project, Includes::new(&lists));
    // Closing the dialog while the package was looked up cancels the creation.
    let cancelled = !this.update(cx, |workbench, _| workbench.is_new_file_dialog(&dialog))?;
    // A notebook opens in its own editor, which reads the file from disk: no text editor is filled.
    let viewer = crate::notebook::is_notebook_path(&prepared.plan.path);
    let effects = if viewer { prepared.viewer_effects(cancelled) } else { prepared.effects(cancelled) };
    for effect in effects {
        match effect {
            CreateEffect::CreateFile(path) => {
                let file = PathBuf::from(path);
                let made = call(&this, cx, move |host, label| async move {
                    blocking(move || ide_host::ops::fs::create_file(&host, &label, file)).await
                })
                .await?;
                if let Err(error) = made {
                    return creation_failed(&dialog, cx, error.message);
                }
            }
            CreateEffect::CreateFileWithText(path, text) => {
                let file = PathBuf::from(path);
                let made = call(&this, cx, move |host, label| async move {
                    ide_host::ops::fs::create_file_bytes(&host, &label, file, text.into_bytes()).await
                })
                .await?;
                if let Err(error) = made {
                    return creation_failed(&dialog, cx, error.message);
                }
            }
            // The file watcher reports the new file to the views; nothing else follows explicit
            // file operations in this window.
            CreateEffect::NotifyCreated(_) => {}
            CreateEffect::OpenFile(path) => this.update_in(cx, |workbench, window, cx| {
                if workbench.is_new_file_dialog(&dialog) {
                    workbench.close_vcs_dialog(window, cx);
                }
                workbench.open_project_file(PathBuf::from(path), window, cx);
            })?,
        }
    }
    if cancelled || viewer {
        return Ok(());
    }
    let Some(editor) = wait_for_editor(&this, cx, &prepared.plan.path).await? else {
        let missing: NotificationRequest = prepared.editor_missing();
        return this.update(cx, |workbench, cx| workbench.notify(missing, cx));
    };
    let fill = this.update(cx, |_, cx| prepared.fill(buffer::len_utf16(editor.read(cx).rope())))?;
    let Some(fill) = fill else { return Ok(()) };
    let applied = this.update_in(cx, |_, window, cx| {
        let insert = TextChange { from: 0, to: 0, insert: fill.insert.clone() };
        let applied =
            editor.update(cx, |editor, cx| editor.apply_edit(&[insert], vec![(fill.caret, fill.caret)], 0, cx));
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
        applied
    })?;
    if !applied {
        return Ok(());
    }
    if fill.reformat {
        let (before, at) = this.update(cx, |_, cx| {
            let editor = editor.read(cx);
            (editor.text(), editor.caret())
        })?;
        // As on creation in the IDE: quietly, the whole file.
        let run = FormatRun { explicit: false, selection_only: false, tool: None };
        let reformatted = format_document(&this, &editor, run, cx).await?;
        this.update(cx, |_, cx| {
            let after = editor.read(cx).text();
            if let Some(caret) = caret_after_fill_reformat(&before, at, &after, reformatted) {
                editor.update(cx, |editor, cx| editor.select(caret, caret, cx));
            }
        })?;
    }
    cx.update(|window, cx| {
        if !execute_action("file.save", None, window, cx) {
            tracing::debug!("the new file was filled but Save is not available in this window");
        }
    })
}
