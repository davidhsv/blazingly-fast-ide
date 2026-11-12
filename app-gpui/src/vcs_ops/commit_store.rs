//! Commit tool window state shared with actions: the message draft, Amend, the inclusion
//! checkboxes and the selection.
//!
//! The message field is pre-filled, unless something was typed, with the message git prepared for a
//! merge / cherry-pick / revert that stopped, otherwise with the repository's commit template.
//! Reading those files is the caller's: [`CommitState::vcs_changed`] and the like say what to read
//! ([`MessageLoad`]) and `apply_*` take the text.

use super::commit_checks::CommitProblem;
use crate::vcs::changes::CheckState;
use ide_vcs::Commit;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CommitTab {
    #[default]
    Changes,
    Shelf,
    Stashes,
}

impl CommitTab {
    pub fn id(self) -> &'static str {
        match self {
            Self::Changes => "changes",
            Self::Shelf => "shelf",
            Self::Stashes => "stashes",
        }
    }
}

/// UI storage keys of the changes tree's view options.
pub const GROUP_BY_DIR_KEY: &str = "openidea.vcs.groupByDir";
pub const GROUP_BY_STAGING_KEY: &str = "openidea.vcs.groupByStaging";
pub const DIFF_PREVIEW_KEY: &str = "openidea.vcs.diffPreview";
pub const COMMIT_DIFF_PREVIEW_KEY: &str = "openidea.vcs.commitDiffPreview";

/// A stored view option: `fallback` when nothing is stored.
pub fn stored_flag(stored: Option<&str>, fallback: bool) -> bool {
    stored.map_or(fallback, |value| value == "true")
}

#[derive(Clone, Debug)]
pub struct CommitState {
    pub message: String,
    pub amend: bool,
    pub sign_off: bool,
    /// Commit Options › Author (`Name <mail>`); blank uses the configured user.
    pub author: String,
    /// Commit Options › Run Git hooks.
    pub run_hooks: bool,
    pub group_by_dir: bool,
    /// Group the changes into Staged / Unstaged instead of one "Changes" group.
    pub group_by_staging: bool,
    /// Whether the Git window's Local Changes tab previews the selection's diff.
    pub diff_preview: bool,
    /// Whether the Commit tool window previews the selection's diff beside the changes.
    pub commit_diff_preview: bool,
    /// User overrides of the inclusion checkboxes (absolute path → state).
    pub checks: HashMap<String, CheckState>,
    /// Changed blocks left out of the commit (absolute path → block keys) of partially included files.
    pub excluded_hunks: HashMap<String, BTreeSet<String>>,
    /// Absolute paths selected in the changes tree.
    pub selection: Vec<String>,
    /// Keys of the selected tree nodes: which staging side of a partially staged file.
    pub selection_keys: Vec<String>,
    /// Bumped to focus the message field.
    pub focus_tick: u64,
    /// Bumped to open the commit message history popup.
    pub history_tick: u64,
    /// Failed before-commit checks; while any are shown, Commit means "Commit Anyway".
    pub check_problems: Vec<CommitProblem>,
    pub tab: CommitTab,
    /// Change list of the selected tree node (list actions without an argument).
    pub selected_list: Option<String>,
    /// Selected shelf tree key.
    pub selected_shelf: Option<String>,
    /// Commit a Fixup… / Squash Into… message was prepared for: offers Commit and Rebase.
    pub fixup_target: Option<Commit>,
    /// The message typed before Amend replaced it, and the amended commit's message.
    amend_draft: Option<(String, String)>,
    /// The message a commit left in the field (not typed since).
    kept_message: Option<String>,
}

impl Default for CommitState {
    fn default() -> Self {
        Self {
            message: String::new(),
            amend: false,
            sign_off: false,
            author: String::new(),
            run_hooks: true,
            group_by_dir: false,
            group_by_staging: false,
            diff_preview: true,
            commit_diff_preview: false,
            checks: HashMap::new(),
            excluded_hunks: HashMap::new(),
            selection: Vec::new(),
            selection_keys: Vec::new(),
            focus_tick: 0,
            history_tick: 0,
            check_problems: Vec::new(),
            tab: CommitTab::Changes,
            selected_list: None,
            selected_shelf: None,
            fixup_target: None,
            amend_draft: None,
            kept_message: None,
        }
    }
}

/// The Fixup… / Squash Into… target while the message still is the one prepared for it.
pub fn pending_fixup<'a>(message: &str, target: Option<&'a Commit>) -> Option<&'a Commit> {
    target.filter(|target| {
        message.starts_with(&format!("fixup! {}", target.summary))
            || message.starts_with(&format!("squash! {}", target.summary))
    })
}

/// How many commit messages the history keeps.
const MAX_HISTORY: usize = 20;

/// UI storage key of the commit message history of the project at `root`.
pub fn history_key(root: Option<&str>) -> String {
    format!("openidea.vcs.commitMessages:{}", root.unwrap_or_default())
}

/// The stored commit message history, newest first; empty when nothing usable is stored.
pub fn parse_history(stored: Option<&str>) -> Vec<String> {
    let parsed = stored.and_then(|raw| serde_json::from_str::<Vec<serde_json::Value>>(raw).ok()).unwrap_or_default();
    parsed.into_iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()
}

/// The history with `message` put first; `None` for a blank message (nothing to remember).
pub fn remembered(history: &[String], message: &str) -> Option<Vec<String>> {
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    let rest = history.iter().filter(|known| known.as_str() != message).cloned();
    Some(std::iter::once(message.to_owned()).chain(rest).take(MAX_HISTORY).collect())
}

/// The message field after a commit: the committed message stays for the next commit unless
/// Settings › Version Control › Commit › "Clear initial commit message" is on.
pub fn message_after_commit(committed: &str, clear: bool) -> String {
    if clear { String::new() } else { committed.trim().to_owned() }
}

/// Operations whose commit message git prepares in `.git/MERGE_MSG`.
fn prepares_message(operation: Option<&str>) -> bool {
    matches!(operation, Some("merge" | "cherry-pick" | "revert"))
}

/// `MERGE_MSG` or a commit template without git's `#` comment lines (the conflict list etc.).
pub fn prepared_message_text(raw: &str) -> String {
    raw.split('\n').filter(|line| !line.starts_with('#')).collect::<Vec<_>>().join("\n").trim().to_owned()
}

/// `commit.template` resolved as git does: `~/` in the home directory, a relative path in the
/// repository.
pub fn template_path(value: &str, root: &str, home: &str) -> String {
    if value == "~" || value.starts_with("~/") {
        let home = home.strip_suffix(['/', '\\']).unwrap_or(home);
        return format!("{home}{}", &value[1..]);
    }
    let drive = matches!(value.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic());
    if value.starts_with('/') || drive { value.to_owned() } else { format!("{root}/{value}") }
}

/// What to read to pre-fill the message field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageLoad {
    None,
    /// The message git prepared, in the file at `path`: pass its text to
    /// [`CommitState::apply_prepared`].
    Prepared {
        path: String,
    },
    /// The commit template of the repository at `root`: read its `commit.template` setting, resolve
    /// it with [`template_path`] and pass the file's text to [`CommitState::apply_template`].
    Template {
        root: String,
    },
}

/// What the message pre-fill reads of the repository state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageContext<'a> {
    pub root: Option<&'a str>,
    /// The operation the selected repository is stopped in.
    pub operation: Option<&'a str>,
}

impl CommitState {
    /// Changes a view option; returns the UI storage entry to write.
    pub fn set_group_by_dir(&mut self, on: bool) -> (&'static str, String) {
        self.group_by_dir = on;
        (GROUP_BY_DIR_KEY, on.to_string())
    }

    pub fn set_group_by_staging(&mut self, on: bool) -> (&'static str, String) {
        self.group_by_staging = on;
        (GROUP_BY_STAGING_KEY, on.to_string())
    }

    pub fn set_diff_preview(&mut self, on: bool) -> (&'static str, String) {
        self.diff_preview = on;
        (DIFF_PREVIEW_KEY, on.to_string())
    }

    pub fn set_commit_diff_preview(&mut self, on: bool) -> (&'static str, String) {
        self.commit_diff_preview = on;
        (COMMIT_DIFF_PREVIEW_KEY, on.to_string())
    }

    /// Amend checkbox. Unchecking restores the draft Amend replaced, unless the amend message was
    /// edited meanwhile. Checking returns `true`: the caller reads the last commit's message and
    /// passes it to [`CommitState::amend_loaded`].
    pub fn set_amend(&mut self, on: bool) -> bool {
        self.amend = on;
        if on {
            return true;
        }
        if let Some((before, amend)) = self.amend_draft.take()
            && self.message == amend
        {
            self.message = before;
        }
        false
    }

    /// The last commit's message arrived (`None`: no commits yet): it replaces the draft, which
    /// is returned to be kept in the message history.
    pub fn amend_loaded(&mut self, last: Option<&str>) -> Option<String> {
        let last = last.map(str::trim_end).filter(|last| !last.is_empty())?;
        if !self.amend || self.message.trim() == last.trim() {
            return None;
        }
        let before = std::mem::replace(&mut self.message, last.to_owned());
        self.amend_draft = Some((before.clone(), last.to_owned()));
        self.focus_tick += 1;
        Some(before)
    }

    /// After a commit: there is no draft to restore.
    pub fn forget_amend_draft(&mut self) {
        self.amend_draft = None;
    }

    /// Leaves the field as a commit leaves it (see [`message_after_commit`]). Returns what to read
    /// for a field left empty.
    pub fn keep_message_after_commit(&mut self, committed: &str, clear: bool, root: Option<&str>) -> MessageLoad {
        let message = message_after_commit(committed, clear);
        self.kept_message = (!message.is_empty()).then(|| message.clone());
        self.message = message;
        if self.message.is_empty() { self.template_load(root) } else { MessageLoad::None }
    }

    /// The field holds nothing the user typed: empty, or what the last commit left there.
    fn message_replaceable(&self) -> bool {
        let message = self.message.trim();
        message.is_empty() || Some(message) == self.kept_message.as_deref()
    }

    fn template_load(&self, root: Option<&str>) -> MessageLoad {
        match root.filter(|root| !root.is_empty()) {
            Some(root) if self.message.trim().is_empty() => MessageLoad::Template { root: root.to_owned() },
            _ => MessageLoad::None,
        }
    }

    fn message_load(&self, context: MessageContext) -> MessageLoad {
        if !prepares_message(context.operation) {
            return self.template_load(context.root);
        }
        match context.root.filter(|root| !root.is_empty()) {
            Some(root) if self.message_replaceable() => {
                MessageLoad::Prepared { path: format!("{root}/.git/MERGE_MSG") }
            }
            _ => MessageLoad::None,
        }
    }

    /// What to read when the Commit view comes up.
    pub fn initial_message_load(&self, context: MessageContext) -> MessageLoad {
        self.message_load(context)
    }

    /// Follows the repository state. Another repository: nothing typed or ticked for the previous
    /// one carries over (Amend least of all). An operation that prepares a message started: that
    /// message is read.
    pub fn vcs_changed(&mut self, prev: MessageContext, now: MessageContext) -> MessageLoad {
        if now.root != prev.root {
            self.amend_draft = None;
            self.kept_message = None;
            self.message.clear();
            self.amend = false;
            self.checks.clear();
            self.excluded_hunks.clear();
            self.selection.clear();
            self.selection_keys.clear();
            self.tab = CommitTab::Changes;
            self.selected_list = None;
            self.selected_shelf = None;
            return self.message_load(now);
        }
        if now.operation != prev.operation && prepares_message(now.operation) {
            return self.message_load(now);
        }
        MessageLoad::None
    }

    /// The text of the file git prepared its message in arrived. Returns whether it was put in
    /// the field.
    pub fn apply_prepared(&mut self, raw: &str) -> bool {
        let text = prepared_message_text(raw);
        let apply = !text.is_empty() && self.message_replaceable();
        if apply {
            self.message = text;
        }
        apply
    }

    /// The text of the commit template of the repository at `root` arrived; `root_now` is the
    /// project's root by then. Returns whether it was put in the field.
    pub fn apply_template(&mut self, raw: &str, root: &str, root_now: Option<&str>) -> bool {
        let text = prepared_message_text(raw);
        if text.is_empty() || root_now != Some(root) || !self.message.trim().is_empty() {
            return false;
        }
        // Not typed: a prepared merge message may replace it.
        self.kept_message = Some(text.clone());
        self.message = text;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = "feat: \n\n# Why:\nRefs: \n";

    fn at(root: &'static str, operation: Option<&'static str>) -> MessageContext<'static> {
        MessageContext { root: Some(root), operation }
    }

    fn template(root: &str) -> MessageLoad {
        MessageLoad::Template { root: root.to_owned() }
    }

    // src/features/vcs/__tests__/commitMode.test.ts › commit message after a commit › keeps the committed message unless Clear initial commit message is on
    #[test]
    fn keeps_the_committed_message_unless_clear_initial_commit_message_is_on() {
        assert_eq!(message_after_commit("  fix: thing \n", false), "fix: thing");
        assert_eq!(message_after_commit("fix: thing", true), "");
    }

    // src/features/vcs/__tests__/commitMode.test.ts › commit message after a commit › lets a prepared merge message replace a kept one, but not a typed one
    #[test]
    fn lets_a_prepared_merge_message_replace_a_kept_one_but_not_a_typed_one() {
        let mut state = CommitState::default();
        assert_eq!(state.initial_message_load(at("/p", None)), template("/p"));
        assert_eq!(state.keep_message_after_commit("fix: thing", false, Some("/p")), MessageLoad::None);
        let load = state.vcs_changed(at("/p", None), at("/p", Some("merge")));
        assert_eq!(load, MessageLoad::Prepared { path: "/p/.git/MERGE_MSG".to_owned() });
        assert!(state.apply_prepared("Merge branch 'x'\n# Conflicts:\n"));
        assert_eq!(state.message, "Merge branch 'x'");
        assert_eq!(state.vcs_changed(at("/p", Some("merge")), at("/p", None)), MessageLoad::None);

        state.message = "typed".to_owned();
        assert_eq!(state.vcs_changed(at("/p", None), at("/p", Some("merge"))), MessageLoad::None);
        assert!(!state.apply_prepared("Merge branch 'x'\n"));
        assert_eq!(state.message, "typed");
    }

    // src/features/vcs/__tests__/commitMode.test.ts › commit template › resolves the configured path as git does
    #[test]
    fn resolves_the_configured_path_as_git_does() {
        assert_eq!(template_path("~/.gitmessage", "/p", "/home/me/"), "/home/me/.gitmessage");
        assert_eq!(template_path("/etc/msg", "/p", ""), "/etc/msg");
        assert_eq!(template_path("C:\\msg.txt", "/p", ""), "C:\\msg.txt");
        assert_eq!(template_path(".gitmessage", "/p", ""), "/p/.gitmessage");
    }

    // src/features/vcs/__tests__/commitMode.test.ts › commit template › fills the empty message field, and again after a commit clears it
    #[test]
    fn fills_the_empty_message_field_and_again_after_a_commit_clears_it() {
        let mut state = CommitState::default();
        assert_eq!(state.initial_message_load(at("/p", None)), template("/p"));
        assert_eq!(template_path(".gitmessage", "/p", ""), "/p/.gitmessage");
        assert!(state.apply_template(TEMPLATE, "/p", Some("/p")));
        assert_eq!(state.message, "feat: \n\nRefs:");

        assert_eq!(state.keep_message_after_commit("feat: x", true, Some("/p")), template("/p"));
        assert!(state.apply_template(TEMPLATE, "/p", Some("/p")));
        assert_eq!(state.message, "feat: \n\nRefs:");
        // A kept message stays; a typed one is never replaced.
        assert_eq!(state.keep_message_after_commit("feat: x", false, Some("/p")), MessageLoad::None);
        state.message = "typed".to_owned();
        assert_eq!(state.vcs_changed(at("/p", None), at("/q", None)), template("/q"));
        state.message = "typed".to_owned();
        assert!(!state.apply_template(TEMPLATE, "/q", Some("/q")));
        assert_eq!(state.message, "typed");
    }

    // Not from a spec: Amend swaps the draft for the last commit's message and back.
    #[test]
    fn amend_replaces_the_draft_and_restores_it() {
        let mut state = CommitState { message: "draft".to_owned(), ..CommitState::default() };
        assert!(state.set_amend(true));
        assert_eq!(state.amend_loaded(Some("last commit\n")).as_deref(), Some("draft"));
        assert_eq!((state.message.as_str(), state.focus_tick), ("last commit", 1));
        assert!(!state.set_amend(false));
        assert_eq!(state.message, "draft");

        state.set_amend(true);
        state.amend_loaded(Some("last commit"));
        state.message = "last commit, edited".to_owned();
        state.set_amend(false);
        assert_eq!(state.message, "last commit, edited");
        assert_eq!(state.amend_loaded(Some("late")), None);
    }

    // Not from a spec: the message history keeps each message once, newest first.
    #[test]
    fn remembers_commit_messages_newest_first() {
        let history = parse_history(Some(r#"["a", 1, "b"]"#));
        assert_eq!(history, ["a", "b"]);
        assert_eq!(remembered(&history, " b "), Some(vec!["b".to_owned(), "a".to_owned()]));
        assert_eq!(remembered(&history, "  "), None);
        assert!(parse_history(Some("not json")).is_empty());
        assert_eq!(history_key(Some("/p")), "openidea.vcs.commitMessages:/p");
        assert!(stored_flag(None, true) && !stored_flag(Some("false"), true));
        let target = super::super::test_support::commit("a1", &[], "Add parser");
        assert!(pending_fixup("fixup! Add parser", Some(&target)).is_some());
        assert!(pending_fixup("something else", Some(&target)).is_none());
    }
}
