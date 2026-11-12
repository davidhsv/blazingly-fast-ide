//! Create Patch: the file name the dialog suggests.

use ide_vcs::Commit;
use regex::Regex;
use std::sync::LazyLock;

/// What a patch is created from.
#[derive(Clone, Copy, Debug)]
pub enum PatchOrigin<'a> {
    /// Local changes.
    Changes,
    Commits(&'a [Commit]),
    /// A shelved change list, by its name.
    Shelf(&'a str),
}

/// Longest suggested name, in UTF-16 units, before the extension.
const MAX_NAME: usize = 60;

/// File name suggested for the patch: the change list (`list_name`) or commit subject, sanitised.
pub fn suggested_patch_name(origin: PatchOrigin, list_name: &str) -> String {
    /// Runs of characters a file name does not keep.
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}._\-]+").expect("valid pattern"));
    let base = match origin {
        PatchOrigin::Commits([only]) => only.summary.clone(),
        PatchOrigin::Commits(commits) => format!("{} commits", commits.len()),
        PatchOrigin::Shelf(name) => name.to_owned(),
        PatchOrigin::Changes => list_name.to_owned(),
    };
    let replaced = UNSAFE.replace_all(&base, "_");
    let mut clean = String::new();
    let mut units = 0;
    for c in replaced.trim_matches('_').chars() {
        units += c.len_utf16();
        if units > MAX_NAME {
            break;
        }
        clean.push(c);
    }
    format!("{}.patch", if clean.is_empty() { "changes" } else { clean.as_str() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(summary: &str) -> Commit {
        super::super::test_support::commit("0123456789abcdef0123456789abcdef01234567", &[], summary)
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › suggests patch file names from the change list or commit
    #[test]
    fn suggests_patch_file_names_from_the_change_list_or_commit() {
        assert_eq!(suggested_patch_name(PatchOrigin::Changes, "Changes"), "Changes.patch");
        let one = [commit("Fix: crash on start!")];
        assert_eq!(suggested_patch_name(PatchOrigin::Commits(&one), "x"), "Fix_crash_on_start.patch");
        let two = [commit("a"), commit("b")];
        assert_eq!(suggested_patch_name(PatchOrigin::Commits(&two), "x"), "2_commits.patch");
        assert_eq!(suggested_patch_name(PatchOrigin::Changes, "///"), "changes.patch");
    }

    // Not from a spec: a shelf's patch is named after the shelf, cut to sixty characters.
    #[test]
    fn names_a_shelf_patch_after_the_shelf() {
        assert_eq!(suggested_patch_name(PatchOrigin::Shelf("Wörk in progress"), "x"), "Wörk_in_progress.patch");
        let long = "a".repeat(80);
        assert_eq!(suggested_patch_name(PatchOrigin::Shelf(&long), "x"), format!("{}.patch", "a".repeat(60)));
    }
}
