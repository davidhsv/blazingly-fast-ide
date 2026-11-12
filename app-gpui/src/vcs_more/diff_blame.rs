//! What `git blame` annotates a side of a diff with.

use crate::vcs_diff::diff_input::DiffSide;

/// The blame of a diff side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffBlameTarget {
    /// The file at that side's revision (it may have been renamed since).
    pub path: String,
    /// Blame history; `None` for the working file, which follows HEAD and the edits, like the
    /// editor's annotations.
    pub revision: Option<String>,
}

/// The blame for one side of the diff of `path`: a revision blames its own history, the working
/// file the file on disk; Compare Files, Local History and plain texts are not Git content. The
/// index (`:0`) is not a revision `git blame` takes, so its text is blamed on HEAD and the lines
/// staged since show as not committed.
pub fn diff_blame_target(side: &DiffSide, path: &str, vcs_available: bool) -> Option<DiffBlameTarget> {
    if !vcs_available {
        return None;
    }
    match side {
        DiffSide::Rev { rev, path: own, .. } => Some(DiffBlameTarget {
            path: own.as_deref().unwrap_or(path).to_owned(),
            revision: Some(if rev.starts_with(':') { "HEAD".to_owned() } else { rev.clone() }),
        }),
        DiffSide::Worktree { .. } => Some(DiffBlameTarget { path: path.to_owned(), revision: None }),
        DiffSide::File { .. } | DiffSide::Text { .. } | DiffSide::History { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str, revision: Option<&str>) -> Option<DiffBlameTarget> {
        Some(DiffBlameTarget { path: path.to_owned(), revision: revision.map(str::to_owned) })
    }

    // src/features/vcs/diffBlame.test.ts › diffBlameTarget › blames a revision on its own history, at the path it had there
    #[test]
    fn blames_a_revision_on_its_own_history_at_the_path_it_had_there() {
        let parent = DiffSide::rev("abc123^");
        assert_eq!(diff_blame_target(&parent, "/p/A.java", true), target("/p/A.java", Some("abc123^")));
        let renamed = DiffSide::Rev { rev: "abc123".to_owned(), path: Some("/p/Old.java".to_owned()), label: None };
        assert_eq!(diff_blame_target(&renamed, "/p/A.java", true), target("/p/Old.java", Some("abc123")));
    }

    // src/features/vcs/diffBlame.test.ts › diffBlameTarget › blames the index on HEAD and the working file like the editor
    #[test]
    fn blames_the_index_on_head_and_the_working_file_like_the_editor() {
        assert_eq!(diff_blame_target(&DiffSide::rev(":0"), "/p/A.java", true), target("/p/A.java", Some("HEAD")));
        assert_eq!(diff_blame_target(&DiffSide::worktree(), "/p/A.java", true), target("/p/A.java", None));
    }

    // src/features/vcs/diffBlame.test.ts › diffBlameTarget › offers nothing for content outside Git
    #[test]
    fn offers_nothing_for_content_outside_git() {
        let outside = [
            DiffSide::File { path: "/p/B.java".to_owned(), label: None },
            DiffSide::text("x", "Clipboard"),
            DiffSide::History { id: "1".to_owned(), label: "Local History".to_owned() },
        ];
        for side in &outside {
            assert_eq!(diff_blame_target(side, "/p/A.java", true), None);
        }
        assert_eq!(diff_blame_target(&DiffSide::worktree(), "/p/A.java", false), None);
    }
}
