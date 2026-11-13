//! Version Control and its pages: Commit, Confirmation, Directory Mappings, Git, Shelf and Issue
//! Navigation.

use super::{custom, select_owned, toggle};
use crate::platform::misc::file_manager::show_in_file_manager_label;
use crate::platform::misc::platform_info::Os;
use crate::settings_model::page_spec::PageSpec;

fn commit() -> PageSpec {
    PageSpec::new("vcs.commit", "Commit", 1)
        .project_scope()
        .keywords(&["commit checks", "commit message", "modal commit", "commit dialog"])
        .section(
            None,
            vec![
                toggle("vcs.commit.nonModal", "Use non-modal commit interface"),
                toggle("vcs.commit.clearInitialMessage", "Clear initial commit message"),
            ],
        )
        .section(
            Some("Commit Message Inspections"),
            vec![
                toggle("vcs.commit.blankLineAfterSubject", "Blank line between subject and body"),
                custom(
                    "Limit subject line",
                    &["Limit body line", "Show right margin", "Wrap when typing reaches right margin"],
                ),
            ],
        )
        .section(
            Some("Commit Checks"),
            vec![custom(
                "Analyze code",
                &["Check TODO", "Reformat code", "Optimize imports", "Cleanup", "before commit"],
            )],
        )
}

fn confirmation() -> PageSpec {
    // The third choice names the system's file manager ("Show in Finder" on macOS).
    let reveal = show_in_file_manager_label(Os::from_platform(std::env::consts::OS));
    let patch_created = vec![("ask", "Ask"), ("no", "No"), ("reveal", reveal)];
    PageSpec::new("vcs.confirmation", "Confirmation", 2)
        .project_scope()
        .keywords(&["add to git", "remove from git", "ask"])
        .section(
            Some("Confirmation"),
            vec![
                custom("When files are created", &["Add silently", "Do not add"]),
                custom("When files are deleted", &["Remove silently", "Do not remove"]),
                toggle("vcs.confirmation.showUpdateOptions", "Show options before Update"),
                toggle("vcs.confirmation.confirmDropCommits", "Ask for confirmation to drop commits"),
            ],
        )
        .section(
            Some("Changes"),
            vec![
                toggle(
                    "vcs.showDirtyDirectories",
                    "Highlight directories that contain modified files in the Project tree",
                ),
                select_owned("vcs.confirmation.onPatchCreated", "When patch is created", patch_created),
                custom("Limit history to", &["rows", "file history"]),
            ],
        )
}

fn directory_mappings() -> PageSpec {
    PageSpec::new("vcs.mappings", "Directory Mappings", 3)
        .keywords(&["root", "repository", "vcs root"])
        .section(None, vec![custom("Directory", &["VCS", "Git"])])
}

fn git() -> PageSpec {
    PageSpec::new("vcs.git", "Git", 8)
        .project_scope()
        .keywords(&["git", "executable", "push", "pull", "update"])
        .section(
            None,
            vec![
                custom("Path to Git executable", &["Test", "git version"]),
                toggle("vcs.git.inlineBlame", "Show inline blame for the current line"),
            ],
        )
        .section(
            Some("Commit"),
            vec![
                toggle("vcs.git.warnCrlf", "Warn if CRLF line separators are about to be committed"),
                toggle("vcs.git.warnDetachedHead", "Warn when committing in detached HEAD or during rebase"),
                toggle("vcs.git.signOff", "Sign-off commit"),
                custom("Warn when committing files larger than", &["MB"]),
                toggle(
                    "vcs.git.warnBadFileNames",
                    "Warn when committing files with names that might cause issues on other systems",
                ),
            ],
        )
        .section(
            Some("Push"),
            vec![
                toggle("vcs.git.autoUpdateOnPushRejected", "Auto-update if push of the current branch was rejected"),
                custom(
                    "Show Push dialog for Commit and Push",
                    &["Show Push dialog only when committing to protected branches"],
                ),
                custom("Protected branches", &["force push"]),
            ],
        )
        .section(
            Some("Update"),
            vec![
                custom("Update method", &["Merge", "Rebase", "pull"]),
                custom("Explicitly check for incoming commits on remotes", &["Auto", "Always", "Never", "fetch"]),
            ],
        )
        .section(Some("Passwords"), vec![custom("Passwords", &["credentials", "remember", "forget", "https"])])
        .section(Some("Stash"), vec![toggle("vcs.git.combineStashAndShelf", "Combine stashes and shelves in one tab")])
}

fn shelf() -> PageSpec {
    PageSpec::new("vcs.shelf", "Shelf", 7)
        .project_scope()
        .keywords(&["shelve", "unshelve"])
        .section(None, vec![toggle("vcs.shelf.removeApplied", "Remove successfully applied files from the shelf")])
}

fn issue_navigation() -> PageSpec {
    PageSpec::new("vcs.issueNavigation", "Issue Navigation", 5)
        .project_scope()
        .keywords(&["issue", "tracker", "jira", "youtrack", "link"])
        .section(
            None,
            vec![
                custom("Issue ID", &["Issue Link", "regex"]),
                custom("Add JIRA Pattern", &["Add YouTrack Pattern", "GitHub"]),
            ],
        )
}

/// The pages in the order they are registered. Version Control itself has no options: it links
/// to its pages.
pub(super) fn pages() -> Vec<PageSpec> {
    vec![
        PageSpec::new("vcs", "Version Control", 300).keywords(&["git", "vcs", "history"]),
        commit(),
        confirmation(),
        directory_mappings(),
        git(),
        shelf(),
        issue_navigation(),
    ]
}
