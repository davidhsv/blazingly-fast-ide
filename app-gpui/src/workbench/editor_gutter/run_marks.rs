//! The run marks of an editor's gutter: the producers' tests, suites and `main`s of a source, found
//! off the window's thread once an edit has rested, and the scripts of a `package.json`; a press opens
//! the mark's Run / Debug menu, which runs, debugs or modifies the context the producers make of the
//! mark's line, or the script's configuration.

use super::{FollowedGutter, RunMarks, Workbench};
use crate::editor::buffer;
use crate::editor::view::Editor;
use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use crate::run::context::RunEnv;
use crate::run::targets::{RunTarget, TargetKind};
use crate::run_producers::context_run::{
    ContextArg, ContextShortcuts, DEBUG_CONTEXT, MenuRow, RUN_CONTEXT, RunCommand, WITH_EXECUTOR, marker_icon,
    target_label, target_menu_items, target_title,
};
use crate::run_producers::npm_gutter::{ScriptCommand, ScriptEntry, ScriptMenuFlow, ScriptMenuStep, script_entries};
use crate::run_producers::producers::{ContextInput, file_targets, jvm_build_owns};
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::branches::{This, finished, host_call, run_action_later};
use gpui_kit::{App, AsyncWindowContext, Context, Entity, EntityId, Pixels, Point, Window};
use ide_vfs::EntryKind;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// The argument of Run / Debug / Modify Run Configuration for `context`: `{ path, line?, dir, content? }`.
fn context_value(context: &ContextArg) -> Value {
    let mut value = json!({ "path": context.path, "dir": context.dir });
    if let Some(line) = context.line {
        value["line"] = json!(line);
    }
    if let Some(content) = &context.content {
        value["content"] = json!(content);
    }
    value
}

/// The icon names a run menu row can carry.
fn static_icon(icon: &str) -> Option<&'static str> {
    match icon {
        "run" => Some("run"),
        "debug" => Some("debug"),
        "edit" => Some("edit"),
        _ => None,
    }
}

/// A run mark's rows as a menu of the window: rows of their own run the command they carry.
fn run_menu(rows: Vec<MenuRow>) -> Vec<MenuItem<RunCommand>> {
    rows.into_iter()
        .filter_map(|row| match row {
            MenuRow::Separator => Some(MenuItem::Separator),
            MenuRow::Header(_) | MenuRow::Action { .. } => None,
            MenuRow::Item(entry) => {
                let submenu = run_menu(entry.submenu);
                let mut item = if submenu.is_empty() {
                    MenuItem::entry(entry.label, entry.run)
                } else {
                    MenuItem::submenu(entry.label, submenu)
                };
                if let Some(icon) = entry.icon.as_deref().and_then(static_icon) {
                    item = item.icon(icon);
                }
                if let Some(shortcut) = entry.shortcut {
                    item = item.shortcut(shortcut);
                }
                Some(item)
            }
        })
        .collect()
}

/// The first keyboard shortcut of `action`, as menus show it.
fn shortcut_of(action: &str, cx: &App) -> Option<String> {
    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(action, is_mac_host()))
}

/// The files (and links to files) of a listed directory.
fn file_names(entries: Vec<ide_vfs::FileEntry>) -> Vec<String> {
    let is_file =
        |entry: &ide_vfs::FileEntry| entry.kind == EntryKind::File || entry.target_kind == Some(EntryKind::File);
    entries.into_iter().filter(is_file).map(|entry| entry.name).collect()
}

/// Makes a script's popup, reading what [`ScriptMenuFlow`] asks for off the window's thread, then
/// shows it at `at`.
async fn show_script_menu(
    this: This,
    cx: &mut AsyncWindowContext,
    mut flow: ScriptMenuFlow,
    mut step: ScriptMenuStep,
    at: Point<Pixels>,
) -> anyhow::Result<()> {
    let menu = loop {
        match step {
            ScriptMenuStep::Done(menu) => break menu,
            ScriptMenuStep::ReadRootPackage(file) => {
                let read = this.update(cx, |workbench, cx| {
                    let path = PathBuf::from(file);
                    host_call(workbench.label.clone(), cx, move |host, label| async move {
                        ide_host::ops::fs::read_text(&host, &label, path).await
                    })
                })?;
                // A root manifest that cannot be read is no workspace's.
                let text = read.await.ok();
                step = flow.root_package_read(text.as_deref());
            }
            ScriptMenuStep::ReadDir(dir) => {
                let listed = this.update(cx, |workbench, cx| {
                    let path = PathBuf::from(dir);
                    host_call(workbench.label.clone(), cx, move |host, label| async move {
                        ide_host::ops::fs::read_dir(&host, &label, path).await
                    })
                })?;
                // A directory that cannot be read has no lockfile: the default manager runs the script.
                let files = listed.await.map(file_names).unwrap_or_default();
                step = flow.dir_read(&files);
            }
        }
    };
    this.update_in(cx, |workbench, window, cx| {
        let (run, debug) = (shortcut_of(RUN_CONTEXT, cx), shortcut_of(DEBUG_CONTEXT, cx));
        let shortcuts = ContextShortcuts { run: run.as_deref(), debug: debug.as_deref() };
        let rows = menu.items(shortcuts, &mut cx.default_global::<Localization>().0);
        let items: Vec<MenuItem<ScriptCommand>> = rows
            .into_iter()
            .map(|row| {
                let item = MenuItem::item(row.label, row.run).icon(row.icon);
                match row.shortcut {
                    Some(shortcut) => item.shortcut(shortcut),
                    None => item,
                }
            })
            .collect();
        workbench.show_menu(
            &items,
            at,
            |workbench, command, window, cx| workbench.run_script(command, window, cx),
            window,
            cx,
        );
    })
}

impl Workbench {
    /// Finds the run marks again whenever the run configurations of the window's project are listed
    /// or change; a project opened in the window since brings its own configurations.
    pub(super) fn watch_run_configs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = self.run_driver().map(|driver| driver.read(cx).store().clone()) else { return };
        if self.editor_gutter.configs_observer.as_ref().is_some_and(|(id, _)| *id == store.entity_id()) {
            return;
        }
        self.editor_gutter.configs_seen = None;
        let observer = cx.observe_in(&store, window, |workbench, store, _, cx| {
            let seen = {
                let store = store.read(cx);
                (store.loaded, store.configs().iter().map(|config| config.id.clone()).collect::<Vec<_>>())
            };
            if workbench.editor_gutter.configs_seen.as_ref() == Some(&seen) {
                return;
            }
            workbench.editor_gutter.configs_seen = Some(seen);
            let ids: Vec<EntityId> = workbench.editor_gutter.followed.keys().copied().collect();
            for id in ids {
                workbench.find_run_marks(id, Duration::ZERO, cx);
            }
        });
        self.editor_gutter.configs_observer = Some((store.entity_id(), observer));
    }

    /// Finds the editor `id`'s run marks once `delay` has passed without another edit, off the
    /// window's thread: the producers' targets of a source, the scripts of a `package.json`.
    pub(super) fn find_run_marks(&mut self, id: EntityId, delay: Duration, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        if matches!(followed.run, RunMarks::None) {
            return;
        }
        let search = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let input = this.update(cx, |workbench, cx| workbench.run_marks_input(id, cx));
            let Ok(Some(input)) = input else { return };
            let found = cx.background_executor().spawn(async move { input.find() }).await;
            let _ = this.update(cx, |workbench, cx| workbench.run_marks_found(id, found, cx));
        });
        if let Some(followed) = self.editor_gutter.followed.get_mut(&id) {
            followed.run_search = Some(search);
        }
    }

    /// What finding the editor `id`'s run marks reads: its text and the project's configurations.
    fn run_marks_input(&self, id: EntityId, cx: &App) -> Option<RunMarksInput> {
        let followed = self.editor_gutter.followed.get(&id)?;
        let editor = followed.editor.upgrade()?;
        let text = editor.read(cx).text();
        let scripts = matches!(followed.run, RunMarks::Scripts(_));
        let (loaded, configs) = match self.run_driver() {
            Some(driver) => {
                let store = driver.read(cx).store().read(cx);
                (store.loaded, store.configs().to_vec())
            }
            None => (false, Vec::new()),
        };
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        Some(RunMarksInput { path: followed.path.clone(), root, configs, loaded, text, scripts })
    }

    /// The run marks found for the editor `id`, placed at the starts of their lines.
    fn run_marks_found(&mut self, id: EntityId, found: FoundRunMarks, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get_mut(&id) else { return };
        followed.run_search = None;
        let Some(editor) = followed.editor.upgrade() else { return };
        let shown = editor.read(cx);
        let lines = shown.line_count();
        let place = |line: u32| (line < lines).then(|| buffer::line_range(shown.rope(), line).start);
        // Sorted by line: the gutter looks the marks of the painted lines up by line.
        followed.run = match found {
            FoundRunMarks::Targets(mut targets) => {
                targets.sort_by_key(|target| target.line);
                RunMarks::Targets(
                    targets.into_iter().filter_map(|target| place(target.line).map(|from| (target, from))).collect(),
                )
            }
            FoundRunMarks::Scripts(mut entries) => {
                entries.sort_by_key(|entry| entry.line);
                RunMarks::Scripts(
                    entries.into_iter().filter_map(|entry| place(entry.line).map(|from| (entry, from))).collect(),
                )
            }
        };
        cx.notify();
    }

    /// The lines of `editor` that have a run mark, with the mark's icon.
    pub fn editor_gutter_run_marks(&self, editor: &Entity<Editor>) -> Vec<(u32, &'static str)> {
        let Some(followed) = self.editor_gutter.followed.get(&editor.entity_id()) else { return Vec::new() };
        match &followed.run {
            RunMarks::None => Vec::new(),
            RunMarks::Targets(marks) => {
                marks.iter().map(|(target, _)| (target.line, marker_icon(target.kind, None))).collect()
            }
            RunMarks::Scripts(marks) => marks.iter().map(|(entry, _)| (entry.line, "gutter-run")).collect(),
        }
    }

    /// The run mark on the editor `id`'s `line` was pressed: its menu opens below it at `at`.
    pub(super) fn run_mark_pressed(
        &mut self,
        id: EntityId,
        line: u32,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let (path, content) = (followed.path.clone(), editor.read(cx).text());
        let Some(driver) = self.run_driver().cloned() else { return };
        let (configs, types) = {
            let driver = driver.read(cx);
            (driver.store().read(cx).configs().to_vec(), driver.types())
        };
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        match &followed.run {
            RunMarks::None => {}
            RunMarks::Targets(marks) => {
                let found = marks.iter().find(|(target, _)| target.line == line);
                let Some(target) = found.map(|(target, _)| target.clone()) else { return };
                let (run, debug) = (shortcut_of(RUN_CONTEXT, cx), shortcut_of(DEBUG_CONTEXT, cx));
                let shortcuts = ContextShortcuts { run: run.as_deref(), debug: debug.as_deref() };
                let env = RunEnv { windows: cfg!(windows), types: Some(&*types), ..RunEnv::default() };
                let project = ContextInput { root: root.as_deref(), configs: &configs, env, ..ContextInput::default() };
                let i18n = &mut cx.default_global::<Localization>().0;
                let rows = target_menu_items(&target, &path, &content, project, shortcuts, &[], i18n);
                let items = run_menu(rows);
                self.show_menu(
                    &items,
                    at,
                    |workbench, command, window, cx| workbench.run_gutter_command(command, window, cx),
                    window,
                    cx,
                );
            }
            RunMarks::Scripts(marks) => {
                let found = marks.iter().find(|(entry, _)| entry.line == line);
                let Some(name) = found.map(|(entry, _)| entry.name.clone()) else { return };
                let (flow, step) = ScriptMenuFlow::start(&name, &path, &content, root.as_deref(), &configs);
                cx.spawn_in(window, async move |this, cx| {
                    finished(show_script_menu(this, cx, flow, step, at).await);
                })
                .detach();
            }
        }
    }

    /// Carries out a row of a run mark's menu.
    fn run_gutter_command(&mut self, command: RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            RunCommand::LaunchContext { context, debug } => {
                self.run_context(debug, Some(context_value(&context)), window, cx);
            }
            RunCommand::Launch { produced, debug } => self.launch_production(*produced, debug, false, window, cx),
            RunCommand::Modify(context) => self.modify_context(Some(context_value(&context)), window, cx),
            RunCommand::Execute { executor, context } => {
                let mut arg = context_value(&context);
                arg["executor"] = json!(executor);
                run_action_later(WITH_EXECUTOR.to_owned(), Some(arg), window, cx);
            }
        }
    }

    /// Carries out a row of a script's menu: the project's configuration runs, or the script's own as a
    /// temporary one, or it is debugged.
    fn run_script(&mut self, command: ScriptCommand, window: &mut Window, cx: &mut Context<Self>) {
        let (action, arg) = match command {
            ScriptCommand::RunConfig(id) => ("run.runConfig", Value::from(id)),
            ScriptCommand::RunTemporary(config) => ("run.runConfig", json!({ "config": *config, "temporary": true })),
            ScriptCommand::Debug(config) => ("run.debugConfig", json!(*config)),
        };
        run_action_later(action.to_owned(), Some(arg), window, cx);
    }

    /// The icon and the tooltip of the `index`-th run mark of `followed`.
    pub(super) fn run_mark_look(
        &self,
        followed: &FollowedGutter,
        index: usize,
        cx: &mut App,
    ) -> (&'static str, String) {
        match &followed.run {
            RunMarks::Targets(marks) => {
                let target = &marks[index].0;
                let verb = target_title(target, &cx.default_global::<Localization>().0);
                (marker_icon(target.kind, None), format!("{verb} '{}'", target_label(target)))
            }
            RunMarks::Scripts(marks) => {
                let name = marks[index].0.name.as_str();
                let title = cx.default_global::<Localization>().0.tx_built(&["Run '", "'"], &[name.into()]);
                ("gutter-run", title)
            }
            RunMarks::None => ("gutter-run", String::new()),
        }
    }
}

/// What finding the run marks of one editor reads, owned so that it is found off the window's thread.
struct RunMarksInput {
    path: String,
    root: Option<String>,
    configs: Vec<ide_run::RunConfig>,
    /// The configurations were listed: JVM tests are left to the build that runs them.
    loaded: bool,
    text: String,
    scripts: bool,
}

/// The run marks found for one editor.
enum FoundRunMarks {
    Targets(Vec<RunTarget>),
    Scripts(Vec<ScriptEntry>),
}

impl RunMarksInput {
    /// The scripts of a `package.json`, or the producers' targets of a source. A JVM test outside a
    /// Maven or Gradle build has no mark: the build tool that runs it marks it in its own gutter.
    fn find(self) -> FoundRunMarks {
        if self.scripts {
            return FoundRunMarks::Scripts(script_entries(&self.text));
        }
        let input =
            ContextInput { path: &self.path, root: self.root.as_deref(), configs: &self.configs, ..Default::default() };
        let targets = file_targets(input, &self.text);
        let jvm = self.path.ends_with(".java") || self.path.ends_with(".kt");
        if !jvm || !self.loaded || jvm_build_owns(input) {
            return FoundRunMarks::Targets(targets);
        }
        let runs = |target: &RunTarget| !matches!(target.kind, TargetKind::Test | TargetKind::Suite);
        FoundRunMarks::Targets(targets.into_iter().filter(runs).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu of a run mark keeps its rows, submenus and separators; headers and action rows are
    /// not in it.
    #[test]
    fn a_run_marks_rows_become_the_menus_rows() {
        use crate::run_producers::context_run::{ActionArg, MenuEntry};
        let context = ContextArg { path: "/p/a.rs".to_owned(), line: Some(0), dir: false, content: None };
        let rows = vec![
            MenuRow::Header("Choose".to_owned()),
            MenuRow::Item(MenuEntry {
                label: "Run 'main'".to_owned(),
                icon: Some("run".to_owned()),
                shortcut: Some("⌃⇧R".to_owned()),
                run: Some(RunCommand::LaunchContext { context: context.clone(), debug: false }),
                ..MenuEntry::default()
            }),
            MenuRow::Action {
                action: RUN_CONTEXT,
                arg: ActionArg::Context(context.clone()),
                label: "x".to_owned(),
                icon: "run".to_owned(),
            },
            MenuRow::Separator,
            MenuRow::Item(MenuEntry {
                label: "Modify Run Configuration…".to_owned(),
                icon: Some("edit".to_owned()),
                run: Some(RunCommand::Modify(context.clone())),
                ..MenuEntry::default()
            }),
        ];
        let items = run_menu(rows);
        let labels: Vec<Option<&str>> = items.iter().map(MenuItem::label).collect();
        assert_eq!(labels, [Some("Run 'main'"), None, Some("Modify Run Configuration…")]);
        assert_eq!(items[2].command(), Some(&RunCommand::Modify(context.clone())));
        assert_eq!(context_value(&context), json!({ "path": "/p/a.rs", "dir": false, "line": 0 }));
    }
}
