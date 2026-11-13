"""Recounts every anchor of github_reconciled.md in the files as they are now.

Usage: python3 -I anchors.py <worktree>

For each hunk: the anchor must occur exactly once in its file (as a substring; one-line anchors also
as a whole line). It also reports any pending integration that inserts a line equal to one of these
anchors (it would make the anchor ambiguous once applied), runs the ide-github hunks' own check
(crate_hunks.py), and compares the drafted file with its copy under files/.
"""
import filecmp
import pathlib
import re
import subprocess
import sys

W = pathlib.Path(sys.argv[1]).resolve()
HERE = pathlib.Path(__file__).resolve().parent

# (hunk id, file, anchor). Multi-line anchors are written with \n.
ANCHORS = [
    ("1.1", "app-gpui/src/workbench/mod.rs", "mod log_actions;"),
    ("1.2", "app-gpui/src/workbench/mod.rs", "use structure_window::STRUCTURE_TOOL_WINDOW;"),
    ("1.3a", "app-gpui/src/workbench/mod.rs", "    vcs: Option<branches::VcsSlot>,"),
    ("1.3b", "app-gpui/src/workbench/mod.rs", "            services: None,"),
    ("1.4a", "app-gpui/src/workbench/mod.rs", "const TOOL_WINDOWS: [&str; 18] = ["),
    ("1.4b", "app-gpui/src/workbench/mod.rs", "    DEBUG_TOOL_WINDOW,\n];"),
    ("1.5", "app-gpui/src/workbench/mod.rs", "    services_window::register_actions(registry);"),
    ("1.6a", "app-gpui/src/workbench/mod.rs",
     "                crate::agents::model::TOOL_WINDOW => match self.mount_agents_panel(window, cx) {"),
    ("1.6b", "app-gpui/src/workbench/mod.rs", "        self.sync_services_window(cx);"),
    ("1.7", "app-gpui/src/workbench/mod.rs", "        self.publish_vcs_context(window, cx);"),
    ("1.8a", "app-gpui/src/workbench/mod.rs",
     "        cx.global::<Backend>().host.close_project_detached(&self.label);"),
    ("1.8b", "app-gpui/src/workbench/mod.rs", "        self.files = Default::default();"),
    ("2.1", "app-gpui/src/workbench/branches.rs",
     "    /// Writes `text` to the window's Git console.\n"
     "    pub(super) fn log_vcs_console(&mut self, kind: ConsoleKind, text: &str) {\n"
     "        if let Some(slot) = self.vcs.as_mut() {\n"
     "            slot.console.log(kind, text, now_ms());\n"
     "        }\n"
     "    }\n"),
    ("2.2", "app-gpui/src/workbench/branches.rs", "        self.sync_log_panes(cx);\n        cx.notify();"),
    ("8", "docs/gpui-port/inventory.mjs", "const actionMap = actions();"),
]

PENDING = [
    "plugins_all_integration", "context_menus_reconciled", "editor_all_integration", "app_actions_reconciled",
    "project_services_terminal_integration",
]

DRAFTED = ["app-gpui/src/workbench/github.rs"]


def fenced_lines(text):
    """The lines inside ``` fences of a notes file."""
    inside, out = False, []
    for line in text.splitlines():
        if line.startswith("```"):
            inside = not inside
            continue
        if inside:
            out.append(line)
    return out


def main():
    bad = 0
    for hunk, rel, anchor in ANCHORS:
        text = (W / rel).read_text()
        n = text.count(anchor)
        whole = sum(1 for line in text.splitlines() if line == anchor) if "\n" not in anchor else n
        ok = n == 1 and whole == 1
        bad += not ok
        first = anchor.splitlines()[0]
        print(f"{'ok ' if ok else 'BAD'} {hunk:5} {rel}: {n} occurrence(s) ({whole} whole-line) | {first}")
    notes = W / "docs/gpui-port/notes/integration"
    single = {a for _, _, a in ANCHORS if "\n" not in a}
    for name in PENDING:
        inserted = fenced_lines((notes / f"{name}.md").read_text())
        clashes = sorted({line for line in inserted if line in single})
        print(f"pending {name}: {'no inserted line equals an anchor' if not clashes else clashes}")
        bad += bool(clashes)
    crate = subprocess.run([sys.executable, "-I", str(HERE / "crate_hunks.py"), str(W)], capture_output=True, text=True)
    print(crate.stdout.strip() or crate.stderr.strip())
    bad += crate.returncode != 0
    for rel in DRAFTED:
        copy = HERE / "files" / rel
        same = copy.exists() and filecmp.cmp(W / rel, copy, shallow=False)
        print(f"{'ok ' if same else 'BAD'} copy of {rel} is byte-identical")
        bad += not same
        lines = (W / rel).read_text().splitlines()
        long = [i + 1 for i, line in enumerate(lines) if len(line) > 120]
        words = [
            i + 1 for i, line in enumerate(lines)
            if line.lstrip().startswith("//") and re.search(r"\b(port|webview|TypeScript|React|parity)\b", line)
        ]
        print(f"{rel}: lines over 120 columns {long}; comment lines with banned words {words}")
        bad += bool(long) + bool(words)
    print("ALL OK" if not bad else f"{bad} problem(s)")
    sys.exit(1 if bad else 0)


main()
