//! File menu actions around the project and the IDE settings: New Project…, Rename Project…,
//! Manage IDE Settings › Export Settings… and the title bar's IDE mode switch.

use crate::platform::virtual_docs::basename;
use regex::Regex;
use std::sync::LazyLock;

static FORBIDDEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[/\\:*?"<>|]"#).expect("a valid pattern"));

/// Why `name` cannot be a project folder name (English source text), or `None`.
pub fn project_name_error(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Enter a project name");
    }
    (FORBIDDEN.is_match(name) || name == "." || name == "..")
        .then_some("The name contains characters a folder name cannot have")
}

/// `name` (trimmed) inside `dir`, with the separator `dir` uses.
pub fn join_path(dir: &str, name: &str) -> String {
    let name = name.trim();
    if dir.ends_with('/') || dir.ends_with('\\') {
        return format!("{dir}{name}");
    }
    let separator = if dir.contains('\\') && !dir.contains('/') { '\\' } else { '/' };
    format!("{dir}{separator}{name}")
}

/// Where New Project… proposes the folder: the Default Project Directory, else the parent of the
/// open project. `None`: the user's home, which the caller knows.
pub fn initial_location(default_directory: &str, project_root: Option<&str>) -> Option<String> {
    let preset = default_directory.trim();
    if !preset.is_empty() {
        return Some(preset.to_owned());
    }
    project_root.map(|root| match root.rfind('/') {
        Some(slash) if slash > 0 => root[..slash].to_owned(),
        _ => "/".to_owned(),
    })
}

/// What creating the project folder needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateStep {
    /// List the folder; report [`CreateProject::listed`] with its entry count (`None`: it is not
    /// a folder that can be listed, so it is created).
    ReadDir(String),
    /// Create the folder, then report [`CreateProject::created`].
    CreateDir(String),
    /// `git init` the folder, then report [`CreateProject::created`].
    GitInit(String),
    /// The folder is ready: open it, in a new window when this one has a project.
    Open { path: String, new_window: bool },
    /// The dialog stays open with this error.
    Failed(String),
}

/// Creating the empty project folder `path` (or reusing it when it is an empty folder), with a
/// Git repository when asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProject {
    path: String,
    git: bool,
    new_window: bool,
    initialized: bool,
}

impl CreateProject {
    pub fn start(path: &str, git: bool, has_project: bool) -> (Self, CreateStep) {
        let flow = Self { path: path.to_owned(), git, new_window: has_project, initialized: false };
        (flow, CreateStep::ReadDir(path.to_owned()))
    }

    fn after_folder(&mut self) -> CreateStep {
        if self.git && !self.initialized {
            self.initialized = true;
            return CreateStep::GitInit(self.path.clone());
        }
        CreateStep::Open { path: self.path.clone(), new_window: self.new_window }
    }

    pub fn listed(&mut self, entries: Option<usize>) -> CreateStep {
        match entries {
            Some(0) => self.after_folder(),
            Some(_) => CreateStep::Failed(format!("{} already exists and is not empty", self.path)),
            None => CreateStep::CreateDir(self.path.clone()),
        }
    }

    /// The folder was created or initialized, or that failed.
    pub fn created(&mut self, result: Result<(), String>) -> CreateStep {
        match result {
            Ok(()) => self.after_folder(),
            Err(error) => CreateStep::Failed(error),
        }
    }
}

/// Storage prefixes of IDE-level state exported with the settings (templates, macros, layouts).
pub const EXPORTED_STORAGE: [&str; 5] = [
    "openidea.liveTemplates.",
    "openidea.fileTemplates.",
    "openidea.postfix.",
    "openidea.macros.",
    "openidea.toolWindowLayouts.",
];

/// IDE-level preferences exported with the settings as well.
const EXPORTED_KEYS: [&str; 10] = [
    "openidea.projectTree.options",
    "openidea.todo.v1",
    "openidea.vim.v1",
    "openidea.vcs.diffOptions",
    "openidea.vcs.diffPreview",
    "openidea.vcs.branchesPopup.options",
    "openidea.merge.autoApplyNonConflicting",
    "openidea.markdown.defaultMode",
    "openidea.imageViewer.chessboard",
    "openidea.welcome.reopenLastProject",
];

/// Whether the stored key is IDE-level state that export and import carry.
pub fn is_exported(key: &str) -> bool {
    EXPORTED_KEYS.contains(&key) || EXPORTED_STORAGE.iter().any(|prefix| key.starts_with(prefix))
}

/// The stored entries Export Settings… writes; per-project state stays behind.
pub fn exported_storage<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<(String, String)> {
    entries.into_iter().filter(|(key, _)| is_exported(key)).map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// Rename Project…: the name it starts from (the primary project's own name, not the window's
/// label for an attached workspace) and the folder an empty name falls back to.
pub fn rename_prompt(name: &str, root: &str) -> (String, String) {
    let folder = basename(root.trim_end_matches(['/', '\\'])).into_owned();
    (name.to_owned(), folder)
}

/// The title bar's IDE mode switch: shown for a project, not without one or in LightEdit.
pub fn ide_mode_toggle_visible(has_project: bool, light_edit: bool) -> bool {
    has_project && !light_edit
}

#[cfg(test)]
mod tests {
    use super::*;

    // File › New Project… › validates the folder name
    #[test]
    fn validates_the_folder_name() {
        assert_eq!(project_name_error("demo"), None);
        assert_eq!(project_name_error("  "), Some("Enter a project name"));
        assert!(project_name_error("a/b").is_some());
        assert!(project_name_error("..").is_some());
    }

    // File › New Project… › joins the location and name with the location's separator
    #[test]
    fn joins_the_location_and_name_with_the_locations_separator() {
        assert_eq!(join_path("/Users/me/Projects", " demo "), "/Users/me/Projects/demo");
        assert_eq!(join_path("/Users/me/", "demo"), "/Users/me/demo");
        assert_eq!(join_path(r"C:\Users\me", "demo"), r"C:\Users\me\demo");
    }

    // Manage IDE Settings › Export Settings… › exports templates, macros, layouts and view options, not per-project state
    #[test]
    fn exports_templates_macros_layouts_and_view_options_not_per_project_state() {
        let data = [
            ("openidea.liveTemplates.v1", "[]"),
            ("openidea.macros.v1", "{}"),
            ("openidea.toolWindowLayouts.v1", "{}"),
            ("openidea.projectTree.options", r#"{"singleClickOpen":true}"#),
            ("openidea.projectTree.expanded:/p", "[]"),
            ("openidea.runAnything.history.v1:/p", "[]"),
        ];
        let mut keys: Vec<String> = exported_storage(data).into_iter().map(|(key, _)| key).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "openidea.liveTemplates.v1",
                "openidea.macros.v1",
                "openidea.projectTree.options",
                "openidea.toolWindowLayouts.v1"
            ]
        );
    }

    // renames from the primary name while updating the attached window label
    #[test]
    fn renames_from_the_primary_name_while_updating_the_attached_window_label() {
        // The window shows "primary, other"; the rename starts from the primary project's name.
        assert_eq!(rename_prompt("primary", "/primary/"), ("primary".to_owned(), "primary".to_owned()));
    }

    // is hidden without a project and in LightEdit windows
    #[test]
    fn is_hidden_without_a_project_and_in_light_edit_windows() {
        assert!(!ide_mode_toggle_visible(false, false));
        assert!(!ide_mode_toggle_visible(true, true));
        assert!(ide_mode_toggle_visible(true, false));
    }

    // Not from a spec: the folder is created (or an empty one reused), initialized, then opened.
    #[test]
    fn creates_initializes_and_opens_the_project_folder() {
        let (mut flow, step) = CreateProject::start("/work/demo", true, true);
        assert_eq!(step, CreateStep::ReadDir("/work/demo".into()));
        assert_eq!(flow.listed(None), CreateStep::CreateDir("/work/demo".into()));
        assert_eq!(flow.created(Ok(())), CreateStep::GitInit("/work/demo".into()));
        assert_eq!(flow.created(Ok(())), CreateStep::Open { path: "/work/demo".into(), new_window: true });
        let (mut busy, _) = CreateProject::start("/work/full", false, false);
        assert_eq!(busy.listed(Some(2)), CreateStep::Failed("/work/full already exists and is not empty".into()));
        let (mut empty, _) = CreateProject::start("/work/empty", false, false);
        assert_eq!(empty.listed(Some(0)), CreateStep::Open { path: "/work/empty".into(), new_window: false });
        assert_eq!(initial_location(" /preset ", Some("/p/x")).as_deref(), Some("/preset"));
        assert_eq!(initial_location("", Some("/p/x")).as_deref(), Some("/p"));
        assert_eq!(initial_location("", None), None);
    }
}
