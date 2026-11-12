//! The welcome screen as data: the recent projects a search text leaves, in the order they are
//! listed, the groups they are filed under, the keys of the list, the rows of its menus and the
//! buttons beside the search field. It also decides where a recent project opens (in a folder
//! or through a remote target) and which project a window goes back to when the app starts.

use crate::platform::i18n::I18n;
use crate::platform::misc::file_manager::show_in_file_manager_label;
use crate::platform::misc::platform_info::Os;
use crate::platform_more::remote::{RemoteTarget, parse_remote_location, target_name};
use crate::project_tree_paths::tildify;
use crate::ui::matching::fuzzy_match;
use crate::vcs_ops::menu::MenuItem;
use ide_workspace::{RecentProject, RecentProjectGroup};
use std::path::PathBuf;

/// How many rows Page Up and Page Down move by.
pub const PAGE_SIZE: usize = 8;

/// The action behind the Open button: it is offered whether or not anything handles it.
pub const OPEN_PROJECT: &str = "file.openProject";

/// A recent project as the welcome screen lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// Its folder, or the location of a remote project.
    pub path: String,
    pub name: String,
    /// The git branch it was on when it was last open.
    pub branch: Option<String>,
}

impl RecentEntry {
    /// Whether the project lives on another machine: it has no local folder to show or to group.
    pub fn is_remote(&self) -> bool {
        parse_remote_location(&self.path).is_some()
    }
}

impl From<&RecentProject> for RecentEntry {
    fn from(project: &RecentProject) -> Self {
        Self {
            path: project.path.0.to_string_lossy().into_owned(),
            name: project.name.clone(),
            branch: project.branch.clone(),
        }
    }
}

/// A named group of recent projects, listed above the projects that are in no group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGroup {
    pub name: String,
    /// Its projects, in the order they were added.
    pub paths: Vec<String>,
    pub expanded: bool,
}

impl ProjectGroup {
    fn has(&self, path: &str) -> bool {
        self.paths.iter().any(|member| member.as_str() == path)
    }
}

impl From<&RecentProjectGroup> for ProjectGroup {
    fn from(group: &RecentProjectGroup) -> Self {
        Self {
            name: group.name.clone(),
            paths: group.paths.iter().map(|path| path.0.to_string_lossy().into_owned()).collect(),
            expanded: group.expanded,
        }
    }
}

impl From<&ProjectGroup> for RecentProjectGroup {
    fn from(group: &ProjectGroup) -> Self {
        Self {
            name: group.name.clone(),
            paths: group.paths.iter().map(|path| PathBuf::from(path).into()).collect(),
            expanded: group.expanded,
        }
    }
}

/// A recent project the search text leaves in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMatch {
    pub project: RecentEntry,
    /// The byte offsets of the matched characters of the name, when the name matched.
    pub name_match: Option<Vec<usize>>,
    /// The byte offsets of the matched characters of [`Self::display_path`], when only it matched.
    pub path_match: Option<Vec<usize>>,
    /// The path as it is shown: under `~` in the home directory, `host:path` for a remote project.
    pub display_path: String,
}

/// The path shown under a project's name.
pub fn display_path(path: &str, home: Option<&str>) -> String {
    match parse_remote_location(path) {
        Some((target, remote_path)) => format!("{}:{remote_path}", target_name(&target)),
        None => tildify(path, home),
    }
}

/// Whether the matched characters lie close together: a subsequence scattered over a long path
/// is noise. The room allowed is counted in UTF-16 units, six per matched character.
fn is_compact_match(text: &str, indices: &[usize]) -> bool {
    let [first, .., last] = indices else { return true };
    text.get(*first..*last).is_some_and(|between| between.encode_utf16().count() < indices.len() * 6)
}

/// The projects `query` matches: those whose name matches first, then those whose shown path
/// does, each in the order given (the most recent first). An empty query leaves them all.
pub fn filter_recent(projects: &[RecentEntry], query: &str, home: Option<&str>) -> Vec<RecentMatch> {
    let query = query.trim();
    let all = projects.iter().map(|project| RecentMatch {
        project: project.clone(),
        name_match: None,
        path_match: None,
        display_path: display_path(&project.path, home),
    });
    if query.is_empty() {
        return all.collect();
    }
    let (mut by_name, mut by_path) = (Vec::new(), Vec::new());
    for found in all {
        if let Some(name) = fuzzy_match(&found.project.name, query) {
            by_name.push(RecentMatch { name_match: Some(name), ..found });
        } else if let Some(path) = fuzzy_match(&found.display_path, query)
            && is_compact_match(&found.display_path, &path)
        {
            by_path.push(RecentMatch { path_match: Some(path), ..found });
        }
    }
    by_name.extend(by_path);
    by_name
}

/// A row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentRow {
    /// A group, with how many of the listed projects are in it.
    Group { group: ProjectGroup, count: usize },
    /// A project; `group` names the group it is listed under.
    Project { found: RecentMatch, group: Option<String> },
}

impl RecentRow {
    /// The row as one line of text: `[Work 2]` for a group, a project's name, indented under a group.
    pub fn line(&self) -> String {
        match self {
            Self::Group { group, count } => format!("[{} {count}]", group.name),
            Self::Project { found, group: Some(_) } => format!("  {}", found.project.name),
            Self::Project { found, group: None } => found.project.name.clone(),
        }
    }

    pub fn project(&self) -> Option<&RecentEntry> {
        match self {
            Self::Project { found, .. } => Some(&found.project),
            Self::Group { .. } => None,
        }
    }
}

/// The rows of the list. Without a search text the groups come first, each expanded one followed
/// by its projects (the most recent first), then the projects in no group; a search text lists
/// the matches only.
pub fn recent_rows(
    projects: &[RecentEntry],
    groups: &[ProjectGroup],
    query: &str,
    home: Option<&str>,
) -> Vec<RecentRow> {
    let matches = filter_recent(projects, query, home);
    if !query.trim().is_empty() || groups.is_empty() {
        return matches.into_iter().map(|found| RecentRow::Project { found, group: None }).collect();
    }
    let mut rows = Vec::new();
    for group in groups {
        let members: Vec<&RecentMatch> = matches.iter().filter(|found| group.has(&found.project.path)).collect();
        rows.push(RecentRow::Group { group: group.clone(), count: members.len() });
        if group.expanded {
            rows.extend(
                members
                    .into_iter()
                    .map(|found| RecentRow::Project { found: found.clone(), group: Some(group.name.clone()) }),
            );
        }
    }
    let ungrouped = matches.into_iter().filter(|found| group_of(&found.project.path, groups).is_none());
    rows.extend(ungrouped.map(|found| RecentRow::Project { found, group: None }));
    rows
}

/// The projects and the groups once `path` is no longer a recent project.
pub fn remove_recent(
    projects: &[RecentEntry],
    groups: &[ProjectGroup],
    path: &str,
) -> (Vec<RecentEntry>, Vec<ProjectGroup>) {
    (projects.iter().filter(|project| project.path != path).cloned().collect(), move_to_group(groups, path, None))
}

// ---- Groups ----

/// The group `path` is in.
pub fn group_of<'a>(path: &str, groups: &'a [ProjectGroup]) -> Option<&'a ProjectGroup> {
    groups.iter().find(|group| group.has(path))
}

/// Why a text cannot name a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupNameError {
    Empty,
    /// Another group has the name.
    Exists(String),
}

impl GroupNameError {
    /// What the user is told.
    pub fn text(&self, i18n: &mut I18n) -> String {
        match self {
            Self::Empty => i18n.tx("Enter a group name"),
            Self::Exists(name) => i18n.tx_built(&["A group named '", "' already exists"], &[name.as_str().into()]),
        }
    }
}

/// Why `value` cannot name a new group, or the group `current` once renamed.
pub fn group_name_error(groups: &[ProjectGroup], value: &str, current: Option<&str>) -> Option<GroupNameError> {
    let name = value.trim();
    if name.is_empty() {
        return Some(GroupNameError::Empty);
    }
    let taken = Some(name) != current && groups.iter().any(|group| group.name == name);
    taken.then(|| GroupNameError::Exists(name.to_owned()))
}

/// The groups with a new one called `name` at their end, holding the project at `path` when one
/// is given (it leaves the group it was in). `None` when the name cannot be used.
pub fn create_group(groups: &[ProjectGroup], name: &str, path: Option<&str>) -> Option<Vec<ProjectGroup>> {
    let name = name.trim();
    if group_name_error(groups, name, None).is_some() {
        return None;
    }
    let mut next = match path {
        Some(path) => move_to_group(groups, path, None),
        None => groups.to_vec(),
    };
    next.push(ProjectGroup {
        name: name.to_owned(),
        paths: path.map(str::to_owned).into_iter().collect(),
        expanded: true,
    });
    Some(next)
}

/// The groups with `name` called `to`. `None` when nothing changes or the name cannot be used.
pub fn rename_group(groups: &[ProjectGroup], name: &str, to: &str) -> Option<Vec<ProjectGroup>> {
    let to = to.trim();
    if to == name || group_name_error(groups, to, Some(name)).is_some() {
        return None;
    }
    let renamed = |group: &ProjectGroup| {
        let mut group = group.clone();
        if group.name == name {
            group.name = to.to_owned();
        }
        group
    };
    Some(groups.iter().map(renamed).collect())
}

/// The groups without `name`. Its projects stay in the list, in no group.
pub fn remove_group(groups: &[ProjectGroup], name: &str) -> Vec<ProjectGroup> {
    groups.iter().filter(|group| group.name != name).cloned().collect()
}

/// The groups with `path` in group `name` only, or in none.
pub fn move_to_group(groups: &[ProjectGroup], path: &str, name: Option<&str>) -> Vec<ProjectGroup> {
    groups
        .iter()
        .map(|group| {
            let mut paths: Vec<String> = group.paths.iter().filter(|member| member.as_str() != path).cloned().collect();
            if Some(group.name.as_str()) == name {
                paths.push(path.to_owned());
            }
            ProjectGroup { name: group.name.clone(), paths, expanded: group.expanded }
        })
        .collect()
}

/// The groups with `name` folded or unfolded.
pub fn set_group_expanded(groups: &[ProjectGroup], name: &str, expanded: bool) -> Vec<ProjectGroup> {
    let folded = |group: &ProjectGroup| {
        let mut group = group.clone();
        if group.name == name {
            group.expanded = expanded;
        }
        group
    };
    groups.iter().map(folded).collect()
}

// ---- Keys ----

/// The modifiers the list's keys look at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    /// ⌘ on macOS, the Windows key elsewhere.
    pub platform: bool,
    pub shift: bool,
}

/// What a key does to the list. Each carries the index of the row it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Select this row.
    Move(usize),
    /// Open the selected project, fold or unfold the selected group.
    Open(usize),
    /// Ask whether to remove the selected project.
    Remove(usize),
    /// Show the menu of the selected row.
    Menu(usize),
}

/// The row a navigation key selects in a list of `len` rows with row `active` selected; `None`
/// for any other key. The selection stops at both ends.
pub fn list_navigate(key: &str, len: usize, active: usize, page: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match key {
        "down" => (active + 1).min(last),
        "up" => active.saturating_sub(1),
        "home" => 0,
        "end" => last,
        "pagedown" => (active + page).min(last),
        "pageup" => active.saturating_sub(page),
        _ => return None,
    })
}

/// What `key` does while the search field has the keyboard: Enter opens, Delete with the caret
/// at the end of the search text and ⌘Backspace remove, Shift+F10 shows the row's menu, and the
/// arrows, Home, End and the paging keys move the selection. `None` leaves the key to the field.
pub fn handle_list_key(key: &str, mods: KeyMods, len: usize, current: usize, caret_at_end: bool) -> Option<ListKey> {
    let on_row = current < len;
    if key == "enter" {
        return on_row.then_some(ListKey::Open(current));
    }
    if on_row && ((key == "delete" && caret_at_end) || (key == "backspace" && mods.platform)) {
        return Some(ListKey::Remove(current));
    }
    if on_row && key == "f10" && mods.shift {
        return Some(ListKey::Menu(current));
    }
    list_navigate(key, len, current, PAGE_SIZE).map(ListKey::Move)
}

/// Whether `key` folds (`false`) or unfolds (`true`) the selected row: ← and → do on a group,
/// like on a tree node, while the search field is empty and its caret has nowhere to go.
pub fn fold_key(key: &str, row: Option<&RecentRow>, search_is_empty: bool) -> Option<bool> {
    if !search_is_empty || !matches!(row, Some(RecentRow::Group { .. })) {
        return None;
    }
    match key {
        "left" => Some(false),
        "right" => Some(true),
        _ => None,
    }
}

// ---- Buttons ----

/// A button of the welcome screen: it runs a registered action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WelcomeAction {
    pub id: &'static str,
    pub label: &'static str,
    /// The icon it has on the page shown without recent projects.
    pub icon: &'static str,
}

const ACTIONS: [WelcomeAction; 3] = [
    WelcomeAction { id: OPEN_PROJECT, label: "Open", icon: "folder-open" },
    WelcomeAction { id: "vcs.clone", label: "Clone Repository", icon: "branch" },
    WelcomeAction { id: "remote.connect", label: "Remote Development", icon: "terminal" },
];

/// The buttons shown: Open always, the others when their action is registered.
pub fn welcome_actions(is_registered: impl Fn(&str) -> bool) -> Vec<WelcomeAction> {
    ACTIONS.into_iter().filter(|action| action.id == OPEN_PROJECT || is_registered(action.id)).collect()
}

/// What the page without recent projects says can be done.
pub fn landing_text(can_clone: bool) -> &'static str {
    if can_clone {
        "Open a folder from disk or clone a repository from version control."
    } else {
        "Open a folder from disk."
    }
}

// ---- Menus ----

/// What a row of the welcome screen's menus does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeCommand {
    /// Open the project at this path.
    Open(String),
    /// Show the project's folder in the file manager.
    Reveal(String),
    CopyPath(String),
    /// Put the project in this group, or in none.
    MoveToGroup {
        path: String,
        group: Option<String>,
    },
    /// Ask for the name of a new group; the project at `path` goes into it.
    NewGroup {
        path: Option<String>,
    },
    /// Ask whether to remove the project from the recent projects.
    AskRemove(String),
    /// Ask for the new name of this group.
    RenameGroup(String),
    RemoveGroup(String),
}

/// The English label of the row that takes a project out of the group `group`.
pub fn remove_from_group_label(group: &str) -> String {
    format!("Remove from '{group}'")
}

/// Move to Group: the other groups, a new one, and out of the group the project is in.
fn move_to_group_menu(project: &RecentEntry, groups: &[ProjectGroup]) -> MenuItem<WelcomeCommand> {
    let current = group_of(&project.path, groups).map(|group| group.name.as_str());
    let path = || project.path.clone();
    let mut submenu: Vec<MenuItem<WelcomeCommand>> = groups
        .iter()
        .filter(|group| Some(group.name.as_str()) != current)
        .map(|group| {
            MenuItem::item(&group.name, WelcomeCommand::MoveToGroup { path: path(), group: Some(group.name.clone()) })
        })
        .collect();
    if !submenu.is_empty() {
        submenu.push(MenuItem::Separator);
    }
    submenu.push(MenuItem::item("New Group…", WelcomeCommand::NewGroup { path: Some(path()) }));
    if let Some(current) = current {
        submenu.push(MenuItem::item(
            remove_from_group_label(current),
            WelcomeCommand::MoveToGroup { path: path(), group: None },
        ));
    }
    MenuItem::submenu("Move to Group", submenu).icon("folder")
}

/// The menu of a project: opening it here or in a new window, its folder in the file manager of
/// `os`, its path, its group, and removing it from the list. A remote project has no local
/// folder to show, is in no group, and opens in a window of its own only through its target.
pub fn project_menu(project: &RecentEntry, groups: &[ProjectGroup], os: Os) -> Vec<MenuItem<WelcomeCommand>> {
    let path = || project.path.clone();
    let mut items = vec![MenuItem::item("Open", WelcomeCommand::Open(path())).icon("folder-open")];
    if !project.is_remote() {
        items.push(MenuItem::action_with("file.openInNewWindow", path().into()).labelled("Open in New Window"));
        items
            .push(MenuItem::item(show_in_file_manager_label(os), WelcomeCommand::Reveal(path())).icon("external-link"));
    }
    items.push(MenuItem::item("Copy Path", WelcomeCommand::CopyPath(path())).icon("copy"));
    items.push(MenuItem::Separator);
    if !project.is_remote() {
        items.push(move_to_group_menu(project, groups));
        items.push(MenuItem::Separator);
    }
    items
        .push(MenuItem::item("Remove from Recent Projects…", WelcomeCommand::AskRemove(path())).icon("close").danger());
    items
}

/// The menu of a group.
pub fn group_menu(group: &ProjectGroup) -> Vec<MenuItem<WelcomeCommand>> {
    vec![
        MenuItem::item("Rename Group…", WelcomeCommand::RenameGroup(group.name.clone())).icon("edit"),
        MenuItem::item("New Project Group…", WelcomeCommand::NewGroup { path: None }).icon("plus"),
        MenuItem::Separator,
        MenuItem::item("Remove Group", WelcomeCommand::RemoveGroup(group.name.clone())).icon("close").danger(),
    ]
}

/// The menu of the Manage button: settings, the keymap, plugins, help and the app itself.
pub fn manage_menu() -> Vec<MenuItem<WelcomeCommand>> {
    vec![
        MenuItem::action("file.settings"),
        MenuItem::action("settings.keymap"),
        MenuItem::action("plugins.installFromDisk"),
        MenuItem::Separator,
        MenuItem::action("help.keymapReference"),
        MenuItem::action("help.showLog"),
        MenuItem::action("app.checkForUpdates"),
        MenuItem::Separator,
        MenuItem::action("app.about"),
    ]
}

// ---- Opening ----

/// Where a recent project opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRoute {
    /// As a folder of this machine.
    Local(String),
    /// Through a remote target, at this path on it.
    Remote { target: RemoteTarget, path: String },
}

/// Where the project at `location` opens. A remote location names its target itself; any other
/// is asked of `resolve_wsl`, which knows the folders that are really inside a WSL distribution
/// (a project saved by its `\\wsl.localhost\…` share).
pub fn open_route(location: &str, resolve_wsl: impl FnOnce(&str) -> Option<(RemoteTarget, String)>) -> OpenRoute {
    match parse_remote_location(location).or_else(|| resolve_wsl(location)) {
        Some((target, path)) => OpenRoute::Remote { target, path },
        None => OpenRoute::Local(location.to_owned()),
    }
}

// ---- The project a window goes back to ----

/// Whether the last project reopens when the app starts: what `general.reopenLastProject` says
/// when the settings have it, else the preference kept on this machine.
pub fn reopen_enabled(setting: Option<bool>, preference: bool) -> bool {
    setting.unwrap_or(preference)
}

/// The projects remembered between runs. Closing a project forgets it, so the next start shows
/// the welcome screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMemory {
    /// The project that was open when the app last quit.
    pub last_project: Option<String>,
    /// The project this window shows; it gets it back when the window is rebuilt.
    pub window_project: Option<String>,
}

impl SessionMemory {
    /// The window went from showing `before` to showing `now` (locations; `None`: no project).
    /// While a project is being reopened, the window showing nothing yet forgets nothing.
    pub fn project_changed(&mut self, before: Option<&str>, now: Option<&str>, reopening: bool) {
        if now == before {
            return;
        }
        match now {
            Some(now) => self.last_project = Some(now.to_owned()),
            None if !reopening => self.last_project = None,
            None => {}
        }
        if now.is_some() || !reopening {
            self.window_project = now.map(str::to_owned);
        }
    }

    fn forget(&mut self) {
        *self = Self::default();
    }

    fn remember(&mut self, location: &str) {
        self.last_project = Some(location.to_owned());
        self.window_project = Some(location.to_owned());
    }
}

/// The project a window reopens when the app starts, once the launch has opened what it named.
/// A window that the launch gave a project or loose files (`launched`) keeps them; a rebuilt
/// window gets its own project back; the main window gets the last session's project when
/// reopening is `enabled`; any other window was opened for a project of its own.
pub fn startup_project(memory: &SessionMemory, launched: bool, main_window: bool, enabled: bool) -> Option<&str> {
    if launched {
        return None;
    }
    if let Some(own) = memory.window_project.as_deref() {
        return Some(own);
    }
    if !main_window || !enabled {
        return None;
    }
    memory.last_project.as_deref()
}

/// What to do with a project that finished opening for a reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reopened {
    /// It is the window's project now, and remembered.
    Keep,
    /// The reopen was cancelled while it opened and the window still shows it: close it again.
    Close,
    /// The reopen was cancelled and the window shows something else since: leave that alone.
    Superseded,
}

/// A startup reopen in flight. Opening cannot be interrupted, so cancelling one only makes its
/// outcome stale: each reopen has a ticket, and an outcome with an old ticket is not kept.
#[derive(Debug, Default)]
pub struct Reopen {
    ticket: u64,
    reopening: Option<String>,
}

impl Reopen {
    /// The location being reopened: the window shows that instead of the welcome screen.
    pub fn reopening(&self) -> Option<&str> {
        self.reopening.as_deref()
    }

    /// Starts reopening `location` and returns its ticket. The project is forgotten until it is
    /// open, so that quitting a reopen that hangs does not try it again on the next start.
    pub fn begin(&mut self, location: &str, memory: &mut SessionMemory) -> u64 {
        self.ticket += 1;
        memory.forget();
        self.reopening = Some(location.to_owned());
        self.ticket
    }

    /// Gives up the reopen in flight: the window shows the welcome screen again.
    pub fn cancel(&mut self, memory: &mut SessionMemory) {
        self.ticket += 1;
        memory.forget();
        self.reopening = None;
    }

    /// The project of `ticket` is open at `opened`; `current` is the location the window shows.
    pub fn opened(&mut self, ticket: u64, opened: &str, current: Option<&str>, memory: &mut SessionMemory) -> Reopened {
        if ticket != self.ticket {
            return if current == Some(opened) { Reopened::Close } else { Reopened::Superseded };
        }
        memory.remember(current.unwrap_or(opened));
        self.reopening = None;
        Reopened::Keep
    }

    /// The project of `ticket` could not be opened. Returns whether to tell the user: not when
    /// the reopen was cancelled, by them or by the open itself.
    pub fn failed(&mut self, ticket: u64, cancelled: bool, memory: &mut SessionMemory) -> bool {
        memory.forget();
        if ticket != self.ticket {
            return false;
        }
        self.reopening = None;
        !cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: Option<&str> = Some("/Users/me");
    const ACME: &str = "/Users/me/dev/acme";
    const OPENIDEA: &str = "/Users/me/dev/openidea";
    const TOKIO: &str = "/Users/me/oss/tokio";
    const UNC: &str = r"\\wsl.localhost\Ubuntu\home\me\app";

    fn project(name: &str, path: &str, branch: Option<&str>) -> RecentEntry {
        RecentEntry { name: name.to_owned(), path: path.to_owned(), branch: branch.map(str::to_owned) }
    }

    fn projects() -> Vec<RecentEntry> {
        vec![
            project("acme-api", ACME, Some("main")),
            project("openidea-ide", OPENIDEA, None),
            project("tokio", TOKIO, None),
            project("notes", "/Volumes/data/acme-notes", None),
        ]
    }

    fn names(matches: &[RecentMatch]) -> Vec<&str> {
        matches.iter().map(|found| found.project.name.as_str()).collect()
    }

    fn lines(groups: &[ProjectGroup], query: &str) -> Vec<String> {
        recent_rows(&projects(), groups, query, HOME).iter().map(RecentRow::line).collect()
    }

    fn group_names(groups: &[ProjectGroup]) -> Vec<&str> {
        groups.iter().map(|group| group.name.as_str()).collect()
    }

    /// The folders that are inside the Ubuntu distribution.
    fn wsl(location: &str) -> Option<(RemoteTarget, String)> {
        (location == UNC).then(|| (RemoteTarget::Wsl { distro: "Ubuntu".to_owned() }, "/home/me/app".to_owned()))
    }

    // src/features/welcome/recent.test.ts › filterRecent › returns everything in recency order for an empty query
    #[test]
    fn returns_everything_in_recency_order_for_an_empty_query() {
        let all = filter_recent(&projects(), "  ", HOME);
        assert_eq!(names(&all), ["acme-api", "openidea-ide", "tokio", "notes"]);
        assert_eq!(all[0].display_path, "~/dev/acme");
        assert_eq!(all[3].display_path, "/Volumes/data/acme-notes");
    }

    // src/features/welcome/recent.test.ts › filterRecent ›
    // puts name matches first, then path matches, with highlight indices
    #[test]
    fn puts_name_matches_first_then_path_matches_with_highlight_indices() {
        let found = filter_recent(&projects(), "acme", HOME);
        assert_eq!(names(&found), ["acme-api", "notes"]);
        assert_eq!(found[0].name_match, Some(vec![0, 1, 2, 3]));
        assert_eq!(found[1].name_match, None);
        assert_eq!(found[1].path_match, Some(vec![14, 15, 16, 17]));
    }

    // src/features/welcome/recent.test.ts › filterRecent › matches camel-hump / word starts and is case-insensitive
    #[test]
    fn matches_camel_hump_word_starts_and_is_case_insensitive() {
        assert_eq!(names(&filter_recent(&projects(), "EIde", None)), ["openidea-ide"]);
        assert_eq!(names(&filter_recent(&projects(), "TOK", None)), ["tokio"]);
        assert!(filter_recent(&projects(), "zzz", None).is_empty());
    }

    // src/features/welcome/recent.test.ts › filterRecent › ignores scattered path subsequences
    #[test]
    fn ignores_scattered_path_subsequences() {
        let scattered = project("x", "/a/bcdefghijklmnopqrstuvwxyz/o/z", None);
        assert!(filter_recent(std::slice::from_ref(&scattered), "az", None).is_empty());
    }

    // src/features/welcome/recent.test.ts › helpers › abbreviates the home directory
    #[test]
    fn abbreviates_the_home_directory() {
        assert_eq!(display_path("/Users/me/dev", HOME), "~/dev");
        assert_eq!(display_path("/Users/meow", HOME), "/Users/meow");
        assert_eq!(display_path("/x", None), "/x");
    }

    // src/features/welcome/recent.test.ts › session restore › prefers the settings key over the local preference
    #[test]
    fn prefers_the_settings_key_over_the_local_preference() {
        assert!(reopen_enabled(None, true));
        assert!(!reopen_enabled(None, false));
        assert!(!reopen_enabled(Some(false), true));
        assert!(reopen_enabled(Some(true), false));
    }

    // src/features/welcome/recent.test.ts › session restore ›
    // remembers the open project and forgets it on explicit close
    #[test]
    fn remembers_the_open_project_and_forgets_it_on_explicit_close() {
        let mut memory = SessionMemory::default();
        memory.project_changed(None, Some("/w/a"), false);
        assert_eq!(memory.last_project.as_deref(), Some("/w/a"));
        memory.project_changed(Some("/w/a"), Some("/w/b"), false);
        assert_eq!(memory.last_project.as_deref(), Some("/w/b"));
        memory.project_changed(Some("/w/b"), None, false);
        assert_eq!(memory, SessionMemory::default());
    }

    // src/features/welcome/recent.test.ts › project groups ›
    // lists groups first with their projects, then the rest; a search lists matches only
    #[test]
    fn lists_groups_first_with_their_projects_then_the_rest_a_search_lists_matches_only() {
        assert_eq!(lines(&[], ""), ["acme-api", "openidea-ide", "tokio", "notes"]);
        let groups = create_group(&[], " Work ", Some(ACME)).expect("the name is free");
        let groups = move_to_group(&groups, OPENIDEA, Some("Work"));
        let groups = create_group(&groups, "Oss", Some(TOKIO)).expect("the name is free");
        assert_eq!(lines(&groups, ""), ["[Work 2]", "  acme-api", "  openidea-ide", "[Oss 1]", "  tokio", "notes"]);
        assert_eq!(lines(&groups, "tok"), ["tokio"]);

        let groups = set_group_expanded(&groups, "Work", false);
        assert_eq!(lines(&groups, ""), ["[Work 2]", "[Oss 1]", "  tokio", "notes"]);
    }

    // src/features/welcome/recent.test.ts › project groups ›
    // moves a project between groups, renames and removes groups without dropping projects
    #[test]
    fn moves_a_project_between_groups_renames_and_removes_groups_without_dropping_projects() {
        let groups = create_group(&[], "Work", Some(ACME)).expect("the name is free");
        let groups = create_group(&groups, "Oss", None).expect("the name is free");
        let mut i18n = I18n::default();
        let error = |value: &str, current: Option<&str>, i18n: &mut I18n| {
            group_name_error(&groups, value, current).map(|error| error.text(i18n))
        };
        assert_eq!(error("work ", None, &mut i18n), None);
        assert_eq!(error(" Work", None, &mut i18n).as_deref(), Some("A group named 'Work' already exists"));
        assert_eq!(error("Work", Some("Work"), &mut i18n), None);
        assert_eq!(error("  ", None, &mut i18n).as_deref(), Some("Enter a group name"));
        // A second group of the same name is not created.
        assert_eq!(create_group(&groups, "Work", Some(TOKIO)), None);
        assert_eq!(group_names(&groups), ["Work", "Oss"]);

        let groups = move_to_group(&groups, ACME, Some("Oss"));
        assert_eq!(lines(&groups, ""), ["[Work 0]", "[Oss 1]", "  acme-api", "openidea-ide", "tokio", "notes"]);
        let groups = rename_group(&groups, "Oss", "Open source").expect("the name is free");
        let groups = move_to_group(&groups, TOKIO, Some("Open source"));
        let groups = remove_group(&groups, "Work");
        assert_eq!(lines(&groups, ""), ["[Open source 2]", "  acme-api", "  tokio", "openidea-ide", "notes"]);
        let groups = move_to_group(&groups, ACME, None);
        let groups = remove_group(&groups, "Open source");
        assert_eq!(lines(&groups, ""), ["acme-api", "openidea-ide", "tokio", "notes"]);
    }

    // src/features/welcome/openRecent.test.ts › openRecent ›
    // opens a legacy WSL UNC recent through the distro, not as a local folder
    #[test]
    fn opens_a_legacy_wsl_unc_recent_through_the_distro_not_as_a_local_folder() {
        assert_eq!(
            open_route(UNC, wsl),
            OpenRoute::Remote {
                target: RemoteTarget::Wsl { distro: "Ubuntu".to_owned() },
                path: "/home/me/app".to_owned()
            }
        );
    }

    // src/features/welcome/openRecent.test.ts › openRecent › still opens a plain local recent locally
    #[test]
    fn still_opens_a_plain_local_recent_locally() {
        assert_eq!(open_route("/p/local", wsl), OpenRoute::Local("/p/local".to_owned()));
    }

    fn last_session(window_project: Option<&str>) -> SessionMemory {
        SessionMemory { last_project: Some("/p/last".to_owned()), window_project: window_project.map(str::to_owned) }
    }

    // src/features/welcome/session.test.ts › reopenLastProject ›
    // waits for the command-line project instead of opening the last one too
    #[test]
    fn waits_for_the_command_line_project_instead_of_opening_the_last_one_too() {
        // The question is asked once the launch is over, with whether it gave the window a project.
        assert_eq!(startup_project(&last_session(None), true, true, true), None);
    }

    // src/features/welcome/session.test.ts › reopenLastProject ›
    // keeps launched loose files instead of restoring the last session project
    #[test]
    fn keeps_launched_loose_files_instead_of_restoring_the_last_session_project() {
        assert_eq!(startup_project(&last_session(None), true, true, true), None);
    }

    // src/features/welcome/session.test.ts › reopenLastProject ›
    // keeps launched loose files instead of restoring the window session project
    #[test]
    fn keeps_launched_loose_files_instead_of_restoring_the_window_session_project() {
        assert_eq!(startup_project(&last_session(Some("/p/window")), true, true, true), None);
    }

    // src/features/welcome/session.test.ts › reopenLastProject › reopens the last project when the launch named none
    #[test]
    fn reopens_the_last_project_when_the_launch_named_none() {
        let enabled = reopen_enabled(Some(true), true);
        assert_eq!(startup_project(&last_session(None), false, true, enabled), Some("/p/last"));
        // A rebuilt window gets its own project back; another window has none to go back to.
        assert_eq!(startup_project(&last_session(Some("/p/window")), false, false, enabled), Some("/p/window"));
        assert_eq!(startup_project(&last_session(None), false, false, enabled), None);
        assert_eq!(startup_project(&last_session(None), false, true, false), None);
    }

    // src/features/welcome/session.test.ts › reopenLastProject ›
    // restores a session saved as a WSL UNC folder through the distro
    #[test]
    fn restores_a_session_saved_as_a_wsl_unc_folder_through_the_distro() {
        let memory = SessionMemory { last_project: Some(UNC.to_owned()), window_project: None };
        let location = startup_project(&memory, false, true, true).expect("the last project reopens");
        assert_eq!(
            open_route(location, wsl),
            OpenRoute::Remote {
                target: RemoteTarget::Wsl { distro: "Ubuntu".to_owned() },
                path: "/home/me/app".to_owned()
            }
        );
    }

    // src/features/welcome/session.test.ts › reopenLastProject ›
    // clears the remembered project while reopening and lets the user cancel
    #[test]
    fn clears_the_remembered_project_while_reopening_and_lets_the_user_cancel() {
        let (mut memory, mut reopen) = (last_session(None), Reopen::default());
        let ticket = reopen.begin("/p/last", &mut memory);
        assert_eq!(memory.last_project, None);
        assert_eq!(reopen.reopening(), Some("/p/last"));

        reopen.cancel(&mut memory);
        assert_eq!(reopen.reopening(), None);
        // The open finishes all the same: the project is closed again and stays forgotten.
        assert_eq!(reopen.opened(ticket, "/p/last", Some("/p/last"), &mut memory), Reopened::Close);
        assert_eq!(memory.last_project, None);
    }

    // src/features/welcome/session.test.ts › reopenLastProject › keeps a project the user opened after cancelling
    #[test]
    fn keeps_a_project_the_user_opened_after_cancelling() {
        let (mut memory, mut reopen) = (last_session(None), Reopen::default());
        let ticket = reopen.begin("/p/last", &mut memory);
        reopen.cancel(&mut memory);
        assert_eq!(reopen.opened(ticket, "/p/last", Some("/p/other"), &mut memory), Reopened::Superseded);
    }

    #[test]
    fn a_reopen_that_is_not_cancelled_is_kept_and_a_failed_one_is_reported() {
        let (mut memory, mut reopen) = (last_session(None), Reopen::default());
        let ticket = reopen.begin("/p/last", &mut memory);
        assert_eq!(reopen.opened(ticket, "/p/last", Some("/p/last"), &mut memory), Reopened::Keep);
        assert_eq!((reopen.reopening(), memory.clone()), (None, last_session(Some("/p/last"))));

        let ticket = reopen.begin("/p/last", &mut memory);
        assert!(reopen.failed(ticket, false, &mut memory));
        assert_eq!((reopen.reopening(), memory.clone()), (None, SessionMemory::default()));
        // Nothing is said about a reopen the user gave up, or one that was cancelled as it opened.
        let ticket = reopen.begin("/p/last", &mut memory);
        reopen.cancel(&mut memory);
        assert!(!reopen.failed(ticket, false, &mut memory));
        let ticket = reopen.begin("/p/last", &mut memory);
        assert!(!reopen.failed(ticket, true, &mut memory));
    }

    #[test]
    fn a_window_that_shows_nothing_while_it_reopens_forgets_nothing() {
        let mut memory = last_session(Some("/p/last"));
        memory.project_changed(Some("/p/last"), None, true);
        assert_eq!(memory, last_session(Some("/p/last")));
        memory.project_changed(None, Some("/p/next"), true);
        assert_eq!(
            memory,
            SessionMemory { last_project: Some("/p/next".to_owned()), window_project: Some("/p/next".to_owned()) }
        );
    }

    // src/features/welcome/welcome.test.tsx › Welcome › Projects ›
    // names the platform's file manager for the reveal action
    #[test]
    fn names_the_platforms_file_manager_for_the_reveal_action() {
        let reveal_label = |os: Os| {
            let acme = project("acme-api", "/Users/me/repo/acme-api", None);
            let items = project_menu(&acme, &[], os);
            let reveal = items.iter().find(|item| matches!(item.command(), Some(WelcomeCommand::Reveal(_))));
            reveal.and_then(MenuItem::label).map(str::to_owned)
        };
        assert_eq!(reveal_label(Os::Windows).as_deref(), Some("Show in Explorer"));
        assert_eq!(reveal_label(Os::Linux).as_deref(), Some("Show in Files"));
        assert_eq!(reveal_label(Os::Macos).as_deref(), Some("Show in Finder"));
    }

    // src/features/welcome/welcome.test.tsx › helpers › maps list keys
    #[test]
    fn maps_list_keys() {
        let plain = KeyMods::default();
        assert_eq!(handle_list_key("down", plain, 2, 0, true), Some(ListKey::Move(1)));
        assert_eq!(handle_list_key("delete", plain, 2, 1, false), None);
        assert_eq!(handle_list_key("delete", plain, 2, 1, true), Some(ListKey::Remove(1)));
        assert_eq!(handle_list_key("enter", plain, 0, 0, true), None);
    }

    #[test]
    fn enter_opens_the_platform_backspace_removes_and_shift_f10_shows_the_menu() {
        let (plain, platform, shift) =
            (KeyMods::default(), KeyMods { platform: true, shift: false }, KeyMods { platform: false, shift: true });
        assert_eq!(handle_list_key("enter", plain, 3, 2, false), Some(ListKey::Open(2)));
        assert_eq!(handle_list_key("backspace", platform, 3, 1, false), Some(ListKey::Remove(1)));
        assert_eq!(handle_list_key("backspace", plain, 3, 1, true), None);
        assert_eq!(handle_list_key("f10", shift, 3, 1, true), Some(ListKey::Menu(1)));
        assert_eq!(handle_list_key("f10", plain, 3, 1, true), None);
        // Nothing is selected in an empty list: only typing is left.
        for key in ["delete", "backspace", "f10", "down", "home"] {
            assert_eq!(handle_list_key(key, KeyMods { platform: true, shift: true }, 0, 0, true), None);
        }
        assert_eq!(handle_list_key("a", plain, 3, 1, true), None);
    }

    #[test]
    fn the_selection_moves_by_row_and_by_page_and_stops_at_both_ends() {
        assert_eq!(list_navigate("down", 3, 2, PAGE_SIZE), Some(2));
        assert_eq!(list_navigate("up", 3, 0, PAGE_SIZE), Some(0));
        assert_eq!(list_navigate("up", 3, 2, PAGE_SIZE), Some(1));
        assert_eq!(list_navigate("home", 20, 7, PAGE_SIZE), Some(0));
        assert_eq!(list_navigate("end", 20, 7, PAGE_SIZE), Some(19));
        assert_eq!(list_navigate("pagedown", 20, 7, PAGE_SIZE), Some(15));
        assert_eq!(list_navigate("pagedown", 20, 15, PAGE_SIZE), Some(19));
        assert_eq!(list_navigate("pageup", 20, 15, PAGE_SIZE), Some(7));
        assert_eq!(list_navigate("pageup", 20, 3, PAGE_SIZE), Some(0));
        assert_eq!(list_navigate("down", 0, 0, PAGE_SIZE), None);
        assert_eq!(list_navigate("left", 3, 0, PAGE_SIZE), None);
    }

    #[test]
    fn the_arrows_fold_a_selected_group_while_the_search_field_is_empty() {
        let groups = create_group(&[], "Work", Some(ACME)).expect("the name is free");
        let rows = recent_rows(&projects(), &groups, "", HOME);
        assert_eq!(fold_key("left", rows.first(), true), Some(false));
        assert_eq!(fold_key("right", rows.first(), true), Some(true));
        assert_eq!(fold_key("down", rows.first(), true), None);
        // A project row and a search text leave the arrows to the caret.
        assert_eq!(fold_key("left", rows.get(1), true), None);
        assert_eq!(fold_key("left", rows.first(), false), None);
        assert_eq!(fold_key("left", None, true), None);
    }

    #[test]
    fn a_projects_menu_opens_shows_copies_groups_and_removes_it() {
        let acme = project("acme-api", ACME, Some("main"));
        let labels = |items: &[MenuItem<WelcomeCommand>]| -> Vec<String> {
            items.iter().map(|item| item.label().unwrap_or("-").to_owned()).collect()
        };
        let items = project_menu(&acme, &[], Os::Macos);
        assert_eq!(
            labels(&items),
            [
                "Open",
                "Open in New Window",
                "Show in Finder",
                "Copy Path",
                "-",
                "Move to Group",
                "-",
                "Remove from Recent Projects…"
            ]
        );
        assert_eq!(items[0].command(), Some(&WelcomeCommand::Open(ACME.to_owned())));
        // Opening in a new window is the window action's, with the folder as its argument.
        assert_eq!(items[1].action_id(), Some("file.openInNewWindow"));
        assert_eq!(items[1].arg().and_then(serde_json::Value::as_str), Some(ACME));
        assert_eq!(items[3].command(), Some(&WelcomeCommand::CopyPath(ACME.to_owned())));
        assert_eq!(items[7].command(), Some(&WelcomeCommand::AskRemove(ACME.to_owned())));
        let MenuItem::Item(remove) = &items[7] else { panic!("a row of its own") };
        assert!(remove.danger);

        // With no group yet, the project can only go into a new one.
        let MenuItem::Item(move_to) = &items[5] else { panic!("a row of its own") };
        assert_eq!(labels(&move_to.submenu), ["New Group…"]);
        assert_eq!(move_to.submenu[0].command(), Some(&WelcomeCommand::NewGroup { path: Some(ACME.to_owned()) }));

        // In a group: the other groups, a new one, and out of its own.
        let groups = create_group(&[], "Work", Some(ACME)).and_then(|groups| create_group(&groups, "Oss", None));
        let items = project_menu(&acme, &groups.expect("the names are free"), Os::Macos);
        let MenuItem::Item(move_to) = &items[5] else { panic!("a row of its own") };
        assert_eq!(labels(&move_to.submenu), ["Oss", "-", "New Group…", "Remove from 'Work'"]);
        assert_eq!(
            move_to.submenu[0].command(),
            Some(&WelcomeCommand::MoveToGroup { path: ACME.to_owned(), group: Some("Oss".to_owned()) })
        );
        assert_eq!(
            move_to.submenu[3].command(),
            Some(&WelcomeCommand::MoveToGroup { path: ACME.to_owned(), group: None })
        );
    }

    #[test]
    fn a_remote_project_has_no_folder_to_show_and_no_group() {
        let remote = project("api", "ssh://build-box/srv/api", None);
        assert!(remote.is_remote());
        assert_eq!(display_path(&remote.path, HOME), "build-box:/srv/api");
        let items = project_menu(&remote, &[], Os::Linux);
        let labels: Vec<&str> = items.iter().map(|item| item.label().unwrap_or("-")).collect();
        assert_eq!(labels, ["Open", "Copy Path", "-", "Remove from Recent Projects…"]);
    }

    #[test]
    fn a_groups_menu_renames_it_adds_another_and_removes_it() {
        let group = ProjectGroup { name: "Work".to_owned(), paths: vec![ACME.to_owned()], expanded: true };
        let items = group_menu(&group);
        let labels: Vec<&str> = items.iter().map(|item| item.label().unwrap_or("-")).collect();
        assert_eq!(labels, ["Rename Group…", "New Project Group…", "-", "Remove Group"]);
        assert_eq!(items[0].command(), Some(&WelcomeCommand::RenameGroup("Work".to_owned())));
        assert_eq!(items[1].command(), Some(&WelcomeCommand::NewGroup { path: None }));
        assert_eq!(items[3].command(), Some(&WelcomeCommand::RemoveGroup("Work".to_owned())));
    }

    #[test]
    fn the_manage_menu_lists_settings_help_and_the_app() {
        let ids: Vec<&str> = manage_menu().iter().map(|item| item.action_id().unwrap_or("-")).collect();
        assert_eq!(
            ids,
            [
                "file.settings",
                "settings.keymap",
                "plugins.installFromDisk",
                "-",
                "help.keymapReference",
                "help.showLog",
                "app.checkForUpdates",
                "-",
                "app.about"
            ]
        );
    }

    #[test]
    fn open_is_always_offered_and_the_other_buttons_when_their_action_is_registered() {
        let ids = |registered: &[&str]| -> Vec<&'static str> {
            welcome_actions(|id| registered.contains(&id)).into_iter().map(|action| action.id).collect()
        };
        assert_eq!(ids(&[]), ["file.openProject"]);
        assert_eq!(ids(&["remote.connect"]), ["file.openProject", "remote.connect"]);
        assert_eq!(ids(&["vcs.clone", "remote.connect"]), ["file.openProject", "vcs.clone", "remote.connect"]);
        assert_eq!(landing_text(false), "Open a folder from disk.");
        assert_eq!(landing_text(true), "Open a folder from disk or clone a repository from version control.");
    }

    #[test]
    fn removing_a_project_takes_it_out_of_the_list_and_of_its_group() {
        let groups = create_group(&[], "Work", Some(ACME)).expect("the name is free");
        let (left, groups) = remove_recent(&projects(), &groups, ACME);
        let names: Vec<&str> = left.iter().map(|project| project.name.as_str()).collect();
        assert_eq!(names, ["openidea-ide", "tokio", "notes"]);
        assert_eq!(groups, [ProjectGroup { name: "Work".to_owned(), paths: Vec::new(), expanded: true }]);
    }

    #[test]
    fn groups_keep_their_paths_to_and_from_the_stored_form() {
        let group = ProjectGroup { name: "Work".to_owned(), paths: vec![ACME.to_owned()], expanded: false };
        let stored = RecentProjectGroup::from(&group);
        assert_eq!(stored.paths.iter().map(|path| path.0.clone()).collect::<Vec<_>>(), [PathBuf::from(ACME)]);
        assert_eq!(ProjectGroup::from(&stored), group);
    }
}
