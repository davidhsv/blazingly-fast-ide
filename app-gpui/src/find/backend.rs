//! Where the search models meet the backend: their queries and results converted to and from
//! `ide_search`'s, in one place, so a field added on either side fails to compile here.

use super::ipc::{FileMatches, LineMatch, SearchQuery, SearchScope, SearchSummary};
use crate::platform::project_roots::ProjectFileSet;
use crate::platform::search::SearchContext;
use ide_core::IpcPath;
use std::path::PathBuf;

fn path(text: &str) -> IpcPath {
    IpcPath(PathBuf::from(text))
}

/// The query as the backend takes it.
pub fn to_backend(query: &SearchQuery) -> ide_search::SearchQuery {
    let SearchQuery { pattern, is_regex, case_sensitive, whole_word, include, exclude, scope, max_matches, context } =
        query;
    ide_search::SearchQuery {
        pattern: pattern.clone(),
        is_regex: *is_regex,
        case_sensitive: *case_sensitive,
        whole_word: *whole_word,
        include: include.clone(),
        exclude: exclude.clone(),
        scope: match scope {
            SearchScope::Project => ide_search::SearchScope::Project,
            SearchScope::Directory { path: directory } => ide_search::SearchScope::Directory { path: path(directory) },
            SearchScope::Files { paths } => {
                ide_search::SearchScope::Files { paths: paths.iter().map(|file| path(file)).collect() }
            }
            SearchScope::FileSet { set } => ide_search::SearchScope::FileSet {
                set: match set {
                    ProjectFileSet::All => ide_search::ProjectFileSet::All,
                    ProjectFileSet::Production => ide_search::ProjectFileSet::Production,
                    ProjectFileSet::Test => ide_search::ProjectFileSet::Test,
                },
            },
            SearchScope::Pattern { pattern } => ide_search::SearchScope::Pattern { pattern: pattern.clone() },
        },
        max_matches: *max_matches,
        context: match context {
            SearchContext::Any => ide_search::SearchContext::Any,
            SearchContext::InComments => ide_search::SearchContext::InComments,
            SearchContext::InStringLiterals => ide_search::SearchContext::InStringLiterals,
            SearchContext::ExceptComments => ide_search::SearchContext::ExceptComments,
            SearchContext::ExceptStringLiterals => ide_search::SearchContext::ExceptStringLiterals,
            SearchContext::ExceptCommentsAndStringLiterals => {
                ide_search::SearchContext::ExceptCommentsAndStringLiterals
            }
        },
    }
}

/// A file's matches as the models hold them.
pub fn file_matches(file: ide_search::FileMatches) -> FileMatches {
    let ide_search::FileMatches { path, rel_path, matches } = file;
    FileMatches {
        path: path.0.to_string_lossy().into_owned(),
        rel_path,
        matches: matches
            .into_iter()
            .map(|ide_search::LineMatch { line, text, ranges, text_offset }| LineMatch {
                line,
                text,
                ranges,
                text_offset,
            })
            .collect(),
    }
}

/// How a search ended, as the models hold it.
pub fn summary(summary: ide_search::SearchSummary) -> SearchSummary {
    let ide_search::SearchSummary { files, matches, limit_hit, cancelled, elapsed_ms } = summary;
    SearchSummary { files, matches, limit_hit, cancelled, elapsed_ms }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_and_its_results_cross_the_edge_unchanged() {
        let query = SearchQuery {
            pattern: "needle".to_owned(),
            is_regex: true,
            case_sensitive: true,
            whole_word: true,
            include: vec!["*.rs".to_owned()],
            exclude: vec!["target".to_owned()],
            scope: SearchScope::Directory { path: "/p/src".to_owned() },
            max_matches: 7,
            context: SearchContext::ExceptComments,
        };
        let sent = to_backend(&query);
        assert_eq!(
            (sent.pattern.as_str(), sent.is_regex, sent.case_sensitive, sent.whole_word),
            ("needle", true, true, true)
        );
        assert_eq!(
            (sent.include, sent.exclude, sent.max_matches),
            (vec!["*.rs".to_owned()], vec!["target".to_owned()], 7)
        );
        assert!(
            matches!(sent.scope, ide_search::SearchScope::Directory { path } if path.0 == std::path::Path::new("/p/src"))
        );
        assert_eq!(sent.context, ide_search::SearchContext::ExceptComments);

        let found = file_matches(ide_search::FileMatches {
            path: path("/p/src/a.rs"),
            rel_path: "src/a.rs".to_owned(),
            matches: vec![ide_search::LineMatch {
                line: 3,
                text: "a needle".to_owned(),
                ranges: vec![(2, 8)],
                text_offset: 4,
            }],
        });
        assert_eq!((found.path.as_str(), found.rel_path.as_str()), ("/p/src/a.rs", "src/a.rs"));
        let line = LineMatch { line: 3, text: "a needle".to_owned(), ranges: vec![(2, 8)], text_offset: 4 };
        assert_eq!(found.matches, [line]);
        let ended = summary(ide_search::SearchSummary {
            files: 1,
            matches: 2,
            limit_hit: true,
            cancelled: false,
            elapsed_ms: 9,
        });
        assert_eq!(ended, SearchSummary { files: 1, matches: 2, limit_hit: true, cancelled: false, elapsed_ms: 9 });
    }
}
