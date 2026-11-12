//! Repository identity for the rows of a log that several repositories share: the same hash is a
//! different commit in each of them.

use crate::vcs::graph::{EdgeKind, GraphCommit};
use crate::vcs_ops::log_store::{RepoCommit, commit_key};
use ide_vcs::Commit;

/// The commit as the graph links it: its key and those of its parents in the same repository. The
/// parents are those of the filtered graph when the log carries them, followed by the parents a
/// shallow history ends at.
pub fn graph_commit(c: &RepoCommit) -> GraphCommit {
    let commit = &c.commit;
    let repo = c.repo.as_deref();
    let shown = commit.graph_parents.as_ref().unwrap_or(&commit.parents);
    let dashed = if commit.graph_dashed.is_empty() { &commit.dashed } else { &commit.graph_dashed };
    let kind = |index: usize| {
        if dashed.get(index).copied().unwrap_or(false) { EdgeKind::Filtered } else { EdgeKind::Normal }
    };
    GraphCommit {
        id: c.key(),
        parents: shown.iter().chain(&commit.graph_boundary_parents).map(|id| commit_key(id, repo)).collect(),
        dashed: dashed.clone(),
        refs: Some(commit.refs.clone()),
        remote_names: c.ref_branches.iter().filter(|branch| branch.remote).map(|branch| branch.name.clone()).collect(),
        edge_types: (0..shown.len())
            .map(kind)
            .chain(commit.graph_boundary_parents.iter().map(|_| EdgeKind::Shallow))
            .collect(),
    }
}

/// The time a log of several repositories orders a commit by: its committer time, else its author
/// time.
fn order_time(commit: &Commit) -> f64 {
    if commit.commit_time_ms == 0.0 { commit.time_ms } else { commit.commit_time_ms }
}

/// Merges the logs of several repositories, newest first, into at most `limit` rows without
/// breaking any repository's own order.
pub fn merge_repo_logs(streams: &[Vec<RepoCommit>], limit: usize) -> Vec<RepoCommit> {
    let mut offsets = vec![0; streams.len()];
    let mut result = Vec::new();
    while result.len() < limit {
        let mut best: Option<(usize, &RepoCommit)> = None;
        for (index, stream) in streams.iter().enumerate() {
            let Some(next) = stream.get(offsets[index]) else { continue };
            if best.is_none_or(|(_, found)| order_time(&next.commit) > order_time(&found.commit)) {
                best = Some((index, next));
            }
        }
        let Some((index, next)) = best else { break };
        result.push(next.clone());
        offsets[index] += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::PAGE;
    use super::super::log_loading::test_support::{Harness, commit, repo_commit};
    use super::*;
    use crate::vcs_diff::diff_input::{CommitRef, commit_file_diff_data};
    use crate::vcs_ops::log_store::{LogFilters, LogTabKind, LogTabLocation, MAIN_LOG, toggle_repository};
    use ide_vcs::{ChangeKind, ChangedFile};
    use std::collections::{HashMap, HashSet};

    const FRONT: &str = "/project/front";
    const BACK: &str = "/project/back";

    fn timed(id: &str, time_ms: f64) -> Commit {
        Commit { time_ms, ..commit(id, &[], &[]) }
    }

    /// A project with the repositories `/project/front` (the selected one) and `/project/back`.
    fn project() -> Harness {
        let mut h = Harness::new();
        h.vcs.root = Some("/project".to_owned());
        h.vcs.roots = vec![FRONT.to_owned(), BACK.to_owned()];
        h.vcs.repo = Some(FRONT.to_owned());
        h
    }

    /// The same project with the front repository alone.
    fn front_only() -> Harness {
        let mut h = project();
        h.vcs.roots = vec![FRONT.to_owned()];
        h
    }

    fn repos(h: &Harness) -> Vec<Option<&str>> {
        h.log.view.commits.iter().map(|c| c.repo.as_deref()).collect()
    }

    /// `count` commits of each repository, each two milliseconds older than the one before it and
    /// the back repository's one millisecond behind the front's.
    fn histories(count: usize, newest: f64) -> HashMap<String, Vec<Commit>> {
        let history = |root: usize| {
            (0..count).map(|i| timed(&format!("{root}-{i}"), newest - (i * 2 + root) as f64)).collect::<Vec<_>>()
        };
        HashMap::from([(FRONT.to_owned(), history(0)), (BACK.to_owned(), history(1))])
    }

    /// Answers from `history`, each commit saying where it is in its repository's log.
    fn answer_counted(h: &mut Harness, history: HashMap<String, Vec<Commit>>) {
        h.answer = Box::new(move |repo, query| {
            let rows = history.get(repo.unwrap_or_default()).map(Vec::as_slice).unwrap_or_default();
            let total = u32::try_from(rows.len()).ok();
            let page = rows.iter().enumerate().skip(query.skip as usize).take(query.limit as usize);
            let counted = |(position, c): (usize, &Commit)| Commit {
                log_position: u32::try_from(position).ok(),
                log_total: total,
                ..c.clone()
            };
            Ok(page.map(counted).collect())
        });
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › keeps filtered edge hints from both graph metadata formats
    #[test]
    fn keeps_filtered_edge_hints_from_both_graph_metadata_formats() {
        let mut row = repo_commit(commit("tip", &["real"], &[]), "/a");
        row.commit.dashed = vec![true];
        let legacy = graph_commit(&row);
        assert_eq!(legacy.parents, ["/a\0real"]);
        assert_eq!((legacy.dashed, legacy.edge_types), (vec![true], vec![EdgeKind::Filtered]));
        row.commit.graph_parents = Some(vec!["visible".to_owned()]);
        row.commit.graph_dashed = vec![false];
        let graph = graph_commit(&row);
        assert_eq!(graph.parents, ["/a\0visible"]);
        assert_eq!((graph.dashed, graph.edge_types), (vec![false], vec![EdgeKind::Normal]));
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › merges streams by time without violating each root's topological order
    #[test]
    fn merges_streams_by_time_without_violating_each_roots_topological_order() {
        let mut child = repo_commit(timed("child", 1.0), "a");
        child.commit.parents = vec!["parent".to_owned()];
        let parent = repo_commit(timed("parent", 100.0), "a");
        let other = repo_commit(timed("other", 50.0), "b");
        let merged = merge_repo_logs(&[vec![child.clone(), parent.clone()], vec![other.clone()]], 3);
        let keys: Vec<String> = merged.iter().map(RepoCommit::key).collect();
        assert_eq!(keys, [other.key(), child.key(), parent.key()]);
        assert_eq!(graph_commit(&child).parents, [parent.key()]);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › keeps identical SHAs distinct in selection and file paths
    #[test]
    fn keeps_identical_shas_distinct_in_selection_and_file_paths() {
        let mut h = project();
        let front = repo_commit(commit("same", &[], &[]), FRONT);
        let back = repo_commit(commit("same", &[], &[]), BACK);
        h.log.view.commits = vec![front.clone(), back.clone()];
        h.log.select_commit(Some(&front.key()), false);
        let selected = |h: &Harness| h.log.selected_commits().into_iter().map(RepoCommit::key).collect::<Vec<_>>();
        assert_eq!(selected(&h), [front.key()]);
        h.log.toggle_commit_selection(&back.key());
        assert_eq!(selected(&h), [front.key(), back.key()]);
        let of_back = CommitRef {
            id: &back.commit.id,
            short_id: &back.commit.short_id,
            parents: &back.commit.parents,
            repo: back.repo.as_deref(),
        };
        let file = ChangedFile { path: "main.ts".to_owned(), kind: ChangeKind::Added, old_path: None };
        assert_eq!(commit_file_diff_data(of_back, &file, h.vcs.current_repo_root()).path, "/project/back/main.ts");
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › pages each root by the number of its emitted commits, without gaps or duplicates
    #[test]
    fn pages_each_root_by_the_number_of_its_emitted_commits_without_gaps_or_duplicates() {
        let mut h = project();
        h.answer_from(histories(PAGE + 5, 10_000.0));
        h.reload(false);
        assert_eq!(h.log.view.commits.len(), PAGE);
        h.load_more();
        h.load_more();
        let rows = &h.log.view.commits;
        assert_eq!(rows.len(), (PAGE + 5) * 2);
        let keys: HashSet<String> = rows.iter().map(RepoCommit::key).collect();
        assert_eq!(keys.len(), rows.len());
        assert!(rows.windows(2).all(|pair| pair[0].commit.time_ms >= pair[1].commit.time_ms));
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › does not mark native root pages complete before every fetched commit has been emitted
    #[test]
    fn does_not_mark_native_root_pages_complete_before_every_fetched_commit_has_been_emitted() {
        let mut h = project();
        answer_counted(&mut h, histories(180, 10_000.0));
        h.reload(false);
        assert_eq!(h.log.view.commits.len(), PAGE);
        assert!(!h.log.view.done);
        h.load_more();
        assert_eq!(h.log.view.commits.len(), 360);
        assert!(h.log.view.done);
        let keys: HashSet<String> = h.log.view.commits.iter().map(RepoCommit::key).collect();
        assert_eq!(keys.len(), 360);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › filters repositories and keeps history tabs pinned when the selected root changes
    #[test]
    fn filters_repositories_and_keeps_history_tabs_pinned_when_the_selected_root_changes() {
        let mut h = project();
        h.answer_with(vec![commit("same", &[], &[])]);
        h.log.view.filters = LogFilters { repositories: Some(vec![BACK.to_owned()]), ..LogFilters::default() };
        h.reload(false);
        assert_eq!(repos(&h), [Some(BACK)]);
        let target = "/project/back/main.ts";
        let opened = h.log.open_log_tab(LogTabKind::History, target, LogTabLocation::ToolWindow, None, &h.vcs);
        h.reload(false);
        h.vcs.repo = Some(FRONT.to_owned());
        h.log.select_git_tab(MAIN_LOG);
        h.vcs.repo = Some(BACK.to_owned());
        h.log.select_git_tab(&opened.id);
        assert_eq!(h.log.view.filters.path.as_deref(), Some(target));
        assert_eq!(repos(&h).first(), Some(&Some(BACK)));
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › shows a submodule folder's own history, not the folder's history in the enclosing repository
    #[test]
    fn shows_a_submodule_folders_own_history_not_the_folders_history_in_the_enclosing_repository() {
        let mut h = project();
        h.vcs.roots = vec!["/r".to_owned(), "/r/libs/core".to_owned()];
        for target in ["/r/libs/core", "/r/libs/core/src/Lib.kt", "/r/libs"] {
            h.log.open_log_tab(LogTabKind::History, target, LogTabLocation::ToolWindow, None, &h.vcs);
        }
        let tab_repos: Vec<Option<&str>> = h.log.log_tabs.iter().map(|tab| tab.repo.as_deref()).collect();
        assert_eq!(tab_repos, [Some("/r/libs/core"), Some("/r/libs/core"), Some("/r")]);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › multi-root Git log › shows several repositories chosen one click at a time in the Repository filter
    #[test]
    fn shows_several_repositories_chosen_one_click_at_a_time_in_the_repository_filter() {
        let mut h = project();
        h.answer = Box::new(|repo, _| Ok(vec![commit(repo.unwrap_or_default(), &[], &[])]));
        let repositories = toggle_repository(None, FRONT);
        let repositories = toggle_repository(Some(&repositories), BACK);
        assert_eq!(repositories, [FRONT, BACK]);
        h.log.view.filters = LogFilters { repositories: Some(repositories.clone()), ..LogFilters::default() };
        h.reload(false);
        let shown: HashSet<Option<&str>> = repos(&h).into_iter().collect();
        assert_eq!(shown, HashSet::from([Some(FRONT), Some(BACK)]));
        assert_eq!(toggle_repository(Some(&repositories), FRONT), [BACK]);
        assert!(toggle_repository(Some(&[BACK.to_owned()]), BACK).is_empty());
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › Git log background refresh › keeps history and selection visible until the replacement is ready
    #[test]
    fn keeps_history_and_selection_visible_until_the_replacement_is_ready() {
        let mut h = front_only();
        h.answer_with(vec![commit("old", &[], &[])]);
        h.reload(false);
        let old = h.log.view.commits[0].key();
        h.log.select_commit(Some(&old), false);
        let refresh = h.start_reload(MAIN_LOG, true).expect("the log is read");
        assert_eq!(h.ids(), ["old"]);
        assert!(h.log.view.loading);
        h.settle(refresh, vec![vec![commit("new", &[], &[]), commit("old", &[], &[])]]);
        assert_eq!(h.ids(), ["new", "old"]);
        assert_eq!(h.log.view.selected, Some(old));
        assert!(!h.log.view.loading);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › Git log background refresh › refreshes the loaded range and resumes paging at the replacement's end
    #[test]
    fn refreshes_the_loaded_range_and_resumes_paging_at_the_replacements_end() {
        let mut h = front_only();
        let mut history: Vec<Commit> = (0..PAGE * 3).map(|i| commit(&i.to_string(), &[], &[])).collect();
        h.answer_from(HashMap::from([(FRONT.to_owned(), history.clone())]));
        h.reload(false);
        h.load_more();
        history.insert(0, commit("new", &[], &[]));
        h.answer_from(HashMap::from([(FRONT.to_owned(), history.clone())]));
        h.reload(true);
        assert_eq!(h.log.view.commits.len(), PAGE * 2);
        assert_eq!((h.last_query().skip as usize, h.last_query().limit as usize), (0, PAGE * 2));
        h.load_more();
        assert_eq!((h.last_query().skip as usize, h.last_query().limit as usize), (PAGE * 2, PAGE));
        let expected: Vec<&str> = history[..PAGE * 3].iter().map(|c| c.id.as_str()).collect();
        assert_eq!(h.ids(), expected);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › Git log background refresh › keeps the loaded multi-root window on refresh without emitting older candidates
    #[test]
    fn keeps_the_loaded_multi_root_window_on_refresh_without_emitting_older_candidates() {
        let mut h = project();
        let mut history = histories(180, 1000.0);
        answer_counted(&mut h, history.clone());
        h.reload(false);
        let loaded: Vec<String> = h.ids().into_iter().map(str::to_owned).collect();
        assert_eq!(loaded.len(), PAGE);

        history.get_mut(FRONT).expect("the front repository has a history").insert(0, timed("new", 2000.0));
        answer_counted(&mut h, history.clone());
        h.reload(true);
        let mut refreshed = vec!["new"];
        refreshed.extend(loaded.iter().map(String::as_str));
        assert_eq!(h.ids(), refreshed);
        assert!(!h.log.view.done);

        h.load_more();
        let mut expected: Vec<&Commit> = history[FRONT].iter().chain(&history[BACK]).collect();
        expected.sort_by(|a, b| b.time_ms.total_cmp(&a.time_ms));
        let expected: Vec<&str> = expected.into_iter().map(|c| c.id.as_str()).collect();
        assert_eq!(h.ids(), expected);
        assert!(h.log.view.done);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › Git log background refresh › retains the current history if refreshing fails
    #[test]
    fn retains_the_current_history_if_refreshing_fails() {
        let mut h = front_only();
        h.answer_with(vec![commit("old", &[], &[])]);
        h.reload(false);
        h.answer = Box::new(|_, _| Err("unavailable".to_owned()));
        h.reload(true);
        assert_eq!(h.ids(), ["old"]);
        assert!(h.log.view.error.as_deref().is_some_and(|error| error.contains("unavailable")));
        assert!(!h.log.view.loading);
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › Git log background refresh › ignores a refresh that finishes after the filters have changed
    #[test]
    fn ignores_a_refresh_that_finishes_after_the_filters_have_changed() {
        let mut h = front_only();
        let refresh = h.start_reload(MAIN_LOG, true).expect("the log is read");
        h.log.view.filters = LogFilters { text: "filtered".to_owned(), ..LogFilters::default() };
        h.answer_with(vec![commit("filtered", &[], &[])]);
        h.reload(false);
        h.settle(refresh, vec![vec![commit("stale", &[], &[])]]);
        assert_eq!(h.ids(), ["filtered"]);
    }
}
