//! Add to .gitignore / .git/info/exclude: the write to run and what follows it.
//!
//! Whatever the write came to, the caller refreshes the status and reloads the project tree
//! afterwards: a selection across several repositories may have been written in part.

use super::ops::vcs_notice;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_vcs::IgnoreDestination;

/// The ignore rules to write.
#[derive(Clone, Debug)]
pub struct IgnoreWrite {
    /// Title of the operation (English source text).
    pub title: &'static str,
    pub paths: Vec<String>,
    pub destination: IgnoreDestination,
}

/// The write that ignores `paths`; `None` for an empty selection.
pub fn ignore_write(paths: &[String], destination: IgnoreDestination) -> Option<IgnoreWrite> {
    let title = match destination {
        IgnoreDestination::Gitignore => "Add to .gitignore",
        IgnoreDestination::LocalExclude => "Add to .git/info/exclude",
    };
    (!paths.is_empty()).then(|| IgnoreWrite { title, paths: paths.to_vec(), destination })
}

/// What follows a write, besides the refresh.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IgnoreOutcome {
    pub notification: Option<NotificationRequest>,
    /// The ignore file to open in the editor, focused.
    pub open: Option<String>,
}

/// What follows the write of `destination`. `changed`: the files it wrote rules to, `None` when it
/// failed (the failure is reported by the operation itself).
pub fn ignore_written(destination: IgnoreDestination, changed: Option<&[String]>) -> IgnoreOutcome {
    match changed {
        // Existing rules already cover the selection (e.g. a `_*/` directory pattern).
        Some([]) => {
            IgnoreOutcome { notification: Some(vcs_notice(NotificationLevel::Info, "Already ignored")), open: None }
        }
        // Show the rule where it was written: the nearest .gitignore may be a nested one.
        Some([file, ..]) if matches!(destination, IgnoreDestination::Gitignore) && !file.is_empty() => {
            IgnoreOutcome { notification: None, open: Some(file.clone()) }
        }
        _ => IgnoreOutcome::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn routes_all_paths(destination: IgnoreDestination) {
        let paths = strings(&["/p/file", "/p/directory"]);
        let write = ignore_write(&paths, destination).unwrap();
        assert_eq!(write.paths, paths);
        let same = matches!(
            (write.destination, destination),
            (IgnoreDestination::Gitignore, IgnoreDestination::Gitignore)
                | (IgnoreDestination::LocalExclude, IgnoreDestination::LocalExclude)
        );
        assert!(same, "{:?}", write.destination);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › routes all paths to gitignore then refreshes status and tree
    #[test]
    fn routes_all_paths_to_gitignore() {
        routes_all_paths(IgnoreDestination::Gitignore);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › routes all paths to localExclude then refreshes status and tree
    #[test]
    fn routes_all_paths_to_local_exclude() {
        routes_all_paths(IgnoreDestination::LocalExclude);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › refreshes after a failed or partially completed multi-repository write
    #[test]
    fn a_failed_write_is_still_followed_by_the_refresh_only() {
        assert!(ignore_write(&strings(&["/p/file"]), IgnoreDestination::Gitignore).is_some());
        assert_eq!(ignore_written(IgnoreDestination::Gitignore, None), IgnoreOutcome::default());
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › ignores an empty selection
    #[test]
    fn ignores_an_empty_selection() {
        assert!(ignore_write(&[], IgnoreDestination::Gitignore).is_none());
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › opens the .gitignore the rules were written to, which may be a nested one
    #[test]
    fn opens_the_gitignore_the_rules_were_written_to_which_may_be_a_nested_one() {
        let outcome = ignore_written(IgnoreDestination::Gitignore, Some(&strings(&["/p/pkg/.gitignore"])));
        assert_eq!(outcome.open.as_deref(), Some("/p/pkg/.gitignore"));
        assert_eq!(outcome.notification, None);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › opens nothing for the local exclude file
    #[test]
    fn opens_nothing_for_the_local_exclude_file() {
        let outcome = ignore_written(IgnoreDestination::LocalExclude, Some(&strings(&["/p/.git/info/exclude"])));
        assert_eq!(outcome.open, None);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › reports an already ignored selection instead of opening an unchanged file
    #[test]
    fn reports_an_already_ignored_selection_instead_of_opening_an_unchanged_file() {
        let outcome = ignore_written(IgnoreDestination::Gitignore, Some(&[]));
        assert_eq!(outcome.notification.map(|notice| notice.title).as_deref(), Some("Already ignored"));
        assert_eq!(outcome.open, None);
    }

    // src/features/vcs/__tests__/ignore.test.ts › ignore action › stays quiet after a failure
    #[test]
    fn stays_quiet_after_a_failure() {
        assert_eq!(ignore_written(IgnoreDestination::Gitignore, None).notification, None);
    }
}
