//! Speed search of the Git log: typing in the list finds the next commit whose visible columns hold
//! the typed words.

use crate::vcs::log_options::LogOptions;
use crate::vcs_ops::log_store::RepoCommit;

/// The text of the columns a commit's row shows. `time` formats a commit's time (author or
/// committer time, as the options say) the way the Date column does.
pub fn visible_commit_text(c: &RepoCommit, options: &LogOptions, time: impl Fn(f64) -> String) -> Vec<String> {
    let commit = &c.commit;
    let mut columns = vec![commit.summary.clone(), commit.refs.join(" ")];
    if options.show_repository {
        columns.push(c.repo.clone().unwrap_or_default());
    }
    if options.show_author {
        columns.push(commit.author.clone());
    }
    if options.show_date {
        let committed = options.commit_timestamp && commit.commit_time_ms != 0.0;
        columns.push(time(if committed { commit.commit_time_ms } else { commit.time_ms }));
    }
    if options.show_hash {
        columns.push(commit.short_id.clone());
    }
    columns
}

/// Whether every word of `query` is found in a column, its characters in order but not necessarily
/// next to each other, whatever their case. Columns never form a word together.
pub fn speed_matches<S: AsRef<str>>(query: &str, columns: &[S]) -> bool {
    let query = query.to_lowercase();
    let lowered: Vec<String> = columns.iter().map(|column| column.as_ref().to_lowercase()).collect();
    query.split_whitespace().all(|word| {
        lowered.iter().any(|column| {
            let mut wanted = word.chars().peekable();
            for c in column.chars() {
                if wanted.peek() == Some(&c) {
                    wanted.next();
                }
            }
            wanted.peek().is_none()
        })
    })
}

/// The index of the next commit after `from` matching `query`, wrapping around; backward when
/// `forward` is false. `None` when no commit matches.
pub fn next_speed_match(
    commits: &[RepoCommit],
    query: &str,
    options: &LogOptions,
    from: usize,
    forward: bool,
    time: impl Fn(f64) -> String,
) -> Option<usize> {
    let count = commits.len();
    (1..=count)
        .map(|step| if forward { (from + step) % count } else { (from % count + count - step % count) % count })
        .find(|index| speed_matches(query, &visible_commit_text(&commits[*index], options, &time)))
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::test_support::commit;
    use super::*;

    fn row(id: &str) -> RepoCommit {
        let mut c = commit(id, &[], &["main"]);
        c.summary = "Synchronize client".to_owned();
        c.message = "body-only".to_owned();
        c.author = "Unique author".to_owned();
        c.email = "hidden@example.com".to_owned();
        RepoCommit::from(c)
    }

    // src/features/vcs/__tests__/logSearch.test.ts › log speed search › matches fuzzy words within visible columns and cycles in both directions
    #[test]
    fn matches_fuzzy_words_within_visible_columns_and_cycles_in_both_directions() {
        let commits = [row("one"), row("two")];
        let options = LogOptions::default();
        let time = |_: f64| String::new();
        assert!(speed_matches("snc clt", &visible_commit_text(&commits[0], &options, time)));
        assert_eq!(next_speed_match(&commits, "client", &options, 0, true, time), Some(1));
        assert_eq!(next_speed_match(&commits, "client", &options, 1, true, time), Some(0));
        assert_eq!(next_speed_match(&commits, "client", &options, 0, false, time), Some(1));
        let no_author = LogOptions { show_author: false, ..LogOptions::default() };
        assert!(!speed_matches("Unique", &visible_commit_text(&commits[0], &no_author, time)));
        assert!(!speed_matches("body-only", &visible_commit_text(&commits[0], &options, time)));
    }
}
