//! IDEA's "Specify Scope" choices for the analyses (Inspect Code, Locate Duplicates, Search Structurally),
//! resolved to a backend [`SearchScope`].

use crate::platform::i18n::{I18n, Param};
use crate::platform::search::{CustomScope, find_custom_scope};
use ide_core::IpcPath;
use ide_core::ipc_path::to_ipc;
use ide_search::SearchScope;
use ide_vcs::{ChangeKind, FileStatus};

/// The kinds of scope the dialogs offer, in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Project,
    Directory,
    File,
    Changed,
    Custom,
}

pub const SCOPE_KINDS: [ScopeKind; 5] =
    [ScopeKind::Project, ScopeKind::Directory, ScopeKind::File, ScopeKind::Changed, ScopeKind::Custom];

impl ScopeKind {
    /// English source text of the choice, for the view to translate.
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Whole project",
            Self::Directory => "Module or directory",
            Self::File => "Current file",
            Self::Changed => "Uncommitted files",
            Self::Custom => "Custom scope",
        }
    }
}

/// A scope chosen in a dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeChoice {
    pub kind: ScopeKind,
    /// Absolute directory for [`ScopeKind::Directory`].
    pub directory: String,
    /// Name of the custom scope for [`ScopeKind::Custom`].
    pub custom: Option<String>,
}

/// The directory of `path` (`/` for a top-level path).
pub fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if i > 0 => &path[..i],
        _ => "/",
    }
}

/// The initial scope: the directory of the active editor's file (a real file) is offered, else the root.
pub fn default_scope(kind: ScopeKind, current_file: Option<&str>, root: Option<&str>) -> ScopeChoice {
    let directory = current_file.map_or_else(|| root.unwrap_or_default().to_owned(), |file| dirname(file).to_owned());
    ScopeChoice { kind, directory, custom: None }
}

/// The paths of files with uncommitted changes that still exist, from the Git status.
pub fn changed_files(status: &[FileStatus]) -> Vec<String> {
    status
        .iter()
        .filter(|s| s.unstaged != Some(ChangeKind::Deleted) && s.staged != Some(ChangeKind::Deleted))
        .map(|s| to_ipc(&s.path))
        .collect()
}

/// What resolving a scope reads besides the choice.
#[derive(Debug, Clone, Copy)]
pub struct ScopeEnv<'a> {
    /// The active editor's file, if it is a real file.
    pub current_file: Option<&'a str>,
    /// [`changed_files`], or why the status could not be read; only read for [`ScopeKind::Changed`].
    pub changed_files: Result<&'a [String], &'a str>,
    /// The project's custom scopes.
    pub custom_scopes: &'a [CustomScope],
}

/// Whether resolving `choice` needs the Git status (ask for it first).
pub fn needs_changed_files(choice: &ScopeChoice) -> bool {
    choice.kind == ScopeKind::Changed
}

/// The backend scope of `choice`, or the English message why there is none (a notification body).
pub fn resolve_scope(choice: &ScopeChoice, env: ScopeEnv) -> Result<SearchScope, String> {
    match choice.kind {
        ScopeKind::Project => Ok(SearchScope::Project),
        ScopeKind::Directory => match choice.directory.trim() {
            "" => Err("Choose a directory".to_owned()),
            path => Ok(SearchScope::Directory { path: IpcPath::from(path) }),
        },
        ScopeKind::File => env
            .current_file
            .map(|file| SearchScope::Files { paths: vec![IpcPath::from(file)] })
            .ok_or_else(|| "No file is open".to_owned()),
        ScopeKind::Changed => match env.changed_files {
            Err(message) => Err(message.to_owned()),
            Ok([]) => Err("There are no uncommitted files".to_owned()),
            Ok(paths) => Ok(SearchScope::Files { paths: paths.iter().map(|p| IpcPath::from(p.as_str())).collect() }),
        },
        ScopeKind::Custom => choice
            .custom
            .as_deref()
            .and_then(|name| find_custom_scope(env.custom_scopes, name))
            .map(|scope| SearchScope::Pattern { pattern: scope.pattern.clone() })
            .ok_or_else(|| "Choose a scope".to_owned()),
    }
}

/// "Project", "Directory src/app", "File a.rs", "3 uncommitted files", "Scope 'Src'"; paths in the project
/// relative to `root`.
pub fn describe_scope(choice: &ScopeChoice, scope: &SearchScope, root: Option<&str>, i18n: &mut I18n) -> String {
    let root = root.unwrap_or_default();
    let rel = |path: &str| -> String {
        if root.is_empty() {
            return path.to_owned();
        }
        path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path).to_owned()
    };
    match (choice.kind, scope) {
        (ScopeKind::Project, _) => i18n.tx("Project"),
        (ScopeKind::Directory, _) => i18n.tx_built(&["Directory ", ""], &[Param::from(rel(&choice.directory))]),
        (ScopeKind::File, SearchScope::Files { paths }) => {
            let first = paths.first().map(|p| to_ipc(p)).unwrap_or_default();
            i18n.tx_built(&["File ", ""], &[Param::from(rel(&first))])
        }
        (ScopeKind::File, _) => i18n.tx("Current file"),
        (ScopeKind::Changed, SearchScope::Files { paths }) => {
            let strings: [&str; 2] =
                if paths.len() == 1 { ["", " uncommitted file"] } else { ["", " uncommitted files"] };
            i18n.tx_built(&strings, &[Param::from(paths.len())])
        }
        (ScopeKind::Changed, _) => i18n.tx("Uncommitted files"),
        (ScopeKind::Custom, _) => {
            i18n.tx_built(&["Scope '", "'"], &[Param::from(choice.custom.clone().unwrap_or_default())])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(kind: ScopeKind, directory: &str, custom: Option<&str>) -> ScopeChoice {
        ScopeChoice { kind, directory: directory.to_owned(), custom: custom.map(str::to_owned) }
    }

    const NONE: ScopeEnv<'static> = ScopeEnv { current_file: None, changed_files: Ok(&[]), custom_scopes: &[] };

    #[test]
    fn resolves_scopes_or_explains_why_not() {
        assert!(matches!(resolve_scope(&choice(ScopeKind::Project, "", None), NONE), Ok(SearchScope::Project)));
        match resolve_scope(&choice(ScopeKind::Directory, " /p/src ", None), NONE) {
            Ok(SearchScope::Directory { path }) => assert_eq!(to_ipc(&path), "/p/src"),
            _ => panic!("a directory scope"),
        }
        let error = |kind, custom: Option<&str>, env| resolve_scope(&choice(kind, "", custom), env).err();
        assert_eq!(error(ScopeKind::Directory, None, NONE).as_deref(), Some("Choose a directory"));
        assert_eq!(error(ScopeKind::File, None, NONE).as_deref(), Some("No file is open"));
        assert_eq!(error(ScopeKind::Changed, None, NONE).as_deref(), Some("There are no uncommitted files"));
        assert_eq!(error(ScopeKind::Custom, None, NONE).as_deref(), Some("Choose a scope"));
        let scopes = [CustomScope { name: "Src".to_owned(), pattern: "file:src//*".to_owned() }];
        let env = ScopeEnv { custom_scopes: &scopes, ..NONE };
        match resolve_scope(&choice(ScopeKind::Custom, "", Some("Src")), env) {
            Ok(SearchScope::Pattern { pattern }) => assert_eq!(pattern, "file:src//*"),
            _ => panic!("a pattern scope"),
        }
    }
}
