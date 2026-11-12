//! The welcome screen: what a window shows while it has no project. The app's name beside a
//! search field over the recent projects, in their groups, with the buttons that open, clone or
//! connect to a project; without recent projects, a page that offers the same three.
//!
//! The keyboard stays in the search field: the arrows move the selection, Enter opens it, Delete
//! asks to remove it. The view changes nothing but itself: what it wants opened, removed or
//! regrouped it reports as a [`WelcomeEvent`], and the menus of its rows are for its window to show.

use super::project_color::{project_colors, project_initials};
use super::welcome_model::{
    GroupNameError, KeyMods, ListKey, OPEN_PROJECT, ProjectGroup, RecentEntry, RecentMatch, RecentRow, WelcomeAction,
    WelcomeCommand, create_group, fold_key, group_menu, group_name_error, group_of, handle_list_key, landing_text,
    manage_menu, move_to_group, project_menu, recent_rows, remove_group, remove_recent, rename_group,
    set_group_expanded, welcome_actions,
};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::platform::misc::platform_info::Os;
use crate::theme;
use crate::theme::tokens::Color;
use crate::ui::matching::index_runs;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    IntoElement, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, PromptLevel, Render, Rgba, ScrollStrategy,
    SharedString, Stateful, Subscription, UniformList, UniformListScrollHandle, Window, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, uniform_list,
};
use ide_workspace::{RecentProject, RecentProjectGroup};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The height of a row of the list: a project's name over its path.
const ROW_HEIGHT: f32 = 48.0;

/// What a menu of the welcome screen is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    Project(RecentEntry),
    /// The group of this name.
    Group(String),
    /// The Manage button under the app's name.
    Manage,
}

/// What the welcome screen asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum WelcomeEvent {
    /// Open the project at this path.
    Open(PathBuf),
    /// Run this registered action: a button was pressed.
    RunAction(&'static str),
    /// The project at this path is no longer listed: forget it.
    Remove(PathBuf),
    /// The groups are these now: store them.
    SetGroups(Vec<RecentProjectGroup>),
    /// Show this folder in the file manager.
    Reveal(PathBuf),
    /// Show the menu of `target` (see [`Welcome::menu`]) with its corner at `position`.
    ShowMenu { target: MenuTarget, position: Point<Pixels> },
}

/// A project row as it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownProject {
    /// The letters on its coloured tile.
    pub initials: String,
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
}

/// What a group name is being typed for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NamingPurpose {
    /// A new group; the project at `path` goes into it.
    NewGroup { path: Option<String> },
    /// Another name for this group.
    RenameGroup(String),
}

/// The group name being typed, above the list.
struct Naming {
    purpose: NamingPurpose,
    field: Entity<TextField>,
    /// Why the text cannot be the name, once it has been found out.
    error: Option<GroupNameError>,
    _events: Subscription,
}

pub struct Welcome {
    /// Has the keyboard while there is no list to search.
    focus: FocusHandle,
    field: Entity<TextField>,
    projects: Vec<RecentEntry>,
    groups: Vec<ProjectGroup>,
    home: Option<String>,
    version: Option<String>,
    /// The system whose file manager the menus name.
    os: Os,
    rows: Rc<Vec<RecentRow>>,
    /// Index into `rows` of the selected row.
    active: usize,
    naming: Option<Naming>,
    scroll: UniformListScrollHandle,
    /// The claim on the keys of the list, ahead of the keymap.
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<WelcomeEvent> for Welcome {}

fn entries(recent: &[RecentProject]) -> Vec<RecentEntry> {
    recent.iter().map(RecentEntry::from).collect()
}

fn groups_of(groups: &[RecentProjectGroup]) -> Vec<ProjectGroup> {
    groups.iter().map(ProjectGroup::from).collect()
}

fn tx(cx: &mut App, text: &str) -> SharedString {
    SharedString::from(cx.default_global::<Localization>().0.tx(text))
}

impl Welcome {
    /// The welcome screen over `recent` (the most recent first) in `groups`, showing paths under
    /// `home` with `~`.
    pub fn new(
        recent: &[RecentProject],
        groups: &[RecentProjectGroup],
        home: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Search projects");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        // Enter is the list's key, claimed before the field sees it, and Escape has nothing to close.
        let events = cx.subscribe(&field, |view: &mut Welcome, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                view.query_changed(cx);
            }
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |view, cx| view.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let mut view = Self {
            focus: cx.focus_handle(),
            field,
            projects: entries(recent),
            groups: groups_of(groups),
            home: home.map(|home| home.to_string_lossy().into_owned()),
            version: None,
            os: Os::from_platform(std::env::consts::OS),
            rows: Rc::default(),
            active: 0,
            naming: None,
            scroll: UniformListScrollHandle::new(),
            key_claim,
            _field_events: events,
        };
        view.rebuild(cx);
        view
    }

    /// Shows the app's version under its name.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// The recent projects and their groups are these now. The search text stays, and the
    /// selection stays on its row as far as the list still has it.
    pub fn set_recent(&mut self, recent: &[RecentProject], groups: &[RecentProjectGroup], cx: &mut Context<Self>) {
        self.projects = entries(recent);
        self.groups = groups_of(groups);
        if self.projects.is_empty() {
            self.naming = None;
        }
        self.rebuild(cx);
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The rows as text: `[Work 2]` for a group, a project's name, indented under its group.
    pub fn lines(&self) -> Vec<String> {
        self.rows.iter().map(RecentRow::line).collect()
    }

    /// The selected row, as [`Self::lines`] shows it.
    pub fn selected(&self) -> Option<String> {
        self.rows.get(self.active).map(RecentRow::line)
    }

    /// The project rows as they are drawn, in order.
    pub fn shown_projects(&self) -> Vec<ShownProject> {
        let shown = |row: &RecentRow| match row {
            RecentRow::Group { .. } => None,
            RecentRow::Project { found, .. } => Some(ShownProject {
                initials: project_initials(&found.project.name),
                name: found.project.name.clone(),
                path: found.display_path.clone(),
                branch: found.project.branch.clone(),
            }),
        };
        self.rows.iter().filter_map(shown).collect()
    }

    /// What is said in place of the list: the page's title while there are no recent projects,
    /// that nothing matches while a search leaves none.
    pub fn empty_text(&self) -> Option<&'static str> {
        if self.projects.is_empty() {
            Some("Welcome to OpenIDEA")
        } else if self.rows.is_empty() {
            Some("No matching projects")
        } else {
            None
        }
    }

    /// The ids of the actions the buttons run, in the order of the buttons.
    pub fn actions(&self, cx: &App) -> Vec<&'static str> {
        shown_actions(cx).into_iter().map(|action| action.id).collect()
    }

    /// The title of the form a group name is being typed in.
    pub fn naming_title(&self) -> Option<&'static str> {
        self.naming.as_ref().map(|naming| match naming.purpose {
            NamingPurpose::NewGroup { .. } => "New Project Group",
            NamingPurpose::RenameGroup(_) => "Rename Project Group",
        })
    }

    /// The field a group name is being typed in.
    pub fn naming_field(&self) -> Option<&Entity<TextField>> {
        self.naming.as_ref().map(|naming| &naming.field)
    }

    /// Why the typed text cannot name the group, as the form says it.
    pub fn naming_error(&self, cx: &mut App) -> Option<String> {
        let error = self.naming.as_ref()?.error.as_ref()?;
        Some(error.text(&mut cx.default_global::<Localization>().0))
    }

    /// Puts the keyboard in the search field, with its text selected: typing looks for another project.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.projects.is_empty() {
            window.focus(&self.focus, cx);
        } else {
            self.field.update(cx, TextField::select_all);
            window.focus(&self.field.focus_handle(cx), cx);
        }
    }

    /// The rows of the menu of `target`, for the window to show; a chosen row of the menu's own
    /// comes back through [`Self::run`].
    pub fn menu(&self, target: &MenuTarget, cx: &mut App) -> Vec<MenuItem<WelcomeCommand>> {
        match target {
            MenuTarget::Project(project) => {
                if let Some(group) = group_of(&project.path, &self.groups) {
                    // The label is built once here, so that the menu finds it in the user's language.
                    let i18n = &mut cx.default_global::<Localization>().0;
                    i18n.tx_built(&["Remove from '", "'"], &[group.name.as_str().into()]);
                }
                project_menu(project, &self.groups, self.os)
            }
            MenuTarget::Group(name) => {
                self.groups.iter().find(|group| group.name == *name).map(group_menu).unwrap_or_default()
            }
            MenuTarget::Manage => manage_menu(),
        }
    }

    /// Carries out a row of one of the menus.
    pub fn run(&mut self, command: WelcomeCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            WelcomeCommand::Open(path) => cx.emit(WelcomeEvent::Open(PathBuf::from(path))),
            WelcomeCommand::Reveal(path) => cx.emit(WelcomeEvent::Reveal(PathBuf::from(path))),
            WelcomeCommand::CopyPath(path) => cx.write_to_clipboard(ClipboardItem::new_string(path)),
            WelcomeCommand::MoveToGroup { path, group } => {
                self.save_groups(move_to_group(&self.groups, &path, group.as_deref()), cx);
            }
            WelcomeCommand::NewGroup { path } => self.start_naming(NamingPurpose::NewGroup { path }, window, cx),
            WelcomeCommand::AskRemove(path) => self.ask_remove(&path, window, cx),
            WelcomeCommand::RenameGroup(name) => self.start_naming(NamingPurpose::RenameGroup(name), window, cx),
            WelcomeCommand::RemoveGroup(name) => self.save_groups(remove_group(&self.groups, &name), cx),
        }
    }

    /// Lists what the search text leaves. The selection stays on its row index, as far as there
    /// is one.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let rows = recent_rows(&self.projects, &self.groups, self.field.read(cx).text(), self.home.as_deref());
        self.active = self.active.min(rows.len().saturating_sub(1));
        self.rows = Rc::new(rows);
        cx.notify();
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.active = 0;
        self.rebuild(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.field.update(cx, |field, cx| field.set_text("", cx));
        self.query_changed(cx);
        window.focus(&self.field.focus_handle(cx), cx);
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.rows.len() {
            self.active = index;
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Opens the project of row `index`; a group folds or unfolds.
    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.rows.get(index).cloned() {
            Some(RecentRow::Group { group, .. }) => self.fold(&group.name, !group.expanded, cx),
            Some(RecentRow::Project { found, .. }) => cx.emit(WelcomeEvent::Open(PathBuf::from(found.project.path))),
            None => {}
        }
    }

    fn fold(&mut self, group: &str, expanded: bool, cx: &mut Context<Self>) {
        self.save_groups(set_group_expanded(&self.groups, group, expanded), cx);
    }

    /// Shows `groups` at once and reports them for storing.
    fn save_groups(&mut self, groups: Vec<ProjectGroup>, cx: &mut Context<Self>) {
        if groups == self.groups {
            return;
        }
        self.groups = groups;
        self.rebuild(cx);
        cx.emit(WelcomeEvent::SetGroups(self.groups.iter().map(RecentProjectGroup::from).collect()));
    }

    /// Selects row `index` and asks the window for its menu at `position`.
    fn show_row_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let target = match self.rows.get(index) {
            Some(RecentRow::Project { found, .. }) => MenuTarget::Project(found.project.clone()),
            Some(RecentRow::Group { group, .. }) => MenuTarget::Group(group.name.clone()),
            None => return,
        };
        self.select(index, cx);
        cx.emit(WelcomeEvent::ShowMenu { target, position });
    }

    /// Where the menu of row `index` opens when the keyboard asks for it: inside the row, near
    /// its right end.
    fn row_menu_position(&self, index: usize) -> Point<Pixels> {
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let top = list.top() + state.base_handle.offset().y + px(ROW_HEIGHT) * index as f32;
        point(list.right() - px(40.0), top + px(24.0))
    }

    /// Asks whether the project at `path` is to be removed from the list, and removes it on a yes.
    fn ask_remove(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.projects.iter().find(|project| project.path == path) else { return };
        let name = project.name.clone();
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx("Remove Recent Project");
        let message = i18n.tx_built(&["Remove '", "' from the list of recent projects?"], &[name.as_str().into()]);
        let answers = [i18n.tx("Remove"), i18n.tx("Cancel")];
        let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
        let answer = window.prompt(PromptLevel::Info, &title, Some(&message), &answers, cx);
        let path = path.to_owned();
        cx.spawn_in(window, async move |view, cx| {
            if answer.await.ok() == Some(0) {
                // The view is gone when its window shows a project by now.
                let _ = view.update(cx, |view, cx| view.remove(&path, cx));
            }
        })
        .detach();
    }

    /// Takes the project at `path` off the list at once and reports it for forgetting.
    fn remove(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.projects.iter().any(|project| project.path == path) {
            return;
        }
        (self.projects, self.groups) = remove_recent(&self.projects, &self.groups, path);
        if self.projects.is_empty() {
            self.naming = None;
        }
        self.rebuild(cx);
        cx.emit(WelcomeEvent::Remove(PathBuf::from(path)));
    }

    /// Opens the form for a group name, with the keyboard in it.
    fn start_naming(&mut self, purpose: NamingPurpose, window: &mut Window, cx: &mut Context<Self>) {
        let initial = match &purpose {
            NamingPurpose::NewGroup { .. } => String::new(),
            NamingPurpose::RenameGroup(name) => name.clone(),
        };
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&initial, cx);
            field.select_all(cx);
            field
        });
        let events =
            cx.subscribe_in(&field, window, |view: &mut Welcome, _, event: &TextFieldEvent, window, cx| match event {
                TextFieldEvent::Changed => view.check_name(cx),
                TextFieldEvent::Submitted => view.commit_naming(window, cx),
                TextFieldEvent::Cancelled => view.cancel_naming(window, cx),
            });
        window.focus(&field.focus_handle(cx), cx);
        self.naming = Some(Naming { purpose, field, error: None, _events: events });
        cx.notify();
    }

    /// Why the typed text cannot be the group's name.
    fn name_error(&self, cx: &App) -> Option<GroupNameError> {
        let naming = self.naming.as_ref()?;
        let current = match &naming.purpose {
            NamingPurpose::NewGroup { .. } => None,
            NamingPurpose::RenameGroup(name) => Some(name.as_str()),
        };
        group_name_error(&self.groups, naming.field.read(cx).text(), current)
    }

    fn check_name(&mut self, cx: &mut Context<Self>) {
        let error = self.name_error(cx);
        if let Some(naming) = &mut self.naming {
            naming.error = error;
        }
        cx.notify();
    }

    /// Creates or renames the group when the typed name can be used; says why not otherwise.
    fn commit_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_error(cx).is_some() {
            self.check_name(cx);
            return;
        }
        let Some(naming) = self.naming.take() else { return };
        let name = naming.field.read(cx).text().to_owned();
        let next = match &naming.purpose {
            NamingPurpose::NewGroup { path } => create_group(&self.groups, &name, path.as_deref()),
            NamingPurpose::RenameGroup(current) => rename_group(&self.groups, current, &name),
        };
        if let Some(next) = next {
            self.save_groups(next, cx);
        }
        self.end_naming(window, cx);
    }

    fn cancel_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        self.end_naming(window, cx);
    }

    /// The keyboard goes back to the search field.
    fn end_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.field.focus_handle(cx), cx);
        cx.notify();
    }

    /// The keys of the list, while the keyboard is in the search field: the keymap's bindings
    /// for them do not run. Returns whether `keystroke` is one of them.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (key, modifiers) = (keystroke.key.as_str(), keystroke.modifiers);
        let (search_is_empty, caret_at_end) = {
            let field = self.field.read(cx);
            let end = field.text().len();
            (end == 0, field.selection() == (end..end))
        };
        if !modifiers.modified()
            && let Some(expanded) = fold_key(key, self.rows.get(self.active), search_is_empty)
        {
            if let Some(RecentRow::Group { group, .. }) = self.rows.get(self.active).cloned() {
                self.fold(&group.name, expanded, cx);
            }
            return true;
        }
        let mods = KeyMods { platform: modifiers.platform, shift: modifiers.shift };
        match handle_list_key(key, mods, self.rows.len(), self.active, caret_at_end) {
            None => return false,
            Some(ListKey::Move(index)) => self.select(index, cx),
            Some(ListKey::Open(index)) => self.activate(index, cx),
            // A group is removed from its menu: the key is taken and does nothing on one.
            Some(ListKey::Remove(index)) => {
                let path = self.rows.get(index).and_then(RecentRow::project).map(|project| project.path.clone());
                if let Some(path) = path {
                    self.ask_remove(&path, window, cx);
                }
            }
            Some(ListKey::Menu(index)) => {
                let position = self.row_menu_position(index);
                self.show_row_menu(index, position, cx);
            }
        }
        true
    }

    /// The keyboard follows what is shown: it is in the search field while there is a list, and
    /// on the page itself while there is none.
    fn keep_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = self.field.focus_handle(cx);
        if self.projects.is_empty() {
            if field.is_focused(window) {
                window.focus(&self.focus, cx);
            }
        } else if self.naming.is_none() && self.focus.is_focused(window) {
            window.focus(&field, cx);
        }
    }

    /// The app's name, its version and what it is, over the pages and the Manage button.
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let tagline = SharedString::from(cx.default_global::<Localization>().0.t("app.about.tagline", &[]));
        let (name, projects) = (tx(cx, "OpenIDEA"), tx(cx, "Projects"));
        let (strong, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let hover = theme::color(cx, "bg-hover");
        let manage = div()
            .id("welcome-manage")
            .debug_selector(|| "welcome-manage".to_owned())
            .size(px(26.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-control"))
            .hover(|el| el.bg(hover))
            .child(theme::icon(cx, "settings", 16.0, theme::color(cx, "fg-secondary")))
            .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                cx.emit(WelcomeEvent::ShowMenu { target: MenuTarget::Manage, position: event.position() });
            }));
        div()
            .flex_none()
            .w(px(240.0))
            .h_full()
            .flex()
            .flex_col()
            .pt(px(28.0))
            .px(px(12.0))
            .pb(px(14.0))
            .bg(theme::color(cx, "bg-elev"))
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(
                div()
                    .debug_selector(|| "welcome-brand".to_owned())
                    .flex()
                    .flex_col()
                    .px(px(6.0))
                    .pb(px(8.0))
                    .child(div().text_color(strong).child(name))
                    .children(
                        self.version.clone().map(|version| div().text_color(muted).child(SharedString::from(version))),
                    ),
            )
            .child(
                div().px(px(6.0)).pb(px(20.0)).text_size(theme::length(cx, "fs-tiny")).text_color(muted).child(tagline),
            )
            .child(
                div()
                    .h(px(34.0))
                    .px(px(24.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-selection"))
                    .bg(theme::color(cx, "bg-tree-selected"))
                    .text_color(strong)
                    .child(projects),
            )
            .child(div().flex_1())
            .child(div().flex().px(px(4.0)).child(manage))
    }

    /// The search field with the buttons beside it.
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let muted = theme::color(cx, "fg-muted");
        let hover = theme::color(cx, "bg-hover");
        let clear = (!self.field.read(cx).text().is_empty()).then(|| {
            div()
                .id("welcome-clear")
                .debug_selector(|| "welcome-clear".to_owned())
                .flex_none()
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-control"))
                .hover(|el| el.bg(hover))
                .child(theme::icon(cx, "close", 12.0, muted))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.clear_search(window, cx)))
        });
        let buttons: Vec<Stateful<Div>> = shown_actions(cx)
            .into_iter()
            .map(|action| {
                let label = tx(cx, action.label);
                button(cx, format!("welcome-action-{}", action.id), label, false)
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(WelcomeEvent::RunAction(action.id))))
            })
            .collect();
        div()
            .flex_none()
            .h(px(76.0))
            .ml(px(14.0))
            .pl(px(6.0))
            .pr(px(14.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(theme::icon(cx, "search", 16.0, muted))
            .child(div().debug_selector(|| "welcome-search".to_owned()).flex_1().min_w_0().child(self.field.clone()))
            .children(clear)
            .child(div().flex_none().flex().gap(px(8.0)).children(buttons))
    }

    /// The form a group name is typed in: Enter takes the name, Escape gives up.
    fn naming_form(&self, naming: &Naming, cx: &mut Context<Self>) -> Div {
        let (title, accept) = match naming.purpose {
            NamingPurpose::NewGroup { .. } => ("New Project Group", "Create"),
            NamingPurpose::RenameGroup(_) => ("Rename Project Group", "Rename"),
        };
        let error = naming.error.as_ref().map(|error| error.text(&mut cx.default_global::<Localization>().0));
        let (title, label, accept, cancel) = (tx(cx, title), tx(cx, "Group name:"), tx(cx, accept), tx(cx, "Cancel"));
        let danger = theme::color(cx, "danger");
        let frame = if error.is_some() { danger } else { theme::color(cx, "border-button") };
        let accept = button(cx, "welcome-naming-accept".to_owned(), accept, true)
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.commit_naming(window, cx)));
        let cancel = button(cx, "welcome-naming-cancel".to_owned(), cancel, false)
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.cancel_naming(window, cx)));
        div()
            .flex_none()
            .ml(px(14.0))
            .pl(px(6.0))
            .pr(px(14.0))
            .py(px(10.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(div().text_color(theme::color(cx, "fg-strong")).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_none().child(label))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(theme::length(cx, "h-field"))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .rounded(theme::length(cx, "radius-control"))
                            .border_1()
                            .border_color(frame)
                            .bg(theme::color(cx, "bg-input"))
                            .child(naming.field.clone()),
                    )
                    .child(accept)
                    .child(cancel),
            )
            .children(error.map(|error| {
                div().text_size(theme::length(cx, "fs-small")).text_color(danger).child(SharedString::from(error))
            }))
    }

    /// The recent projects, in their groups.
    fn list(&self, cx: &mut Context<Self>) -> UniformList {
        let (rows, active) = (Rc::clone(&self.rows), self.active);
        uniform_list(
            "welcome-projects",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let muted = theme::color(cx, "fg-muted");
                let hover = theme::color(cx, "bg-tree-hover");
                // Lighter than the selection of the pages beside the list.
                let selected_bg = theme::color(cx, "bg-list-selected").opacity(0.7);
                let more_hover = theme::color(cx, "bg-hover");
                let radius = theme::length(cx, "radius-popup");
                range
                    .map(|index| {
                        let selected = index == active;
                        let inside = match &rows[index] {
                            RecentRow::Group { group, count } => group_row(cx, group, *count),
                            RecentRow::Project { found, group } => project_row(cx, found, group.is_some()),
                        };
                        let more = div()
                            .id(SharedString::from(format!("welcome-more:{index}")))
                            .debug_selector(|| format!("welcome-more-{index}"))
                            .flex_none()
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme::length(cx, "radius-control"))
                            .hover(|el| el.bg(more_hover))
                            .child(theme::icon(cx, "more", 16.0, muted))
                            .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                                // The row under the button is not opened by this click.
                                cx.stop_propagation();
                                view.show_row_menu(index, event.position(), cx);
                            }));
                        div()
                            .id(index)
                            .debug_selector(|| format!("welcome-row-{index}"))
                            .h(px(ROW_HEIGHT))
                            .w_full()
                            .py(px(1.0))
                            .child(
                                inside
                                    .size_full()
                                    .rounded(radius)
                                    .when(selected, |el| el.bg(selected_bg).child(more))
                                    .when(!selected, |el| el.hover(|el| el.bg(hover))),
                            )
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.select(index, cx);
                                view.activate(index, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                                    view.show_row_menu(index, event.position, cx);
                                }),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full()
    }

    /// The search field, the buttons and the list of recent projects.
    fn projects_page(&self, cx: &mut Context<Self>) -> Div {
        let header = self.header(cx);
        let naming = self.naming.as_ref().map(|naming| self.naming_form(naming, cx));
        let body = if self.rows.is_empty() {
            let nothing = tx(cx, "No matching projects");
            div().px(px(10.0)).py(px(8.0)).text_color(theme::color(cx, "fg-faint")).child(nothing).into_any_element()
        } else {
            self.list(cx).into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .children(naming)
            .child(div().flex_1().min_h_0().pt(px(14.0)).pb(px(14.0)).pl(px(24.0)).pr(px(16.0)).child(body))
    }

    /// The page shown without recent projects: what can be done here, as large buttons.
    fn landing(&self, cx: &mut Context<Self>) -> Div {
        let actions = shown_actions(cx);
        let can_clone = actions.iter().any(|action| action.id == "vcs.clone");
        let (title, what) = (tx(cx, "Welcome to OpenIDEA"), tx(cx, landing_text(can_clone)));
        let (dropping, hint) = (tx(cx, "You can also drop a folder onto this window."), tx(cx, "Open a folder"));
        let shortcut =
            cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(OPEN_PROJECT, is_mac_host()));
        let (strong, muted, faint) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"));
        let (tile_bg, tile_border, hover) =
            (theme::color(cx, "bg-elev"), theme::color(cx, "border-button"), theme::color(cx, "bg-hover"));
        let large: Vec<Stateful<Div>> = actions
            .into_iter()
            .map(|action| {
                let label = tx(cx, action.label);
                div()
                    .id(SharedString::from(format!("welcome-landing:{}", action.id)))
                    .debug_selector(|| format!("welcome-landing-{}", action.id))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(strong)
                    .child(
                        div()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme::length(cx, "radius-popup-large"))
                            .bg(tile_bg)
                            .border_1()
                            .border_color(tile_border)
                            .hover(|el| el.bg(hover))
                            .child(theme::icon(cx, action.icon, 20.0, strong)),
                    )
                    .child(label)
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(WelcomeEvent::RunAction(action.id));
                    }))
            })
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.0))
            .pb(px(40.0))
            .child(div().text_size(px(18.0)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(title))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_color(muted)
                    .child(div().child(what))
                    .child(div().child(dropping)),
            )
            .child(div().flex().gap(px(24.0)).mt(px(16.0)).children(large))
            .children(shortcut.map(|shortcut| {
                div().flex().items_center().gap(px(8.0)).mt(px(8.0)).text_color(faint).child(hint).child(
                    div()
                        .px(px(6.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(tile_border)
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(shortcut)),
                )
            }))
    }
}

/// The buttons shown: Open, and the others when their action is registered.
fn shown_actions(cx: &App) -> Vec<WelcomeAction> {
    let registry = cx.try_global::<ActionRegistry>();
    welcome_actions(|id| registry.is_some_and(|registry| registry.contains(id)))
}

fn hsla(color: Color) -> Hsla {
    Rgba { r: color.r, g: color.g, b: color.b, a: color.a }.into()
}

/// A push button; the `primary` one is what Enter does.
fn button(cx: &App, selector: String, label: SharedString, primary: bool) -> Stateful<Div> {
    let hover = theme::color(cx, "bg-hover");
    let look = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(|| selector)
        .flex_none()
        .h(theme::length(cx, "h-button"))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-control"))
        .whitespace_nowrap()
        .child(label);
    if primary {
        look.bg(theme::color(cx, "bg-button-primary")).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        look.bg(theme::color(cx, "bg-button"))
            .border_1()
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
            .hover(|el| el.bg(hover))
    }
}

/// `text` with the characters at the byte offsets `indices` on the search-match colour.
fn marked(cx: &App, text: &str, indices: Option<&[usize]>) -> Div {
    let (match_bg, match_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
    let plain = |part: &str| div().child(SharedString::from(part.to_owned()));
    let mut line = div().flex().whitespace_nowrap();
    let mut at = 0;
    for run in index_runs(text, indices.unwrap_or_default()) {
        if run.start < at {
            continue;
        }
        if run.start > at {
            line = line.child(plain(&text[at..run.start]));
        }
        at = run.end;
        line = line.child(plain(&text[run]).bg(match_bg).text_color(match_fg).rounded(px(3.0)));
    }
    if at < text.len() {
        line = line.child(plain(&text[at..]));
    }
    line
}

/// The tile of a project: its initials on the colours of its name.
fn tile(cx: &App, name: &str) -> Div {
    let colors = cx.try_global::<theme::ActiveTheme>().map(|active| project_colors(name, &active.theme));
    let start = colors.and_then(|colors| colors.start).map_or_else(|| theme::color(cx, "accent"), hsla);
    let end = colors.and_then(|colors| colors.end).map_or(start, hsla);
    div()
        .flex_none()
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .bg(linear_gradient(135.0, linear_color_stop(start, 0.0), linear_color_stop(end, 1.0)))
        .text_color(theme::color(cx, "fg-button-primary"))
        .text_size(theme::length(cx, "fs-tiny"))
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(project_initials(name)))
}

/// The inside of a project's row: its tile, its name with its branch, and its path under them.
fn project_row(cx: &App, found: &RecentMatch, nested: bool) -> Div {
    let muted = theme::color(cx, "fg-muted");
    let branch = found.project.branch.clone().map(|branch| {
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_color(muted)
            .child(theme::icon(cx, "branch", 14.0, muted))
            .child(SharedString::from(branch))
    });
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .pl(px(if nested { 32.0 } else { 10.0 }))
        .pr(px(10.0))
        .child(tile(cx, &found.project.name))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .child(marked(cx, &found.project.name, found.name_match.as_deref()))
                        .children(branch),
                )
                .child(marked(cx, &found.display_path, found.path_match.as_deref()).text_color(muted)),
        )
}

/// The inside of a group's row: whether it is folded, its name, and how many projects it lists.
fn group_row(cx: &App, group: &ProjectGroup, count: usize) -> Div {
    let muted = theme::color(cx, "fg-muted");
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .child(theme::icon(cx, if group.expanded { "chevron-down" } else { "chevron-right" }, 16.0, muted))
        .child(theme::icon(cx, "folder", 16.0, muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_color(theme::color(cx, "fg-strong"))
                .child(SharedString::from(group.name.clone())),
        )
        .child(div().flex_none().text_color(theme::color(cx, "fg-faint")).child(SharedString::from(count.to_string())))
}

impl Focusable for Welcome {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.projects.is_empty() { self.focus.clone() } else { self.field.focus_handle(cx) }
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.keep_keyboard(window, cx);
        let landing = self.projects.is_empty();
        let sidebar = self.sidebar(cx);
        let page = if landing { self.landing(cx) } else { self.projects_page(cx) };
        div()
            .size_full()
            .flex()
            .when(landing, |el| el.track_focus(&self.focus))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(sidebar)
            .child(div().flex_1().min_w_0().h_full().bg(theme::color(cx, "bg-island")).child(page))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{Action, ActionMeta};
    use crate::ui::popup_menu::{Line, resolve};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<WelcomeEvent>>>;

    const ACME: &str = "/Users/me/repo/acme-api";
    const DELTA: &str = "/Users/me/repo/delta";
    const PY_DEMO: &str = "/tmp/py-demo";

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn recent(name: &str, path: &str, branch: Option<&str>) -> RecentProject {
        RecentProject {
            path: PathBuf::from(path).into(),
            name: name.to_owned(),
            opened_at_ms: 0.0,
            branch: branch.map(str::to_owned),
        }
    }

    fn three() -> Vec<RecentProject> {
        vec![recent("acme-api", ACME, Some("dev")), recent("delta", DELTA, None), recent("py-demo", PY_DEMO, None)]
    }

    fn group(name: &str, paths: &[&str], expanded: bool) -> RecentProjectGroup {
        RecentProjectGroup {
            name: name.to_owned(),
            paths: paths.iter().map(|path| PathBuf::from(path).into()).collect(),
            expanded,
        }
    }

    /// The welcome screen in a window of its own, with the keyboard, and what it has asked of the
    /// window so far.
    fn open(
        cx: &mut TestAppContext,
        recent: Vec<RecentProject>,
        groups: Vec<RecentProjectGroup>,
    ) -> (Entity<Welcome>, Events, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(move |_, cx| {
            Welcome::new(&recent, &groups, Some(Path::new("/Users/me")), cx).with_version("1.2.3")
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &WelcomeEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn lines(view: &Entity<Welcome>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    fn selected(view: &Entity<Welcome>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.selected())
    }

    fn last(events: &Events) -> Option<WelcomeEvent> {
        events.borrow().last().cloned()
    }

    fn center(cx: &mut VisualTestContext, selector: &'static str) -> Point<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center()
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let at = center(cx, selector);
        cx.simulate_click(at, Modifiers::default());
        cx.run_until_parked();
    }

    fn search_has_the_keyboard(view: &Entity<Welcome>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| view.read(cx).field().focus_handle(cx).is_focused(window))
    }

    fn stored(groups: &[RecentProjectGroup]) -> Option<WelcomeEvent> {
        Some(WelcomeEvent::SetGroups(groups.to_vec()))
    }

    // src/features/welcome/welcome.test.tsx › Welcome › Projects ›
    // shows brand, version and project rows with badges, ~ paths and branches
    #[gpui_kit::test]
    async fn shows_brand_version_and_project_rows_with_badges_home_relative_paths_and_branches(
        cx: &mut TestAppContext,
    ) {
        init(cx);
        let (view, events, cx) = open(cx, three(), Vec::new());
        assert!(cx.debug_bounds("welcome-brand").is_some(), "the app's name is drawn");
        view.read_with(cx, |view, cx| {
            assert_eq!(view.version(), Some("1.2.3"));
            assert_eq!(view.lines(), ["acme-api", "delta", "py-demo"]);
            let shown = view.shown_projects();
            assert_eq!(
                shown[0],
                ShownProject {
                    initials: project_initials("acme-api"),
                    name: "acme-api".to_owned(),
                    path: "~/repo/acme-api".to_owned(),
                    branch: Some("dev".to_owned()),
                }
            );
            assert_eq!((shown[2].path.as_str(), shown[2].branch.as_deref()), ("/tmp/py-demo", None));
            assert_eq!(view.empty_text(), None);
            // Nothing clones or connects here: Open is the only button.
            assert_eq!(view.actions(cx), ["file.openProject"]);
        });
        assert!(search_has_the_keyboard(&view, cx));
        assert!(events.borrow().is_empty());

        click(cx, "welcome-action-file.openProject");
        assert_eq!(last(&events), Some(WelcomeEvent::RunAction("file.openProject")));
    }

    // src/features/welcome/welcome.test.tsx › Welcome › Projects ›
    // moves with arrows, filters by typing, opens with Enter and removes with Delete
    #[gpui_kit::test]
    async fn moves_with_arrows_filters_by_typing_opens_with_enter_and_removes_with_delete(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), Vec::new());
        assert_eq!(selected(&view, cx).as_deref(), Some("acme-api"));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&view, cx).as_deref(), Some("delta"));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), Some(WelcomeEvent::Open(PathBuf::from(DELTA))));

        // Delete asks first; nothing is removed until the answer.
        cx.simulate_keystrokes("delete");
        assert_eq!(
            cx.pending_prompt(),
            Some(("Remove Recent Project".to_owned(), "Remove 'delta' from the list of recent projects?".to_owned()))
        );
        assert_eq!(events.borrow().len(), 1);
        cx.simulate_prompt_answer("Remove");
        cx.run_until_parked();
        assert_eq!(last(&events), Some(WelcomeEvent::Remove(PathBuf::from(DELTA))));
        assert_eq!(lines(&view, cx), ["acme-api", "py-demo"]);

        cx.simulate_input("py");
        assert_eq!(lines(&view, cx), ["py-demo"]);
        assert_eq!(selected(&view, cx).as_deref(), Some("py-demo"));
    }

    // src/features/welcome/welcome.test.tsx › Welcome › Projects ›
    // offers Open in New Window in the row menu when the action exists
    #[gpui_kit::test]
    async fn offers_open_in_new_window_in_the_row_menu_when_the_action_exists(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), Vec::new());
        // The button at the end of the selected row asks for the row's menu and opens nothing.
        click(cx, "welcome-more-0");
        let Some(WelcomeEvent::ShowMenu { target, .. }) = last(&events) else { panic!("the row's menu is asked for") };
        assert_eq!(target, MenuTarget::Project(RecentEntry::from(&three()[0])));
        assert_eq!(events.borrow().len(), 1);

        let shown = |cx: &mut VisualTestContext| -> Vec<String> {
            cx.update(|window, cx| {
                let items = view.update(cx, |view, cx| view.menu(&target, cx));
                let label = |line: &Line<WelcomeCommand>| match line {
                    Line::Row(row) => Some(row.label.clone()),
                    Line::Separator => None,
                };
                resolve(&items, window, cx).iter().filter_map(label).collect()
            })
        };
        let without = shown(cx);
        assert!(!without.iter().any(|label| label == "Open in New Window"));
        assert_eq!(without.last().map(String::as_str), Some("Remove from Recent Projects…"));

        cx.update(|_, cx| {
            let meta = ActionMeta::new("file.openInNewWindow", "Open in New Window");
            cx.default_global::<ActionRegistry>().register(Action::new(meta, |_, _, _| Ok(())));
        });
        let with = shown(cx);
        assert_eq!(with[..2], ["Open", "Open in New Window"]);
        assert_eq!(with.last().map(String::as_str), Some("Remove from Recent Projects…"));
    }

    // src/features/welcome/welcome.test.tsx › Welcome › Projects › shows the landing page without recent projects
    #[gpui_kit::test]
    async fn shows_the_landing_page_without_recent_projects(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, Vec::new(), Vec::new());
        view.read_with(cx, |view, _| {
            assert_eq!(view.empty_text(), Some("Welcome to OpenIDEA"));
            assert!(view.lines().is_empty() && view.selected().is_none());
        });
        assert!(cx.debug_bounds("welcome-search").is_none(), "there is nothing to search");
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)), "the page has the keyboard");

        click(cx, "welcome-landing-file.openProject");
        assert_eq!(last(&events), Some(WelcomeEvent::RunAction("file.openProject")));

        // Once a project is recent, the list is shown and the keyboard is in its search field.
        view.update(cx, |view, cx| view.set_recent(&three(), &[], cx));
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["acme-api", "delta", "py-demo"]);
        assert!(cx.debug_bounds("welcome-search").is_some());
        assert!(search_has_the_keyboard(&view, cx));
    }

    #[gpui_kit::test]
    async fn the_buttons_follow_the_registered_actions(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| {
            let registry = cx.default_global::<ActionRegistry>();
            for (id, title) in [("vcs.clone", "Clone Repository…"), ("remote.connect", "Remote Development…")] {
                registry.register(Action::new(ActionMeta::new(id, title), |_, _, _| Ok(())));
            }
        });
        let (view, events, cx) = open(cx, three(), Vec::new());
        assert_eq!(
            view.read_with(cx, |view, cx| view.actions(cx)),
            ["file.openProject", "vcs.clone", "remote.connect"]
        );
        click(cx, "welcome-action-vcs.clone");
        assert_eq!(last(&events), Some(WelcomeEvent::RunAction("vcs.clone")));
        click(cx, "welcome-action-remote.connect");
        assert_eq!(last(&events), Some(WelcomeEvent::RunAction("remote.connect")));
    }

    #[gpui_kit::test]
    async fn a_click_opens_a_project_and_a_right_click_asks_for_its_menu(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), Vec::new());
        click(cx, "welcome-row-2");
        assert_eq!(selected(&view, cx).as_deref(), Some("py-demo"));
        assert_eq!(last(&events), Some(WelcomeEvent::Open(PathBuf::from(PY_DEMO))));
        assert!(search_has_the_keyboard(&view, cx), "a click leaves the keyboard in the search field");

        let at = center(cx, "welcome-row-1");
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        assert_eq!(selected(&view, cx).as_deref(), Some("delta"));
        let delta = MenuTarget::Project(RecentEntry::from(&three()[1]));
        assert_eq!(last(&events), Some(WelcomeEvent::ShowMenu { target: delta.clone(), position: at }));

        // Shift+F10 asks for the menu of the selected row, somewhere inside the list.
        cx.simulate_keystrokes("shift-f10");
        let Some(WelcomeEvent::ShowMenu { target, .. }) = last(&events) else { panic!("the row's menu is asked for") };
        assert_eq!(target, delta);
        assert_eq!(events.borrow().len(), 3);

        // The Manage button asks for the menu of the app's settings and help.
        click(cx, "welcome-manage");
        let Some(WelcomeEvent::ShowMenu { target, .. }) = last(&events) else { panic!("the menu is asked for") };
        assert_eq!(target, MenuTarget::Manage);
        let ids: Vec<Option<&str>> =
            view.update(cx, |view, cx| view.menu(&target, cx)).iter().map(MenuItem::action_id).collect();
        assert_eq!(ids.first(), Some(&Some("file.settings")));
        assert_eq!(ids.last(), Some(&Some("app.about")));
    }

    #[gpui_kit::test]
    async fn a_search_that_matches_nothing_says_so_and_can_be_cleared(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), Vec::new());
        cx.simulate_input("zzz");
        assert!(lines(&view, cx).is_empty());
        view.read_with(cx, |view, _| assert_eq!(view.empty_text(), Some("No matching projects")));
        // There is nothing to open, to remove or to move to.
        cx.simulate_keystrokes("enter delete down");
        assert!(events.borrow().is_empty() && !cx.has_pending_prompt());

        click(cx, "welcome-clear");
        assert_eq!(lines(&view, cx), ["acme-api", "delta", "py-demo"]);
        assert!(search_has_the_keyboard(&view, cx));

        // A path matches when no name does: the home directory is searched as it is shown.
        cx.simulate_input("repo");
        assert_eq!(lines(&view, cx), ["acme-api", "delta"]);
        // Delete inside the text edits the text; at its end it is about the selected project.
        cx.simulate_keystrokes("left delete");
        assert_eq!(view.read_with(cx, |view, cx| view.field().read(cx).text().to_owned()), "rep");
        assert!(!cx.has_pending_prompt());
        cx.simulate_keystrokes("delete");
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(events.borrow().is_empty());
        assert_eq!(lines(&view, cx), ["acme-api", "delta"]);
    }

    #[gpui_kit::test]
    async fn groups_list_their_projects_and_fold_with_the_keyboard_and_the_pointer(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), vec![group("Work", &[DELTA], true)]);
        assert_eq!(lines(&view, cx), ["[Work 1]", "  delta", "acme-api", "py-demo"]);
        assert_eq!(selected(&view, cx).as_deref(), Some("[Work 1]"));

        // ← folds the selected group and → unfolds it; Enter and a click do either.
        cx.simulate_keystrokes("left");
        assert_eq!(lines(&view, cx), ["[Work 1]", "acme-api", "py-demo"]);
        assert_eq!(last(&events), stored(&[group("Work", &[DELTA], false)]));
        cx.simulate_keystrokes("left");
        assert_eq!(events.borrow().len(), 1, "a folded group has nothing more to fold");
        cx.simulate_keystrokes("right");
        assert_eq!(last(&events), stored(&[group("Work", &[DELTA], true)]));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), stored(&[group("Work", &[DELTA], false)]));
        click(cx, "welcome-row-0");
        assert_eq!(last(&events), stored(&[group("Work", &[DELTA], true)]));
        assert_eq!(lines(&view, cx), ["[Work 1]", "  delta", "acme-api", "py-demo"]);

        // Delete does nothing on a group, and its menu is its own.
        cx.simulate_keystrokes("delete");
        assert!(!cx.has_pending_prompt());
        click(cx, "welcome-more-0");
        let Some(WelcomeEvent::ShowMenu { target, .. }) = last(&events) else { panic!("the menu is asked for") };
        assert_eq!(target, MenuTarget::Group("Work".to_owned()));
        let labels: Vec<String> = view
            .update(cx, |view, cx| view.menu(&target, cx))
            .iter()
            .map(|item| item.label().unwrap_or("-").to_owned())
            .collect();
        assert_eq!(labels, ["Rename Group…", "New Project Group…", "-", "Remove Group"]);

        // A search lists the matches only, out of their groups.
        cx.simulate_input("de");
        assert_eq!(lines(&view, cx), ["delta", "py-demo"]);
    }

    #[gpui_kit::test]
    async fn a_group_is_named_in_a_form_above_the_list(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), vec![group("Work", &[ACME], true)]);
        let run = |cx: &mut VisualTestContext, command: WelcomeCommand| {
            view.update_in(cx, |view, window, cx| view.run(command, window, cx));
            cx.run_until_parked();
        };
        let error = |cx: &mut VisualTestContext| view.update(cx, |view, cx| view.naming_error(cx));
        let title = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.naming_title());

        // Move to Group › New Group…: the name of another group is refused, as it is typed and on Enter.
        run(cx, WelcomeCommand::NewGroup { path: Some(DELTA.to_owned()) });
        assert_eq!(title(cx), Some("New Project Group"));
        assert!(!search_has_the_keyboard(&view, cx), "the keyboard is in the form");
        cx.simulate_keystrokes("enter");
        assert_eq!(error(cx).as_deref(), Some("Enter a group name"));
        cx.simulate_input("Work");
        assert_eq!(error(cx).as_deref(), Some("A group named 'Work' already exists"));
        cx.simulate_keystrokes("enter");
        assert!(events.borrow().is_empty() && title(cx).is_some());

        // A free name creates the group with the project in it, and the keyboard goes back.
        cx.simulate_input("shop");
        assert_eq!(error(cx), None);
        cx.simulate_keystrokes("enter");
        let groups = [group("Work", &[ACME], true), group("Workshop", &[DELTA], true)];
        assert_eq!(last(&events), stored(&groups));
        assert_eq!(lines(&view, cx), ["[Work 1]", "  acme-api", "[Workshop 1]", "  delta", "py-demo"]);
        assert_eq!(title(cx), None);
        assert!(search_has_the_keyboard(&view, cx));

        // Rename Group…: the form starts with the name selected, so typing replaces it.
        run(cx, WelcomeCommand::RenameGroup("Workshop".to_owned()));
        assert_eq!(title(cx), Some("Rename Project Group"));
        cx.simulate_input("Shop");
        click(cx, "welcome-naming-accept");
        assert_eq!(last(&events), stored(&[group("Work", &[ACME], true), group("Shop", &[DELTA], true)]));

        // Escape and Cancel give up: nothing is stored.
        let before = events.borrow().len();
        run(cx, WelcomeCommand::NewGroup { path: None });
        cx.simulate_input("Other");
        cx.simulate_keystrokes("escape");
        assert_eq!(title(cx), None);
        run(cx, WelcomeCommand::NewGroup { path: None });
        click(cx, "welcome-naming-cancel");
        assert_eq!((title(cx), events.borrow().len()), (None, before));
        assert!(search_has_the_keyboard(&view, cx));
    }

    #[gpui_kit::test]
    async fn the_rows_of_a_menu_open_show_copy_regroup_and_remove(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, three(), vec![group("Work", &[ACME], true), group("Oss", &[], true)]);
        let run = |cx: &mut VisualTestContext, command: WelcomeCommand| {
            view.update_in(cx, |view, window, cx| view.run(command, window, cx));
            cx.run_until_parked();
        };
        run(cx, WelcomeCommand::Open(ACME.to_owned()));
        assert_eq!(last(&events), Some(WelcomeEvent::Open(PathBuf::from(ACME))));
        run(cx, WelcomeCommand::Reveal(ACME.to_owned()));
        assert_eq!(last(&events), Some(WelcomeEvent::Reveal(PathBuf::from(ACME))));
        run(cx, WelcomeCommand::CopyPath(ACME.to_owned()));
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(ACME));
        assert_eq!(events.borrow().len(), 2);

        run(cx, WelcomeCommand::MoveToGroup { path: ACME.to_owned(), group: Some("Oss".to_owned()) });
        assert_eq!(last(&events), stored(&[group("Work", &[], true), group("Oss", &[ACME], true)]));
        assert_eq!(lines(&view, cx), ["[Work 0]", "[Oss 1]", "  acme-api", "delta", "py-demo"]);
        run(cx, WelcomeCommand::MoveToGroup { path: ACME.to_owned(), group: None });
        assert_eq!(last(&events), stored(&[group("Work", &[], true), group("Oss", &[], true)]));
        run(cx, WelcomeCommand::RemoveGroup("Work".to_owned()));
        assert_eq!(last(&events), stored(&[group("Oss", &[], true)]));
        assert_eq!(lines(&view, cx), ["[Oss 0]", "acme-api", "delta", "py-demo"]);

        // Remove from Recent Projects… asks; a removed project leaves its group too.
        run(cx, WelcomeCommand::MoveToGroup { path: DELTA.to_owned(), group: Some("Oss".to_owned()) });
        run(cx, WelcomeCommand::AskRemove(DELTA.to_owned()));
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Remove");
        cx.run_until_parked();
        assert_eq!(last(&events), Some(WelcomeEvent::Remove(PathBuf::from(DELTA))));
        assert_eq!(lines(&view, cx), ["[Oss 0]", "acme-api", "py-demo"]);
    }

    #[gpui_kit::test]
    async fn a_new_list_keeps_the_search_and_the_selection_in_range(cx: &mut TestAppContext) {
        init(cx);
        let (view, _, cx) = open(cx, three(), Vec::new());
        cx.simulate_keystrokes("end");
        assert_eq!(selected(&view, cx).as_deref(), Some("py-demo"));
        view.update(cx, |view, cx| view.set_recent(&three()[..2], &[], cx));
        assert_eq!(selected(&view, cx).as_deref(), Some("delta"));

        cx.simulate_input("ac");
        view.update(cx, |view, cx| view.set_recent(&three(), &[group("Work", &[ACME], true)], cx));
        assert_eq!(lines(&view, cx), ["acme-api"]);

        // The last project gone: the page without recent projects takes the keyboard.
        view.update(cx, |view, cx| view.set_recent(&[], &[], cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert_eq!(view.empty_text(), Some("Welcome to OpenIDEA")));
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)));
    }
}
