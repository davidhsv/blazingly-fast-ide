//! Go module and workspace commands (`go mod tidy`, `go work use`…) for manifest files.

use crate::run::configs::{blank_config, to_cwd};
use crate::run::go_generate::MenuAction;
use ide_run::RunConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoModuleOperation {
    Tidy,
    Download,
    Sync,
    Use,
}

impl GoModuleOperation {
    /// The `go mod` / `go work` subcommand.
    pub fn name(self) -> &'static str {
        match self {
            Self::Tidy => "tidy",
            Self::Download => "download",
            Self::Sync => "sync",
            Self::Use => "use",
        }
    }

    /// Id and title of the operation's action.
    pub fn action(self) -> (&'static str, &'static str) {
        match self {
            Self::Tidy => ("go.mod.tidy", "Go Mod Tidy"),
            Self::Download => ("go.mod.download", "Download Go Modules"),
            Self::Sync => ("go.work.sync", "Sync Go Workspace"),
            Self::Use => ("go.work.use", "Add Directory to Go Workspace…"),
        }
    }

    /// The operation works on `go.mod` (else on `go.work`).
    fn of_module(self) -> bool {
        matches!(self, Self::Tidy | Self::Download)
    }
}

/// The run of `operation` on the manifest at `path`, from the manifest's directory. `directory`: what
/// `go work use` adds.
pub fn go_module_command(operation: GoModuleOperation, path: &str, directory: Option<&str>) -> Option<RunConfig> {
    let manifest = if operation.of_module() { "/go.mod" } else { "/go.work" };
    if !path.ends_with(manifest) || path.contains('\0') {
        return None;
    }
    let tool = if operation.of_module() { "mod" } else { "work" };
    let mut args = vec![tool.to_string(), operation.name().to_string()];
    if operation == GoModuleOperation::Use {
        let directory = directory.filter(|dir| !dir.trim().is_empty() && !dir.contains('\0'))?;
        args.extend(["--".to_string(), directory.to_string()]);
    }
    let name = format!("go {tool} {}", operation.name());
    let mut config = blank_config(format!("go:{}:{path}", operation.name()), name, "go");
    config.command = "go".to_string();
    config.args = args;
    config.cwd = to_cwd(path.rsplit_once('/').map(|(dir, _)| dir));
    config.detected = true;
    Some(config)
}

/// The Go module entries of a context menu: for a single `go.mod` or `go.work`.
pub fn go_module_menu(paths: &[String]) -> Vec<MenuAction> {
    let [path] = paths else { return Vec::new() };
    let operations: &[GoModuleOperation] = if path.ends_with("/go.mod") {
        &[GoModuleOperation::Tidy, GoModuleOperation::Download]
    } else if path.ends_with("/go.work") {
        &[GoModuleOperation::Sync, GoModuleOperation::Use]
    } else {
        &[]
    };
    operations.iter().map(|operation| MenuAction { action: operation.action().0, path: path.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::cwd_of;

    #[test]
    fn runs_module_operations_from_the_selected_manifest_directory() {
        for operation in [GoModuleOperation::Tidy, GoModuleOperation::Download] {
            let config = go_module_command(operation, "/work/app/go.mod", None).expect("a module command");
            assert_eq!((config.kind.as_str(), config.command.as_str(), config.is_test), ("go", "go", false));
            assert_eq!(config.args, ["mod", operation.name()]);
            assert_eq!(cwd_of(&config), Some("/work/app"));
        }
        let sync = go_module_command(GoModuleOperation::Sync, "/work/go.work", None).expect("a workspace command");
        assert_eq!(sync.args, ["work", "sync"]);
        let used = go_module_command(GoModuleOperation::Use, "/work/go.work", Some("./module with spaces"));
        assert_eq!(used.expect("a workspace command").args, ["work", "use", "--", "./module with spaces"]);
    }

    #[test]
    fn does_not_reinterpret_invalid_manifests_missing_directories_or_nuls() {
        assert_eq!(go_module_command(GoModuleOperation::Tidy, "/work/go.work", None), None);
        assert_eq!(go_module_command(GoModuleOperation::Sync, "/work/go.mod", None), None);
        assert_eq!(go_module_command(GoModuleOperation::Use, "/work/go.work", None), None);
        assert_eq!(go_module_command(GoModuleOperation::Use, "/work/go.work", Some("x\0y")), None);
    }

    #[test]
    fn contributes_stable_action_ids_only_to_applicable_manifest_menus() {
        let actions = |paths: &[&str]| {
            let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
            go_module_menu(&paths).into_iter().map(|item| item.action).collect::<Vec<_>>()
        };
        assert_eq!(actions(&["/p/go.mod"]), ["go.mod.tidy", "go.mod.download"]);
        assert_eq!(actions(&["/p/go.work"]), ["go.work.sync", "go.work.use"]);
        assert!(actions(&["/p/go.work", "/p/go.mod"]).is_empty());
        assert!(actions(&["/p/main.go"]).is_empty());
    }
}
