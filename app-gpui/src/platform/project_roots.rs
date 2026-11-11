//! Source / test / resource / excluded roots of the open project: detected by the backend from
//! build files and merged here with the Project Structure marks, so marking a folder shows at
//! once.

use serde_json::Value;
use std::cmp::Ordering;

/// Root kinds, as in IDEA's Project Structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Source,
    Test,
    Resources,
    TestResources,
    Excluded,
}

impl FolderRole {
    /// Every role, in the order user marks are applied.
    pub const ALL: [Self; 5] = [Self::Source, Self::Test, Self::Resources, Self::TestResources, Self::Excluded];
}

/// A root of the project: user-marked (`detected_from` is `None`) or detected from a build file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootFolder {
    /// Project-relative, `/`-separated; `""` is the project root.
    pub path: String,
    pub kind: FolderRole,
    pub detected_from: Option<String>,
}

/// The folder marks of the Project Structure settings: project-relative, `/`-separated folders,
/// the content root itself stored as `.`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectStructure {
    pub source_roots: Vec<String>,
    pub test_roots: Vec<String>,
    pub resource_roots: Vec<String>,
    pub test_resource_roots: Vec<String>,
    pub excluded: Vec<String>,
    /// Auto-detected roots the user unmarked.
    pub unmarked: Vec<String>,
}

impl ProjectStructure {
    fn marked(&self, role: FolderRole) -> &[String] {
        match role {
            FolderRole::Source => &self.source_roots,
            FolderRole::Test => &self.test_roots,
            FolderRole::Resources => &self.resource_roots,
            FolderRole::TestResources => &self.test_resource_roots,
            FolderRole::Excluded => &self.excluded,
        }
    }

    fn marked_mut(&mut self, role: FolderRole) -> &mut Vec<String> {
        match role {
            FolderRole::Source => &mut self.source_roots,
            FolderRole::Test => &mut self.test_roots,
            FolderRole::Resources => &mut self.resource_roots,
            FolderRole::TestResources => &mut self.test_resource_roots,
            FolderRole::Excluded => &mut self.excluded,
        }
    }

    fn lists_mut(&mut self) -> [&mut Vec<String>; 6] {
        [
            &mut self.source_roots,
            &mut self.test_roots,
            &mut self.resource_roots,
            &mut self.test_resource_roots,
            &mut self.excluded,
            &mut self.unmarked,
        ]
    }
}

/// White space in the ECMAScript sense, the set the settings were trimmed with when written.
/// Unlike `char::is_whitespace` it has U+FEFF and lacks U+0085.
fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// Orders by UTF-16 code units. The sorted lists end up in settings the webview UI reads and
/// writes too, and that is its order; `str::cmp` differs for characters beyond U+FFFF.
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Project-relative folder: `/`-separated, no leading `./` or `/`, no trailing `/`.
fn normalize_folder(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut rest = path.trim_matches(is_space);
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest.trim_matches('/').to_owned()
}

/// Settings store the content root itself as `.`; roots use `""`.
fn folder_key(stored: &str) -> &str {
    if stored == "." { "" } else { stored }
}

fn stored_folder(rel: &str) -> String {
    let folder = normalize_folder(rel);
    if folder.is_empty() { ".".to_owned() } else { folder }
}

/// The folder marks in the raw `projectStructure` settings value: every list normalized, without
/// empty entries or duplicates; anything that is not a list of strings counts as empty.
pub fn validate_project_structure(raw: &Value) -> ProjectStructure {
    let folders = |key: &str| {
        let mut out: Vec<String> = Vec::new();
        let listed = raw.get(key).and_then(Value::as_array).into_iter().flatten();
        for folder in listed.filter_map(Value::as_str).map(normalize_folder) {
            if !folder.is_empty() && !out.contains(&folder) {
                out.push(folder);
            }
        }
        out
    };
    ProjectStructure {
        source_roots: folders("sourceRoots"),
        test_roots: folders("testRoots"),
        resource_roots: folders("resourceRoots"),
        test_resource_roots: folders("testResourceRoots"),
        excluded: folders("excluded"),
        unmarked: folders("unmarked"),
    }
}

/// User-marked role of a project-relative folder (exact match), or `None`.
pub fn folder_role(ps: &ProjectStructure, rel: &str) -> Option<FolderRole> {
    let stored = stored_folder(rel);
    FolderRole::ALL.into_iter().find(|&role| ps.marked(role).contains(&stored))
}

/// Effective roots: user marks win, `unmarked` hides detected roots, sorted by path.
pub fn effective_roots(detected: &[RootFolder], ps: &ProjectStructure) -> Vec<RootFolder> {
    let mut out: Vec<RootFolder> = Vec::new();
    for role in FolderRole::ALL {
        for path in ps.marked(role).iter().map(String::as_str).map(folder_key) {
            if !out.iter().any(|r| r.path == path) {
                out.push(RootFolder { path: path.to_owned(), kind: role, detected_from: None });
            }
        }
    }
    let unmarked: Vec<&str> = ps.unmarked.iter().map(String::as_str).map(folder_key).collect();
    for d in detected {
        if !unmarked.contains(&d.path.as_str()) && !out.iter().any(|r| r.path == d.path) {
            out.push(d.clone());
        }
    }
    out.sort_by(|a, b| utf16_cmp(&a.path, &b.path));
    out
}

/// Effective role of a folder (exact match): what its icon shows.
pub fn root_role(roots: &[RootFolder], rel: &str) -> Option<FolderRole> {
    let folder = normalize_folder(rel);
    roots.iter().find(|root| root.path == folder).map(|root| root.kind)
}

/// Whether `path` is `folder` or lies below it.
fn at_or_below(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The innermost root containing the project-relative path `rel`.
pub fn root_for<'a>(roots: &'a [RootFolder], rel: &str) -> Option<&'a RootFolder> {
    let rel = normalize_folder(rel);
    // Reversed, so that among equally deep roots the first one listed wins.
    roots
        .iter()
        .rev()
        .filter(|root| root.path.is_empty() || at_or_below(&rel, &root.path))
        .max_by_key(|root| root.path.len())
}

/// Test files recognised by name, whatever their folder.
pub fn is_test_file_name(name: &str) -> bool {
    if name.ends_with("_test.go") {
        return true;
    }
    if let Some(stem) = name.strip_suffix(".py") {
        return stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest";
    }
    // `x.test.ts`, `x.spec.jsx`, `x.test.mjs`…: a stem on one line, the kind, a script extension.
    let mut parts = name.rsplitn(3, '.');
    let (extension, kind, stem) = (parts.next(), parts.next(), parts.next());
    matches!(extension, Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"))
        && matches!(kind, Some("test" | "spec"))
        && stem.is_some_and(|stem| !stem.is_empty() && !stem.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// IDEA's "Project Test Files": under a test (resource) root, or a test file by name.
pub fn is_test_path(roots: &[RootFolder], rel: &str) -> bool {
    match root_for(roots, rel).map(|root| root.kind) {
        Some(FolderRole::Test | FolderRole::TestResources) => true,
        Some(FolderRole::Excluded) => false,
        _ => is_test_file_name(normalize_folder(rel).rsplit('/').next().unwrap_or_default()),
    }
}

/// Gives `rel` the `role` (`None` = Unmark). Marking a detected root with its detected role
/// needs no override; unmarking a detected root records it in `unmarked`.
pub fn mark_folder(
    ps: &ProjectStructure,
    rel: &str,
    role: Option<FolderRole>,
    detected: &[RootFolder],
) -> ProjectStructure {
    let stored = stored_folder(rel);
    let mut next = ps.clone();
    for list in next.lists_mut() {
        list.retain(|folder| *folder != stored);
    }
    let auto = detected.iter().find(|d| d.path == folder_key(&stored)).map(|d| d.kind);
    let list = match role {
        None if auto.is_some() => &mut next.unmarked,
        Some(role) if auto != Some(role) => next.marked_mut(role),
        _ => return next,
    };
    list.push(stored);
    list.sort_by(|a, b| utf16_cmp(a, b));
    next
}

/// Detected roots the user unmarked.
pub fn unmarked_roots<'a>(detected: &'a [RootFolder], ps: &ProjectStructure) -> Vec<&'a RootFolder> {
    detected.iter().filter(|d| ps.unmarked.iter().any(|stored| folder_key(stored) == d.path)).collect()
}

/// Brings back every unmarked detected root.
pub fn restore_detected(ps: &ProjectStructure, detected: &[RootFolder]) -> ProjectStructure {
    let mut next = ps.clone();
    next.unmarked.retain(|stored| !detected.iter().any(|d| d.path == folder_key(stored)));
    next
}

/// The detected roots last read from the backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedRoots {
    /// Project the roots belong to.
    pub root: Option<String>,
    pub detected: Vec<RootFolder>,
}

/// What the roots need to know of the open project.
#[derive(Debug, Clone, Copy)]
pub struct OpenProject<'a> {
    pub root: &'a str,
    pub name: &'a str,
    /// Member folders (absolute paths) when the project is a workspace.
    pub workspace: Option<&'a [String]>,
}

/// Detected roots of the open project; none while `state` still holds another project's.
pub fn current_detected_roots<'a>(state: &'a DetectedRoots, project_root: Option<&str>) -> &'a [RootFolder] {
    if state.root.as_deref() == project_root { &state.detected } else { &[] }
}

/// Effective roots of the open project.
pub fn current_roots(state: &DetectedRoots, project_root: Option<&str>, ps: &ProjectStructure) -> Vec<RootFolder> {
    effective_roots(current_detected_roots(state, project_root), ps)
}

/// The user-visible folder containing `path`: the project root, or in a workspace the innermost
/// member holding it. Workspace state is never a content root.
fn content_root<'a>(project: OpenProject<'a>, path: &str) -> Option<&'a str> {
    match project.workspace {
        None => Some(project.root),
        // Reversed, so that among equally long members the first one listed wins.
        Some(members) => members
            .iter()
            .rev()
            .map(String::as_str)
            .filter(|member| at_or_below(path, member))
            .max_by_key(|member| member.len()),
    }
}

/// Project-relative path of an absolute `path`, or `None` outside the project.
pub fn project_relative<'p>(project: Option<OpenProject<'_>>, path: &'p str) -> Option<&'p str> {
    let root = content_root(project?, path).filter(|root| !root.is_empty())?;
    let rest = path.strip_prefix(root)?;
    if rest.is_empty() { Some(rest) } else { rest.strip_prefix('/') }
}

/// The innermost root containing the absolute `path`.
pub fn root_of_path<'a>(
    project: Option<OpenProject<'_>>,
    path: &str,
    roots: &'a [RootFolder],
) -> Option<&'a RootFolder> {
    root_for(roots, project_relative(project, path)?)
}

/// Whether the absolute `path` is a test file (test root or test-file name).
pub fn is_test_file(project: Option<OpenProject<'_>>, path: &str, roots: &[RootFolder]) -> bool {
    project_relative(project, path).is_some_and(|rel| is_test_path(roots, rel))
}

/// IDEA's named scopes over the roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFileSet {
    All,
    Production,
    Test,
}

/// Whether the absolute `path` belongs to a named project scope ("Project Production Files" …).
pub fn in_file_set(set: ProjectFileSet, project: Option<OpenProject<'_>>, path: &str, roots: &[RootFolder]) -> bool {
    let Some(rel) = project_relative(project, path) else { return false };
    if root_for(roots, rel).is_some_and(|root| root.kind == FolderRole::Excluded) {
        return false;
    }
    match set {
        ProjectFileSet::All => true,
        ProjectFileSet::Production => !is_test_path(roots, rel),
        ProjectFileSet::Test => is_test_path(roots, rel),
    }
}

/// A module: the project itself or a folder with a build file roots were detected from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectModule {
    pub name: String,
    /// Project-relative, or absolute for a workspace member; `""` is a plain project root.
    pub path: String,
}

/// The last segment of a `/`-separated path.
fn basename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Modules of the project, root first, then by path.
pub fn project_modules(detected: &[RootFolder], project_name: &str) -> Vec<ProjectModule> {
    let mut dirs: Vec<&str> = detected
        .iter()
        .filter_map(|root| root.detected_from.as_deref())
        .filter(|build_file| !build_file.is_empty())
        .map(|build_file| build_file.rsplit_once('/').map_or("", |(dir, _)| dir))
        .chain([""])
        .collect();
    dirs.sort_by(|a, b| utf16_cmp(a, b));
    dirs.dedup();
    dirs.into_iter()
        .map(|path| {
            let name = if path.is_empty() { project_name } else { basename(path) };
            ProjectModule { name: name.to_owned(), path: path.to_owned() }
        })
        .collect()
}

/// Modules of the open project: the members of a workspace, else the modules its detected roots
/// (see [`current_detected_roots`]) come from.
pub fn open_project_modules(project: Option<OpenProject<'_>>, detected: &[RootFolder]) -> Vec<ProjectModule> {
    match project.and_then(|project| project.workspace) {
        Some(members) => {
            members.iter().map(|path| ProjectModule { name: basename(path).to_owned(), path: path.clone() }).collect()
        }
        None => project_modules(detected, project.map_or("", |project| project.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PROJECT: OpenProject<'static> = OpenProject { root: "/p", name: "p", workspace: None };

    fn pom(path: &str, kind: FolderRole) -> RootFolder {
        RootFolder { path: path.to_owned(), kind, detected_from: Some("pom.xml".to_owned()) }
    }

    fn pom_roots() -> Vec<RootFolder> {
        vec![
            pom("src/main/java", FolderRole::Source),
            pom("src/main/resources", FolderRole::Resources),
            pom("src/test/java", FolderRole::Test),
            pom("src/test/resources", FolderRole::TestResources),
            pom("target", FolderRole::Excluded),
        ]
    }

    fn go_root() -> RootFolder {
        RootFolder { path: String::new(), kind: FolderRole::Source, detected_from: Some("go.mod".to_owned()) }
    }

    fn no_marks() -> ProjectStructure {
        validate_project_structure(&json!({}))
    }

    /// The roots of project `/p` with `out` excluded in its settings, read while the detected
    /// roots in the store belong to `store_root`.
    fn roots_with_store_of(store_root: &str) -> Vec<RootFolder> {
        let state = DetectedRoots { root: Some(store_root.to_owned()), detected: pom_roots() };
        current_roots(&state, Some(PROJECT.root), &validate_project_structure(&json!({ "excluded": ["out"] })))
    }

    fn paths(roots: &[RootFolder]) -> Vec<&str> {
        roots.iter().map(|root| root.path.as_str()).collect()
    }

    // src/platform/projectRoots.test.ts › shows detected roots until the user marks or unmarks them
    #[test]
    fn shows_detected_roots_until_the_user_marks_or_unmarks_them() {
        let detected = pom_roots();
        let ps = validate_project_structure(&json!({
            "sourceRoots": ["gen"],
            "testRoots": ["src/main/java"],
            "unmarked": ["target"],
        }));
        let roots = effective_roots(&detected, &ps);
        let summary: Vec<(&str, FolderRole, Option<&str>)> =
            roots.iter().map(|r| (r.path.as_str(), r.kind, r.detected_from.as_deref())).collect();
        assert_eq!(
            summary,
            [
                ("gen", FolderRole::Source, None),
                ("src/main/java", FolderRole::Test, None),
                ("src/main/resources", FolderRole::Resources, Some("pom.xml")),
                ("src/test/java", FolderRole::Test, Some("pom.xml")),
                ("src/test/resources", FolderRole::TestResources, Some("pom.xml")),
            ]
        );
        assert_eq!(root_role(&roots, "target"), None);
        let unmarked: Vec<&str> = unmarked_roots(&detected, &ps).into_iter().map(|r| r.path.as_str()).collect();
        assert_eq!(unmarked, ["target"]);
        let restored = effective_roots(&detected, &restore_detected(&ps, &detected));
        assert_eq!(root_role(&restored, "target"), Some(FolderRole::Excluded));
    }

    // src/platform/projectRoots.test.ts › marks against detection: same role needs no override, unmark records the detected root
    #[test]
    fn marks_against_detection() {
        let detected = pom_roots();
        let mut ps = mark_folder(&no_marks(), "src/main/java", Some(FolderRole::Source), &detected);
        assert!(ps.source_roots.is_empty());
        ps = mark_folder(&ps, "src/main/java", None, &detected);
        assert_eq!(ps.unmarked, ["src/main/java"]);
        assert_eq!(root_role(&effective_roots(&detected, &ps), "src/main/java"), None);
        // Re-marking it brings it back without a user entry.
        ps = mark_folder(&ps, "src/main/java", Some(FolderRole::Source), &detected);
        assert_eq!(ps, no_marks());
        ps = mark_folder(&ps, "docs", Some(FolderRole::TestResources), &detected);
        assert_eq!(folder_role(&ps, "docs"), Some(FolderRole::TestResources));
        assert!(mark_folder(&ps, "docs", None, &detected).test_resource_roots.is_empty());
    }

    // src/platform/projectRoots.test.ts › stores the content root as '.'
    #[test]
    fn stores_the_content_root_as_a_dot() {
        let go = [go_root()];
        let ps = mark_folder(&no_marks(), "", None, &go);
        assert_eq!(ps.unmarked, ["."]);
        assert!(effective_roots(&go, &ps).is_empty());
        let marked = mark_folder(&no_marks(), "", Some(FolderRole::Test), &[]);
        assert_eq!(marked.test_roots, ["."]);
        assert_eq!(root_role(&effective_roots(&[], &marked), ""), Some(FolderRole::Test));
    }

    // src/platform/projectRoots.test.ts › finds the innermost root and recognises test files
    #[test]
    fn finds_the_innermost_root_and_recognises_test_files() {
        let mut detected = pom_roots();
        detected.push(go_root());
        let roots = effective_roots(&detected, &no_marks());
        assert_eq!(root_for(&roots, "src/test/java/a/B.java").map(|r| r.path.as_str()), Some("src/test/java"));
        assert_eq!(root_for(&roots, "src/test/javax/B.java").map(|r| r.path.as_str()), Some(""));
        assert!(is_test_path(&roots, "src/test/java/a/B.java"));
        assert!(is_test_path(&roots, "src/test/resources/app.yml"));
        assert!(!is_test_path(&roots, "src/main/java/a/B.java"));
        assert!(is_test_path(&roots, "pkg/math_test.go"));
        assert!(!is_test_path(&roots, "target/x_test.go"));
        for name in ["a_test.go", "test_x.py", "x_test.py", "conftest.py", "a.test.ts", "b.spec.jsx"] {
            assert!(is_test_file_name(name), "{name}");
        }
        for name in ["test.ts", "latest.go", "contest.py", "a.test.rs"] {
            assert!(!is_test_file_name(name), "{name}");
        }
    }

    // src/platform/projectRoots.test.ts › merges detected roots with the current settings
    #[test]
    fn merges_detected_roots_with_the_current_settings() {
        let roots = roots_with_store_of("/p");
        assert!(paths(&roots).contains(&"out"));
        let root = root_of_path(Some(PROJECT), "/p/src/main/java/a/B.java", &roots);
        assert_eq!(root.map(|r| r.kind), Some(FolderRole::Source));
        assert_eq!(root_of_path(Some(PROJECT), "/elsewhere/B.java", &roots), None);
        assert!(is_test_file(Some(PROJECT), "/p/src/test/java/a/BTest.java", &roots));
    }

    // src/platform/projectRoots.test.ts › answers IDEA's named scopes
    #[test]
    fn answers_ideas_named_scopes() {
        let roots = roots_with_store_of("/p");
        let project = Some(PROJECT);
        assert!(in_file_set(ProjectFileSet::Production, project, "/p/src/main/java/A.java", &roots));
        assert!(!in_file_set(ProjectFileSet::Production, project, "/p/src/test/java/ATest.java", &roots));
        assert!(in_file_set(ProjectFileSet::Test, project, "/p/src/test/java/ATest.java", &roots));
        assert!(in_file_set(ProjectFileSet::All, project, "/p/README.md", &roots));
        assert!(!in_file_set(ProjectFileSet::All, project, "/p/target/x.txt", &roots));
        assert!(!in_file_set(ProjectFileSet::All, project, "/elsewhere/x.txt", &roots));
    }

    // src/platform/projectRoots.test.ts › ignores roots of another project
    #[test]
    fn ignores_roots_of_another_project() {
        assert_eq!(paths(&roots_with_store_of("/other")), ["out"]);
    }

    // src/platform/projectRoots.test.ts › lists workspace members as scopes and admits their files without admitting workspace state
    #[test]
    fn lists_workspace_members_as_scopes_and_admits_their_files_without_admitting_workspace_state() {
        let members = ["/z".to_owned(), "/a".to_owned()];
        let project = Some(OpenProject { root: "/workspace", name: "Workspace", workspace: Some(members.as_slice()) });
        let state = DetectedRoots { root: Some("/p".to_owned()), detected: pom_roots() };
        let detected = current_detected_roots(&state, Some("/workspace"));
        assert_eq!(
            open_project_modules(project, detected),
            [
                ProjectModule { name: "z".to_owned(), path: "/z".to_owned() },
                ProjectModule { name: "a".to_owned(), path: "/a".to_owned() },
            ]
        );
        let roots = effective_roots(detected, &no_marks());
        assert!(in_file_set(ProjectFileSet::All, project, "/z/src/main.ts", &roots));
        assert!(!in_file_set(ProjectFileSet::All, project, "/workspace/.openidea/workspace.json", &roots));
    }
}
