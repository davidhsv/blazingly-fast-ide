//! Read-only build context shared by file and folder presentation consumers: which folders are
//! Maven or Gradle modules, and whether the build uses Spring.

use serde::Deserialize;

/// How long to wait after a build file changed before reading the build model again, so that a
/// burst of changes costs one read.
pub const BUILD_FILE_RELOAD_DELAY_MS: u64 = 100;

/// What presentation reads of the backend's build model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct BuildModel {
    pub maven: Option<MavenModel>,
    /// Every independently linked Gradle build.
    #[serde(default)]
    pub gradle: Vec<GradleModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MavenModel {
    pub projects: Vec<MavenProject>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MavenProject {
    /// Project-relative directory; `""` is the project root.
    pub dir: String,
    pub dependencies: Vec<MavenArtifact>,
    pub plugins: Vec<MavenArtifact>,
}

/// A dependency or a plugin of a Maven project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MavenArtifact {
    pub group_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GradleModel {
    /// The root project first, then subprojects.
    pub projects: Vec<GradleProject>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GradleProject {
    /// Project-relative directory.
    pub dir: String,
    pub tasks: Vec<GradleTask>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct GradleTask {
    pub name: String,
}

/// The build tool a module belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
    Gradle,
}

/// Module directories (project-relative; `""` is the project root) with their build tool, in the
/// order the build model lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Modules(Vec<(String, BuildTool)>);

impl Modules {
    pub fn get(&self, dir: &str) -> Option<BuildTool> {
        self.0.iter().find(|(listed, _)| listed == dir).map(|(_, tool)| *tool)
    }

    pub fn contains(&self, dir: &str) -> bool {
        self.get(dir).is_some()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sets the build tool of `dir`. A directory already listed keeps its place.
    pub fn insert(&mut self, dir: &str, tool: BuildTool) {
        match self.0.iter().position(|(listed, _)| listed == dir) {
            Some(index) => self.0[index].1 = tool,
            None => self.0.push((dir.to_owned(), tool)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, BuildTool)> {
        self.0.iter().map(|(dir, tool)| (dir.as_str(), *tool))
    }

    pub fn dirs(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(dir, _)| dir.as_str())
    }
}

/// The build context presentation needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildPresentation {
    pub modules: Modules,
    /// Whether any module uses Spring: `application*.yml` and the like are then Spring configuration.
    pub spring: bool,
}

fn is_spring(artifact: &MavenArtifact) -> bool {
    artifact.group_id.starts_with("org.springframework")
}

/// The modules of every Maven and Gradle build in `model`, Maven's first, and whether one of them
/// depends on Spring, applies a Spring plugin or has Gradle's `bootRun` task.
pub fn build_presentation(model: &BuildModel) -> BuildPresentation {
    let maven: Vec<&MavenProject> = model.maven.iter().flat_map(|maven| &maven.projects).collect();
    let gradle: Vec<&GradleProject> = model.gradle.iter().flat_map(|build| &build.projects).collect();
    let mut modules = Modules::default();
    for project in &maven {
        modules.insert(&project.dir, BuildTool::Maven);
    }
    for project in &gradle {
        modules.insert(&project.dir, BuildTool::Gradle);
    }
    let spring = maven.iter().any(|project| project.dependencies.iter().chain(&project.plugins).any(is_spring))
        || gradle.iter().any(|project| project.tasks.iter().any(|task| task.name == "bootRun"));
    BuildPresentation { modules, spring }
}

/// A Maven / Gradle module folder below the project root: IDEA shows its name in bold.
pub fn is_module_folder(rel: &str, root: bool, modules: &Modules) -> bool {
    !root && !rel.is_empty() && modules.contains(rel)
}

/// The icon of a module folder: the build tool's own glyph for the project root, its module
/// glyph below it. `None` for a folder that is no module.
pub fn module_folder_icon(rel: &str, root: bool, modules: &Modules) -> Option<&'static str> {
    Some(match (modules.get(rel)?, root) {
        (BuildTool::Maven, true) => "maven-project",
        (BuildTool::Maven, false) => "maven-module",
        (BuildTool::Gradle, true) => "gradle-project",
        (BuildTool::Gradle, false) => "gradle-module",
    })
}

/// A build file whose edits change the modules or the Spring detection.
pub fn is_build_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(name, "pom.xml" | "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts")
}

/// Whether a change of these files calls for reading the build model again.
pub fn changes_build_model<P: AsRef<str>>(paths: &[P]) -> bool {
    paths.iter().any(|path| is_build_file(path.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::BuildTool::{Gradle, Maven};
    use super::*;
    use serde_json::{Value, json};

    fn model(partial: Value) -> BuildModel {
        serde_json::from_value(partial).expect("a build model")
    }

    fn modules(listed: &[(&str, BuildTool)]) -> Modules {
        let mut modules = Modules::default();
        for (dir, tool) in listed {
            modules.insert(dir, *tool);
        }
        modules
    }

    // src/platform/projectPresentation.test.ts › uses distinct %s root and nested module glyphs
    #[test]
    fn uses_distinct_root_and_nested_module_glyphs() {
        for (tool, project, module) in
            [(Maven, "maven-project", "maven-module"), (Gradle, "gradle-project", "gradle-module")]
        {
            let modules = modules(&[("", tool), ("app", tool)]);
            assert_eq!(module_folder_icon("", true, &modules), Some(project));
            assert_eq!(module_folder_icon("app", false, &modules), Some(module));
            assert_eq!(module_folder_icon("plain", false, &modules), None);
        }
    }

    // src/platform/projectPresentation.test.ts › marks nested module folders, not the project root or plain folders
    #[test]
    fn marks_nested_module_folders_not_the_project_root_or_plain_folders() {
        let modules = modules(&[("", Maven), ("core", Maven), ("services/api", Gradle)]);
        assert!(is_module_folder("core", false, &modules));
        assert!(is_module_folder("services/api", false, &modules));
        assert!(!is_module_folder("", true, &modules));
        assert!(!is_module_folder("services", false, &modules));
        assert!(!is_module_folder("core/src", false, &modules));
    }

    // src/platform/projectPresentation.test.ts › includes Maven aggregators and modules without source roots
    #[test]
    fn includes_maven_aggregators_and_modules_without_source_roots() {
        let context = build_presentation(&model(json!({
            "maven": {
                "projects": [
                    { "dir": "", "dependencies": [], "plugins": [] },
                    { "dir": "app", "dependencies": [{ "groupId": "org.springframework.boot" }], "plugins": [] },
                ],
            },
        })));
        assert_eq!(context.modules.iter().collect::<Vec<_>>(), [("", Maven), ("app", Maven)]);
        assert!(context.spring);
    }

    // src/platform/projectPresentation.test.ts › includes Gradle root and nested project directories
    #[test]
    fn includes_gradle_root_and_nested_project_directories() {
        let context = build_presentation(&model(json!({
            "gradle": [{ "projects": [{ "dir": "", "tasks": [] }, { "dir": "lib/core", "tasks": [] }] }],
        })));
        assert_eq!(context.modules.iter().collect::<Vec<_>>(), [("", Gradle), ("lib/core", Gradle)]);
        assert!(!context.spring);
    }

    // src/platform/projectPresentation.test.ts › detects Spring build plugins and Gradle Boot tasks
    #[test]
    fn detects_spring_build_plugins_and_gradle_boot_tasks() {
        let maven = model(json!({
            "maven": {
                "projects": [
                    { "dir": "", "dependencies": [], "plugins": [{ "groupId": "org.springframework.boot" }] },
                ],
            },
        }));
        assert!(build_presentation(&maven).spring);
        let gradle = model(json!({ "gradle": [{ "projects": [{ "dir": "", "tasks": [{ "name": "bootRun" }] }] }] }));
        assert!(build_presentation(&gradle).spring);
    }

    // src/platform/projectPresentation.test.ts › keeps plain folders plain
    #[test]
    fn keeps_plain_folders_plain() {
        assert_eq!(build_presentation(&model(json!({}))).modules.len(), 0);
    }

    // src/platform/projectPresentation.test.ts › includes modules and Spring tasks from every independently linked Gradle build
    #[test]
    fn includes_modules_and_spring_tasks_from_every_independently_linked_gradle_build() {
        let context = build_presentation(&model(json!({
            "gradle": [
                { "projects": [{ "dir": "first", "tasks": [] }, { "dir": "first/app", "tasks": [] }] },
                { "projects": [{ "dir": "second", "tasks": [{ "name": "bootRun" }] }] },
            ],
        })));
        assert_eq!(context.modules.dirs().collect::<Vec<_>>(), ["first", "first/app", "second"]);
        assert!(context.spring);
    }

    /// Not from a spec: the files whose change reloads the build model.
    #[test]
    fn reloads_for_build_files_only() {
        for path in ["pom.xml", "/p/core/pom.xml", "/p/build.gradle", "/p/app/build.gradle.kts", "/p/settings.gradle"] {
            assert!(is_build_file(path), "{path}");
        }
        for path in ["/p/my-pom.xml", "/p/pom.xml.bak", "/p/gradle/libs.gradle", r"C:\p\pom.xml"] {
            assert!(!is_build_file(path), "{path}");
        }
        assert!(changes_build_model(&["/p/src/A.java", "/p/pom.xml"]));
        assert!(!changes_build_model(&["/p/src/A.java"]));
        assert!(!changes_build_model::<&str>(&[]));
    }
}
