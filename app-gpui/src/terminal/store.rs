//! Terminal tabs and the tab groups the tool window splits into (IDEA's Split Right / Split Down),
//! each group with its own tab strip.
//!
//! [`Terminals`] holds the tabs, the layout of their groups and the sessions behind them as plain
//! data. It starts no process and stores nothing: a transition queues the [`TerminalEffect`]s its
//! caller carries out (start or end a session's terminal, store a project's remembered tabs).
//! Drain them with [`Terminals::take_effects`] after every call, so that what is stored is what
//! the next project switch reads.

use super::persist::{self, StoredTab, StoredTerminals};
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::basename;
use std::collections::BTreeMap;

/// How a split lays its children out: `Horizontal` = side by side, `Vertical` = stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

impl SplitOrientation {
    /// The name the orientation is stored by.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Horizontal, Self::Vertical].into_iter().find(|orientation| orientation.as_str() == name)
    }
}

/// A tab group id, or a split of nested layouts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupLayout {
    Group(String),
    Split(GroupSplit),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSplit {
    pub id: String,
    pub orientation: SplitOrientation,
    pub children: Vec<GroupLayout>,
}

impl GroupLayout {
    fn is_group(&self, group: &str) -> bool {
        matches!(self, Self::Group(id) if id == group)
    }

    /// The groups, left to right and top to bottom.
    pub fn groups(&self) -> Vec<&str> {
        match self {
            Self::Group(group) => vec![group.as_str()],
            Self::Split(split) => split.children.iter().flat_map(Self::groups).collect(),
        }
    }

    pub fn contains(&self, group: &str) -> bool {
        self.groups().contains(&group)
    }

    /// Inserts group `key` after `target` (or `before` it), nesting a new split (`id`) when the
    /// orientation differs from the parent's.
    pub fn split(&self, target: &str, key: &str, orientation: SplitOrientation, id: &str, before: bool) -> Self {
        let added = || Self::Group(key.to_owned());
        match self {
            Self::Group(group) if group == target => {
                let children = if before { vec![added(), self.clone()] } else { vec![self.clone(), added()] };
                Self::Split(GroupSplit { id: id.to_owned(), orientation, children })
            }
            Self::Group(_) => self.clone(),
            Self::Split(split) => {
                let at = split.children.iter().position(|child| child.is_group(target));
                let children = match at {
                    Some(index) if split.orientation == orientation => {
                        let mut children = split.children.clone();
                        children.insert(if before { index } else { index + 1 }, added());
                        children
                    }
                    _ => split.children.iter().map(|c| c.split(target, key, orientation, id, before)).collect(),
                };
                Self::Split(GroupSplit { id: split.id.clone(), orientation: split.orientation, children })
            }
        }
    }

    /// The layout without group `key`: a split left with one child collapses into it, and
    /// children of the same orientation merge up. `None` when nothing is left.
    pub fn without(&self, key: &str) -> Option<Self> {
        let split = match self {
            Self::Group(group) => return (group != key).then(|| self.clone()),
            Self::Split(split) => split,
        };
        let mut children = Vec::new();
        for child in &split.children {
            match child.without(key) {
                None => {}
                Some(Self::Split(inner)) if inner.orientation == split.orientation => children.extend(inner.children),
                Some(kept) => children.push(kept),
            }
        }
        if children.len() < 2 {
            return children.pop();
        }
        Some(Self::Split(GroupSplit { id: split.id.clone(), orientation: split.orientation, children }))
    }

    /// The group beside `group` in its split (the one before it, else after); `None` when not split.
    pub fn neighbour(&self, group: &str) -> Option<&str> {
        let Self::Split(split) = self else { return None };
        match split.children.iter().position(|child| child.is_group(group)) {
            Some(0) => split.children.get(1).and_then(|next| next.groups().first().copied()),
            Some(index) => split.children[index - 1].groups().last().copied(),
            None => split.children.iter().find_map(|child| child.neighbour(group)),
        }
    }

    /// The group in the top right corner, whose header has the tool window's buttons.
    pub fn top_right(&self) -> &str {
        match self {
            Self::Group(group) => group.as_str(),
            Self::Split(split) => {
                let corner = match split.orientation {
                    SplitOrientation::Horizontal => split.children.last(),
                    SplitOrientation::Vertical => split.children.first(),
                };
                corner.map_or("", |child| child.top_right())
            }
        }
    }
}

/// An edge of a group's terminal a dragged tab is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTab {
    pub id: String,
    /// Default name ("Local", "Local (2)", or the directory name for "Open in Terminal").
    pub name: String,
    /// Set by the user via rename; wins over `name`.
    pub custom_name: Option<String>,
    /// Predefined shell program. Absent for the default shell and for one-shot commands.
    pub shell: Option<String>,
    /// Key of the tab's session.
    pub session: String,
    /// The tab group showing it.
    pub group: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionStatus {
    #[default]
    Idle,
    Starting,
    Running,
    Exited,
    Error,
}

/// What a session's terminal reports about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub exit_code: Option<i32>,
    /// Shell name reported by the backend (`zsh`).
    pub title: String,
    /// Window title set by the running program (OSC 0/2), `None` when unset or blank.
    pub app_title: Option<String>,
    /// A command is running (shell integration's OSC 133 C … D), or the session runs a non-shell program.
    pub command_running: bool,
    /// Current directory (initial cwd, updated by OSC 7).
    pub cwd: String,
}

/// The program a session runs instead of the configured shell (e.g. `docker exec -it <id> sh`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionCommand {
    pub program: String,
    pub args: Vec<String>,
    /// It is a shell (a predefined session), so shell integration still applies.
    pub shell: bool,
    pub env: BTreeMap<String, String>,
    /// The plugin request the session runs for; its owner is told when the session ends.
    pub plugin_operation: Option<String>,
}

impl SessionCommand {
    /// A one-shot command.
    pub fn new(program: &str, args: &[&str]) -> Self {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        Self { program: program.to_owned(), args, ..Self::default() }
    }

    /// A predefined shell session.
    pub fn predefined_shell(program: &str) -> Self {
        Self { program: program.to_owned(), shell: true, ..Self::default() }
    }
}

/// A session: what its terminal is started from, and the state that terminal reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSession {
    pub key: String,
    pub command: Option<SessionCommand>,
    /// Restored tabs and their splits only: the project root, where the session starts when its
    /// directory no longer exists (deleted since the last run).
    pub fallback_root: Option<String>,
    /// `state.cwd` is the directory the terminal starts in until the shell reports another.
    pub state: SessionSnapshot,
}

/// What the caller of a transition has to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEffect {
    /// A session was added: [`Terminals::session`] has what its terminal starts from.
    SessionCreated(String),
    /// End the session's terminal. `plugin_operation`: the plugin request it ran for, whose owner
    /// is told the session was closed.
    SessionDisposed { key: String, plugin_operation: Option<String> },
    /// Store `value` under `key`: the bound project's remembered tabs.
    Store { key: String, value: String },
}

/// A terminal session a plugin asks for (authorized by the host), shown as a tab like the user's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSessionRequest {
    /// Identifies the request: asking again while its session runs selects the tab instead.
    pub operation: String,
    pub title: Option<String>,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSession {
    /// The request's session is running: its tab was selected.
    Selected(String),
    /// A tab was opened. The caller starts its session, and reports a session that did not
    /// start with [`Terminals::plugin_session_failed`].
    Opened(TerminalTab),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSessionError {
    /// The request names no program or no directory.
    InvalidRequest,
    DidNotStart,
}

impl std::fmt::Display for PluginSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRequest => "Invalid terminal request",
            Self::DidNotStart => "Terminal did not start",
        })
    }
}

impl std::error::Error for PluginSessionError {}

/// Next free default name: "Local", "Local (2)", …
pub fn next_tab_name<S: AsRef<str>>(existing: &[S], base: &str) -> String {
    let taken = |name: &str| existing.iter().any(|known| known.as_ref() == name);
    if !taken(base) {
        return base.to_owned();
    }
    let mut number = 2u32;
    loop {
        let name = format!("{base} ({number})");
        if !taken(&name) {
            return name;
        }
        number += 1;
    }
}

/// Shortens `text` to `max` characters by eliding its middle.
pub fn trim_middle(text: &str, max: usize) -> String {
    let len = text.chars().count();
    if len <= max {
        return text.to_owned();
    }
    let kept = max.saturating_sub(1);
    let head = kept.div_ceil(2);
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(len - (kept - head)).collect();
    format!("{start}…{end}")
}

const APP_TITLE_MAX: usize = 30;

/// The title the program in the tab set, while a command runs (at the prompt it is the shell's own).
fn app_title(session: Option<&SessionSnapshot>) -> Option<&str> {
    session.filter(|state| state.command_running).and_then(|state| state.app_title.as_deref())
}

/// The tab's label: a name the user gave it, else the title the running command set (OSC 0/2,
/// middle-trimmed), else the default name. `session`: the state of the tab's session.
pub fn tab_title(tab: &TerminalTab, session: Option<&SessionSnapshot>) -> String {
    if let Some(custom) = tab.custom_name.as_deref().filter(|name| !name.is_empty()) {
        return custom.to_owned();
    }
    match app_title(session).filter(|title| !title.is_empty()) {
        Some(title) => trim_middle(title, APP_TITLE_MAX),
        None => tab.name.clone(),
    }
}

/// Untrimmed title for the tab's tooltip.
pub fn tab_full_title(tab: &TerminalTab, session: Option<&SessionSnapshot>) -> String {
    tab.custom_name.as_deref().or_else(|| app_title(session)).unwrap_or(tab.name.as_str()).to_owned()
}

/// Settings › Terminal › Start directory (`setting`), resolved against the project root.
pub fn start_directory(setting: &str, root: &str) -> String {
    let dir = setting.trim_end_matches('/');
    if dir.is_empty() {
        return root.to_owned();
    }
    let drive = matches!(dir.as_bytes(), [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic());
    if dir.starts_with('/') || drive || dir.starts_with("~/") {
        return dir.to_owned();
    }
    format!("{}/{}", root.trim_end_matches('/'), dir.strip_prefix("./").unwrap_or(dir))
}

/// The tabs of `group`, in strip order.
pub fn group_tabs<'a>(tabs: &'a [TerminalTab], group: &str) -> Vec<&'a TerminalTab> {
    tabs.iter().filter(|tab| tab.group == group).collect()
}

/// `tabs` with tab `id` moved to insertion `index`, an index into `tabs` before the move.
pub fn reorder_tabs(tabs: &[TerminalTab], id: &str, index: usize) -> Vec<TerminalTab> {
    let Some(from) = tabs.iter().position(|tab| tab.id == id) else { return tabs.to_vec() };
    let mut rest: Vec<TerminalTab> = tabs.iter().filter(|tab| tab.id != id).cloned().collect();
    let at = if index > from { index - 1 } else { index };
    rest.insert(at.min(rest.len()), tabs[from].clone());
    rest
}

/// Where a tab dropped on the middle of a group (its terminal, or the header's empty part)
/// lands: at the end, like an editor tab dropped on a pane; `None` when it is already in that
/// group, so a sloppy release doesn't reorder it.
pub fn group_drop_index(tabs: &[TerminalTab], id: &str, group: &str) -> Option<usize> {
    let tab = tabs.iter().find(|tab| tab.id == id)?;
    (tab.group != group).then(|| group_tabs(tabs, group).len())
}

/// The question asked before ending sessions that are running a command, like IDEA
/// (Terminate / Cancel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminateConfirmation {
    pub title: String,
    pub message: String,
    /// Label of the button that goes ahead.
    pub ok: String,
}

/// The confirmation for closing the tab (or tabs) called `name`. Ask it only when one of their
/// sessions is busy; idle sessions end without a question.
pub fn terminate_confirmation(name: &str, i18n: &I18n) -> TerminateConfirmation {
    let named = [("name", Param::from(name))];
    TerminateConfirmation {
        title: i18n.t("terminal.close.title", &named),
        message: i18n.t("terminal.close.message", &named),
        ok: i18n.t("terminal.close.terminate", &[]),
    }
}

/// The terminal tool window's tabs, tab groups and sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminals {
    /// Every tab, in tab strip order within its group.
    tabs: Vec<TerminalTab>,
    layout: GroupLayout,
    /// The selected tab of each group.
    selected: BTreeMap<String, String>,
    /// The selected tab of the group used last; the one New Tab, Split and Find act on.
    active_tab: Option<String>,
    sessions: BTreeMap<String, TerminalSession>,
    /// Sessions and tabs made so far: a session `s<n>` and its tab `t<n>` share the number.
    seq: u64,
    group_seq: u64,
    split_seq: u64,
    /// Project whose tabs are held. `None` before the first bind and after unbind.
    bound_project: Option<String>,
    /// What the bound project's storage is known to describe, so only changes are stored.
    remembered: Option<String>,
    /// Plugin request → its tab.
    plugin_sessions: BTreeMap<String, String>,
    effects: Vec<TerminalEffect>,
}

impl Default for Terminals {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            layout: GroupLayout::Group("g1".to_owned()),
            selected: BTreeMap::new(),
            active_tab: None,
            sessions: BTreeMap::new(),
            seq: 0,
            group_seq: 1,
            split_seq: 0,
            bound_project: None,
            remembered: None,
            plugin_sessions: BTreeMap::new(),
            effects: Vec::new(),
        }
    }
}

impl Terminals {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tabs(&self) -> &[TerminalTab] {
        &self.tabs
    }

    pub fn tab(&self, id: &str) -> Option<&TerminalTab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn layout(&self) -> &GroupLayout {
        &self.layout
    }

    pub fn selected(&self) -> &BTreeMap<String, String> {
        &self.selected
    }

    pub fn active_tab(&self) -> Option<&str> {
        self.active_tab.as_deref()
    }

    pub fn session(&self, key: &str) -> Option<&TerminalSession> {
        self.sessions.get(key)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &TerminalSession> {
        self.sessions.values()
    }

    pub fn tab_of_session(&self, key: &str) -> Option<&TerminalTab> {
        self.tabs.iter().find(|tab| tab.session == key)
    }

    /// The session of the active tab.
    pub fn active_session(&self) -> Option<&TerminalSession> {
        self.sessions.get(&self.tab(self.active_tab.as_deref()?)?.session)
    }

    /// [`tab_title`] of tab `id`, as its session stands.
    pub fn title(&self, id: &str) -> Option<String> {
        let tab = self.tab(id)?;
        Some(tab_title(tab, self.sessions.get(&tab.session).map(|session| &session.state)))
    }

    pub fn bound_project(&self) -> Option<&str> {
        self.bound_project.as_deref()
    }

    /// What the transitions since the last call ask their caller to do, in order.
    pub fn take_effects(&mut self) -> Vec<TerminalEffect> {
        std::mem::take(&mut self.effects)
    }

    fn new_group_id(&mut self) -> String {
        self.group_seq += 1;
        format!("g{}", self.group_seq)
    }

    fn new_split_id(&mut self) -> String {
        self.split_seq += 1;
        format!("split{}", self.split_seq)
    }

    fn create_session(&mut self, cwd: &str, command: Option<SessionCommand>, fallback_root: Option<String>) -> String {
        self.seq += 1;
        let key = format!("s{}", self.seq);
        let state = SessionSnapshot { cwd: cwd.to_owned(), ..SessionSnapshot::default() };
        self.sessions.insert(key.clone(), TerminalSession { key: key.clone(), command, fallback_root, state });
        self.effects.push(TerminalEffect::SessionCreated(key.clone()));
        key
    }

    fn dispose_session(&mut self, key: &str) {
        let Some(session) = self.sessions.remove(key) else { return };
        let plugin_operation = session.command.and_then(|command| command.plugin_operation);
        if let Some(operation) = &plugin_operation {
            self.plugin_sessions.remove(operation);
        }
        self.effects.push(TerminalEffect::SessionDisposed { key: key.to_owned(), plugin_operation });
    }

    fn default_tab_name(&self, base: &str) -> String {
        let names: Vec<&str> = self.tabs.iter().map(|tab| tab.name.as_str()).collect();
        next_tab_name(&names, base)
    }

    /// Opens a tab in the active tab's group, with a session in `cwd`. `name`: the base of its
    /// default name, Settings › Terminal › Tab name unless the caller names the tab.
    pub fn new_tab(&mut self, cwd: &str, name: &str, command: Option<SessionCommand>) -> TerminalTab {
        let shell = command.as_ref().filter(|command| command.shell).map(|command| command.program.clone());
        let name = self.default_tab_name(name);
        let session = self.create_session(cwd, command, None);
        let active_group = self.active_tab.as_deref().and_then(|id| self.tab(id)).map(|tab| tab.group.clone());
        let first_group = || self.layout.groups().first().copied().unwrap_or_default().to_owned();
        let group = active_group.unwrap_or_else(first_group);
        let tab = TerminalTab { id: format!("t{}", self.seq), name, custom_name: None, shell, session, group };
        self.tabs.push(tab.clone());
        self.selected.insert(tab.group.clone(), tab.id.clone());
        self.active_tab = Some(tab.id.clone());
        self.remember();
        tab
    }

    /// "Open in Terminal": a tab in `dir`, named after it.
    pub fn new_tab_at(&mut self, dir: &str) -> TerminalTab {
        let name = basename(dir);
        let name: &str = &name;
        self.new_tab(dir, if name.is_empty() { "Local" } else { name }, None)
    }

    /// Takes tab `id` out. Its group selects the neighbouring tab or, left empty, leaves the
    /// split (the last group stays); an active tab passes to the group's new selection, else to
    /// the neighbouring group's.
    fn detach_tab(&mut self, id: &str) -> Option<TerminalTab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let group = self.tabs[index].group.clone();
        let at = group_tabs(&self.tabs, &group).iter().position(|tab| tab.id == id).unwrap_or(0);
        let tab = self.tabs.remove(index);
        let rest: Vec<String> = group_tabs(&self.tabs, &group).iter().map(|tab| tab.id.clone()).collect();
        let heir = if rest.is_empty() {
            self.selected.remove(&group);
            let neighbour = self.layout.neighbour(&group).map(str::to_owned);
            if matches!(self.layout, GroupLayout::Split(_))
                && let Some(layout) = self.layout.without(&group)
            {
                self.layout = layout;
            }
            neighbour
        } else {
            if self.selected.get(&group).is_some_and(|selected| selected == id) {
                self.selected.insert(group.clone(), rest[at.min(rest.len() - 1)].clone());
            }
            Some(group)
        };
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = heir.and_then(|group| self.selected.get(&group).cloned());
        }
        Some(tab)
    }

    /// Closes tab `id` and ends its session, without asking.
    pub fn close_tab(&mut self, id: &str) {
        let Some(session) = self.tab(id).map(|tab| tab.session.clone()) else { return };
        self.dispose_session(&session);
        self.detach_tab(id);
        self.remember();
    }

    /// Ends every session and leaves one empty group. Stores nothing: swapping projects must not
    /// write the empty layout over either of them.
    fn clear(&mut self) {
        let keys: Vec<String> = self.sessions.keys().cloned().collect();
        for key in keys {
            self.dispose_session(&key);
        }
        self.tabs.clear();
        self.layout = GroupLayout::Group(self.new_group_id());
        self.selected.clear();
        self.active_tab = None;
    }

    pub fn close_all(&mut self) {
        self.clear();
        self.remember();
    }

    /// Selects tab `id` in its group and makes it the active tab.
    pub fn select_tab(&mut self, id: &str) {
        let Some(group) = self.tab(id).map(|tab| tab.group.clone()) else { return };
        self.selected.insert(group, id.to_owned());
        self.active_tab = Some(id.to_owned());
        self.remember();
    }

    /// Makes `group`'s selected tab the active one, for actions run from the group's header.
    pub fn activate_group(&mut self, group: &str) {
        if let Some(id) = self.selected.get(group).cloned() {
            self.select_tab(&id);
        }
    }

    /// Renames tab `id`; a blank name gives it its default name back.
    pub fn rename_tab(&mut self, id: &str, name: &str) {
        let trimmed = name.trim();
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.custom_name = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        }
        self.remember();
    }

    /// Split Right / Split Down, as IDEA: a new tab group beside tab `id`'s, with a new session
    /// in its directory (named from `tab_name`, Settings › Terminal › Tab name), or with
    /// `move_tab` (Split and Move) the tab itself, which needs another tab to stay behind.
    /// Returns the new group's tab.
    pub fn split_tab(
        &mut self,
        id: &str,
        orientation: SplitOrientation,
        move_tab: bool,
        tab_name: &str,
    ) -> Option<String> {
        let tab = self.tab(id)?.clone();
        if move_tab && group_tabs(&self.tabs, &tab.group).len() < 2 {
            return None;
        }
        let group = self.new_group_id();
        let split = self.new_split_id();
        let layout = self.layout.split(&tab.group, &group, orientation, &split, false);
        let placed = if move_tab {
            let mut moved = self.detach_tab(id)?;
            moved.group = group.clone();
            moved
        } else {
            let cwd = self.sessions.get(&tab.session).map_or("/", |session| session.state.cwd.as_str()).to_owned();
            let name = self.default_tab_name(tab_name);
            let session = self.create_session(&cwd, None, self.bound_project.clone());
            let id = format!("t{}", self.seq);
            TerminalTab { id, name, custom_name: None, shell: None, session, group: group.clone() }
        };
        let placed_id = placed.id.clone();
        self.tabs.push(placed);
        self.layout = layout;
        self.selected.insert(group, placed_id.clone());
        self.active_tab = Some(placed_id.clone());
        self.remember();
        Some(placed_id)
    }

    /// Unsplit: merges `group`'s tabs into the group beside it, keeping its selected tab.
    pub fn unsplit(&mut self, group: &str) {
        let Some(into) = self.layout.neighbour(group).map(str::to_owned) else { return };
        let keep = self.selected.get(group).or_else(|| self.selected.get(&into)).cloned();
        // In layout order, so the merged strip keeps the tabs left to right / top to bottom.
        let mut tabs: Vec<TerminalTab> = Vec::with_capacity(self.tabs.len());
        for member in self.layout.groups() {
            tabs.extend(group_tabs(&self.tabs, member).into_iter().cloned());
        }
        for tab in tabs.iter_mut().filter(|tab| tab.group == group) {
            tab.group.clone_from(&into);
        }
        self.tabs = tabs;
        if let Some(layout) = self.layout.without(group) {
            self.layout = layout;
        }
        self.selected.remove(group);
        if let Some(keep) = &keep {
            self.selected.insert(into, keep.clone());
        }
        self.active_tab = keep;
        self.remember();
    }

    /// Drag & drop: tab `id` dropped on an edge of `group`'s terminal becomes a new group on that
    /// side, like an editor tab dropped on the edge of a pane. A group's only tab dropped on its
    /// own edge stays: there is nothing to split off from. Returns whether the tab moved.
    pub fn move_tab_to_split(&mut self, id: &str, group: &str, edge: SplitEdge) -> bool {
        let Some(from) = self.tab(id).map(|tab| tab.group.clone()) else { return false };
        if !self.layout.contains(group) || (from == group && group_tabs(&self.tabs, group).len() < 2) {
            return false;
        }
        let Some(mut tab) = self.detach_tab(id) else { return false };
        let target = self.new_group_id();
        let split = self.new_split_id();
        let (orientation, before) = match edge {
            SplitEdge::Left => (SplitOrientation::Horizontal, true),
            SplitEdge::Right => (SplitOrientation::Horizontal, false),
            SplitEdge::Top => (SplitOrientation::Vertical, true),
            SplitEdge::Bottom => (SplitOrientation::Vertical, false),
        };
        tab.group.clone_from(&target);
        self.tabs.push(tab);
        self.layout = self.layout.split(group, &target, orientation, &split, before);
        self.selected.insert(target, id.to_owned());
        self.active_tab = Some(id.to_owned());
        self.remember();
        true
    }

    /// Drag & drop: moves tab `id` to insertion `index` of `group`'s strip. Within its own group
    /// that reorders it; into another it also becomes the selected tab.
    pub fn move_tab(&mut self, id: &str, group: &str, index: usize) {
        let Some(from) = self.tab(id).map(|tab| tab.group.clone()) else { return };
        if !self.layout.contains(group) {
            return;
        }
        let strip = if from == group {
            let strip: Vec<TerminalTab> = group_tabs(&self.tabs, group).into_iter().cloned().collect();
            reorder_tabs(&strip, id, index)
        } else {
            let Some(mut tab) = self.detach_tab(id) else { return };
            group.clone_into(&mut tab.group);
            let mut strip: Vec<TerminalTab> = group_tabs(&self.tabs, group).into_iter().cloned().collect();
            strip.insert(index.min(strip.len()), tab);
            self.selected.insert(group.to_owned(), id.to_owned());
            self.active_tab = Some(id.to_owned());
            strip
        };
        self.tabs.retain(|tab| tab.group != group);
        self.tabs.extend(strip);
        self.remember();
    }

    /// The terminal of session `key` reports a new state.
    pub fn set_session_state(&mut self, key: &str, state: SessionSnapshot) {
        if let Some(session) = self.sessions.get_mut(key) {
            session.state = state;
        }
        self.remember();
    }

    /// The terminal of session `key` took the focus: its tab becomes the active one.
    pub fn session_focused(&mut self, key: &str) {
        if let Some(id) = self.tab_of_session(key).map(|tab| tab.id.clone()) {
            self.select_tab(&id);
        }
    }

    /// The shell of session `key` exited. `close_on_exit`: Settings › Terminal › Close session
    /// when it ends.
    pub fn session_exited(&mut self, key: &str, close_on_exit: bool) {
        if close_on_exit && let Some(id) = self.tab_of_session(key).map(|tab| tab.id.clone()) {
            self.close_tab(&id);
        }
    }

    /// Opens the session a plugin asks for, or selects its tab while that session still runs.
    /// `tab_name`: Settings › Terminal › Tab name, for a request without a title.
    pub fn plugin_session(
        &mut self,
        request: PluginSessionRequest,
        tab_name: &str,
    ) -> Result<PluginSession, PluginSessionError> {
        let running = self.plugin_sessions.get(&request.operation).and_then(|id| self.tab(id)).filter(|tab| {
            self.sessions.get(&tab.session).is_some_and(|session| session.state.status == SessionStatus::Running)
        });
        if let Some(id) = running.map(|tab| tab.id.clone()) {
            self.select_tab(&id);
            return Ok(PluginSession::Selected(id));
        }
        let (Some(cwd), Some(program)) = (request.cwd.filter(|cwd| !cwd.is_empty()), request.program) else {
            return Err(PluginSessionError::InvalidRequest);
        };
        if program.is_empty() {
            return Err(PluginSessionError::InvalidRequest);
        }
        let command = SessionCommand {
            program,
            args: request.args,
            shell: false,
            env: request.env,
            plugin_operation: Some(request.operation.clone()),
        };
        let tab = self.new_tab(&cwd, request.title.as_deref().unwrap_or(tab_name), Some(command));
        self.plugin_sessions.insert(request.operation, tab.id.clone());
        Ok(PluginSession::Opened(tab))
    }

    /// The session opened for plugin request `operation` did not start: its tab is closed.
    pub fn plugin_session_failed(&mut self, operation: &str) -> PluginSessionError {
        if let Some(id) = self.plugin_sessions.remove(operation) {
            self.close_tab(&id);
        }
        PluginSessionError::DidNotStart
    }

    fn capture(&self) -> StoredTerminals {
        let tabs = self.tabs.iter().filter_map(|tab| {
            let cwd = self.sessions.get(&tab.session).map(|session| session.state.cwd.as_str())?;
            (!cwd.is_empty()).then(|| StoredTab {
                id: tab.id.clone(),
                name: tab.name.clone(),
                custom_name: tab.custom_name.clone(),
                cwd: cwd.to_owned(),
                group: tab.group.clone(),
                shell: tab.shell.clone(),
            })
        });
        StoredTerminals {
            tabs: tabs.collect(),
            layout: self.layout.clone(),
            selected: self.selected.clone(),
            active_tab: self.active_tab.clone(),
        }
    }

    /// Has the bound project's tabs stored when they changed.
    fn remember(&mut self) {
        let Some(project) = self.bound_project.as_deref() else { return };
        let value = self.capture().serialize();
        if self.remembered.as_deref() == Some(value.as_str()) {
            return;
        }
        self.effects.push(TerminalEffect::Store { key: persist::storage_key(project), value: value.clone() });
        self.remembered = Some(value);
    }

    fn mapped_group(&mut self, group_of: &mut BTreeMap<String, String>, group: &str) -> String {
        if let Some(mapped) = group_of.get(group) {
            return mapped.clone();
        }
        let mapped = self.new_group_id();
        group_of.insert(group.to_owned(), mapped.clone());
        mapped
    }

    fn remap_layout(&mut self, node: &GroupLayout, group_of: &mut BTreeMap<String, String>) -> GroupLayout {
        match node {
            GroupLayout::Group(group) => GroupLayout::Group(self.mapped_group(group_of, group)),
            GroupLayout::Split(split) => {
                let id = self.new_split_id();
                let children = split.children.iter().map(|child| self.remap_layout(child, group_of)).collect();
                GroupLayout::Split(GroupSplit { id, orientation: split.orientation, children })
            }
        }
    }

    /// Opens the saved tabs in fresh sessions. Group ids are new, so they don't collide with ones
    /// made later.
    fn restore(&mut self, saved: &StoredTerminals, root: &str) {
        let mut group_of = BTreeMap::new();
        let layout = self.remap_layout(&saved.layout, &mut group_of);
        let mut id_of: BTreeMap<&str, String> = BTreeMap::new();
        let mut tabs: Vec<TerminalTab> = Vec::new();
        for saved_tab in &saved.tabs {
            let command = saved_tab.shell.as_deref().map(SessionCommand::predefined_shell);
            // A saved directory may have been deleted since the last run: the project root is the fallback.
            let session = self.create_session(&saved_tab.cwd, command, Some(root.to_owned()));
            let id = format!("t{}", self.seq);
            id_of.insert(saved_tab.id.as_str(), id.clone());
            tabs.push(TerminalTab {
                id,
                name: saved_tab.name.clone(),
                custom_name: saved_tab.custom_name.clone(),
                shell: saved_tab.shell.clone(),
                session,
                group: self.mapped_group(&mut group_of, &saved_tab.group),
            });
        }
        let mut selected = BTreeMap::new();
        for group in layout.groups() {
            let saved_group = group_of.iter().find(|(_, mapped)| mapped.as_str() == group).map(|(saved, _)| saved);
            let wanted = saved_group.and_then(|saved_group| saved.selected.get(saved_group));
            let wanted = wanted.and_then(|id| id_of.get(id.as_str()));
            let in_group = |id: &String| tabs.iter().any(|tab| tab.group == group && tab.id == *id);
            let first = tabs.iter().find(|tab| tab.group == group).map(|tab| &tab.id);
            if let Some(id) = wanted.filter(|id| in_group(id)).or(first) {
                selected.insert(group.to_owned(), id.clone());
            }
        }
        let active = saved.active_tab.as_deref().and_then(|id| id_of.get(id));
        self.active_tab = active.or_else(|| tabs.first().map(|tab| &tab.id)).cloned();
        self.selected = selected;
        self.tabs = tabs;
        self.layout = layout;
    }

    /// Points the terminal at the project at `root`, remembering the project it was bound to.
    /// `stored` reads the value stored under a key ([`persist::storage_key`]). The same root
    /// again is a no-op, so tabs are not reopened.
    pub fn sync_project(&mut self, root: Option<&str>, stored: impl FnOnce(&str) -> Option<String>) {
        if root == self.bound_project.as_deref() {
            return;
        }
        self.remember();
        self.bound_project = root.map(str::to_owned);
        self.remembered = None;
        self.clear();
        let Some(root) = root else { return };
        let saved = persist::parse(stored(&persist::storage_key(root)).as_deref());
        if let Some(saved) = saved.filter(|saved| !saved.tabs.is_empty()) {
            self.restore(&saved, root);
        }
        // What was just read is what is stored: only a change from here on is written back.
        self.remembered = Some(self.capture().serialize());
    }

    /// Drops the sessions without storing the empty layout over the project that owned them.
    pub fn unbind(&mut self) {
        self.remember();
        self.bound_project = None;
        self.remembered = None;
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::SplitOrientation::{Horizontal, Vertical};
    use super::*;

    const TAB_NAME: &str = "Local";

    fn group(id: &str) -> GroupLayout {
        GroupLayout::Group(id.to_owned())
    }

    fn split(id: &str, orientation: SplitOrientation, children: Vec<GroupLayout>) -> GroupLayout {
        GroupLayout::Split(GroupSplit { id: id.to_owned(), orientation, children })
    }

    /// The orientation and children of a split, whatever its id.
    fn shape(layout: &GroupLayout) -> Option<(SplitOrientation, Vec<GroupLayout>)> {
        match layout {
            GroupLayout::Group(_) => None,
            GroupLayout::Split(split) => Some((split.orientation, split.children.clone())),
        }
    }

    fn tab(t: &Terminals, id: &str) -> TerminalTab {
        t.tab(id).cloned().unwrap()
    }

    fn strip(t: &Terminals, group: &str) -> Vec<String> {
        group_tabs(t.tabs(), group).into_iter().map(|tab| tab.id.clone()).collect()
    }

    fn cwd<'a>(t: &'a Terminals, session: &str) -> Option<&'a str> {
        t.session(session).map(|session| session.state.cwd.as_str())
    }

    fn new_tab(t: &mut Terminals, cwd: &str) -> TerminalTab {
        t.new_tab(cwd, TAB_NAME, None)
    }

    fn split_tab(t: &mut Terminals, id: &str, orientation: SplitOrientation) -> String {
        t.split_tab(id, orientation, false, TAB_NAME).unwrap()
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › splits right into a new group with a session in
    // the tab's directory
    #[test]
    fn splits_right_into_a_new_group_with_a_session_in_the_tabs_directory() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/repo/app");
        let b = split_tab(&mut t, &a.id, Horizontal);
        let g2 = tab(&t, &b).group;
        assert_ne!(g2, a.group);
        assert_eq!(shape(t.layout()), Some((Horizontal, vec![group(&a.group), group(&g2)])));
        assert_eq!(tab(&t, &b).name, "Local (2)");
        assert_eq!(cwd(&t, &tab(&t, &b).session), Some("/repo/app"));
        assert_eq!(t.selected(), &BTreeMap::from([(a.group.clone(), a.id.clone()), (g2.clone(), b.clone())]));
        assert_eq!(t.active_tab(), Some(b.as_str()));
        // New tabs open in the active group.
        assert_eq!(new_tab(&mut t, "/repo").group, g2);
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › collapses a group when its last tab closes,
    // activating the neighbour
    #[test]
    fn collapses_a_group_when_its_last_tab_closes_activating_the_neighbour() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        let b = split_tab(&mut t, &a.id, Vertical);
        t.close_tab(&b);
        assert_eq!(t.layout(), &group(&a.group));
        assert_eq!(t.active_tab(), Some(a.id.as_str()));
        assert_eq!(t.selected().keys().collect::<Vec<_>>(), [&a.group]);
        t.close_tab(&a.id);
        assert_eq!(t.layout(), &group(&a.group));
        assert_eq!(t.active_tab(), None);
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › moves the tab itself with Split and Move, which
    // needs another tab left behind
    #[test]
    fn moves_the_tab_itself_with_split_and_move_which_needs_another_tab_left_behind() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        assert_eq!(t.split_tab(&a.id, Vertical, true, TAB_NAME), None);
        let c = new_tab(&mut t, "/p").id;
        assert_eq!(t.split_tab(&c, Vertical, true, TAB_NAME).as_deref(), Some(c.as_str()));
        assert_ne!(tab(&t, &c).group, a.group);
        assert_eq!(strip(&t, &a.group), [a.id.as_str()]);
        assert_eq!(t.selected().get(&a.group), Some(&a.id));
        assert_eq!(shape(t.layout()), Some((Vertical, vec![group(&a.group), group(&tab(&t, &c).group)])));
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › unsplits into the neighbouring group, in layout
    // order, keeping the group's selection
    #[test]
    fn unsplits_into_the_neighbouring_group_in_layout_order_keeping_the_groups_selection() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        let b = split_tab(&mut t, &a.id, Horizontal);
        let g2 = tab(&t, &b).group;
        let c = new_tab(&mut t, "/p").id;
        t.unsplit(&a.group);
        assert_eq!(t.layout(), &group(&g2));
        assert_eq!(strip(&t, &g2), [a.id.as_str(), b.as_str(), c.as_str()]);
        assert_eq!(t.selected(), &BTreeMap::from([(g2.clone(), a.id.clone())]));
        assert_eq!(t.active_tab(), Some(a.id.as_str()));
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › drags tabs between groups and within one
    #[test]
    fn drags_tabs_between_groups_and_within_one() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        let b = split_tab(&mut t, &a.id, Horizontal);
        t.move_tab(&b, &a.group, 0);
        assert_eq!(t.layout(), &group(&a.group));
        assert_eq!(strip(&t, &a.group), [b.as_str(), a.id.as_str()]);
        assert_eq!(t.active_tab(), Some(b.as_str()));
        t.move_tab(&b, &a.group, 2);
        assert_eq!(strip(&t, &a.group), [a.id.as_str(), b.as_str()]);
        // A group's only tab dropped on its own strip stays put.
        let d = split_tab(&mut t, &a.id, Horizontal);
        let own = tab(&t, &d).group;
        t.move_tab(&d, &own, 0);
        assert!(shape(t.layout()).is_some());
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › drops a tab released on a group's header or
    // terminal at the end, unless it is already there
    #[test]
    fn drops_a_tab_released_on_a_group_at_the_end_unless_it_is_already_there() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        let b = split_tab(&mut t, &a.id, Horizontal);
        // Opens in the active group, b's.
        let c = new_tab(&mut t, "/p");
        let b_group = tab(&t, &b).group;
        assert_eq!(strip(&t, &b_group), [b.as_str(), c.id.as_str()]);
        assert_eq!(group_drop_index(t.tabs(), &b, &a.group), Some(1));
        assert_eq!(group_drop_index(t.tabs(), &a.id, &b_group), Some(2));
        assert_eq!(group_drop_index(t.tabs(), &c.id, &b_group), None);
        assert_eq!(group_drop_index(t.tabs(), "gone", &a.group), None);
    }

    // src/features/terminal/groups.test.ts › terminal tab groups › splits off a new group from a tab dropped on an
    // edge of a group's terminal
    #[test]
    fn splits_off_a_new_group_from_a_tab_dropped_on_an_edge_of_a_groups_terminal() {
        let mut t = Terminals::new();
        let a = new_tab(&mut t, "/p");
        let b = split_tab(&mut t, &a.id, Horizontal);
        // In b's group.
        let c = new_tab(&mut t, "/p");
        let group_a = a.group.clone();
        let group_b = tab(&t, &b).group;
        // Right edge: a new group beside a's, which becomes the active one.
        assert!(t.move_tab_to_split(&c.id, &group_a, SplitEdge::Right));
        let group_g = tab(&t, &c.id).group;
        assert_eq!(t.layout().groups(), [group_a.as_str(), group_g.as_str(), group_b.as_str()]);
        assert_eq!(t.selected().get(&group_g), Some(&c.id));
        assert_eq!(t.active_tab(), Some(c.id.as_str()));
        // Bottom edge nests a vertical split; the emptied group leaves the layout.
        assert!(t.move_tab_to_split(&b, &group_a, SplitEdge::Bottom));
        let group_h = tab(&t, &b).group;
        assert_eq!(t.layout().groups(), [group_a.as_str(), group_h.as_str(), group_g.as_str()]);
        let (orientation, children) = shape(t.layout()).unwrap();
        assert_eq!((orientation, children.len(), &children[1]), (Horizontal, 2, &group(&group_g)));
        assert_eq!(shape(&children[0]), Some((Vertical, vec![group(&group_a), group(&group_h)])));
        // Left and top edges insert before the target.
        assert!(t.move_tab_to_split(&c.id, &group_a, SplitEdge::Left));
        let group_k = tab(&t, &c.id).group;
        assert_eq!(t.layout().groups(), [group_k.as_str(), group_a.as_str(), group_h.as_str()]);
        // A group's only tab has nothing to split off from; with company it can.
        assert!(!t.move_tab_to_split(&c.id, &group_k, SplitEdge::Right));
        assert_eq!(t.layout().groups(), [group_k.as_str(), group_a.as_str(), group_h.as_str()]);
        t.move_tab(&b, &group_k, 1);
        assert!(t.move_tab_to_split(&b, &group_k, SplitEdge::Right));
        let moved = tab(&t, &b).group;
        assert_eq!(t.layout().groups(), [group_k.as_str(), moved.as_str(), group_a.as_str()]);
        assert_eq!(strip(&t, &group_k), [c.id.as_str()]);
    }

    fn nested() -> GroupLayout {
        split("s1", Horizontal, vec![group("A"), split("s2", Vertical, vec![group("B"), group("C")])])
    }

    // src/features/terminal/groups.test.ts › group layout › finds the group an unsplit merges into: the one before,
    // else after, in the same split
    #[test]
    fn finds_the_group_an_unsplit_merges_into() {
        let l = nested();
        assert_eq!(l.neighbour("A"), Some("B"));
        assert_eq!(l.neighbour("B"), Some("C"));
        assert_eq!(l.neighbour("C"), Some("B"));
        assert_eq!(group("A").neighbour("A"), None);
    }

    // src/features/terminal/groups.test.ts › group layout › finds the top right group
    #[test]
    fn finds_the_top_right_group() {
        assert_eq!(nested().top_right(), "B");
        assert_eq!(split("s", Vertical, vec![nested(), group("D")]).top_right(), "B");
        assert_eq!(group("A").top_right(), "A");
    }

    // src/features/terminal/terminal.test.ts › split layout › nests a split when the orientation changes
    #[test]
    fn nests_a_split_when_the_orientation_changes() {
        let mut l = group("a").split("a", "b", Vertical, "g1", false);
        assert_eq!(l, split("g1", Vertical, vec![group("a"), group("b")]));
        l = l.split("b", "c", Horizontal, "g2", false);
        let inner = split("g2", Horizontal, vec![group("b"), group("c")]);
        assert_eq!(l, split("g1", Vertical, vec![group("a"), inner]));
        l = l.split("a", "d", Vertical, "g3", false);
        assert_eq!(l.groups(), ["a", "d", "b", "c"]);
    }

    // src/features/terminal/terminal.test.ts › split layout › collapses single-child splits on removal
    #[test]
    fn collapses_single_child_splits_on_removal() {
        let l = group("a").split("a", "b", Vertical, "g1", false).split("b", "c", Horizontal, "g2", false);
        assert_eq!(l.without("a"), Some(split("g2", Horizontal, vec![group("b"), group("c")])));
        assert_eq!(l.without("c"), Some(split("g1", Vertical, vec![group("a"), group("b")])));
        assert_eq!(group("a").without("a"), None);
    }

    // src/features/terminal/terminal.test.ts › tab names › numbers duplicates like IDEA
    #[test]
    fn numbers_duplicates_like_idea() {
        assert_eq!(next_tab_name::<&str>(&[], "Local"), "Local");
        assert_eq!(next_tab_name(&["Local"], "Local"), "Local (2)");
        assert_eq!(next_tab_name(&["Local", "Local (2)"], "Local"), "Local (3)");
        assert_eq!(next_tab_name(&["Local"], "api"), "api");
    }

    fn strip_of(ids: &str) -> Vec<TerminalTab> {
        let tab = |id: char| TerminalTab {
            id: id.to_string(),
            name: TAB_NAME.to_owned(),
            custom_name: None,
            shell: None,
            session: format!("s-{id}"),
            group: "g1".to_owned(),
        };
        ids.chars().map(tab).collect()
    }

    fn ids(tabs: &[TerminalTab]) -> String {
        tabs.iter().map(|tab| tab.id.as_str()).collect()
    }

    // src/features/terminal/terminal.test.ts › tab order › moves a dragged tab to the insertion index
    #[test]
    fn moves_a_dragged_tab_to_the_insertion_index() {
        let tabs = strip_of("abc");
        assert_eq!(ids(&reorder_tabs(&tabs, "a", 3)), "bca");
        assert_eq!(ids(&reorder_tabs(&tabs, "a", 2)), "bac");
        assert_eq!(ids(&reorder_tabs(&tabs, "c", 0)), "cab");
        assert_eq!(ids(&reorder_tabs(&tabs, "b", 1)), "abc");
        assert_eq!(ids(&reorder_tabs(&tabs, "b", 2)), "abc");
    }

    // src/features/terminal/terminal.test.ts › tab order › ignores unknown tabs
    #[test]
    fn ignores_unknown_tabs() {
        assert_eq!(ids(&reorder_tabs(&strip_of("abc"), "x", 0)), "abc");
    }

    fn local_tab() -> TerminalTab {
        TerminalTab {
            id: "t1".to_owned(),
            name: "Local".to_owned(),
            custom_name: None,
            shell: None,
            session: "s1".to_owned(),
            group: "g1".to_owned(),
        }
    }

    fn snap(app_title: Option<&str>, command_running: bool) -> SessionSnapshot {
        SessionSnapshot {
            status: SessionStatus::Running,
            exit_code: None,
            title: "zsh".to_owned(),
            app_title: app_title.map(str::to_owned),
            command_running,
            cwd: "/p".to_owned(),
        }
    }

    // src/features/terminal/terminal.test.ts › terminal tab titles › shows the default name at the prompt, whatever
    // title the shell sets
    #[test]
    fn shows_the_default_name_at_the_prompt_whatever_title_the_shell_sets() {
        let at_prompt = snap(Some("thomas@mac: ~/repo"), false);
        assert_eq!(tab_title(&local_tab(), Some(&at_prompt)), "Local");
        assert_eq!(tab_full_title(&local_tab(), Some(&at_prompt)), "Local");
    }

    // src/features/terminal/terminal.test.ts › terminal tab titles › follows the title the running program sets,
    // else the default name
    #[test]
    fn follows_the_title_the_running_program_sets_else_the_default_name() {
        let tab = local_tab();
        assert_eq!(tab_title(&tab, Some(&snap(None, true))), "Local");
        assert_eq!(tab_title(&tab, Some(&snap(Some("vim notes.md"), true))), "vim notes.md");
        let long = "ssh deploy@build-server-01.internal.example.com";
        let running = snap(Some(long), true);
        assert_eq!(tab_title(&tab, Some(&running)).chars().count(), 30);
        assert_eq!(tab_title(&tab, Some(&running)), trim_middle(long, 30));
        assert_eq!(tab_full_title(&tab, Some(&running)), long);
    }

    // src/features/terminal/terminal.test.ts › terminal tab titles › keeps a name the user gave the tab
    #[test]
    fn keeps_a_name_the_user_gave_the_tab() {
        let renamed = TerminalTab { custom_name: Some("Server".to_owned()), ..local_tab() };
        let running = snap(Some("top"), true);
        assert_eq!(tab_title(&renamed, Some(&running)), "Server");
        assert_eq!(tab_full_title(&renamed, Some(&running)), "Server");
    }

    // src/features/terminal/terminal.test.ts › terminal tab titles › elides the middle
    #[test]
    fn elides_the_middle() {
        assert_eq!(trim_middle("abcdefghij", 5), "ab…ij");
        assert_eq!(trim_middle("abc", 5), "abc");
    }

    /// Terminals with the storage their effects are carried out on.
    #[derive(Default)]
    struct Harness {
        terminals: Terminals,
        storage: BTreeMap<String, String>,
        disposed: Vec<String>,
    }

    impl Harness {
        fn apply(&mut self) {
            for effect in self.terminals.take_effects() {
                match effect {
                    TerminalEffect::Store { key, value } => {
                        self.storage.insert(key, value);
                    }
                    TerminalEffect::SessionDisposed { key, .. } => self.disposed.push(key),
                    TerminalEffect::SessionCreated(_) => {}
                }
            }
        }

        fn sync(&mut self, root: Option<&str>) {
            let storage = &self.storage;
            self.terminals.sync_project(root, |key| storage.get(key).cloned());
            self.apply();
        }

        fn new_tab(&mut self, cwd: &str, name: &str, command: Option<SessionCommand>) -> TerminalTab {
            let tab = self.terminals.new_tab(cwd, name, command);
            self.apply();
            tab
        }

        fn split(&mut self, id: &str) -> String {
            let side = split_tab(&mut self.terminals, id, Horizontal);
            self.apply();
            side
        }

        /// The shell of the session reports that it changed directory.
        fn change_directory(&mut self, session: &str, cwd: &str) {
            let mut state = self.terminals.session(session).unwrap().state.clone();
            cwd.clone_into(&mut state.cwd);
            self.terminals.set_session_state(session, state);
            self.apply();
        }

        fn names(&self) -> Vec<&str> {
            let tabs = self.terminals.tabs().iter();
            tabs.map(|tab| tab.custom_name.as_deref().unwrap_or(tab.name.as_str())).collect()
        }

        fn session(&self, tab: usize) -> &TerminalSession {
            self.terminals.session(&self.terminals.tabs()[tab].session).unwrap()
        }
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › keeps a plugin terminal opened before the
    // terminal pane first mounts
    #[test]
    fn keeps_a_plugin_terminal_opened_before_the_terminal_pane_first_mounts() {
        let mut h = Harness::default();
        h.sync(Some("/lazy-project"));
        let request = PluginSessionRequest {
            operation: "first-plugin-terminal".to_owned(),
            title: Some("Wrapper".to_owned()),
            program: Some("/approved/wrapper".to_owned()),
            cwd: Some("/lazy-project".to_owned()),
            ..PluginSessionRequest::default()
        };
        let opened = h.terminals.plugin_session(request, TAB_NAME);
        h.apply();
        let tab = h.terminals.tabs()[0].clone();
        assert_eq!(opened, Ok(PluginSession::Opened(tab.clone())));
        assert_eq!(tab.name, "Wrapper");
        let session = h.terminals.session(&tab.session).cloned();
        h.sync(Some("/lazy-project"));
        assert_eq!(h.terminals.session(&tab.session).cloned(), session);
        assert!(h.terminals.tabs().contains(&tab));
        assert!(h.disposed.is_empty());
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › restores each project's tabs, working
    // directories, renames and splits
    #[test]
    fn restores_each_projects_tabs_working_directories_renames_and_splits() {
        let mut h = Harness::default();
        h.sync(Some("/repo"));
        let app = h.new_tab("/repo", TAB_NAME, None);
        h.terminals.rename_tab(&app.id, "Server");
        let logs = h.new_tab("/repo/logs", TAB_NAME, None);
        let side = h.split(&logs.id);
        assert_eq!(h.terminals.active_tab(), Some(side.as_str()));
        h.change_directory(&logs.session, "/repo/logs/today");

        h.sync(Some("/other"));
        assert!(h.terminals.tabs().is_empty());
        h.new_tab("/other", TAB_NAME, None);

        h.sync(Some("/repo"));
        assert_eq!(h.names(), ["Server", "Local (2)", "Local (3)"]);
        assert_eq!(h.session(0).state.cwd, "/repo");
        assert_eq!(h.session(1).state.cwd, "/repo/logs/today");
        assert_eq!(h.session(2).state.cwd, "/repo/logs");
        assert_eq!(h.terminals.layout().groups().len(), 2);
        let restored = h.terminals.tabs();
        assert_ne!(restored[2].group, restored[1].group);
        assert_eq!(h.terminals.active_tab(), Some(restored[2].id.as_str()));

        h.sync(Some("/other"));
        assert_eq!(h.names(), ["Local"]);
        assert_eq!(h.session(0).state.cwd, "/other");
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › reopens a predefined shell, and a one-shot
    // command as a plain shell in the same directory
    #[test]
    fn reopens_a_predefined_shell_and_a_one_shot_command_as_a_plain_shell_in_the_same_directory() {
        let mut h = Harness::default();
        h.sync(Some("/repo"));
        h.new_tab("/repo", "zsh", Some(SessionCommand::predefined_shell("/bin/zsh")));
        h.new_tab("/repo/app", "build", Some(SessionCommand::new("npm", &["test"])));
        h.sync(None);
        h.sync(Some("/repo"));
        assert_eq!(h.session(0).command, Some(SessionCommand::predefined_shell("/bin/zsh")));
        assert_eq!(h.session(1).command, None);
        assert_eq!(h.session(1).state.cwd, "/repo/app");
        assert_eq!(h.terminals.tabs()[1].name, "build");
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › keeps the saved tabs when the terminal runtime
    // is dropped
    #[test]
    fn keeps_the_saved_tabs_when_the_terminals_are_unbound() {
        let mut h = Harness::default();
        h.sync(Some("/repo"));
        let tab = h.new_tab("/repo", TAB_NAME, None);
        h.terminals.rename_tab(&tab.id, "Ship");
        h.terminals.unbind();
        h.apply();
        assert!(h.terminals.tabs().is_empty());
        h.sync(Some("/repo"));
        assert_eq!(h.names(), ["Ship"]);
        assert_eq!(h.session(0).state.cwd, "/repo");
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › restored tabs fall back to the project root,
    // fresh tabs do not
    #[test]
    fn restored_tabs_fall_back_to_the_project_root_fresh_tabs_do_not() {
        let mut h = Harness::default();
        h.sync(Some("/repo"));
        h.new_tab("/repo/logs", TAB_NAME, None);
        h.sync(None);
        h.sync(Some("/repo"));
        let restored = h.terminals.tabs()[0].clone();
        assert_eq!(h.session(0).fallback_root.as_deref(), Some("/repo"));

        let fresh = h.new_tab("/repo/app", TAB_NAME, None);
        assert_eq!(h.terminals.session(&fresh.session).unwrap().fallback_root, None);

        let side = h.split(&restored.id);
        let side_session = tab(&h.terminals, &side).session;
        assert_eq!(h.terminals.session(&side_session).unwrap().fallback_root.as_deref(), Some("/repo"));
    }

    // src/features/terminal/persist.test.ts › terminal tab memory › restores a tab whose saved directory is gone
    // with the project root as its fallback
    #[test]
    fn restores_a_tab_whose_saved_directory_is_gone_with_the_project_root_as_its_fallback() {
        let mut h = Harness::default();
        h.sync(Some("/repo"));
        let tab = h.new_tab("/repo", TAB_NAME, None);
        h.change_directory(&tab.session, "/repo/gone");

        h.sync(None);
        h.sync(Some("/repo"));
        assert_eq!(h.session(0).state.cwd, "/repo/gone");
        assert_eq!(h.session(0).fallback_root.as_deref(), Some("/repo"));
    }

    // Not from a spec: only changes are stored, under the bound project's key, and none while unbound.
    #[test]
    fn stores_the_bound_projects_tabs_when_they_change() {
        let mut t = Terminals::new();
        let unbound = new_tab(&mut t, "/p");
        assert_eq!(t.take_effects(), [TerminalEffect::SessionCreated(unbound.session.clone())]);
        t.sync_project(Some("/repo"), |_| None);
        let disposed = TerminalEffect::SessionDisposed { key: unbound.session, plugin_operation: None };
        assert_eq!(t.take_effects(), [disposed]);
        let tab = new_tab(&mut t, "/repo");
        let effects = t.take_effects();
        assert_eq!(effects.len(), 2);
        assert!(matches!(&effects[1], TerminalEffect::Store { key, .. } if key == "openidea.terminal.tabs.v1:/repo"));
        t.select_tab(&tab.id);
        t.session_focused(&tab.session);
        assert!(t.take_effects().is_empty());
        t.session_exited(&tab.session, false);
        assert_eq!(t.tabs().len(), 1);
        t.session_exited(&tab.session, true);
        assert!(t.tabs().is_empty());
        assert_eq!(t.take_effects().len(), 2);
    }

    // Not from a spec: a plugin request needs a program and a directory, and its tab goes when it did not start.
    #[test]
    fn opens_and_drops_plugin_sessions() {
        let mut t = Terminals::new();
        let request = PluginSessionRequest {
            operation: "op".to_owned(),
            program: Some("/bin/tool".to_owned()),
            cwd: Some("/p".to_owned()),
            ..PluginSessionRequest::default()
        };
        let incomplete = PluginSessionRequest { cwd: None, ..request.clone() };
        assert_eq!(t.plugin_session(incomplete, TAB_NAME), Err(PluginSessionError::InvalidRequest));
        let Ok(PluginSession::Opened(tab)) = t.plugin_session(request.clone(), TAB_NAME) else {
            panic!("the request opens a tab");
        };
        let mut state = t.session(&tab.session).unwrap().state.clone();
        state.status = SessionStatus::Running;
        t.set_session_state(&tab.session, state);
        assert_eq!(t.plugin_session(request, TAB_NAME), Ok(PluginSession::Selected(tab.id.clone())));
        assert_eq!(t.plugin_session_failed("op").to_string(), "Terminal did not start");
        assert!(t.tabs().is_empty());
        let closed = TerminalEffect::SessionDisposed { key: tab.session, plugin_operation: Some("op".to_owned()) };
        assert_eq!(t.take_effects().last(), Some(&closed));
    }

    // Not from a spec: the start directory setting is absolute, home-relative or relative to the project root.
    #[test]
    fn resolves_the_start_directory_against_the_project_root() {
        assert_eq!(start_directory("", "/repo"), "/repo");
        assert_eq!(start_directory("/srv/app/", "/repo"), "/srv/app");
        assert_eq!(start_directory(r"C:\work", "/repo"), r"C:\work");
        assert_eq!(start_directory("~/src", "/repo"), "~/src");
        assert_eq!(start_directory("./web", "/repo/"), "/repo/web");
        assert_eq!(start_directory("web/app", "/repo"), "/repo/web/app");
    }

    // Not from a spec: "Open in Terminal" names the tab after the directory, and closing asks in the UI language.
    #[test]
    fn names_a_directory_tab_and_words_the_terminate_question() {
        let mut t = Terminals::new();
        assert_eq!(t.new_tab_at("/repo/web").name, "web");
        assert_eq!(t.title("t1").as_deref(), Some("web"));
        assert_eq!(t.active_session().map(|session| session.key.as_str()), Some("s1"));
        let question = terminate_confirmation("web", &I18n::default());
        assert_eq!(question.title, "Process 'web' Is Running");
        assert_eq!(question.message, "Do you want to terminate the process 'web'?");
        assert_eq!(question.ok, "Terminate");
    }
}
