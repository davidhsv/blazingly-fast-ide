//! Git operations of the menus that have no dialog of their own: a tag's menu and the
//! notification that follows its deletion, Create Git Repository's notifications, Stash Selected
//! Files' notice when nothing is selected, and where Move Worktree puts a worktree.

use super::menu::MenuItem;
use super::ops::{Notice, vcs_notice};
use crate::platform::i18n::{I18n, Param};
use crate::vcs_ops::dialogs::ConfirmRequest;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_vcs::DeletedTag;

/// The key of the "Deleted tag" notification of a tag starts so; its buttons' commands are
/// `<key>:<button>`.
pub const DELETED_TAG_PREFIX: &str = "vcs.deletedTag.";

/// What choosing a row of a tag's menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagCommand {
    /// Checks the tag out, the HEAD detached.
    Checkout(String),
    /// New Branch from the tag.
    NewBranch(String),
    /// Compare with Current.
    Compare(String),
    ShowDiffWithWorkingTree(String),
    /// The Git log of the tag.
    ShowHistory(String),
    /// Pushes the tag to `remote`.
    Push {
        tag: String,
        remote: String,
    },
    Delete(String),
}

/// The menu of tag `tag` in a repository whose remotes are `remotes` (as its remote-tracking
/// branches name them): Checkout and New Branch, the comparisons and the history, a Push row per
/// remote, then Delete.
pub fn tag_menu(tag: &str, remotes: &[String], i18n: &mut I18n) -> Vec<MenuItem<TagCommand>> {
    let owned = || tag.to_owned();
    let new_branch = i18n.tx_built(&["New Branch from '", "'…"], &[Param::from(tag)]);
    let mut items = vec![
        MenuItem::item("Checkout", TagCommand::Checkout(owned())),
        MenuItem::item(new_branch, TagCommand::NewBranch(owned())).icon("branch"),
        MenuItem::Separator,
        MenuItem::item("Compare with Current", TagCommand::Compare(owned())).icon("diff"),
        MenuItem::item("Show Diff with Working Tree", TagCommand::ShowDiffWithWorkingTree(owned())),
        MenuItem::item("Show History", TagCommand::ShowHistory(owned())).icon("history"),
    ];
    for (at, remote) in remotes.iter().enumerate() {
        if at == 0 {
            items.push(MenuItem::Separator);
        }
        let label = i18n.tx_built(&["Push to ", ""], &[Param::from(remote.as_str())]);
        items.push(MenuItem::item(label, TagCommand::Push { tag: owned(), remote: remote.clone() }).icon("git-push"));
    }
    items.push(MenuItem::Separator);
    items.push(MenuItem::item("Delete", TagCommand::Delete(owned())).icon("trash").danger());
    items
}

/// The title of the operation that deletes tag `name`.
pub fn delete_tag_title(name: &str, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Delete tag ", ""], &[Param::from(name)])
}

/// What a button of the "Deleted tag" notification does. `root` is the project the tag was deleted
/// in: a window that opened another project since does nothing with the button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagFollowUp {
    /// Recreates the tag where it pointed, as an operation titled `title` that reports `success`.
    Restore { root: Option<String>, repo: Option<String>, deleted: DeletedTag, title: String, success: String },
    /// Deletes the tag on each of `remotes` in turn, stopping at the first that fails.
    DeleteOnRemotes { root: Option<String>, repo: Option<String>, tag: String, remotes: Vec<String> },
}

impl TagFollowUp {
    /// The project the follow-up belongs to.
    pub fn root(&self) -> Option<&str> {
        match self {
            Self::Restore { root, .. } | Self::DeleteOnRemotes { root, .. } => root.as_deref(),
        }
    }
}

/// The notification that follows the deletion of tag `name` in `repo` (of the project at `root`):
/// Restore, and with remotes known, Delete on Remote (Delete on Remotes for several).
pub fn deleted_tag_notice(
    name: &str,
    root: Option<&str>,
    repo: Option<&str>,
    deleted: &DeletedTag,
    remotes: &[String],
    i18n: &mut I18n,
) -> Notice<TagFollowUp> {
    let key = format!("{DELETED_TAG_PREFIX}{name}");
    let (root, repo) = (root.map(str::to_owned), repo.map(str::to_owned));
    let title = i18n.tx_built(&["Deleted tag ", ""], &[Param::from(name)]);
    let mut request = vcs_notice(NotificationLevel::Info, title);
    request.key = Some(key.clone());
    let restore = TagFollowUp::Restore {
        root: root.clone(),
        repo: repo.clone(),
        deleted: deleted.clone(),
        title: i18n.tx_built(&["Restore tag ", ""], &[Param::from(name)]),
        success: i18n.tx_built(&["Restored tag ", ""], &[Param::from(name)]),
    };
    let mut notice = Notice::new(request).with_action("Restore", &format!("{key}:restore"), restore);
    if !remotes.is_empty() {
        let label = if remotes.len() == 1 { "Delete on Remote" } else { "Delete on Remotes" };
        let delete = TagFollowUp::DeleteOnRemotes { root, repo, tag: name.to_owned(), remotes: remotes.to_vec() };
        notice = notice.with_action(label, &format!("{key}:deleteOnRemote"), delete);
    }
    notice
}

/// The notification key a button's command `command` belongs to (`<key>:<button>`).
pub fn notice_key_of(command: &str) -> Option<&str> {
    command.rsplit_once(':').map(|(key, _)| key).filter(|key| key.starts_with(DELETED_TAG_PREFIX))
}

/// What the Git console and the progress say of deleting `tag` on `remote`.
pub fn remote_tag_detail(remote: &str, tag: &str) -> String {
    format!("{remote}: {tag}")
}

/// What the Git console and the progress say of pushing `tag` to `remote`.
pub fn push_tag_detail(tag: &str, remote: &str) -> String {
    format!("{tag} → {remote}")
}

/// What Create Git Repository reports: the repository made in `dir`, or why it could not be.
pub fn create_repository_notice(dir: &str, failure: Option<&str>, i18n: &mut I18n) -> NotificationRequest {
    match failure {
        Some(message) => {
            let mut request = vcs_notice(NotificationLevel::Error, i18n.tx("Couldn't create Git repository"));
            request.body = Some(message.to_owned());
            request
        }
        None => {
            let title = i18n.tx_built(&["Created Git repository in ", ""], &[Param::from(dir)]);
            vcs_notice(NotificationLevel::Info, title)
        }
    }
}

/// Stash Selected Files… with nothing selected.
pub fn nothing_to_stash_notice(i18n: &I18n) -> NotificationRequest {
    vcs_notice(NotificationLevel::Info, i18n.tx("Select the files to stash in the Commit view first"))
}

/// The last component of `path`, separators of either kind, a trailing one ignored.
fn last_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// Where Move Worktree puts the worktree at `path` when the folder `folder` is chosen: the folder
/// itself when it already bears the worktree's name, else a folder of that name inside it, joined
/// with the separator the chosen folder uses.
pub fn worktree_move_target(path: &str, folder: &str) -> String {
    let name = last_name(path);
    let trimmed = folder.trim_end_matches(['/', '\\']);
    let target = if trimmed.is_empty() { folder } else { trimmed };
    if last_name(target) == name {
        return target.to_owned();
    }
    let separator = if target.contains('\\') && !target.contains('/') { '\\' } else { '/' };
    if target.ends_with(separator) { format!("{target}{name}") } else { format!("{target}{separator}{name}") }
}

/// The question before moving the worktree at `from` to `to`.
pub fn move_worktree_confirm(from: &str, to: &str, i18n: &mut I18n) -> ConfirmRequest {
    let message = i18n.tx_built(&["Move the worktree at ", " to ", "?"], &[Param::from(from), Param::from(to)]);
    ConfirmRequest { title: "Move Worktree".to_owned(), message, ok_label: "Move".to_owned(), danger: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[MenuItem<TagCommand>]) -> Vec<&str> {
        items.iter().map(|item| item.label().unwrap_or("-")).collect()
    }

    // Not from a spec: a tag's rows, with a Push row per known remote.
    #[test]
    fn lists_the_rows_of_a_tag_with_a_push_row_per_remote() {
        let mut i18n = I18n::default();
        let menu = tag_menu("v1", &[], &mut i18n);
        assert_eq!(
            labels(&menu),
            [
                "Checkout",
                "New Branch from 'v1'…",
                "-",
                "Compare with Current",
                "Show Diff with Working Tree",
                "Show History",
                "-",
                "Delete"
            ]
        );
        let delete = menu.last().and_then(MenuItem::command);
        assert_eq!(delete, Some(&TagCommand::Delete("v1".to_owned())));

        let remotes = ["origin".to_owned(), "fork".to_owned()];
        let menu = tag_menu("v1", &remotes, &mut i18n);
        let shown = labels(&menu);
        assert_eq!(shown[6..], ["-", "Push to origin", "Push to fork", "-", "Delete"]);
        let push = menu.iter().find(|item| item.label() == Some("Push to fork")).and_then(MenuItem::command);
        assert_eq!(push, Some(&TagCommand::Push { tag: "v1".to_owned(), remote: "fork".to_owned() }));
    }

    // Not from a spec: the notification offers Restore, and Delete on Remote(s) only with remotes.
    #[test]
    fn the_deleted_tag_notice_offers_restore_and_deletes_on_the_remotes_it_knows() {
        let mut i18n = I18n::default();
        let deleted = DeletedTag { name: "v1".to_owned(), target: "abc".to_owned() };
        let notice = deleted_tag_notice("v1", Some("/p"), None, &deleted, &[], &mut i18n);
        assert_eq!(notice.request.title, "Deleted tag v1");
        assert_eq!(notice.request.key.as_deref(), Some("vcs.deletedTag.v1"));
        let buttons: Vec<&str> = notice.request.actions.iter().map(|action| action.label.as_str()).collect();
        assert_eq!(buttons, ["Restore"]);
        let Some(TagFollowUp::Restore { title, success, deleted: kept, root, .. }) = notice.follow_up_of("Restore")
        else {
            panic!("Restore restores")
        };
        assert_eq!((title.as_str(), success.as_str()), ("Restore tag v1", "Restored tag v1"));
        assert_eq!((kept, root.as_deref()), (&deleted, Some("/p")));

        let one = deleted_tag_notice("v1", None, None, &deleted, &["origin".to_owned()], &mut i18n);
        assert!(one.follow_up_of("Delete on Remote").is_some());
        let two = ["origin".to_owned(), "fork".to_owned()];
        let several = deleted_tag_notice("v1", None, None, &deleted, &two, &mut i18n);
        let Some(TagFollowUp::DeleteOnRemotes { remotes, .. }) = several.follow_up_of("Delete on Remotes") else {
            panic!("Delete on Remotes deletes on both")
        };
        assert_eq!(remotes, &two);
        assert_eq!(notice_key_of("vcs.deletedTag.v1:restore"), Some("vcs.deletedTag.v1"));
        assert_eq!(notice_key_of("vcs.deleted.v1:restore"), None, "a branch's notice is not a tag's");
    }

    // Not from a spec: a chosen folder bearing the worktree's name is the target, else the worktree
    // goes inside it.
    #[test]
    fn moves_a_worktree_into_the_folder_chosen_unless_it_bears_its_name() {
        assert_eq!(worktree_move_target("/w/feature", "/elsewhere"), "/elsewhere/feature");
        assert_eq!(worktree_move_target("/w/feature/", "/elsewhere/"), "/elsewhere/feature");
        assert_eq!(worktree_move_target("/w/feature", "/elsewhere/feature"), "/elsewhere/feature");
        assert_eq!(worktree_move_target("C:\\w\\feature", "D:\\trees"), "D:\\trees\\feature");
        let mut i18n = I18n::default();
        let confirm = move_worktree_confirm("/w/feature", "/elsewhere/feature", &mut i18n);
        assert_eq!(confirm.message, "Move the worktree at /w/feature to /elsewhere/feature?");
        assert!(!confirm.danger);
    }

    // Not from a spec: Create Git Repository says where it made the repository, or why it could not.
    #[test]
    fn create_repository_reports_the_folder_or_the_failure() {
        let mut i18n = I18n::default();
        let made = create_repository_notice("/p", None, &mut i18n);
        assert_eq!((made.title.as_str(), made.level), ("Created Git repository in /p", NotificationLevel::Info));
        let failed = create_repository_notice("/p", Some("denied"), &mut i18n);
        assert_eq!(failed.title, "Couldn't create Git repository");
        assert_eq!((failed.body.as_deref(), failed.level), (Some("denied"), NotificationLevel::Error));
    }
}
