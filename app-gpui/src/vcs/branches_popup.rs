//! The Git Branches popup: a search over the Git actions and the branches and tags of the
//! repository, in collapsible Recent / Local / Remote / Tags groups with folders by `/`, the
//! current branch and the favourites first. With several repositories it lists them, each opening
//! its own branches, and the branches they have in common (or the current repository's).
//!
//! [`crate::vcs::branches`] decides the rows and [`crate::vcs_ops::branch_menu`] the menu of a
//! branch; this view keeps the query, the selection and the open submenu, and tells the workbench
//! what was chosen. It reads no repository: the workbench hands it the state with
//! [`BranchesPopup::set_input`] and carries out what [`BranchesPopupEvent`] asks.
//!
//! The keyboard stays in the search field: ↑ ↓ move over the rows, → at the end of the query opens
//! a branch's submenu, a group or a repository, ← at its start closes the submenu, a group or the
//! repository, Enter chooses and Escape closes the submenu, then the popup.

use super::branches::{
    self, BranchRow, BranchRowsInput, BranchTreeOptions, CommonBranch, Matches, MultiRepoInput, PopupAction, RepoEntry,
    RepoRow, build_branch_rows, build_multi_repo_rows, repo_head_label, with_expanded,
};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::matching::index_runs;
use crate::ui::popup_menu::{Choice, Line as MenuLine, resolve};
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::branch_menu::{BranchCommand, branch_menu};
use crate::vcs_ops::branch_widget::operation_actions;
use crate::vcs_ops::menu::MenuItem;
use crate::vcs_ops::operations::in_progress;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton,
    Render, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window, deferred, div, prelude::*, px,
    uniform_list,
};
use ide_vcs::{Branch, BranchInfo};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::rc::Rc;

/// Width of the popup with one repository, and with several.
const WIDTH: f32 = 380.0;
const WIDE: f32 = 440.0;
const ROW_HEIGHT: f32 = 24.0;
/// The list is at most this high, and at least three rows.
const MAX_LIST_HEIGHT: f32 = 440.0;
const INDENT: f32 = 16.0;
const SUBMENU_WIDTH: f32 = 300.0;

/// The actions on top of the popup of one repository, and of a project with several.
fn popup_actions() -> Vec<PopupAction> {
    let action = |id: &str, label: &str, icon: &str, group: &str| PopupAction {
        id: id.to_owned(),
        label: label.to_owned(),
        icon: Some(icon.to_owned()),
        group: Some(group.to_owned()),
        ..PopupAction::default()
    };
    vec![
        action("vcs.update", "Update Project…", "git-pull", "sync"),
        action("vcs.commit", "Commit…", "commit", "sync"),
        action("vcs.push", "Push…", "git-push", "sync"),
        action("vcs.newBranch", "New Branch…", "plus", "branch"),
        action("vcs.checkoutRevision", "Checkout Tag or Revision…", "branch", "branch"),
    ]
}

/// The actions of one repository of several, on top of its own branches.
fn repo_actions() -> Vec<PopupAction> {
    popup_actions().into_iter().filter(|action| action.group.as_deref() == Some("branch")).collect()
}

/// The repositories a command acts in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoTarget {
    /// The selected repository, as it is.
    Selected,
    /// This repository, selected first.
    Repo(String),
    /// Every one of these repositories (a branch they all have).
    All(Vec<String>),
}

/// A row of a branch's submenu as the workbench carries it out.
#[derive(Clone, Debug)]
pub struct BranchesCommand {
    pub target: RepoTarget,
    pub command: BranchCommand,
}

/// What the popup asks of the workbench.
#[derive(Clone, Debug)]
pub enum BranchesPopupEvent {
    /// Escape, or a click beside the popup.
    Dismissed,
    /// Run action `id` with `arg`, with repository `repo` selected first when given. `close`: the
    /// popup is done (an action row); Fetch leaves it open.
    RunAction { id: String, arg: Option<Value>, repo: Option<String>, close: bool },
    /// Carry out a row of a branch's submenu. The popup is done.
    Command(BranchesCommand),
    /// The star of a branch was clicked: add it to the favourites or remove it. The popup stays.
    ToggleFavorite { target: RepoTarget, branch: Branch },
    /// The gear options or the groups opened in the popup of one repository changed: remember them.
    OptionsChanged(BranchTreeOptions),
    /// The groups opened in the popup of several repositories changed: remember them.
    MultiExpansionChanged(HashMap<String, bool>),
}

/// What the popup shows. The workbench builds it from the version control state and hands it in
/// again whenever that changes.
#[derive(Clone, Debug, Default)]
pub struct BranchesPopupInput {
    /// The selected repository's root; `None` is the project's primary one.
    pub repo: Option<String>,
    /// HEAD of the selected repository: its operation in progress puts Continue / Abort on top.
    pub branch: Option<BranchInfo>,
    /// The selected repository's branches.
    pub branches: Vec<Branch>,
    /// `branches` reflects the repository (false while they are listed).
    pub loaded: bool,
    /// Recently checked-out branches, newest first.
    pub recent: Vec<String>,
    /// The user's favourite branches; `None` until they edit them.
    pub favorites: Option<Vec<String>>,
    /// Tags of the current repository.
    pub tags: Vec<String>,
    /// Files left with conflicts: Resolve Conflicts goes on top.
    pub conflicts: usize,
    /// Actions other features add to the top ([`crate::vcs_ops::branch_widget_extensions`]).
    pub extension_actions: Vec<PopupAction>,
    /// The gear options, with the groups the user opened or closed in the popup of one repository.
    pub options: BranchTreeOptions,
    /// The groups the user opened or closed in the popup of several repositories.
    pub multi_expanded: HashMap<String, bool>,
    /// The project's repositories with their HEAD; the popup lists them when there are several.
    pub repos: Vec<RepoEntry>,
    /// Root → its branches, for every repository (missing while loading).
    pub repo_branches: HashMap<String, Vec<Branch>>,
    /// Root → the operation it is stopped in.
    pub operations: BTreeMap<String, String>,
    /// The current repository: the one of the active editor's file ([`branches::current_repo_for`]).
    pub current: Option<String>,
}

#[derive(Clone, Debug)]
struct BranchLine {
    key: String,
    target: RepoTarget,
    /// The current branch of the branch's repository (`None` on a detached HEAD).
    head: Option<String>,
    branch: Branch,
    label: String,
    depth: usize,
    matches: Matches,
    favorite: bool,
}

#[derive(Clone, Debug)]
struct RepoLine {
    key: String,
    repo: RepoEntry,
    color: usize,
    matches: Matches,
    operation: Option<String>,
    head: String,
}

#[derive(Clone, Debug)]
struct CommonLine {
    key: String,
    branch: CommonBranch,
    label: String,
    depth: usize,
    matches: Matches,
    /// The branch as the first repository has it, for its menu.
    full: Option<Branch>,
    head: Option<String>,
    roots: Vec<String>,
}

/// A row of the popup.
#[derive(Clone, Debug)]
enum Line {
    Separator { key: String },
    Section { key: String, label: String },
    Action { key: String, action: Box<PopupAction>, matches: Matches, repo: Option<String> },
    Group { key: String, label: String, expanded: bool, depth: usize },
    Repo(Box<RepoLine>),
    Branch(Box<BranchLine>),
    Common(Box<CommonLine>),
    Tag { key: String, tag: String, depth: usize, matches: Matches },
}

impl Line {
    fn key(&self) -> &str {
        match self {
            Self::Separator { key }
            | Self::Section { key, .. }
            | Self::Action { key, .. }
            | Self::Group { key, .. }
            | Self::Tag { key, .. } => key,
            Self::Repo(line) => &line.key,
            Self::Branch(line) => &line.key,
            Self::Common(line) => &line.key,
        }
    }

    fn selectable(&self) -> bool {
        !matches!(self, Self::Separator { .. } | Self::Section { .. })
    }

    /// The row as text: indented by depth, a group with `v` (open) or `>`, a branch with its marks.
    fn text(&self, i18n: &I18n) -> String {
        let indent = |depth: usize| "  ".repeat(depth);
        match self {
            Self::Separator { .. } => "-".to_owned(),
            Self::Section { label, .. } => format!("[{}]", i18n.tx(label)),
            Self::Action { action, .. } => i18n.tx(&action.label),
            Self::Group { label, expanded, depth, .. } => {
                let shown = if *depth == 0 { i18n.tx(label) } else { label.clone() };
                format!("{}{} {shown}", indent(*depth), if *expanded { "v" } else { ">" })
            }
            Self::Repo(line) => {
                let operation = line.operation.as_ref().map(|op| format!(" ({op})")).unwrap_or_default();
                format!("{} {}{operation}", line.repo.name, line.head)
            }
            Self::Branch(line) => {
                let branch = &line.branch;
                let mut text = format!("{}{}", indent(line.depth), line.label);
                if branch.current {
                    text.push_str(" [current]");
                } else if line.favorite {
                    text.push_str(" ★");
                }
                if branch.behind > 0 {
                    text.push_str(&format!(" ↓{}", branch.behind));
                }
                if branch.ahead > 0 {
                    text.push_str(&format!(" ↑{}", branch.ahead));
                }
                if let Some(upstream) = branch.upstream.as_deref().filter(|up| !branch.remote && !up.is_empty()) {
                    text.push_str(&format!(" {upstream}"));
                }
                text
            }
            Self::Common(line) => {
                let current = if line.branch.current { " [current]" } else { "" };
                format!("{}{}{current}", indent(line.depth), line.label)
            }
            Self::Tag { tag, depth, .. } => format!("{}{tag}", indent(*depth)),
        }
    }
}

/// The open submenu of a branch row.
struct Submenu {
    /// The key of the row it belongs to.
    key: String,
    lines: Vec<MenuLine<BranchesCommand>>,
    active: Option<usize>,
}

impl Submenu {
    fn selectable(&self, index: usize) -> bool {
        matches!(self.lines.get(index), Some(MenuLine::Row(row)) if !row.disabled)
    }

    /// The next selectable row `delta` steps from the selection, going round.
    fn step(&mut self, delta: isize) {
        let count = self.lines.len() as isize;
        let mut at = self.active.map_or(if delta > 0 { -1 } else { 0 }, |at| at as isize);
        for _ in 0..count {
            at = (at + delta).rem_euclid(count);
            if self.selectable(at as usize) {
                self.active = Some(at as usize);
                return;
            }
        }
    }
}

/// What a frame draws of the submenu: its rows and the selected one.
#[derive(Clone)]
struct SubmenuSnapshot {
    key: String,
    lines: Vec<MenuLine<BranchesCommand>>,
    active: Option<usize>,
}

/// The gear menu's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopupOption {
    GroupByPrefix,
    ShowRecent,
    ShowTags,
}

impl PopupOption {
    fn label(self) -> &'static str {
        match self {
            Self::GroupByPrefix => "Group by Prefix",
            Self::ShowRecent => "Show Recent Branches",
            Self::ShowTags => "Show Tags",
        }
    }
}

pub struct BranchesPopup {
    field: Entity<TextField>,
    input: BranchesPopupInput,
    /// The repository of several whose own branches are shown (→ on its row).
    scope: Option<String>,
    lines: Rc<Vec<Line>>,
    active: Option<usize>,
    submenu: Option<Submenu>,
    options_open: bool,
    code_font: SharedString,
    scroll: UniformListScrollHandle,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<BranchesPopupEvent> for BranchesPopup {}

/// The first font of the theme's `--font-mono` list that is installed: branch names are code.
fn code_font(cx: &App) -> SharedString {
    crate::theme::installed_fonts::mono_family(cx)
}

/// The slim branches the row models read.
fn slim(list: &[Branch]) -> Vec<branches::Branch> {
    list.iter()
        .map(|branch| branches::Branch { name: branch.name.clone(), remote: branch.remote, current: branch.current })
        .collect()
}

/// The full branch of `list` a row stands for.
fn full_branch(list: &[Branch], row: &branches::Branch) -> Option<Branch> {
    list.iter().find(|branch| branch.name == row.name && branch.remote == row.remote).cloned()
}

/// `text` cut into the pieces to draw, each with whether it is matched (`indices` are byte offsets).
fn highlighted(text: &str, indices: &[usize]) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut at = 0;
    for run in index_runs(text, indices) {
        if run.start < at {
            continue;
        }
        if run.start > at {
            parts.push((text[at..run.start].to_owned(), false));
        }
        at = run.end;
        parts.push((text[run].to_owned(), true));
    }
    if at < text.len() {
        parts.push((text[at..].to_owned(), false));
    }
    parts
}

/// `text` with its matched characters in the accent colour.
fn marked_text(cx: &App, text: &str, matches: &Matches) -> Div {
    let accent = theme::color(cx, "accent");
    let parts = match matches {
        Some(indices) if !indices.is_empty() => highlighted(text, indices),
        _ => vec![(text.to_owned(), false)],
    };
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(parts.into_iter().map(|(part, matched)| {
        let part = div().child(SharedString::from(part));
        if matched { part.text_color(accent) } else { part }
    }))
}

/// The incoming and outgoing markers of a branch, with their counts.
fn sync_marks(cx: &App, behind: u32, ahead: u32, font: &SharedString) -> Option<Div> {
    if behind == 0 && ahead == 0 {
        return None;
    }
    let mark = |icon: &str, color: &str, count: u32| {
        let color = theme::color(cx, color);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(1.0))
            .text_color(color)
            .child(theme::icon(cx, icon, 12.0, color))
            .child(SharedString::from(count.to_string()))
    };
    Some(
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(3.0))
            .font_family(font.clone())
            .text_size(theme::length(cx, "fs-small"))
            .when(behind > 0, |el| el.child(mark("vcs-incoming", "accent", behind)))
            .when(ahead > 0, |el| el.child(mark("vcs-outgoing", "success", ahead))),
    )
}

impl BranchesPopup {
    /// The popup over `input`, with the keyboard in its search field.
    pub fn new(input: BranchesPopupInput, cx: &mut Context<Self>) -> Self {
        let field = cx.new(TextField::new);
        let field_events = cx.subscribe(&field, |popup: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => popup.query_changed(cx),
            // Enter and Escape are the popup's own keys: they are claimed before the field sees them.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(BranchesPopupEvent::Dismissed),
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |popup, cx| popup.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        let mut popup = Self {
            field,
            input,
            scope: None,
            lines: Rc::default(),
            active: None,
            submenu: None,
            options_open: false,
            code_font: code_font(cx),
            scroll: UniformListScrollHandle::new(),
            key_claim,
            _field_events: field_events,
        };
        popup.set_placeholder(cx);
        popup.rebuild(None, cx);
        popup
    }

    /// Shows the state again: after a refresh, a favourite toggled, the branches listed.
    pub fn set_input(&mut self, input: BranchesPopupInput, cx: &mut Context<Self>) {
        self.input = input;
        if self.scope.as_ref().is_some_and(|root| !self.input.repos.iter().any(|repo| &repo.root == root)) {
            self.scope = None;
            self.set_placeholder(cx);
        }
        let keep = self.active.and_then(|at| self.lines.get(at)).map(|line| line.key().to_owned());
        self.rebuild(keep.as_deref(), cx);
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The rows as text, as [`Line::text`] writes them.
    pub fn rows(&self, cx: &App) -> Vec<String> {
        let fallback;
        let i18n = match cx.try_global::<Localization>() {
            Some(localization) => &localization.0,
            None => {
                fallback = I18n::default();
                &fallback
            }
        };
        self.lines.iter().map(|line| line.text(i18n)).collect()
    }

    /// The selected row, as [`Self::rows`] writes it.
    pub fn selected(&self, cx: &App) -> Option<String> {
        self.rows(cx).into_iter().nth(self.active?)
    }

    /// The open submenu: each row's label, `> ` before the selected one, `-` for a separator.
    pub fn submenu_rows(&self) -> Vec<String> {
        let Some(submenu) = &self.submenu else { return Vec::new() };
        submenu
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| match line {
                MenuLine::Separator => "-".to_owned(),
                MenuLine::Row(row) if submenu.active == Some(index) => format!("> {}", row.label),
                MenuLine::Row(row) => row.label.clone(),
            })
            .collect()
    }

    /// The repository of several whose own branches are shown.
    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    /// What the list says, in English, when it shows nothing.
    pub fn empty_text(&self) -> Option<&'static str> {
        if !self.lines.is_empty() {
            None
        } else if self.multi() {
            Some("No matching repositories or branches")
        } else if self.loaded() {
            Some("No matching branches")
        } else {
            Some("Loading…")
        }
    }

    /// The popup lists the repositories of a project with several.
    fn multi(&self) -> bool {
        self.input.repos.len() > 1 && self.scope.is_none()
    }

    fn loaded(&self) -> bool {
        match &self.scope {
            Some(root) => self.input.repo_branches.contains_key(root),
            None => self.input.loaded,
        }
    }

    fn roots(&self) -> Vec<String> {
        self.input.repos.iter().map(|repo| repo.root.clone()).collect()
    }

    fn repo_name(&self, root: &str) -> String {
        self.input.repos.iter().find(|repo| repo.root == root).map_or_else(|| root.to_owned(), |repo| repo.name.clone())
    }

    fn query(&self, cx: &App) -> String {
        self.field.read(cx).text().to_owned()
    }

    fn set_placeholder(&mut self, cx: &mut Context<Self>) {
        let i18n = &mut cx.default_global::<Localization>().0;
        let placeholder = match &self.scope {
            Some(root) => {
                let name = self.repo_name(root);
                i18n.tx_built(&["Search branches of ", ""], &[Param::from(name.as_str())])
            }
            None if self.input.repos.len() > 1 => i18n.tx("Search for branches, actions, repositories"),
            None => i18n.tx("Search branches and actions"),
        };
        self.field.update(cx, |field, cx| {
            *field = std::mem::replace(field, TextField::new(cx)).with_placeholder(placeholder);
        });
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.submenu = None;
        self.rebuild(None, cx);
    }

    /// Builds the rows again; the selection stays on the row with key `keep`, else goes to the first
    /// row that can be chosen.
    fn rebuild(&mut self, keep: Option<&str>, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let lines = if self.multi() { self.multi_lines(&query, cx) } else { self.single_lines(&query, cx) };
        let kept = keep.and_then(|key| lines.iter().position(|line| line.key() == key));
        self.active = kept.or_else(|| lines.iter().position(Line::selectable));
        if self.submenu.as_ref().is_some_and(|submenu| !lines.iter().any(|line| line.key() == submenu.key)) {
            self.submenu = None;
        }
        self.lines = Rc::new(lines);
        if let Some(at) = self.active {
            self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// The rows of one repository: the selected one, or the repository of several opened with →.
    fn single_lines(&self, query: &str, cx: &mut Context<Self>) -> Vec<Line> {
        let input = &self.input;
        let (list, target, head, op) = match &self.scope {
            Some(root) => {
                let list = input.repo_branches.get(root).map(Vec::as_slice).unwrap_or_default();
                let repo = input.repos.iter().find(|repo| &repo.root == root);
                let head = repo.and_then(|repo| repo.head.clone());
                let op = in_progress(input.operations.get(root).map(String::as_str));
                (list, RepoTarget::Repo(root.clone()), head, op)
            }
            None => {
                let head = input.branch.as_ref().and_then(|branch| branch.head.clone());
                let op = in_progress(input.branch.as_ref().and_then(|branch| branch.operation.as_deref()));
                (input.branches.as_slice(), RepoTarget::Selected, head, op)
            }
        };
        let mut actions = operation_actions(op, input.conflicts);
        match &self.scope {
            Some(_) => actions.extend(repo_actions()),
            None => {
                actions.extend(popup_actions());
                actions.extend(input.extension_actions.iter().cloned());
            }
        }
        let slim_list = slim(list);
        let tags: &[String] = if self.scope.is_none() { &input.tags } else { &[] };
        let rows_input = BranchRowsInput {
            branches: &slim_list,
            recent: &input.recent,
            query,
            actions: &actions,
            favorites: input.favorites.as_deref(),
            tags,
            options: input.options.clone(),
        };
        let i18n = &cx.default_global::<Localization>().0;
        let action_repo = self.scope.clone();
        build_branch_rows(&rows_input, i18n)
            .into_iter()
            .filter_map(|row| match row {
                BranchRow::Section { key, label } if label.is_empty() => Some(Line::Separator { key }),
                BranchRow::Section { key, label } => Some(Line::Section { key, label }),
                BranchRow::Action { key, action, matches } => {
                    Some(Line::Action { key, action: Box::new(action), matches, repo: action_repo.clone() })
                }
                BranchRow::Group { key, label, expanded, depth } => Some(Line::Group { key, label, expanded, depth }),
                BranchRow::Branch { key, branch, label, depth, matches, favorite, .. } => {
                    let branch = full_branch(list, &branch)?;
                    let (target, head) = (target.clone(), head.clone());
                    let line = BranchLine { key, target, head, branch, label, depth, matches, favorite };
                    Some(Line::Branch(Box::new(line)))
                }
                BranchRow::Tag { key, tag, depth, matches } => Some(Line::Tag { key, tag, depth, matches }),
            })
            .collect()
    }

    /// The rows of a project with several repositories.
    fn multi_lines(&self, query: &str, cx: &mut Context<Self>) -> Vec<Line> {
        let input = &self.input;
        let roots = self.roots();
        let op = in_progress(input.branch.as_ref().and_then(|branch| branch.operation.as_deref()));
        let mut actions = operation_actions(op, input.conflicts);
        // New Branch of a project with several repositories creates the branch in each of them.
        let in_each = json!({ "repos": roots });
        actions.extend(popup_actions().into_iter().map(|action| match action.id.as_str() {
            "vcs.newBranch" => PopupAction { arg: Some(in_each.clone()), ..action },
            _ => action,
        }));
        actions.extend(input.extension_actions.iter().cloned());
        let slim_lists: HashMap<String, Vec<branches::Branch>> =
            input.repo_branches.iter().map(|(root, list)| (root.clone(), slim(list))).collect();
        let options = &input.options;
        let rows_input = MultiRepoInput {
            repos: &input.repos,
            branches: &slim_lists,
            query,
            actions: &actions,
            expanded: &input.multi_expanded,
            favorites: input.favorites.as_deref(),
            current: input.current.as_deref(),
            recent: &input.recent,
            tags: &input.tags,
            group_by_prefix: options.group_by_prefix,
            show_recent: options.show_recent,
            show_tags: options.show_tags,
        };
        let i18n = &mut cx.default_global::<Localization>().0;
        let rows = build_multi_repo_rows(&rows_input, i18n);
        let first = input.repos.first();
        let first_list =
            first.and_then(|repo| input.repo_branches.get(&repo.root)).map(Vec::as_slice).unwrap_or_default();
        rows.into_iter()
            .filter_map(|row| match row {
                RepoRow::Section { key, label } if label.is_empty() => Some(Line::Separator { key }),
                RepoRow::Section { key, label } => Some(Line::Section { key, label }),
                RepoRow::Action { key, action, matches } => {
                    Some(Line::Action { key, action: Box::new(action), matches, repo: None })
                }
                RepoRow::Group { key, label, expanded, depth } => Some(Line::Group { key, label, expanded, depth }),
                RepoRow::Repo { key, repo, color, matches } => {
                    let operation = input.operations.get(&repo.root).filter(|op| !op.is_empty()).cloned();
                    let head = repo_head_label(&repo, i18n);
                    Some(Line::Repo(Box::new(RepoLine { key, repo, color, matches, operation, head })))
                }
                RepoRow::Common { key, branch, label, depth, matches } => {
                    let slim_branch =
                        branches::Branch { name: branch.name.clone(), remote: branch.remote, current: branch.current };
                    let full =
                        full_branch(first_list, &slim_branch).map(|full| Branch { current: branch.current, ..full });
                    let head = first.and_then(|repo| repo.head.clone());
                    let roots = roots.clone();
                    Some(Line::Common(Box::new(CommonLine { key, branch, label, depth, matches, full, head, roots })))
                }
                RepoRow::Branch { key, repo, branch, label, depth, matches, favorite, .. } => {
                    let list = input.repo_branches.get(&repo.root).map(Vec::as_slice).unwrap_or_default();
                    let branch = full_branch(list, &branch)?;
                    let target = RepoTarget::Repo(repo.root.clone());
                    let head = repo.head.clone();
                    let line = BranchLine { key, target, head, branch, label, depth, matches, favorite };
                    Some(Line::Branch(Box::new(line)))
                }
                RepoRow::Tag { key, tag, depth, matches, .. } => Some(Line::Tag { key, tag, depth, matches }),
            })
            .collect()
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.lines.get(index).is_some_and(Line::selectable) {
            self.active = Some(index);
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Moves the selection `delta` rows over the separators and section headers, going round.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let selectable: Vec<usize> = (0..self.lines.len()).filter(|&at| self.lines[at].selectable()).collect();
        let Some(last) = selectable.len().checked_sub(1) else { return };
        let at = self.active.and_then(|active| selectable.iter().position(|&index| index == active));
        let next = match at {
            None => 0,
            Some(0) if delta < 0 => last,
            Some(at) if at == last && delta > 0 => 0,
            Some(at) => at.saturating_add_signed(delta).min(last),
        };
        self.select(selectable[next], cx);
    }

    /// Opens or closes group `key`; while searching every group is open.
    fn set_group(&mut self, key: &str, open: bool, cx: &mut Context<Self>) {
        if !self.query(cx).trim().is_empty() {
            return;
        }
        if self.multi() {
            self.input.multi_expanded = with_expanded(&self.input.multi_expanded, key, open);
            cx.emit(BranchesPopupEvent::MultiExpansionChanged(self.input.multi_expanded.clone()));
        } else {
            self.input.options.expanded = with_expanded(&self.input.options.expanded, key, open);
            cx.emit(BranchesPopupEvent::OptionsChanged(self.input.options.clone()));
        }
        self.rebuild(Some(key), cx);
    }

    /// The menu of a branch row, in the vocabulary of the popup.
    fn menu_of(&self, line: &Line, cx: &mut Context<Self>) -> Option<Vec<MenuItem<BranchesCommand>>> {
        let i18n = &mut cx.default_global::<Localization>().0;
        let (items, target) = match line {
            Line::Branch(line) => {
                let items = branch_menu(&line.branch, line.head.as_deref(), &[], line.favorite, i18n);
                (items, line.target.clone())
            }
            Line::Common(line) => {
                let full = line.full.as_ref()?;
                let items = branch_menu(full, line.head.as_deref(), &[], line.branch.favorite, i18n);
                (items, RepoTarget::All(line.roots.clone()))
            }
            _ => return None,
        };
        let wrap = move |command: BranchCommand| BranchesCommand { target: target.clone(), command };
        Some(items.into_iter().map(|item| item.map(&wrap)).collect())
    }

    /// Opens the submenu of row `index` beside it.
    fn open_submenu(&mut self, index: usize, window: &Window, cx: &mut Context<Self>) {
        let lines = Rc::clone(&self.lines);
        let Some(line) = lines.get(index) else { return };
        let Some(items) = self.menu_of(line, cx) else { return };
        let menu_lines = resolve(&items, window, cx);
        let mut submenu = Submenu { key: line.key().to_owned(), lines: menu_lines, active: None };
        submenu.step(1);
        self.active = Some(index);
        self.submenu = Some(submenu);
        self.options_open = false;
        cx.notify();
    }

    /// Opens the branches of repository `root` in the popup's place; ← at the start of the query goes
    /// back.
    fn enter_repo(&mut self, root: &str, cx: &mut Context<Self>) {
        self.scope = Some(root.to_owned());
        self.submenu = None;
        self.field.update(cx, |field, cx| field.set_text("", cx));
        self.set_placeholder(cx);
        self.rebuild(None, cx);
    }

    fn leave_repo(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.scope.take() else { return };
        self.field.update(cx, |field, cx| field.set_text("", cx));
        self.set_placeholder(cx);
        self.rebuild(Some(&format!("repo:{root}")), cx);
    }

    /// Enter on row `index`, or a click: an action runs, a group opens or closes, a branch opens its
    /// submenu, a repository its branches.
    fn activate(&mut self, index: usize, window: &Window, cx: &mut Context<Self>) {
        let lines = Rc::clone(&self.lines);
        let Some(line) = lines.get(index) else { return };
        match line {
            Line::Action { action, repo, .. } => cx.emit(BranchesPopupEvent::RunAction {
                id: action.id.clone(),
                arg: action.arg.clone(),
                repo: repo.clone(),
                close: true,
            }),
            Line::Group { key, expanded, .. } => self.set_group(key, !expanded, cx),
            Line::Branch(_) | Line::Common(_) => self.open_submenu(index, window, cx),
            Line::Repo(line) => self.enter_repo(&line.repo.root, cx),
            Line::Tag { .. } | Line::Separator { .. } | Line::Section { .. } => {}
        }
    }

    /// Chooses the selected row of the submenu.
    fn choose_submenu(&mut self, cx: &mut Context<Self>) {
        let Some(submenu) = &self.submenu else { return };
        let Some(MenuLine::Row(row)) = submenu.active.and_then(|at| submenu.lines.get(at)) else { return };
        if row.disabled {
            return;
        }
        let Some(choice) = row.choice.clone() else { return };
        let event = match choice {
            Choice::Command(BranchesCommand { target, command: BranchCommand::Action { id, arg } }) => {
                let repo = match target {
                    RepoTarget::Repo(root) => Some(root),
                    RepoTarget::Selected | RepoTarget::All(_) => None,
                };
                BranchesPopupEvent::RunAction { id: id.to_owned(), arg, repo, close: true }
            }
            Choice::Command(command) => BranchesPopupEvent::Command(command),
            Choice::Action { id, arg } => BranchesPopupEvent::RunAction { id, arg, repo: None, close: true },
        };
        self.submenu = None;
        cx.emit(event);
        cx.notify();
    }

    /// The star of a branch row was clicked.
    fn toggle_favorite(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Line::Branch(line)) = self.lines.get(index) else { return };
        cx.emit(BranchesPopupEvent::ToggleFavorite { target: line.target.clone(), branch: line.branch.clone() });
    }

    fn toggle_option(&mut self, option: PopupOption, cx: &mut Context<Self>) {
        let options = &mut self.input.options;
        match option {
            PopupOption::GroupByPrefix => options.group_by_prefix = !options.group_by_prefix,
            PopupOption::ShowRecent => options.show_recent = !options.show_recent,
            PopupOption::ShowTags => options.show_tags = !options.show_tags,
        }
        cx.emit(BranchesPopupEvent::OptionsChanged(self.input.options.clone()));
        let keep = self.active.and_then(|at| self.lines.get(at)).map(|line| line.key().to_owned());
        self.rebuild(keep.as_deref(), cx);
    }

    /// Whether the caret of the query is at its end (`end`) or its start, with nothing selected.
    fn caret_at(&self, end: bool, cx: &App) -> bool {
        let field = self.field.read(cx);
        let selection = field.selection();
        selection.is_empty() && if end { selection.end == field.text().len() } else { selection.start == 0 }
    }

    /// The keys the popup handles itself while the keyboard is in its search field.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return false;
        }
        let key = keystroke.key.as_str();
        if let Some(submenu) = &mut self.submenu {
            match key {
                "up" => submenu.step(-1),
                "down" => submenu.step(1),
                "enter" => self.choose_submenu(cx),
                "left" | "escape" => self.submenu = None,
                // The submenu has no deeper level: → stays on it.
                "right" => {}
                _ => return false,
            }
            cx.notify();
            return true;
        }
        let row = self.active.and_then(|at| self.lines.get(at)).cloned();
        match key {
            "up" | "down" if modifiers.shift => return false,
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "enter" => {
                if let Some(at) = self.active {
                    self.activate(at, window, cx);
                }
            }
            "escape" if self.options_open => {
                self.options_open = false;
                cx.notify();
            }
            "escape" => cx.emit(BranchesPopupEvent::Dismissed),
            "right" if !modifiers.shift && self.caret_at(true, cx) => match (row, self.active) {
                (Some(Line::Group { key, .. }), _) => self.set_group(&key, true, cx),
                (Some(Line::Branch(_) | Line::Common(_)), Some(at)) => self.open_submenu(at, window, cx),
                (Some(Line::Repo(line)), _) => self.enter_repo(&line.repo.root, cx),
                _ => return false,
            },
            "left" if !modifiers.shift && self.caret_at(false, cx) => match row {
                Some(Line::Group { key, expanded: true, .. }) if self.query(cx).trim().is_empty() => {
                    self.set_group(&key, false, cx)
                }
                _ if self.scope.is_some() => self.leave_repo(cx),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    fn render_line(&self, index: usize, line: &Line, active: bool, cx: &mut Context<Self>) -> Div {
        let (fg, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let (subtle, faint) = (theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let small = theme::length(cx, "fs-small");
        let font = self.code_font.clone();
        let chevron = |cx: &App| theme::icon(cx, "chevron-right", 10.0, faint);
        let content = div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .rounded(px(5.0))
            .whitespace_nowrap()
            .overflow_hidden()
            .text_color(fg)
            .when(active, |el| el.bg(theme::color(cx, "bg-tree-selected")));
        let indent = |depth: usize| px(8.0 + INDENT * depth as f32);
        match line {
            Line::Separator { .. } => div()
                .size_full()
                .flex()
                .items_center()
                .px(px(4.0))
                .child(div().flex_1().h(px(1.0)).bg(theme::color(cx, "border-popup"))),
            Line::Section { label, .. } => {
                let label = cx.default_global::<Localization>().0.tx(label);
                content.px(px(10.0)).text_color(subtle).text_size(small).child(SharedString::from(label))
            }
            Line::Action { action, matches, .. } => {
                let label = cx.default_global::<Localization>().0.tx(&action.label);
                let icon = match (action.checked, action.icon.as_deref()) {
                    (Some(false), _) => div().flex_none().size(px(14.0)).into_any_element(),
                    (Some(true), _) => theme::icon(cx, "check", 14.0, subtle),
                    (None, icon) => theme::icon(cx, icon.unwrap_or("more"), 14.0, subtle),
                };
                let keys =
                    cx.try_global::<Keymap>().and_then(|keymap| keymap.0.shortcut_label(&action.id, is_mac_host()));
                let keys = keys
                    .map(|keys| div().flex_none().text_color(subtle).text_size(small).child(SharedString::from(keys)));
                content.child(icon).child(marked_text(cx, &label, matches).flex_1()).children(keys)
            }
            Line::Group { label, expanded, depth, .. } => {
                let shown = if *depth == 0 { cx.default_global::<Localization>().0.tx(label) } else { label.clone() };
                let chevron_icon = if *expanded { "chevron-down" } else { "chevron-right" };
                content
                    .pl(indent(*depth))
                    .when(*depth == 0, |el| el.text_color(theme::color(cx, "fg-secondary")))
                    .child(theme::icon(cx, chevron_icon, 12.0, subtle))
                    .when(*depth > 0, |el| el.child(theme::icon(cx, "folder", 14.0, subtle)))
                    .child(SharedString::from(shown))
            }
            Line::Repo(line) => {
                let color = theme::color(cx, &format!("project-color-{}", line.color));
                content
                    .child(div().flex_none().size(px(10.0)).mx(px(2.0)).rounded(px(2.0)).bg(color))
                    .child(marked_text(cx, &line.repo.name, &line.matches))
                    .children(line.operation.clone().map(|op| {
                        div()
                            .flex_none()
                            .text_color(theme::color(cx, "warning"))
                            .text_size(theme::length(cx, "fs-tiny"))
                            .child(SharedString::from(op.to_uppercase()))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .justify_end()
                            .overflow_hidden()
                            .text_color(theme::color(cx, "fg-muted"))
                            .font_family(font)
                            .text_size(small)
                            .child(SharedString::from(line.head.clone())),
                    )
                    .child(chevron(cx))
            }
            Line::Branch(line) => {
                let branch = &line.branch;
                let accent = theme::color(cx, "accent");
                let mark = if branch.current {
                    theme::icon(cx, "current-branch", 14.0, accent)
                } else {
                    let (icon, color) = if line.favorite { ("star", accent) } else { ("branch", subtle) };
                    let hover_bg = theme::color(cx, "bg-hover");
                    div()
                        .id(SharedString::from(format!("branches-star-{}", line.key)))
                        .debug_selector(|| format!("branches-star-{}", line.key))
                        .flex_none()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .child(theme::icon(cx, icon, 14.0, color))
                        .hover(move |el| el.bg(hover_bg))
                        .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            popup.toggle_favorite(index, cx);
                        }))
                        .into_any_element()
                };
                let current = branch.current.then(|| {
                    let label = cx.default_global::<Localization>().0.tx("current");
                    div()
                        .flex_none()
                        .px(px(5.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .bg(theme::color(cx, "accent-soft"))
                        .text_color(accent)
                        .text_size(theme::length(cx, "fs-tiny"))
                        .child(SharedString::from(label))
                });
                let upstream = branch.upstream.clone().filter(|up| !branch.remote && !up.is_empty()).map(|up| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .justify_end()
                        .overflow_hidden()
                        .text_color(faint)
                        .font_family(self.code_font.clone())
                        .text_size(theme::length(cx, "fs-tiny"))
                        .child(SharedString::from(up))
                });
                let has_upstream = upstream.is_some();
                content
                    .pl(indent(line.depth))
                    .child(mark)
                    .child(
                        marked_text(cx, &line.label, &line.matches)
                            .font_family(font.clone())
                            .text_size(small)
                            .when(branch.current, |el| el.text_color(strong)),
                    )
                    .children(current)
                    .children(sync_marks(cx, branch.behind, branch.ahead, &font))
                    .children(upstream)
                    .when(!has_upstream, |el| el.child(div().flex_1()))
                    .child(chevron(cx))
            }
            Line::Common(line) => {
                let accent = theme::color(cx, "accent");
                let icon = if line.branch.current {
                    theme::icon(cx, "current-branch", 14.0, accent)
                } else if line.branch.favorite {
                    theme::icon(cx, "star", 14.0, accent)
                } else {
                    theme::icon(cx, "branch", 14.0, subtle)
                };
                content
                    .pl(indent(line.depth))
                    .child(icon)
                    .child(marked_text(cx, &line.label, &line.matches).font_family(font).text_size(small))
                    .child(div().flex_1())
                    .child(chevron(cx))
            }
            Line::Tag { tag, depth, matches, .. } => content
                .pl(indent(*depth))
                .child(theme::icon(cx, "tag", 14.0, subtle))
                .child(marked_text(cx, tag, matches).font_family(font).text_size(small))
                .child(div().flex_1()),
        }
    }

    /// The submenu beside its row: drawn over everything else, out of the list's clip.
    fn render_submenu(&self, snapshot: &SubmenuSnapshot, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, faint) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"));
        let danger = theme::color(cx, "danger");
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let rows = snapshot.lines.iter().enumerate().map(|(index, line)| match line {
            MenuLine::Separator => {
                div().h(px(1.0)).my(px(4.0)).mx(px(14.0)).bg(theme::color(cx, "border-popup")).into_any_element()
            }
            MenuLine::Row(row) => {
                let color = match (row.disabled, row.danger) {
                    (true, _) => faint,
                    (false, true) => danger,
                    (false, false) => fg,
                };
                let disabled = row.disabled;
                div()
                    .id(SharedString::from(format!("branches-submenu-{index}")))
                    .debug_selector(|| format!("branches-submenu-{index}"))
                    .h(theme::length(cx, "h-list-row"))
                    .mx(px(6.0))
                    .pl(px(6.0))
                    .pr(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(5.0))
                    .whitespace_nowrap()
                    .text_color(color)
                    .when(snapshot.active == Some(index) && !disabled, |el| el.bg(selected_bg))
                    .child(
                        div()
                            .w(px(16.0))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .children(row.icon.as_deref().map(|icon| theme::icon(cx, icon, 16.0, muted))),
                    )
                    .child(div().flex_1().min_w_0().child(SharedString::from(row.label.clone())))
                    .on_mouse_move(cx.listener(move |popup, _, _, cx| {
                        if let Some(submenu) = popup.submenu.as_mut().filter(|_| !disabled)
                            && submenu.active != Some(index)
                        {
                            submenu.active = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| {
                        if let Some(submenu) = popup.submenu.as_mut().filter(|_| !disabled) {
                            submenu.active = Some(index);
                            popup.choose_submenu(cx);
                        }
                    }))
                    .into_any_element()
            }
        });
        deferred(
            div()
                .absolute()
                .top(px(-6.0))
                .left(px(width))
                .w(px(SUBMENU_WIDTH))
                .py(px(6.0))
                .flex()
                .flex_col()
                .bg(theme::color(cx, "bg-popup"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .rounded(theme::length(cx, "radius-popup"))
                .occlude()
                // A click on the submenu is not a click beside the popup.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(rows),
        )
        .with_priority(1)
    }

    fn render_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let options = &self.input.options;
        let mut shown =
            vec![(PopupOption::GroupByPrefix, options.group_by_prefix), (PopupOption::ShowRecent, options.show_recent)];
        if !self.input.tags.is_empty() {
            shown.push((PopupOption::ShowTags, options.show_tags));
        }
        let fg = theme::color(cx, "fg-strong");
        let muted = theme::color(cx, "fg-muted");
        let hover_bg = theme::color(cx, "bg-hover");
        let rows: Vec<_> = shown
            .into_iter()
            .enumerate()
            .map(|(index, (option, on))| {
                let label = cx.default_global::<Localization>().0.tx(option.label());
                div()
                    .id(SharedString::from(format!("branches-option-{index}")))
                    .debug_selector(|| format!("branches-option-{index}"))
                    .h(theme::length(cx, "h-list-row"))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(5.0))
                    .whitespace_nowrap()
                    .text_color(fg)
                    .hover(|el| el.bg(hover_bg))
                    .child(div().w(px(16.0)).flex_none().children(on.then(|| theme::icon(cx, "check", 16.0, muted))))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.toggle_option(option, cx)))
            })
            .collect();
        deferred(
            div()
                .absolute()
                .top(px(34.0))
                .right(px(6.0))
                .py(px(6.0))
                .px(px(6.0))
                .flex()
                .flex_col()
                .bg(theme::color(cx, "bg-popup"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .rounded(theme::length(cx, "radius-popup"))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(rows),
        )
        .with_priority(1)
    }
}

impl Focusable for BranchesPopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for BranchesPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = if self.input.repos.len() > 1 { WIDE } else { WIDTH };
        let (muted, subtle) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let line_color = theme::color(cx, "border-popup");
        let icon_button = |id: &'static str, icon: &str, cx: &App| {
            div()
                .id(id)
                .debug_selector(|| id.to_owned())
                .flex_none()
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(theme::icon(cx, icon, 14.0, muted))
        };
        let unscoped = self.scope.is_none();
        let header = div()
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(8.0))
            .pr(px(6.0))
            .py(px(6.0))
            .border_b_1()
            .border_color(line_color)
            .child(theme::icon(cx, "search", 14.0, subtle))
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .when(unscoped, |el| {
                let fetch = cx.listener(|_, _: &ClickEvent, _, cx| {
                    let id = "vcs.fetch".to_owned();
                    cx.emit(BranchesPopupEvent::RunAction { id, arg: None, repo: None, close: false });
                });
                let options = cx.listener(|popup, _: &ClickEvent, _, cx| {
                    popup.options_open = !popup.options_open;
                    popup.submenu = None;
                    cx.notify();
                });
                el.child(icon_button("branches-fetch", "git-fetch", cx).on_click(fetch))
                    .child(icon_button("branches-options", "settings", cx).on_click(options))
            })
            .when(self.options_open, |el| el.child(self.render_options(cx)));

        let list_height = (self.lines.len() as f32 * ROW_HEIGHT).clamp(ROW_HEIGHT * 3.0, MAX_LIST_HEIGHT);
        let body = match self.empty_text() {
            Some(text) => {
                let text = cx.default_global::<Localization>().0.tx(text);
                div()
                    .flex_none()
                    .h(px(ROW_HEIGHT * 3.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::color(cx, "fg-faint"))
                    .child(SharedString::from(text))
                    .into_any_element()
            }
            None => {
                let lines = Rc::clone(&self.lines);
                let active = self.active;
                let submenu = self.submenu.as_ref().map(|submenu| SubmenuSnapshot {
                    key: submenu.key.clone(),
                    lines: submenu.lines.clone(),
                    active: submenu.active,
                });
                let list = uniform_list(
                    "branches-popup",
                    lines.len(),
                    cx.processor(move |popup, range: Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let line = &lines[index];
                                let selectable = line.selectable();
                                let content = popup.render_line(index, line, active == Some(index), cx);
                                let open = submenu.as_ref().filter(|submenu| submenu.key == line.key());
                                div()
                                    .id(index)
                                    .debug_selector(|| format!("branches-row-{index}"))
                                    .relative()
                                    .h(px(ROW_HEIGHT))
                                    .w_full()
                                    .px(px(6.0))
                                    .child(content)
                                    .children(open.map(|open| popup.render_submenu(open, width - 8.0, cx)))
                                    .when(selectable, |el| {
                                        el.on_mouse_move(cx.listener(move |popup, _, _, cx| {
                                            if popup.submenu.is_none() && popup.active != Some(index) {
                                                popup.active = Some(index);
                                                cx.notify();
                                            }
                                        }))
                                        .on_click(cx.listener(
                                            move |popup, _: &ClickEvent, window, cx| {
                                                popup.submenu = None;
                                                popup.select(index, cx);
                                                popup.activate(index, window, cx);
                                            },
                                        ))
                                    })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full();
                div().flex_none().h(px(list_height)).py(px(4.0)).child(list).into_any_element()
            }
        };
        div()
            .w(px(width))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(header)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BranchesPopupEvent>>>;

    fn branch(name: &str, remote: bool, current: bool, upstream: Option<&str>, ahead: u32, behind: u32) -> Branch {
        let wire = json!({
            "name": name,
            "remote": remote,
            "current": current,
            "upstream": upstream,
            "ahead": ahead,
            "behind": behind,
            "lastCommitMs": 0,
        });
        serde_json::from_value(wire).expect("a branch in its wire form")
    }

    fn head(name: Option<&str>, operation: Option<&str>) -> BranchInfo {
        let wire =
            json!({ "head": name, "headCommit": "0123456789ab", "ahead": 0, "behind": 0, "operation": operation });
        serde_json::from_value(wire).expect("branch info in its wire form")
    }

    /// One repository on `main`, which tracks `origin/main` with one commit to push; a folder of
    /// feature branches, a plain topic branch and two remote branches besides the remote HEAD.
    fn single() -> BranchesPopupInput {
        BranchesPopupInput {
            branch: Some(head(Some("main"), None)),
            branches: vec![
                branch("main", false, true, Some("origin/main"), 1, 0),
                branch("feature/demo", false, false, None, 0, 0),
                branch("topic", false, false, None, 0, 2),
                branch("origin/HEAD", true, false, None, 0, 0),
                branch("origin/develop", true, false, None, 0, 0),
                branch("origin/main", true, false, None, 0, 0),
            ],
            loaded: true,
            ..BranchesPopupInput::default()
        }
    }

    fn entry(root: &str, head: &str) -> RepoEntry {
        RepoEntry {
            root: root.to_owned(),
            name: root.trim_start_matches("/p/").to_owned(),
            head: Some(head.to_owned()),
            head_commit: Some("0123456789ab".to_owned()),
        }
    }

    /// Two repositories on different branches.
    fn multi() -> BranchesPopupInput {
        let a = vec![branch("main", false, true, None, 0, 0), branch("shared", false, false, None, 0, 0)];
        let b = vec![branch("dev", false, true, None, 0, 0), branch("shared", false, false, None, 0, 0)];
        BranchesPopupInput {
            branch: Some(head(Some("main"), None)),
            branches: a.clone(),
            loaded: true,
            repo: Some("/p/a".to_owned()),
            repos: vec![entry("/p/a", "main"), entry("/p/b", "dev")],
            repo_branches: HashMap::from([("/p/a".to_owned(), a), ("/p/b".to_owned(), b)]),
            current: Some("/p/a".to_owned()),
            ..BranchesPopupInput::default()
        }
    }

    fn open(
        cx: &mut TestAppContext,
        input: BranchesPopupInput,
    ) -> (Entity<BranchesPopup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (popup, cx) = cx.add_window_view(|_, cx| BranchesPopup::new(input, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &BranchesPopupEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    fn rows(popup: &Entity<BranchesPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.read_with(cx, |popup, cx| popup.rows(cx))
    }

    fn selected(popup: &Entity<BranchesPopup>, cx: &mut VisualTestContext) -> Option<String> {
        popup.read_with(cx, |popup, cx| popup.selected(cx))
    }

    fn submenu(popup: &Entity<BranchesPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.read_with(cx, |popup, _| popup.submenu_rows())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    // src/features/vcs/__tests__/branchesPopupHost.test.tsx › opens Git Branches without a mounted toolbar widget
    #[gpui_kit::test]
    async fn opens_git_branches_without_a_mounted_toolbar_widget(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        // The actions in their groups, then Local (open: nothing recent) with the current branch
        // first, a closed folder and the other branches, then the closed Remote group.
        assert_eq!(
            rows(&popup, cx),
            [
                "Update Project…",
                "Commit…",
                "Push…",
                "-",
                "New Branch…",
                "Checkout Tag or Revision…",
                "-",
                "v Local",
                "  main [current] ↑1 origin/main",
                "  > feature",
                "  topic ↓2",
                "> Remote",
            ]
        );
        assert_eq!(selected(&popup, cx).as_deref(), Some("Update Project…"));
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the search has the keyboard");
        cx.simulate_keystrokes("escape");
        assert!(matches!(events.borrow().as_slice(), [BranchesPopupEvent::Dismissed]));
    }

    // Adapted from src/features/vcs/__tests__/branchFilterPopup.test.tsx › moves over section headers with the arrow
    // keys
    #[gpui_kit::test]
    async fn moves_over_separators_with_the_arrow_keys(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        cx.simulate_keystrokes("down down down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("New Branch…"), "the separator is passed over");
        cx.simulate_keystrokes("up up up up");
        assert_eq!(selected(&popup, cx).as_deref(), Some("> Remote"), "above the first row comes the last");
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("Update Project…"));
        cx.simulate_keystrokes("enter");
        let ran = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::RunAction { id, repo: None, close: true, .. } if id == "vcs.update")
        });
        assert!(ran, "Enter runs the selected action");
    }

    // Adapted from src/features/vcs/__tests__/branchFilterPopup.test.tsx › searches case-insensitively and hides groups
    // without a match
    #[gpui_kit::test]
    async fn searches_and_hides_groups_without_a_match(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        cx.simulate_input("DEVELOP");
        // No action matches; Local has no match and goes; every group is open while searching.
        assert_eq!(rows(&popup, cx), ["v Remote", "  v origin", "    develop"]);
        cx.simulate_keystrokes("down down enter");
        assert_eq!(submenu(&popup, cx)[0], "> Checkout");
        cx.simulate_keystrokes("enter");
        let checkout = events.borrow().iter().any(|event| {
            matches!(
                event,
                BranchesPopupEvent::Command(BranchesCommand {
                    target: RepoTarget::Selected,
                    command: BranchCommand::Checkout(name),
                }) if name == "origin/develop"
            )
        });
        assert!(checkout, "Checkout of the remote branch is asked for");
        assert!(submenu(&popup, cx).is_empty(), "choosing closes the submenu");
    }

    // Adapted from src/features/vcs/__tests__/branchFilterPopup.test.tsx › highlights fuzzy matches and shows an empty
    // state
    #[gpui_kit::test]
    async fn shows_an_empty_state(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, single());
        cx.simulate_input("missing");
        assert!(rows(&popup, cx).is_empty());
        assert_eq!(popup.read_with(cx, |popup, _| popup.empty_text()), Some("No matching branches"));
        let loading = BranchesPopupInput { loaded: false, branches: Vec::new(), ..single() };
        let (popup, _, cx) = open(cx, loading);
        cx.simulate_input("x");
        assert_eq!(popup.read_with(cx, |popup, _| popup.empty_text()), Some("Loading…"));
    }

    // Not from a spec: → at the end of the query opens a branch's submenu, ← and Escape close it,
    // Escape again closes the popup.
    #[gpui_kit::test]
    async fn right_opens_a_branchs_submenu_and_left_closes_it(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        // Down to topic, past the actions, Local, main and the feature folder.
        cx.simulate_keystrokes("down down down down down down down down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  topic ↓2"));
        cx.simulate_keystrokes("right");
        assert_eq!(
            submenu(&popup, cx),
            [
                "> Checkout",
                "New Branch from 'topic'…",
                "Checkout and Rebase onto 'main'",
                "-",
                "Compare with 'main'",
                "Show Diff with Working Tree",
                "Show History",
                "-",
                "Rebase 'main' onto 'topic'",
                "Merge 'topic' into 'main'",
                "-",
                "Update",
                "Push…",
                "-",
                "Rename…",
                "Add to Favorites",
                "Delete",
            ]
        );
        cx.simulate_keystrokes("up");
        assert_eq!(submenu(&popup, cx).last().map(String::as_str), Some("> Delete"), "the submenu goes round");
        cx.simulate_keystrokes("left");
        assert!(submenu(&popup, cx).is_empty());
        assert_eq!(selected(&popup, cx).as_deref(), Some("  topic ↓2"), "the row stays selected");
        cx.simulate_keystrokes("right escape");
        assert!(submenu(&popup, cx).is_empty());
        assert!(events.borrow().is_empty(), "Escape closed the submenu, not the popup");

        // Show History is an action: it runs with the branch.
        cx.simulate_keystrokes("right down down down down down enter");
        let history = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::RunAction { id, arg: Some(arg), close: true, .. }
                if id == "vcs.showLog" && arg == &json!({ "branch": "topic" }))
        });
        assert!(history);
        cx.simulate_keystrokes("escape");
        assert!(matches!(events.borrow().last(), Some(BranchesPopupEvent::Dismissed)));
    }

    // Not from a spec: ← closes the selected group and → opens it again; the choice is reported.
    #[gpui_kit::test]
    async fn left_and_right_close_and_open_a_group(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        cx.simulate_keystrokes("down down down down down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("v Local"));
        cx.simulate_keystrokes("left");
        assert_eq!(rows(&popup, cx)[7..], ["> Local", "> Remote"]);
        assert_eq!(selected(&popup, cx).as_deref(), Some("> Local"));
        let closed = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::OptionsChanged(options)
                if options.expanded.get("group:local") == Some(&false))
        });
        assert!(closed);
        cx.simulate_keystrokes("right");
        assert_eq!(rows(&popup, cx)[7], "v Local");
        // Enter on a group toggles it too.
        cx.simulate_keystrokes("down enter");
        assert_eq!(rows(&popup, cx)[8], "  main [current] ↑1 origin/main", "Enter on a branch keeps the rows");
        assert_eq!(submenu(&popup, cx)[0], "> New Branch from 'main'…", "the current branch has its own menu");
    }

    // Not from a spec: a stopped rebase and a conflict put their controls on top.
    #[gpui_kit::test]
    async fn puts_the_controls_of_a_stopped_rebase_on_top(cx: &mut TestAppContext) {
        let input = BranchesPopupInput { branch: Some(head(Some("main"), Some("rebase"))), conflicts: 1, ..single() };
        let (popup, _, cx) = open(cx, input);
        assert_eq!(
            rows(&popup, cx)[..6],
            ["Resolve Conflicts…", "Continue Rebase", "Skip Commit", "Abort Rebase", "-", "Update Project…"]
        );
    }

    // Not from a spec: a click runs an action row; the star of a branch asks to toggle the favourite
    // and leaves the popup open.
    #[gpui_kit::test]
    async fn clicks_run_actions_and_toggle_favorites(cx: &mut TestAppContext) {
        let (_popup, events, cx) = open(cx, single());
        click(cx, "branches-star-local:topic");
        let toggled = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::ToggleFavorite { target: RepoTarget::Selected, branch }
                if branch.name == "topic")
        });
        assert!(toggled);
        click(cx, "branches-row-2");
        let pushed = events
            .borrow()
            .iter()
            .any(|event| matches!(event, BranchesPopupEvent::RunAction { id, close: true, .. } if id == "vcs.push"));
        assert!(pushed);
        click(cx, "branches-fetch");
        let fetched = events
            .borrow()
            .iter()
            .any(|event| matches!(event, BranchesPopupEvent::RunAction { id, close: false, .. } if id == "vcs.fetch"));
        assert!(fetched);
    }

    // Not from a spec: the gear options change the rows and are reported.
    #[gpui_kit::test]
    async fn the_options_change_the_rows(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, single());
        click(cx, "branches-options");
        click(cx, "branches-option-0");
        assert!(rows(&popup, cx).contains(&"  feature/demo".to_owned()), "without folders the full name shows");
        let reported = events
            .borrow()
            .iter()
            .any(|event| matches!(event, BranchesPopupEvent::OptionsChanged(options) if !options.group_by_prefix));
        assert!(reported);
    }

    // Not from a spec: with several repositories the popup lists them; → opens one's own branches and
    // ← at the start of the query goes back to the repositories.
    #[gpui_kit::test]
    async fn opens_the_branches_of_one_of_several_repositories(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, multi());
        let shown = rows(&popup, cx);
        assert!(shown.contains(&"a main".to_owned()) && shown.contains(&"b dev".to_owned()), "{shown:?}");
        cx.simulate_keystrokes("down down down down down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("a main"));
        cx.simulate_keystrokes("right");
        assert_eq!(popup.read_with(cx, |popup, _| popup.scope().map(str::to_owned)).as_deref(), Some("/p/a"));
        assert_eq!(rows(&popup, cx)[..3], ["New Branch…", "Checkout Tag or Revision…", "-"]);
        cx.simulate_keystrokes("enter");
        let new_branch = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::RunAction { id, repo: Some(repo), .. }
                if id == "vcs.newBranch" && repo == "/p/a")
        });
        assert!(new_branch, "the repository's own action runs in it");
        cx.simulate_keystrokes("left");
        assert_eq!(popup.read_with(cx, |popup, _| popup.scope().map(str::to_owned)), None);
        assert_eq!(selected(&popup, cx).as_deref(), Some("a main"));

        // New Branch on top acts in every repository.
        cx.simulate_keystrokes("up up enter");
        let everywhere = events.borrow().iter().any(|event| {
            matches!(event, BranchesPopupEvent::RunAction { id, arg: Some(arg), repo: None, .. }
                if id == "vcs.newBranch" && arg == &json!({ "repos": ["/p/a", "/p/b"] }))
        });
        assert!(everywhere);
    }

    // Not from a spec: every design token and icon the popup draws with exists.
    #[test]
    fn the_design_tokens_the_popup_uses_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-faint",
            "fg-secondary",
            "accent",
            "accent-soft",
            "success",
            "warning",
            "danger",
            "bg-popup",
            "bg-hover",
            "bg-tree-selected",
            "border-popup",
            "project-color-1",
            "project-color-9",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "fs-small", "fs-tiny", "radius-small", "radius-popup", "h-list-row"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
        let icons = [
            "search",
            "git-fetch",
            "settings",
            "git-pull",
            "commit",
            "git-push",
            "plus",
            "branch",
            "current-branch",
            "star",
            "tag",
            "folder",
            "chevron-down",
            "chevron-right",
            "check",
            "more",
            "warning",
            "vcs-incoming",
            "vcs-outgoing",
        ];
        for name in icons {
            assert!(theme::icons::icon_path(name).is_some(), "the {name} icon exists");
        }
    }
}
