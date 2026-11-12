//! `workspaceContains:<path>` context keys: whether any project root holds `<path>`. The keys come
//! from the `when` clauses of the live contributions; each is answered from the listing of its
//! parent folder, and asked again when the contributions, the roots or a watched path change.
//!
//! [`WorkspaceContains`] reads nothing itself: a refresh returns a [`ContainsRequest`] naming the
//! folders to list, and [`WorkspaceContains::answer`] turns their listings into the key values to
//! set. An answer to a request older than the latest is dropped.

use super::contribution::Contribution;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

const PREFIX: &str = "workspaceContains:";

/// A whole key: at the start, or after a character that cannot be part of one.
static KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z0-9_.\-:/])(workspaceContains:[A-Za-z0-9_.\-:/]+)").expect("the key pattern is valid")
});

/// The distinct `workspaceContains:` keys of `whens`, in order of first appearance.
pub fn workspace_contains_keys<'a>(whens: impl IntoIterator<Item = Option<&'a str>>) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for when in whens.into_iter().flatten() {
        for found in KEY.captures_iter(when) {
            let key = &found[1];
            if !keys.iter().any(|k| k == key) {
                keys.push(key.to_owned());
            }
        }
    }
    keys
}

/// The path of `key` relative to a root; `None` for an absolute path or one that climbs out of it.
fn relative_path(key: &str) -> Option<&str> {
    let path = key.strip_prefix(PREFIX)?.trim_end_matches('/');
    if path.is_empty() || path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return None;
    }
    Some(path)
}

/// Whether a change of `changed` (created, removed or excluded paths) can change one of `keys`: a
/// watched path or one of its parents changed.
pub fn affects_workspace_contains(changed: &[String], roots: &[String], keys: &[String]) -> bool {
    let targets: Vec<String> = keys
        .iter()
        .filter_map(|key| relative_path(key))
        .flat_map(|path| roots.iter().map(move |root| format!("{root}/{path}")))
        .collect();
    /// Whether `target` is under folder `changed`.
    fn within(target: &str, changed: &str) -> bool {
        target.strip_prefix(changed).is_some_and(|rest| rest.starts_with('/'))
    }
    changed.iter().any(|p| targets.iter().any(|t| t == p || within(t, p)))
}

/// How one key is answered: the folders to look in and the name to find there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Check {
    key: String,
    /// `None`: the key is false without looking.
    probe: Option<(Vec<String>, String)>,
}

/// The folders to list for one refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainsRequest {
    pub ticket: u64,
    /// Every folder to list, once each.
    pub dirs: Vec<String>,
    checks: Vec<Check>,
}

/// The `workspaceContains:` keys of a window and what they depend on.
#[derive(Debug, Default)]
pub struct WorkspaceContains {
    latest: u64,
    roots: Vec<String>,
    keys: Vec<String>,
}

impl WorkspaceContains {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keys(&self) -> &[String] {
        &self.keys
    }

    /// Asks again for `keys` under `roots`; supersedes every earlier request.
    pub fn refresh_with(&mut self, roots: &[String], keys: &[String]) -> ContainsRequest {
        self.latest += 1;
        let mut dirs: Vec<String> = Vec::new();
        let checks = keys
            .iter()
            .map(|key| {
                let probe = relative_path(key).map(|path| {
                    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
                    let folders: Vec<String> = roots
                        .iter()
                        .map(|root| if parent.is_empty() { root.clone() } else { format!("{root}/{parent}") })
                        .collect();
                    for folder in &folders {
                        if !dirs.contains(folder) {
                            dirs.push(folder.clone());
                        }
                    }
                    (folders, name.to_owned())
                });
                Check { key: key.clone(), probe }
            })
            .collect();
        ContainsRequest { ticket: self.latest, dirs, checks }
    }

    /// Asks again with the current roots and keys.
    pub fn refresh(&mut self) -> ContainsRequest {
        let (roots, keys) = (self.roots.clone(), self.keys.clone());
        self.refresh_with(&roots, &keys)
    }

    /// The live contributions changed: their `when` clauses name the keys.
    pub fn set_contributions(&mut self, contributions: &[Contribution]) -> ContainsRequest {
        self.keys = workspace_contains_keys(contributions.iter().map(Contribution::when));
        self.refresh()
    }

    /// The window's project roots changed; nothing to ask when they are the same.
    pub fn set_roots(&mut self, roots: Vec<String>) -> Option<ContainsRequest> {
        if roots == self.roots {
            return None;
        }
        self.roots = roots;
        Some(self.refresh())
    }

    /// Files were created or removed.
    pub fn files_changed(&mut self, created: &[String], removed: &[String]) -> Option<ContainsRequest> {
        let changed: Vec<String> = created.iter().chain(removed).cloned().collect();
        affects_workspace_contains(&changed, &self.roots, &self.keys).then(|| self.refresh())
    }

    /// Folders were excluded or included again.
    pub fn excluded_dirs_changed(&mut self, dirs: &[String]) -> Option<ContainsRequest> {
        affects_workspace_contains(dirs, &self.roots, &self.keys).then(|| self.refresh())
    }

    /// The value of each key from the listings of `request.dirs` (a folder missing from `listings`
    /// could not be read and holds nothing); `None` when a newer request was made since.
    pub fn answer(
        &self,
        request: &ContainsRequest,
        listings: &HashMap<String, Vec<String>>,
    ) -> Option<Vec<(String, bool)>> {
        if request.ticket != self.latest {
            return None;
        }
        let values = request
            .checks
            .iter()
            .map(|check| {
                let found = check.probe.as_ref().is_some_and(|(folders, name)| {
                    folders.iter().any(|folder| listings.get(folder).is_some_and(|names| names.contains(name)))
                });
                (check.key.clone(), found)
            })
            .collect();
        Some(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn listings(entries: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        entries.iter().map(|(dir, names)| ((*dir).to_owned(), strings(names))).collect()
    }

    type Listing<'a> = &'a [(&'a str, &'a [&'a str])];

    fn values(model: &mut WorkspaceContains, roots: &[&str], keys: &[&str], files: Listing<'_>) -> Vec<(String, bool)> {
        let request = model.refresh_with(&strings(roots), &strings(keys));
        model.answer(&request, &listings(files)).expect("the latest request")
    }

    fn value(values: &[(String, bool)], key: &str) -> bool {
        values.iter().find(|(k, _)| k == key).expect("the key is answered").1
    }

    #[test]
    fn collects_keys_from_when_clauses() {
        let keys = workspace_contains_keys([
            Some("workspaceContains:a/b.txt && editorFocus"),
            None,
            Some("!workspaceContains:pom.xml"),
            Some("workspaceContains:a/b.txt"),
        ]);
        assert_eq!(keys, strings(&["workspaceContains:a/b.txt", "workspaceContains:pom.xml"]));
    }

    #[test]
    fn sets_each_key_from_its_parent_folders_listing() {
        let mut model = WorkspaceContains::new();
        let keys = ["workspaceContains:.devcontainer/devcontainer.json", "workspaceContains:pom.xml"];
        let request = model.refresh_with(&strings(&["/p"]), &strings(&keys));
        assert!(request.dirs.contains(&"/p".to_owned()));
        let answered =
            model.answer(&request, &listings(&[("/p/.devcontainer", &["devcontainer.json"]), ("/p", &["src"])]));
        let answered = answered.expect("the latest request");
        assert!(value(&answered, keys[0]));
        assert!(!value(&answered, keys[1]));
    }

    #[test]
    fn is_false_without_a_project_or_when_the_folder_cant_be_read() {
        let mut model = WorkspaceContains::new();
        assert!(!value(&values(&mut model, &["/p"], &["workspaceContains:x/y"], &[]), "workspaceContains:x/y"));
        let request = model.refresh_with(&[], &strings(&["workspaceContains:x/y"]));
        assert!(request.dirs.is_empty());
        let answered = model.answer(&request, &HashMap::new()).expect("the latest request");
        assert!(!value(&answered, "workspaceContains:x/y"));
    }

    #[test]
    fn is_true_when_any_root_contains_the_path() {
        let mut model = WorkspaceContains::new();
        let answered = values(&mut model, &["/a", "/b"], &["workspaceContains:pom.xml"], &[("/b", &["pom.xml"])]);
        assert!(value(&answered, "workspaceContains:pom.xml"));
    }

    #[test]
    fn ignores_keys_that_are_absolute_or_climb_out_of_the_project_and_a_trailing_slash() {
        let mut model = WorkspaceContains::new();
        let keys = [
            "workspaceContains:/etc/passwd",
            "workspaceContains:../a",
            "workspaceContains:a/../../b",
            "workspaceContains:.devcontainer/",
        ];
        let request = model.refresh_with(&strings(&["/p"]), &strings(&keys));
        assert_eq!(request.dirs, strings(&["/p"]));
        let answered = model.answer(&request, &listings(&[("/p", &[".devcontainer"])])).expect("the latest request");
        assert!(!value(&answered, keys[0]));
        assert!(!value(&answered, keys[1]));
        assert!(!value(&answered, keys[2]));
        assert!(value(&answered, keys[3]));
    }

    #[test]
    fn only_picks_keys_at_a_word_boundary() {
        let keys = workspace_contains_keys([Some("xworkspaceContains:../a"), Some("a && workspaceContains:b")]);
        assert_eq!(keys, strings(&["workspaceContains:b"]));
    }

    #[test]
    fn refreshes_only_when_a_watched_path_or_one_of_its_parents_changes() {
        let keys = strings(&["workspaceContains:.devcontainer/devcontainer.json"]);
        let affects =
            |paths: &[&str], roots: &[&str]| affects_workspace_contains(&strings(paths), &strings(roots), &keys);
        assert!(!affects(&["/p2/x"], &["/p"]));
        assert!(!affects(&["/p/src/a.ts"], &["/p"]));
        assert!(affects(&["/p/.devcontainer"], &["/p"]));
        assert!(affects(&["/p/.devcontainer/devcontainer.json"], &["/p"]));
        assert!(affects(&["/q/.devcontainer"], &["/p", "/q"]));
        assert!(!affects(&["/p/.devcontainer"], &[]));
    }

    #[test]
    fn ignores_a_stale_refresh_that_resolves_after_a_newer_one() {
        let mut model = WorkspaceContains::new();
        let keys = strings(&["workspaceContains:pom.xml"]);
        let first = model.refresh_with(&strings(&["/a"]), &keys);
        let second = model.refresh_with(&strings(&["/b"]), &keys);
        let fresh = model.answer(&second, &listings(&[("/b", &["pom.xml"])])).expect("the latest request");
        assert!(value(&fresh, "workspaceContains:pom.xml"));
        assert_eq!(model.answer(&first, &listings(&[("/a", &[])])), None);
    }

    const KEY_NAME: &str = "workspaceContains:.devcontainer/devcontainer.json";

    /// A tracker for a window on `/p` whose contribution of `point` names the key.
    fn started(point: &str) -> (WorkspaceContains, ContainsRequest) {
        let mut model = WorkspaceContains::new();
        model.set_roots(strings(&["/p"]));
        let request = model.set_contributions(&[Contribution::new("x", point, "k", json!({ "when": KEY_NAME }))]);
        (model, request)
    }

    fn key_is(model: &WorkspaceContains, request: &ContainsRequest, files: &[(&str, &[&str])]) -> bool {
        value(&model.answer(request, &listings(files)).expect("the latest request"), KEY_NAME)
    }

    #[test]
    fn re_evaluates_when_the_file_is_created_and_deleted() {
        for point in ["menus", "views"] {
            let (mut model, request) = started(point);
            assert!(!key_is(&model, &request, &[("/p", &["src"])]));
            let created = model.files_changed(&strings(&["/p/.devcontainer"]), &[]).expect("a refresh");
            let files: Listing<'_> = &[("/p", &[".devcontainer"]), ("/p/.devcontainer", &["devcontainer.json"])];
            assert!(key_is(&model, &created, files));
            let removed =
                model.files_changed(&[], &strings(&["/p/.devcontainer/devcontainer.json"])).expect("a refresh");
            assert!(!key_is(&model, &removed, &[("/p", &[".devcontainer"]), ("/p/.devcontainer", &[])]));
        }
    }

    #[test]
    fn re_evaluates_when_the_project_changes() {
        for point in ["menus", "views"] {
            let (mut model, request) = started(point);
            let files: Listing<'_> = &[("/q", &[".devcontainer"]), ("/q/.devcontainer", &["devcontainer.json"])];
            assert!(!key_is(&model, &request, files));
            let moved = model.set_roots(strings(&["/q"])).expect("a refresh");
            assert!(key_is(&model, &moved, files));
            assert_eq!(model.set_roots(strings(&["/q"])), None, "the same roots ask nothing");
        }
    }

    #[test]
    fn re_evaluates_when_ignored_folder_changes() {
        for point in ["menus", "views"] {
            for dir in ["/p/.devcontainer", "/p"] {
                let (mut model, request) = started(point);
                assert!(!key_is(&model, &request, &[]));
                let again = model.excluded_dirs_changed(&strings(&[dir])).expect("a refresh");
                assert!(key_is(&model, &again, &[("/p/.devcontainer", &["devcontainer.json"])]));
            }
        }
    }

    #[test]
    fn ignores_unrelated_changes() {
        for point in ["menus", "views"] {
            let (mut model, _) = started(point);
            assert_eq!(model.files_changed(&strings(&["/p/src/a.ts"]), &[]), None);
            assert_eq!(model.excluded_dirs_changed(&strings(&["/p/node_modules"])), None);
        }
    }
}
