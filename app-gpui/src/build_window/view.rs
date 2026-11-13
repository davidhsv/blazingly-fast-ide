//! The Build tool window and the main toolbar's Build Project hammer.
//!
//! [`BuildView`] shows the build → file → message tree on the left and, on the right, the build's
//! console, or the full text and location of the selected message. Selecting a row shows it;
//! Enter, F4 and a double click open its source. The toolbar reruns the last build and stops the
//! running one. [`BuildTitleItem`] is the hammer left of the run widget.
//!
//! Both read the [`BuildRun`] the workbench owns. The tree's selection is kept there (a failed build
//! selects its first error); everything else the views need done is an event.

use crate::build_window::model::{
    BuildEffect, BuildNode, BuildRun, BuildStatus, ConsoleLine, LineKind, file_children, file_counts, first_located,
    message_path, navigate_to, root_children, root_detail, title_item_shown,
};
use crate::everywhere::open::byte_at_utf16;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::virtual_docs::{basename, relative_path};
use crate::run::links::{FileTarget, OutputLink, find_output_links};
use crate::run::navigate::run_directory;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Font, FontStyle, Hsla,
    InteractiveText, IntoElement, Keystroke, Render, ScrollStrategy, SharedString, Stateful, StyledText, Subscription,
    TextRun, UnderlineStyle, UniformListScrollHandle, Window, div, font, prelude::*, px, uniform_list,
};
use ide_core::Severity;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

/// How far each level of the tree is indented, in pixels.
const INDENT: f32 = 16.0;
/// Height of a console line, in pixels.
const LINE_HEIGHT: f32 = 20.0;
/// Width of the tree beside the console.
const TREE_WIDTH: f32 = 380.0;
/// The action the hammer runs.
pub const BUILD_PROJECT_ACTION: &str = "build.buildProject";

/// One visible row of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRow {
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
    pub node: BuildNode,
}

/// A toolbar button with its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub enabled: bool,
}

/// What the Build tool window asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildViewEvent {
    /// Build again what was built last ([`BuildRun::rerun_target`]).
    Rerun,
    /// Stop the running build ([`BuildRun::stop`]).
    Stop,
    /// Open `path` at the 0-based line and UTF-16 column, giving the editor the keyboard when `focus`.
    Navigate {
        path: String,
        line: Option<u32>,
        col: Option<u32>,
        focus: bool,
    },
    OpenUrl(String),
    /// Open a file location printed by a build step that ran in `cwd`.
    OpenLocation {
        target: FileTarget,
        cwd: String,
    },
}

/// The icon of a severity.
fn severity_icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info | Severity::Hint => "info",
    }
}

/// The colour token of a severity.
fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "danger",
        Severity::Warning => "warning",
        Severity::Info | Severity::Hint => "fg-muted",
    }
}

/// The folder of `path` relative to the project at `root`.
fn relative_dir(root: Option<&str>, path: &str) -> String {
    let rel = match root.filter(|root| !root.is_empty()) {
        Some(root) => relative_path(root, path),
        None => path.into(),
    };
    let name = basename(&rel);
    rel.strip_suffix(&*name).map_or("", |dir| dir.strip_suffix('/').unwrap_or(dir)).to_owned()
}

/// `:12:5`, the 1-based line and column of a message.
fn position(node: &BuildNode) -> Option<String> {
    let BuildNode::Message { message, .. } = node else { return None };
    message.range.map(|range| format!(":{}:{}", range.start.line + 1, range.start.col + 1))
}

/// The first line of a message's text.
fn first_line(text: &str) -> &str {
    text.split('\n').next().unwrap_or_default()
}

/// The first family of the theme's `--font-mono` list.
fn mono_family(cx: &App) -> SharedString {
    let family = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
        .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
    SharedString::from(family)
}

pub struct BuildView {
    focus: FocusHandle,
    run: Entity<BuildRun>,
    /// Keys of the collapsed rows; the build and its files start expanded.
    collapsed: HashSet<String>,
    /// The selection the tree last scrolled to.
    shown: Option<String>,
    project_root: Option<String>,
    tree_scroll: UniformListScrollHandle,
    console_scroll: UniformListScrollHandle,
    key_claim: u64,
    _observe: Subscription,
}

impl EventEmitter<BuildViewEvent> for BuildView {}

impl BuildView {
    pub fn new(run: Entity<BuildRun>, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let observe = cx.observe(&run, |view, _, cx| view.run_changed(cx));
        let mut view = Self {
            focus,
            run,
            collapsed: HashSet::new(),
            shown: None,
            project_root: None,
            tree_scroll: UniformListScrollHandle::new(),
            console_scroll: UniformListScrollHandle::new(),
            key_claim,
            _observe: observe,
        };
        view.run_changed(cx);
        view
    }

    /// The project's folder: file and message locations show relative to it, and console links of a
    /// step without a directory resolve against it.
    pub fn set_project_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.project_root = root;
        cx.notify();
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    /// The visible rows: the build, its files with their messages, then the messages about the build.
    pub fn rows(&self, cx: &App) -> Vec<BuildRow> {
        let run = self.run.read(cx);
        let Some(root) = run.root() else { return Vec::new() };
        let open = |key: &str| !self.collapsed.contains(key);
        let root_open = open(root.key());
        let mut rows = vec![BuildRow { depth: 0, expandable: true, expanded: root_open, node: root }];
        if !root_open {
            return rows;
        }
        for child in root_children(&run.messages) {
            let children = match &child {
                BuildNode::File { key, messages, .. } => Some(open(key).then(|| file_children(key, messages))),
                _ => None,
            };
            let (expandable, expanded) = (children.is_some(), children.as_ref().is_some_and(Option::is_some));
            rows.push(BuildRow { depth: 1, expandable, expanded, node: child });
            let leaves = children.flatten().unwrap_or_default();
            rows.extend(leaves.into_iter().map(|node| BuildRow { depth: 2, expandable: false, expanded: false, node }));
        }
        rows
    }

    /// The rows as text, indented two spaces per level: the build's title and state; a file's name,
    /// folder and counts; a message's first line and position.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let run = self.run.read(cx);
        self.rows(cx)
            .into_iter()
            .map(|row| {
                let indent = "  ".repeat(row.depth);
                match &row.node {
                    BuildNode::Root { title, status, errors, warnings } => {
                        format!("{title}  {}", root_detail(*status, run.duration_ms, *errors, *warnings))
                    }
                    BuildNode::File { path, messages, .. } => {
                        let dir = relative_dir(self.project_root.as_deref(), path);
                        let parts = [basename(path).into_owned(), dir, file_counts(messages)];
                        let parts: Vec<String> = parts.into_iter().filter(|part| !part.is_empty()).collect();
                        format!("{indent}{}", parts.join("  "))
                    }
                    BuildNode::Message { message, .. } => {
                        let at = position(&row.node).map(|at| format!(" {at}")).unwrap_or_default();
                        format!("{indent}{}{at}", first_line(&message.text))
                    }
                }
            })
            .collect()
    }

    /// The full text of the selected message, shown in place of the console.
    pub fn detail_text(&self, cx: &App) -> Option<String> {
        match self.run.read(cx).selected_node()? {
            BuildNode::Message { message, .. } => Some(message.text),
            _ => None,
        }
    }

    /// The console's lines.
    pub fn console_lines(&self, cx: &App) -> Vec<String> {
        self.run.read(cx).lines.iter().map(|line| line.text.clone()).collect()
    }

    /// The toolbar: Rerun (greyed while a build runs) and Stop (only while one runs).
    pub fn toolbar(&self, cx: &App) -> Vec<ToolButton> {
        let running = self.run.read(cx).is_running();
        vec![
            ToolButton { id: "build-rerun", icon: "rerun", title: "Rebuild Last", enabled: !running },
            ToolButton { id: "build-stop", icon: "stop", title: "Stop Build", enabled: running },
        ]
    }

    // -----------------------------------------------------------------------------------------------
    // Interaction

    /// The build changed: a selection the model made (the first error of a failed build) opens its
    /// file and is scrolled to; the console follows the output.
    fn run_changed(&mut self, cx: &mut Context<Self>) {
        let (selected, lines) = {
            let run = self.run.read(cx);
            (run.selected.clone(), run.lines.len())
        };
        if selected != self.shown {
            if let Some((file, _)) = selected.as_deref().and_then(|key| key.split_once('\u{0}')) {
                self.collapsed.remove(file);
            }
            if let Some(index) = selected.as_deref().and_then(|key| self.index_of(key, cx)) {
                self.tree_scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            }
            self.shown = selected;
        }
        if let Some(last) = lines.checked_sub(1) {
            self.console_scroll.scroll_to_item(last, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    fn index_of(&self, key: &str, cx: &App) -> Option<usize> {
        self.rows(cx).iter().position(|row| row.node.key() == key)
    }

    /// Selects the row `key`: a message shows its text beside the tree.
    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        let key = key.to_owned();
        self.run.update(cx, |run, cx| {
            run.select(Some(key));
            cx.notify();
        });
    }

    /// Enter, F4 and a double click: opens the source of a message, or of a file's first message.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(row) = rows.iter().find(|row| row.node.key() == key) else { return };
        let effect = first_located(&row.node).and_then(|message| navigate_to(message, true));
        if let Some(BuildEffect::Navigate { path, line, col, focus }) = effect {
            cx.emit(BuildViewEvent::Navigate { path, line, col, focus });
        }
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "build-rerun" => cx.emit(BuildViewEvent::Rerun),
            "build-stop" => cx.emit(BuildViewEvent::Stop),
            _ => {}
        }
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows(cx).into_iter().nth(index) else { return };
        self.select(row.node.key(), cx);
        self.tree_scroll.scroll_to_item(index, ScrollStrategy::Nearest);
    }

    /// The keys the tree handles while it has the keyboard, ahead of the keymap (F4 opens sources
    /// elsewhere too).
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let rows = self.rows(cx);
        let Some(last) = rows.len().checked_sub(1) else { return false };
        let selected = self.run.read(cx).selected.clone();
        let at = selected.as_deref().and_then(|key| rows.iter().position(|row| row.node.key() == key));
        match (keystroke.key.as_str(), at) {
            ("down", None) | ("home", _) => self.select_index(0, cx),
            ("down", Some(at)) => self.select_index((at + 1).min(last), cx),
            ("up", None) | ("end", _) => self.select_index(last, cx),
            ("up", Some(at)) => self.select_index(at.saturating_sub(1), cx),
            ("right", Some(at)) if rows[at].expandable && !rows[at].expanded => self.toggle(rows[at].node.key(), cx),
            ("right", Some(at)) if rows[at].expanded => self.select_index(at + 1, cx),
            ("left", Some(at)) if rows[at].expanded => self.toggle(rows[at].node.key(), cx),
            ("left", Some(at)) => {
                if let Some(parent) = rows[..at].iter().rposition(|above| above.depth < rows[at].depth) {
                    self.select_index(parent, cx);
                }
            }
            ("enter" | "f4", Some(at)) => self.activate(rows[at].node.key(), cx),
            _ => return false,
        }
        true
    }

    /// A link in the console was clicked.
    fn open_link(&mut self, link: &OutputLink, cwd: Option<&str>, cx: &mut Context<Self>) {
        match link {
            OutputLink::Url { url, .. } => cx.emit(BuildViewEvent::OpenUrl(url.clone())),
            OutputLink::File { target, .. } => {
                let cwd = cwd.map_or_else(|| run_directory(self.project_root.as_deref(), None), str::to_owned);
                cx.emit(BuildViewEvent::OpenLocation { target: target.clone(), cwd });
            }
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div()
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        for button in self.toolbar(cx) {
            let color = theme::color(cx, if button.enabled { "fg-muted" } else { "fg-faint" });
            let id = button.id;
            bar = bar.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_owned())
                    // Stop names the action it runs, with its shortcut; Rebuild Last runs none.
                    .map(|el| match id {
                        "build-stop" => el.with_tooltip_shortcut(button.title, "build.stop"),
                        _ => el.with_tooltip(button.title),
                    })
                    .size(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .when(button.enabled, |el| {
                        el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(id, cx)))
                    })
                    .child(theme::icon(cx, button.icon, 16.0, color)),
            );
        }
        bar
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let run = self.run.read(cx);
        let (duration, selected) = (run.duration_ms, run.selected.clone());
        let rows = Rc::new(self.rows(cx));
        let root = self.project_root.clone();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, strong, muted, danger, chevron_color) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "danger"),
            theme::color(cx, "fg-icon"),
        );
        let list = uniform_list(
            "build-tree",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let key = row.node.key().to_owned();
                        let is_selected = selected.as_deref() == Some(key.as_str());
                        let chevron = if row.expandable {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let toggled = key.clone();
                            div()
                                .id("toggle")
                                .debug_selector(move || format!("build-toggle-{index}"))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron_color))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let line = div()
                            .id(index)
                            .debug_selector(move || format!("build-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + INDENT * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(is_selected, |el| el.bg(selected_bg))
                            .child(chevron);
                        let line =
                            match &row.node {
                                BuildNode::Root { title, status, errors, warnings } => {
                                    let (icon, color) = match status {
                                        BuildStatus::Running => ("refresh", muted),
                                        BuildStatus::Success => ("success", theme::color(cx, "success")),
                                        BuildStatus::Failed => ("error", danger),
                                        BuildStatus::Cancelled => ("warning", theme::color(cx, "warning")),
                                    };
                                    let detail = root_detail(*status, duration, *errors, *warnings);
                                    let detail = cx.default_global::<Localization>().0.tx_template(&detail);
                                    line.child(theme::icon(cx, icon, 14.0, color))
                                        .child(div().text_color(strong).child(SharedString::from(title.clone())))
                                        .child(div().text_color(muted).child(SharedString::from(detail)))
                                }
                                BuildNode::File { path, messages, .. } => {
                                    let name = basename(path).into_owned();
                                    let failed = messages.iter().any(|message| message.severity == Severity::Error);
                                    let name_color = if failed { danger } else { strong };
                                    line.child(theme::icon(cx, theme::icons::file_icon_name(&name), 16.0, fg))
                                        .child(div().text_color(name_color).child(SharedString::from(name)))
                                        .child(
                                            div()
                                                .text_color(muted)
                                                .child(SharedString::from(relative_dir(root.as_deref(), path))),
                                        )
                                        .child(div().text_color(muted).child(SharedString::from(file_counts(messages))))
                                }
                                BuildNode::Message { message, .. } => {
                                    let color = theme::color(cx, severity_color(message.severity));
                                    line.child(theme::icon(cx, severity_icon(message.severity), 14.0, color))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .overflow_hidden()
                                                .text_color(fg)
                                                .child(SharedString::from(first_line(&message.text).to_owned())),
                                        )
                                        .children(position(&row.node).map(|at| {
                                            div().flex_none().text_color(muted).child(SharedString::from(at))
                                        }))
                                }
                            };
                        let clicked = key;
                        line.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                            window.focus(&view.focus, cx);
                            view.select(&clicked, cx);
                            if event.click_count() >= 2 {
                                view.activate(&clicked, cx);
                            }
                        }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.tree_scroll)
        .size_full();
        div()
            .id("build-tree-area")
            .debug_selector(|| "build-tree".to_owned())
            .w(px(TREE_WIDTH))
            .min_w(px(120.0))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(list)
    }

    /// One console line, its file locations and URLs as links.
    fn console_line(line: &ConsoleLine, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let links = if line.kind == LineKind::System { Vec::new() } else { find_output_links(&line.text) };
        let base = font(mono_family(cx));
        let base = if line.kind == LineKind::System { Font { style: FontStyle::Italic, ..base } } else { base };
        let stream: Hsla = theme::color(
            cx,
            match line.kind {
                LineKind::Out => "fg",
                LineKind::Err => "danger",
                LineKind::System => "fg-subtle",
            },
        );
        let link_color = theme::color(cx, "link");
        let run = |len: usize, link: bool| TextRun {
            len,
            font: base.clone(),
            color: if link { link_color } else { stream },
            background_color: None,
            underline: link.then_some(UnderlineStyle { thickness: px(1.0), color: None, wavy: false }),
            strikethrough: None,
        };
        let text = if line.text.is_empty() { " ".to_owned() } else { line.text.clone() };
        let mut runs = Vec::new();
        let mut ranges = Vec::new();
        let mut kept = Vec::new();
        let mut at = 0;
        for link in links {
            let (start, end) = (byte_at_utf16(&text, link.start() as usize), byte_at_utf16(&text, link.end() as usize));
            if start < at || end <= start {
                continue;
            }
            if start > at {
                runs.push(run(start - at, false));
            }
            runs.push(run(end - start, true));
            ranges.push(start..end);
            kept.push(link);
            at = end;
        }
        if at < text.len() {
            runs.push(run(text.len() - at, false));
        }
        let weak = cx.weak_entity();
        let cwd = line.cwd.clone();
        let styled = StyledText::new(text).with_runs(runs);
        let interactive = InteractiveText::new(SharedString::from(format!("build-text-{index}")), styled).on_click(
            ranges,
            move |clicked, _, cx| {
                if let Some(link) = kept.get(clicked) {
                    let _ = weak.update(cx, |view, cx| view.open_link(link, cwd.as_deref(), cx));
                }
            },
        );
        div()
            .id(index)
            .w_full()
            .h(px(LINE_HEIGHT))
            .px(px(12.0))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .overflow_hidden()
            .child(interactive)
    }

    fn render_output(&self, cx: &mut Context<Self>) -> Div {
        let pane = div().flex_1().min_w(px(120.0)).h_full();
        if let Some(BuildNode::Message { message, .. }) = self.run.read(cx).selected_node() {
            let location = message_path(&message).map(|path| {
                let dir = relative_dir(self.project_root.as_deref(), &path);
                let at = position(&BuildNode::Message { key: String::new(), message: message.clone() });
                let name = basename(&path).into_owned();
                let shown = if dir.is_empty() { name } else { format!("{dir}/{name}") };
                format!("{shown}{}", at.unwrap_or_default())
            });
            let text_color = theme::color(cx, if message.severity == Severity::Error { "danger" } else { "fg" });
            let navigate = navigate_to(&message, true);
            return pane
                .debug_selector(|| "build-message-detail".to_owned())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(12.0))
                .children(location.map(|location| {
                    div()
                        .id("build-message-location")
                        .debug_selector(|| "build-message-location".to_owned())
                        .text_color(theme::color(cx, "link"))
                        .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                        .child(SharedString::from(location))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            if let Some(BuildEffect::Navigate { path, line, col, focus }) = navigate.clone() {
                                cx.emit(BuildViewEvent::Navigate { path, line, col, focus });
                            }
                        }))
                }))
                .child(
                    div().font_family(mono_family(cx)).text_color(text_color).child(SharedString::from(message.text)),
                );
        }
        let lines: Rc<Vec<ConsoleLine>> = Rc::new(self.run.read(cx).lines.iter().cloned().collect());
        let list = uniform_list(
            "build-console",
            lines.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range.map(|index| Self::console_line(&lines[index], index, cx)).collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.console_scroll)
        .size_full();
        pane.debug_selector(|| "build-console".to_owned())
            .bg(theme::color(cx, "bg-editor"))
            .text_size(theme::length(cx, "fs-code"))
            .child(list)
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let i18n = &cx.default_global::<Localization>().0;
        let (title, hint) = (i18n.tx("No build output"), i18n.tx("Build the project to see compiler messages here"));
        div()
            .debug_selector(|| "build-empty".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(title)))
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(hint)))
    }
}

impl Focusable for BuildView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BuildView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        if self.run.read(cx).status.is_none() {
            return root.child(self.render_empty(cx));
        }
        let toolbar = self.render_toolbar(cx);
        let tree = self.render_tree(window, cx);
        let output = self.render_output(cx);
        root.child(toolbar).child(div().flex_1().min_h_0().flex().child(tree).child(output))
    }
}

/// What the hammer depends on besides the build: the context keys `run.cmakeBuild`, `build.maven`,
/// `build.gradle` and `run.selectedKind`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleItemContext {
    /// A CMake target is selected to run.
    pub cmake_build: bool,
    pub maven: bool,
    pub gradle: bool,
    /// The kind of the selected run configuration.
    pub selected_kind: String,
}

/// What the hammer asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildTitleEvent {
    /// Run this action ([`BUILD_PROJECT_ACTION`]).
    RunAction(&'static str),
}

/// The main toolbar's Build Project hammer: shown for compiled configurations, greyed while a build
/// runs.
pub struct BuildTitleItem {
    run: Entity<BuildRun>,
    context: TitleItemContext,
    _observe: Subscription,
}

impl EventEmitter<BuildTitleEvent> for BuildTitleItem {}

impl BuildTitleItem {
    pub fn new(run: Entity<BuildRun>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&run, |_, _, cx| cx.notify());
        Self { run, context: TitleItemContext::default(), _observe: observe }
    }

    /// The context keys changed.
    pub fn set_context(&mut self, context: TitleItemContext, cx: &mut Context<Self>) {
        if context != self.context {
            self.context = context;
            cx.notify();
        }
    }

    pub fn is_shown(&self) -> bool {
        let context = &self.context;
        title_item_shown(context.cmake_build, context.maven, context.gradle, &context.selected_kind)
    }

    pub fn is_enabled(&self, cx: &App) -> bool {
        !self.run.read(cx).is_running()
    }
}

impl Render for BuildTitleItem {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_shown() {
            return div();
        }
        let enabled = self.is_enabled(cx);
        let color = theme::color(cx, if enabled { "fg" } else { "fg-faint" });
        div().child(
            div()
                .id("build-title-item")
                .debug_selector(|| "build-title-item".to_owned())
                .size(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(enabled, |el| {
                    el.hover(|el| el.bg(theme::color(cx, "bg-hover"))).on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| {
                            cx.emit(BuildTitleEvent::RunAction(BUILD_PROJECT_ACTION))
                        }),
                    )
                })
                .child(theme::icon(cx, "build-project", 16.0, color)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_window::model::BuildRun;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_build::compile::{BuildEvent, BuildKind, BuildMessage, BuildResult, BuildSystem};
    use ide_core::{IpcPath, Pos, RequestId};
    use std::cell::RefCell;
    use std::path::PathBuf;

    type Events = Rc<RefCell<Vec<BuildViewEvent>>>;

    const REQUEST: RequestId = RequestId(1);

    fn err(path: &str, line: u32, text: &str) -> BuildMessage {
        BuildMessage {
            severity: Severity::Error,
            text: text.to_owned(),
            path: Some(IpcPath(PathBuf::from(path))),
            range: Some(ide_core::Range::new(Pos::new(line, 4), Pos::new(line, 4))),
            code: None,
        }
    }

    fn warn(path: &str, line: u32) -> BuildMessage {
        BuildMessage { severity: Severity::Warning, ..err(path, line, "deprecated") }
    }

    fn result(messages: Vec<BuildMessage>, success: bool) -> BuildResult {
        let count = |severity: Severity| messages.iter().filter(|m| m.severity == severity).count() as u32;
        BuildResult {
            system: BuildSystem::Maven,
            title: "demo".to_owned(),
            success,
            errors: count(Severity::Error),
            warnings: count(Severity::Warning),
            duration_ms: 3000.0,
            messages,
        }
    }

    fn open(cx: &mut TestAppContext) -> (Entity<BuildRun>, Entity<BuildView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let run = cx.update(|cx| cx.new(|_| BuildRun::default()));
        let model = run.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| BuildView::new(model, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &BuildViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            view.update(cx, |view, cx| view.set_project_root(Some("/p".to_owned()), cx));
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (run, view, events, cx)
    }

    /// Changes the build as the workbench does, and lets the window follow.
    fn change(run: &Entity<BuildRun>, cx: &mut VisualTestContext, change: impl FnOnce(&mut BuildRun)) {
        run.update(cx, |run, cx| {
            change(run);
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    // Not from a spec: the header's buttons say what they do when the pointer rests on them; Stop Build
    // with the shortcut of its action (BuildView.tsx's IconButtons).
    #[gpui_kit::test]
    async fn the_header_buttons_show_their_tooltips(cx: &mut TestAppContext) {
        use crate::ui::tooltip::hovered_tooltip;
        let (run, _view, _, cx) = open(cx);
        change(&run, cx, |run| {
            run.start(BuildKind::Build, None, true, REQUEST, true);
            run.apply(REQUEST, BuildEvent::Started { system: BuildSystem::Maven, title: "demo".to_owned() });
        });
        assert!(hovered_tooltip(cx, "build-rerun", "Rebuild Last", None));
        assert!(hovered_tooltip(cx, "build-stop", "Stop Build", Some("build.stop")));
    }

    #[gpui_kit::test]
    async fn streams_a_failing_build_into_the_tree_and_console(cx: &mut TestAppContext) {
        let (run, view, events, cx) = open(cx);
        assert!(cx.debug_bounds("build-empty").is_some(), "No build output");
        let messages = vec![err("/p/src/A.java", 11, "cannot find symbol"), warn("/p/src/B.java", 2)];
        let first = messages[0].clone();
        change(&run, cx, |run| {
            run.start(BuildKind::Build, None, true, REQUEST, true);
            run.apply(REQUEST, BuildEvent::Started { system: BuildSystem::Maven, title: "demo".to_owned() });
            let text = "[ERROR] /p/src/A.java:[12,5] boom\n".to_owned();
            run.apply(REQUEST, BuildEvent::Output { text, stderr: false });
            run.apply(REQUEST, BuildEvent::Message { message: first });
        });
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            ["demo  running…", "  A.java  src  1 error", "    cannot find symbol :12:5"]
        );
        assert!(cx.debug_bounds("build-console").is_some());
        assert_eq!(view.read_with(cx, |view, cx| view.console_lines(cx)), ["[ERROR] /p/src/A.java:[12,5] boom"]);
        let enabled: Vec<bool> = view.read_with(cx, |view, cx| view.toolbar(cx)).iter().map(|b| b.enabled).collect();
        assert_eq!(enabled, [false, true], "Rerun waits, Stop works while the build runs");
        click(cx, "build-stop");
        assert_eq!(events.borrow().last(), Some(&BuildViewEvent::Stop));
        change(&run, cx, |run| {
            run.finish(REQUEST, result(messages, false));
        });
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "demo  1 error, 1 warning",
                "  A.java  src  1 error",
                "    cannot find symbol :12:5",
                "  B.java  src  1 warning",
                "    deprecated :3:5",
            ]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.detail_text(cx)).as_deref(), Some("cannot find symbol"));
        assert!(cx.debug_bounds("build-message-detail").is_some());
        click(cx, "build-rerun");
        assert_eq!(events.borrow().last(), Some(&BuildViewEvent::Rerun));
    }

    #[gpui_kit::test]
    async fn shows_a_message_on_single_click_and_navigates_on_f4(cx: &mut TestAppContext) {
        let (run, view, events, cx) = open(cx);
        let messages = vec![err("/p/src/A.java", 11, "first problem"), warn("/p/src/B.java", 2)];
        change(&run, cx, |run| {
            run.start(BuildKind::Build, None, true, REQUEST, true);
            run.finish(REQUEST, result(messages, true));
        });
        assert!(cx.debug_bounds("build-console").is_some());
        click(cx, "build-row-4");
        assert!(events.borrow().is_empty(), "a click only shows the message");
        assert_eq!(view.read_with(cx, |view, cx| view.detail_text(cx)).as_deref(), Some("deprecated"));
        assert!(cx.debug_bounds("build-message-detail").is_some());
        cx.simulate_keystrokes("f4");
        let navigate =
            BuildViewEvent::Navigate { path: "/p/src/B.java".to_owned(), line: Some(2), col: Some(4), focus: true };
        assert_eq!(events.borrow().last(), Some(&navigate));
        cx.simulate_keystrokes("up");
        assert!(cx.debug_bounds("build-console").is_some(), "a file row shows the console");
        click(cx, "build-toggle-1");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)).len(), 4);
    }

    #[gpui_kit::test]
    async fn shows_the_hammer_for_cmake_and_builds_through_the_shared_action(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let run = cx.update(|cx| cx.new(|_| BuildRun::default()));
        let model = run.clone();
        let (item, cx) = cx.add_window_view(move |_, cx| BuildTitleItem::new(model, cx));
        let events: Rc<RefCell<Vec<BuildTitleEvent>>> = Rc::default();
        let log = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&item, move |_, event: &BuildTitleEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        assert!(cx.debug_bounds("build-title-item").is_none());
        let context = TitleItemContext { cmake_build: true, ..TitleItemContext::default() };
        item.update(cx, |item, cx| item.set_context(context, cx));
        click(cx, "build-title-item");
        assert_eq!(*events.borrow(), [BuildTitleEvent::RunAction(BUILD_PROJECT_ACTION)]);
        run.update(cx, |run, cx| {
            run.start(BuildKind::Build, None, true, REQUEST, true);
            cx.notify();
        });
        click(cx, "build-title-item");
        assert_eq!(events.borrow().len(), 1, "greyed while a build runs");
    }
}
