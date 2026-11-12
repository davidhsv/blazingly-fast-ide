//! Build, Execution, Deployment › Run Targets: the page, and the small text edits of its fields.

use super::page_spec::{OptionSpec, PageSpec};

/// The group of the Build, Execution, Deployment pages.
pub const BUILD_GROUP: &str = "Build, Execution, Deployment";

/// A setting typed as a list of words (`8000:8000 5678:5678`, `a.yaml; b.yaml`): split at blanks,
/// commas and semicolons.
pub fn split_list(text: &str) -> Vec<String> {
    let is_separator = |c: char| c.is_whitespace() || c == ',' || c == ';';
    text.split(is_separator).filter(|word| !word.is_empty()).map(str::to_owned).collect()
}

/// The folder above a remote folder; `None` for a root (`/`, `~`) and for a relative name.
pub fn parent_path(path: &str) -> Option<&str> {
    let path = if path.len() > 1 { path.trim_end_matches('/') } else { path };
    if matches!(path, "/" | "~" | "") {
        return None;
    }
    match path.rfind('/')? {
        0 => Some("/"),
        slash => Some(&path[..slash]),
    }
}

/// The Run Targets page.
pub fn run_targets_page() -> PageSpec {
    PageSpec::new("build.runTargets", "Run Targets", 364)
        .group(BUILD_GROUP)
        .project_scope()
        .keywords(&[
            "ssh",
            "remote",
            "target",
            "run on",
            "interpreter",
            "upload",
            "sync",
            "docker",
            "compose",
            "container",
        ])
        .section(
            None,
            vec![OptionSpec::Custom {
                label: "Run Targets",
                keywords: vec![
                    "SSH configuration",
                    "Project folder on target",
                    "Upload changed project files before each run",
                    "Python interpreter",
                    "Node.js interpreter",
                    "Java executable",
                    "Docker connection",
                    "Image",
                    "Compose files",
                    "Service",
                    "Container folder",
                    "Published ports",
                    "Run options",
                ],
            }],
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::page_spec::spec_labels;
    use ide_core::settings::{RunTarget, RunTargetKind};
    use serde_json::json;

    // Run Targets page › is found by its fields
    #[test]
    fn is_found_by_its_fields() {
        let labels = spec_labels(&run_targets_page());
        for label in ["SSH configuration", "Project folder on target", "Python interpreter"] {
            assert!(labels.contains(&label), "{label}");
        }
        for label in ["Docker connection", "Image", "Service", "Published ports"] {
            assert!(labels.contains(&label), "{label}");
        }
    }

    // Run Targets page › reads SSH targets saved before container ones existed, and container fields
    //
    // The backend reads them the same; where the webview dropped a malformed list item or took an
    // unknown kind as SSH, the backend refuses the target.
    #[test]
    fn reads_ssh_targets_saved_before_container_ones_existed_and_container_fields() {
        let stored = json!([
            { "id": "box", "name": "Box", "sshConfig": "s" },
            {
                "kind": "compose",
                "id": "web",
                "composeFiles": ["compose.yaml"],
                "service": "web",
                "ports": ["8000:8000"],
            },
        ]);
        let targets: Vec<RunTarget> = serde_json::from_value(stored).expect("the backend takes the targets");
        let (ssh, compose) = (&targets[0], &targets[1]);
        assert_eq!((ssh.kind, ssh.ssh_config.as_str(), ssh.image.as_str()), (RunTargetKind::Ssh, "s", ""));
        assert!(ssh.compose_files.is_empty() && !ssh.image_user);
        assert_eq!(compose.kind, RunTargetKind::Compose);
        assert_eq!(compose.compose_files, ["compose.yaml"]);
        assert_eq!((compose.service.as_str(), compose.ports.as_slice()), ("web", ["8000:8000".to_owned()].as_slice()));
        let mixed = json!([{ "kind": "compose", "id": "web", "composeFiles": ["compose.yaml", 3] }]);
        assert!(serde_json::from_value::<Vec<RunTarget>>(mixed).is_err());
        assert!(serde_json::from_value::<Vec<RunTarget>>(json!([{ "kind": "kubernetes", "id": "k" }])).is_err());
    }

    // Run Targets page › splits ports and file lists
    #[test]
    fn splits_ports_and_file_lists() {
        assert_eq!(split_list(" 8000:8000, 5678:5678;a.yaml  "), ["8000:8000", "5678:5678", "a.yaml"]);
        assert!(split_list("  ").is_empty());
    }

    // Run Targets page › walks up remote folders
    #[test]
    fn walks_up_remote_folders() {
        assert_eq!(parent_path("/home/me/app"), Some("/home/me"));
        assert_eq!(parent_path("/home/"), Some("/"));
        assert_eq!(parent_path("~/app"), Some("~"));
        assert_eq!(parent_path("~"), None);
        assert_eq!(parent_path("/"), None);
    }
}
