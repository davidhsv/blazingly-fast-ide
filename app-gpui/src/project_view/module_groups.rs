//! IDEA's module groups in the Project view, for Gradle builds whose modules live outside the
//! project folder (`includeFlat`, computed `projectDir`s, `includeBuild`): the modules are
//! grouped by their qualified names (`<root project>.billing.core`), as IntelliJ's
//! `AbstractProjectNode.modulesAndGroups` and `ModuleGroup` do.

use crate::platform::collate::{self, Sensitivity};
use crate::project_tree_model::ExtraRoot;
use crate::project_tree_paths::{is_same_or_inside, name_of};
use ide_build::GradleProject;
use std::cmp::Ordering;

struct Module {
    /// The qualified name, in segments: `<root project>`, then the path segments.
    name: Vec<String>,
    dir: String,
}

impl Module {
    fn qualified(&self) -> String {
        self.name.join(".")
    }

    /// The group the module is in: its name without the last segment.
    fn group(&self) -> &[String] {
        self.name.split_last().map_or(&[], |(_, group)| group)
    }
}

fn is_absolute(path: &str) -> bool {
    let drive = matches!(path.as_bytes(), [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic());
    path.starts_with('/') || drive
}

fn is_child(parent: &[String], group: &[String]) -> bool {
    group.len() > parent.len() && group.starts_with(parent)
}

fn by_label(a: &ExtraRoot, b: &ExtraRoot) -> Ordering {
    collate::compare_with(&a.label, &b.label, false, Sensitivity::Base)
}

struct Groups<'a> {
    modules: Vec<Module>,
    /// Tells the nodes of independently linked builds apart.
    build_dir: Option<&'a str>,
}

impl Groups<'_> {
    fn identity(&self, name: &str) -> String {
        match self.build_dir {
            Some(build_dir) => serde_json::json!([build_dir, name]).to_string(),
            None => name.to_owned(),
        }
    }

    fn is_module_path(&self, path: &[String]) -> bool {
        self.modules.iter().any(|module| module.name == path)
    }

    fn module_node(&self, module: &Module) -> ExtraRoot {
        let folder = Some(name_of(&module.dir)).filter(|name| !name.is_empty());
        let label = folder.unwrap_or(module.dir.as_str()).to_owned();
        let qualified = module.qualified();
        ExtraRoot {
            id: Some(self.identity(&qualified)),
            detail: (qualified != label).then_some(qualified),
            label,
            icon: "gradle-module".to_owned(),
            path: Some(module.dir.clone()),
            ..ExtraRoot::default()
        }
    }

    /// The node of `group`: its subgroups, then its modules, each by label.
    fn group_node(&self, group: &[String]) -> ExtraRoot {
        // Direct subgroups that no module is named like; those fold into this group.
        let mut child_groups: Vec<Vec<String>> = Vec::new();
        for module in &self.modules {
            let of = module.group();
            if !is_child(group, of) {
                continue;
            }
            let direct = of[..=group.len()].to_vec();
            if !self.is_module_path(&direct) && !child_groups.contains(&direct) {
                child_groups.push(direct);
            }
        }
        // This group's modules, and those under a subgroup named like a module.
        let member = |module: &&Module| {
            let of = module.group();
            of == group || (is_child(group, of) && self.is_module_path(&of[..=group.len()]))
        };
        let mut dirs: Vec<&str> = Vec::new();
        let mut modules: Vec<ExtraRoot> = Vec::new();
        for module in self.modules.iter().filter(member) {
            if !dirs.contains(&module.dir.as_str()) {
                dirs.push(&module.dir);
                modules.push(self.module_node(module));
            }
        }
        let mut children: Vec<ExtraRoot> = child_groups.iter().map(|child| self.group_node(child)).collect();
        children.sort_by(by_label);
        modules.sort_by(by_label);
        children.extend(modules);
        ExtraRoot {
            id: Some(self.identity(&format!("gradleGroup:{}", group.join(".")))),
            label: group.last().cloned().unwrap_or_default(),
            icon: "gradle-project".to_owned(),
            children: Some(children),
            ..ExtraRoot::default()
        }
    }
}

/// The group nodes to list beside the project folder for the `projects` of a Gradle build whose
/// root project is named `root_name`. None while the project folder is the only top-level module
/// root (every module is inside it), as in IDEA. `build_dir` names the build when several are
/// linked independently.
pub fn gradle_module_groups(
    projects: &[GradleProject],
    root_name: &str,
    project_root: &str,
    build_dir: Option<&str>,
) -> Vec<ExtraRoot> {
    let modules: Vec<Module> = projects
        .iter()
        .map(|project| {
            let path: Vec<&str> = project.path.split(':').filter(|segment| !segment.is_empty()).collect();
            // An included build's modules are named after that build (`lib.core`); buildSrc's and
            // the opened build's after the root project (`shop.buildSrc`).
            let own = project.build.as_deref().is_none_or(|build| build.is_empty() || build == ":buildSrc");
            let qualified = if own { [&[root_name][..], &path[..]].concat().join(".") } else { path.join(".") };
            let dir = if is_absolute(&project.dir) {
                project.dir.clone()
            } else if project.dir.is_empty() {
                project_root.to_owned()
            } else {
                format!("{project_root}/{}", project.dir)
            };
            Module { name: qualified.split('.').map(str::to_owned).collect(), dir }
        })
        .collect();
    let groups = Groups { modules, build_dir };
    // A content root is top level when no other content root contains it.
    let contained = |dir: &str| {
        let mut roots = std::iter::once(project_root).chain(groups.modules.iter().map(|module| module.dir.as_str()));
        roots.any(|root| root != dir && is_same_or_inside(dir, root))
    };
    let top_level: Vec<&Module> =
        groups.modules.iter().filter(|module| module.dir == project_root || !contained(&module.dir)).collect();
    if top_level.len() <= 1 {
        return Vec::new();
    }
    // Ungrouped top-level modules as they are (the project folder is shown anyway), and one node
    // for the common group path of the grouped ones.
    let mut loose: Vec<ExtraRoot> = top_level
        .iter()
        .filter(|module| module.group().is_empty() && module.dir != project_root)
        .map(|module| groups.module_node(module))
        .collect();
    loose.sort_by(by_label);
    let grouped: Vec<&[String]> =
        top_level.iter().map(|module| module.group()).filter(|group| !group.is_empty()).collect();
    let Some(first) = grouped.first() else { return loose };
    let shared = |group: &&[String]| first.iter().zip(group.iter()).take_while(|(a, b)| a == b).count();
    let common = &first[..grouped.iter().map(shared).min().unwrap_or(0)];
    let mut tops: Vec<&[String]> = Vec::new();
    if common.is_empty() {
        for group in &grouped {
            if !tops.iter().any(|top| top[0] == group[0]) {
                tops.push(&group[..1]);
            }
        }
    } else {
        tops.push(common);
    }
    let mut nodes: Vec<ExtraRoot> = tops.into_iter().map(|group| groups.group_node(group)).collect();
    nodes.extend(loose);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tree_model::{DEFAULT_TREE_OPTIONS, Listings, ProjectNode, build_model, with_extra_roots};
    use ide_core::ignored::IgnoredNames;
    use ide_vfs::{EntryKind, FileEntry};
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn project(path: &str, dir: &str) -> GradleProject {
        GradleProject { path: path.to_owned(), dir: dir.to_owned(), ..GradleProject::default() }
    }

    fn included(path: &str, dir: &str, build: &str) -> GradleProject {
        GradleProject { build: Some(build.to_owned()), ..project(path, dir) }
    }

    /// A node as the tree shows it: its label, its detail and its children, where it has them.
    fn shape(root: &ExtraRoot) -> Value {
        let mut shown = json!({ "label": root.label });
        if let Some(detail) = &root.detail {
            shown["detail"] = json!(detail);
        }
        if let Some(children) = &root.children {
            shown["children"] = Value::from(children.iter().map(shape).collect::<Vec<Value>>());
        }
        shown
    }

    fn shapes(roots: &[ExtraRoot]) -> Value {
        Value::from(roots.iter().map(shape).collect::<Vec<Value>>())
    }

    /// `includeFlat` sibling modules, one with layer modules nested in its folder.
    fn flat() -> Vec<GradleProject> {
        vec![
            project(":", ""),
            project(":common", "/w/common"),
            project(":billing", "/w/billing"),
            project(":billing:core", "/w/billing/modules/core"),
            project(":billing:api", "/w/billing/modules/api"),
        ]
    }

    // src/features/project-tree/moduleGroups.test.ts › gradleModuleGroups (IDEA's module groups) ›
    // lists every module of the group, nested ones flattened, as IntelliJ shows it
    #[test]
    fn lists_every_module_of_the_group_nested_ones_flattened_as_intellij_shows_it() {
        let expected = json!([{
            "label": "Shop",
            "children": [
                { "label": "api", "detail": "Shop.billing.api" },
                { "label": "billing", "detail": "Shop.billing" },
                { "label": "common", "detail": "Shop.common" },
                { "label": "core", "detail": "Shop.billing.core" },
            ],
        }]);
        assert_eq!(shapes(&gradle_module_groups(&flat(), "Shop", "/w/shop", None)), expected);
    }

    // src/features/project-tree/moduleGroups.test.ts › gradleModuleGroups (IDEA's module groups) ›
    // shows no group while every module lives in the project folder
    #[test]
    fn shows_no_group_while_every_module_lives_in_the_project_folder() {
        let inside = [project(":", ""), project(":app", "app"), project(":libs:core", "libs/core")];
        assert!(gradle_module_groups(&inside, "demo", "/w/demo", None).is_empty());
    }

    // src/features/project-tree/moduleGroups.test.ts › gradleModuleGroups (IDEA's module groups) ›
    // keeps a subgroup that no module is named like
    #[test]
    fn keeps_a_subgroup_that_no_module_is_named_like() {
        let projects = [
            project(":", ""),
            project(":svc", "/w/svc"),
            project(":libs:a", "/w/libs/a"),
            project(":libs:b", "/w/libs/b"),
        ];
        let expected = json!([{
            "label": "R",
            "children": [
                {
                    "label": "libs",
                    "children": [
                        { "label": "a", "detail": "R.libs.a" },
                        { "label": "b", "detail": "R.libs.b" },
                    ],
                },
                { "label": "svc", "detail": "R.svc" },
            ],
        }]);
        assert_eq!(shapes(&gradle_module_groups(&projects, "R", "/w/r", None)), expected);
    }

    // src/features/project-tree/moduleGroups.test.ts › gradleModuleGroups (IDEA's module groups) ›
    // names an included build's modules after it and lists it as an ungrouped module
    #[test]
    fn names_an_included_builds_modules_after_it_and_lists_it_as_an_ungrouped_module() {
        let mut projects = vec![
            project(":", ""),
            project(":app", "app"),
            included(":buildSrc", "buildSrc", ":buildSrc"),
            included(":lib", "/w/lib", ":lib"),
            included(":lib:core", "/w/lib/core", ":lib"),
        ];
        let lib_alone = gradle_module_groups(&projects, "Shop", "/w/shop", None);
        assert_eq!(shapes(&lib_alone), json!([{ "label": "lib" }]));
        // Beside includeFlat modules: the group (every `Shop.*` module), then the included build.
        projects.push(project(":svc", "/w/svc"));
        let expected = json!([
            {
                "label": "Shop",
                "children": [
                    { "label": "app", "detail": "Shop.app" },
                    { "label": "buildSrc", "detail": "Shop.buildSrc" },
                    { "label": "svc", "detail": "Shop.svc" },
                ],
            },
            { "label": "lib" },
        ]);
        assert_eq!(shapes(&gradle_module_groups(&projects, "Shop", "/w/shop", None)), expected);
    }

    fn dir(path: &str) -> FileEntry {
        FileEntry {
            name: name_of(path).to_owned(),
            path: path.to_owned().into(),
            kind: EntryKind::Dir,
            size: 0,
            mtime_ms: 1.0,
            ignored: false,
            excluded: false,
            hidden: false,
            target_kind: None,
        }
    }

    // src/features/project-tree/moduleGroups.test.ts › module group nodes in the tree ›
    // lists the group above the project and keeps a nested module whose parent is expanded
    #[test]
    fn lists_the_group_above_the_project_and_keeps_a_nested_module_whose_parent_is_expanded() {
        let mut listings = Listings::new();
        listings.insert("/w/billing".to_owned(), vec![dir("/w/billing/modules")]);
        listings.insert("/w/billing/modules".to_owned(), vec![dir("/w/billing/modules/core")]);
        let ignored = IgnoredNames::default();
        let base = build_model("/p", "p", &Listings::new(), &DEFAULT_TREE_OPTIONS, &ignored, None);
        let module = |label: &str, path: &str| ExtraRoot {
            label: label.to_owned(),
            icon: "gradle-module".to_owned(),
            path: Some(path.to_owned()),
            ..ExtraRoot::default()
        };
        let group = ExtraRoot {
            label: "Shop".to_owned(),
            icon: "gradle-project".to_owned(),
            before_project: true,
            children: Some(vec![module("billing", "/w/billing"), module("core", "/w/billing/modules/core")]),
            ..ExtraRoot::default()
        };
        let m = with_extra_roots(base, &[group], &listings, &DEFAULT_TREE_OPTIONS, &ignored, None);
        let labels =
            |nodes: &[Arc<ProjectNode>]| -> Vec<String> { nodes.iter().map(|node| node.label.clone()).collect() };
        assert_eq!(labels(&m.roots), ["Shop", "p"]);
        let modules = m.children.get(&m.roots[0].key).expect("the group lists its modules");
        assert_eq!(labels(modules), ["billing", "core"]);
    }

    // src/features/project-tree/moduleGroups.test.ts ›
    // namespaces module group identities by independently linked build
    #[test]
    fn namespaces_module_group_identities_by_independently_linked_build() {
        let projects = [project(":", "/w/first"), project(":app", "/w/shared")];
        let first = gradle_module_groups(&projects, "same", "/w/project", Some("first"));
        let second = gradle_module_groups(&projects, "same", "/w/project", Some("second"));
        assert!(!first.is_empty());
        assert_ne!(first[0].id, second[0].id);
        assert_eq!(shapes(&first), shapes(&second));
    }
}
