//! Files a language server only checks for syntax because they belong to none of its projects: a
//! Java file outside every module's source folders, a Rust file in no crate, a Go file in no
//! package of the workspace. The server says so in a diagnostic; navigation, completion and usages
//! then fail without saying why, so the editor shows a banner with the reason (and, where a build
//! reload may bring the file in, the reloads).

use ide_core::Diagnostic;
use regex::Regex;
use std::sync::LazyLock;

/// What a server's report means for its file. `reload`: a build reload may bring the file into a
/// project (a module the import missed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    pub text: &'static str,
    pub reload: bool,
}

static NOTICES: LazyLock<Vec<(Regex, Notice)>> = LazyLock::new(|| {
    let notice = |test: &str, text: &'static str, reload: bool| {
        (Regex::new(test).expect("the pattern is valid"), Notice { text, reload })
    };
    vec![
        notice(
            r"is not on the classpath of project .*only syntax errors are reported",
            "This file is outside the source folders of its Java module: only syntax errors are reported, and navigation and completion don't work.",
            true,
        ),
        // The server still completes and navigates JDK classes in such a file.
        notice(
            r"is a non-project file, only syntax errors are reported",
            "This file does not belong to an imported Java project: only syntax errors are reported, and only JDK classes resolve.",
            true,
        ),
        // Before the generic "No packages found" below.
        notice(
            r"(?i)excluded[^\n]*(?:build|GOOS/GOARCH)",
            "This Go file is excluded by the current build constraints.",
            false,
        ),
        notice(
            r"^No packages found for open file",
            "This Go file belongs to no package of the workspace: code insight is limited.",
            false,
        ),
        notice(
            r"not included in any crates",
            "This Rust file is not included in any crate: rust-analyzer can't offer code insight for it.",
            false,
        ),
    ]
});

/// What a server's `message` means, when it says the file is outside its projects.
pub fn outside_project_notice(message: &str) -> Option<Notice> {
    NOTICES.iter().find(|(test, _)| test.is_match(message)).map(|(_, notice)| *notice)
}

/// Why code insight is limited in a file, from its diagnostics.
pub fn outside_project_reason(diagnostics: &[Diagnostic]) -> Option<Notice> {
    diagnostics.iter().find_map(|diagnostic| outside_project_notice(&diagnostic.message))
}

/// The build reloads a banner may offer: action id and English label.
const RELOADS: [(&str, &str); 2] =
    [("maven.reload", "Reload Maven Projects"), ("gradle.reload", "Reload Gradle Project")];

/// The banner above an editor whose file a server treats as outside its projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutsideProjectBanner {
    pub text: &'static str,
    /// (action id, label) of the reloads offered.
    pub actions: Vec<(&'static str, &'static str)>,
}

fn is_same_or_inside(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

/// The banner for the file at `path` with `diagnostics`; `roots` are the open project's root and
/// its workspace projects, `enabled` tells which reload actions exist and are enabled.
pub fn outside_project_banner(
    path: &str,
    diagnostics: &[Diagnostic],
    roots: &[&str],
    enabled: &dyn Fn(&str) -> bool,
) -> Option<OutsideProjectBanner> {
    let notice = outside_project_reason(diagnostics)?;
    let in_project = roots.iter().any(|root| !root.is_empty() && is_same_or_inside(path, root));
    let actions = if notice.reload && in_project {
        RELOADS.iter().copied().filter(|(id, _)| enabled(id)).collect()
    } else {
        Vec::new()
    };
    Some(OutsideProjectBanner { text: notice.text, actions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Pos, Range, Severity};

    #[test]
    fn recognizes_each_servers_report_that_a_file_is_outside_its_projects() {
        let cases: [(&str, Option<&str>); 10] = [
            (
                "Extra.java is not on the classpath of project service, only syntax errors are reported",
                Some("source folders of its Java module"),
            ),
            ("Scratch.java is a non-project file, only syntax errors are reported", Some("imported Java project")),
            (
                "No packages found for open file x.go.\nThis file may be excluded due to its build tags",
                Some("build constraints"),
            ),
            (
                "This file may be excluded due to its GOOS/GOARCH, or other build constraints.",
                Some("build constraints"),
            ),
            ("No packages found for open file x.go.", Some("no package of the workspace")),
            (
                "This file is not included in any crates, so rust-analyzer can't offer IDE services.",
                Some("not included in any crate"),
            ),
            ("undefined: missingSymbol", None),
            ("invalid build tags syntax", None),
            ("ActionDTO cannot be resolved to a type", None),
            ("Missing artifact demo:api:jar:1.0", None),
        ];
        for (message, expected) in cases {
            let text = outside_project_notice(message).map(|notice| notice.text);
            match expected {
                Some(part) => assert!(text.is_some_and(|text| text.contains(part)), "{message}"),
                None => assert_eq!(text, None, "{message}"),
            }
        }
        // A build reload is offered only where it can bring the file into a project.
        let java = outside_project_notice("A.java is a non-project file, only syntax errors are reported");
        assert_eq!(java.map(|notice| notice.reload), Some(true));
        assert_eq!(outside_project_notice("No packages found for open file x.go.").map(|n| n.reload), Some(false));
    }

    fn warning(message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Pos::new(0, 0), Pos::new(0, 1)),
            severity: Severity::Warning,
            message: message.into(),
            source: Some("Java".into()),
            code: Some("32".into()),
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    fn maven_only(id: &str) -> bool {
        id == "maven.reload"
    }

    #[test]
    fn shows_a_banner_while_the_server_reports_the_file_with_the_build_reloads_available() {
        let path = "/p/service/src/extra/Extra.java";
        assert_eq!(outside_project_banner(path, &[], &["/p"], &maven_only), None);
        let reported =
            [warning("Extra.java is not on the classpath of project service, only syntax errors are reported")];
        let banner = outside_project_banner(path, &reported, &["/p"], &maven_only).expect("a banner");
        assert!(banner.text.contains("source folders of its Java module"));
        assert_eq!(banner.actions, [("maven.reload", "Reload Maven Projects")]);
    }

    #[test]
    fn offers_a_build_reload_only_for_files_of_the_open_project() {
        for (path, labels) in [
            ("/data/scratches/scratch.java", Vec::new()),
            ("/p/tools/Gen.java", vec!["Reload Maven Projects"]),
            ("/ws/Gen.java", vec!["Reload Maven Projects"]),
        ] {
            let name = path.rsplit('/').next().unwrap_or_default();
            let reported = [warning(&format!("{name} is a non-project file, only syntax errors are reported"))];
            let banner = outside_project_banner(path, &reported, &["/ws", "/p"], &maven_only).expect("a banner");
            assert!(banner.text.contains("only JDK classes resolve"), "{path}");
            let shown: Vec<&str> = banner.actions.iter().map(|(_, label)| *label).collect();
            assert_eq!(shown, labels, "{path}");
        }
    }
}
