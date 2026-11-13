#!/usr/bin/env python3
"""Counts every anchor of app_actions_reconciled.md in the worktree (and at HEAD).

Run from anywhere: python3 -I anchors.py [worktree]. Each anchor must occur exactly once in
its file. Anchors created by a pending integration are counted in that integration's notes
instead (they must occur there at least once) and, once applied, in the file.
"""
import subprocess
import sys
from pathlib import Path

WT = Path(sys.argv[1] if len(sys.argv) > 1 else
          "/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01")
SRC = "app-gpui/src/"
NOTES = WT / "docs/gpui-port/notes/integration"

ANCHORS = [
    ("1", "lib.rs", "pub mod app_models;\n"),
    ("2", "platform/storage.rs",
     "    pub fn get(&self, key: &str) -> Option<&str> {\n"
     "        self.values.get(key).map(String::as_str)\n"
     "    }\n"),
    ("3a", "workbench/mod.rs", "mod agents;\nmod app_shell;\n"),
    ("3b", "workbench/mod.rs", "    settings_window::register_actions(registry);\n"),
    ("3c", "workbench/mod.rs", "        self.publish_merge_context(window, cx);\n"),
    ("3d", "workbench/mod.rs",
     "            IdeEvent::ProjectTrustChanged { trusted, .. } => {\n"
     "                self.agents_trust_changed(*trusted, cx);\n"
     "                return;\n"
     "            }\n"),
    ("3e", "workbench/mod.rs",
     "        (self.editor().is_some() || self.task_status().is_some())\n"
     "            && crate::platform::settings::global(cx).appearance.show_status_bar\n"),
    ("3f", "workbench/mod.rs",
     "                // \"Tool Window Bars\" off hides the stripes; the windows stay reachable by action.\n"
     "                let bars = crate::platform::settings::global(cx).appearance.show_tool_window_bars;\n"),
    ("3g", "workbench/mod.rs",
     "            .child(self.title_bar().clone())\n"
     "            .child(div().flex_1().min_h_0().w_full().child(body))\n"),
    ("3h", "workbench/mod.rs", "                        layout.load(&scope, stored.as_deref());\n"),
    ("3i", "workbench/mod.rs",
     "        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect(\"the folder exists\")).collect();\n"
     "        for root in &roots {\n"
     "            std::fs::write(root.join(\"notes.txt\"), \"notes\\n\").expect(\"the file is written\");\n"
     "        }\n"),
    ("3j", "workbench/mod.rs",
     "        let name = root.file_name().expect(\"a folder name\").to_string_lossy().into_owned();\n"
     "        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(\"w1\", window, cx));\n"),
    ("4", "workbench/chrome.rs",
     "                (TitleBarKind::Project, title_bar_mode(false, show_main_toolbar), None, Some(project))\n"),
    ("5a.1", "workbench/app_shell.rs",
     "pub(super) fn quit(window: &mut Window, cx: &mut App) {\n"
     "    if quitting(cx) {\n"
     "        return;\n"
     "    }\n"),
    ("5a.2", "workbench/app_shell.rs", "        cx.update(|cx| cx.quit());\n"),
    ("5b", "workbench/app_shell.rs",
     "            OpenStep::OpenProject { path } => {\n"
     "                this.update_in(cx, |workbench, window, cx| {\n"
     "                    // Leaving what the window showed was decided first: it saves nothing more.\n"
     "                    workbench.leave_decided = true;\n"
     "                    workbench.open_project(PathBuf::from(path), window, cx);\n"
     "                    // An open that did not reach `leave` must not spare a later one its autosave.\n"
     "                    workbench.leave_decided = false;\n"
     "                })?;\n"
     "                let opened = project_opened(this, cx).await?;\n"
     "                cx.update(|_, cx| flow.project_opened(opened, i18n(cx)))?\n"
     "            }\n"),
    ("5c", "workbench/app_shell.rs",
     "            Content::Empty | Content::Editor(_) | Content::Viewer(_) => {\n"
     "                Some(Err(IdeError::new(ErrorKind::Cancelled, \"Project opening cancelled\")))\n"
     "            }\n"),
    ("6.1", "workbench/windows.rs", "                Target::Project(root) => workbench.open_project(root, window, cx),\n"),
    ("6.2", "workbench/windows.rs",
     "        Some(workbench) => workbench.update(cx, |workbench, cx| workbench.open_project(path.to_path_buf(), window, cx)),\n"),
]

# Anchors that a pending integration creates: (hunk, file, text, notes file that creates it).
CREATED = [
    ("7", "workbench/menu.rs",
     "        (super::menu_spec::DYNAMIC_PLUGIN_TOOLS.to_owned(), super::plugin_contributions::plugin_tool_items(cx)),\n",
     "plugins_all_integration.md"),
]

def head(path):
    out = subprocess.run(["git", "-C", str(WT), "show", f"HEAD:{SRC}{path}"], capture_output=True, text=True)
    return out.stdout if out.returncode == 0 else ""


def main():
    bad = 0
    for hunk, path, text in ANCHORS:
        now = (WT / SRC / path).read_text().count(text)
        at_head = head(path).count(text)
        flag = "" if now == 1 else "   <-- NOT ONE"
        bad += now != 1
        print(f"{hunk:5} {path:28} worktree={now} HEAD={at_head}{flag}")
    for hunk, path, text, notes in CREATED:
        in_notes = (NOTES / notes).read_text().count(text)
        now = (WT / SRC / path).read_text().count(text)
        state = "applied" if now == 1 else ("pending" if now == 0 else "NOT ONE")
        bad += in_notes < 1 or now > 1
        print(f"{hunk:5} {path:28} created by {notes}: in notes={in_notes}, file={now} ({state})")
    print(f"NOT ONE: {bad}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
