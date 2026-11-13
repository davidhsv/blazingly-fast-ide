//! The Git Log's Branches pane, left of the commit list: HEAD, Local, Remote › <remote> and Tags
//! (for a project with several repositories, a subtree per repository under each group), branch
//! names with `/` in folders, the current branch and favourites first, each branch with its
//! incoming / outgoing commits. The search field keeps the branches and tags whose name matches.
//!
//! A click only selects (⌘ / Ctrl adds or removes a row, ⇧ selects a range); a double click or
//! Enter switches the log to the ref, or to every selected ref when it is part of the selection.
//! ⌘C / Ctrl+C copies the selected refs, one per line. The vertical toolbar hides the pane and runs
//! the pane's actions on the selection; the context menu is the branch's, the tag's, or Manage
//! Remotes… on the remotes.
//!
//! [`LogBranchesPane`] shows a [`BranchesPaneInput`] handed in with
//! [`LogBranchesPane::set_input`] and reports what the user asks for through
//! [`BranchesPaneEvent`].

use super::branches::{
    self, BranchNodeKind, BranchTreeNode, RepositoryBranches, branch_folder_keys, branch_group_keys, build_branch_tree,
    build_repository_branch_tree, filter_branch_tree, is_favorite_branch,
};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::branch_menu::{BranchCommand, branch_menu};
use crate::vcs_ops::branch_ops::can_delete_branch;
use crate::vcs_ops::log_pane_actions::{BranchTarget, can_update_branch};
use crate::vcs_ops::menu::{MenuEntry, MenuItem};
use gpui_kit::{
    App, ClickEvent, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    Modifiers, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_vcs::Branch;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The action ids of the pane's toolbar.
pub const SHOW_BRANCHES: &str = "vcs.log.showBranches";
pub const NEW_BRANCH: &str = "vcs.log.branches.new";
pub const UPDATE_SELECTED: &str = "vcs.log.branches.update";
pub const DELETE_BRANCH: &str = "vcs.log.branches.delete";
pub const NAVIGATE: &str = "vcs.log.branches.navigate";
pub const TOGGLE_FAVORITE: &str = "vcs.log.branches.toggleFavorite";
pub const FETCH: &str = "vcs.fetch";
/// The toolbar's Refresh, which is no action: [`BranchesPaneEvent::Refresh`].
pub const REFRESH: &str = "refresh";

/// A repository of the log with its branches and tags.
#[derive(Clone, Debug, Default)]
pub struct PaneRepository {
    pub root: String,
    pub name: String,
    pub branches: Vec<Branch>,
    pub tags: Vec<String>,
}

/// What the pane shows.
#[derive(Clone, Debug, Default)]
pub struct BranchesPaneInput {
    /// The log's repositories: one for a single repository (the selected one).
    pub repositories: Vec<PaneRepository>,
    /// The current branch of the selected repository (`None` on a detached HEAD).
    pub current: Option<String>,
    /// The current branch of each repository, for a project with several.
    pub heads: HashMap<String, Option<String>>,
    /// The favourite branch names as stored; `None`: the defaults.
    pub favorites: Option<Vec<String>>,
    /// The log's branch filter (`HEAD` for HEAD) and the repositories it applies to.
    pub filter_branches: Vec<String>,
    pub filter_repositories: Vec<String>,
    /// A log opened on other refs, which changes nothing: Show in Git Log and Copy only.
    pub read_only: bool,
}

/// What a context menu row of the pane does.
#[derive(Clone, Debug)]
pub enum PaneCommand {
    /// A row of the branch's menu, run in `repo` (`None`: the selected repository).
    Branch {
        command: BranchCommand,
        repo: Option<String>,
    },
    ShowInLog(String),
    Copy(String),
}

/// The context menu a row asks for.
#[derive(Clone, Debug)]
pub enum PaneMenu {
    Items(Vec<MenuItem<PaneCommand>>),
    /// A tag's menu, built by the workbench.
    Tag {
        tag: String,
        repo: Option<String>,
    },
    None,
}

/// What the Branches pane asks of the Git tool window.
#[derive(Clone, Debug)]
pub enum BranchesPaneEvent {
    /// Show the log of these refs (the log filter's branches and repositories).
    SetLogFilter { branches: Vec<String>, repositories: Vec<String> },
    /// Run this action with `arg` (the toolbar).
    RunAction { id: &'static str, arg: Option<Value> },
    /// Carry out a row of a branch's context menu, in `repo` (`None`: the selected repository).
    Branch { command: BranchCommand, repo: Option<String> },
    /// The selected refs, in tree order: what the pane's actions act on without an argument.
    SelectionChanged(Vec<BranchTarget>),
    /// Show this context menu at `position`; carry out the chosen row with [`LogBranchesPane::run_command`].
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<PaneCommand>> },
    /// Show the tag's context menu at `position`.
    ShowTagMenu { position: Point<Pixels>, tag: String, repo: Option<String> },
    /// Reload the branches (the log, for a read-only one).
    Refresh,
}

/// A button of the vertical toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneToolButton {
    /// The action it runs, or [`REFRESH`].
    pub id: &'static str,
    pub selector: &'static str,
    pub icon: &'static str,
    pub enabled: bool,
    pub pressed: bool,
}

/// A visible line of the tree.
#[derive(Clone, Debug)]
pub struct PaneRow {
    pub key: String,
    /// As shown: HEAD's and the groups' labels translated.
    pub label: String,
    pub kind: BranchNodeKind,
    pub ref_name: Option<String>,
    pub repo: Option<String>,
    pub favorite: bool,
    /// The branch as the backend lists it.
    pub branch: Option<Branch>,
    pub depth: usize,
    /// `None` for a row without children.
    pub expanded: Option<bool>,
}

/// The slim branches the tree models read.
fn slim(list: &[Branch]) -> Vec<branches::Branch> {
    list.iter()
        .map(|branch| branches::Branch { name: branch.name.clone(), remote: branch.remote, current: branch.current })
        .collect()
}

/// The tree of `input`: one repository's, or a subtree per repository under each group.
fn tree_of(input: &BranchesPaneInput) -> Vec<BranchTreeNode> {
    let favorites = input.favorites.as_deref();
    match input.repositories.as_slice() {
        [] => build_branch_tree(&[], favorites, &[]),
        [only] => build_branch_tree(&slim(&only.branches), favorites, &only.tags),
        many => {
            let repositories: Vec<RepositoryBranches> = many
                .iter()
                .map(|repo| RepositoryBranches {
                    root: repo.root.clone(),
                    name: repo.name.clone(),
                    branches: slim(&repo.branches),
                    tags: repo.tags.clone(),
                })
                .collect();
            build_repository_branch_tree(&repositories, favorites)
        }
    }
}

/// The keys the log filter selects: HEAD, or each branch (in each filtered repository).
fn filter_keys(input: &BranchesPaneInput) -> Vec<String> {
    let several = input.repositories.len() > 1;
    let repositories: Vec<&str> = if input.filter_repositories.is_empty() {
        input.repositories.iter().map(|repo| repo.root.as_str()).collect()
    } else {
        input.filter_repositories.iter().map(String::as_str).collect()
    };
    input
        .filter_branches
        .iter()
        .flat_map(|branch| match (branch.as_str(), several) {
            ("HEAD", _) => vec!["head".to_owned()],
            (_, false) => vec![format!("b:{branch}")],
            (_, true) => repositories.iter().map(|repo| format!("{repo}:b:{branch}")).collect(),
        })
        .collect()
}

/// A ref of the tree: its node's key, the ref, the branch it is (if one) and its repository.
type RefEntry = (String, String, Option<branches::Branch>, Option<String>);

/// The refs of `nodes` in tree order.
fn refs_in_order(nodes: &[BranchTreeNode], out: &mut Vec<RefEntry>) {
    for node in nodes {
        if let Some(ref_name) = &node.ref_name {
            out.push((node.key.clone(), ref_name.clone(), node.branch.clone(), node.repo.clone()));
        }
        refs_in_order(node.children.as_deref().unwrap_or_default(), out);
    }
}

/// The rows of a branch menu, carried out in `repo`.
fn in_repo(items: Vec<MenuItem<BranchCommand>>, repo: Option<&str>) -> Vec<MenuItem<PaneCommand>> {
    items
        .into_iter()
        .map(|item| match item {
            MenuItem::Separator => MenuItem::Separator,
            MenuItem::Action(action) => MenuItem::Action(action),
            MenuItem::Item(entry) => MenuItem::Item(MenuEntry {
                label: entry.label,
                icon: entry.icon,
                shortcut: entry.shortcut,
                disabled: entry.disabled,
                danger: entry.danger,
                run: entry.run.map(|command| PaneCommand::Branch { command, repo: repo.map(str::to_owned) }),
                submenu: in_repo(entry.submenu, repo),
            }),
        })
        .collect()
}

/// The Git Log's Branches pane.
pub struct LogBranchesPane {
    input: BranchesPaneInput,
    /// The whole tree; the rows show it filtered by the search.
    all: Vec<BranchTreeNode>,
    rows: Rc<Vec<PaneRow>>,
    query: String,
    expanded: HashSet<String>,
    /// The repositories the expansion was set up for.
    seeded: Option<Vec<String>>,
    selected: HashSet<String>,
    lead: Option<String>,
    anchor: Option<String>,
    /// The log filter the selection last followed.
    shown_filter: Vec<String>,
    search: Entity<TextField>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    key_claim: u64,
    _search_events: Subscription,
}

impl EventEmitter<BranchesPaneEvent> for LogBranchesPane {}

impl LogBranchesPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextField::new(cx).with_placeholder("Branch or tag"));
        let events = cx.subscribe(&search, |pane: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                pane.query = field.read(cx).text().to_owned();
                pane.rebuild(cx);
            }
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => {
                field.update(cx, |field, cx| field.set_text("", cx));
                pane.query.clear();
                pane.rebuild(cx);
            }
        });
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |pane, cx| pane.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|pane, cx| input::release_keys(pane.key_claim, cx)).detach();
        let mut pane = Self {
            input: BranchesPaneInput::default(),
            all: Vec::new(),
            rows: Rc::default(),
            query: String::new(),
            expanded: HashSet::new(),
            seeded: None,
            selected: HashSet::new(),
            lead: None,
            anchor: None,
            shown_filter: Vec::new(),
            search,
            focus,
            scroll: UniformListScrollHandle::new(),
            key_claim,
            _search_events: events,
        };
        pane.all = tree_of(&pane.input);
        pane.rebuild(cx);
        pane
    }

    /// The branches, tags, favourites or the log filter changed. A new set of repositories opens
    /// every group but Tags, and the folders down to the current branch; when the log filter changed
    /// elsewhere the selection follows it.
    pub fn set_input(&mut self, input: BranchesPaneInput, cx: &mut Context<Self>) {
        self.input = input;
        self.all = tree_of(&self.input);
        let roots: Vec<String> = self.input.repositories.iter().map(|repo| repo.root.clone()).collect();
        if self.seeded.as_ref() != Some(&roots) {
            self.seeded = Some(roots);
            let mut open: HashSet<String> =
                branch_group_keys(&self.all).into_iter().filter(|key| !key.starts_with("g:tags")).collect();
            if let Some(current) = &self.input.current {
                open.extend(branch_folder_keys(&self.all, current));
            }
            self.expanded = open;
        }
        let filter = filter_keys(&self.input);
        if filter != self.shown_filter {
            self.selected = filter.iter().cloned().collect();
            self.lead = filter.first().cloned();
            self.anchor.clone_from(&self.lead);
            self.shown_filter = filter;
            cx.emit(BranchesPaneEvent::SelectionChanged(self.targets()));
        }
        self.rebuild(cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    pub fn rows(&self) -> &[PaneRow] {
        &self.rows
    }

    /// The rows as text, indented by depth: the label, then `current` and the incoming (↓) and
    /// outgoing (↑) commits.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                let mut line = format!("{}{}", "  ".repeat(row.depth), row.label);
                if let Some(branch) = &row.branch {
                    if branch.current {
                        line.push_str(" current");
                    }
                    if branch.behind > 0 {
                        line.push_str(&format!(" ↓{}", branch.behind));
                    }
                    if branch.ahead > 0 {
                        line.push_str(&format!(" ↑{}", branch.ahead));
                    }
                }
                line
            })
            .collect()
    }

    /// The keys of the selected rows.
    pub fn selected_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.selected.iter().cloned().collect();
        keys.sort();
        keys
    }

    /// The selected refs in tree order.
    pub fn targets(&self) -> Vec<BranchTarget> {
        let mut refs = Vec::new();
        refs_in_order(&self.all, &mut refs);
        refs.into_iter()
            .filter(|(key, ..)| self.selected.contains(key))
            .map(|(_, ref_name, branch, repo)| {
                let branch = branch.and_then(|branch| self.full_branch(repo.as_deref(), &branch));
                BranchTarget { ref_name, repo, branch }
            })
            .collect()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search
    }

    /// What the empty tree says; `None` while it shows rows.
    pub fn empty_text(&self) -> Option<&'static str> {
        (self.rows.is_empty() && !self.query.trim().is_empty()).then_some("No matching branches")
    }

    /// The vertical toolbar, top to bottom: Hide Branches, then (unless the log is read-only) New
    /// Branch…, Update Selected, Delete Branch, Navigate Log to Selected Branch, the favourite
    /// star and Fetch, then Refresh.
    pub fn toolbar(&self) -> Vec<PaneToolButton> {
        let targets = self.targets();
        let target = targets.first();
        let favorite = target
            .and_then(|target| target.branch.as_ref())
            .is_some_and(|branch| is_favorite_branch(&branch.name, branch.remote, self.input.favorites.as_deref()));
        let button = |id, selector, icon, enabled| PaneToolButton { id, selector, icon, enabled, pressed: false };
        let mut buttons = vec![button(SHOW_BRANCHES, "branches-hide", "chevron-left", true)];
        if !self.input.read_only {
            buttons.extend([
                button(NEW_BRANCH, "branches-new", "plus", true),
                button(UPDATE_SELECTED, "branches-update", "git-pull", can_update_branch(target)),
                button(DELETE_BRANCH, "branches-delete", "trash", targets.iter().any(|t| can_delete_branch(Some(t)))),
                button(NAVIGATE, "branches-navigate", "locate", target.is_some()),
                PaneToolButton {
                    pressed: favorite,
                    ..button(
                        TOGGLE_FAVORITE,
                        "branches-favorite",
                        if favorite { "star" } else { "star-outline" },
                        target.is_some_and(|target| target.branch.is_some()),
                    )
                },
                button(FETCH, "branches-fetch", "git-fetch", true),
            ]);
        }
        buttons.push(button(REFRESH, "branches-refresh", "refresh", true));
        buttons
    }

    /// The context menu of row `index`.
    pub fn menu(&self, index: usize, i18n: &mut I18n) -> PaneMenu {
        let Some(row) = self.rows.get(index) else { return PaneMenu::None };
        if self.input.read_only {
            let Some(ref_name) = row.ref_name.clone() else { return PaneMenu::None };
            return PaneMenu::Items(vec![
                MenuItem::item(i18n.tx("Show in Git Log"), PaneCommand::ShowInLog(ref_name.clone())),
                MenuItem::item(i18n.tx("Copy"), PaneCommand::Copy(ref_name)),
            ]);
        }
        if let Some(branch) = &row.branch {
            let picked = if self.selected.contains(&row.key) {
                self.targets()
            } else {
                let ref_name = row.ref_name.clone().unwrap_or_default();
                vec![BranchTarget { ref_name, repo: row.repo.clone(), branch: Some(branch.clone()) }]
            };
            let current = match &row.repo {
                Some(repo) => self.input.heads.get(repo).cloned().flatten(),
                None => self.input.current.clone(),
            };
            let items = branch_menu(branch, current.as_deref(), &picked, row.favorite, i18n);
            return PaneMenu::Items(in_repo(items, row.repo.as_deref()));
        }
        match row.kind {
            BranchNodeKind::Tag => match &row.ref_name {
                Some(tag) => PaneMenu::Tag { tag: tag.clone(), repo: row.repo.clone() },
                None => PaneMenu::None,
            },
            BranchNodeKind::Remote => PaneMenu::Items(vec![remotes_item(i18n)]),
            BranchNodeKind::Group if row.key.ends_with("g:remote") => PaneMenu::Items(vec![remotes_item(i18n)]),
            _ => PaneMenu::None,
        }
    }

    /// Carries out a context menu row the user chose.
    pub fn run_command(&mut self, command: PaneCommand, cx: &mut Context<Self>) {
        match command {
            PaneCommand::Branch { command, repo } => cx.emit(BranchesPaneEvent::Branch { command, repo }),
            PaneCommand::ShowInLog(ref_name) => {
                cx.emit(BranchesPaneEvent::SetLogFilter { branches: vec![ref_name], repositories: Vec::new() });
            }
            PaneCommand::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Internals

    /// The branch as the backend lists it, of `repo` (`None`: the only repository).
    fn full_branch(&self, repo: Option<&str>, slim: &branches::Branch) -> Option<Branch> {
        let repository = match repo {
            Some(root) => self.input.repositories.iter().find(|repository| repository.root == root),
            None => self.input.repositories.first(),
        }?;
        repository.branches.iter().find(|branch| branch.name == slim.name && branch.remote == slim.remote).cloned()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let roots = filter_branch_tree(&self.all, &self.query);
        // While searching every match shows, its folders open.
        let open: HashSet<String> = if self.query.trim().is_empty() {
            self.expanded.clone()
        } else {
            branch_group_keys(&roots).into_iter().collect()
        };
        let i18n = &cx.default_global::<Localization>().0;
        let mut rows = Vec::new();
        self.flatten(&roots, 0, &open, i18n, &mut rows);
        self.rows = Rc::new(rows);
        cx.notify();
    }

    fn flatten(
        &self,
        nodes: &[BranchTreeNode],
        depth: usize,
        open: &HashSet<String>,
        i18n: &I18n,
        out: &mut Vec<PaneRow>,
    ) {
        for node in nodes {
            let expanded = node.children.as_ref().map(|_| open.contains(&node.key));
            let translated = matches!(node.kind, BranchNodeKind::Head | BranchNodeKind::Group);
            out.push(PaneRow {
                key: node.key.clone(),
                label: if translated { i18n.tx(&node.label) } else { node.label.clone() },
                kind: node.kind,
                ref_name: node.ref_name.clone(),
                repo: node.repo.clone(),
                favorite: node.favorite,
                branch: node.branch.as_ref().and_then(|branch| self.full_branch(node.repo.as_deref(), branch)),
                depth,
                expanded,
            });
            if expanded == Some(true) {
                self.flatten(node.children.as_deref().unwrap_or_default(), depth + 1, open, i18n, out);
            }
        }
    }

    fn index_of(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|row| row.key == key)
    }

    fn lead_index(&self) -> Option<usize> {
        self.lead.as_deref().and_then(|key| self.index_of(key))
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(BranchesPaneEvent::SelectionChanged(self.targets()));
        cx.notify();
    }

    /// Selects row `index` alone.
    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(key) = self.rows.get(index).map(|row| row.key.clone()) else { return };
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        self.selected = HashSet::from([key.clone()]);
        self.lead = Some(key.clone());
        self.anchor = Some(key);
        self.selection_changed(cx);
    }

    /// A click on row `index`: ⌘ / Ctrl adds or removes it, ⇧ selects the range from the anchor.
    fn click(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(key) = self.rows.get(index).map(|row| row.key.clone()) else { return };
        if modifiers.platform || modifiers.control {
            if !self.selected.remove(&key) {
                self.selected.insert(key.clone());
            }
            self.lead = Some(key.clone());
            self.anchor = Some(key);
            self.selection_changed(cx);
        } else if modifiers.shift
            && let Some(from) = self.anchor.as_deref().and_then(|anchor| self.index_of(anchor))
        {
            let (start, end) = (from.min(index), from.max(index));
            self.selected = self.rows[start..=end].iter().map(|row| row.key.clone()).collect();
            self.lead = Some(key);
            self.selection_changed(cx);
        } else {
            self.select_index(index, cx);
        }
    }

    /// A double click or Enter: the log shows the ref (with the rest of the selection when it is
    /// part of it); a row without a ref opens or closes.
    fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).cloned() else { return };
        let Some(ref_name) = row.ref_name.clone() else {
            if row.expanded.is_some() {
                self.toggle(&row.key, cx);
            }
            return;
        };
        let branches = if self.selected.contains(&row.key) {
            self.targets().into_iter().filter(|target| target.repo == row.repo).map(|target| target.ref_name).collect()
        } else {
            vec![ref_name]
        };
        let repositories = row.repo.into_iter().collect();
        cx.emit(BranchesPaneEvent::SetLogFilter { branches, repositories });
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        self.rebuild(cx);
    }

    /// ⌘C / Ctrl+C: the selected refs, one per line in tree order. `false` with nothing selected.
    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let names: Vec<String> = self.targets().into_iter().map(|target| target.ref_name).collect();
        if names.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(names.join("\n")));
        true
    }

    fn press(&mut self, button: PaneToolButton, cx: &mut Context<Self>) {
        if !button.enabled {
            return;
        }
        let targets = self.targets();
        let one = targets.first().and_then(|target| serde_json::to_value(target).ok());
        let arg = match button.id {
            REFRESH => {
                cx.emit(BranchesPaneEvent::Refresh);
                return;
            }
            SHOW_BRANCHES | FETCH => None,
            DELETE_BRANCH => serde_json::to_value(&targets).ok(),
            _ => one,
        };
        cx.emit(BranchesPaneEvent::RunAction { id: button.id, arg });
    }

    fn show_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let menu = self.menu(index, &mut cx.default_global::<Localization>().0);
        match menu {
            PaneMenu::Items(items) if !items.is_empty() => cx.emit(BranchesPaneEvent::ShowMenu { position, items }),
            PaneMenu::Tag { tag, repo } => cx.emit(BranchesPaneEvent::ShowTagMenu { position, tag, repo }),
            PaneMenu::Items(_) | PaneMenu::None => {}
        }
    }

    /// The keys the tree handles while it has the keyboard, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        if (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.shift && key == "c" {
            return self.copy(cx);
        }
        if modifiers.modified() {
            return false;
        }
        let Some(last) = self.rows.len().checked_sub(1) else { return false };
        let at = self.lead_index();
        let row = at.and_then(|at| self.rows.get(at).cloned());
        match (key, row) {
            ("down", _) => self.select_index(at.map_or(0, |at| (at + 1).min(last)), cx),
            ("up", _) => self.select_index(at.map_or(last, |at| at.saturating_sub(1)), cx),
            ("home", _) => self.select_index(0, cx),
            ("end", _) => self.select_index(last, cx),
            ("right", Some(PaneRow { expanded: Some(false), key, .. })) => self.toggle(&key, cx),
            ("right", Some(PaneRow { expanded: Some(true), .. })) => {
                self.select_index(at.map_or(0, |at| (at + 1).min(last)), cx);
            }
            ("left", Some(PaneRow { expanded: Some(true), key, .. })) => self.toggle(&key, cx),
            ("left", Some(PaneRow { depth, .. })) if depth > 0 => {
                let parent = at.and_then(|at| self.rows[..at].iter().rposition(|row| row.depth < depth));
                if let Some(parent) = parent {
                    self.select_index(parent, cx);
                }
            }
            ("enter", Some(_)) => {
                if let Some(at) = at {
                    self.activate(at, cx);
                }
            }
            _ => return false,
        }
        true
    }
}

fn remotes_item(i18n: &I18n) -> MenuItem<PaneCommand> {
    MenuItem::action("vcs.manageRemotes").labelled(i18n.tx("Manage Remotes…"))
}

impl Focusable for LogBranchesPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The icon of a row, as IDEA's Branches pane draws it.
fn row_icon(row: &PaneRow) -> &'static str {
    match row.kind {
        BranchNodeKind::Head => "commit",
        BranchNodeKind::Tag => "tag",
        _ if row.branch.as_ref().is_some_and(|branch| branch.current) => "current-branch",
        BranchNodeKind::Branch if row.favorite => "star",
        BranchNodeKind::Branch => "branch",
        _ => "folder",
    }
}

/// The incoming and outgoing markers of row `index`'s branch, with their counts.
fn sync_marks(cx: &App, index: usize, behind: u32, ahead: u32) -> Option<Div> {
    if behind == 0 && ahead == 0 {
        return None;
    }
    let mark = |selector: String, icon: &str, color: &str, count: u32| {
        let color = theme::color(cx, color);
        div()
            .debug_selector(move || selector.clone())
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
            .text_size(theme::length(cx, "fs-small"))
            .when(behind > 0, |el| {
                el.child(mark(format!("branches-incoming-{index}"), "vcs-incoming", "accent", behind))
            })
            .when(ahead > 0, |el| {
                el.child(mark(format!("branches-outgoing-{index}"), "vcs-outgoing", "success", ahead))
            }),
    )
}

fn tool_button(cx: &App, button: PaneToolButton) -> Stateful<Div> {
    let color = theme::color(
        cx,
        if !button.enabled {
            "fg-faint"
        } else if button.pressed {
            "fg-strong"
        } else {
            "fg-muted"
        },
    );
    let selector = button.selector;
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(button.pressed, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
        .when(button.enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .child(theme::icon(cx, button.icon, 16.0, color))
}

impl Render for LogBranchesPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &cx.default_global::<Localization>().0;
        let empty = self.empty_text().map(|text| i18n.tx(text));
        let current_label = i18n.tx("current");
        let border = theme::color(cx, "border");
        let mut toolbar = div()
            .flex_none()
            .w(px(30.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.0))
            .py(px(4.0))
            .border_r_1()
            .border_color(border);
        for button in self.toolbar() {
            let press = cx.listener(move |pane, _: &ClickEvent, _, cx| pane.press(button, cx));
            toolbar = toolbar.child(tool_button(cx, button).on_click(press));
        }
        let search = div()
            .flex_none()
            .h(theme::length(cx, "h-compact-header"))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(border)
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
            .child(div().debug_selector(|| "branches-search".to_owned()).flex_1().min_w_0().child(self.search.clone()));
        let tree = match empty {
            Some(empty) => div()
                .debug_selector(|| "branches-empty".to_owned())
                .flex_1()
                .p(px(8.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(empty))
                .into_any_element(),
            None => self.tree(window, current_label, cx),
        };
        div()
            .size_full()
            .flex()
            .text_size(theme::length(cx, "fs-ui"))
            .child(toolbar)
            .child(div().flex_1().min_w_0().flex().flex_col().child(search).child(tree))
    }
}

impl LogBranchesPane {
    fn tree(&self, window: &Window, current_label: String, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let focused = self.focus.is_focused(window);
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, muted, subtle) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let chevron_color = theme::color(cx, "fg-icon");
        let row_height = theme::length(cx, "h-row");
        let rows = Rc::clone(&self.rows);
        let selected: Rc<HashSet<usize>> =
            Rc::new((0..rows.len()).filter(|index| self.selected.contains(&rows[*index].key)).collect());
        let list = uniform_list(
            "branches-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let chevron = div().size(px(16.0)).flex_none().children(row.expanded.map(|open| {
                            theme::icon(cx, if open { "chevron-down" } else { "chevron-right" }, 16.0, chevron_color)
                        }));
                        let detail = row.branch.as_ref().map(|branch| {
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .when(branch.current, |el| {
                                    el.child(div().text_color(subtle).child(SharedString::from(current_label.clone())))
                                })
                                .children(sync_marks(cx, index, branch.behind, branch.ahead))
                        });
                        div()
                            .id(index)
                            .debug_selector(move || format!("branches-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + 16.0 * row.depth as f32))
                            .pr(px(6.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(selected.contains(&index), |el| el.bg(selected_bg))
                            .child(chevron)
                            .child(theme::icon(cx, row_icon(row), 16.0, muted))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_color(fg)
                                    .child(SharedString::from(row.label.clone())),
                            )
                            .children(detail)
                            .on_click(cx.listener(move |pane, event: &ClickEvent, window, cx| {
                                window.focus(&pane.focus, cx);
                                pane.click(index, event.modifiers(), cx);
                                if event.click_count() >= 2 && !event.modifiers().shift {
                                    pane.activate(index, cx);
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |pane, event: &MouseDownEvent, _, cx| {
                                    pane.show_menu(index, event.position, cx);
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
            .debug_selector(|| "branches-tree".to_owned())
            .flex_1()
            .min_h_0()
            .track_focus(&self.focus)
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{MouseUpEvent, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BranchesPaneEvent>>>;

    /// A branch as the backend lists it.
    fn b(name: &str, current: bool, ahead: u32, behind: u32) -> Branch {
        let remote = name.starts_with("origin/");
        let wire = json!({
            "name": name,
            "remote": remote,
            "current": current,
            "ahead": ahead,
            "behind": behind,
            "lastCommitMs": 0,
        });
        serde_json::from_value(wire).expect("a branch in its wire form")
    }

    fn input(branches: Vec<Branch>, tags: &[&str]) -> BranchesPaneInput {
        let current = branches.iter().find(|branch| branch.current).map(|branch| branch.name.clone());
        BranchesPaneInput {
            repositories: vec![PaneRepository {
                root: "/p".to_owned(),
                name: "p".to_owned(),
                branches,
                tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            }],
            current,
            ..BranchesPaneInput::default()
        }
    }

    /// `main` (current) and `feature`.
    fn main_and_feature() -> BranchesPaneInput {
        input(vec![b("main", true, 0, 0), b("feature", false, 0, 0)], &[])
    }

    fn open(
        cx: &mut TestAppContext,
        shown: BranchesPaneInput,
    ) -> (Entity<LogBranchesPane>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| LogBranchesPane::new(cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &BranchesPaneEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        view.update(cx, |view, cx| view.set_input(shown, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn lines(view: &Entity<LogBranchesPane>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    /// The index of the row labelled `label`.
    fn row(view: &Entity<LogBranchesPane>, label: &str, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| view.rows().iter().position(|row| row.label == label))
            .unwrap_or_else(|| panic!("{label} is listed"))
    }

    fn click_row(view: &Entity<LogBranchesPane>, label: &str, modifiers: Modifiers, cx: &mut VisualTestContext) {
        let index = row(view, label, cx);
        cx.run_until_parked();
        let selector: &'static str = ROW_SELECTORS[index];
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
        cx.run_until_parked();
    }

    /// A first click, then a second press GPUI counts as a double click.
    fn double_click_row(view: &Entity<LogBranchesPane>, label: &str, cx: &mut VisualTestContext) {
        click_row(view, label, Modifiers::default(), cx);
        let index = row(view, label, cx);
        let position = cx.debug_bounds(ROW_SELECTORS[index]).expect("the row is drawn").center();
        let (button, modifiers) = (MouseButton::Left, Modifiers::default());
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
        cx.run_until_parked();
    }

    fn right_click_row(view: &Entity<LogBranchesPane>, label: &str, cx: &mut VisualTestContext) {
        let index = row(view, label, cx);
        cx.run_until_parked();
        let bounds = cx.debug_bounds(ROW_SELECTORS[index]).expect("the row is drawn");
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    const ROW_SELECTORS: [&str; 10] = [
        "branches-row-0",
        "branches-row-1",
        "branches-row-2",
        "branches-row-3",
        "branches-row-4",
        "branches-row-5",
        "branches-row-6",
        "branches-row-7",
        "branches-row-8",
        "branches-row-9",
    ];

    fn primary(cx: &mut VisualTestContext) -> Modifiers {
        cx.update(|_, cx| Modifiers::secondary_key(cx))
    }

    fn log_filter(events: &Events) -> Option<Vec<String>> {
        events.borrow().iter().rev().find_map(|event| match event {
            BranchesPaneEvent::SetLogFilter { branches, .. } => Some(branches.clone()),
            _ => None,
        })
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn selected(view: &Entity<LogBranchesPane>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.selected_keys())
    }

    fn labels(items: &[MenuItem<PaneCommand>]) -> Vec<String> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Item(entry) => Some(entry.label.clone()),
                MenuItem::Action(action) => action.label.clone(),
                MenuItem::Separator => None,
            })
            .collect()
    }

    fn last_menu(events: &Events) -> Option<Vec<MenuItem<PaneCommand>>> {
        events.borrow().iter().rev().find_map(|event| match event {
            BranchesPaneEvent::ShowMenu { items, .. } => Some(items.clone()),
            _ => None,
        })
    }

    #[gpui_kit::test]
    async fn offers_manage_remotes_on_the_remote_node_and_on_each_remote_like_idea(cx: &mut TestAppContext) {
        let mut shown = main_and_feature();
        shown.repositories[0].branches.push(b("origin/main", false, 0, 0));
        let (view, events, cx) = open(cx, shown);
        for label in ["Remote", "origin"] {
            events.borrow_mut().clear();
            right_click_row(&view, label, cx);
            let menu = last_menu(&events).unwrap_or_else(|| panic!("{label} has a menu"));
            assert_eq!(menu.len(), 1);
            assert_eq!(menu[0].action_id(), Some("vcs.manageRemotes"));
            assert_eq!(labels(&menu), ["Manage Remotes…"]);
        }
    }

    #[gpui_kit::test]
    async fn one_click_only_selects_the_branch(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, main_and_feature());
        click_row(&view, "feature", Modifiers::default(), cx);
        assert_eq!(log_filter(&events), None);
        assert_eq!(selected(&view, cx), ["b:feature"]);
    }

    #[gpui_kit::test]
    async fn renders_shared_branch_markers_for_ahead_and_behind(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, main_and_feature());
        for (ahead, behind) in [(2, 0), (0, 3), (2, 3), (0, 0)] {
            view.update(cx, |view, cx| view.set_input(input(vec![b("main", true, ahead, behind)], &[]), cx));
            cx.run_until_parked();
            let main = row(&view, "main", cx);
            assert_eq!(main, 2, "HEAD, Local, main");
            let case = format!("ahead {ahead} / behind {behind}");
            assert_eq!(cx.debug_bounds("branches-incoming-2").is_some(), behind > 0, "{case}");
            assert_eq!(cx.debug_bounds("branches-outgoing-2").is_some(), ahead > 0, "{case}");
            let line = lines(&view, cx)[main].clone();
            assert_eq!(line.contains(&format!("↓{behind}")), behind > 0);
            assert_eq!(line.contains(&format!("↑{ahead}")), ahead > 0);
        }
    }

    #[gpui_kit::test]
    async fn double_click_switches_the_log_to_that_branch(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, main_and_feature());
        double_click_row(&view, "feature", cx);
        assert_eq!(log_filter(&events), Some(vec!["feature".to_owned()]));
    }

    #[gpui_kit::test]
    async fn double_click_on_the_current_branch_switches_to_its_log_too(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, main_and_feature());
        double_click_row(&view, "main", cx);
        assert_eq!(log_filter(&events), Some(vec!["main".to_owned()]));
    }

    #[gpui_kit::test]
    async fn double_click_on_head_shows_heads_log(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, main_and_feature());
        double_click_row(&view, "HEAD (Current Branch)", cx);
        assert_eq!(log_filter(&events), Some(vec!["HEAD".to_owned()]));
    }

    #[gpui_kit::test]
    async fn enter_switches_the_log_to_every_selected_branch_in_tree_order(cx: &mut TestAppContext) {
        let shown = input(vec![b("main", true, 0, 0), b("beta", false, 0, 0), b("alpha", false, 0, 0)], &[]);
        let (view, events, cx) = open(cx, shown);
        click_row(&view, "beta", Modifiers::default(), cx);
        let primary = primary(cx);
        click_row(&view, "alpha", primary, cx);
        assert_eq!(log_filter(&events), None);
        cx.simulate_keystrokes("enter");
        assert_eq!(log_filter(&events), Some(vec!["alpha".to_owned(), "beta".to_owned()]));
    }

    #[gpui_kit::test]
    async fn follows_the_log_filter_when_it_changes_elsewhere(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, main_and_feature());
        click_row(&view, "main", Modifiers::default(), cx);
        assert_eq!(selected(&view, cx), ["b:main"]);
        let filtered = BranchesPaneInput { filter_branches: vec!["feature".to_owned()], ..main_and_feature() };
        view.update(cx, |view, cx| view.set_input(filtered, cx));
        assert_eq!(selected(&view, cx), ["b:feature"]);
    }

    #[gpui_kit::test]
    async fn copies_the_selected_branch_name(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, main_and_feature());
        click_row(&view, "feature", Modifiers::default(), cx);
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("feature"));
    }

    #[gpui_kit::test]
    async fn copies_head_as_head(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, main_and_feature());
        click_row(&view, "HEAD (Current Branch)", Modifiers::default(), cx);
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("HEAD"));
    }

    #[gpui_kit::test]
    async fn copies_the_folder_path_along_with_the_branch_name(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, input(vec![b("feat/idea-parity", true, 0, 0)], &[]));
        click_row(&view, "idea-parity", Modifiers::default(), cx);
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("feat/idea-parity"));
    }

    #[gpui_kit::test]
    async fn copies_every_selected_branch_one_per_line_in_tree_order(cx: &mut TestAppContext) {
        let shown = input(vec![b("main", true, 0, 0), b("beta", false, 0, 0), b("alpha", false, 0, 0)], &[]);
        let (view, _, cx) = open(cx, shown);
        let primary = primary(cx);
        click_row(&view, "beta", primary, cx);
        click_row(&view, "alpha", primary, cx);
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("alpha\nbeta"));
    }

    #[gpui_kit::test]
    async fn leaves_cmd_ctrl_c_in_the_branch_search_field_alone(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, main_and_feature());
        click_row(&view, "feature", Modifiers::default(), cx);
        cx.update(|window, cx| {
            let field = view.read(cx).search_field().focus_handle(cx);
            window.focus(&field, cx);
        });
        cx.simulate_keystrokes("cmd-c");
        assert_ne!(clipboard(cx).as_deref(), Some("feature"));
    }

    #[test]
    fn enables_update_and_delete_for_the_branches_they_apply_to() {
        let target =
            |ref_name: &str, branch: Option<Branch>| BranchTarget { ref_name: ref_name.to_owned(), repo: None, branch };
        let tracked = Branch { upstream: Some("origin/feature".to_owned()), ..b("feature", false, 0, 0) };
        assert!(!can_delete_branch(Some(&target("main", Some(b("main", true, 0, 0))))));
        assert!(can_delete_branch(Some(&target("feature", Some(tracked.clone())))));
        assert!(!can_delete_branch(Some(&target("v1", None))));
        assert!(can_update_branch(Some(&target("feature", Some(tracked)))));
        assert!(!can_update_branch(Some(&target("x", Some(b("x", false, 0, 0))))));
        assert!(!can_update_branch(Some(&target("origin/x", Some(b("origin/x", false, 0, 0))))));
    }

    #[gpui_kit::test]
    async fn context_delete_respects_membership_in_the_selection(cx: &mut TestAppContext) {
        for inside in [true, false] {
            let branches = ["main", "one", "two", "outside"].map(|name| b(name, name == "main", 0, 0)).to_vec();
            let (view, events, cx) = open(cx, input(branches, &["v1"]));
            double_click_row(&view, "Tags", cx);
            click_row(&view, "one", Modifiers::default(), cx);
            let primary = primary(cx);
            click_row(&view, "two", primary, cx);
            click_row(&view, "v1", primary, cx);
            click_row(&view, "Local", primary, cx);
            right_click_row(&view, if inside { "two" } else { "outside" }, cx);
            let menu = last_menu(&events).expect("the branch has a menu");
            let delete = menu.iter().find_map(|item| match item {
                MenuItem::Item(entry) if entry.label == "Delete" => entry.run.clone(),
                _ => None,
            });
            let Some(PaneCommand::Branch { command: BranchCommand::DeleteSelection(targets), .. }) = delete else {
                panic!("Delete acts on a selection: {delete:?}");
            };
            let refs: Vec<String> = targets.into_iter().map(|target| target.ref_name).collect();
            assert_eq!(refs, if inside { vec!["one", "two", "v1"] } else { vec!["outside"] });
        }
    }

    #[gpui_kit::test]
    async fn has_a_vertical_toolbar_acting_on_the_selected_branch_and_shows_tags(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, input(vec![b("main", true, 0, 0), b("feature", false, 0, 0)], &["v1"]));
        let delete_enabled = |view: &Entity<LogBranchesPane>, cx: &mut VisualTestContext| {
            let toolbar = view.read_with(cx, |view, _| view.toolbar());
            toolbar.iter().any(|button| button.id == DELETE_BRANCH && button.enabled)
        };
        let ids: Vec<&str> = view.read_with(cx, |view, _| view.toolbar().iter().map(|button| button.id).collect());
        let all = [SHOW_BRANCHES, NEW_BRANCH, UPDATE_SELECTED, DELETE_BRANCH, NAVIGATE, TOGGLE_FAVORITE, FETCH];
        assert_eq!(ids, [all.as_slice(), &[REFRESH]].concat());
        assert!(!delete_enabled(&view, cx));
        click_row(&view, "main", Modifiers::default(), cx);
        assert!(!delete_enabled(&view, cx));
        right_click_row(&view, "main", cx);
        let menu = last_menu(&events).expect("the current branch has a menu");
        assert!(!labels(&menu).contains(&"Delete".to_owned()));
        click_row(&view, "feature", Modifiers::default(), cx);
        let targets = view.read_with(cx, |view, _| view.targets());
        let selected: Vec<String> = targets.into_iter().map(|target| target.ref_name).collect();
        assert_eq!(selected, ["feature"]);
        assert!(delete_enabled(&view, cx));
        click(cx, "branches-delete");
        let deleted = events.borrow().iter().rev().find_map(|event| match event {
            BranchesPaneEvent::RunAction { id: DELETE_BRANCH, arg } => arg.clone(),
            _ => None,
        });
        assert_eq!(deleted.and_then(|arg| arg[0]["ref"].as_str().map(str::to_owned)).as_deref(), Some("feature"));
        assert!(lines(&view, cx).contains(&"Tags".to_owned()));
        click(cx, "branches-hide");
        let last = events.borrow().last().cloned();
        assert!(matches!(last, Some(BranchesPaneEvent::RunAction { id: SHOW_BRANCHES, arg: None })));
    }

    // Not from a spec: the search keeps the matching refs with their folders open, and says when
    // nothing matches.
    #[gpui_kit::test]
    async fn searches_branches_and_tags(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, input(vec![b("main", true, 0, 0), b("feat/login", false, 0, 0)], &["v1.9"]));
        let field = view.read_with(cx, |view, _| view.search_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input("login");
        assert_eq!(lines(&view, cx), ["Local", "  feat", "    login"]);
        cx.simulate_input("zzz");
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No matching branches"));
        cx.simulate_keystrokes("escape");
        assert_eq!(view.read_with(cx, |view, _| view.query().to_owned()), "");
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }
}
