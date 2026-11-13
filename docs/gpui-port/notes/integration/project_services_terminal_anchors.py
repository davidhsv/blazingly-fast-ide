#!/usr/bin/env python3
"""Recounts every anchor of project_services_terminal_reconciled.md.

Run from the worktree root: python3 -I <this file>. Each anchor of today's code must occur exactly
once in its file; an anchor that an earlier pending integration creates must occur in that
integration's notes (and 0 or 1 times in the file, depending on whether it is applied yet).
Prints one line per anchor and a final 'NOT ONE: n'.
"""
import pathlib
import sys

ROOT = pathlib.Path.cwd()
NOTES = ROOT / "docs/gpui-port/notes/integration"

# (file, anchor, created_by_notes or None)
ANCHORS = [
    # A. workbench/mod.rs
    ("app-gpui/src/workbench/mod.rs", "\nmod project_files;\n", None),
    ("app-gpui/src/workbench/mod.rs", "\nmod services_window;\n", None),
    ("app-gpui/src/workbench/mod.rs", "\nmod todo_window;\n", None),
    ("app-gpui/src/workbench/mod.rs", "    todo_window::register_actions(registry);\n", None),
    (
        "app-gpui/src/workbench/mod.rs",
        '    registry.handle("terminal.closeSession", |_, window, cx| {\n'
        "        on_terminals(window, cx, |terminals, cx| terminals.close_active_tab(cx))\n"
        "    });\n",
        None,
    ),
    # B. workbench/project_files.rs
    (
        "app-gpui/src/workbench/project_files.rs",
        "\ntype Done = Box<dyn FnOnce(&mut Workbench, Outcome, &mut Window, &mut Context<Workbench>)>;\n",
        None,
    ),
    ("app-gpui/src/workbench/project_files.rs", "\n    fn with_view<R>(\n", None),
    ("app-gpui/src/workbench/project_files.rs", "\n    fn start_flow(\n", None),
    (
        "app-gpui/src/workbench/project_files.rs",
        "\n    fn nodes_for(&mut self, arg: Option<&Value>, cx: &mut Context<Self>) -> Vec<Arc<ProjectNode>> {\n",
        None,
    ),
    ("app-gpui/src/workbench/project_files.rs", "\n    fn active_file_path(&self, cx: &App) -> Option<String> {\n", None),
    (
        "app-gpui/src/workbench/project_files.rs",
        "\n    fn with_menu_context<R>(&self, cx: &App, build: impl FnOnce(&MenuContext<'_>, &App) -> R) -> R {\n",
        None,
    ),
    ("app-gpui/src/workbench/project_files.rs", "\n    fn run_project_command(\n", None),
    (
        "app-gpui/src/workbench/project_files.rs",
        "            Effect::MarkDirectory { path, .. } => {\n"
        "                let request = NotificationRequest {\n"
        "                    body: Some(path),\n"
        '                    ..NotificationRequest::new(NotificationLevel::Warning, "Mark Directory as is not available here")\n'
        "                };\n"
        "                self.notify(request, cx);\n"
        "            }\n",
        None,
    ),
    (
        "app-gpui/src/workbench/project_files.rs",
        "            Effect::RevealInFileManager(paths) => {\n"
        "                for path in paths {\n"
        "                    cx.reveal_path(Path::new(&path));\n"
        "                }\n"
        "            }\n",
        None,
    ),
    # C. terminal/panel.rs
    (
        "app-gpui/src/terminal/panel.rs",
        "use super::store::{SessionCommand, SessionSnapshot, SessionStatus, TerminalEffect, Terminals, start_directory};\n",
        None,
    ),
    (
        "app-gpui/src/terminal/panel.rs",
        "use gpui_kit::{\n"
        "    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Subscription,\n"
        "    Window, div, prelude::*, px,\n"
        "};\n",
        None,
    ),
    (
        "app-gpui/src/terminal/panel.rs",
        "    pub fn terminals(&self) -> &Terminals {\n        &self.terminals\n    }\n",
        None,
    ),
    (
        "app-gpui/src/terminal/panel.rs",
        "    /// Starts the shell of the active tab's session if it has none yet. A tab that is not\n"
        "    /// showing, such as one remembered from the last run, costs no process until it is selected.\n"
        "    fn start_active(&mut self, cx: &mut Context<Self>) {\n"
        "        let Some(key) = self.terminals.active_session().map(|session| session.key.clone()) else { return };\n"
        "        if self.views.contains_key(&key) {\n"
        "            return;\n"
        "        }\n"
        "        let Some(options) = self.options(&key, cx) else { return };\n"
        "        let label = self.window_label.clone();\n"
        "        let view = cx.new(|cx| TerminalView::new(&label, options, cx));\n"
        "        let session = key.clone();\n"
        "        let events = cx.subscribe(&view, move |panel: &mut TerminalPanel, view, event: &TerminalEvent, cx| {\n"
        "            panel.terminal_event(&session, &view, event, cx);\n"
        "        });\n"
        "        let state = self.snapshot(&key, &view, cx);\n"
        "        self.views.insert(key.clone(), (view, events));\n"
        "        self.terminals.set_session_state(&key, state);\n"
        "    }\n",
        None,
    ),
    (
        "app-gpui/src/terminal/panel.rs",
        "    pub fn started(&self) -> usize {\n        self.views.len()\n    }\n",
        None,
    ),
    (
        "app-gpui/src/terminal/panel.rs",
        "            .child(div().flex_1().min_h_0().children(self.active_view().cloned()))\n",
        None,
    ),
    # D. editor/actions.rs (the first line stays the same through editor_all 2.7)
    ("app-gpui/src/editor/actions.rs", '    registry.handle("edit.undo", |_, window, cx| {\n', None),
    # E. crates/ide-host/src/lib.rs
    ("crates/ide-host/src/lib.rs", "pub type SinkFactory = Box<dyn Fn(&str) -> SharedSink + Send + Sync>;\n", None),
    ("crates/ide-host/src/lib.rs", "    pub(crate) mcp_executable: RwLock<Option<PathBuf>>,\n", None),
    ("crates/ide-host/src/lib.rs", "            mcp_executable: RwLock::default(),\n", None),
    (
        "crates/ide-host/src/lib.rs",
        "    pub fn set_mcp_executable(&self, executable: Option<PathBuf>) {\n"
        "        *self.mcp_executable.write() = executable;\n"
        "    }\n",
        None,
    ),
    # F. crates/ide-host/src/ops/fs.rs
    (
        "crates/ide-host/src/ops/fs.rs",
        "/// Moves to the trash; Local History keeps the contents so deleted files can be restored.\n"
        "pub async fn delete(host: &Host, window: &str, path: PathBuf) -> IdeResult<()> {\n"
        "    let history = host.project(window).ok().map(|p| p.history.clone());\n"
        "    host.blocking(move || {\n"
        "        let paths = ide_workspace::file_ops::deletion_paths(&path)?;\n"
        "        if let Some(h) = history {\n"
        "            for p in paths {\n"
        "                h.record_deletion(&p, DELETE_SNAPSHOT_LIMIT);\n"
        "            }\n"
        "        }\n"
        "        ide_workspace::file_ops::delete(&path)\n"
        "    })\n"
        "    .await\n"
        "}\n",
        None,
    ),
    (
        "crates/ide-host/src/ops/fs.rs",
        "        None => Ok(ide_vfs::load_text(&path)?.text),\n    })\n    .await\n}\n",
        None,
    ),
    # D again: the first line of editor_all 2.7's replacement is the same line
    (
        "app-gpui/src/editor/actions.rs",
        '    registry.handle("edit.undo", |_, window, cx| {\n'
        "        // With nothing of its own left to undo, the editor undoes the refactoring of its file.\n",
        "editor_all_integration.md",
    ),
    # G. workbench/context_menus.rs (the reconciled draft, declared by context_menus_reconciled.md)
    (
        "app-gpui/src/workbench/context_menus.rs",
        "    /// registers: Folding, Rename…, Refactor, Generate…, Open In, Local History and the Compare rows\n",
        None,
    ),
    ("app-gpui/src/workbench/context_menus.rs", '        "Send to Agent",\n        "Git",\n    ];\n', None),
    ("app-gpui/src/workbench/context_menus.rs", "    const ROOT: [&str; ", None),
    # H. mod.rs test a_tabs_menu_closes_pins_and_splits, as context_menus_reconciled.md 1h leaves it
    (
        "app-gpui/src/workbench/mod.rs",
        '                "Split Down",\n'
        "                // The file's rows whose actions are registered: the bookmark group, Git, Copy Path/Reference…\n"
        "                // (Copy Absolute Path) and Rename File…\n"
        '                "Add Bookmark",\n'
        '                "Git",\n',
        "context_menus_reconciled.md",
    ),
]


def main() -> int:
    bad = 0
    for path, anchor, created_by in ANCHORS:
        text = (ROOT / path).read_text(encoding="utf-8")
        count = text.count(anchor)
        first = anchor.strip().splitlines()[0][:70]
        if created_by:
            in_notes = (NOTES / created_by).read_text(encoding="utf-8").count(anchor)
            ok = in_notes >= 1 and count in (0, 1)
            print(f"{'ok ' if ok else 'BAD'} {path}: {count} in file, {in_notes} in {created_by}: {first}")
        else:
            ok = count == 1
            print(f"{'ok ' if ok else 'BAD'} {path}: {count}: {first}")
        bad += 0 if ok else 1
    print(f"NOT ONE: {bad}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
