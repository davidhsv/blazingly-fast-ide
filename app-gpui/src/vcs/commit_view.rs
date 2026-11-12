//! The Commit tool window's changes: the files that differ from the last commit, grouped as
//! merge conflicts, changes and unversioned files, by directory, each in the colour of what
//! happened to it. Read from the backend when shown and again when the repository changes.

use super::changes::{ChangeKind, ChangeNode, EffectiveKind, FileStatus, NodeType, build_change_tree, strip_slash};
use crate::backend::{Backend, BackendEvent};
use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, ScrollStrategy,
    SharedString, Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::IdeEvent;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

/// What the changes view asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitPanelEvent {
    /// Open this changed file.
    Open(PathBuf),
}

/// One visible line: a group, a directory or a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    key: String,
    depth: usize,
    node_type: NodeType,
    label: String,
    detail: Option<String>,
    kind: Option<EffectiveKind>,
    path: Option<String>,
    /// How many files are below a group or directory.
    files: usize,
    expanded: bool,
}

pub struct CommitPanel {
    focus: FocusHandle,
    window_label: String,
    /// The project folder paths are shown relative to.
    root: String,
    status: Vec<FileStatus>,
    tree: Vec<ChangeNode>,
    /// Keys of the groups and directories the user closed; everything else is open.
    collapsed: BTreeSet<String>,
    rows: Rc<Vec<Row>>,
    selected: Option<String>,
    loading: bool,
    error: Option<String>,
    scroll: UniformListScrollHandle,
    listener: Option<u64>,
    reading: Option<Task<()>>,
}

impl EventEmitter<CommitPanelEvent> for CommitPanel {}

fn kind(kind: ide_vcs::ChangeKind) -> ChangeKind {
    match kind {
        ide_vcs::ChangeKind::Added => ChangeKind::Added,
        ide_vcs::ChangeKind::Modified => ChangeKind::Modified,
        ide_vcs::ChangeKind::Deleted => ChangeKind::Deleted,
        ide_vcs::ChangeKind::Renamed => ChangeKind::Renamed,
        ide_vcs::ChangeKind::Untracked => ChangeKind::Untracked,
        ide_vcs::ChangeKind::Ignored => ChangeKind::Ignored,
        ide_vcs::ChangeKind::Conflicted => ChangeKind::Conflicted,
    }
}

/// A changed file as the change models hold it.
fn file_status(status: ide_vcs::FileStatus) -> FileStatus {
    let ide_vcs::FileStatus { path, rel_path, staged, unstaged, renamed_from } = status;
    FileStatus {
        path: path.0.to_string_lossy().into_owned(),
        rel_path,
        staged: staged.map(kind),
        unstaged: unstaged.map(kind),
        renamed_from,
    }
}

/// The colour token of a change, by what happened to the file.
fn kind_color(kind: EffectiveKind) -> &'static str {
    match kind {
        EffectiveKind::Modified | EffectiveKind::Renamed => "vcs-modified",
        EffectiveKind::Added => "vcs-added",
        EffectiveKind::Deleted => "vcs-deleted",
        EffectiveKind::Untracked => "vcs-untracked",
        EffectiveKind::Ignored => "vcs-ignored",
        EffectiveKind::Conflicted => "vcs-conflict",
    }
}

fn flatten(nodes: &[ChangeNode], depth: usize, collapsed: &BTreeSet<String>, rows: &mut Vec<Row>) {
    for node in nodes {
        let expanded = node.node_type != NodeType::File && !collapsed.contains(&node.key);
        rows.push(Row {
            key: node.key.clone(),
            depth,
            node_type: node.node_type,
            label: node.label.clone(),
            detail: node.detail.clone(),
            kind: node.kind,
            path: node.path.clone(),
            files: node.files.len(),
            expanded,
        });
        if expanded {
            flatten(&node.children, depth + 1, collapsed, rows);
        }
    }
}

impl CommitPanel {
    pub fn new(window_label: &str, root: PathBuf, cx: &mut Context<Self>) -> Self {
        let weak = cx.weak_entity();
        let listener = cx.try_global::<Backend>().is_some().then(|| {
            cx.global_mut::<Backend>().listen(move |event, cx| {
                let _ = weak.update(cx, |panel, cx| panel.backend_event(event, cx));
            })
        });
        cx.on_release(|panel, cx| {
            if let Some(listener) = panel.listener.filter(|_| cx.has_global::<Backend>()) {
                cx.global_mut::<Backend>().unlisten(listener);
            }
        })
        .detach();
        let mut panel = Self {
            focus: cx.focus_handle(),
            window_label: window_label.to_owned(),
            root: root.to_string_lossy().into_owned(),
            status: Vec::new(),
            tree: Vec::new(),
            collapsed: BTreeSet::new(),
            rows: Rc::default(),
            selected: None,
            loading: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            listener,
            reading: None,
        };
        panel.read(cx);
        panel
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The changed files as the backend last reported them.
    pub fn status(&self) -> &[FileStatus] {
        &self.status
    }

    /// The visible lines, indented by depth: a group or directory with its number of files.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let count = if row.node_type == NodeType::File { String::new() } else { format!(" ({})", row.files) };
                format!("{}{}{count}", "  ".repeat(row.depth), row.label)
            })
            .collect()
    }

    fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>) {
        if event.window.as_deref().is_some_and(|window| window != self.window_label) {
            return;
        }
        if matches!(event.event, IdeEvent::VcsChanged { .. }) {
            self.read(cx);
        }
    }

    /// Reads the changes again.
    pub fn read(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let window = self.window_label.clone();
        let status = backend.with_host(move |host| async move { ide_host::ops::vcs::status(&host, &window).await });
        self.loading = true;
        self.reading = Some(cx.spawn(async move |panel, cx| {
            let status = status.await;
            let _ = panel.update(cx, |panel: &mut CommitPanel, cx| {
                panel.loading = false;
                match status {
                    Ok(Ok(status)) => {
                        panel.error = None;
                        panel.status = status.into_iter().map(file_status).collect();
                        panel.rebuild(cx);
                    }
                    Ok(Err(error)) => panel.error = Some(error.message),
                    Err(error) => panel.error = Some(error.to_string()),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let i18n = &mut cx.default_global::<Localization>().0;
        self.tree = build_change_tree(&self.status, &self.root, true, None, &[], false, i18n);
        self.flatten();
    }

    fn flatten(&mut self) {
        let mut rows = Vec::new();
        flatten(&self.tree, 0, &self.collapsed, &mut rows);
        if self.selected.as_ref().is_some_and(|key| !rows.iter().any(|row| &row.key == key)) {
            self.selected = None;
        }
        self.rows = Rc::new(rows);
    }

    /// Opens or closes the group or directory `key`.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        self.flatten();
        cx.notify();
    }

    /// Opens the file of row `key`, or toggles its group or directory (Enter, double click).
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter().find(|row| row.key == key).cloned() else { return };
        match (row.node_type, row.path) {
            // A deleted file has nothing to open.
            (NodeType::File, Some(path)) if row.kind != Some(EffectiveKind::Deleted) => {
                cx.emit(CommitPanelEvent::Open(PathBuf::from(strip_slash(&path))));
            }
            (NodeType::File, _) => {}
            _ => self.toggle(key, cx),
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() || self.rows.is_empty() {
            return;
        }
        let rows = Rc::clone(&self.rows);
        let current = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.key == key));
        let row = current.and_then(|index| rows.get(index));
        match (event.keystroke.key.as_str(), row) {
            ("down", _) => {
                self.selected = rows.get(current.map_or(0, |at| (at + 1).min(rows.len() - 1))).map(|r| r.key.clone())
            }
            ("up", _) => self.selected = rows.get(current.map_or(0, |at| at.saturating_sub(1))).map(|r| r.key.clone()),
            ("right", Some(row)) if row.node_type != NodeType::File && !row.expanded => {
                self.toggle(&row.key.clone(), cx)
            }
            ("left", Some(row)) if row.node_type != NodeType::File && row.expanded => self.toggle(&row.key.clone(), cx),
            ("enter", Some(row)) => self.activate(&row.key.clone(), cx),
            _ => return,
        }
        if let Some(at) = self.selected.as_ref().and_then(|key| self.rows.iter().position(|row| &row.key == key)) {
            self.scroll.scroll_to_item(at, ScrollStrategy::Top);
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for CommitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CommitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let (fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let message = |text: String| {
            div().size_full().flex().items_center().justify_center().text_color(muted).child(SharedString::from(text))
        };
        if let Some(error) = &self.error {
            return div().size_full().track_focus(&self.focus).child(message(error.clone()));
        }
        if self.rows.is_empty() {
            let i18n = &cx.default_global::<Localization>().0;
            let text = if self.loading { i18n.tx("Loading…") } else { i18n.tx("No changes") };
            return div().size_full().track_focus(&self.focus).child(message(text));
        }
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let chevron_color = theme::color(cx, "fg-icon");
        let rows = Rc::clone(&self.rows);
        let selected = self.selected.clone();
        let list = uniform_list(
            "changes",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let folder = row.node_type != NodeType::File;
                        let color = row.kind.map_or(fg, |kind| theme::color(cx, kind_color(kind)));
                        let chevron = if folder {
                            theme::icon(
                                cx,
                                if row.expanded { "chevron-down" } else { "chevron-right" },
                                16.0,
                                chevron_color,
                            )
                        } else {
                            div().size(px(16.0)).into_any_element()
                        };
                        let icon = match row.node_type {
                            NodeType::File => {
                                Some(theme::icon(cx, theme::icons::file_icon_name(&row.label), 16.0, color))
                            }
                            NodeType::Dir => Some(theme::icon(cx, "folder", 16.0, fg)),
                            NodeType::Group => None,
                        };
                        let (clicked, toggled) = (row.key.clone(), row.key.clone());
                        div()
                            .id(SharedString::from(format!("change:{}", row.key)))
                            .h(theme::length(cx, "h-row"))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + 20.0 * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .text_color(color)
                            .when(selected.as_deref() == Some(row.key.as_str()), |el| el.bg(selected_bg))
                            .child(
                                div()
                                    .id(SharedString::from(format!("toggle:{}", row.key)))
                                    .size(px(16.0))
                                    .flex_none()
                                    .child(chevron)
                                    .when(folder, |el| {
                                        el.on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                            panel.toggle(&toggled, cx);
                                            cx.stop_propagation();
                                        }))
                                    }),
                            )
                            .children(icon)
                            .child(SharedString::from(row.label.clone()))
                            .children(
                                row.detail
                                    .clone()
                                    .map(|detail| div().text_color(muted).child(SharedString::from(detail))),
                            )
                            .when(folder, |el| {
                                let i18n = &mut cx.default_global::<Localization>().0;
                                let count = crate::vcs_diff::model::file_count(row.files, i18n);
                                el.child(div().text_color(muted).child(SharedString::from(count)))
                            })
                            .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                                window.focus(&panel.focus, cx);
                                panel.selected = Some(clicked.clone());
                                if event.click_count() >= 2 {
                                    panel.activate(&clicked, cx);
                                }
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.test")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.test")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn settle(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within ten seconds");
    }

    fn lines(panel: &Entity<CommitPanel>, cx: &mut VisualTestContext) -> Vec<String> {
        panel.read_with(cx, |panel, _| panel.lines())
    }

    /// A real repository with a modified, a deleted and a new file: they are listed under
    /// Changes and Unversioned Files by directory, each with its kind, folders close and open,
    /// Enter asks for a file to be opened, and a change outside is picked up on the backend's
    /// event.
    #[gpui_kit::test]
    async fn lists_the_repositorys_changes_by_group_and_directory(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        std::fs::create_dir(root.join("src")).expect("a folder");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("the file is written");
        std::fs::write(root.join("src/gone.rs"), "// gone\n").expect("the file is written");
        std::fs::write(root.join("README.md"), "# readme\n").expect("the file is written");
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "first"]);
        std::fs::write(root.join("src/main.rs"), "fn main() { println!(); }\n").expect("the file is written");
        std::fs::remove_file(root.join("src/gone.rs")).expect("removed");
        std::fs::write(root.join("notes.txt"), "new\n").expect("the file is written");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");

        let panel_root = root.clone();
        let (panel, cx) = cx.add_window_view(|_, cx| CommitPanel::new("w1", panel_root, cx));
        settle(cx, "the changes are read", |cx| panel.read_with(cx, |panel, _| !panel.is_loading()));
        assert_eq!(panel.read_with(cx, |panel, _| panel.error().map(str::to_owned)), None);
        assert_eq!(
            lines(&panel, cx),
            ["Changes (2)", "  src (2)", "    gone.rs", "    main.rs", "Unversioned Files (1)", "  notes.txt"]
        );
        panel.read_with(cx, |panel, _| {
            let kinds: Vec<(&str, Option<EffectiveKind>)> = panel
                .rows
                .iter()
                .filter(|row| row.node_type == NodeType::File)
                .map(|row| (row.label.as_str(), row.kind))
                .collect();
            assert_eq!(
                kinds,
                [
                    ("gone.rs", Some(EffectiveKind::Deleted)),
                    ("main.rs", Some(EffectiveKind::Modified)),
                    ("notes.txt", Some(EffectiveKind::Untracked)),
                ]
            );
        });

        let opened: Rc<std::cell::RefCell<Vec<PathBuf>>> = Rc::default();
        let log = opened.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&panel, move |_, event: &CommitPanelEvent, _| {
                let CommitPanelEvent::Open(path) = event;
                log.borrow_mut().push(path.clone());
            })
        });
        cx.update(|window, cx| {
            let focus = panel.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        // Down to Changes, to src; Left closes the folder, Right opens it again.
        cx.simulate_keystrokes("down down left");
        assert_eq!(lines(&panel, cx), ["Changes (2)", "  src (2)", "Unversioned Files (1)", "  notes.txt"]);
        cx.simulate_keystrokes("right down enter");
        assert!(opened.borrow().is_empty(), "a deleted file has nothing to open");
        cx.simulate_keystrokes("down enter");
        assert_eq!(*opened.borrow(), [root.join("src/main.rs")]);

        // The change is committed outside: the list follows on the backend's event.
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "second"]);
        let changed = BackendEvent {
            window: Some("w1".to_owned()),
            event: IdeEvent::VcsChanged { root: ide_core::IpcPath(root.clone()) },
        };
        panel.update(cx, |panel, cx| panel.backend_event(&changed, cx));
        settle(cx, "the list is empty", |cx| {
            lines(&panel, cx).is_empty() && panel.read_with(cx, |p, _| !p.is_loading())
        });
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }
}
