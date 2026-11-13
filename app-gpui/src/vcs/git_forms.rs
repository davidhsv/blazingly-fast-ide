//! The forms of the Git menu's dialogs as data: Pull, Update Project, Merge, Rebase, Git Reset,
//! Tag, Stash, Unstash Changes, Rollback Changes, Git Remotes with Define / Edit Remote, Worktrees
//! and New Worktree.
//!
//! Each form keeps what was chosen in it and describes itself as a [`FormSpec`]: rows (hints,
//! labels, text fields, selects, checkboxes, radio groups, lists) and buttons with their enabled
//! state, its texts already in the UI language. [`crate::vcs::git_form_dialog::GitFormDialog`]
//! draws any of them and feeds the edits back with [`GitForm::edit`]; a button that can be pressed
//! answers with a [`FormAnswer`], which the workbench carries out on the backend. No form runs
//! anything itself.

use crate::platform::git_ref_name::clean_up_branch_name_on_typing;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::branches::{self, sort_local, sort_remote};
use crate::vcs::changes::{ChangeKind, FileStatus, base_name, dir_name, rel_to, strip_slash};
use crate::vcs_more::worktrees::{RepoWorktrees, is_current_worktree, worktree_contains_project};
use crate::vcs_ops::branch_ops::{local_names, validate_branch_name};
use crate::vcs_ops::change_ops::UnstashOptions;
use crate::vcs_ops::dialogs::PromptRequest;
use crate::vcs_ops::operations::validate_tag_name;
use ide_vcs::{Branch, BranchInfo, MergeOptions, RebaseOptions, Remote, RemoteEdit, ResetMode, Stash, Worktree};
use std::collections::BTreeSet;

/// The id of the button that accepts a form.
pub const OK: &str = "ok";
/// The id of the button that closes a form without an answer; Escape does the same.
pub const CANCEL: &str = "cancel";

/// An option of a select or a radio group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    /// Shown after (a select) or below (a radio option) the label.
    pub detail: Option<String>,
}

impl Choice {
    fn new(value: &str, label: &str, detail: Option<String>) -> Self {
        Self { value: value.to_owned(), label: label.to_owned(), detail }
    }
}

/// A row of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub key: String,
    pub label: String,
    /// Muted text after the label.
    pub detail: String,
    /// How deep the row sits in a tree.
    pub depth: usize,
    /// The row's checkbox, when it has one.
    pub checked: Option<bool>,
}

/// A row of a form, top to bottom. Its `Debug` output names the row's kind and control, never a
/// text: a field may hold a remote URL with its credentials as it is typed.
#[derive(Clone, PartialEq, Eq)]
pub enum FormRow {
    /// Muted text; a line break starts a new line.
    Hint(String),
    /// What the control below it is for.
    Label(String),
    Text {
        id: &'static str,
        value: String,
        placeholder: String,
        mono: bool,
        invalid: bool,
    },
    Select {
        id: &'static str,
        value: Option<String>,
        placeholder: String,
        options: Vec<Choice>,
    },
    Check {
        id: &'static str,
        label: String,
        on: bool,
        enabled: bool,
    },
    Radio {
        id: &'static str,
        value: String,
        options: Vec<Choice>,
    },
    /// A list of rows, one of them selected; `empty` shows while it has none.
    List {
        id: &'static str,
        rows: Vec<ListRow>,
        selected: Option<String>,
        empty: String,
    },
    /// A problem, in the danger colour.
    Error(String),
}

/// What `Debug` writes in place of a text a form may hold.
const REDACTED: &str = "<redacted>";

impl std::fmt::Debug for FormRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hint(_) => f.debug_tuple("Hint").field(&REDACTED).finish(),
            Self::Label(_) => f.debug_tuple("Label").field(&REDACTED).finish(),
            Self::Text { id, mono, invalid, .. } => f
                .debug_struct("Text")
                .field("id", id)
                .field("value", &REDACTED)
                .field("mono", mono)
                .field("invalid", invalid)
                .finish_non_exhaustive(),
            Self::Select { id, value, options, .. } => f
                .debug_struct("Select")
                .field("id", id)
                .field("value", &value.as_ref().map(|_| REDACTED))
                .field("options", &options.len())
                .finish_non_exhaustive(),
            Self::Check { id, on, enabled, .. } => f
                .debug_struct("Check")
                .field("id", id)
                .field("on", on)
                .field("enabled", enabled)
                .finish_non_exhaustive(),
            Self::Radio { id, options, .. } => {
                f.debug_struct("Radio").field("id", id).field("options", &options.len()).finish_non_exhaustive()
            }
            Self::List { id, rows, selected, .. } => f
                .debug_struct("List")
                .field("id", id)
                .field("rows", &rows.len())
                .field("selected", &selected.as_ref().map(|_| REDACTED))
                .finish_non_exhaustive(),
            Self::Error(_) => f.debug_tuple("Error").field(&REDACTED).finish(),
        }
    }
}

/// Where a button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonPlace {
    /// Above the rows: the small buttons of a list.
    Toolbar,
    /// At the start of the footer, away from the default button.
    FooterStart,
    Footer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormButton {
    pub id: &'static str,
    pub label: String,
    /// The default button: filled, and pressed by Enter.
    pub primary: bool,
    pub enabled: bool,
    /// Shown when the pointer rests on the button.
    pub tooltip: Option<String>,
    pub place: ButtonPlace,
}

impl FormButton {
    fn footer(id: &'static str, label: String, primary: bool, enabled: bool) -> Self {
        Self { id, label, primary, enabled, tooltip: None, place: ButtonPlace::Footer }
    }

    fn toolbar(id: &'static str, label: String, enabled: bool) -> Self {
        Self { id, label, primary: false, enabled, tooltip: None, place: ButtonPlace::Toolbar }
    }

    fn cancel(i18n: &I18n) -> Self {
        Self::footer(CANCEL, i18n.tx("Cancel"), false, true)
    }
}

/// What a form shows. Its `Debug` output holds the title, the rows' kinds and controls and the
/// buttons' ids, never a text of a row.
#[derive(Clone, PartialEq)]
pub struct FormSpec {
    pub title: String,
    pub width: f32,
    pub rows: Vec<FormRow>,
    pub buttons: Vec<FormButton>,
}

impl std::fmt::Debug for FormSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let buttons: Vec<_> = self.buttons.iter().map(|button| (button.id, button.enabled)).collect();
        f.debug_struct("FormSpec")
            .field("title", &self.title)
            .field("width", &self.width)
            .field("rows", &self.rows)
            .field("buttons", &buttons)
            .finish()
    }
}

impl FormSpec {
    /// The button `id`, when the form has it.
    pub fn button(&self, id: &str) -> Option<&FormButton> {
        self.buttons.iter().find(|button| button.id == id)
    }
}

/// An edit of a form's control `id`. Its `Debug` output names the kind of edit, never the text.
#[derive(Clone, PartialEq, Eq)]
pub enum FormInput {
    /// A text field's text.
    Text(String),
    /// A select's or a radio group's value, or a list's selected row.
    Choose(String),
    /// A checkbox.
    Toggle,
    /// The checkbox of a list's row.
    ToggleRow(String),
}

impl std::fmt::Debug for FormInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(_) => f.debug_tuple("Text").field(&REDACTED).finish(),
            Self::Choose(_) => f.debug_tuple("Choose").field(&REDACTED).finish(),
            Self::Toggle => f.write_str("Toggle"),
            Self::ToggleRow(_) => f.debug_tuple("ToggleRow").field(&REDACTED).finish(),
        }
    }
}

/// What the Git Remotes dialog asks for.
#[derive(Clone, PartialEq, Eq)]
pub enum RemotesCommand {
    Add,
    Edit(Remote),
    Remove(String),
    Close,
}

impl std::fmt::Debug for RemotesCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Add => f.write_str("Add"),
            Self::Edit(remote) => write!(f, "Edit({}, {})", remote.name, redacted_url(&remote.url)),
            Self::Remove(name) => write!(f, "Remove({name})"),
            Self::Close => f.write_str("Close"),
        }
    }
}

/// Where the user info of `url` is (`scheme://<user info>@host/…`), and its scheme.
fn user_info(url: &str) -> Option<(&str, std::ops::Range<usize>)> {
    let scheme_end = url.find("://")?;
    let start = scheme_end + 3;
    let authority_end = url[start..].find(['/', '?', '#']).map_or(url.len(), |at| start + at);
    let end = start + url[start..authority_end].rfind('@')?;
    Some((&url[..scheme_end], start..end))
}

/// The user info as it may be shown: a password masked (`ada:***`), and over HTTP(S), where a user
/// name alone is usually a token, all of it (`***`).
fn masked_user_info(scheme: &str, info: &str) -> String {
    match info.split_once(':') {
        Some((user, _)) => format!("{user}:***"),
        None if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") => "***".to_owned(),
        None => info.to_owned(),
    }
}

/// `url` with its credentials masked (`https://ada:secret@host` → `https://ada:***@host`,
/// `https://token@host` → `https://***@host`), for showing a remote's URL: a credential typed into
/// a URL is never shown again.
pub fn redacted_url(url: &str) -> std::borrow::Cow<'_, str> {
    let Some((scheme, range)) = user_info(url) else { return url.into() };
    let masked = masked_user_info(scheme, &url[range.clone()]);
    if masked == url[range.clone()] {
        return url.into();
    }
    format!("{}{masked}{}", &url[..range.start], &url[range.end..]).into()
}

/// `typed`, an edit of the remote URL `original` shown redacted: where its user info is still the
/// mask shown, the original credentials go back in its place.
fn keep_credentials(typed: &str, original: &str) -> String {
    let (Some((scheme, kept)), Some((_, range))) = (user_info(original), user_info(typed)) else {
        return typed.to_owned();
    };
    let info = &original[kept];
    if masked_user_info(scheme, info) == typed[range.clone()] && masked_user_info(scheme, info) != info {
        return format!("{}{info}{}", &typed[..range.start], &typed[range.end..]);
    }
    typed.to_owned()
}

/// What the Worktrees dialog asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreesCommand {
    Refresh,
    /// New Worktree… in repository `repo` (`None`: the selected one).
    New {
        repo: Option<String>,
    },
    /// Remove the worktree at `path` of the repository at `root`.
    Remove {
        root: String,
        path: String,
    },
    /// Open the worktree at this path in a window of its own.
    Open(String),
    Close,
}

/// What a pressed button asks the workbench to do. It may hold a remote's URL with its credentials,
/// so it has no `Debug`.
#[derive(Clone)]
pub enum FormAnswer {
    /// `detail`: what the progress and the Git console say of the pull.
    Pull {
        remote: String,
        branch: Option<String>,
        rebase: bool,
        options: MergeOptions,
        detail: String,
    },
    /// Update Project with this method; `hide`: Don't show again.
    Update {
        rebase: bool,
        hide: bool,
    },
    ResetToRemote,
    Merge {
        branch: String,
        options: MergeOptions,
    },
    Rebase {
        upstream: String,
        options: RebaseOptions,
    },
    Reset {
        commit: String,
        mode: ResetMode,
    },
    Tag {
        name: String,
        commit: String,
        message: Option<String>,
        force: bool,
    },
    Stash {
        message: Option<String>,
        keep_index: bool,
        include_untracked: bool,
    },
    Unstash(UnstashOptions),
    Rollback {
        files: Vec<FileStatus>,
        delete_added: bool,
    },
    Remotes(RemotesCommand),
    EditRemote(RemoteEdit),
    Worktrees(WorktreesCommand),
    NewWorktree {
        path: String,
        source: String,
        new_branch: Option<String>,
    },
}

/// A form of a Git dialog.
pub trait GitForm: 'static {
    /// What the form shows now.
    fn spec(&self, i18n: &mut I18n) -> FormSpec;
    /// Control `id` was edited.
    fn edit(&mut self, id: &str, input: FormInput);
    /// What pressing `button` answers; `None` while it cannot be pressed or when it closes the
    /// form without an answer.
    fn press(&self, button: &str, i18n: &mut I18n) -> Option<FormAnswer>;
    /// A problem the workbench met carrying out the answer, shown in the form, which stays open.
    fn set_error(&mut self, _error: Option<String>) {}
}

fn head_or_default(head: Option<&str>) -> &str {
    head.filter(|head| !head.is_empty()).unwrap_or("HEAD")
}

/// A text field that is never shown as invalid.
fn text_row(id: &'static str, value: String, placeholder: String, mono: bool) -> FormRow {
    FormRow::Text { id, value, placeholder, mono, invalid: false }
}

// ---------------------------------------------------------------------------------------------
// Merge options

/// A merge option of the Merge and Pull dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MergeFlag {
    NoFf,
    FfOnly,
    Squash,
    NoCommit,
}

impl MergeFlag {
    pub const ALL: [MergeFlag; 4] = [MergeFlag::NoFf, MergeFlag::FfOnly, MergeFlag::Squash, MergeFlag::NoCommit];

    /// The control id of the flag's checkbox.
    pub fn id(self) -> &'static str {
        match self {
            Self::NoFf => "noFf",
            Self::FfOnly => "ffOnly",
            Self::Squash => "squash",
            Self::NoCommit => "noCommit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NoFf => "--no-ff",
            Self::FfOnly => "--ff-only",
            Self::Squash => "--squash",
            Self::NoCommit => "--no-commit",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|flag| flag.id() == id)
    }

    /// The flags git refuses together with this one.
    fn exclusive(self) -> &'static [MergeFlag] {
        match self {
            Self::NoFf => &[Self::FfOnly, Self::Squash],
            Self::FfOnly => &[Self::NoFf, Self::Squash, Self::NoCommit],
            Self::Squash => &[Self::NoFf, Self::FfOnly],
            Self::NoCommit => &[Self::FfOnly],
        }
    }
}

/// Merge flags that cannot be turned on next to the `on` ones; with `--rebase`, none can.
pub fn disabled_merge_flags(on: &BTreeSet<MergeFlag>, rebase: bool) -> BTreeSet<MergeFlag> {
    if rebase {
        return MergeFlag::ALL.into_iter().collect();
    }
    on.iter().flat_map(|flag| flag.exclusive().iter().copied()).collect()
}

/// The backend's merge options for the `on` flags and the `-m` message, blank meaning none.
pub fn merge_options_of(on: &BTreeSet<MergeFlag>, message: Option<&str>) -> MergeOptions {
    MergeOptions {
        no_ff: on.contains(&MergeFlag::NoFf),
        ff_only: on.contains(&MergeFlag::FfOnly),
        squash: on.contains(&MergeFlag::Squash),
        no_commit: on.contains(&MergeFlag::NoCommit),
        message: message.map(str::trim).filter(|message| !message.is_empty()).map(str::to_owned),
    }
}

/// The checkboxes of the merge flags: one that clashes with a checked one cannot be checked.
fn flag_rows(on: &BTreeSet<MergeFlag>, rebase: bool) -> Vec<FormRow> {
    let disabled = disabled_merge_flags(on, rebase);
    MergeFlag::ALL
        .into_iter()
        .map(|flag| FormRow::Check {
            id: flag.id(),
            label: flag.label().to_owned(),
            on: on.contains(&flag),
            enabled: !disabled.contains(&flag) || on.contains(&flag),
        })
        .collect()
}

/// Turns `flag` on or off, unless a checked flag it clashes with keeps it off.
fn toggle_flag(on: &mut BTreeSet<MergeFlag>, flag: MergeFlag, rebase: bool) {
    if on.remove(&flag) {
        return;
    }
    if !disabled_merge_flags(on, rebase).contains(&flag) {
        on.insert(flag);
    }
}

/// The backend's branches as the branch models order them.
fn sortable(branches: &[Branch]) -> Vec<branches::Branch> {
    branches
        .iter()
        .map(|branch| branches::Branch { name: branch.name.clone(), remote: branch.remote, current: branch.current })
        .collect()
}

/// The branches to pick from: names, without the remotes' `HEAD`.
fn branch_choices<'a>(branches: impl IntoIterator<Item = &'a branches::Branch>) -> Vec<Choice> {
    branches
        .into_iter()
        .filter(|branch| !branch.name.ends_with("/HEAD"))
        .map(|branch| Choice::new(&branch.name, &branch.name, None))
        .collect()
}

/// The local branches but the current one, then the remote ones: what Merge and Rebase offer.
fn other_branches(branches: &[Branch]) -> Vec<Choice> {
    let branches = sortable(branches);
    let local = sort_local(&branches, None).into_iter().filter(|branch| !branch.current);
    let mut choices = branch_choices(local);
    choices.extend(branch_choices(sort_remote(&branches, None)));
    choices
}

/// Remote branches of `remote` without the remote prefix (`origin/main` → `main`).
pub fn remote_branches_of(branches: &[Branch], remote: &str) -> Vec<String> {
    let prefix = format!("{remote}/");
    let branches = sortable(branches);
    sort_remote(&branches, None)
        .into_iter()
        .filter_map(|branch| branch.name.strip_prefix(&prefix))
        .filter(|name| *name != "HEAD")
        .map(str::to_owned)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Pull

/// Pull…: a remote, one of its branches, `--rebase` and the merge flags.
pub struct PullForm {
    head: Option<String>,
    upstream: Option<String>,
    remotes: Vec<Remote>,
    branches: Vec<Branch>,
    remote: Option<String>,
    chosen: Option<String>,
    rebase: bool,
    flags: BTreeSet<MergeFlag>,
}

impl PullForm {
    /// The form for the current branch `info`, with the repository's `remotes` and `branches`:
    /// the upstream's remote is chosen, else the first.
    pub fn new(info: Option<&BranchInfo>, remotes: Vec<Remote>, branches: Vec<Branch>) -> Self {
        let upstream = info.and_then(|info| info.upstream.clone()).filter(|upstream| !upstream.is_empty());
        let upstream_remote = upstream.as_deref().and_then(|upstream| upstream.split('/').next());
        let remote = remotes
            .iter()
            .find(|remote| Some(remote.name.as_str()) == upstream_remote)
            .or(remotes.first())
            .map(|remote| remote.name.clone());
        Self {
            head: info.and_then(|info| info.head.clone()),
            upstream,
            remotes,
            branches,
            remote,
            chosen: None,
            rebase: false,
            flags: BTreeSet::new(),
        }
    }

    /// The branch pulled: the one chosen, else the upstream's, else the current branch's
    /// namesake, else the remote's first.
    pub fn branch(&self) -> Option<String> {
        fn listed_one<'a>(name: Option<&'a str>, listed: &[String]) -> Option<&'a str> {
            name.filter(|name| listed.iter().any(|listed| listed == name))
        }
        let remote = self.remote.as_deref()?;
        let listed = remote_branches_of(&self.branches, remote);
        let tracked = self
            .upstream
            .as_deref()
            .and_then(|upstream| upstream.strip_prefix(remote))
            .and_then(|rest| rest.strip_prefix('/'));
        let preferred = tracked.or(self.head.as_deref());
        listed_one(self.chosen.as_deref(), &listed)
            .or_else(|| listed_one(preferred, &listed))
            .map(str::to_owned)
            .or_else(|| listed.first().cloned())
    }

    /// What the progress and the Git console say of the pull.
    pub fn detail(&self) -> String {
        let branch = self.branch().map(|branch| format!("/{branch}")).unwrap_or_default();
        format!("{} ← {}{branch}", head_or_default(self.head.as_deref()), self.remote.as_deref().unwrap_or_default())
    }
}

impl GitForm for PullForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let mut rows = Vec::new();
        if self.remotes.is_empty() {
            rows.push(FormRow::Hint(i18n.tx("No remotes are configured. Add one with Git › Manage Remotes….")));
        } else {
            let remotes = self
                .remotes
                .iter()
                .map(|remote| Choice::new(&remote.name, &remote.name, Some(redacted_url(&remote.url).into_owned())));
            rows.push(FormRow::Select {
                id: "remote",
                value: self.remote.clone(),
                placeholder: String::new(),
                options: remotes.collect(),
            });
            let branches = self.remote.as_deref().map(|remote| remote_branches_of(&self.branches, remote));
            rows.push(FormRow::Select {
                id: "branch",
                value: self.branch(),
                placeholder: i18n.tx("Branch"),
                options: branches.unwrap_or_default().iter().map(|name| Choice::new(name, name, None)).collect(),
            });
        }
        rows.push(FormRow::Label(i18n.tx("Options:")));
        rows.push(FormRow::Check { id: "rebase", label: "--rebase".to_owned(), on: self.rebase, enabled: true });
        rows.extend(flag_rows(&self.flags, self.rebase));
        rows.push(FormRow::Hint(i18n.tx("Uncommitted changes are stashed before the pull and restored afterwards.")));
        let head = head_or_default(self.head.as_deref());
        FormSpec {
            title: i18n.tx_built(&["Pull to ", ""], &[Param::from(head)]),
            width: 460.0,
            rows,
            buttons: vec![
                FormButton::footer(OK, i18n.tx("Pull"), true, self.remote.is_some()),
                FormButton::cancel(i18n),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("remote", FormInput::Choose(remote)) => self.remote = Some(remote),
            ("branch", FormInput::Choose(branch)) => self.chosen = Some(branch),
            ("rebase", FormInput::Toggle) => self.rebase = !self.rebase,
            (id, FormInput::Toggle) => {
                if let Some(flag) = MergeFlag::from_id(id) {
                    toggle_flag(&mut self.flags, flag, self.rebase);
                }
            }
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let remote = self.remote.clone().filter(|_| button == OK)?;
        let flags = if self.rebase { BTreeSet::new() } else { self.flags.clone() };
        Some(FormAnswer::Pull {
            remote,
            branch: self.branch(),
            rebase: self.rebase,
            options: merge_options_of(&flags, None),
            detail: self.detail(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Update Project

/// Update Project: merge or rebase, Reset to the Remote Branch, Don't show again.
pub struct UpdateForm {
    head: Option<String>,
    upstream: Option<String>,
    behind: u32,
    /// The project has one repository: Reset to the Remote Branch is offered.
    single: bool,
    rebase: bool,
    hide: bool,
}

impl UpdateForm {
    /// The form for the current branch `info`; `rebase`: the stored update method is rebase.
    pub fn new(info: Option<&BranchInfo>, single: bool, rebase: bool) -> Self {
        Self {
            head: info.and_then(|info| info.head.clone()),
            upstream: info.and_then(|info| info.upstream.clone()).filter(|upstream| !upstream.is_empty()),
            behind: info.map_or(0, |info| info.behind),
            single,
            rebase,
            hide: false,
        }
    }
}

impl GitForm for UpdateForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let summary = match &self.upstream {
            Some(upstream) => {
                let incoming = if self.behind > 0 { format!(" ({} incoming)", self.behind) } else { String::new() };
                format!("Update {} from {upstream}{incoming}", head_or_default(self.head.as_deref()))
            }
            None => i18n.tx("The current branch has no tracked branch; git pull may fail."),
        };
        let options = vec![
            Choice::new(
                "merge",
                &i18n.tx("Merge incoming changes into the current branch"),
                Some(i18n.tx("git pull --no-rebase --ff")),
            ),
            Choice::new(
                "rebase",
                &i18n.tx("Rebase the current branch on top of incoming changes"),
                Some(i18n.tx("git pull --rebase")),
            ),
        ];
        let rows = vec![
            FormRow::Hint(summary),
            FormRow::Label(i18n.tx("Update type:")),
            FormRow::Radio { id: "type", value: if self.rebase { "rebase" } else { "merge" }.to_owned(), options },
            FormRow::Hint(i18n.tx("Uncommitted changes are stashed before the update and restored afterwards.")),
            FormRow::Check { id: "hide", label: i18n.tx("Don't show again"), on: self.hide, enabled: true },
        ];
        let mut buttons = Vec::new();
        if self.single {
            buttons.push(FormButton {
                place: ButtonPlace::FooterStart,
                ..FormButton::footer("resetToRemote", i18n.tx("Reset to the Remote Branch"), false, true)
            });
        }
        buttons.push(FormButton::footer(OK, i18n.tx("OK"), true, true));
        buttons.push(FormButton::cancel(i18n));
        FormSpec { title: i18n.tx("Update Project"), width: if self.single { 540.0 } else { 420.0 }, rows, buttons }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("type", FormInput::Choose(value)) => self.rebase = value == "rebase",
            ("hide", FormInput::Toggle) => self.hide = !self.hide,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        match button {
            OK => Some(FormAnswer::Update { rebase: self.rebase, hide: self.hide }),
            "resetToRemote" if self.single => Some(FormAnswer::ResetToRemote),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Merge

/// Merge…: the branch to merge into the current one, the merge flags and `-m`.
pub struct MergeForm {
    head: Option<String>,
    options: Vec<Choice>,
    branch: Option<String>,
    flags: BTreeSet<MergeFlag>,
    with_message: bool,
    message: String,
}

impl MergeForm {
    pub fn new(head: Option<String>, branches: &[Branch]) -> Self {
        Self {
            head,
            options: other_branches(branches),
            branch: None,
            flags: BTreeSet::new(),
            with_message: false,
            message: String::new(),
        }
    }
}

impl GitForm for MergeForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let head = head_or_default(self.head.as_deref()).to_owned();
        let mut rows = vec![
            FormRow::Label(i18n.tx("Branch to merge:")),
            FormRow::Select {
                id: "branch",
                value: self.branch.clone(),
                placeholder: i18n.tx("Select branch"),
                options: self.options.clone(),
            },
            FormRow::Label(i18n.tx("Options:")),
        ];
        rows.extend(flag_rows(&self.flags, false));
        rows.push(FormRow::Check {
            id: "withMessage",
            label: i18n.tx("-m (commit message)"),
            on: self.with_message,
            enabled: true,
        });
        if self.with_message {
            let placeholder = match &self.branch {
                Some(branch) => {
                    let values = [Param::from(branch.as_str()), Param::from(head.as_str())];
                    i18n.tx_built(&["Merge branch '", "' into ", ""], &values)
                }
                None => String::new(),
            };
            let message = self.message.clone();
            rows.push(FormRow::Text { id: "message", value: message, placeholder, mono: false, invalid: false });
        }
        FormSpec {
            title: i18n.tx_built(&["Merge into ", ""], &[Param::from(head.as_str())]),
            width: 460.0,
            rows,
            buttons: vec![
                FormButton::footer(OK, i18n.tx("Merge"), true, self.branch.is_some()),
                FormButton::cancel(i18n),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("branch", FormInput::Choose(branch)) => self.branch = Some(branch),
            ("withMessage", FormInput::Toggle) => self.with_message = !self.with_message,
            ("message", FormInput::Text(text)) => self.message = text,
            (id, FormInput::Toggle) => {
                if let Some(flag) = MergeFlag::from_id(id) {
                    toggle_flag(&mut self.flags, flag, false);
                }
            }
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let branch = self.branch.clone().filter(|_| button == OK)?;
        let message = self.with_message.then_some(self.message.as_str());
        Some(FormAnswer::Merge { branch, options: merge_options_of(&self.flags, message) })
    }
}

// ---------------------------------------------------------------------------------------------
// Rebase

/// Rebase…: the upstream, `--onto` a new base, `--rebase-merges`, `--keep-empty`.
pub struct RebaseForm {
    head: Option<String>,
    options: Vec<Choice>,
    upstream: Option<String>,
    use_onto: bool,
    onto: Option<String>,
    rebase_merges: bool,
    keep_empty: bool,
}

impl RebaseForm {
    pub fn new(head: Option<String>, branches: &[Branch]) -> Self {
        Self {
            head,
            options: other_branches(branches),
            upstream: None,
            use_onto: false,
            onto: None,
            rebase_merges: false,
            keep_empty: false,
        }
    }

    fn ready(&self) -> bool {
        self.upstream.is_some() && (!self.use_onto || self.onto.is_some())
    }
}

impl GitForm for RebaseForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let head = head_or_default(self.head.as_deref());
        let label = if self.use_onto {
            i18n.tx_built(&["Rebase commits of ", " not on:"], &[Param::from(head)])
        } else {
            format!("Rebase {head} onto:")
        };
        let mut rows = vec![
            FormRow::Label(label),
            FormRow::Select {
                id: "upstream",
                value: self.upstream.clone(),
                placeholder: i18n.tx("Select branch"),
                options: self.options.clone(),
            },
            FormRow::Check { id: "onto", label: "--onto".to_owned(), on: self.use_onto, enabled: true },
        ];
        if self.use_onto {
            rows.push(FormRow::Select {
                id: "newBase",
                value: self.onto.clone(),
                placeholder: i18n.tx("New base"),
                options: self.options.clone(),
            });
        }
        rows.push(FormRow::Check {
            id: "rebaseMerges",
            label: "--rebase-merges".to_owned(),
            on: self.rebase_merges,
            enabled: true,
        });
        let keep_empty = self.keep_empty;
        rows.push(FormRow::Check { id: "keepEmpty", label: "--keep-empty".to_owned(), on: keep_empty, enabled: true });
        rows.push(FormRow::Hint(i18n.tx("Uncommitted changes are stashed before the rebase and restored afterwards.")));
        FormSpec {
            title: i18n.tx("Rebase"),
            width: 460.0,
            rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("Rebase"), true, self.ready()), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("upstream", FormInput::Choose(branch)) => self.upstream = Some(branch),
            ("newBase", FormInput::Choose(branch)) => self.onto = Some(branch),
            ("onto", FormInput::Toggle) => self.use_onto = !self.use_onto,
            ("rebaseMerges", FormInput::Toggle) => self.rebase_merges = !self.rebase_merges,
            ("keepEmpty", FormInput::Toggle) => self.keep_empty = !self.keep_empty,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        if button != OK || !self.ready() {
            return None;
        }
        let upstream = self.upstream.clone()?;
        let options = RebaseOptions {
            onto: self.onto.clone().filter(|_| self.use_onto),
            rebase_merges: self.rebase_merges,
            keep_empty: self.keep_empty,
        };
        Some(FormAnswer::Rebase { upstream, options })
    }
}

// ---------------------------------------------------------------------------------------------
// Git Reset

/// The `git reset` mode's name, as the success message says it.
pub fn reset_mode_name(mode: ResetMode) -> &'static str {
    match mode {
        ResetMode::Soft => "soft",
        ResetMode::Mixed => "mixed",
        ResetMode::Hard => "hard",
        ResetMode::Keep => "keep",
    }
}

const RESET_MODES: [(ResetMode, &str, &str); 4] = [
    (ResetMode::Soft, "Soft", "Files won't change, differences will be staged for commit."),
    (ResetMode::Mixed, "Mixed", "Files won't change, differences won't be staged."),
    (
        ResetMode::Hard,
        "Hard",
        "Files will be reverted to the state of the selected commit. Warning: any local changes will be lost.",
    ),
    (
        ResetMode::Keep,
        "Keep",
        "Files will be reverted to the state of the selected commit, but local changes will be kept intact.",
    ),
];

/// Git Reset: the commit to reset the current branch to and the mode. Git › Reset HEAD… types the
/// commit; Reset Current Branch to Here… of the Git log names its commit instead of a field.
pub struct ResetForm {
    target: String,
    mode: ResetMode,
    /// The log's commit as its short hash and subject, shown in place of the field.
    commit_line: Option<String>,
}

impl Default for ResetForm {
    fn default() -> Self {
        Self { target: "HEAD".to_owned(), mode: ResetMode::Mixed, commit_line: None }
    }
}

impl ResetForm {
    /// Reset Current Branch to Here…: resets to the commit `id`, shown as `short_id` and `summary`.
    pub fn on_commit(id: &str, short_id: &str, summary: &str) -> Self {
        Self { target: id.to_owned(), mode: ResetMode::Mixed, commit_line: Some(format!("{short_id} {summary}")) }
    }
}

impl GitForm for ResetForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let options = RESET_MODES
            .iter()
            .map(|(mode, label, hint)| Choice::new(reset_mode_name(*mode), &i18n.tx(label), Some(i18n.tx(hint))))
            .collect();
        let mut rows = match &self.commit_line {
            Some(line) => {
                let explanation = i18n.tx(concat!(
                    "This will reset the current branch head to the selected commit, and update the working tree ",
                    "and the index according to the selected mode:"
                ));
                vec![FormRow::Hint(format!("{explanation}\n{line}"))]
            }
            None => vec![
                FormRow::Label(i18n.tx("To commit:")),
                text_row("target", self.target.clone(), String::new(), true),
            ],
        };
        rows.push(FormRow::Radio { id: "mode", value: reset_mode_name(self.mode).to_owned(), options });
        let ready = !self.target.trim().is_empty();
        FormSpec {
            title: i18n.tx("Git Reset"),
            width: 520.0,
            rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("Reset"), true, ready), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("target", FormInput::Text(text)) => self.target = text,
            ("mode", FormInput::Choose(value)) => {
                if let Some((mode, ..)) = RESET_MODES.iter().find(|(mode, ..)| reset_mode_name(*mode) == value) {
                    self.mode = *mode;
                }
            }
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let commit = self.target.trim();
        (button == OK && !commit.is_empty()).then(|| FormAnswer::Reset { commit: commit.to_owned(), mode: self.mode })
    }
}

// ---------------------------------------------------------------------------------------------
// Tag

/// New Tag…: the name (checked against the repository's tags unless forced), the commit and an
/// optional message, which makes the tag annotated. New Tag… of the Git log asks for the name alone,
/// of a lightweight tag on its commit.
pub struct TagForm {
    existing: Vec<String>,
    name: String,
    commit: String,
    message: String,
    force: bool,
    /// The log's question for the name (title, label, button); the commit is fixed.
    prompt: Option<PromptRequest>,
}

impl TagForm {
    /// Git › New Tag…: a tag on the commit typed, `HEAD` at first.
    pub fn new(existing: Vec<String>) -> Self {
        Self {
            existing,
            name: String::new(),
            commit: "HEAD".to_owned(),
            message: String::new(),
            force: false,
            prompt: None,
        }
    }

    /// New Tag… of the Git log: the name of a lightweight tag on `commit`, asked as `prompt` says.
    pub fn on_commit(prompt: PromptRequest, commit: &str, existing: Vec<String>) -> Self {
        Self { commit: commit.to_owned(), prompt: Some(prompt), ..Self::new(existing) }
    }

    /// Why the name cannot be the tag's.
    pub fn problem(&self) -> Option<String> {
        validate_tag_name(&self.name, if self.force { &[] } else { &self.existing })
    }
}

impl GitForm for TagForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let problem = self.problem();
        let shown = problem.clone().filter(|_| !self.name.is_empty());
        if let Some(prompt) = &self.prompt {
            let mut rows: Vec<FormRow> = prompt.label.iter().map(|label| FormRow::Label(i18n.tx(label))).collect();
            rows.push(FormRow::Text {
                id: "name",
                value: self.name.clone(),
                placeholder: String::new(),
                mono: prompt.mono,
                invalid: shown.is_some(),
            });
            rows.extend(shown.map(|problem| FormRow::Error(i18n.tx(&problem))));
            let ok = if prompt.ok_label.is_empty() { "OK" } else { prompt.ok_label.as_str() };
            return FormSpec {
                title: prompt.title.clone(),
                width: 420.0,
                rows,
                buttons: vec![FormButton::footer(OK, i18n.tx(ok), true, problem.is_none()), FormButton::cancel(i18n)],
            };
        }
        let mut rows = vec![
            FormRow::Label(i18n.tx("Tag name:")),
            FormRow::Text {
                id: "name",
                value: self.name.clone(),
                placeholder: String::new(),
                mono: true,
                invalid: shown.is_some(),
            },
        ];
        rows.extend(shown.map(FormRow::Error));
        rows.extend([
            FormRow::Check { id: "force", label: i18n.tx("Force"), on: self.force, enabled: true },
            FormRow::Label(i18n.tx("Commit:")),
            text_row("commit", self.commit.clone(), String::new(), true),
            FormRow::Label(i18n.tx("Message:")),
            FormRow::Text {
                id: "message",
                value: self.message.clone(),
                placeholder: i18n.tx("Optional; creates an annotated tag"),
                mono: false,
                invalid: false,
            },
        ]);
        FormSpec {
            title: i18n.tx("Tag"),
            width: 460.0,
            rows,
            buttons: vec![
                FormButton::footer(OK, i18n.tx("Create Tag"), true, problem.is_none()),
                FormButton::cancel(i18n),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("name", FormInput::Text(text)) => self.name = text,
            ("commit", FormInput::Text(text)) => self.commit = text,
            ("message", FormInput::Text(text)) => self.message = text,
            ("force", FormInput::Toggle) => self.force = !self.force,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        if button != OK || self.problem().is_some() {
            return None;
        }
        let commit = self.commit.trim();
        Some(FormAnswer::Tag {
            name: self.name.trim().to_owned(),
            commit: if commit.is_empty() { "HEAD" } else { commit }.to_owned(),
            message: (!self.message.is_empty()).then(|| self.message.clone()),
            force: self.force,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Stash and Unstash

/// Stash Changes…: a message, Keep index, Include untracked files.
pub struct StashForm {
    branch: Option<String>,
    message: String,
    keep_index: bool,
    include_untracked: bool,
}

impl StashForm {
    /// The form on the current branch `branch`.
    pub fn new(branch: Option<String>) -> Self {
        Self { branch, message: String::new(), keep_index: false, include_untracked: false }
    }
}

impl GitForm for StashForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let mut rows = Vec::new();
        if let Some(branch) = self.branch.as_deref().filter(|branch| !branch.is_empty()) {
            rows.push(FormRow::Hint(i18n.tx_built(&["Current branch: ", ""], &[Param::from(branch)])));
        }
        rows.extend([
            FormRow::Label(i18n.tx("Message:")),
            FormRow::Text {
                id: "message",
                value: self.message.clone(),
                placeholder: i18n.tx("WIP"),
                mono: false,
                invalid: false,
            },
            FormRow::Check { id: "keepIndex", label: i18n.tx("Keep index"), on: self.keep_index, enabled: true },
            FormRow::Check {
                id: "includeUntracked",
                label: i18n.tx("Include untracked files"),
                on: self.include_untracked,
                enabled: true,
            },
        ]);
        FormSpec {
            title: i18n.tx("Stash"),
            width: 440.0,
            rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("Create Stash"), true, true), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("message", FormInput::Text(text)) => self.message = text,
            ("keepIndex", FormInput::Toggle) => self.keep_index = !self.keep_index,
            ("includeUntracked", FormInput::Toggle) => self.include_untracked = !self.include_untracked,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let message = self.message.trim();
        (button == OK).then(|| FormAnswer::Stash {
            message: (!message.is_empty()).then(|| message.to_owned()),
            keep_index: self.keep_index,
            include_untracked: self.include_untracked,
        })
    }
}

/// Unstash Changes…: a stash of the list, Pop stash, Reinstate index, As new branch.
pub struct UnstashForm {
    stashes: Vec<Stash>,
    index: u32,
    pop: bool,
    reinstate_index: bool,
    branch: String,
}

impl UnstashForm {
    /// The form over the repository's `stashes`, stash `index` chosen.
    pub fn new(stashes: Vec<Stash>, index: u32) -> Self {
        Self { stashes, index, pop: false, reinstate_index: false, branch: String::new() }
    }

    /// The stash applied: the one chosen, else the first.
    fn chosen(&self) -> Option<&Stash> {
        self.stashes.iter().find(|stash| stash.index == self.index).or(self.stashes.first())
    }

    fn as_branch(&self) -> &str {
        self.branch.trim()
    }
}

impl GitForm for UnstashForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let rows: Vec<ListRow> = self
            .stashes
            .iter()
            .map(|stash| ListRow {
                key: stash.index.to_string(),
                label: format!("stash@{{{}}}: {}", stash.index, stash.message),
                detail: String::new(),
                depth: 0,
                checked: None,
            })
            .collect();
        let as_branch = !self.as_branch().is_empty();
        let label = if as_branch {
            "Create Branch"
        } else if self.pop {
            "Pop Stash"
        } else {
            "Apply Stash"
        };
        FormSpec {
            title: i18n.tx("Unstash Changes"),
            width: 520.0,
            rows: vec![
                FormRow::List {
                    id: "stashes",
                    rows,
                    selected: self.chosen().map(|stash| stash.index.to_string()),
                    empty: i18n.tx("No stashes"),
                },
                FormRow::Check {
                    id: "pop",
                    label: i18n.tx("Pop stash"),
                    on: self.pop || as_branch,
                    enabled: !as_branch,
                },
                FormRow::Check {
                    id: "reinstateIndex",
                    label: i18n.tx("Reinstate index"),
                    on: self.reinstate_index || as_branch,
                    enabled: !as_branch,
                },
                FormRow::Label(i18n.tx("As new branch:")),
                FormRow::Text {
                    id: "branch",
                    value: self.branch.clone(),
                    placeholder: String::new(),
                    mono: true,
                    invalid: false,
                },
            ],
            buttons: vec![
                FormButton::footer(OK, i18n.tx(label), true, self.chosen().is_some()),
                FormButton::cancel(i18n),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        let as_branch = !self.as_branch().is_empty();
        match (id, input) {
            ("stashes", FormInput::Choose(key)) => {
                if let Ok(index) = key.parse() {
                    self.index = index;
                }
            }
            ("pop", FormInput::Toggle) if !as_branch => self.pop = !self.pop,
            ("reinstateIndex", FormInput::Toggle) if !as_branch => self.reinstate_index = !self.reinstate_index,
            ("branch", FormInput::Text(text)) => self.branch = text,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let stash = self.chosen().filter(|_| button == OK)?;
        let branch = self.as_branch();
        Some(FormAnswer::Unstash(UnstashOptions {
            index: stash.index,
            pop: self.pop,
            reinstate_index: self.reinstate_index,
            branch: (!branch.is_empty()).then(|| branch.to_owned()),
        }))
    }
}

// ---------------------------------------------------------------------------------------------
// Rollback

/// Rollback Changes: the changed files, each checked to be rolled back, and Delete local copies of
/// added files when one was added.
pub struct RollbackForm {
    root: String,
    files: Vec<FileStatus>,
    unchecked: BTreeSet<String>,
    delete_added: bool,
}

impl RollbackForm {
    /// The form over `files`, every one checked; `root` is the project root the folders are
    /// shown relative to.
    pub fn new(root: String, files: Vec<FileStatus>) -> Self {
        Self { root, files, unchecked: BTreeSet::new(), delete_added: false }
    }

    fn chosen(&self) -> Vec<FileStatus> {
        self.files.iter().filter(|file| !self.unchecked.contains(&file.path)).cloned().collect()
    }
}

impl GitForm for RollbackForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let selected = self.files.len() - self.files.iter().filter(|file| self.unchecked.contains(&file.path)).count();
        let counts = [Param::from(selected), Param::from(self.files.len())];
        let message = if self.files.len() == 1 {
            let tail = " file will be reverted to their last committed state. Uncommitted changes are lost.";
            i18n.tx_built(&["", " of ", tail], &counts)
        } else {
            let tail = " files will be reverted to their last committed state. Uncommitted changes are lost.";
            i18n.tx_built(&["", " of ", tail], &counts)
        };
        let rows: Vec<ListRow> = self
            .files
            .iter()
            .map(|file| ListRow {
                key: file.path.clone(),
                label: base_name(&file.path).to_owned(),
                detail: dir_name(rel_to(&self.root, strip_slash(&file.path))).to_owned(),
                depth: 0,
                checked: Some(!self.unchecked.contains(&file.path)),
            })
            .collect();
        let mut form_rows =
            vec![FormRow::Hint(message), FormRow::List { id: "files", rows, selected: None, empty: String::new() }];
        if self.files.iter().any(|file| file.staged == Some(ChangeKind::Added)) {
            form_rows.push(FormRow::Check {
                id: "deleteAdded",
                label: i18n.tx("Delete local copies of added files"),
                on: self.delete_added,
                enabled: true,
            });
        }
        FormSpec {
            title: i18n.tx("Rollback Changes"),
            width: 520.0,
            rows: form_rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("Rollback"), true, selected > 0), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("files", FormInput::ToggleRow(path)) => {
                if !self.unchecked.remove(&path) && self.files.iter().any(|file| file.path == path) {
                    self.unchecked.insert(path);
                }
            }
            ("deleteAdded", FormInput::Toggle) => self.delete_added = !self.delete_added,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let files = self.chosen();
        (button == OK && !files.is_empty()).then_some(FormAnswer::Rollback { files, delete_added: self.delete_added })
    }
}

// ---------------------------------------------------------------------------------------------
// Remotes

/// Why `name` cannot be a remote's name; `existing`: the other remotes' names.
pub fn validate_remote_name(name: &str, existing: &[String]) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Enter a remote name".to_owned());
    }
    let malformed = name.contains(char::is_whitespace)
        || name.starts_with('-')
        || name.contains(['~', '^', ':', '?', '*', '[', '\\'])
        || name.contains("..");
    if malformed {
        return Some("Invalid remote name".to_owned());
    }
    existing.iter().any(|other| other == name).then(|| format!("Remote '{name}' already exists"))
}

/// Git Remotes: the repository's remotes, with Add, Remove and Edit, and the last edit's failure.
pub struct RemotesForm {
    remotes: Vec<Remote>,
    selected: Option<String>,
    error: Option<String>,
}

impl RemotesForm {
    /// The list of `remotes`; `selected` stays selected while it is listed, else the first is.
    pub fn new(remotes: Vec<Remote>, selected: Option<String>, error: Option<String>) -> Self {
        let selected = selected
            .filter(|name| remotes.iter().any(|remote| &remote.name == name))
            .or_else(|| remotes.first().map(|remote| remote.name.clone()));
        Self { remotes, selected, error }
    }

    fn current(&self) -> Option<&Remote> {
        self.remotes.iter().find(|remote| Some(&remote.name) == self.selected.as_ref())
    }
}

impl GitForm for RemotesForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let rows = self
            .remotes
            .iter()
            .map(|remote| ListRow {
                key: remote.name.clone(),
                label: remote.name.clone(),
                detail: redacted_url(&remote.url).into_owned(),
                depth: 0,
                checked: None,
            })
            .collect();
        let mut form_rows =
            vec![FormRow::List { id: "remotes", rows, selected: self.selected.clone(), empty: i18n.tx("No remotes") }];
        form_rows.extend(self.error.clone().map(FormRow::Error));
        let some = self.current().is_some();
        FormSpec {
            title: i18n.tx("Git Remotes"),
            width: 560.0,
            rows: form_rows,
            buttons: vec![
                FormButton::toolbar("add", i18n.tx("Add Remote…"), true),
                FormButton::toolbar("remove", i18n.tx("Remove Remote"), some),
                FormButton::toolbar("edit", i18n.tx("Edit Remote…"), some),
                FormButton::footer(OK, i18n.tx("OK"), true, true),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        if let ("remotes", FormInput::Choose(name)) = (id, input) {
            self.selected = Some(name);
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let command = match button {
            "add" => RemotesCommand::Add,
            "remove" => RemotesCommand::Remove(self.current()?.name.clone()),
            "edit" => RemotesCommand::Edit(self.current()?.clone()),
            OK => RemotesCommand::Close,
            _ => return None,
        };
        Some(FormAnswer::Remotes(command))
    }

    fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }
}

/// Define Remote (a new one) or Edit Remote: its name and URL.
pub struct RemoteForm {
    original: Option<Remote>,
    existing: Vec<String>,
    name: String,
    url: String,
}

impl RemoteForm {
    /// The form for `remote` (`None`: a new one); `names`: every remote's name.
    pub fn new(remote: Option<Remote>, names: &[String]) -> Self {
        let existing: Vec<String> =
            names.iter().filter(|name| remote.as_ref().is_none_or(|remote| &remote.name != *name)).cloned().collect();
        let name = match &remote {
            Some(remote) => remote.name.clone(),
            None if existing.iter().any(|name| name == "origin") => String::new(),
            None => "origin".to_owned(),
        };
        // The field shows the URL without its password; left as shown, the URL keeps it.
        let url = remote.as_ref().map(|remote| redacted_url(&remote.url).into_owned()).unwrap_or_default();
        Self { original: remote, existing, name, url }
    }

    pub fn problem(&self) -> Option<String> {
        validate_remote_name(&self.name, &self.existing)
            .or_else(|| self.url.trim().is_empty().then(|| "Enter the remote URL".to_owned()))
    }
}

impl GitForm for RemoteForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let problem = self.problem();
        let mut rows = vec![
            FormRow::Label(i18n.tx("Name:")),
            text_row("name", self.name.clone(), String::new(), false),
            FormRow::Label(i18n.tx("URL:")),
            text_row("url", self.url.clone(), String::new(), true),
        ];
        if !self.name.is_empty() || !self.url.is_empty() {
            rows.extend(problem.clone().map(|problem| FormRow::Error(i18n.tx(&problem))));
        }
        let title = if self.original.is_some() { "Edit Remote" } else { "Define Remote" };
        FormSpec {
            title: i18n.tx(title),
            width: 460.0,
            rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("OK"), true, problem.is_none()), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("name", FormInput::Text(text)) => self.name = text,
            ("url", FormInput::Text(text)) => self.url = text,
            _ => {}
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        if button != OK || self.problem().is_some() {
            return None;
        }
        let (name, url) = (self.name.trim().to_owned(), self.url.trim().to_owned());
        let edit = match &self.original {
            Some(original) => {
                let url = keep_credentials(&url, &original.url);
                RemoteEdit::Edit { name: original.name.clone(), new_name: name, url }
            }
            None => RemoteEdit::Add { name, url },
        };
        Some(FormAnswer::EditRemote(edit))
    }
}

// ---------------------------------------------------------------------------------------------
// Worktrees

/// Default folder of a new worktree: next to the repository, `<repo>-<branch>`.
pub fn default_worktree_path(repo_root: &str, branch: &str) -> String {
    let sep = if repo_root.contains('\\') && !repo_root.contains('/') { '\\' } else { '/' };
    let trimmed = repo_root.trim_end_matches(['/', '\\']);
    let cut = trimmed.rfind(['/', '\\']).filter(|cut| *cut > 0);
    let (parent, name) = match cut {
        Some(cut) => (&trimmed[..cut], &trimmed[cut + 1..]),
        None => (trimmed, trimmed.trim_start_matches(['/', '\\'])),
    };
    let suffix = branch.replace(['/', '\\'], "-");
    let suffix = if suffix.is_empty() { "worktree" } else { suffix.as_str() };
    format!("{parent}{sep}{name}-{suffix}")
}

fn worktree_path(worktree: &Worktree) -> String {
    worktree.path.0.to_string_lossy().into_owned()
}

/// A repository of the Worktrees dialog: its worktrees, as listed from one of its roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeGroup {
    pub list: RepoWorktrees,
    /// The repository's name, as the version control views show it.
    pub name: String,
}

/// Worktrees: the worktrees of every repository of the project as one tree, with Refresh, New,
/// Remove and Open in New Window.
pub struct WorktreesForm {
    groups: Vec<WorktreeGroup>,
    /// The repository roots the window works in.
    roots: Vec<String>,
    project_root: Option<String>,
    /// The selected repository.
    repo: Option<String>,
    selected: Option<String>,
    error: Option<String>,
}

impl WorktreesForm {
    /// The `groups` of the project's repositories; `roots`: the repository roots the window works in,
    /// `repo`: the selected one.
    pub fn new(
        groups: Vec<WorktreeGroup>,
        roots: Vec<String>,
        project_root: Option<String>,
        repo: Option<String>,
    ) -> Self {
        Self { groups, roots, project_root, repo, selected: None, error: None }
    }

    /// The selected worktree and the repository it belongs to.
    fn current(&self) -> Option<(&WorktreeGroup, &Worktree)> {
        let key = self.selected.as_deref()?.strip_prefix("w:")?;
        self.groups.iter().find_map(|group| {
            let worktree = group.list.worktrees.iter().find(|worktree| worktree_path(worktree) == key)?;
            Some((group, worktree))
        })
    }

    fn is_current(&self, path: &str) -> bool {
        self.roots.iter().any(|root| is_current_worktree(path, Some(root)))
    }

    fn contains_project(&self) -> bool {
        self.current().is_some_and(|(_, worktree)| {
            worktree_contains_project(&worktree_path(worktree), self.project_root.as_deref())
        })
    }

    fn detail(&self, worktree: &Worktree, i18n: &I18n) -> String {
        let path = worktree_path(worktree);
        let flag = if self.is_current(&path) {
            i18n.tx("current")
        } else if worktree.main {
            "main".to_owned()
        } else if worktree.locked {
            i18n.tx("locked")
        } else if worktree.prunable {
            i18n.tx("prunable")
        } else {
            String::new()
        };
        if flag.is_empty() { path } else { format!("{flag}  {path}") }
    }
}

impl GitForm for WorktreesForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let mut rows = Vec::new();
        for group in &self.groups {
            let main = group.list.worktrees.iter().find(|worktree| worktree.main).map(worktree_path);
            rows.push(ListRow {
                key: format!("r:{}", main.unwrap_or_else(|| group.list.root.clone())),
                label: group.name.clone(),
                detail: String::new(),
                depth: 0,
                checked: None,
            });
            for worktree in &group.list.worktrees {
                let label = match (&worktree.branch, &worktree.head) {
                    (Some(branch), _) => branch.clone(),
                    (None, Some(head)) => head.chars().take(8).collect(),
                    (None, None) => i18n.tx("(bare)"),
                };
                rows.push(ListRow {
                    key: format!("w:{}", worktree_path(worktree)),
                    label,
                    detail: self.detail(worktree, i18n),
                    depth: 1,
                    checked: None,
                });
            }
        }
        let mut form_rows =
            vec![FormRow::List { id: "worktrees", rows, selected: self.selected.clone(), empty: String::new() }];
        form_rows.extend(self.groups.iter().filter_map(|group| group.list.error.clone()).map(FormRow::Error));
        form_rows.extend(self.error.clone().map(FormRow::Error));
        let current = self.current();
        let contains = self.contains_project();
        let removable = current.is_some_and(|(_, worktree)| !worktree.main) && !contains;
        let remove_tip =
            if contains { "Cannot remove a worktree containing the open project." } else { "Remove Worktree" };
        FormSpec {
            title: i18n.tx("Worktrees"),
            width: 620.0,
            rows: form_rows,
            buttons: vec![
                FormButton::toolbar("refresh", i18n.tx("Refresh"), true),
                FormButton::toolbar("new", i18n.tx("New Worktree…"), true),
                FormButton {
                    tooltip: Some(i18n.tx(remove_tip)),
                    ..FormButton::toolbar("remove", i18n.tx("Remove Worktree"), removable)
                },
                FormButton::toolbar("open", i18n.tx("Open in New Window"), current.is_some()),
                FormButton::footer("close", i18n.tx("Close"), true, true),
            ],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        if let ("worktrees", FormInput::Choose(key)) = (id, input) {
            self.selected = Some(key);
        }
    }

    fn press(&self, button: &str, _: &mut I18n) -> Option<FormAnswer> {
        let current = self.current();
        let command = match button {
            "refresh" => WorktreesCommand::Refresh,
            "new" => {
                let repo = current.map(|(group, _)| group.list.root.clone()).or_else(|| self.repo.clone());
                WorktreesCommand::New { repo: repo.or_else(|| self.roots.first().cloned()) }
            }
            "remove" => {
                let (group, worktree) = current.filter(|(_, worktree)| !worktree.main)?;
                if self.contains_project() {
                    return None;
                }
                WorktreesCommand::Remove { root: group.list.root.clone(), path: worktree_path(worktree) }
            }
            "open" => WorktreesCommand::Open(worktree_path(current?.1)),
            "close" => WorktreesCommand::Close,
            _ => return None,
        };
        Some(FormAnswer::Worktrees(command))
    }

    fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }
}

/// New Worktree: what to check out, a new branch for it (suggested from it), and the folder.
pub struct NewWorktreeForm {
    root: String,
    branches: Vec<Branch>,
    source: Option<String>,
    create_branch: bool,
    new_branch: String,
    path: Option<String>,
    error: Option<String>,
}

impl NewWorktreeForm {
    /// The form for the repository at `root` with its `branches`.
    pub fn new(root: String, branches: Vec<Branch>) -> Self {
        Self { root, branches, source: None, create_branch: true, new_branch: String::new(), path: None, error: None }
    }

    fn source_remote(&self) -> bool {
        let source = self.source.as_deref();
        self.branches.iter().find(|branch| Some(branch.name.as_str()) == source).is_some_and(|branch| branch.remote)
    }

    /// The source without its remote (`origin/x` → `x`).
    fn short_source(&self) -> &str {
        let source = self.source.as_deref().unwrap_or_default();
        if self.source_remote() { source.split_once('/').map_or(source, |(_, rest)| rest) } else { source }
    }

    /// The new branch's name when none is typed: a remote branch's local name, else `<name>-wt`.
    pub fn suggested_branch(&self) -> String {
        match &self.source {
            None => String::new(),
            Some(_) if self.source_remote() => self.short_source().to_owned(),
            Some(_) => format!("{}-wt", self.short_source()),
        }
    }

    fn branch_name(&self) -> String {
        let typed = self.new_branch.trim();
        if typed.is_empty() { self.suggested_branch() } else { typed.to_owned() }
    }

    /// The folder: the one typed, else next to the repository after the branch.
    pub fn target(&self) -> String {
        if let Some(path) = &self.path {
            return path.clone();
        }
        if self.source.is_none() {
            return String::new();
        }
        let name = if self.create_branch { self.branch_name() } else { self.short_source().to_owned() };
        default_worktree_path(&self.root, &name)
    }

    fn problem(&self, i18n: &mut I18n) -> Option<String> {
        if self.source.is_none() {
            return Some(i18n.tx("Choose what to check out"));
        }
        if self.create_branch {
            return validate_branch_name(&self.branch_name(), false, &local_names(&self.branches), i18n)
                .map(|problem| i18n.tx(&problem));
        }
        self.target().trim().is_empty().then(|| i18n.tx("Enter a folder"))
    }
}

impl GitForm for NewWorktreeForm {
    fn spec(&self, i18n: &mut I18n) -> FormSpec {
        let branches = sortable(&self.branches);
        let mut options = branch_choices(sort_local(&branches, None));
        options.extend(branch_choices(sort_remote(&branches, None)));
        let mut rows = vec![
            FormRow::Label(i18n.tx("Check out:")),
            FormRow::Select {
                id: "source",
                value: self.source.clone(),
                placeholder: i18n.tx("Select branch"),
                options,
            },
            FormRow::Check {
                id: "createBranch",
                label: i18n.tx("Create a new branch"),
                on: self.create_branch,
                enabled: true,
            },
        ];
        if self.create_branch {
            rows.push(FormRow::Text {
                id: "newBranch",
                value: self.new_branch.clone(),
                placeholder: self.suggested_branch(),
                mono: true,
                invalid: false,
            });
        }
        rows.push(FormRow::Label(i18n.tx("Folder:")));
        rows.push(text_row("folder", self.target(), String::new(), true));
        let problem = self.problem(i18n);
        let shown = self.error.clone().or_else(|| problem.clone().filter(|_| self.source.is_some()));
        rows.extend(shown.map(FormRow::Error));
        FormSpec {
            title: i18n.tx("New Worktree"),
            width: 520.0,
            rows,
            buttons: vec![FormButton::footer(OK, i18n.tx("Create"), true, problem.is_none()), FormButton::cancel(i18n)],
        }
    }

    fn edit(&mut self, id: &str, input: FormInput) {
        match (id, input) {
            ("source", FormInput::Choose(source)) => self.source = Some(source),
            ("createBranch", FormInput::Toggle) => self.create_branch = !self.create_branch,
            ("newBranch", FormInput::Text(text)) => self.new_branch = clean_up_branch_name_on_typing(&text),
            ("folder", FormInput::Text(text)) => {
                // The field shows the suggested folder until the user types one of their own.
                if text != self.target() {
                    self.path = Some(text);
                }
            }
            _ => return,
        }
        self.error = None;
    }

    fn press(&self, button: &str, i18n: &mut I18n) -> Option<FormAnswer> {
        if button != OK || self.problem(i18n).is_some() {
            return None;
        }
        Some(FormAnswer::NewWorktree {
            path: self.target().trim().to_owned(),
            source: self.source.clone()?,
            new_branch: self.create_branch.then(|| self.branch_name()),
        })
    }

    fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }
}

/// Whether `spec` has `button` and lets it be pressed.
pub fn can_press(spec: &FormSpec, button: &str) -> bool {
    spec.button(button).is_some_and(|button| button.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Not from a spec: a form's debug output never holds a remote URL being typed, nor its credentials.
    #[test]
    fn the_debug_output_of_a_form_never_holds_a_typed_url_or_its_credentials() {
        let url = "https://user:secret@host/x.git";
        let spec = FormSpec {
            title: "Define Remote".to_owned(),
            width: 420.0,
            rows: vec![
                FormRow::Hint(url.to_owned()),
                FormRow::Label(url.to_owned()),
                FormRow::Text {
                    id: "url",
                    value: url.to_owned(),
                    placeholder: url.to_owned(),
                    mono: true,
                    invalid: true,
                },
                FormRow::Select {
                    id: "remote",
                    value: Some(url.to_owned()),
                    placeholder: String::new(),
                    options: vec![Choice::new(url, url, Some(url.to_owned()))],
                },
                FormRow::Check { id: "fetch", label: url.to_owned(), on: true, enabled: true },
                FormRow::Radio { id: "mode", value: url.to_owned(), options: vec![Choice::new(url, url, None)] },
                FormRow::List {
                    id: "remotes",
                    rows: vec![ListRow {
                        key: url.to_owned(),
                        label: url.to_owned(),
                        detail: url.to_owned(),
                        depth: 0,
                        checked: None,
                    }],
                    selected: Some(url.to_owned()),
                    empty: url.to_owned(),
                },
                FormRow::Error(url.to_owned()),
            ],
            buttons: vec![FormButton::footer(OK, "OK".to_owned(), true, true)],
        };
        let inputs = [
            FormInput::Text(url.to_owned()),
            FormInput::Choose(url.to_owned()),
            FormInput::ToggleRow(url.to_owned()),
            FormInput::Toggle,
        ];
        let debug = format!("{spec:?} {inputs:?} {:?}", spec.rows[2]);
        assert!(!debug.contains("secret"), "{debug}");
        assert!(!debug.contains(url) && !debug.contains("host/x.git"), "{debug}");
        assert!(debug.contains("\"url\"") && debug.contains("Text") && debug.contains(REDACTED), "{debug}");
    }

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        let wire = json!({
            "name": name, "remote": remote, "current": current, "ahead": 0, "behind": 0, "lastCommitMs": 0,
        });
        serde_json::from_value(wire).expect("a branch in its wire form")
    }

    fn worktree(path: &str, branch: &str, main: bool) -> Worktree {
        let wire = json!({
            "path": path, "branch": branch, "head": "12345678", "main": main, "locked": false, "prunable": false,
        });
        serde_json::from_value(wire).expect("a worktree in its wire form")
    }

    /// The Worktrees form of the spec: `/work/app` (main) and `/work/app-feature`, listed from each
    /// of `roots`, with the project at `project`.
    fn worktrees_form(roots: &[&str], project: &str) -> WorktreesForm {
        let worktrees = vec![worktree("/work/app", "main", true), worktree("/work/app-feature", "feature", false)];
        let lists: Vec<RepoWorktrees> = roots
            .iter()
            .map(|root| RepoWorktrees { root: (*root).to_owned(), worktrees: worktrees.clone(), error: None })
            .collect();
        let groups = crate::vcs_more::worktrees::group_worktrees(&lists)
            .into_iter()
            .map(|list| WorktreeGroup { name: base_name(&list.root).to_owned(), list })
            .collect();
        let roots = roots.iter().map(|root| (*root).to_owned()).collect();
        WorktreesForm::new(groups, roots, Some(project.to_owned()), None)
    }

    fn list_rows(spec: &FormSpec) -> Vec<ListRow> {
        spec.rows
            .iter()
            .find_map(|row| match row {
                FormRow::List { rows, .. } => Some(rows.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn detail_of(spec: &FormSpec, key: &str) -> Option<String> {
        list_rows(spec).into_iter().find(|row| row.key == key).map(|row| row.detail)
    }

    // src/features/vcs/__tests__/worktreesPanel.test.tsx › marks the linked project root current when its main worktree is also listed
    #[test]
    fn marks_the_linked_project_root_current_when_its_main_worktree_is_also_listed() {
        let form = worktrees_form(&["/work/app-feature"], "/work/app-feature");
        let spec = form.spec(&mut I18n::default());
        assert_eq!(detail_of(&spec, "w:/work/app-feature").as_deref(), Some("current  /work/app-feature"));
        assert_eq!(detail_of(&spec, "w:/work/app").as_deref(), Some("main  /work/app"));
    }

    // src/features/vcs/__tests__/worktreesPanel.test.tsx › marks both opened roots current while grouping their repository once
    #[test]
    fn marks_both_opened_roots_current_while_grouping_their_repository_once() {
        let form = worktrees_form(&["/work/app-feature", "/work/app"], "/work/app-feature");
        let spec = form.spec(&mut I18n::default());
        assert_eq!(detail_of(&spec, "w:/work/app-feature").as_deref(), Some("current  /work/app-feature"));
        assert_eq!(detail_of(&spec, "w:/work/app").as_deref(), Some("current  /work/app"));
        assert_eq!(list_rows(&spec).len(), 3, "one repository row and its two worktrees");
    }

    // src/features/vcs/__tests__/worktreesPanel.test.tsx › disables removal of the worktree containing project %s
    #[test]
    fn disables_removal_of_the_worktree_containing_project() {
        for project in ["/work/app-feature", "/work/app-feature/src"] {
            let mut form = worktrees_form(&["/work/app-feature"], project);
            form.edit("worktrees", FormInput::Choose("w:/work/app-feature".to_owned()));
            let spec = form.spec(&mut I18n::default());
            let remove = spec.button("remove").expect("Remove Worktree is offered");
            assert!(!remove.enabled, "{project}");
            assert_eq!(remove.tooltip.as_deref(), Some("Cannot remove a worktree containing the open project."));
            assert!(form.press("remove", &mut I18n::default()).is_none());
        }
    }

    // src/features/vcs/__tests__/worktreesPanel.test.tsx › allows removal when project %s is outside the worktree
    #[test]
    fn allows_removal_when_project_is_outside_the_worktree() {
        for project in ["/work", "/work/app-feature-sibling"] {
            let mut form = worktrees_form(&["/work/app-feature"], project);
            form.edit("worktrees", FormInput::Choose("w:/work/app-feature".to_owned()));
            let spec = form.spec(&mut I18n::default());
            assert!(spec.button("remove").is_some_and(|remove| remove.enabled), "{project}");
            let Some(FormAnswer::Worktrees(WorktreesCommand::Remove { path, .. })) =
                form.press("remove", &mut I18n::default())
            else {
                panic!("Remove asks to remove the worktree");
            };
            assert_eq!(path, "/work/app-feature");
        }
    }

    // src/features/vcs/__tests__/worktreesPanel.test.tsx › keeps the main worktree protected
    #[test]
    fn keeps_the_main_worktree_protected() {
        let mut form = worktrees_form(&["/work/app-feature"], "/work");
        form.edit("worktrees", FormInput::Choose("w:/work/app".to_owned()));
        let spec = form.spec(&mut I18n::default());
        assert!(spec.button("remove").is_some_and(|remove| !remove.enabled));
        assert!(spec.button("open").is_some_and(|open| open.enabled));
    }

    // Not from a spec: the merge flags git refuses together cannot be checked together, and
    // `--rebase` turns them all off.
    #[test]
    fn clashing_merge_flags_cannot_be_checked_together() {
        let on: BTreeSet<MergeFlag> = [MergeFlag::FfOnly].into_iter().collect();
        let disabled = disabled_merge_flags(&on, false);
        assert_eq!(disabled, [MergeFlag::NoFf, MergeFlag::Squash, MergeFlag::NoCommit].into_iter().collect());
        assert_eq!(disabled_merge_flags(&BTreeSet::new(), true).len(), 4);
        let mut flags = on;
        toggle_flag(&mut flags, MergeFlag::NoFf, false);
        assert!(!flags.contains(&MergeFlag::NoFf), "--no-ff is refused next to --ff-only");
        toggle_flag(&mut flags, MergeFlag::FfOnly, false);
        toggle_flag(&mut flags, MergeFlag::NoFf, false);
        let options = merge_options_of(&flags, Some("  "));
        assert!(options.no_ff && !options.ff_only && options.message.is_none());
        assert_eq!(merge_options_of(&flags, Some(" Merge it ")).message.as_deref(), Some("Merge it"));
    }

    // Not from a spec: Pull picks the upstream's remote and branch, else the current branch's
    // namesake, else the remote's first; a branch chosen that the remote lacks is ignored.
    #[test]
    fn pull_defaults_to_the_upstream_and_says_what_it_pulls() {
        let info: BranchInfo = serde_json::from_value(json!({
            "head": "main", "headCommit": "c1", "upstream": "fork/dev", "ahead": 0, "behind": 0,
        }))
        .expect("branch info in its wire form");
        let remotes = vec![
            Remote { name: "origin".to_owned(), url: "/o.git".to_owned() },
            Remote { name: "fork".to_owned(), url: "/f.git".to_owned() },
        ];
        let branches = vec![
            branch("main", false, true),
            branch("origin/HEAD", true, false),
            branch("origin/main", true, false),
            branch("fork/dev", true, false),
            branch("fork/main", true, false),
        ];
        let mut form = PullForm::new(Some(&info), remotes, branches);
        assert_eq!(form.branch().as_deref(), Some("dev"));
        assert_eq!(form.detail(), "main ← fork/dev");
        form.edit("remote", FormInput::Choose("origin".to_owned()));
        assert_eq!(form.branch().as_deref(), Some("main"), "the current branch's namesake");
        form.edit("branch", FormInput::Choose("dev".to_owned()));
        assert_eq!(form.branch().as_deref(), Some("main"), "origin has no dev");
        form.edit("rebase", FormInput::Toggle);
        form.edit("squash", FormInput::Toggle);
        let Some(FormAnswer::Pull { remote, rebase, options, detail, .. }) = form.press(OK, &mut I18n::default())
        else {
            panic!("Pull answers");
        };
        assert_eq!((remote.as_str(), rebase, options.squash), ("origin", true, false));
        assert_eq!(detail, "main ← origin/main");
        assert_eq!(remote_branches_of(&form.branches, "origin"), ["main"]);
        let empty = PullForm::new(Some(&info), Vec::new(), Vec::new());
        let spec = empty.spec(&mut I18n::default());
        assert!(!can_press(&spec, OK), "nothing to pull from");
    }

    // Not from a spec: Merge and Rebase offer the other local branches, then the remote ones, and
    // can only be pressed once a branch is chosen.
    #[test]
    fn merge_and_rebase_offer_the_other_branches() {
        let branches =
            vec![branch("main", false, true), branch("topic", false, false), branch("origin/main", true, false)];
        let mut merge = MergeForm::new(Some("main".to_owned()), &branches);
        let values: Vec<String> = merge.options.iter().map(|choice| choice.value.clone()).collect();
        assert_eq!(values, ["topic", "origin/main"]);
        assert!(!can_press(&merge.spec(&mut I18n::default()), OK));
        merge.edit("branch", FormInput::Choose("topic".to_owned()));
        merge.edit("withMessage", FormInput::Toggle);
        merge.edit("message", FormInput::Text("Bring topic".to_owned()));
        let Some(FormAnswer::Merge { branch, options }) = merge.press(OK, &mut I18n::default()) else {
            panic!("Merge answers");
        };
        assert_eq!((branch.as_str(), options.message.as_deref()), ("topic", Some("Bring topic")));

        let mut rebase = RebaseForm::new(Some("main".to_owned()), &branches);
        rebase.edit("upstream", FormInput::Choose("topic".to_owned()));
        rebase.edit("onto", FormInput::Toggle);
        assert!(rebase.press(OK, &mut I18n::default()).is_none(), "--onto needs a new base");
        rebase.edit("newBase", FormInput::Choose("origin/main".to_owned()));
        let Some(FormAnswer::Rebase { upstream, options }) = rebase.press(OK, &mut I18n::default()) else {
            panic!("Rebase answers");
        };
        assert_eq!((upstream.as_str(), options.onto.as_deref()), ("topic", Some("origin/main")));
    }

    // Not from a spec: a remote's name is checked as git would, and Define Remote suggests origin.
    #[test]
    fn remote_names_are_checked_and_origin_is_suggested() {
        let taken = ["origin".to_owned()];
        assert_eq!(validate_remote_name(" ", &[]).as_deref(), Some("Enter a remote name"));
        assert_eq!(validate_remote_name("my fork", &[]).as_deref(), Some("Invalid remote name"));
        assert_eq!(validate_remote_name("-x", &[]).as_deref(), Some("Invalid remote name"));
        assert_eq!(validate_remote_name("a..b", &[]).as_deref(), Some("Invalid remote name"));
        assert_eq!(validate_remote_name("origin", &taken).as_deref(), Some("Remote 'origin' already exists"));
        assert_eq!(validate_remote_name("fork", &taken), None);
        let new = RemoteForm::new(None, &[]);
        assert_eq!(new.name, "origin");
        assert_eq!(new.problem().as_deref(), Some("Enter the remote URL"));
        let edit = RemoteForm::new(Some(Remote { name: "origin".to_owned(), url: "/o".to_owned() }), &taken);
        assert_eq!(edit.problem(), None, "a remote keeps its own name");
    }

    // Not from a spec: the default folder of a new worktree sits next to the repository.
    #[test]
    fn a_new_worktree_goes_next_to_the_repository() {
        assert_eq!(default_worktree_path("/work/app", "feature/x"), "/work/app-feature-x");
        assert_eq!(default_worktree_path("/work/app/", ""), "/work/app-worktree");
        assert_eq!(default_worktree_path("C:\\work\\app", "x"), "C:\\work\\app-x");
        let branches = vec![branch("main", false, true), branch("origin/dev", true, false)];
        let mut form = NewWorktreeForm::new("/work/app".to_owned(), branches);
        assert!(form.press(OK, &mut I18n::default()).is_none(), "nothing is chosen to check out");
        form.edit("source", FormInput::Choose("origin/dev".to_owned()));
        assert_eq!((form.suggested_branch().as_str(), form.target().as_str()), ("dev", "/work/app-dev"));
        form.edit("source", FormInput::Choose("main".to_owned()));
        assert_eq!(form.target(), "/work/app-main-wt");
        form.edit("newBranch", FormInput::Text("my next".to_owned()));
        assert_eq!(form.target(), "/work/app-my-next");
        form.edit("folder", FormInput::Text("/tmp/wt".to_owned()));
        let Some(FormAnswer::NewWorktree { path, source, new_branch }) = form.press(OK, &mut I18n::default()) else {
            panic!("Create answers");
        };
        assert_eq!((path.as_str(), source.as_str(), new_branch.as_deref()), ("/tmp/wt", "main", Some("my-next")));
    }

    // Not from a spec: Unstash's button says what it does, As new branch implies Pop and
    // Reinstate index.
    #[test]
    fn unstash_names_what_it_does() {
        let stash = |index: u32, message: &str| Stash { index, message: message.to_owned(), time_ms: 0.0 };
        let mut form = UnstashForm::new(vec![stash(0, "WIP on main"), stash(1, "older")], 0);
        let label = |form: &UnstashForm| form.spec(&mut I18n::default()).button(OK).map(|b| b.label.clone());
        assert_eq!(label(&form).as_deref(), Some("Apply Stash"));
        form.edit("pop", FormInput::Toggle);
        assert_eq!(label(&form).as_deref(), Some("Pop Stash"));
        form.edit("stashes", FormInput::Choose("1".to_owned()));
        form.edit("branch", FormInput::Text("from-stash".to_owned()));
        assert_eq!(label(&form).as_deref(), Some("Create Branch"));
        let Some(FormAnswer::Unstash(options)) = form.press(OK, &mut I18n::default()) else { panic!("it answers") };
        assert_eq!((options.index, options.branch.as_deref()), (1, Some("from-stash")));
        assert!(UnstashForm::new(Vec::new(), 0).press(OK, &mut I18n::default()).is_none(), "no stashes");
    }

    // Not from a spec: Git Reset of a log commit names the commit instead of asking for it, starts on
    // Mixed and answers the commit's full hash with the chosen mode; Reset HEAD… asks for the commit.
    #[test]
    fn git_reset_names_the_log_commit_or_asks_for_one() {
        let mut i18n = I18n::default();
        let mut form = ResetForm::on_commit(&"a".repeat(40), "aaaaaaaa", "Fix the parser");
        let spec = form.spec(&mut i18n);
        assert_eq!(spec.title, "Git Reset");
        assert!(!spec.rows.iter().any(|row| matches!(row, FormRow::Text { .. })), "no field for the commit");
        let hint = spec.rows.iter().find_map(|row| match row {
            FormRow::Hint(text) => Some(text.clone()),
            _ => None,
        });
        assert!(hint.is_some_and(|hint| hint.ends_with("\naaaaaaaa Fix the parser")));
        assert!(spec.rows.iter().any(|row| matches!(row, FormRow::Radio { value, .. } if value == "mixed")));
        form.edit("mode", FormInput::Choose("hard".to_owned()));
        let Some(FormAnswer::Reset { commit, mode }) = form.press(OK, &mut i18n) else { panic!("Reset answers") };
        assert_eq!((commit, mode), ("a".repeat(40), ResetMode::Hard));

        let mut form = ResetForm::default();
        assert!(form.spec(&mut i18n).rows.iter().any(|row| matches!(row, FormRow::Text { id: "target", .. })));
        form.edit("target", FormInput::Text("  ".to_owned()));
        assert!(!can_press(&form.spec(&mut i18n), OK), "a blank commit cannot be reset to");
    }

    // Not from a spec: New Tag… of a log commit asks for the name only, refuses a tag in use and
    // answers a lightweight tag on the commit.
    #[test]
    fn new_tag_of_a_log_commit_asks_for_the_name_only() {
        let mut i18n = I18n::default();
        let prompt = PromptRequest {
            title: "Create New Tag On 1234abcd".to_owned(),
            label: Some("Enter the name of new tag:".to_owned()),
            ok_label: "Create".to_owned(),
            mono: true,
            ..PromptRequest::default()
        };
        let mut form = TagForm::on_commit(prompt, "1234abcd5678", vec!["v1".to_owned()]);
        let spec = form.spec(&mut i18n);
        assert_eq!(spec.title, "Create New Tag On 1234abcd");
        assert_eq!(spec.button(OK).map(|button| button.label.as_str()), Some("Create"));
        let ids: Vec<&str> = spec
            .rows
            .iter()
            .filter_map(|row| match row {
                FormRow::Text { id, .. } | FormRow::Check { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["name"], "neither Force, Commit nor Message");
        form.edit("name", FormInput::Text("v1".to_owned()));
        assert!(form.spec(&mut i18n).rows.contains(&FormRow::Error("Tag 'v1' already exists".to_owned())));
        assert!(form.press(OK, &mut i18n).is_none(), "a tag in use is refused");
        form.edit("name", FormInput::Text("v2".to_owned()));
        let Some(FormAnswer::Tag { name, commit, message, force }) = form.press(OK, &mut i18n) else {
            panic!("Create answers")
        };
        assert_eq!((name.as_str(), commit.as_str(), message, force), ("v2", "1234abcd5678", None, false));
    }

    // Not from a spec: a remote's password is never shown, in the list, in Pull's remote choice or in
    // Edit Remote's field, and Edit Remote left as shown keeps it.
    #[test]
    fn a_remotes_password_is_never_shown_and_survives_an_edit() {
        assert_eq!(redacted_url("https://ada:s3cret@example.test/r.git"), "https://ada:***@example.test/r.git");
        assert_eq!(redacted_url("https://ghp_t0ken@example.test/r.git"), "https://***@example.test/r.git");
        assert_eq!(redacted_url("ssh://git@example.test/r.git"), "ssh://git@example.test/r.git");
        assert_eq!(redacted_url("git@example.test:r.git"), "git@example.test:r.git");
        assert_eq!(redacted_url("/srv/r.git"), "/srv/r.git");
        let url = "https://ada:s3cret@example.test/r.git";
        let origin = Remote { name: "origin".to_owned(), url: url.to_owned() };
        let mut i18n = I18n::default();
        let shown = format!("{:?}", RemotesForm::new(vec![origin.clone()], None, None).spec(&mut i18n));
        assert!(!shown.contains("s3cret"), "{shown}");
        let pull = PullForm::new(None, vec![origin.clone()], Vec::new());
        let shown = format!("{:?}", pull.spec(&mut i18n));
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(!format!("{:?}", RemotesCommand::Edit(origin.clone())).contains("s3cret"));
        let mut edit = RemoteForm::new(Some(origin.clone()), &["origin".to_owned()]);
        let shown = format!("{:?}", edit.spec(&mut i18n));
        assert!(!shown.contains("s3cret"), "{shown}");
        let Some(FormAnswer::EditRemote(RemoteEdit::Edit { url: kept, .. })) = edit.press(OK, &mut i18n) else {
            panic!("OK answers")
        };
        assert_eq!(kept, url, "the URL left as shown keeps its password");
        // Another host or path with the mask left in place keeps the password too; a new user info
        // replaces it.
        edit.edit("url", FormInput::Text("https://ada:***@example.test/moved.git".to_owned()));
        let Some(FormAnswer::EditRemote(RemoteEdit::Edit { url: moved, .. })) = edit.press(OK, &mut i18n) else {
            panic!("OK answers")
        };
        assert_eq!(moved, "https://ada:s3cret@example.test/moved.git");
        edit.edit("url", FormInput::Text("https://bob:other@example.test/r.git".to_owned()));
        let Some(FormAnswer::EditRemote(RemoteEdit::Edit { url: replaced, .. })) = edit.press(OK, &mut i18n) else {
            panic!("OK answers")
        };
        assert_eq!(replaced, "https://bob:other@example.test/r.git");
        edit.edit("url", FormInput::Text("https://example.test/other.git".to_owned()));
        let Some(FormAnswer::EditRemote(RemoteEdit::Edit { url: typed, .. })) = edit.press(OK, &mut i18n) else {
            panic!("OK answers")
        };
        assert_eq!(typed, "https://example.test/other.git");
    }
}
