//! The tree of the Maven and Gradle tool windows and the run configurations its nodes start.
//!
//! Maven (as IDEA shows it): Profiles, then one node per top-level project; each project has
//! Lifecycle, Plugins (expandable to goals), Dependencies (transitive after a sync), Run
//! Configurations and its modules. Gradle: the root project of each linked build with Tasks
//! (grouped), Run Configurations and its subprojects. Everything here is pure: the workbench feeds
//! the backend's [`BuildModel`] and the project's run configurations.

use crate::platform::collate;
use ide_build::{
    BuildModel, BuildTool, GradleModel, GradleProject, GradleTask, MavenDependency, MavenModel, MavenPlugin,
    MavenProfile, MavenProject,
};
use ide_run::RunConfig;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

/// The Maven tool window's id.
pub const MAVEN_TOOL_WINDOW: &str = "maven";
/// The Gradle tool window's id.
pub const GRADLE_TOOL_WINDOW: &str = "gradle";

/// Where the toolbar toggles and profile choices of a project are kept, followed by its root.
pub const SETTINGS_KEY: &str = "openidea.buildTools:";
/// Where the command lines run from Execute Goal / Task are kept: this, the tool, `:`, the root.
pub const HISTORY_KEY: &str = "openidea.buildTools.history:";
/// How many command lines the history keeps.
const HISTORY_LIMIT: usize = 20;

/// The default lifecycle's phases the tree lists.
pub const LIFECYCLE: [&str; 9] =
    ["clean", "validate", "compile", "test", "package", "verify", "install", "site", "deploy"];

/// Every phase of the clean, default and site lifecycles (Execute Maven Goal completion).
pub const ALL_PHASES: [&str; 30] = [
    "pre-clean",
    "clean",
    "post-clean",
    "validate",
    "initialize",
    "generate-sources",
    "process-sources",
    "generate-resources",
    "process-resources",
    "compile",
    "process-classes",
    "generate-test-sources",
    "process-test-sources",
    "generate-test-resources",
    "process-test-resources",
    "test-compile",
    "process-test-classes",
    "test",
    "prepare-package",
    "package",
    "pre-integration-test",
    "integration-test",
    "post-integration-test",
    "verify",
    "install",
    "deploy",
    "pre-site",
    "site",
    "post-site",
    "site-deploy",
];

/// Maven goals that run tests, so the configuration shows its results in the test tree.
const MAVEN_TEST_GOALS: [&str; 7] =
    ["test", "verify", "integration-test", "install", "deploy", "surefire:test", "failsafe:integration-test"];

/// Gradle tasks that run tests.
const GRADLE_TEST_TASKS: [&str; 4] = ["test", "check", "build", "integrationTest"];

/// The tool window's id of `tool`.
pub fn tool_id(tool: BuildTool) -> &'static str {
    match tool {
        BuildTool::Maven => MAVEN_TOOL_WINDOW,
        BuildTool::Gradle => GRADLE_TOOL_WINDOW,
    }
}

/// The tool of tool window `id`.
pub fn tool_of_window(id: &str) -> Option<BuildTool> {
    match id {
        MAVEN_TOOL_WINDOW => Some(BuildTool::Maven),
        GRADLE_TOOL_WINDOW => Some(BuildTool::Gradle),
        _ => None,
    }
}

// -------------------------------------------------------------------------------------------------
// Options

/// Options applied to every Maven run (toolbar toggles, Profiles checkboxes).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MavenRunOptions {
    pub offline: bool,
    pub skip_tests: bool,
    /// Explicit profile choices by [`profile_key`]. Unlisted profiles keep their default.
    pub profiles: BTreeMap<String, bool>,
}

/// Options applied to every Gradle run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GradleRunOptions {
    pub offline: bool,
}

/// The toolbar toggles and profile choices of one project, as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildSettings {
    pub maven: MavenRunOptions,
    pub gradle: GradleRunOptions,
}

impl BuildSettings {
    /// The stored settings; the defaults when nothing (or nothing readable) is stored.
    pub fn parse(stored: Option<&str>) -> Self {
        stored.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default()
    }

    /// The settings as stored.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// The storage key of the command line history of `tool` in the project at `root`.
pub fn history_key(tool: BuildTool, root: &str) -> String {
    format!("{HISTORY_KEY}{}:{root}", tool_id(tool))
}

/// The stored command line history, newest first.
pub fn parse_history(stored: Option<&str>) -> Vec<String> {
    stored.and_then(|text| serde_json::from_str::<Vec<String>>(text).ok()).unwrap_or_default()
}

/// `history` with `line` first, once, at most [`HISTORY_LIMIT`] long.
pub fn pushed_history(history: &[String], line: &str) -> Vec<String> {
    std::iter::once(line.to_owned())
        .chain(history.iter().filter(|kept| *kept != line).cloned())
        .take(HISTORY_LIMIT)
        .collect()
}

// -------------------------------------------------------------------------------------------------
// Nodes

/// What a node of the tree stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Profiles { project_root: Option<String> },
    Profile(MavenProfile),
    MavenProject { pom: String, name: String, aggregated: bool },
    Lifecycle { pom: String },
    Phase { pom: String, phase: String },
    Plugins { pom: String },
    Plugin { pom: String, plugin: MavenPlugin },
    Goal { pom: String, prefix: String, goal: String },
    Dependencies { pom: String, dependencies: Vec<MavenDependency> },
    Dependency(MavenDependency),
    RunConfigs(Vec<RunConfig>),
    RunConfig(RunConfig),
    GradleProject { build: String, path: String, name: String, build_file: Option<String> },
    Tasks { build: String, path: String },
    TaskGroup { build: String, path: String, group: String, tasks: Vec<GradleTask> },
    Task { build: String, path: String, task: GradleTask },
}

/// A node of the tree: its key is stable while the model is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildNode {
    pub key: String,
    pub kind: NodeKind,
}

impl BuildNode {
    fn new(key: impl Into<String>, kind: NodeKind) -> Self {
        Self { key: key.into(), kind }
    }
}

/// What the tree is built from: the model, the project's run configurations and its root.
#[derive(Clone, Copy)]
pub struct TreeSource<'a> {
    pub model: &'a BuildModel,
    pub configs: &'a [RunConfig],
    pub root: &'a str,
}

impl<'a> TreeSource<'a> {
    fn maven(&self) -> Option<&'a MavenModel> {
        self.model.maven.as_ref()
    }

    fn maven_project(&self, pom: &str) -> Option<&'a MavenProject> {
        self.maven()?.projects.iter().find(|p| p.pom == pom)
    }

    fn gradle_build(&self, dir: &str) -> Option<&'a GradleModel> {
        self.model.gradle.iter().find(|g| g.dir == dir)
    }

    fn gradle_project(&self, dir: &str, path: &str) -> Option<(&'a GradleModel, &'a GradleProject)> {
        let build = self.gradle_build(dir)?;
        build.projects.iter().find(|p| p.path == path).map(|project| (build, project))
    }
}

fn maven_project_key(pom: &str) -> String {
    format!("mvn:{pom}")
}

fn maven_project_node(project: &MavenProject) -> BuildNode {
    BuildNode::new(
        maven_project_key(&project.pom),
        NodeKind::MavenProject {
            pom: project.pom.clone(),
            name: project.display_name.clone(),
            aggregated: project.aggregated,
        },
    )
}

/// `group:artifact[:classifier][:version]`.
pub fn dependency_label(d: &MavenDependency) -> String {
    let mut parts = vec![d.group_id.as_str(), d.artifact_id.as_str()];
    parts.extend(d.classifier.as_deref());
    parts.extend(d.version.as_deref());
    parts.join(":")
}

/// `prefix (group:artifact[:version])`.
pub fn plugin_label(p: &MavenPlugin) -> String {
    let version = p.version.as_deref().map(|v| format!(":{v}")).unwrap_or_default();
    format!("{} ({}:{}{version})", p.prefix, p.group_id, p.artifact_id)
}

/// Profile choices in different workspace members are independent.
pub fn profile_key(profile: &MavenProfile) -> String {
    match &profile.project_root {
        Some(root) => serde_json::to_string(&[root.as_str(), profile.id.as_str()]).unwrap_or_default(),
        None => profile.id.clone(),
    }
}

/// Whether a profile is active for runs: the explicit choice, else `activeByDefault`.
pub fn profile_checked(profile: &MavenProfile, choices: &BTreeMap<String, bool>) -> bool {
    choices.get(&profile_key(profile)).copied().unwrap_or(profile.active_by_default)
}

/// The `-P` value for the choices that differ from the defaults (`a,!b`), or `None`.
pub fn profiles_arg<'a>(
    profiles: impl IntoIterator<Item = &'a MavenProfile>,
    choices: &BTreeMap<String, bool>,
) -> Option<String> {
    let ids: Vec<String> = profiles
        .into_iter()
        .filter_map(|p| match (profile_checked(p, choices), p.active_by_default) {
            (true, false) => Some(p.id.clone()),
            (false, true) => Some(format!("!{}", p.id)),
            _ => None,
        })
        .collect();
    (!ids.is_empty()).then(|| ids.join(","))
}

/// The profile list of a sync (`-P` entries).
pub fn profiles_list<'a>(
    profiles: impl IntoIterator<Item = &'a MavenProfile>,
    choices: &BTreeMap<String, bool>,
) -> Vec<String> {
    profiles_arg(profiles, choices).map(|arg| arg.split(',').map(str::to_owned).collect()).unwrap_or_default()
}

/// The roots of the Maven tree: a Profiles node per project root that has profiles, then the
/// top-level projects.
pub fn maven_roots(model: &MavenModel) -> Vec<BuildNode> {
    let mut roots: Vec<Option<String>> = Vec::new();
    for profile in &model.profiles {
        if !roots.contains(&profile.project_root) {
            roots.push(profile.project_root.clone());
        }
    }
    let profiles = roots.into_iter().map(|project_root| {
        let key = format!("mvn:profiles{}", project_root.as_deref().unwrap_or_default());
        BuildNode::new(key, NodeKind::Profiles { project_root })
    });
    profiles.chain(model.projects.iter().filter(|p| !p.aggregated).map(maven_project_node)).collect()
}

/// The project-relative `pom` of a project from a project-relative or absolute path.
pub fn maven_pom_at_path(projects: &[MavenProject], root: Option<&str>, path: &str) -> Option<String> {
    projects
        .iter()
        .find(|p| p.pom == path || root.is_some_and(|root| format!("{root}/{}", p.pom) == path))
        .map(|p| p.pom.clone())
}

/// A run configuration's working directory as written, `/`-separated.
fn config_cwd(config: &RunConfig) -> &str {
    config.cwd.as_deref().and_then(|cwd| cwd.to_str()).unwrap_or_default()
}

/// `/`-separated, without a leading `./` (or `.`) and trailing slashes.
fn normalize_dir(cwd: &str) -> String {
    let slashed = cwd.replace('\\', "/");
    let rest = slashed.strip_prefix("./").or_else(|| slashed.strip_prefix('.')).unwrap_or(&slashed);
    rest.trim_end_matches('/').to_owned()
}

/// Whether `path` is absolute on any platform (`/x`, `C:\x`, `C:/x`).
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
}

/// The Maven run configurations of a project: `maven` ones whose directory is the project's.
pub fn maven_run_configs(project: &MavenProject, configs: &[RunConfig]) -> Vec<RunConfig> {
    configs.iter().filter(|c| c.kind == "maven" && normalize_dir(config_cwd(c)) == project.dir).cloned().collect()
}

/// The Gradle run configurations under `model`: a nested independent build owns its own runs.
pub fn gradle_run_configs(
    model: &GradleModel,
    models: &[GradleModel],
    configs: &[RunConfig],
    root: &str,
) -> Vec<RunConfig> {
    let absolute = |path: &str| {
        if is_absolute(path) { normalize_dir(path) } else { normalize_dir(&format!("{root}/{path}")) }
    };
    configs
        .iter()
        .filter(|c| {
            if c.kind != "gradle" {
                return false;
            }
            let cwd = absolute(config_cwd(c));
            let contains = |dir: &str| cwd == dir || cwd.starts_with(&format!("{dir}/"));
            let owner_depth = |build: &GradleModel| {
                let dir = absolute(&build.dir);
                if contains(&dir) { dir.split('/').count() } else { 0 }
            };
            let mut owners: Vec<&GradleModel> = models
                .iter()
                .filter(|build| {
                    std::iter::once(&build.dir)
                        .chain(build.projects.iter().map(|p| &p.dir))
                        .any(|d| contains(&absolute(d)))
                })
                .collect();
            owners.sort_by_key(|build| std::cmp::Reverse(owner_depth(*build)));
            owners.first().is_some_and(|owner| owner.dir == model.dir)
        })
        .cloned()
        .collect()
}

fn run_config_nodes(parent: &str, configs: &[RunConfig]) -> Vec<BuildNode> {
    configs
        .iter()
        .map(|config| BuildNode::new(format!("{parent}:{}", config.id), NodeKind::RunConfig(config.clone())))
        .collect()
}

/// The resolved dependencies; occurrences Maven omitted are left to the Dependency Analyzer.
fn dependency_nodes(parent: &str, dependencies: &[MavenDependency]) -> Vec<BuildNode> {
    dependencies
        .iter()
        .enumerate()
        .filter(|(_, d)| d.omitted.is_none())
        .map(|(i, d)| {
            BuildNode::new(format!("{parent}/{}:{}:{i}", d.group_id, d.artifact_id), NodeKind::Dependency(d.clone()))
        })
        .collect()
}

/// The children of a Maven node; `None` for a leaf.
pub fn maven_children(node: &BuildNode, src: TreeSource) -> Option<Vec<BuildNode>> {
    let model = src.maven()?;
    let key = &node.key;
    match &node.kind {
        NodeKind::Profiles { project_root } => Some(
            model
                .profiles
                .iter()
                .filter(|p| p.project_root == *project_root)
                .map(|p| BuildNode::new(format!("mvn:profile:{}", profile_key(p)), NodeKind::Profile(p.clone())))
                .collect(),
        ),
        NodeKind::MavenProject { pom, .. } => {
            let project = src.maven_project(pom)?;
            let mut out = vec![
                BuildNode::new(format!("{key}:lifecycle"), NodeKind::Lifecycle { pom: pom.clone() }),
                BuildNode::new(format!("{key}:plugins"), NodeKind::Plugins { pom: pom.clone() }),
            ];
            if !project.dependencies.is_empty() {
                let dependencies = project.dependencies.clone();
                out.push(BuildNode::new(
                    format!("{key}:deps"),
                    NodeKind::Dependencies { pom: pom.clone(), dependencies },
                ));
            }
            let runs = maven_run_configs(project, src.configs);
            if !runs.is_empty() {
                out.push(BuildNode::new(format!("{key}:runs"), NodeKind::RunConfigs(runs)));
            }
            out.extend(project.modules.iter().filter_map(|module| src.maven_project(module)).map(maven_project_node));
            Some(out)
        }
        NodeKind::Lifecycle { pom } => Some(
            LIFECYCLE
                .iter()
                .map(|phase| {
                    BuildNode::new(
                        format!("{key}:{phase}"),
                        NodeKind::Phase { pom: pom.clone(), phase: (*phase).to_owned() },
                    )
                })
                .collect(),
        ),
        NodeKind::Plugins { pom } => Some(
            src.maven_project(pom)?
                .plugins
                .iter()
                .map(|plugin| {
                    BuildNode::new(
                        format!("{key}:{}:{}", plugin.group_id, plugin.artifact_id),
                        NodeKind::Plugin { pom: pom.clone(), plugin: plugin.clone() },
                    )
                })
                .collect(),
        ),
        NodeKind::Plugin { pom, plugin } => Some(
            plugin
                .goals
                .iter()
                .map(|goal| {
                    BuildNode::new(
                        format!("{key}:{goal}"),
                        NodeKind::Goal { pom: pom.clone(), prefix: plugin.prefix.clone(), goal: goal.clone() },
                    )
                })
                .collect(),
        ),
        NodeKind::Dependencies { dependencies, .. } => Some(dependency_nodes(key, dependencies)),
        NodeKind::Dependency(dependency) => {
            let children = dependency_nodes(key, &dependency.children);
            (!children.is_empty()).then_some(children)
        }
        NodeKind::RunConfigs(configs) => Some(run_config_nodes(key, configs)),
        _ => None,
    }
}

/// Tasks grouped by their group (`other` for none): groups alphabetical with `other` last, tasks
/// alphabetical, as IDEA lists them.
pub fn gradle_task_groups(project: &GradleProject) -> Vec<(String, Vec<GradleTask>)> {
    let mut groups: Vec<(String, Vec<GradleTask>)> = Vec::new();
    for task in &project.tasks {
        let group = if task.group.is_empty() { "other" } else { task.group.as_str() };
        match groups.iter_mut().find(|(name, _)| name == group) {
            Some((_, tasks)) => tasks.push(task.clone()),
            None => groups.push((group.to_owned(), vec![task.clone()])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| match (a.as_str(), b.as_str()) {
        ("other", "other") => std::cmp::Ordering::Equal,
        ("other", _) => std::cmp::Ordering::Greater,
        (_, "other") => std::cmp::Ordering::Less,
        _ => collate::compare(a, b),
    });
    for (_, tasks) in &mut groups {
        tasks.sort_by(|a, b| collate::compare(&a.name, &b.name));
    }
    groups
}

/// The identity of a project within one independently linked build: `gradle:["dir",":path"]`.
pub fn gradle_project_key(build_dir: &str, path: &str) -> String {
    format!("gradle:{}", serde_json::to_string(&[build_dir, path]).unwrap_or_default())
}

/// The project of `key`; without a key, the first project of the first build.
pub fn gradle_project_for_key<'a>(
    models: &'a [GradleModel],
    key: Option<&str>,
) -> Option<(&'a GradleModel, &'a GradleProject)> {
    let found = models.iter().find_map(|build| {
        build
            .projects
            .iter()
            .find(|p| Some(gradle_project_key(&build.dir, &p.path).as_str()) == key)
            .map(|p| (build, p))
    });
    if found.is_some() || key.is_some() {
        return found;
    }
    let build = models.first()?;
    build.projects.first().map(|project| (build, project))
}

fn gradle_project_node(build: &GradleModel, project: &GradleProject) -> BuildNode {
    BuildNode::new(
        gradle_project_key(&build.dir, &project.path),
        NodeKind::GradleProject {
            build: build.dir.clone(),
            path: project.path.clone(),
            name: project.name.clone(),
            build_file: project.build_file.clone(),
        },
    )
}

/// The root project of each linked build.
pub fn gradle_roots(models: &[GradleModel]) -> Vec<BuildNode> {
    models
        .iter()
        .filter_map(|build| {
            let root = build.projects.iter().find(|p| p.path == ":").or_else(|| build.projects.first())?;
            Some(gradle_project_node(build, root))
        })
        .collect()
}

/// The direct subprojects of `path` (`:a` and `:b` under `:`, `:a:x` under `:a`); a project whose
/// parent is not a project (`include(":a:b")` without `:a`) is shown under the root.
pub fn gradle_subprojects<'a>(model: &'a GradleModel, path: &str) -> Vec<&'a GradleProject> {
    model
        .projects
        .iter()
        .filter(|p| {
            if p.path == path || p.path == ":" {
                return false;
            }
            let parent = match p.path.rfind(':') {
                Some(0) | None => ":",
                Some(at) => &p.path[..at],
            };
            parent == path || (path == ":" && !model.projects.iter().any(|q| q.path == parent))
        })
        .collect()
}

/// The children of a Gradle node; `None` for a leaf.
pub fn gradle_children(node: &BuildNode, src: TreeSource) -> Option<Vec<BuildNode>> {
    let key = &node.key;
    match &node.kind {
        NodeKind::GradleProject { build, path, .. } => {
            let model = src.gradle_build(build)?;
            let mut out = vec![BuildNode::new(
                format!("{key}:tasks"),
                NodeKind::Tasks { build: build.clone(), path: path.clone() },
            )];
            if path == ":" {
                let runs = gradle_run_configs(model, &src.model.gradle, src.configs, src.root);
                if !runs.is_empty() {
                    out.push(BuildNode::new(format!("{key}:runs"), NodeKind::RunConfigs(runs)));
                }
            }
            out.extend(gradle_subprojects(model, path).into_iter().map(|sub| gradle_project_node(model, sub)));
            Some(out)
        }
        NodeKind::Tasks { build, path } => {
            let (_, project) = src.gradle_project(build, path)?;
            Some(
                gradle_task_groups(project)
                    .into_iter()
                    .map(|(group, tasks)| {
                        BuildNode::new(
                            format!("{key}:{group}"),
                            NodeKind::TaskGroup { build: build.clone(), path: path.clone(), group, tasks },
                        )
                    })
                    .collect(),
            )
        }
        NodeKind::TaskGroup { build, path, tasks, .. } => Some(
            tasks
                .iter()
                .map(|task| {
                    BuildNode::new(
                        format!("{key}:{}", task.name),
                        NodeKind::Task { build: build.clone(), path: path.clone(), task: task.clone() },
                    )
                })
                .collect(),
        ),
        NodeKind::RunConfigs(configs) => Some(run_config_nodes(key, configs)),
        _ => None,
    }
}

/// The roots of `tool`'s tree.
pub fn roots(tool: BuildTool, model: &BuildModel) -> Vec<BuildNode> {
    match tool {
        BuildTool::Maven => model.maven.as_ref().map(maven_roots).unwrap_or_default(),
        BuildTool::Gradle => gradle_roots(&model.gradle),
    }
}

/// The children of a node of `tool`'s tree.
pub fn children(tool: BuildTool, node: &BuildNode, src: TreeSource) -> Option<Vec<BuildNode>> {
    match tool {
        BuildTool::Maven => maven_children(node, src),
        BuildTool::Gradle => gradle_children(node, src),
    }
}

pub fn node_label(node: &BuildNode) -> String {
    match &node.kind {
        NodeKind::Profiles { .. } => "Profiles".to_owned(),
        NodeKind::Profile(profile) => profile.id.clone(),
        NodeKind::MavenProject { name, .. } | NodeKind::GradleProject { name, .. } => name.clone(),
        NodeKind::Lifecycle { .. } => "Lifecycle".to_owned(),
        NodeKind::Phase { phase, .. } => phase.clone(),
        NodeKind::Plugins { .. } => "Plugins".to_owned(),
        NodeKind::Plugin { plugin, .. } => plugin_label(plugin),
        NodeKind::Goal { prefix, goal, .. } => format!("{prefix}:{goal}"),
        NodeKind::Dependencies { .. } => "Dependencies".to_owned(),
        NodeKind::Dependency(dependency) => dependency_label(dependency),
        NodeKind::RunConfigs(_) => "Run Configurations".to_owned(),
        NodeKind::RunConfig(config) => config.name.clone(),
        NodeKind::Tasks { .. } => "Tasks".to_owned(),
        NodeKind::TaskGroup { group, .. } => group.clone(),
        NodeKind::Task { task, .. } => task.name.clone(),
    }
}

pub fn node_icon(node: &BuildNode) -> &'static str {
    match &node.kind {
        NodeKind::Profiles { .. } | NodeKind::Profile(_) => "maven-profiles",
        NodeKind::MavenProject { aggregated: true, .. } => "maven-module",
        NodeKind::MavenProject { .. } => "maven-project",
        NodeKind::Lifecycle { .. } | NodeKind::Plugins { .. } | NodeKind::RunConfigs(_) | NodeKind::Tasks { .. } => {
            "folder"
        }
        NodeKind::Phase { .. } => "maven-goal",
        NodeKind::Plugin { .. } => "maven-plugin",
        NodeKind::Goal { .. } => "maven-plugin-goal",
        NodeKind::Dependencies { .. } => "folder-library",
        NodeKind::Dependency(_) => "library",
        NodeKind::RunConfig(config) if config.kind == "gradle" => "build-task",
        NodeKind::RunConfig(_) => "maven-goal",
        NodeKind::GradleProject { .. } => "file-gradle",
        NodeKind::TaskGroup { .. } => "build-task-group",
        NodeKind::Task { .. } => "build-task",
    }
}

/// Nodes that run something on a double click or Enter.
pub fn is_runnable(node: &BuildNode) -> bool {
    matches!(node.kind, NodeKind::Phase { .. } | NodeKind::Goal { .. } | NodeKind::Task { .. } | NodeKind::RunConfig(_))
}

/// The Maven `pom` or Gradle project key of the project a node belongs to: where Execute Goal /
/// Task runs once the node is selected.
pub fn node_project(node: &BuildNode) -> Option<String> {
    match &node.kind {
        NodeKind::MavenProject { pom, .. }
        | NodeKind::Lifecycle { pom }
        | NodeKind::Phase { pom, .. }
        | NodeKind::Plugins { pom }
        | NodeKind::Plugin { pom, .. }
        | NodeKind::Goal { pom, .. }
        | NodeKind::Dependencies { pom, .. } => Some(pom.clone()),
        NodeKind::GradleProject { build, path, .. }
        | NodeKind::Tasks { build, path }
        | NodeKind::TaskGroup { build, path, .. }
        | NodeKind::Task { build, path, .. } => Some(gradle_project_key(build, path)),
        _ => None,
    }
}

/// The text after a node's label: a workspace member's folder, a dependency's scope, a task's
/// description.
pub fn node_detail(node: &BuildNode) -> Option<String> {
    match &node.kind {
        NodeKind::Profiles { project_root: Some(root) } => root.rsplit('/').next().map(str::to_owned),
        NodeKind::Dependency(d) if d.scope != "compile" => {
            Some(format!("({}{})", d.scope, if d.optional { ", optional" } else { "" }))
        }
        NodeKind::Dependency(d) if d.optional => Some("(optional)".to_owned()),
        NodeKind::Task { task, .. } => task.description.clone(),
        _ => None,
    }
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRow {
    pub node: BuildNode,
    pub depth: usize,
    pub label: String,
    pub icon: &'static str,
    pub detail: Option<String>,
    pub expandable: bool,
    pub expanded: bool,
    /// The `test` phase while Skip Tests is on: drawn struck through.
    pub skipped: bool,
    /// A profile's checkbox.
    pub checked: Option<bool>,
}

/// The visible lines of `tool`'s tree with `expanded` open, top to bottom.
pub fn visible_rows(
    tool: BuildTool,
    src: TreeSource,
    expanded: &HashSet<String>,
    settings: &BuildSettings,
) -> Vec<BuildRow> {
    fn walk(
        tool: BuildTool,
        nodes: Vec<BuildNode>,
        depth: usize,
        src: TreeSource,
        expanded: &HashSet<String>,
        settings: &BuildSettings,
        out: &mut Vec<BuildRow>,
    ) {
        for node in nodes {
            let kids = children(tool, &node, src).filter(|kids| !kids.is_empty());
            let open = kids.is_some() && expanded.contains(&node.key);
            let skipped = tool == BuildTool::Maven
                && settings.maven.skip_tests
                && matches!(&node.kind, NodeKind::Phase { phase, .. } if phase == "test");
            let checked = match &node.kind {
                NodeKind::Profile(profile) => Some(profile_checked(profile, &settings.maven.profiles)),
                _ => None,
            };
            out.push(BuildRow {
                depth,
                label: node_label(&node),
                icon: node_icon(&node),
                detail: node_detail(&node),
                expandable: kids.is_some(),
                expanded: open,
                skipped,
                checked,
                node,
            });
            if open && let Some(kids) = kids {
                walk(tool, kids, depth + 1, src, expanded, settings, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(tool, roots(tool, src.model), 0, src, expanded, settings, &mut out);
    out
}

/// The keys opened when a tree is first shown: the top-level projects.
pub fn initial_expanded(tool: BuildTool, model: &BuildModel) -> HashSet<String> {
    match tool {
        BuildTool::Maven => model
            .maven
            .iter()
            .flat_map(|m| &m.projects)
            .filter(|p| !p.aggregated)
            .map(|p| maven_project_key(&p.pom))
            .collect(),
        BuildTool::Gradle => gradle_roots(&model.gradle).into_iter().map(|node| node.key).collect(),
    }
}

/// The keys of every expandable node below `roots` (Expand All). Dependencies are not opened
/// (transitive trees can be huge); their "Dependencies" folder is.
pub fn expandable_keys(
    roots: Vec<BuildNode>,
    children: &dyn Fn(&BuildNode) -> Option<Vec<BuildNode>>,
) -> HashSet<String> {
    fn walk(nodes: Vec<BuildNode>, children: &dyn Fn(&BuildNode) -> Option<Vec<BuildNode>>, out: &mut HashSet<String>) {
        for node in nodes {
            if matches!(node.kind, NodeKind::Dependency(_)) {
                continue;
            }
            let Some(kids) = children(&node).filter(|kids| !kids.is_empty()) else { continue };
            out.insert(node.key.clone());
            walk(kids, children, out);
        }
    }
    let mut out = HashSet::new();
    walk(roots, children, &mut out);
    out
}

// -------------------------------------------------------------------------------------------------
// Run configurations

/// Splits a command line into arguments; double and single quotes group.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            has = true;
        } else if c.is_whitespace() {
            if has || !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            has = false;
        } else {
            current.push(c);
        }
    }
    if has || !current.is_empty() {
        out.push(current);
    }
    out
}

/// Whether `word` names `tool`'s launcher: `mvn`, `mvnw`, `./mvnw`, `mvnw.cmd` / `gradle`,
/// `gradlew`, `./gradlew`, `gradlew.bat`.
pub fn is_launcher(tool: BuildTool, word: &str) -> bool {
    let (name, suffix) = match tool {
        BuildTool::Maven => ("mvn", ".cmd"),
        BuildTool::Gradle => ("gradle", ".bat"),
    };
    let rest = word.strip_prefix("./").unwrap_or(word);
    let Some(rest) = rest.strip_prefix(name) else { return false };
    let rest = rest.strip_prefix('w').unwrap_or(rest);
    rest.is_empty() || rest == suffix
}

/// The goals typed in Execute Maven Goal (a leading `mvn` / `mvnw` is left out).
pub fn parse_goal_line(line: &str) -> Vec<String> {
    parse_line(BuildTool::Maven, line)
}

/// The tasks typed in Execute Gradle Task (a leading `gradle` / `gradlew` is left out).
pub fn parse_task_line(line: &str) -> Vec<String> {
    parse_line(BuildTool::Gradle, line)
}

fn parse_line(tool: BuildTool, line: &str) -> Vec<String> {
    let mut args = split_args(line.trim());
    if args.first().is_some_and(|first| is_launcher(tool, first)) {
        args.remove(0);
    }
    args
}

/// `goals` with the toolbar options and the changed profiles, unless the goals set them.
pub fn maven_args<'a>(
    goals: &[String],
    profiles: impl IntoIterator<Item = &'a MavenProfile>,
    options: &MavenRunOptions,
) -> Vec<String> {
    let mut args = goals.to_vec();
    let has = |args: &[String], flag: &str| args.iter().any(|a| a == flag || a.starts_with(&format!("{flag}=")));
    if options.offline && !has(&args, "-o") && !has(&args, "--offline") {
        args.push("-o".to_owned());
    }
    if options.skip_tests && !has(&args, "-DskipTests") {
        args.push("-DskipTests".to_owned());
    }
    let explicit =
        args.iter().any(|a| a.starts_with("-P") || a == "--activate-profiles" || a.starts_with("--activate-profiles="));
    if let Some(profiles) = profiles_arg(profiles, &options.profiles).filter(|_| !explicit) {
        args.push(format!("-P{profiles}"));
    }
    args
}

/// Whether `command` is a Maven wrapper script.
fn is_maven_wrapper(command: &str) -> bool {
    let name = command.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    matches!(name.as_str(), "mvnw" | "mvnw.cmd" | "mvnw.bat")
}

/// A configuration of no saved options: what the generated ones start from.
fn generated(id: String, name: String, kind: &str, command: String, args: Vec<String>, dir: &str) -> RunConfig {
    RunConfig {
        id,
        name,
        kind: kind.to_owned(),
        command,
        args,
        cwd: (!dir.is_empty()).then(|| ide_core::IpcPath(PathBuf::from(dir))),
        env: BTreeMap::new(),
        detected: false,
        is_test: false,
        debug_adapter: None,
        allow_parallel: false,
        extras: Default::default(),
    }
}

/// IDEA names Maven configurations `<project> [goal1,goal2]`.
pub fn maven_config_name(project: &MavenProject, goals: &[String]) -> String {
    let goals: Vec<&str> = goals.iter().map(String::as_str).filter(|g| !g.starts_with('-')).collect();
    format!("{} [{}]", project.display_name, goals.join(","))
}

/// The configuration that runs `goals` in `project`. Its id is `maven:goal:<pom>:<goals>`: the
/// backend knows a generated goal by it and puts Settings › Maven's options ahead of the goals.
pub fn maven_goal_config(
    project: &MavenProject,
    goals: &[String],
    model: &MavenModel,
    options: &MavenRunOptions,
) -> RunConfig {
    let profiles = model.profiles.iter().filter(|p| p.project_root == project.project_root);
    let args = maven_args(goals, profiles, options);
    let is_test = !options.skip_tests
        && !args.iter().any(|a| a == "-DskipTests")
        && goals.iter().any(|g| MAVEN_TEST_GOALS.contains(&g.as_str()));
    let command = if is_maven_wrapper(&project.command) { project.command.clone() } else { "mvn".to_owned() };
    let id = format!("maven:goal:{}:{}", project.pom, goals.join(" "));
    RunConfig {
        env: project.env.clone(),
        is_test,
        ..generated(id, maven_config_name(project, goals), "maven", command, args, &project.dir)
    }
}

/// `:app:test` for task `test` of `:app`, plain `test` in the root project.
pub fn gradle_task_path(project: &GradleProject, task: &str) -> String {
    if project.path == ":" { task.to_owned() } else { format!("{}:{task}", project.path) }
}

/// The configuration that runs `tasks` of `project` from the build's directory with its launcher.
pub fn gradle_task_config(
    model: &GradleModel,
    project: &GradleProject,
    tasks: &[String],
    options: &GradleRunOptions,
) -> RunConfig {
    let mut args: Vec<String> = tasks
        .iter()
        .map(|t| if t.starts_with('-') || t.starts_with(':') { t.clone() } else { gradle_task_path(project, t) })
        .collect();
    if options.offline && !args.iter().any(|a| a == "--offline") {
        args.push("--offline".to_owned());
    }
    let names: Vec<&str> = tasks.iter().map(String::as_str).filter(|t| !t.starts_with('-')).collect();
    let is_test = names.iter().any(|t| GRADLE_TEST_TASKS.contains(&t.rsplit(':').next().unwrap_or_default()));
    let id = format!(
        "gradle:task:{}:{}",
        serde_json::to_string(&[model.dir.as_str(), project.path.as_str()]).unwrap_or_default(),
        tasks.join(" ")
    );
    let name = format!("{} [{}]", project.name, names.join(","));
    RunConfig {
        env: model.env.clone(),
        is_test,
        ..generated(id, name, "gradle", model.command.clone(), args, &model.dir)
    }
}

/// A saved Gradle configuration started from the tree, with the toolbar's Offline applied.
pub fn with_gradle_options(config: RunConfig, options: &GradleRunOptions) -> RunConfig {
    if !options.offline || config.args.iter().any(|a| a == "--offline") {
        return config;
    }
    let mut config = config;
    config.args.push("--offline".to_owned());
    config
}

/// The configuration that runnable `nodes` start: a saved one as it is, or the goals / tasks of
/// the first node's project (the selected nodes of other projects are left out).
pub fn node_config(nodes: &[BuildNode], src: TreeSource, settings: &BuildSettings) -> Option<RunConfig> {
    let first = nodes.first()?;
    match &first.kind {
        NodeKind::RunConfig(config) if config.kind == "gradle" => {
            Some(with_gradle_options(config.clone(), &settings.gradle))
        }
        NodeKind::RunConfig(config) => Some(config.clone()),
        NodeKind::Phase { pom, .. } | NodeKind::Goal { pom, .. } => {
            let maven = src.maven()?;
            let project = src.maven_project(pom)?;
            let goals: Vec<String> = nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    NodeKind::Phase { pom: of, phase } if of == pom => Some(phase.clone()),
                    NodeKind::Goal { pom: of, prefix, goal } if of == pom => Some(format!("{prefix}:{goal}")),
                    _ => None,
                })
                .collect();
            Some(maven_goal_config(project, &goals, maven, &settings.maven))
        }
        NodeKind::Task { build, path, .. } => {
            let (model, project) = src.gradle_project(build, path)?;
            let tasks: Vec<String> = nodes
                .iter()
                .filter_map(|node| match &node.kind {
                    NodeKind::Task { build: b, path: p, task } if b == build && p == path => Some(task.name.clone()),
                    _ => None,
                })
                .collect();
            Some(gradle_task_config(model, project, &tasks, &settings.gradle))
        }
        _ => None,
    }
}

// -------------------------------------------------------------------------------------------------
// Execute Goal / Task completion

/// A completion of the command line's last word, or a recent command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The whole line after accepting.
    pub line: String,
    /// The completed word (the whole line for a recent one).
    pub label: String,
    /// `recent`, `phase`, a goal's plugin or a task's group.
    pub detail: Option<String>,
}

/// The detail of a recent command line.
pub const RECENT: &str = "recent";

impl Completion {
    pub fn is_recent(&self) -> bool {
        self.detail.as_deref() == Some(RECENT)
    }
}

/// The line before its last word, and that word.
fn split_last_word(line: &str) -> (&str, &str) {
    let word: usize = line.chars().rev().take_while(|c| !c.is_whitespace()).map(char::len_utf8).sum();
    line.split_at(line.len() - word)
}

/// The recent lines `line` begins, offered while nothing but the line's first word is typed.
fn recent(line: &str, history: &[String]) -> Vec<Completion> {
    let typed = line.to_lowercase();
    history
        .iter()
        .filter(|h| h.to_lowercase().starts_with(&typed) && *h != line)
        .map(|h| Completion { line: h.clone(), label: h.clone(), detail: Some(RECENT.to_owned()) })
        .collect()
}

/// Completes the last word of `line` with lifecycle phases and the `prefix:goal`s of `plugins`,
/// after the recent lines it begins.
pub fn goal_completions(line: &str, plugins: &[MavenPlugin], history: &[String]) -> Vec<Completion> {
    let (head, word) = split_last_word(line);
    let word = word.to_lowercase();
    let mut out = if head.trim().is_empty() { recent(line, history) } else { Vec::new() };
    if word.starts_with('-') {
        return out;
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |out: &mut Vec<Completion>, label: &str, detail: &str| {
        if !label.to_lowercase().starts_with(&word) || !seen.insert(label.to_owned()) {
            return;
        }
        out.push(Completion {
            line: format!("{head}{label}"),
            label: label.to_owned(),
            detail: Some(detail.to_owned()),
        });
    };
    for phase in LIFECYCLE.into_iter().chain(ALL_PHASES) {
        push(&mut out, phase, "phase");
    }
    let mut goals: Vec<(String, &str)> = plugins
        .iter()
        .flat_map(|p| p.goals.iter().map(move |g| (format!("{}:{g}", p.prefix), p.artifact_id.as_str())))
        .collect();
    goals.sort_by(|a, b| collate::compare(&a.0, &b.0));
    for (label, plugin) in goals {
        push(&mut out, &label, plugin);
    }
    out
}

/// Completes the last word of `line` with the tasks of `model` (names and `:project:task` paths):
/// the recent lines first, then the shortest.
pub fn task_completions(line: &str, model: &GradleModel, history: &[String]) -> Vec<Completion> {
    let (head, word) = split_last_word(line);
    let word = word.to_lowercase();
    let mut out = if head.trim().is_empty() { recent(line, history) } else { Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    for project in &model.projects {
        for task in &project.tasks {
            let label = if project.path == ":" { task.name.clone() } else { format!("{}:{}", project.path, task.name) };
            let matches = label.to_lowercase().starts_with(&word) || task.name.to_lowercase().starts_with(&word);
            if !matches || !seen.insert(label.clone()) {
                continue;
            }
            out.push(Completion { line: format!("{head}{label}"), label, detail: Some(task.group.clone()) });
        }
    }
    out.sort_by(|a, b| match (a.is_recent(), b.is_recent()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.label.len().cmp(&b.label.len()).then_with(|| collate::compare(&a.label, &b.label)),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The aggregator `demo:parent` of the specs; other projects override what differs.
    fn base() -> MavenProject {
        MavenProject {
            pom: "pom.xml".to_owned(),
            group_id: "demo".to_owned(),
            artifact_id: "parent".to_owned(),
            version: "1.0".to_owned(),
            packaging: "pom".to_owned(),
            display_name: "parent".to_owned(),
            java_version: Some(21),
            command: "/p/mvnw".to_owned(),
            env: BTreeMap::from([("JAVA_HOME".to_owned(), "/jdk21".to_owned())]),
            ..MavenProject::default()
        }
    }

    fn surefire() -> MavenPlugin {
        MavenPlugin {
            group_id: "org.apache.maven.plugins".to_owned(),
            artifact_id: "maven-surefire-plugin".to_owned(),
            version: Some("3.2.5".to_owned()),
            prefix: "surefire".to_owned(),
            goals: vec!["help".to_owned(), "test".to_owned()],
        }
    }

    fn dependency(artifact: &str, children: Vec<MavenDependency>) -> MavenDependency {
        MavenDependency {
            group_id: "org.junit.jupiter".to_owned(),
            artifact_id: artifact.to_owned(),
            version: Some("5.10.2".to_owned()),
            kind: "jar".to_owned(),
            scope: "test".to_owned(),
            children,
            ..MavenDependency::default()
        }
    }

    fn maven() -> MavenModel {
        let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        MavenModel {
            projects: vec![
                MavenProject {
                    modules: strings(&["core/pom.xml", "app/pom.xml"]),
                    display_name: "Multi Demo".to_owned(),
                    profiles: strings(&["dev", "ci"]),
                    ..base()
                },
                MavenProject {
                    pom: "core/pom.xml".to_owned(),
                    dir: "core".to_owned(),
                    artifact_id: "core".to_owned(),
                    display_name: "core".to_owned(),
                    packaging: "jar".to_owned(),
                    aggregated: true,
                    plugins: vec![surefire()],
                    dependencies: vec![dependency("junit-jupiter", vec![dependency("junit-jupiter-api", Vec::new())])],
                    ..base()
                },
                MavenProject {
                    pom: "app/pom.xml".to_owned(),
                    dir: "app".to_owned(),
                    artifact_id: "app".to_owned(),
                    display_name: "Demo App".to_owned(),
                    packaging: "jar".to_owned(),
                    aggregated: true,
                    ..base()
                },
            ],
            profiles: vec![
                MavenProfile { project_root: None, id: "dev".to_owned(), active_by_default: true },
                MavenProfile { project_root: None, id: "ci".to_owned(), active_by_default: false },
            ],
            ..MavenModel::default()
        }
    }

    fn build_model(maven: MavenModel) -> BuildModel {
        BuildModel { maven: Some(maven), gradle: Vec::new() }
    }

    fn run_config(id: &str, name: &str, kind: &str, cwd: Option<&str>) -> RunConfig {
        RunConfig {
            detected: true,
            command: "mvn".to_owned(),
            ..generated(id.to_owned(), name.to_owned(), kind, String::new(), Vec::new(), cwd.unwrap_or_default())
        }
    }

    fn labels(nodes: &[BuildNode]) -> Vec<String> {
        nodes.iter().map(node_label).collect()
    }

    fn kids(node: &BuildNode, model: &BuildModel, configs: &[RunConfig]) -> Vec<BuildNode> {
        let src = TreeSource { model, configs, root: "/p" };
        let tool = if model.maven.is_some() { BuildTool::Maven } else { BuildTool::Gradle };
        children(tool, node, src).unwrap_or_default()
    }

    fn options(offline: bool, skip_tests: bool, profiles: &[(&str, bool)]) -> MavenRunOptions {
        MavenRunOptions {
            offline,
            skip_tests,
            profiles: profiles.iter().map(|(id, on)| ((*id).to_owned(), *on)).collect(),
        }
    }

    fn goals(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn shows_profiles_then_the_aggregator_with_lifecycle_plugins_and_its_modules() {
        let model = build_model(maven());
        let roots = maven_roots(model.maven.as_ref().expect("a Maven model"));
        assert_eq!(labels(&roots), ["Profiles", "Multi Demo"]);
        assert_eq!(labels(&kids(&roots[0], &model, &[])), ["dev", "ci"]);
        let top = kids(&roots[1], &model, &[]);
        assert_eq!(labels(&top), ["Lifecycle", "Plugins", "core", "Demo App"]);
        assert_eq!(labels(&kids(&top[0], &model, &[])), LIFECYCLE);
        assert_eq!(node_icon(&top[2]), "maven-module");
        assert_eq!(node_icon(&roots[1]), "maven-project");
    }

    #[test]
    fn expands_plugins_to_goals_and_dependencies_to_their_transitive_ones() {
        let model = build_model(maven());
        let core = kids(&maven_roots(model.maven.as_ref().expect("a Maven model"))[1], &model, &[]).remove(2);
        let below = kids(&core, &model, &[]);
        assert_eq!(labels(&below), ["Lifecycle", "Plugins", "Dependencies"]);
        let plugin = kids(&below[1], &model, &[]).remove(0);
        assert_eq!(node_label(&plugin), "surefire (org.apache.maven.plugins:maven-surefire-plugin:3.2.5)");
        let goals = kids(&plugin, &model, &[]);
        assert_eq!(labels(&goals), ["surefire:help", "surefire:test"]);
        assert!(is_runnable(&goals[1]));
        let deps = kids(&below[2], &model, &[]);
        assert_eq!(labels(&deps), ["org.junit.jupiter:junit-jupiter:5.10.2"]);
        assert_eq!(labels(&kids(&deps[0], &model, &[])), ["org.junit.jupiter:junit-jupiter-api:5.10.2"]);
        assert!(!is_runnable(&deps[0]));
    }

    #[test]
    fn lists_the_projects_maven_run_configurations() {
        let model = build_model(maven());
        let configs = vec![
            RunConfig { detected: false, ..run_config("a", "core [test]", "maven", Some("core")) },
            run_config("b", "root", "maven", None),
            run_config("c", "c", "npm", Some("core")),
        ];
        let roots = maven_roots(model.maven.as_ref().expect("a Maven model"));
        let top = kids(&roots[1], &model, &configs);
        assert_eq!(labels(&top), ["Lifecycle", "Plugins", "Run Configurations", "core", "Demo App"]);
        assert_eq!(labels(&kids(&top[2], &model, &configs)), ["root"]);
        let core = kids(&top[3], &model, &configs);
        let runs = core.iter().find(|n| matches!(n.kind, NodeKind::RunConfigs(_))).expect("core has runs");
        assert_eq!(labels(&kids(runs, &model, &configs)), ["core [test]"]);
    }

    #[test]
    fn expand_all_opens_folders_but_not_dependency_subtrees() {
        let model = build_model(maven());
        let src = TreeSource { model: &model, configs: &[], root: "/p" };
        let keys = expandable_keys(roots(BuildTool::Maven, &model), &|n| maven_children(n, src));
        assert!(keys.contains("mvn:pom.xml"));
        assert!(keys.contains("mvn:core/pom.xml:deps"));
        assert!(!keys.iter().any(|k| k.contains("deps/")));
    }

    #[test]
    fn keeps_explicit_maven_profiles() {
        let model = maven();
        let core = &model.projects[1];
        for profiles in
            [vec!["-P", "prod"], vec!["-Pprod"], vec!["--activate-profiles", "prod"], vec!["--activate-profiles=prod"]]
        {
            let typed: Vec<String> = std::iter::once("test").chain(profiles).map(str::to_owned).collect();
            let config =
                maven_goal_config(core, &typed, &model, &options(false, false, &[("ci", true), ("dev", false)]));
            assert_eq!(config.args, typed);
        }
    }

    #[test]
    fn passes_goals_offline_skip_tests_and_changed_profiles() {
        let model = maven();
        assert_eq!(profiles_arg(&model.profiles, &BTreeMap::new()), None);
        let changed = options(false, false, &[("ci", true), ("dev", false)]).profiles;
        assert_eq!(profiles_arg(&model.profiles, &changed).as_deref(), Some("!dev,ci"));
        let core = &model.projects[1];
        let config =
            maven_goal_config(core, &goals(&["clean", "test"]), &model, &options(true, false, &[("ci", true)]));
        assert_eq!(config.name, "core [clean,test]");
        assert_eq!(config.kind, "maven");
        assert_eq!(config.command, "/p/mvnw");
        assert_eq!(config.args, ["clean", "test", "-o", "-Pci"]);
        assert_eq!(config_cwd(&config), "core");
        assert_eq!(config.env.get("JAVA_HOME").map(String::as_str), Some("/jdk21"));
        assert!(config.is_test);
        assert!(!config.detected);
        let skipped = maven_goal_config(core, &goals(&["test"]), &model, &options(false, true, &[]));
        assert_eq!(skipped.args, ["test", "-DskipTests"]);
        assert!(!skipped.is_test);
        let root = maven_goal_config(&model.projects[0], &goals(&["clean"]), &model, &options(false, false, &[]));
        assert_eq!(root.cwd, None);
        assert!(!root.is_test);
        let configured = MavenProject {
            command: "//?/UNC/server/share/maven/bin/mvn.cmd".to_owned(),
            options: goals(&["-s", "/old/settings.xml", "-Dmaven.repo.local=/old/repository"]),
            ..core.clone()
        };
        let goal = maven_goal_config(&configured, &goals(&["test"]), &model, &options(false, true, &[]));
        assert_eq!(goal.command, "mvn");
        assert_eq!(goal.args, ["test", "-DskipTests"]);
        let own = goals(&["-s", "/own/settings.xml", "test"]);
        assert_eq!(maven_goal_config(&configured, &own, &model, &options(false, false, &[])).args, own);
        assert_eq!(goal.id, "maven:goal:core/pom.xml:test");
    }

    #[test]
    fn parses_typed_command_lines() {
        assert_eq!(parse_goal_line("mvn clean install -Dmsg='a b'"), ["clean", "install", "-Dmsg=a b"]);
        assert_eq!(parse_goal_line("./mvnw -q verify"), ["-q", "verify"]);
        assert_eq!(parse_task_line(r#"./gradlew :app:run --args "x y""#), [":app:run", "--args", "x y"]);
        assert_eq!(split_args(r#"a "" 'b c'"#), ["a", "", "b c"]);
    }

    #[test]
    fn completes_phases_and_plugin_goals_for_the_last_word_history_for_the_whole_line() {
        let plugins = [surefire()];
        let c = goal_completions("clean te", &plugins, &goals(&["clean test -DskipTests"]));
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["test", "test-compile"]);
        assert_eq!(c[0].line, "clean test");
        let sure: Vec<String> = goal_completions("sure", &plugins, &[]).into_iter().map(|x| x.label).collect();
        assert_eq!(sure, ["surefire:help", "surefire:test"]);
        assert_eq!(goal_completions("cl", &[], &goals(&["clean install"]))[0].label, "clean install");
        assert!(goal_completions("-D", &plugins, &[]).is_empty());
    }

    fn task(name: &str, group: &str, description: Option<&str>) -> GradleTask {
        GradleTask { name: name.to_owned(), group: group.to_owned(), description: description.map(str::to_owned) }
    }

    fn gradle() -> GradleModel {
        GradleModel {
            dir: String::new(),
            projects: vec![
                GradleProject {
                    path: ":".to_owned(),
                    name: "gradle-demo".to_owned(),
                    build_file: Some("build.gradle.kts".to_owned()),
                    tasks: vec![task("wrapper", "build setup", None), task("init", "build setup", None)],
                    ..GradleProject::default()
                },
                GradleProject {
                    path: ":app".to_owned(),
                    name: "app".to_owned(),
                    dir: "app".to_owned(),
                    build_file: Some("app/build.gradle.kts".to_owned()),
                    tasks: vec![
                        task("run", "application", Some("Runs this project")),
                        task("compileJava", "other", None),
                        task("test", "verification", None),
                        task("build", "build", None),
                    ],
                    ..GradleProject::default()
                },
                GradleProject {
                    path: ":libs:core".to_owned(),
                    name: "core".to_owned(),
                    dir: "libs/core".to_owned(),
                    ..GradleProject::default()
                },
            ],
            command: "/p/gradlew".to_owned(),
            synced_at: Some(1.0),
            ..GradleModel::default()
        }
    }

    fn gradle_model(builds: Vec<GradleModel>) -> BuildModel {
        BuildModel { maven: None, gradle: builds }
    }

    #[test]
    fn nests_subprojects_and_groups_tasks_other_last() {
        let model = gradle_model(vec![gradle()]);
        let configs = [run_config("g", "app [run]", "gradle", None)];
        let roots = gradle_roots(&model.gradle);
        assert_eq!(labels(&roots), ["gradle-demo"]);
        let top = kids(&roots[0], &model, &configs);
        assert_eq!(labels(&top), ["Tasks", "Run Configurations", "app", "core"]);
        let app_tasks = kids(&top[2], &model, &configs).remove(0);
        let groups = kids(&app_tasks, &model, &configs);
        assert_eq!(labels(&groups), ["application", "build", "verification", "other"]);
        let root_groups = kids(&top[0], &model, &configs);
        assert_eq!(labels(&kids(&root_groups[0], &model, &configs)), ["init", "wrapper"]);
        assert_eq!(node_icon(&groups[0]), "build-task-group");
    }

    #[test]
    fn runs_tasks_by_path_from_the_build_directory() {
        let model = gradle();
        let app = &model.projects[1];
        let config = gradle_task_config(&model, app, &goals(&["test"]), &GradleRunOptions { offline: true });
        assert_eq!(config.name, "app [test]");
        assert_eq!(config.kind, "gradle");
        assert_eq!(config.command, "/p/gradlew");
        assert_eq!(config.args, [":app:test", "--offline"]);
        assert_eq!(config.cwd, None);
        assert!(config.is_test);
        let wrapper =
            gradle_task_config(&model, &model.projects[0], &goals(&["wrapper"]), &GradleRunOptions::default());
        assert_eq!(wrapper.args, ["wrapper"]);
        let own = goals(&[":lib:jar", "--info"]);
        assert_eq!(gradle_task_config(&model, app, &own, &GradleRunOptions::default()).args, own);
    }

    #[test]
    fn keeps_identical_project_paths_and_tasks_of_independent_builds_distinct() {
        let builds: Vec<GradleModel> = ["first", "second"]
            .iter()
            .map(|dir| {
                let base = gradle();
                GradleModel {
                    dir: (*dir).to_owned(),
                    command: format!("/p/{dir}/gradlew"),
                    env: BTreeMap::from([("JAVA_HOME".to_owned(), format!("/{dir}/jdk"))]),
                    projects: base
                        .projects
                        .into_iter()
                        .map(|p| GradleProject {
                            dir: format!("{dir}/{}", p.dir).trim_end_matches('/').to_owned(),
                            ..p
                        })
                        .collect(),
                    ..base
                }
            })
            .collect();
        let model = gradle_model(builds.clone());
        let roots = gradle_roots(&builds);
        assert_ne!(roots[0].key, roots[1].key);
        let tests: Vec<String> = roots
            .iter()
            .map(|root| {
                let app = kids(root, &model, &[])
                    .into_iter()
                    .find(|n| matches!(&n.kind, NodeKind::GradleProject { path, .. } if path == ":app"))
                    .expect("the build has :app");
                let folder = kids(&app, &model, &[]).remove(0);
                let group = kids(&folder, &model, &[])
                    .into_iter()
                    .find(|n| matches!(&n.kind, NodeKind::TaskGroup { group, .. } if group == "verification"))
                    .expect("verification tasks");
                kids(&group, &model, &[]).remove(0).key
            })
            .collect();
        assert_ne!(tests[0], tests[1]);
        let runs: Vec<RunConfig> = builds
            .iter()
            .map(|b| gradle_task_config(b, &b.projects[1], &goals(&["test"]), &GradleRunOptions { offline: true }))
            .collect();
        assert_ne!(runs[0].id, runs[1].id);
        assert_eq!(runs[1].command, "/p/second/gradlew");
        assert_eq!(config_cwd(&runs[1]), "second");
        assert_eq!(runs[1].env.get("JAVA_HOME").map(String::as_str), Some("/second/jdk"));
        assert_eq!(runs[1].args, [":app:test", "--offline"]);
        assert_eq!(gradle_project_key(&builds[1].dir, &builds[1].projects[1].path), r#"gradle:["second",":app"]"#);
    }

    #[test]
    fn shows_run_configurations_only_under_their_owning_linked_build() {
        let root = gradle();
        let nested = GradleModel {
            dir: "nested".to_owned(),
            projects: root
                .projects
                .iter()
                .map(|p| GradleProject { dir: format!("nested/{}", p.dir), ..p.clone() })
                .collect(),
            ..root.clone()
        };
        let configs = [
            run_config("root", "root", "gradle", Some("app")),
            run_config("nested", "nested", "gradle", Some("nested/app")),
            run_config("absolute", "absolute", "gradle", Some("/p/nested/app")),
            run_config("other", "other", "maven", Some("nested")),
        ];
        let builds = [root.clone(), nested.clone()];
        let ids = |of: &GradleModel| -> Vec<String> {
            gradle_run_configs(of, &builds, &configs, "/p").into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(&root), ["root"]);
        assert_eq!(ids(&nested), ["nested", "absolute"]);
    }

    #[test]
    fn places_a_saved_run_under_its_directly_linked_root_instead_of_a_longer_sibling_composite() {
        let base = gradle();
        let app = base.projects[1].clone();
        let composite = GradleModel {
            dir: "very-long-composite".to_owned(),
            projects: vec![GradleProject {
                path: ":b:app".to_owned(),
                dir: "b/app".to_owned(),
                build: Some(":b".to_owned()),
                ..app.clone()
            }],
            ..base.clone()
        };
        let direct = GradleModel {
            dir: "b".to_owned(),
            projects: vec![GradleProject { dir: "b/app".to_owned(), ..app }],
            ..base
        };
        let saved = run_config("saved", "saved", "gradle", Some("b/app"));
        let builds = [composite.clone(), direct.clone()];
        assert!(gradle_run_configs(&composite, &builds, std::slice::from_ref(&saved), "/p").is_empty());
        assert_eq!(gradle_run_configs(&direct, &builds, std::slice::from_ref(&saved), "/p"), [saved]);
    }

    #[test]
    fn completes_task_names_and_paths() {
        let model = gradle();
        let labels: Vec<String> = task_completions("ru", &model, &[]).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, [":app:run"]);
        let lines: Vec<String> = task_completions("clean wr", &model, &[]).into_iter().map(|c| c.line).collect();
        assert_eq!(lines, ["clean wrapper"]);
    }

    #[test]
    fn resolves_the_maven_model_pom_at_its_path() {
        for (pom, path) in [
            ("pom.xml", "/workspace/pom.xml"),
            ("module/pom.xml", "/workspace/module/pom.xml"),
            ("/linked/pom.xml", "/linked/pom.xml"),
            ("/workspace/member/pom.xml", "/workspace/member/pom.xml"),
        ] {
            let projects = [MavenProject { pom: pom.to_owned(), ..base() }];
            assert_eq!(maven_pom_at_path(&projects, Some("/workspace"), path).as_deref(), Some(pom), "{pom} at {path}");
            assert_eq!(maven_pom_at_path(&projects, Some("/workspace"), "/other/pom.xml"), None);
        }
    }

    // Not from a spec: what the tree shows as one list, with Skip Tests striking the test phase.
    #[test]
    fn visible_rows_follow_the_expanded_keys_and_strike_the_test_phase_while_tests_are_skipped() {
        let model = build_model(maven());
        let src = TreeSource { model: &model, configs: &[], root: "/p" };
        let mut expanded = initial_expanded(BuildTool::Maven, &model);
        expanded.insert("mvn:pom.xml:lifecycle".to_owned());
        let settings = BuildSettings { maven: options(false, true, &[]), ..BuildSettings::default() };
        let rows = visible_rows(BuildTool::Maven, src, &expanded, &settings);
        let shown: Vec<(&str, usize)> = rows.iter().map(|r| (r.label.as_str(), r.depth)).collect();
        assert_eq!(&shown[..4], [("Profiles", 0), ("Multi Demo", 0), ("Lifecycle", 1), ("clean", 2)]);
        let test = rows.iter().find(|r| r.label == "test").expect("the test phase is shown");
        assert!(test.skipped);
        assert!(!rows.iter().find(|r| r.label == "compile").expect("compile is shown").skipped);
        assert!(rows[0].expandable && !rows[0].expanded);
    }

    // Not from a spec: the stored settings and history read back the way they were written.
    #[test]
    fn settings_and_history_round_trip_through_storage() {
        let settings =
            BuildSettings { maven: options(true, true, &[("ci", true)]), gradle: GradleRunOptions { offline: true } };
        assert_eq!(BuildSettings::parse(Some(&settings.to_json())), settings);
        assert_eq!(BuildSettings::parse(Some("not json")), BuildSettings::default());
        assert!(settings.to_json().contains(r#""skipTests":true"#));
        let history = pushed_history(&goals(&["a", "b"]), "b");
        assert_eq!(history, ["b", "a"]);
        let long: Vec<String> = (0..30).map(|i| i.to_string()).collect();
        assert_eq!(pushed_history(&long, "new").len(), HISTORY_LIMIT);
        assert_eq!(parse_history(Some(r#"["x","y"]"#)), ["x", "y"]);
        assert_eq!(history_key(BuildTool::Gradle, "/p"), "openidea.buildTools.history:gradle:/p");
    }
}
