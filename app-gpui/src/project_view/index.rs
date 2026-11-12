//! What keeps the Project view in step with its surroundings: the project that is open, Unity
//! regenerating its project files, settings that change what is excluded, and the roots other
//! parts of the IDE have the tree list.

use crate::platform::collate;
use crate::platform::project_roots::{FolderRole, RootFolder};
use crate::project_tree_model::ExtraRoot;
use crate::project_tree_paths::name_of;
use crate::project_view::store::{ProjectInit, ProjectViewRoot, ProjectViewState, StorageWrite, ViewMode};
use crate::vcs_ops::changelists::LocalChangelist;
use ide_workspace::unity::UnityProjectInfo;
use serde_json::Value;

/// How long after a change to Unity's project files their metadata is read again, so that a
/// burst of changes costs one read.
pub const UNITY_REFRESH_DELAY_MS: f64 = 150.0;

/// What the view needs to know of the open project.
#[derive(Debug, Clone, Copy)]
pub struct OpenedProject<'a> {
    pub root: &'a str,
    pub name: &'a str,
    /// Member folders when the project is a workspace.
    pub members: Option<&'a [String]>,
    pub unity: Option<&'a UnityProjectInfo>,
}

/// Brings the view in step with the open project: another project (or other members of the
/// workspace) resets it; the same one updates its name and Unity metadata, and with them the
/// pane. `stored_expanded` is what the storage holds as the expansion of the project.
pub fn sync_project(
    state: &mut ProjectViewState,
    project: Option<OpenedProject<'_>>,
    stored_expanded: Option<&str>,
) -> Vec<StorageWrite> {
    let root = project.map(|project| project.root);
    let members = project.and_then(|project| project.members);
    if root != state.root.as_deref() || members != state.members.as_deref() {
        state.init(project.map(|project| ProjectInit {
            root: project.root,
            name: project.name,
            members: project.members,
            unity: project.unity.cloned(),
            stored_expanded,
        }));
        return Vec::new();
    }
    let Some(project) = project else { return Vec::new() };
    let had_unity = state.unity.is_some();
    state.name = project.name.to_owned();
    state.unity = project.unity.cloned();
    let mode = match (project.unity.is_some(), state.mode) {
        (true, ViewMode::Project) if !had_unity => ViewMode::Unity,
        (false, ViewMode::Unity) => ViewMode::Project,
        _ => return Vec::new(),
    };
    state.set_view_mode(mode, stored_expanded).into_iter().collect()
}

/// Whether a change of `path` may change what Unity reports of the project at `root`: its
/// `Assets`, `Packages` and `ProjectSettings` folders, the editor's version file, and the C#
/// project and solution files it generates at the top.
pub fn is_unity_metadata(root: &str, path: &str) -> bool {
    let Some(rel) = path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) else { return false };
    let folder = rel.strip_suffix("/ProjectVersion.txt").unwrap_or(rel);
    if matches!(folder, "Assets" | "Packages" | "ProjectSettings") {
        return true;
    }
    let named = rel.rsplit_once('.').filter(|(stem, _)| !stem.is_empty());
    let extension = named.map(|(_, extension)| extension.to_ascii_lowercase());
    !rel.contains('/') && matches!(extension.as_deref(), Some("csproj" | "sln" | "slnx"))
}

/// The read of Unity's metadata that fell due: answer it with what the backend now reports of
/// the open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityRead {
    /// The project the metadata is read for.
    pub root: String,
    generation: u64,
}

/// Unity regenerates its project files while the IDE stays open: reads their metadata again
/// once the changes rest, and drops a reply that arrives for another project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnityRefresh {
    generation: u64,
    /// The read that is due, and when.
    due: Option<(UnityRead, f64)>,
}

impl UnityRefresh {
    /// Another project was opened: a read under way no longer counts.
    pub fn project_changed(&mut self) {
        self.generation += 1;
        self.due = None;
    }

    /// Files changed below the project at `root`.
    pub fn fs_changed<P: AsRef<str>>(&mut self, root: &str, paths: &[P], now_ms: f64) {
        if paths.iter().any(|path| is_unity_metadata(root, path.as_ref())) {
            self.generation += 1;
            let read = UnityRead { root: root.to_owned(), generation: self.generation };
            self.due = Some((read, now_ms + UNITY_REFRESH_DELAY_MS));
        }
    }

    /// When [`Self::expire`] is next worth calling.
    pub fn next_deadline(&self) -> Option<f64> {
        self.due.as_ref().map(|(_, at)| *at)
    }

    /// The read to make, once it is due.
    pub fn expire(&mut self, now_ms: f64) -> Option<UnityRead> {
        let (_, at) = self.due.as_ref()?;
        if now_ms < *at {
            return None;
        }
        self.due.take().map(|(read, _)| read)
    }

    /// Whether the reply to `read` still applies: no later change or other project came in
    /// between, and both the project that is open now and the one the backend answered for
    /// are the one the read was made for. Only the Unity metadata is then taken over, so that
    /// whatever else changed in the project meanwhile (its name) stays.
    pub fn accepts(&self, read: &UnityRead, open_root: Option<&str>, answered_root: Option<&str>) -> bool {
        read.generation == self.generation
            && open_root == Some(read.root.as_str())
            && answered_root == Some(read.root.as_str())
    }
}

/// The value of `key` in a section of the settings; null without the section or the key.
fn setting(section: Option<&Value>, key: &str) -> Value {
    section.and_then(|section| section.get(key)).cloned().unwrap_or_default()
}

/// The settings that decide which entries list as excluded: when they change, every loaded
/// folder is read again.
pub fn exclusion_settings(settings: &Value) -> Value {
    let structure = settings.get("projectStructure");
    let exclude = setting(settings.get("files"), "exclude");
    let marks = ["excluded", "unmarked", "excludeFiles"].map(|key| setting(structure, key));
    Value::from(std::iter::once(exclude).chain(marks).collect::<Vec<Value>>())
}

/// The excluded roots detected from build files (`target`, `node_modules`, …): when they change
/// for the same project, every loaded folder is read again.
pub fn detected_excluded(detected: &[RootFolder]) -> Vec<&str> {
    detected.iter().filter(|root| root.kind == FolderRole::Excluded).map(|root| root.path.as_str()).collect()
}

/// The roots the tree lists for the content roots of Project Structure, after the project
/// folder, and the module groups of the build, before it. None of either for a project that is
/// attached to another one's workspace.
pub fn content_root_contributions(content_roots: &[String], module_groups: Vec<ExtraRoot>) -> Vec<ProjectViewRoot> {
    let content = content_roots.iter().zip(0..).map(|(path, index)| {
        let name = Some(name_of(path)).filter(|name| !name.is_empty());
        let root = ExtraRoot {
            id: Some(format!("contentRoot:{path}")),
            label: name.unwrap_or(path.as_str()).to_owned(),
            detail: Some(path.clone()),
            icon: "module".to_owned(),
            path: Some(path.clone()),
            ..ExtraRoot::default()
        };
        ProjectViewRoot { root, order: -100 + index, scratches: false }
    });
    let groups = module_groups.into_iter().zip(0..).map(|(group, index)| {
        let root = ExtraRoot { before_project: true, ..group };
        ProjectViewRoot { root, order: -200 + index, scratches: false }
    });
    content.chain(groups).collect()
}

/// Whether the project owns its content roots and module groups: it is not a workspace, or it is
/// the workspace's primary project.
pub fn owns_content_roots(root: &str, members: Option<&[String]>) -> bool {
    members.is_none_or(|members| members.first().is_some_and(|primary| primary == root))
}

/// The paths of the changed files of the list `list_id`, or of all lists; with `existing`
/// without the files deleted in the working tree. In code-unit order.
pub fn local_changed_paths(lists: &[LocalChangelist], list_id: Option<&str>, existing: bool) -> Vec<String> {
    let listed = lists.iter().filter(|list| list_id.is_none_or(|id| id.is_empty() || list.id == id));
    let files = listed.flat_map(|list| &list.files).filter(|file| !existing || !file.deleted);
    let mut paths: Vec<String> = files.map(|file| file.path.clone()).collect();
    paths.sort_by(|a, b| collate::compare_code_units(a, b));
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_ops::changelists::LocalChange;
    use serde_json::json;

    fn unity(projects: &[&str]) -> UnityProjectInfo {
        UnityProjectInfo {
            editor_version: Some("2022.3.62f1".to_owned()),
            solutions: Vec::new(),
            projects: projects.iter().map(|project| (*project).to_owned()).collect(),
        }
    }

    fn game<'a>(root: &'a str, name: &'a str, unity: Option<&'a UnityProjectInfo>) -> Option<OpenedProject<'a>> {
        Some(OpenedProject { root, name, members: None, unity })
    }

    const GENERATED: [&str; 1] = ["/game/Assembly-CSharp.csproj"];

    // src/features/project-tree/unityRefresh.test.ts ›
    // refreshes generated C# files through backend events without overwriting a project rename
    #[test]
    fn refreshes_generated_c_sharp_files_without_overwriting_a_project_rename() {
        let before = unity(&[]);
        let mut state = ProjectViewState::default();
        sync_project(&mut state, game("/game", "Game", Some(&before)), None);
        let mut refresh = UnityRefresh::default();
        refresh.fs_changed("/game", &GENERATED, 0.0);
        assert_eq!(refresh.expire(100.0), None);
        let read = refresh.expire(160.0).expect("the metadata is read once the changes rest");
        assert_eq!(refresh.expire(400.0), None);
        // The project is renamed while the read is under way.
        sync_project(&mut state, game("/game", "Renamed", Some(&before)), None);
        assert!(refresh.accepts(&read, Some("/game"), Some("/game")));
        let fresh = unity(&["Assembly-CSharp.csproj"]);
        sync_project(&mut state, game("/game", "Renamed", Some(&fresh)), None);
        assert_eq!(state.unity.as_ref().map(|unity| unity.projects.clone()), Some(fresh.projects.clone()));
        assert_eq!((state.name.as_str(), state.mode), ("Renamed", ViewMode::Unity));
    }

    // src/features/project-tree/unityRefresh.test.ts › discards a late reply after switching to another project
    #[test]
    fn discards_a_late_reply_after_switching_to_another_project() {
        let before = unity(&[]);
        let mut state = ProjectViewState::default();
        sync_project(&mut state, game("/game", "Game", Some(&before)), None);
        let mut refresh = UnityRefresh::default();
        refresh.fs_changed("/game", &GENERATED, 0.0);
        let read = refresh.expire(160.0).expect("the metadata is read once the changes rest");
        refresh.project_changed();
        sync_project(&mut state, game("/app", "Game", None), None);
        assert!(!refresh.accepts(&read, Some("/app"), Some("/game")));
        assert_eq!((state.root.as_deref(), state.unity.is_none()), (Some("/app"), true));
        assert_eq!(state.mode, ViewMode::Project);
    }

    // src/features/project-tree/viewModes.test.ts › localChangedPaths ›
    // lists change-list files, all lists or one, optionally without deleted ones
    #[test]
    fn lists_change_list_files_all_lists_or_one_optionally_without_deleted_ones() {
        let change = |path: &str, deleted: bool| LocalChange { path: path.to_owned(), deleted };
        let lists = [
            LocalChangelist {
                id: "default".to_owned(),
                name: "Changes".to_owned(),
                active: true,
                files: vec![change("/p/b.ts", false), change("/p/gone.ts", true)],
            },
            LocalChangelist {
                id: "ui".to_owned(),
                name: "UI".to_owned(),
                active: false,
                files: vec![change("/p/a.ts", false)],
            },
        ];
        assert_eq!(local_changed_paths(&lists, None, true), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(local_changed_paths(&lists, None, false), ["/p/a.ts", "/p/b.ts", "/p/gone.ts"]);
        assert_eq!(local_changed_paths(&lists, Some("ui"), false), ["/p/a.ts"]);
        assert!(local_changed_paths(&[], None, false).is_empty());
    }

    // Not from a spec: only Unity's own folders, its version file and the project files at the
    // top count as its metadata; a later change postpones the read.
    #[test]
    fn only_unitys_project_files_trigger_a_read_and_a_later_change_postpones_it() {
        for path in ["/game/Assets", "/game/ProjectSettings/ProjectVersion.txt", "/game/Game.SLN", "/game/a.slnx"] {
            assert!(is_unity_metadata("/game", path), "{path}");
        }
        for path in ["/game/Assets/Player.cs", "/game/src/a.csproj", "/gamer/Assets", "/game/notes.txt"] {
            assert!(!is_unity_metadata("/game", path), "{path}");
        }
        let mut refresh = UnityRefresh::default();
        refresh.fs_changed("/game", &["/game/Assets/Player.cs"], 0.0);
        assert_eq!(refresh.next_deadline(), None);
        refresh.fs_changed("/game", &GENERATED, 0.0);
        refresh.fs_changed("/game", &GENERATED, 100.0);
        assert_eq!(refresh.next_deadline(), Some(100.0 + UNITY_REFRESH_DELAY_MS));
        let stale = UnityRead { root: "/game".to_owned(), generation: 1 };
        assert!(!refresh.accepts(&stale, Some("/game"), Some("/game")));
    }

    // Not from a spec: content roots are listed after the project and module groups before it,
    // a workspace member contributes none, and exclusion settings are compared as a whole.
    #[test]
    fn contributes_content_roots_and_module_groups_and_compares_exclusion_settings() {
        let id = Some("gradleGroup:Shop".to_owned());
        let group = ExtraRoot { id, label: "Shop".to_owned(), ..ExtraRoot::default() };
        let roots = content_root_contributions(&["/w/shared".to_owned()], vec![group]);
        let shown: Vec<(&str, i32, bool)> =
            roots.iter().map(|root| (root.root.label.as_str(), root.order, root.root.before_project)).collect();
        assert_eq!(shown, [("shared", -100, false), ("Shop", -200, true)]);
        let members = ["/a".to_owned(), "/b".to_owned()];
        assert!(owns_content_roots("/a", Some(&members)) && owns_content_roots("/p", None));
        assert!(!owns_content_roots("/b", Some(&members)));

        let settings = json!({ "files": { "exclude": ["*.log"] }, "projectStructure": { "excluded": ["out"] } });
        assert_eq!(exclusion_settings(&settings), json!([["*.log"], ["out"], null, null]));
        let detected = [
            RootFolder { path: "target".to_owned(), kind: FolderRole::Excluded, detected_from: None },
            RootFolder { path: "src".to_owned(), kind: FolderRole::Source, detected_from: None },
        ];
        assert_eq!(detected_excluded(&detected), ["target"]);
    }
}
