//! "Mark Directory as" of the project view's context menu: edits the folder marks of
//! Settings › Project Structure.

use crate::platform::project_roots::{FolderRole, ProjectStructure, RootFolder, mark_folder, root_role};
use crate::platform::settings::SettingsWrite;
use crate::platform::virtual_docs::relative_path;
use serde_json::Value;

/// The effective role of the folder `path`: marked by the user or detected from a build file.
/// `roots` are the effective roots of the project at `project_root`.
pub fn directory_role(roots: &[RootFolder], project_root: &str, path: &str) -> Option<FolderRole> {
    root_role(roots, &relative_path(project_root, path))
}

/// The lists of the Project Structure settings, by settings key, in a stable order.
fn folder_lists(structure: &ProjectStructure) -> [(&'static str, &[String]); 6] {
    [
        ("projectStructure.sourceRoots", structure.source_roots.as_slice()),
        ("projectStructure.testRoots", structure.test_roots.as_slice()),
        ("projectStructure.resourceRoots", structure.resource_roots.as_slice()),
        ("projectStructure.testResourceRoots", structure.test_resource_roots.as_slice()),
        ("projectStructure.excluded", structure.excluded.as_slice()),
        ("projectStructure.unmarked", structure.unmarked.as_slice()),
    ]
}

/// The settings to write, in the project's scope, to give `path` the `role` (`None`: unmark it).
/// Unmarking a root detected from a build file keeps it unmarked across re-detection. Nothing is
/// written for a folder outside the project. `structure` holds the marks as the settings have
/// them, `detected` the roots detected from build files.
pub fn mark_directory(
    project_root: &str,
    path: &str,
    role: Option<FolderRole>,
    structure: &ProjectStructure,
    detected: &[RootFolder],
) -> Vec<SettingsWrite> {
    let rel = relative_path(project_root, path);
    if rel == path {
        return Vec::new();
    }
    let next = mark_folder(structure, &rel, role, detected);
    let (before, after) = (folder_lists(structure), folder_lists(&next));
    let changed = before.iter().zip(after).filter(|((_, was), (_, now))| was != now);
    changed.map(|(_, (key, folders))| SettingsWrite { key, value: Value::from(folders.to_vec()) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Not from a spec: marking writes only the lists that change, and unmarking a detected root
    // records it as unmarked.
    #[test]
    fn writes_only_the_lists_that_change() {
        let detected_from = Some("pom.xml".to_owned());
        let detected = [RootFolder { path: "src".to_owned(), kind: FolderRole::Source, detected_from }];
        let roots = detected.clone();
        assert_eq!(directory_role(&roots, "/p", "/p/src"), Some(FolderRole::Source));
        assert_eq!(directory_role(&roots, "/p", "/p/docs"), None);

        let none = ProjectStructure::default();
        let marked = mark_directory("/p", "/p/gen", Some(FolderRole::Test), &none, &detected);
        assert_eq!(marked, [SettingsWrite { key: "projectStructure.testRoots", value: json!(["gen"]) }]);
        let unmarked = mark_directory("/p", "/p/src", None, &none, &detected);
        assert_eq!(unmarked, [SettingsWrite { key: "projectStructure.unmarked", value: json!(["src"]) }]);
        // The detected role needs no mark, and a folder outside the project gets none.
        assert!(mark_directory("/p", "/p/src", Some(FolderRole::Source), &none, &detected).is_empty());
        assert!(mark_directory("/p", "/elsewhere/src", Some(FolderRole::Source), &none, &detected).is_empty());
    }
}
