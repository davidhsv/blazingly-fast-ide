//! External Libraries: the project's SDK / library source folders (the Python interpreter's
//! standard library and site-packages, the JDK, node_modules packages, Cargo crates and the Rust
//! standard library, GOROOT and Go modules, Maven / Gradle dependency jars) as a Project view
//! root. Their files open read-only, like IDEA's library sources. Archives (the JDK's `src.zip`,
//! jars) are browsed through the backend's `<archive>!/<entry>` paths.
//!
//! Nothing here calls the backend: [`LibrariesLoader`] says which request to start or cancel
//! and takes the streamed and final answers; an answer to an older refresh is dropped.

use crate::platform::virtual_docs::is_gradle_build_input;
use crate::project_tree_model::ExtraRoot;
use crate::project_view::store::ProjectViewRoot;
use ide_core::RequestId;
use ide_core::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The backend's library group (`ide_sdk::libraries::LibraryGroup`; that crate is not a
/// dependency of this one, so the same JSON shape is mirrored here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGroup {
    /// Display name, e.g. `Python 3.12 (.venv)`.
    pub name: String,
    /// `python` | `jdk` | `nodeModules` | `nodeCore` | `rust` | `cargo` | `go` | `goModules` |
    /// `maven` | `gradle` | `classpath` | `ruby` | `gems`.
    pub kind: String,
    pub roots: Vec<LibraryRoot>,
}

/// The backend's `ide_sdk::libraries::LibraryRoot` (its `IpcPath` is a string on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoot {
    /// Folder name, the package (`serde 1.0.200`) or the archive (`src.zip`).
    pub name: String,
    /// A folder, or an archive's root (`<archive>!`).
    pub path: String,
}

pub const EXTERNAL_LIBRARIES_LABEL: &str = "External Libraries";
/// The id of the External Libraries root and the owner of its read-only library roots.
pub const ROOT_ID: &str = "externalLibraries";
/// How long after a manifest change the libraries are listed again (more changes restart it).
pub const MANIFEST_REFRESH_DELAY_MS: u64 = 500;

/// The icon of a group of `kind`.
pub fn group_icon(kind: &str) -> &'static str {
    match kind {
        "python" => "file-python",
        "jdk" | "classpath" => "file-java",
        "nodeCore" => "file-js",
        "nodeModules" => "file-npm",
        "rust" => "file-rust",
        "cargo" => "file-cargo",
        "go" => "file-go",
        "goModules" => "file-gomod",
        "maven" => "maven-project",
        "gradle" => "file-gradle",
        "ruby" | "gems" => "file-ruby",
        _ => "library",
    }
}

/// An archive's root (`/jdk/lib/src.zip!`): its entries live below `<archive>!/`.
fn is_archive_root(path: &str) -> bool {
    path.ends_with('!')
}

/// Files whose change can change the libraries (the backend re-detects only the affected ecosystem).
const MANIFESTS: &[&str] = &[
    "pyvenv.cfg",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "package.json",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "bun.lock",
    "bun.lockb",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "go.mod",
    "go.sum",
    "go.work",
    ".classpath",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    ".python-version",
    "Gemfile",
    "Gemfile.lock",
    ".ruby-version",
    ".tool-versions",
    "mise.toml",
];

/// Whether a file-system change of `paths` touches a library manifest outside `node_modules`.
pub fn touches_manifest<P: AsRef<str>>(paths: &[P]) -> bool {
    paths.iter().any(|path| {
        let path: &str = path.as_ref();
        let in_node_modules = path.replace('\\', "/").contains("/node_modules/");
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        !in_node_modules && (is_gradle_build_input(path) || MANIFESTS.contains(&name))
    })
}

/// The "External Libraries" root for `groups` (`< Python 3.12 (.venv) >` › `python3.12`,
/// `site-packages`); `None` without groups. It sorts after the project, before Scratches and
/// Consoles (IDEA's order).
pub fn libraries_root(groups: &[LibraryGroup]) -> Option<ProjectViewRoot> {
    if groups.is_empty() {
        return None;
    }
    let children = groups
        .iter()
        .map(|group| ExtraRoot {
            label: format!("< {} >", group.name),
            icon: group_icon(&group.kind).to_owned(),
            children: Some(
                group
                    .roots
                    .iter()
                    .map(|root| ExtraRoot {
                        label: root.name.clone(),
                        icon: if is_archive_root(&root.path) { "file-archive" } else { "folder-library" }.to_owned(),
                        path: Some(root.path.clone()),
                        detail: Some("library root".to_owned()),
                        ..ExtraRoot::default()
                    })
                    .collect(),
            ),
            ..ExtraRoot::default()
        })
        .collect();
    let root = ExtraRoot {
        id: Some(ROOT_ID.to_owned()),
        label: EXTERNAL_LIBRARIES_LABEL.to_owned(),
        icon: "library".to_owned(),
        read_only: true,
        children: Some(children),
        ..ExtraRoot::default()
    };
    Some(ProjectViewRoot { root, order: 5, scratches: false })
}

/// Merges shared SDKs and package groups (same kind and name) without losing different roots.
pub fn merge_library_groups(groups: impl IntoIterator<Item = LibraryGroup>) -> Vec<LibraryGroup> {
    let mut merged: Vec<LibraryGroup> = Vec::new();
    for group in groups {
        match merged.iter_mut().find(|g| g.kind == group.kind && g.name == group.name) {
            None => merged.push(group),
            Some(existing) => {
                for root in group.roots {
                    if !existing.roots.iter().any(|r| r.path == root.path) {
                        existing.roots.push(root);
                    }
                }
            }
        }
    }
    merged
}

/// The folders whose files open read-only in editors, by the feature that set them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryRoots {
    by_owner: BTreeMap<String, Vec<String>>,
}

impl LibraryRoots {
    /// Replaces `owner`'s roots.
    pub fn set(&mut self, owner: &str, roots: Vec<String>) {
        self.by_owner.insert(owner.to_owned(), roots);
    }

    pub fn remove(&mut self, owner: &str) {
        self.by_owner.remove(owner);
    }

    /// Whether `path` is a library root or below one.
    pub fn is_library_path(&self, path: &str) -> bool {
        self.by_owner.values().flatten().any(|root| {
            path == root.as_str()
                || if root.ends_with('/') {
                    path.starts_with(root.as_str())
                } else {
                    path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/'))
                }
        })
    }
}

/// The settings that change which SDKs the project uses, as one comparable value: a change of
/// it lists the libraries again.
pub fn sdk_key(root: Option<&str>, trusted: Option<bool>, settings: Option<&Settings>) -> String {
    let languages = settings.map(|s| &s.languages);
    let structure = settings.map(|s| &s.project_structure);
    let key = (
        root,
        trusted,
        languages.map(|l| &l.python.interpreter),
        languages.map(|l| &l.ruby.interpreter),
        languages.map(|l| &l.java.jdk),
        languages.map(|l| &l.node.interpreter),
        languages.map(|l| &l.rust.toolchain),
        languages.map(|l| &l.rust.stdlib),
        languages.map(|l| &l.go.goroot),
        languages.map(|l| &l.go.gopath),
        languages.map(|l| &l.go.modules),
        structure.map(|s| &s.libraries),
        structure.map(|s| &s.source_roots),
    );
    serde_json::to_string(&key).unwrap_or_default()
}

/// What the Project view and the editors show for the listed libraries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibrariesView {
    /// The External Libraries root; `None` removes it.
    pub root: Option<ProjectViewRoot>,
    /// The read-only library roots, set as [`ROOT_ID`]'s in [`LibraryRoots`].
    pub library_paths: Vec<String>,
}

/// What a refresh asks the caller to do.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Refresh {
    /// Answers carry this ticket.
    pub ticket: u64,
    /// The request of the previous refresh, still in flight.
    pub cancel: Option<RequestId>,
    /// Start `sdkProjectLibraries` with this id, streaming groups to [`LibrariesLoader::streamed`].
    pub request: Option<RequestId>,
    /// The new view, when it changed (a refresh that cannot load clears it).
    pub view: Option<LibrariesView>,
}

/// Listing the project's libraries: refreshed when the project, its trust or its SDK settings
/// change, and after manifest edits (Reload External Libraries does it on demand).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibrariesLoader {
    seq: u64,
    request: Option<RequestId>,
    collected: Vec<LibraryGroup>,
    /// The groups shown, as JSON; `None` when nothing is shown.
    shown: Option<String>,
}

impl LibrariesLoader {
    fn clear(&mut self) -> Option<LibrariesView> {
        self.shown.take().map(|_| LibrariesView::default())
    }

    fn show(&mut self, groups: &[LibraryGroup]) -> Option<LibrariesView> {
        let key = serde_json::to_string(groups).unwrap_or_default();
        if self.shown.as_deref() == Some(key.as_str()) {
            return None;
        }
        self.shown = Some(key);
        let root = libraries_root(groups);
        let library_paths = match root {
            Some(_) => groups.iter().flat_map(|g| g.roots.iter().map(|r| r.path.clone())).collect(),
            None => Vec::new(),
        };
        Some(LibrariesView { root, library_paths })
    }

    /// Lists the libraries again with the request id `request`. `can_load` is false without a
    /// project, in an untrusted one, or without a backend: then the view is cleared.
    pub fn refresh(&mut self, can_load: bool, request: RequestId) -> Refresh {
        self.seq += 1;
        let cancel = self.request.take();
        self.collected.clear();
        if !can_load {
            return Refresh { ticket: self.seq, cancel, request: None, view: self.clear() };
        }
        self.request = Some(request);
        Refresh { ticket: self.seq, cancel, request: Some(request), view: None }
    }

    /// Groups streamed while listing; they merge with the groups streamed before.
    pub fn streamed(&mut self, ticket: u64, groups: Vec<LibraryGroup>) -> Option<LibrariesView> {
        if ticket != self.seq {
            return None;
        }
        let collected = std::mem::take(&mut self.collected);
        self.collected = merge_library_groups(collected.into_iter().chain(groups));
        let collected = self.collected.clone();
        self.show(&collected)
    }

    /// The final answer of `request`: the full list, or `None` when it failed or was cancelled
    /// (the groups streamed so far stay).
    pub fn answered(
        &mut self,
        ticket: u64,
        request: RequestId,
        groups: Option<Vec<LibraryGroup>>,
    ) -> Option<LibrariesView> {
        if self.request == Some(request) {
            self.request = None;
        }
        let groups = groups?;
        if ticket != self.seq {
            return None;
        }
        self.collected = groups;
        let collected = self.collected.clone();
        self.show(&collected)
    }

    /// Stops listing: the request to cancel, and the view to clear.
    pub fn dispose(&mut self) -> (Option<RequestId>, Option<LibrariesView>) {
        self.seq += 1;
        self.collected.clear();
        (self.request.take(), self.clear())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, kind: &str, roots: &[(&str, &str)]) -> LibraryGroup {
        LibraryGroup {
            name: name.to_owned(),
            kind: kind.to_owned(),
            roots: roots.iter().map(|(n, p)| LibraryRoot { name: (*n).to_owned(), path: (*p).to_owned() }).collect(),
        }
    }

    fn groups_of(root: &ProjectViewRoot) -> &[ExtraRoot] {
        root.root.children.as_deref().unwrap_or_default()
    }

    #[test]
    fn groups_each_sdks_folders_under_one_read_only_root() {
        assert_eq!(libraries_root(&[]), None);
        let root = libraries_root(&[group(
            "Python 3.12 (.venv)",
            "python",
            &[("python3.12", "/py/lib/python3.12"), ("site-packages", "/p/.venv/lib/python3.12/site-packages")],
        )])
        .expect("one group");
        assert_eq!(root.root.id.as_deref(), Some("externalLibraries"));
        assert_eq!(root.root.label, "External Libraries");
        assert!(root.root.read_only);
        let python = &groups_of(&root)[0];
        assert_eq!((python.label.as_str(), python.icon.as_str()), ("< Python 3.12 (.venv) >", "file-python"));
        let rows: Vec<(&str, Option<&str>, Option<&str>)> = python
            .children
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|c| (c.label.as_str(), c.path.as_deref(), c.detail.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("python3.12", Some("/py/lib/python3.12"), Some("library root")),
                ("site-packages", Some("/p/.venv/lib/python3.12/site-packages"), Some("library root")),
            ]
        );
    }

    #[test]
    fn gives_each_ecosystems_groups_their_icon() {
        let kinds = ["jdk", "nodeCore", "nodeModules", "rust", "cargo", "go", "goModules", "maven", "gradle", "other"];
        let groups: Vec<LibraryGroup> =
            kinds.iter().map(|kind| group(kind, kind, &[("r", format!("/libs/{kind}").as_str())])).collect();
        let root = libraries_root(&groups).expect("groups");
        let icons: Vec<&str> = groups_of(&root).iter().map(|g| g.icon.as_str()).collect();
        assert_eq!(
            icons,
            [
                "file-java",
                "file-js",
                "file-npm",
                "file-rust",
                "file-cargo",
                "file-go",
                "file-gomod",
                "maven-project",
                "file-gradle",
                "library",
            ]
        );
    }

    #[test]
    fn lists_archives_src_zip_dependency_jars_as_browsable_roots() {
        let root = libraries_root(&[
            group("temurin-21", "jdk", &[("temurin-21", "/jdk"), ("src.zip", "/jdk/lib/src.zip!")]),
            group("Maven: g:a:1", "maven", &[("a-1-sources.jar", "/m2/g/a/1/a-1-sources.jar!")]),
        ])
        .expect("groups");
        let jdk: Vec<&str> =
            groups_of(&root)[0].children.as_deref().unwrap_or_default().iter().map(|c| c.icon.as_str()).collect();
        assert_eq!(jdk, ["folder-library", "file-archive"]);
        let maven = &groups_of(&root)[1];
        assert_eq!((maven.label.as_str(), maven.icon.as_str()), ("< Maven: g:a:1 >", "maven-project"));
        let mut roots = LibraryRoots::default();
        roots.set("test", vec!["/jdk/lib/src.zip!".to_owned()]);
        assert!(roots.is_library_path("/jdk/lib/src.zip!/java.base/java/lang/String.java"));
    }

    #[test]
    fn refreshes_on_manifest_changes_outside_node_modules() {
        assert!(touches_manifest(&["/p/src/main.rs", "/p/Cargo.toml"]));
        assert!(touches_manifest(&["C:\\p\\go.mod"]));
        assert!(touches_manifest(&["/p/.venv/pyvenv.cfg"]));
        assert!(touches_manifest(&["/p/pnpm-workspace.yaml", "/p/Gemfile.lock"]));
        assert!(!touches_manifest(&["/p/node_modules/react/package.json"]));
        assert!(!touches_manifest(&["/p/src/package.json.bak", "/p/README.md"]));
    }

    #[test]
    fn refreshes_when_an_applied_gradle_script_or_properties_file_changes() {
        assert!(touches_manifest(&["/p/gradle/dependencies.gradle"]));
        assert!(touches_manifest(&["C:\\p\\gradle\\dependencies.gradle.kts"]));
        assert!(touches_manifest(&["/p/gradle.properties"]));
        assert!(!touches_manifest(&["/p/node_modules/tool/dependencies.gradle"]));
    }

    #[test]
    fn marks_files_below_library_roots_for_read_only_editors() {
        let mut roots = LibraryRoots::default();
        roots.set("test", vec!["/py/lib/python3.12".to_owned()]);
        assert!(roots.is_library_path("/py/lib/python3.12/os.py"));
        assert!(!roots.is_library_path("/py/lib/python3.120/x.py"));
        roots.remove("test");
        assert!(!roots.is_library_path("/py/lib/python3.12/os.py"));

        // The loader shows streamed groups, drops answers to an older refresh, and clears the
        // view when it can no longer load.
        let mut loader = LibrariesLoader::default();
        let first = loader.refresh(true, RequestId(1));
        let second = loader.refresh(true, RequestId(2));
        assert_eq!(second.cancel, Some(RequestId(1)));
        let python = group("Python", "python", &[("lib", "/py/lib/python3.12")]);
        assert_eq!(loader.streamed(first.ticket, vec![python.clone()]), None);
        let view = loader.streamed(second.ticket, vec![python.clone()]).expect("the view changes");
        assert_eq!(view.library_paths, ["/py/lib/python3.12"]);
        assert_eq!(loader.answered(second.ticket, RequestId(2), Some(vec![python])), None);
        let cleared = loader.refresh(false, RequestId(3));
        assert_eq!(cleared.view, Some(LibrariesView::default()));
    }

    #[test]
    fn merges_streamed_module_groups_without_dropping_ecosystems_or_distinct_roots() {
        let one = |kind: &str, path: &str| group(kind, kind, &[("dep", path)]);
        let merged = merge_library_groups([
            one("cargo", "/cargo/serde"),
            one("nodeModules", "/web/node_modules/dep"),
            one("maven", "/m2/dep.jar!"),
            one("nodeModules", "/api/node_modules/dep"),
            one("cargo", "/cargo/serde"),
        ]);
        assert_eq!(
            merged,
            [
                one("cargo", "/cargo/serde"),
                group(
                    "nodeModules",
                    "nodeModules",
                    &[("dep", "/web/node_modules/dep"), ("dep", "/api/node_modules/dep")],
                ),
                one("maven", "/m2/dep.jar!"),
            ]
        );
    }
}
