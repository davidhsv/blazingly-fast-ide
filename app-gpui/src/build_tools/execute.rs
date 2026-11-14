//! Execute Maven Goal / Execute Gradle Task (IDEA's Run Anything for one build tool): a command line
//! with completion of lifecycle phases and plugin goals (Maven) or task names and paths (Gradle),
//! after the recent command lines. Tab or a click accepts a completion; Enter runs the line in the
//! project chosen in the footer.
//!
//! [`ExecuteState`] is the popup's logic; [`ExecutePopup`] draws it around a text field and asks
//! the workbench to run a line ([`ExecuteEvent::Execute`]).

use super::model::{
    BuildSettings, Completion, goal_completions, gradle_project_for_key, gradle_project_key, gradle_task_config,
    is_launcher, maven_goal_config, parse_goal_line, parse_task_line, task_completions,
};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_build::{BuildModel, BuildTool};
use ide_run::RunConfig;

/// How many completions the popup lists.
const COMPLETION_LIMIT: usize = 200;
/// How many rows Page Up / Page Down move.
const PAGE_ROWS: usize = 10;
/// The popup's width.
const WIDTH: f32 = 560.0;

/// A line of the completion list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecuteRow {
    /// "Recent", "Maven Goals" or "Gradle Tasks": never selected.
    Section {
        key: &'static str,
        label: &'static str,
    },
    Completion(Completion),
}

impl ExecuteRow {
    fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }
}

/// A project the line can run in: its `pom` or Gradle project key, its name and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectChoice {
    pub value: String,
    pub label: String,
    pub detail: String,
}

/// The projects of `tool` in `model`.
pub fn project_choices(tool: BuildTool, model: &BuildModel) -> Vec<ProjectChoice> {
    match tool {
        BuildTool::Maven => model
            .maven
            .iter()
            .flat_map(|m| &m.projects)
            .map(|p| ProjectChoice {
                value: p.pom.clone(),
                label: p.display_name.clone(),
                detail: if p.dir.is_empty() { ".".to_owned() } else { p.dir.clone() },
            })
            .collect(),
        BuildTool::Gradle => model
            .gradle
            .iter()
            .flat_map(|build| {
                build.projects.iter().map(move |p| ProjectChoice {
                    value: gradle_project_key(&build.dir, &p.path),
                    label: p.name.clone(),
                    detail: format!("{} {}", if build.dir.is_empty() { "." } else { build.dir.as_str() }, p.path),
                })
            })
            .collect(),
    }
}

/// A key that moves the selection of the completion list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Down,
    Up,
    PageDown,
    PageUp,
}

/// The index `key` moves `active` to among the rows that are not sections, `wrap`ping at the ends
/// for the arrows; `None` when the list is empty.
fn navigate(rows: &[ExecuteRow], active: Option<usize>, key: Move) -> Option<Option<usize>> {
    let n = rows.len();
    if n == 0 {
        return None;
    }
    let scan = |from: isize, step: isize| -> Option<usize> {
        let mut at = from;
        while at >= 0 && (at as usize) < n {
            if !rows[at as usize].is_section() {
                return Some(at as usize);
            }
            at += step;
        }
        None
    };
    let current = active.map_or(-1, |at| at as isize);
    let last = n as isize - 1;
    let page = PAGE_ROWS as isize;
    let next = match key {
        Move::Down => scan(current + 1, 1).or_else(|| scan(0, 1)),
        Move::Up => {
            let from = if current < 0 { last } else { current - 1 };
            scan(from, -1).or_else(|| scan(last, -1))
        }
        Move::PageDown => match scan(last.min(current + page), -1) {
            Some(at) if at as isize > current => Some(at),
            _ => scan(current + 1, 1),
        },
        Move::PageUp => match scan((current - page).max(0), 1) {
            Some(at) if (at as isize) < current => Some(at),
            _ => scan(current - 1, -1),
        },
    };
    Some(next.or(active))
}

/// What the popup holds: the line, the selected completion, the history and the chosen project.
#[derive(Clone, Debug)]
pub struct ExecuteState {
    tool: BuildTool,
    line: String,
    active: Option<usize>,
    history: Vec<String>,
    project: Option<String>,
}

impl ExecuteState {
    /// The popup over `model`, in `current` when it is one of its projects, else in the first.
    pub fn new(tool: BuildTool, model: &BuildModel, history: Vec<String>, current: Option<&str>) -> Self {
        let choices = project_choices(tool, model);
        let project = match current {
            Some(current) if choices.iter().any(|c| c.value == current) => Some(current.to_owned()),
            _ => choices.first().map(|c| c.value.clone()),
        };
        Self { tool, line: String::new(), active: None, history, project }
    }

    pub fn tool(&self) -> BuildTool {
        self.tool
    }

    pub fn line(&self) -> &str {
        &self.line
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    fn completions(&self, model: &BuildModel) -> Vec<Completion> {
        let mut out = match self.tool {
            BuildTool::Maven => {
                let plugins = model
                    .maven
                    .iter()
                    .flat_map(|m| &m.projects)
                    .find(|p| Some(p.pom.as_str()) == self.project.as_deref())
                    .map(|p| p.plugins.as_slice())
                    .unwrap_or_default();
                goal_completions(&self.line, plugins, &self.history)
            }
            BuildTool::Gradle => match gradle_project_for_key(&model.gradle, self.project.as_deref()) {
                Some((build, _)) => task_completions(&self.line, build, &self.history),
                None => Vec::new(),
            },
        };
        out.truncate(COMPLETION_LIMIT);
        out
    }

    /// The completion list: "Recent" and the tool's own section, each only when it has a row.
    pub fn rows(&self, model: &BuildModel) -> Vec<ExecuteRow> {
        let (recent, commands): (Vec<Completion>, Vec<Completion>) =
            self.completions(model).into_iter().partition(Completion::is_recent);
        let own = match self.tool {
            BuildTool::Maven => ("commands", "Maven Goals"),
            BuildTool::Gradle => ("commands", "Gradle Tasks"),
        };
        let mut rows = Vec::new();
        for ((key, label), items) in [(("recent", "Recent"), recent), (own, commands)] {
            if items.is_empty() {
                continue;
            }
            rows.push(ExecuteRow::Section { key, label });
            rows.extend(items.into_iter().map(ExecuteRow::Completion));
        }
        rows
    }

    /// The line was typed: the list changes, so nothing in it stays selected.
    pub fn set_line(&mut self, line: &str) {
        line.clone_into(&mut self.line);
        self.active = None;
    }

    /// Another project was chosen: its completions replace the list.
    pub fn set_project(&mut self, project: &str) {
        self.project = Some(project.to_owned());
        self.active = None;
    }

    /// Moves the selection; from nothing selected, a move that finds nothing selects the first
    /// completion.
    pub fn move_selection(&mut self, key: Move, model: &BuildModel) {
        let rows = self.rows(model);
        if let Some(next) = navigate(&rows, self.active, key) {
            self.active = next.or_else(|| rows.iter().position(|row| !row.is_section()));
        }
    }

    /// Accepts the row at `index`: a recent line as it is, a completion followed by a space.
    pub fn accept(&mut self, index: usize, model: &BuildModel) {
        if let Some(ExecuteRow::Completion(completion)) = self.rows(model).get(index) {
            self.line = if completion.is_recent() { completion.line.clone() } else { format!("{} ", completion.line) };
            self.active = None;
        }
    }

    /// Tab: accepts the selected completion, else the first.
    pub fn tab(&mut self, model: &BuildModel) {
        let index = self.active.or_else(|| self.rows(model).iter().position(|row| !row.is_section()));
        if let Some(index) = index {
            self.accept(index, model);
        }
    }

    /// Enter: accepts the selected completion; with none selected, the trimmed line to run, if any.
    pub fn enter(&mut self, model: &BuildModel) -> Option<String> {
        match self.active {
            Some(index) => {
                self.accept(index, model);
                None
            }
            None => Some(self.line.trim().to_owned()).filter(|line| !line.is_empty()),
        }
    }
}

/// The line as kept in the history: without a leading launcher (`mvn`, `./gradlew`).
pub fn history_line(tool: BuildTool, text: &str) -> String {
    let text = text.trim();
    match text.split_once(char::is_whitespace) {
        Some((first, rest)) if is_launcher(tool, first) => rest.trim().to_owned(),
        None if is_launcher(tool, text) => String::new(),
        _ => text.to_owned(),
    }
}

/// The configuration that runs the goals / tasks typed in `text` in `project` (the first project
/// when it is not one); `None` when there is nothing to run.
pub fn execute_config(
    tool: BuildTool,
    text: &str,
    project: Option<&str>,
    model: &BuildModel,
    settings: &BuildSettings,
) -> Option<RunConfig> {
    match tool {
        BuildTool::Maven => {
            let maven = model.maven.as_ref()?;
            let chosen = maven.projects.iter().find(|p| Some(p.pom.as_str()) == project);
            let goals = parse_goal_line(text);
            let target = chosen.or_else(|| maven.projects.first())?;
            (!goals.is_empty()).then(|| maven_goal_config(target, &goals, maven, &settings.maven))
        }
        BuildTool::Gradle => {
            let (build, target) = gradle_project_for_key(&model.gradle, project)
                .or_else(|| gradle_project_for_key(&model.gradle, None))?;
            let tasks = parse_task_line(text);
            (!tasks.is_empty()).then(|| gradle_task_config(build, target, &tasks, &settings.gradle))
        }
    }
}

// -------------------------------------------------------------------------------------------------
// The popup

/// What the popup asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecuteEvent {
    /// Run `line` in `project`; the popup stays open until the workbench closes it.
    Execute { line: String, project: Option<String> },
    /// Escape.
    Dismissed,
}

/// Execute Maven Goal / Execute Gradle Task.
pub struct ExecutePopup {
    state: ExecuteState,
    model: BuildModel,
    field: Entity<TextField>,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<ExecuteEvent> for ExecutePopup {}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl ExecutePopup {
    /// The popup over `model` with the tool's `history` (newest first), in project `current`.
    pub fn new(
        tool: BuildTool,
        model: BuildModel,
        history: Vec<String>,
        current: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = tx(cx, if tool == BuildTool::Maven { "clean install -DskipTests" } else { "build --info" });
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let events = cx.subscribe(&field, |popup: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                popup.state.set_line(&text);
                cx.notify();
            }
            // Enter is the popup's own key: it is claimed before the field sees it.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(ExecuteEvent::Dismissed),
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        let state = ExecuteState::new(tool, &model, history, current);
        Self { state, model, field, key_claim, _field_events: events }
    }

    pub fn state(&self) -> &ExecuteState {
        &self.state
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The completion list as shown.
    pub fn rows(&self) -> Vec<ExecuteRow> {
        self.state.rows(&self.model)
    }

    pub fn projects(&self) -> Vec<ProjectChoice> {
        project_choices(self.state.tool(), &self.model)
    }

    /// The build model changed: completions follow it.
    pub fn set_model(&mut self, model: BuildModel, cx: &mut Context<Self>) {
        self.model = model;
        cx.notify();
    }

    /// Chooses the project the line runs in.
    pub fn choose_project(&mut self, project: &str, cx: &mut Context<Self>) {
        self.state.set_project(project);
        cx.notify();
    }

    /// Puts the state's line into the field when accepting changed it.
    fn sync_field(&mut self, cx: &mut Context<Self>) {
        let line = self.state.line().to_owned();
        if self.field.read(cx).text() != line {
            self.field.update(cx, |field, cx| field.set_text(&line, cx));
        }
        cx.notify();
    }

    /// A click on the row `index`: sections are ignored.
    pub fn click_row(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.accept(index, &self.model);
        self.sync_field(cx);
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return false;
        }
        let key = match keystroke.key.as_str() {
            "down" => Some(Move::Down),
            "up" => Some(Move::Up),
            "pagedown" => Some(Move::PageDown),
            "pageup" => Some(Move::PageUp),
            _ => None,
        };
        if let Some(key) = key {
            self.state.move_selection(key, &self.model);
            cx.notify();
            return true;
        }
        match keystroke.key.as_str() {
            "tab" => {
                self.state.tab(&self.model);
                self.sync_field(cx);
                true
            }
            "enter" => {
                match self.state.enter(&self.model) {
                    Some(line) => {
                        let project = self.state.project().map(str::to_owned);
                        cx.emit(ExecuteEvent::Execute { line, project });
                    }
                    None => self.sync_field(cx),
                }
                true
            }
            _ => false,
        }
    }
}

impl Focusable for ExecutePopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for ExecutePopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let maven = self.state.tool() == BuildTool::Maven;
        let (title, command, icon) = if maven {
            ("Execute Maven Goal", "mvn", "file-maven")
        } else {
            ("Execute Gradle Task", "gradle", "file-gradle")
        };
        let rows = self.rows();
        let active = self.state.active();
        let (fg, subtle, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-muted"));
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let row_height = theme::length(cx, "h-row");
        let empty = if self.state.line().trim().is_empty() { "Type a goal" } else { "No completions" };
        let list = div().flex().flex_col().max_h(px(320.0)).overflow_hidden().children(rows.iter().enumerate().map(
            |(index, row)| {
                match row {
                    ExecuteRow::Section { label, .. } => div()
                        .id(SharedString::from(format!("execute-row-{index}")))
                        .h(px(28.0))
                        .px(px(10.0))
                        .flex()
                        .items_end()
                        .text_size(theme::length(cx, "fs-small"))
                        .text_color(muted)
                        .child(SharedString::from(tx(cx, label))),
                    ExecuteRow::Completion(completion) => {
                        let selector = format!("execute-row-{index}");
                        let detail = completion.detail.clone().filter(|d| d != "recent" && d != "phase");
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .h(row_height)
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .when(active == Some(index), |el| el.bg(selected_bg))
                            .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.click_row(index, cx)))
                            .child(theme::icon(cx, icon, 16.0, subtle))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_color(fg)
                                    .child(SharedString::from(format!("{command} {}", completion.line))),
                            )
                            .children(detail.map(|d| div().text_color(subtle).child(SharedString::from(d))))
                    }
                }
            },
        ));
        let current = self.state.project().map(str::to_owned);
        let projects = self.projects();
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(tx(cx, title))))
                    .child(div().text_color(muted).child(SharedString::from(tx(cx, "Tab to complete, Enter to run")))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .h(px(32.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(theme::icon(cx, icon, 16.0, subtle))
                    .child(div().text_color(subtle).child(command))
                    .child(div().flex_1().child(self.field.clone())),
            )
            .child(if rows.is_empty() {
                div().px(px(10.0)).py(px(8.0)).text_color(muted).child(SharedString::from(tx(cx, empty)))
            } else {
                list
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .border_t_1()
                    .border_color(theme::color(cx, "border"))
                    .child(div().text_color(muted).child(SharedString::from(tx(cx, "Project:"))))
                    .children(projects.into_iter().map(|choice| {
                        let selector = format!("execute-project-{}", choice.value);
                        let chosen = current.as_deref() == Some(choice.value.as_str());
                        let value = choice.value.clone();
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .px(px(6.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .when(chosen, |el| el.bg(selected_bg))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.choose_project(&value, cx)))
                            .child(SharedString::from(choice.label))
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_build::{GradleModel, GradleProject, GradleTask};

    fn maven_state(history: &[&str]) -> (ExecuteState, BuildModel) {
        let model = BuildModel::default();
        let history = history.iter().map(|h| (*h).to_owned()).collect();
        (ExecuteState::new(BuildTool::Maven, &model, history, None), model)
    }

    fn press(state: &mut ExecuteState, model: &BuildModel, keys: &[Move]) {
        for key in keys {
            state.move_selection(*key, model);
        }
    }

    #[test]
    fn separates_recent_commands_from_maven_goals_and_accepts_a_recent_command_with_tab() {
        let (mut state, model) = maven_state(&["package -o"]);
        let sections: Vec<&str> = state
            .rows(&model)
            .iter()
            .filter_map(|row| match row {
                ExecuteRow::Section { label, .. } => Some(*label),
                ExecuteRow::Completion(_) => None,
            })
            .collect();
        assert_eq!(sections, ["Recent", "Maven Goals"]);
        state.tab(&model);
        assert_eq!(state.line(), "package -o");
    }

    #[test]
    fn skips_section_headers_during_keyboard_navigation() {
        let (mut state, model) = maven_state(&["package -o"]);
        press(&mut state, &model, &[Move::Down, Move::Down]);
        state.tab(&model);
        assert_eq!(state.line(), "clean ");
    }

    #[test]
    fn omits_the_empty_recent_section_when_no_commands_have_been_run() {
        let (mut state, model) = maven_state(&[]);
        let rows = state.rows(&model);
        assert_eq!(rows[0], ExecuteRow::Section { key: "commands", label: "Maven Goals" });
        assert!(!rows.iter().any(|row| matches!(row, ExecuteRow::Section { label: "Recent", .. })));
        state.tab(&model);
        assert_eq!(state.line(), "clean ");
    }

    #[test]
    fn starts_page_up_on_a_completion_rather_than_a_section() {
        let (mut state, model) = maven_state(&[]);
        press(&mut state, &model, &[Move::PageUp]);
        assert_eq!(state.enter(&model), None);
        assert_eq!(state.line(), "clean ");
    }

    #[test]
    fn navigates_sections_and_wraps() {
        let cases: [(&[Move], &str); 5] = [
            (&[Move::Up], "site-deploy "),
            (&[Move::Up, Move::Down], "package -o"),
            (&[Move::Down, Move::Up], "site-deploy "),
            (&[Move::Down, Move::Down, Move::Up], "package -o"),
            (&[Move::PageDown, Move::PageUp], "package -o"),
        ];
        for (keys, expected) in cases {
            let (mut state, model) = maven_state(&["package -o"]);
            press(&mut state, &model, keys);
            state.tab(&model);
            assert_eq!(state.line(), expected, "{keys:?}");
        }
    }

    #[test]
    fn clears_selection_when_typing_changes_the_rows_and_ignores_section_clicks() {
        let (mut state, model) = maven_state(&["package -o"]);
        press(&mut state, &model, &[Move::Up]);
        state.set_line("cl");
        assert_eq!(state.active(), None);
        let section = state.rows(&model).iter().position(|row| matches!(row, ExecuteRow::Section { .. }));
        state.accept(section.expect("a section"), &model);
        assert_eq!(state.line(), "cl");
        state.tab(&model);
        assert_eq!(state.line(), "clean ");
    }

    #[test]
    fn keeps_an_empty_completion_list_safe_for_all_navigation_and_acceptance_keys() {
        let (mut state, model) = maven_state(&[]);
        state.set_line("-Dunknown");
        for key in [Move::Down, Move::Up, Move::PageDown, Move::PageUp] {
            state.move_selection(key, &model);
            assert_eq!(state.line(), "-Dunknown");
        }
        state.tab(&model);
        assert_eq!(state.line(), "-Dunknown");
        assert_eq!(state.enter(&model).as_deref(), Some("-Dunknown"), "the line itself is what Enter runs");
        assert_eq!(state.line(), "-Dunknown");
        assert!(state.rows(&model).is_empty());
    }

    #[test]
    fn page_down_selects_a_completion_rather_than_a_section_at_its_landing_position() {
        let history: Vec<String> = (0..9).rev().map(|i| format!("command-{i}")).collect();
        let model = BuildModel::default();
        let mut state = ExecuteState::new(BuildTool::Maven, &model, history, None);
        press(&mut state, &model, &[Move::Down, Move::PageDown]);
        assert_eq!(state.enter(&model), None);
        let first = goal_completions("", &[], &[]).remove(0).line;
        assert_eq!(state.line(), format!("{first} "));
    }

    fn build(dir: &str, name: &str) -> GradleModel {
        GradleModel {
            dir: dir.to_owned(),
            projects: vec![GradleProject {
                path: ":".to_owned(),
                name: name.to_owned(),
                dir: dir.to_owned(),
                build_file: Some("build.gradle.kts".to_owned()),
                tasks: vec![GradleTask { name: name.to_owned(), group: "build".to_owned(), description: None }],
                ..GradleProject::default()
            }],
            command: "gradle".to_owned(),
            ..GradleModel::default()
        }
    }

    #[test]
    fn resets_selection_when_changing_the_gradle_build_used_for_completion() {
        let model = BuildModel { maven: None, gradle: vec![build("a", "assemble"), build("b", "build")] };
        let mut state = ExecuteState::new(BuildTool::Gradle, &model, Vec::new(), None);
        assert_eq!(state.rows(&model)[0], ExecuteRow::Section { key: "commands", label: "Gradle Tasks" });
        state.move_selection(Move::Down, &model);
        assert_eq!(state.active(), Some(1));
        state.set_project(&gradle_project_key("b", ":"));
        assert_eq!(state.active(), None);
        state.tab(&model);
        assert_eq!(state.line(), "build ");
        let emptied = BuildModel::default();
        state.tab(&emptied);
        assert_eq!(state.line(), "build ");
    }

    // Not from a spec: the line a run keeps in the history, and the configuration it starts.
    #[test]
    fn a_typed_line_runs_in_the_chosen_project_and_is_kept_without_its_launcher() {
        assert_eq!(history_line(BuildTool::Maven, "./mvnw clean  install"), "clean  install");
        assert_eq!(history_line(BuildTool::Gradle, "build --info"), "build --info");
        let model = BuildModel { maven: None, gradle: vec![build("a", "assemble"), build("b", "build")] };
        let settings = BuildSettings::default();
        let key = gradle_project_key("b", ":");
        let config = execute_config(BuildTool::Gradle, "gradle build", Some(&key), &model, &settings);
        let config = config.expect("the line runs");
        assert_eq!(config.args, ["build"]);
        assert_eq!(config.cwd.as_deref().and_then(|cwd| cwd.to_str()), Some("b"));
        assert!(execute_config(BuildTool::Gradle, "gradle", Some(&key), &model, &settings).is_none());
        assert!(execute_config(BuildTool::Maven, "clean", None, &model, &settings).is_none(), "no Maven project");
    }
}
