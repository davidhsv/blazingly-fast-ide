//! What to offer when Maven or Gradle could not be started, or Gradle refuses the JDK it is
//! imported with: a hint in front of the error and a link to the tool's settings. Shared by
//! build sync, run configurations and language-server import errors.

use crate::platform::i18n::I18n;
use crate::platform::misc::jdk::{DownloadJdkFix, download_jdk_fix};
use regex::Regex;
use std::sync::LazyLock;

/// The action that opens Settings on a page.
pub const SETTINGS_ACTION: &str = "file.settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
    Gradle,
}

impl BuildTool {
    /// The settings page of the tool: the argument of [`SETTINGS_ACTION`].
    pub fn settings_page(self) -> &'static str {
        match self {
            Self::Maven => "build.maven",
            Self::Gradle => "build.gradle",
        }
    }
}

/// An action offered next to the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// "Download JDK N": the caller labels and runs the fix.
    DownloadJdk(DownloadJdkFix),
    /// Runs [`SETTINGS_ACTION`] with [`BuildTool::settings_page`] of `tool`.
    OpenSettings { label: String, tool: BuildTool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildToolRecovery {
    /// The message to show: the error, with a hint in front when there is a recovery.
    pub body: String,
    pub actions: Vec<RecoveryAction>,
}

static INCOMPATIBLE_JDK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Can't use Java .* and Gradle .* to import Gradle project").expect("the pattern is valid")
});
static MISSING_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)command not found: ([^\r\n]+)|cannot start ([^\r\n]+?): [^\r\n]*\(os error [23]\)")
        .expect("the pattern is valid")
});
static MAVEN_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[/\\])(?:mvn|mvnw|mvnd)(?:\.(?:cmd|bat|exe))?$").expect("the pattern is valid")
});
static GRADLE_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[/\\])gradlew?(?:\.(?:bat|exe))?$").expect("the pattern is valid"));

const QUOTES: [char; 3] = ['"', '\'', '`'];

/// The command a "command not found" / "cannot start" error names, without quotes around it.
fn missing_command(message: &str) -> &str {
    let Some(found) = MISSING_COMMAND.captures(message) else {
        return "";
    };
    let command = found.get(1).or_else(|| found.get(2)).map_or("", |group| group.as_str()).trim();
    let command = command.strip_prefix(QUOTES).unwrap_or(command);
    command.strip_suffix(QUOTES).unwrap_or(command)
}

/// The recovery for a build, run or import error `message`. `download_jdk_registered`: the
/// Download JDK action can run. A message that is about neither tool comes back as it is, with
/// at most the Download JDK fix.
pub fn build_tool_recovery(message: &str, download_jdk_registered: bool, i18n: &I18n) -> BuildToolRecovery {
    let mut actions: Vec<RecoveryAction> =
        download_jdk_fix(Some(message), download_jdk_registered).map(RecoveryAction::DownloadJdk).into_iter().collect();
    let incompatible = INCOMPATIBLE_JDK.is_match(message);
    let command = missing_command(message);
    let named = if MAVEN_COMMAND.is_match(command) {
        Some(BuildTool::Maven)
    } else if GRADLE_COMMAND.is_match(command) {
        Some(BuildTool::Gradle)
    } else {
        None
    };
    let Some(tool) = (if incompatible { Some(BuildTool::Gradle) } else { named }) else {
        return BuildToolRecovery { body: message.to_owned(), actions };
    };
    let (label, hint) = match tool {
        BuildTool::Gradle if incompatible => (
            "Choose Gradle JVM",
            "Choose a JDK compatible with this Gradle version in Gradle JVM. Download JDK is available in the JVM \
             selector.",
        ),
        BuildTool::Maven => (
            "Open Maven Settings",
            "Maven could not be started. Configure Maven home in Maven settings, install Maven on PATH, or use a \
             project with a Maven wrapper.",
        ),
        BuildTool::Gradle => (
            "Open Gradle Settings",
            "Gradle could not be started. Configure a local installation in Gradle settings, install Gradle on PATH, \
             or use a project with a Gradle wrapper.",
        ),
    };
    actions.push(RecoveryAction::OpenSettings { label: i18n.tx(label), tool });
    BuildToolRecovery { body: format!("{}\n\n{message}", i18n.tx(hint)), actions }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery(message: &str) -> BuildToolRecovery {
        build_tool_recovery(message, false, &I18n::default())
    }

    // links missing build tool to its settings: %s
    #[test]
    fn links_missing_build_tool_to_its_settings() {
        for (message, tool) in [
            ("cannot start mvn: No such file or directory (os error 2)", BuildTool::Maven),
            ("command not found: mvn", BuildTool::Maven),
            ("command not found: /opt/apache maven/bin/mvn", BuildTool::Maven),
            (r"command not found: C:\Maven\bin\mvn.cmd", BuildTool::Maven),
            ("cannot start gradle: No such file or directory (os error 2)", BuildTool::Gradle),
            ("command not found: gradle", BuildTool::Gradle),
            (r"command not found: C:\Gradle\bin\gradle.bat", BuildTool::Gradle),
            ("command not found: ./gradlew", BuildTool::Gradle),
        ] {
            let result = recovery(message);
            let name = if tool == BuildTool::Maven { "Maven" } else { "Gradle" };
            assert!(result.body.contains(&format!("{name} could not be started")), "{message}");
            assert!(result.body.contains(message), "{message}");
            let [RecoveryAction::OpenSettings { tool: opened, .. }] = result.actions.as_slice() else {
                panic!("{message}: one settings action, not {:?}", result.actions);
            };
            assert_eq!(*opened, tool, "{message}");
            assert_eq!(opened.settings_page(), if tool == BuildTool::Maven { "build.maven" } else { "build.gradle" });
        }
    }

    // links the reported Gradle import rejection to the Gradle JVM picker and its JDK download
    #[test]
    fn links_the_reported_gradle_import_rejection_to_the_gradle_jvm_picker_and_its_jdk_download() {
        let message = "Nov 9, 2026, 3:49:11 PM Can't use Java 27.0.1.0.3 and Gradle 9.7.1 to import Gradle project \
                       kotlin-gradle.";
        let result = recovery(message);
        assert!(result.body.contains("Download JDK"));
        assert_eq!(
            result.actions,
            [RecoveryAction::OpenSettings { label: "Choose Gradle JVM".to_owned(), tool: BuildTool::Gradle }]
        );
        assert_eq!((SETTINGS_ACTION, BuildTool::Gradle.settings_page()), ("file.settings", "build.gradle"));
    }

    // leaves unrelated failures alone: %s
    #[test]
    fn leaves_unrelated_failures_alone() {
        for message in [
            "command not found: npm",
            "Maven tests failed",
            "Gradle build failed",
            "cannot start mvn: Permission denied (os error 13)",
        ] {
            assert_eq!(recovery(message), BuildToolRecovery { body: message.to_owned(), actions: Vec::new() });
        }
    }

    // Not from a spec: quotes around the command are dropped, and a message that also asks for
    // a JDK keeps the Download JDK fix in front of the settings link.
    #[test]
    fn reads_quoted_commands_and_keeps_the_download_jdk_fix_first() {
        let quoted = r#"command not found: "C:\Program Files\Maven\bin\mvn.cmd""#;
        assert_eq!(missing_command(quoted), r"C:\Program Files\Maven\bin\mvn.cmd");
        let message = "command not found: 'mvnw'\nit needs JDK 21 or newer";
        let result = build_tool_recovery(message, true, &I18n::default());
        assert!(matches!(
            result.actions.as_slice(),
            [RecoveryAction::DownloadJdk(fix), RecoveryAction::OpenSettings { tool: BuildTool::Maven, .. }]
                if fix.args.version == 21
        ));
    }
}
