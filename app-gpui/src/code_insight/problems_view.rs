//! The Problems tool window: the current editor's file ("File: main.rs  3") or the whole project's
//! errors ("Project Errors"), as a tree of files and their problems, each with its severity icon,
//! message, line and producer. The toolbar steps through the problems and shows or hides errors,
//! warnings and infos; the ⋮ menu sorts by severity or name and turns the preview and Autoscroll to
//! Source on. ↑ / ↓ move, → / ← open or close a file, Enter, F4 or a double click opens the problem
//! in the editor, ⌥Enter shows its quick fixes, ⌘C copies its description, and the context menu has
//! all of those and the problem's inspection settings.
//!
//! The view shows what [`super::problems::ProblemsStore`] holds, handed in with
//! [`ProblemsView::set_problems`]; it reports what the user asks for through [`ProblemsViewEvent`].

use super::problems::{
    ProblemCommand, ProblemFile, ProblemShortcuts, ProblemTarget, ProblemsMap, ProblemsOptions, ProblemsTab,
    SeverityFilter, SeverityGroup, build_problem_nodes, diagnostic_inspection_of, empty_text, file_summary,
    file_tab_count, problem_menu, problem_text, project_error_count,
};
use crate::find::store::StorageWrite;
use crate::find::utf16;
use crate::find::window_store::HeaderMenuItem;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::virtual_docs::{basename, relative_dir};
use crate::platform_more::inspection_controls::InspectionCommand;
use crate::platform_more::inspections::StyledDiagnostic;
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
use crate::settings_model::inspections_model::ProfileOverrides;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::{Diagnostic, Severity};
use std::collections::HashSet;
use std::rc::Rc;

/// Lines the preview shows around the problem.
const PREVIEW_CONTEXT: usize = 4;

/// What the Problems tool window asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum ProblemsViewEvent {
    /// Open the problem; `focus`: the editor takes the keyboard (not for Autoscroll to Source).
    Open { path: String, line: u32, col: u32, focus: bool },
    /// Open the problem and show its quick fixes (`code.showIntentions`).
    ShowQuickFixes { path: String, line: u32, col: u32 },
    /// Show this context menu at `position`; carry out the chosen row with [`ProblemsView::run_command`].
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<ProblemCommand>> },
    /// Change the problem's inspection in the profile.
    Inspection(InspectionCommand),
    /// Keep the view options.
    Store(StorageWrite),
    /// The preview needs the lines of `path`: hand them in with [`ProblemsView::set_preview_lines`].
    PreviewWanted(String),
}

/// A line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    File { file: usize, expanded: bool },
    Problem { file: usize, item: usize },
}

/// The Problems tool window.
pub struct ProblemsView {
    map: ProblemsMap,
    profile: ProfileOverrides,
    current: Option<String>,
    root: String,
    tab: ProblemsTab,
    show: SeverityFilter,
    options: ProblemsOptions,
    shortcuts: ProblemShortcuts,
    /// Files the user closed; every other file is open.
    collapsed: HashSet<String>,
    files: Rc<Vec<ProblemFile>>,
    rows: Rc<Vec<Row>>,
    /// The key of the selected row: a file's path, or the path and index of a problem.
    selected: Option<String>,
    preview_lines: Option<(String, Rc<Vec<String>>)>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<ProblemsViewEvent> for ProblemsView {}

fn problem_key(path: &str, item: usize) -> String {
    format!("{path}\u{0}{item}")
}

fn severity_icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info | Severity::Hint => "info",
    }
}

fn severity_color(cx: &App, severity: Severity) -> Hsla {
    theme::color(
        cx,
        match severity {
            Severity::Error => "danger",
            Severity::Warning => "warning",
            Severity::Info | Severity::Hint => "info",
        },
    )
}

fn target_of(path: &str, diagnostic: &Diagnostic) -> ProblemTarget {
    let start = diagnostic.range.start;
    ProblemTarget { path: path.to_owned(), line: start.line, col: start.col }
}

impl ProblemsView {
    /// The tool window with the view options as they were kept.
    pub fn new(options: ProblemsOptions, cx: &mut Context<Self>) -> Self {
        Self {
            map: ProblemsMap::default(),
            profile: ProfileOverrides::new(),
            current: None,
            root: String::new(),
            tab: ProblemsTab::File,
            show: SeverityFilter::default(),
            options,
            shortcuts: ProblemShortcuts::default(),
            collapsed: HashSet::new(),
            files: Rc::default(),
            rows: Rc::default(),
            selected: None,
            preview_lines: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// The problems changed.
    pub fn set_problems(&mut self, map: &ProblemsMap, cx: &mut Context<Self>) {
        self.map = map.clone();
        self.rebuild(cx);
    }

    /// The inspection profile in effect, for the context menu's severity rows.
    pub fn set_profile(&mut self, profile: ProfileOverrides) {
        self.profile = profile;
    }

    /// The active editor's file changed.
    pub fn set_current_path(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if self.current != path {
            self.current = path;
            self.rebuild(cx);
        }
    }

    pub fn set_project_root(&mut self, root: &str, cx: &mut Context<Self>) {
        self.root = root.to_owned();
        cx.notify();
    }

    /// The shortcuts the context menu shows.
    pub fn set_shortcuts(&mut self, shortcuts: ProblemShortcuts) {
        self.shortcuts = shortcuts;
    }

    /// The lines of `path`, for the preview.
    pub fn set_preview_lines(&mut self, path: &str, lines: Vec<String>, cx: &mut Context<Self>) {
        self.preview_lines = Some((path.to_owned(), Rc::new(lines)));
        cx.notify();
    }

    pub fn tab(&self) -> ProblemsTab {
        self.tab
    }

    pub fn severity_filter(&self) -> SeverityFilter {
        self.show
    }

    pub fn options(&self) -> ProblemsOptions {
        self.options
    }

    /// The scope tabs as the header shows them: "File" with the file's name, and "Project Errors",
    /// each with its count.
    pub fn tab_labels(&self, i18n: &I18n) -> [(ProblemsTab, String, Option<usize>); 2] {
        let file = match &self.current {
            Some(path) => format!("{} {}", i18n.tx("File"), basename(path)),
            None => i18n.tx("File"),
        };
        [
            (ProblemsTab::File, file, file_tab_count(&self.map, self.current.as_deref())),
            (ProblemsTab::Project, i18n.tx("Project Errors"), project_error_count(&self.map)),
        ]
    }

    /// The entries of the tool window's ⋮ menu.
    pub fn header_menu(&self) -> [HeaderMenuItem; 6] {
        self.options.header_menu()
    }

    /// The tree as text, a line per row: a file's name, or a problem's message and line, indented.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| match row {
                Row::File { file, .. } => basename(&self.files[*file].path).into_owned(),
                Row::Problem { file, item } => {
                    let diagnostic = &self.files[*file].items[*item].diagnostic;
                    format!("  {} :{}", first_line(&diagnostic.message), diagnostic.range.start.line + 1)
                }
            })
            .collect()
    }

    /// The selected row's key.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// What the empty view says; `None` while it lists problems.
    pub fn empty_text(&self) -> Option<&'static str> {
        self.rows.is_empty().then(|| empty_text(self.tab, self.current.as_deref()))
    }

    pub fn select_tab(&mut self, tab: ProblemsTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.selected = None;
            self.rebuild(cx);
        }
    }

    /// Shows or hides a severity group.
    pub fn toggle_severity(&mut self, group: SeverityGroup, cx: &mut Context<Self>) {
        self.show.toggle(group);
        self.rebuild(cx);
    }

    /// Toggles the ⋮ menu option `id`.
    pub fn toggle_option(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(stored) = self.options.toggle(id) else { return };
        cx.emit(ProblemsViewEvent::Store(stored));
        self.rebuild(cx);
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<&str> = match self.tab {
            ProblemsTab::File => self.current.as_deref().into_iter().collect(),
            ProblemsTab::Project => self.map.paths().collect(),
        };
        self.files = Rc::new(build_problem_nodes(&self.map, &paths, self.show, self.options.order));
        let mut rows = Vec::new();
        for (file, problems) in self.files.iter().enumerate() {
            let expanded = !self.collapsed.contains(&problems.path);
            rows.push(Row::File { file, expanded });
            if expanded {
                rows.extend((0..problems.items.len()).map(|item| Row::Problem { file, item }));
            }
        }
        self.rows = Rc::new(rows);
        if self.selected.as_ref().is_some_and(|key| self.index_of(key).is_none()) {
            self.selected = None;
        }
        self.want_preview(cx);
        cx.notify();
    }

    fn key_of(&self, row: &Row) -> String {
        match row {
            Row::File { file, .. } => self.files[*file].path.clone(),
            Row::Problem { file, item } => problem_key(&self.files[*file].path, *item),
        }
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|row| self.key_of(row) == key)
    }

    fn selected_index(&self) -> Option<usize> {
        self.selected.as_deref().and_then(|key| self.index_of(key))
    }

    /// The problem a row stands for: its own, or a file's first.
    fn target(&self, index: usize) -> Option<(&str, &StyledDiagnostic)> {
        let (file, item) = match self.rows.get(index)? {
            Row::File { file, .. } => (*file, 0),
            Row::Problem { file, item } => (*file, *item),
        };
        let problems = self.files.get(file)?;
        Some((problems.path.as_str(), problems.items.get(item)?))
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else { return };
        self.selected = Some(self.key_of(&row));
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        // Autoscroll to Source (off by default, as in IDEA): the problem shows, the list keeps the keyboard.
        if self.options.autoscroll
            && matches!(row, Row::Problem { .. })
            && let Some((path, problem)) = self.target(index)
        {
            let at = target_of(path, &problem.diagnostic);
            cx.emit(ProblemsViewEvent::Open { path: at.path, line: at.line, col: at.col, focus: false });
        }
        self.want_preview(cx);
        cx.notify();
    }

    fn want_preview(&mut self, cx: &mut Context<Self>) {
        if !self.options.preview {
            return;
        }
        let selected = self.selected_index().and_then(|index| self.target(index));
        let Some(path) = selected.map(|(path, _)| path.to_owned()) else { return };
        if self.preview_lines.as_ref().is_none_or(|(loaded, _)| *loaded != path) {
            cx.emit(ProblemsViewEvent::PreviewWanted(path));
        }
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((path, problem)) = self.target(index) {
            let at = target_of(path, &problem.diagnostic);
            cx.emit(ProblemsViewEvent::Open { path: at.path, line: at.line, col: at.col, focus: true });
        }
    }

    fn quick_fixes(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((path, problem)) = self.target(index) {
            let at = target_of(path, &problem.diagnostic);
            cx.emit(ProblemsViewEvent::ShowQuickFixes { path: at.path, line: at.line, col: at.col });
        }
    }

    fn copy(&self, index: usize, cx: &mut Context<Self>) {
        if let (Some(Row::Problem { .. }), Some((_, problem))) = (self.rows.get(index), self.target(index)) {
            cx.write_to_clipboard(ClipboardItem::new_string(problem_text(&problem.diagnostic)));
        }
    }

    fn toggle_file(&mut self, file: usize, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(file).map(|problems| problems.path.clone()) else { return };
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        self.rebuild(cx);
    }

    /// Next / Previous Occurrence from anywhere: selects the next problem and opens it, focusing the
    /// editor only when `focus_editor`. `None` when there is none in that direction.
    pub fn step_occurrence(
        &mut self,
        direction: Direction,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) -> Option<OccurrenceStep> {
        let problems: Vec<usize> =
            (0..self.rows.len()).filter(|index| matches!(self.rows[*index], Row::Problem { .. })).collect();
        let current = self.selected_index();
        let next = match direction {
            Direction::Next => problems.iter().position(|index| current.is_none_or(|at| *index > at)),
            Direction::Previous => problems.iter().rposition(|index| current.is_some_and(|at| *index < at)),
        }?;
        let index = problems[next];
        let autoscroll = std::mem::replace(&mut self.options.autoscroll, false);
        self.select(index, cx);
        self.options.autoscroll = autoscroll;
        if let Some((path, problem)) = self.target(index) {
            let at = target_of(path, &problem.diagnostic);
            cx.emit(ProblemsViewEvent::Open { path: at.path, line: at.line, col: at.col, focus: focus_editor });
        }
        Some(OccurrenceStep { index: next + 1, total: problems.len() })
    }

    /// The context menu of row `index`.
    pub fn menu(&self, index: usize, i18n: &mut I18n) -> Vec<MenuItem<ProblemCommand>> {
        let Some((path, problem)) = self.target(index) else { return Vec::new() };
        let on_problem = matches!(self.rows.get(index), Some(Row::Problem { .. }));
        let diagnostic = on_problem.then_some(&problem.diagnostic);
        let inspection = diagnostic.and_then(|diagnostic| diagnostic_inspection_of(&self.map, path, diagnostic));
        let target = target_of(path, &problem.diagnostic);
        problem_menu(target, diagnostic, inspection.as_ref(), &self.profile, &self.shortcuts, i18n)
    }

    /// Carries out a context menu row the user chose.
    pub fn run_command(&mut self, command: ProblemCommand, cx: &mut Context<Self>) {
        match command {
            ProblemCommand::ShowQuickFixes(at) => {
                cx.emit(ProblemsViewEvent::ShowQuickFixes { path: at.path, line: at.line, col: at.col });
            }
            ProblemCommand::JumpToSource(at) => {
                cx.emit(ProblemsViewEvent::Open { path: at.path, line: at.line, col: at.col, focus: true });
            }
            ProblemCommand::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            ProblemCommand::Inspection(command) => cx.emit(ProblemsViewEvent::Inspection(command)),
        }
    }

    fn show_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.select(index, cx);
        let items = self.menu(index, &mut cx.default_global::<Localization>().0);
        if !items.is_empty() {
            cx.emit(ProblemsViewEvent::ShowMenu { position, items });
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        let current = self.selected_index();
        // Copy is ⌘C on macOS and Ctrl+C elsewhere; the list has no other use for either.
        if (modifiers.platform || modifiers.control) && !modifiers.alt && key == "c" {
            if let Some(index) = current {
                self.copy(index, cx);
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.alt && !modifiers.platform && !modifiers.control && key == "enter" {
            if let Some(index) = current {
                self.quick_fixes(index, cx);
            }
            cx.stop_propagation();
            return;
        }
        if modifiers.modified() {
            return;
        }
        let Some(last) = self.rows.len().checked_sub(1) else { return };
        let row = current.and_then(|index| self.rows.get(index).cloned());
        match (key, row) {
            ("down", _) => self.select(current.map_or(0, |index| (index + 1).min(last)), cx),
            ("up", _) => self.select(current.map_or(0, |index| index.saturating_sub(1)), cx),
            ("right", Some(Row::File { file, expanded: false })) => self.toggle_file(file, cx),
            ("right", Some(Row::File { .. })) => self.select(current.map_or(0, |index| (index + 1).min(last)), cx),
            ("left", Some(Row::File { file, expanded: true })) => self.toggle_file(file, cx),
            ("left", Some(Row::Problem { file, .. })) => {
                let parent = self.rows.iter().position(|row| *row == Row::File { file, expanded: true });
                if let Some(parent) = parent {
                    self.select(parent, cx);
                }
            }
            ("enter" | "f4", Some(_)) => {
                if let Some(index) = current {
                    self.open(index, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

fn first_line(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

impl Focusable for ProblemsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn tool_button(cx: &App, selector: &'static str, icon: &str, pressed: bool) -> Stateful<Div> {
    let color = theme::color(cx, if pressed { "fg-strong" } else { "fg-muted" });
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, color))
}

fn text(content: &str, color: Hsla) -> Div {
    div().flex_none().text_color(color).child(SharedString::from(content.to_owned()))
}

impl Render for ProblemsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &mut cx.default_global::<Localization>().0;
        let tabs = self.tab_labels(i18n);
        let empty = self.empty_text().map(|text| i18n.tx(text));
        let summaries: Vec<String> = self.files.iter().map(|file| file_summary(&file.items, i18n)).collect();
        let (strong, fg) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg"));
        let (subtle, faint) = (theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let border = theme::color(cx, "border");

        let tab_bar = div()
            .flex_none()
            .h(theme::length(cx, "h-compact-header"))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(border)
            .children(tabs.into_iter().map(|(tab, label, count)| {
                let selector = match tab {
                    ProblemsTab::File => "problems-tab-file",
                    ProblemsTab::Project => "problems-tab-project",
                };
                let on = tab == self.tab;
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .h(px(22.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(theme::length(cx, "radius-tab"))
                    .text_color(if on { strong } else { fg })
                    .when(on, |el| el.bg(theme::color(cx, "bg-tab-selected")))
                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                    .child(SharedString::from(label))
                    .children(count.map(|count| text(&count.to_string(), theme::color(cx, "fg-muted"))))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(tab, cx)))
            }));

        let show = self.show;
        let toolbar =
            div()
                .flex_none()
                .w(px(30.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.0))
                .py(px(4.0))
                .border_r_1()
                .border_color(border)
                .child(tool_button(cx, "problems-previous", "arrow-up", false).on_click(cx.listener(
                    |view, _: &ClickEvent, _, cx| {
                        view.step_occurrence(Direction::Previous, false, cx);
                    },
                )))
                .child(tool_button(cx, "problems-next", "arrow-down", false).on_click(cx.listener(
                    |view, _: &ClickEvent, _, cx| {
                        view.step_occurrence(Direction::Next, false, cx);
                    },
                )))
                .child(tool_button(cx, "problems-show-errors", "error", show.error).on_click(
                    cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_severity(SeverityGroup::Error, cx)),
                ))
                .child(tool_button(cx, "problems-show-warnings", "warning", show.warning).on_click(
                    cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_severity(SeverityGroup::Warning, cx)),
                ))
                .child(tool_button(cx, "problems-show-infos", "info", show.info).on_click(
                    cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_severity(SeverityGroup::Info, cx)),
                ));

        let body = match empty {
            Some(empty) => {
                let icon = if self.tab == ProblemsTab::File && self.current.is_none() { "problems" } else { "check" };
                div()
                    .debug_selector(|| "problems-empty".to_owned())
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .text_color(subtle)
                    .child(theme::icon(cx, icon, 32.0, faint))
                    .child(SharedString::from(empty))
                    .into_any_element()
            }
            None => self.tree(window, summaries, cx),
        };
        let preview = self.options.preview.then(|| self.preview_pane(cx));
        div().size_full().flex().flex_col().text_size(theme::length(cx, "fs-ui")).child(tab_bar).child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(toolbar)
                .child(div().flex_1().min_w_0().child(body))
                .children(preview),
        )
    }
}

impl ProblemsView {
    fn tree(&self, window: &Window, summaries: Vec<String>, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.focus.is_focused(window);
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (strong, fg) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg"));
        let muted = theme::color(cx, "fg-muted");
        let (subtle, faint) = (theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let (danger, chevron_color) = (theme::color(cx, "danger"), theme::color(cx, "fg-icon"));
        let row_height = theme::length(cx, "h-row");
        let rows = Rc::clone(&self.rows);
        let files = Rc::clone(&self.files);
        let selected = self.selected_index();
        let root = self.root.clone();
        let list = uniform_list(
            "problems-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let line = div()
                            .id(index)
                            .debug_selector(move || format!("problems-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(selected == Some(index), |el| el.bg(selected_bg));
                        let line = match &rows[index] {
                            Row::File { file, expanded } => {
                                let problems = &files[*file];
                                let name = basename(&problems.path).into_owned();
                                let dir = relative_dir(&root, &problems.path).into_owned();
                                let errors =
                                    problems.items.iter().any(|item| item.diagnostic.severity == Severity::Error);
                                let file = *file;
                                let chevron = div()
                                    .id(SharedString::from(format!("problems-toggle-{index}")))
                                    .size(px(16.0))
                                    .flex_none()
                                    .child(theme::icon(
                                        cx,
                                        if *expanded { "chevron-down" } else { "chevron-right" },
                                        16.0,
                                        chevron_color,
                                    ))
                                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                        view.toggle_file(file, cx);
                                        cx.stop_propagation();
                                    }));
                                line.pl(px(4.0))
                                    .child(chevron)
                                    .child(theme::icon(cx, theme::icons::file_icon_name(&name), 16.0, muted))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(strong)
                                            .when(errors, |el| {
                                                el.underline().text_decoration_wavy().text_decoration_color(danger)
                                            })
                                            .child(SharedString::from(name)),
                                    )
                                    .child(text(&dir, subtle))
                                    .child(text(&summaries[file], subtle))
                            }
                            Row::Problem { file, item } => {
                                let problem = &files[*file].items[*item].diagnostic;
                                let source = problem.source.as_ref().map(|source| match &problem.code {
                                    Some(code) => format!("{source}({code})"),
                                    None => source.clone(),
                                });
                                line.pl(px(42.0))
                                    .child(theme::icon(
                                        cx,
                                        severity_icon(problem.severity),
                                        14.0,
                                        severity_color(cx, problem.severity),
                                    ))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_color(fg)
                                            .when(problem.deprecated, |el| el.line_through())
                                            .child(SharedString::from(first_line(&problem.message).to_owned())),
                                    )
                                    .child(text(&format!(":{}", problem.range.start.line + 1), subtle))
                                    .child(div().flex_1())
                                    .children(source.map(|source| text(&source, faint)))
                            }
                        };
                        line.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                            window.focus(&view.focus, cx);
                            view.select(index, cx);
                            if event.click_count() >= 2 {
                                view.open(index, cx);
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.show_menu(index, event.position, cx);
                                cx.stop_propagation();
                            }),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .debug_selector(|| "problems-tree".to_owned())
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(list)
            .into_any_element()
    }

    /// The lines around the selected problem, its line on the current-line colour and its range marked.
    fn preview_pane(&self, cx: &App) -> Div {
        let pane = div()
            .debug_selector(|| "problems-preview".to_owned())
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_l_1()
            .border_color(theme::color(cx, "border"))
            .text_size(theme::length(cx, "fs-code"));
        let Some((path, problem)) = self.selected_index().and_then(|index| self.target(index)) else {
            return pane;
        };
        let Some((_, lines)) = self.preview_lines.as_ref().filter(|(loaded, _)| loaded == path) else {
            return pane;
        };
        let range = problem.diagnostic.range;
        let line = range.start.line as usize;
        let from = line.saturating_sub(PREVIEW_CONTEXT);
        let to = (line + PREVIEW_CONTEXT + 1).min(lines.len());
        let gutter = theme::color(cx, "fg-line-number");
        let (current, mark) = (theme::color(cx, "bg-current-line"), theme::color(cx, "bg-text-search-result"));
        pane.children((from..to).map(|at| {
            let content = lines[at].as_str();
            let number = (at + 1).to_string();
            let number = div().w(px(40.0)).flex_none().text_color(gutter).child(SharedString::from(number));
            let row = div().flex().px(px(6.0)).whitespace_nowrap().child(number);
            if at != line {
                return row.child(SharedString::from(content.to_owned()));
            }
            let start = range.start.col;
            let end = if range.end.line == range.start.line { range.end.col } else { utf16::len(content) };
            let end = end.max(start + 1);
            row.bg(current)
                .child(div().child(SharedString::from(utf16::slice(content, 0, start).to_owned())))
                .child(div().bg(mark).child(SharedString::from(utf16::slice(content, start, end).to_owned())))
                .child(div().child(SharedString::from(utf16::slice(content, end, u32::MAX).to_owned())))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::problems::{OPTIONS_KEY, ProblemsStore};
    use gpui_kit::{Entity, Modifiers, MouseUpEvent, TestAppContext, VisualTestContext};
    use ide_core::{Pos, Range};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<ProblemsViewEvent>>>;

    fn d(line: u32, severity: Severity, message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(line, 2), Pos::new(line, 6)),
            severity,
            message: message.to_owned(),
            source: None,
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    /// `a.rs` with an error and a warning from rust-analyzer, `b.rs` with a warning.
    fn store() -> ProblemsStore {
        let mut store = ProblemsStore::default();
        store.apply(
            "/p/src/a.rs",
            "rust-analyzer",
            vec![d(3, Severity::Warning, "unused variable"), d(1, Severity::Error, "mismatched types")],
        );
        store.apply("/p/src/b.rs", "rust-analyzer", vec![d(0, Severity::Warning, "dead code")]);
        store
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        options: ProblemsOptions,
        current: Option<&str>,
    ) -> (Entity<ProblemsView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let current = current.map(str::to_owned);
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = ProblemsView::new(options, cx);
            view.set_project_root("/p", cx);
            view.set_problems(store().map(), cx);
            view.set_current_path(current, cx);
            view
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &ProblemsViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn lines(view: &Entity<ProblemsView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn opened(line: u32, focus: bool) -> ProblemsViewEvent {
        ProblemsViewEvent::Open { path: "/p/src/a.rs".to_owned(), line, col: 2, focus }
    }

    // Not from a spec: the File tab lists the current file's problems, errors first; Project Errors
    // lists every file, files with errors first; each tab counts.
    #[gpui_kit::test]
    async fn lists_the_current_files_problems_or_the_projects(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, ProblemsOptions::default(), Some("/p/src/a.rs"));
        assert_eq!(lines(&view, cx), ["a.rs", "  mismatched types :2", "  unused variable :4"]);
        let tabs = view.read_with(cx, |view, _| view.tab_labels(&I18n::default()));
        assert_eq!(
            tabs.map(|(_, label, count)| (label, count)),
            [("File a.rs".to_owned(), Some(2)), ("Project Errors".to_owned(), Some(1))]
        );
        click(cx, "problems-tab-project");
        assert_eq!(view.read_with(cx, |view, _| view.tab()), ProblemsTab::Project);
        let project = ["a.rs", "  mismatched types :2", "  unused variable :4", "b.rs", "  dead code :1"];
        assert_eq!(lines(&view, cx), project);
        click(cx, "problems-show-warnings");
        assert_eq!(lines(&view, cx), ["a.rs", "  mismatched types :2"]);
        click(cx, "problems-show-errors");
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No problems found"));
        assert!(cx.debug_bounds("problems-empty").is_some());
    }

    // Not from a spec: what the empty File tab says.
    #[gpui_kit::test]
    async fn says_when_no_file_is_open_or_it_has_no_problems(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, ProblemsOptions::default(), None);
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No file is open"));
        view.update(cx, |view, cx| view.set_current_path(Some("/p/src/c.rs".to_owned()), cx));
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No problems in this file"));
        let tabs = view.read_with(cx, |view, _| view.tab_labels(&I18n::default()));
        assert_eq!(tabs[0].2, None);
    }

    // Not from a spec: the keyboard moves, opens and closes files, opens a problem (Enter, F4), shows
    // its quick fixes (⌥Enter) and copies its description (⌘C).
    #[gpui_kit::test]
    async fn opens_problems_from_the_keyboard(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, ProblemsOptions::default(), Some("/p/src/a.rs"));
        cx.simulate_keystrokes("down down");
        let selected = |view: &Entity<ProblemsView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| view.selected().map(str::to_owned))
        };
        assert_eq!(selected(&view, cx).as_deref(), Some("/p/src/a.rs\u{0}0"));
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&opened(1, true)));
        cx.simulate_keystrokes("down f4");
        assert_eq!(events.borrow().last(), Some(&opened(3, true)));
        cx.simulate_keystrokes("alt-enter");
        let fixes = ProblemsViewEvent::ShowQuickFixes { path: "/p/src/a.rs".to_owned(), line: 3, col: 2 };
        assert_eq!(events.borrow().last(), Some(&fixes));
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("unused variable (rust-analyzer)"));
        cx.simulate_keystrokes("left");
        assert_eq!(selected(&view, cx).as_deref(), Some("/p/src/a.rs"));
        cx.simulate_keystrokes("left");
        assert_eq!(lines(&view, cx), ["a.rs"]);
        cx.simulate_keystrokes("right");
        assert_eq!(lines(&view, cx).len(), 3);
    }

    // Not from a spec: a double click opens, a right click asks for the context menu, whose rows the
    // view carries out.
    #[gpui_kit::test]
    async fn opens_on_double_click_and_offers_a_context_menu(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, ProblemsOptions::default(), Some("/p/src/a.rs"));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("problems-row-1").expect("the first problem is drawn");
        let (position, modifiers, button) = (bounds.center(), Modifiers::default(), MouseButton::Left);
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
        cx.run_until_parked();
        assert!(events.borrow().contains(&opened(1, true)));
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let menu = events.borrow().iter().rev().find_map(|event| match event {
            ProblemsViewEvent::ShowMenu { items, .. } => Some(items.clone()),
            _ => None,
        });
        let menu = menu.expect("a context menu is asked for");
        let labels: Vec<Option<String>> = menu
            .iter()
            .map(|item| match item {
                MenuItem::Item(entry) => Some(entry.label.clone()),
                MenuItem::Separator | MenuItem::Action(_) => None,
            })
            .collect();
        assert_eq!(
            labels[..4],
            [
                Some("Show Quick Fixes".to_owned()),
                Some("Jump to Source".to_owned()),
                None,
                Some("Copy Problem Description".to_owned())
            ]
        );
        assert!(labels.contains(&Some("Disable Inspection 'rust-analyzer'".to_owned())));
        let copy = menu[3].command().cloned().expect("a problem can be copied");
        view.update(cx, |view, cx| view.run_command(copy, cx));
        assert_eq!(clipboard(cx).as_deref(), Some("mismatched types (rust-analyzer)"));
        let file_menu = view.read_with(cx, |view, _| view.menu(0, &mut I18n::default()));
        assert!(file_menu[3].command().is_none(), "a file has no description to copy");
    }

    // Not from a spec: the ⋮ menu's options are kept, Autoscroll to Source opens what is selected
    // without the keyboard, and Next Occurrence steps through the problems.
    #[gpui_kit::test]
    async fn options_are_kept_and_autoscroll_and_occurrences_open_problems(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, ProblemsOptions::default(), Some("/p/src/a.rs"));
        view.update(cx, |view, cx| view.toggle_option("autoscroll", cx));
        assert!(view.read_with(cx, |view, _| view.options().autoscroll));
        let stored = events.borrow().last().cloned();
        assert!(matches!(stored, Some(ProblemsViewEvent::Store(write)) if write.key == OPTIONS_KEY));
        cx.simulate_keystrokes("down down");
        assert_eq!(events.borrow().last(), Some(&opened(1, false)));
        let step = view.update(cx, |view, cx| view.step_occurrence(Direction::Next, true, cx));
        assert_eq!(step, Some(OccurrenceStep { index: 2, total: 2 }));
        assert_eq!(events.borrow().last(), Some(&opened(3, true)));
        assert_eq!(view.update(cx, |view, cx| view.step_occurrence(Direction::Next, true, cx)), None);
        let menu = view.read_with(cx, |view, _| view.header_menu());
        let autoscroll = HeaderMenuItem::Check { id: "autoscroll", label: "Autoscroll to Source", checked: true };
        assert!(menu.contains(&autoscroll));
    }
}
