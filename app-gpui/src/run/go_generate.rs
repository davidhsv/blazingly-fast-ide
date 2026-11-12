//! `go generate` for a file or its package, and for one `//go:generate` directive.

use crate::run::configs::{blank_config, to_cwd};
use ide_run::RunConfig;

/// What `go generate` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerateScope {
    File,
    Package,
}

impl GenerateScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Package => "package",
        }
    }
}

/// `text` as a literal in a regular expression.
pub(crate) fn escape_reg_exp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The `go generate` run of the Go file at `path`: of its directives or of its package, or of the one
/// `directive` (its literal line). Nothing goes through a shell.
pub fn go_generate_config(path: &str, scope: GenerateScope, directive: Option<&str>) -> Option<RunConfig> {
    if !path.ends_with(".go") || path.contains('\0') {
        return None;
    }
    let (dir, file) = match path.rsplit_once('/') {
        Some((dir, file)) => (Some(if dir.is_empty() { "/" } else { dir }), file),
        None => (None, path),
    };
    let pattern = directive.filter(|d| !d.is_empty()).map(|d| format!("^{}$", escape_reg_exp(d.trim())));
    let target = match scope {
        GenerateScope::Package => ".".to_string(),
        GenerateScope::File if file.starts_with('-') => format!("./{file}"),
        GenerateScope::File => file.to_string(),
    };
    let id = format!("go:generate:{}:{path}:{}", scope.as_str(), pattern.as_deref().unwrap_or_default());
    let mut config = blank_config(id, format!("go generate {target}"), "go");
    config.command = "go".to_string();
    config.args = std::iter::once("generate".to_string())
        .chain(pattern.into_iter().flat_map(|pattern| ["-run".to_string(), pattern]))
        .chain([target])
        .collect();
    config.cwd = to_cwd(dir);
    config.detected = true;
    config.extras.params.insert("useTags".to_string(), "true".to_string());
    Some(config)
}

/// An action a context menu offers for a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuAction {
    pub action: &'static str,
    pub path: String,
}

pub const GENERATE_FILE_ACTION: &str = "go.generate.file";
pub const GENERATE_PACKAGE_ACTION: &str = "go.generate.package";

/// The Go Generate entries of a context menu: for a single Go source file.
pub fn go_generate_menu(paths: &[String], is_dir: bool) -> Vec<MenuAction> {
    let [path] = paths else { return Vec::new() };
    if !path.ends_with(".go") || is_dir {
        return Vec::new();
    }
    [GENERATE_FILE_ACTION, GENERATE_PACKAGE_ACTION].map(|action| MenuAction { action, path: path.clone() }).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::config_types::TypeEnv;
    use crate::run::configs::cwd_of;
    use crate::run::go_types::go_build_type;
    use crate::run::types::form_params;

    const PATH: &str = "/work/app/tools/generate.go";

    #[test]
    fn runs_files_and_packages_in_their_own_directory_without_shell_interpretation() {
        let file = go_generate_config(PATH, GenerateScope::File, None).expect("a generate run");
        assert_eq!((file.command.as_str(), cwd_of(&file)), ("go", Some("/work/app/tools")));
        assert_eq!(file.args, ["generate", "generate.go"]);
        assert_eq!((file.is_test, file.debug_adapter.as_deref()), (false, None));
        let package = go_generate_config(PATH, GenerateScope::Package, None).expect("a generate run");
        assert_eq!(package.args, ["generate", "."]);
        let dashed = go_generate_config("/work/app/-name.go", GenerateScope::File, None).expect("a generate run");
        assert_eq!(dashed.args, ["generate", "./-name.go"]);
        assert_eq!(form_params(&file, Some(&go_build_type()), &TypeEnv::default()), None);
        assert_eq!(go_generate_config("/work/app/go.mod", GenerateScope::File, None), None);
    }

    #[test]
    fn adds_file_and_package_actions_only_for_a_single_go_source_file() {
        let paths = |paths: &[&str]| paths.iter().map(|path| path.to_string()).collect::<Vec<_>>();
        let actions: Vec<&str> = go_generate_menu(&paths(&[PATH]), false).iter().map(|item| item.action).collect();
        assert_eq!(actions, ["go.generate.file", "go.generate.package"]);
        assert!(go_generate_menu(&paths(&[PATH, "/work/other.go"]), false).is_empty());
        assert!(go_generate_menu(&paths(&["/work/app/go.mod"]), false).is_empty());
    }

    // Not from a spec: a directive narrows the run to its literal line.
    #[test]
    fn a_directive_becomes_an_anchored_literal_pattern() {
        let directive = "//go:generate go run gen.go -out=$GOFILE";
        let config = go_generate_config(PATH, GenerateScope::File, Some(directive)).expect("a generate run");
        let pattern = "^//go:generate go run gen\\.go -out=\\$GOFILE$";
        assert_eq!(config.args, ["generate", "-run", pattern, "generate.go"]);
        assert_eq!(config.id, format!("go:generate:file:{PATH}:{pattern}"));
    }
}
