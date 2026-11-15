//! In a test build, the app starts no Maven, Gradle or JVM but the stand-ins a test writes into its
//! own temporary folders. The backend resolves the launcher itself (Settings › Maven home, a build's
//! wrapper, a version manager's install, else the one on `PATH`), so the check reads the launcher
//! the backend reports for the build (`build_model`) and refuses before the backend is asked to run
//! it: a setting the backend ignored would show up here as `mvn`, never as a run of the real one.

use ide_build::BuildTool;
use ide_core::error::ErrorKind;
use ide_core::{IdeError, IdeResult};
use ide_host::Host;
use ide_run::RunConfig;
use std::path::Path;

/// The launchers of build tools and JVMs, by file name without extension.
const GUARDED: &[&str] = &["mvn", "mvnw", "mvnd", "gradle", "gradlew", "java", "javaw"];

fn refused(command: &str) -> IdeError {
    IdeError::new(
        ErrorKind::PermissionDenied,
        format!("a test starts no build tool or JVM outside its temporary folders: {command}"),
    )
}

/// Whether `command` names an existing file inside the temporary folder of the process.
fn is_stand_in(command: &str) -> bool {
    let path = Path::new(command);
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    let temp = std::env::temp_dir();
    let roots = [temp.canonicalize().unwrap_or_else(|_| temp.clone()), temp];
    path.canonicalize().is_ok_and(|path| roots.iter().any(|root| path.starts_with(root)))
}

/// Whether `command` is a build tool or a JVM launcher, whatever folder it is in.
fn is_guarded(command: &str) -> bool {
    let name = command.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    let stem = name.split('.').next().unwrap_or_default();
    GUARDED.contains(&stem)
}

/// The launchers the backend reports for `tool`'s builds of `window`'s project (all of them, or
/// those of `build_dir`); `None` while the project is in Restricted Mode, where the backend starts
/// nothing.
async fn reported_launchers(
    host: &Host,
    window: &str,
    tool: BuildTool,
    build_dir: Option<&str>,
) -> IdeResult<Option<Vec<String>>> {
    if host.require_trust(window).is_err() {
        return Ok(None);
    }
    let model = ide_host::ops::build::build_model(host, window).await?;
    let root = host.project(window)?.root.clone();
    let launchers: Vec<String> = match tool {
        BuildTool::Maven => {
            let daemon = host.project(window)?.settings.effective().languages.java.maven.daemon;
            if daemon {
                // The Maven Daemon replaces the configured Maven and is looked for on the machine.
                return Err(refused("mvnd"));
            }
            model.maven.iter().flat_map(|m| &m.projects).map(|p| p.command.clone()).collect()
        }
        BuildTool::Gradle => model
            .gradle
            .iter()
            .filter(|g| build_dir.is_none_or(|dir| root.join(dir) == root.join(&g.dir)))
            .map(|g| g.command.clone())
            .collect(),
    };
    Ok(Some(launchers))
}

fn require_stand_ins(launchers: Option<Vec<String>>) -> IdeResult<()> {
    let Some(launchers) = launchers else { return Ok(()) };
    if launchers.is_empty() {
        return Err(refused("no launcher is reported for the build"));
    }
    match launchers.iter().find(|command| !is_stand_in(command)) {
        Some(command) => Err(refused(command)),
        None => Ok(()),
    }
}

/// Refuses a sync of `tool` whose launcher is not a stand-in in a temporary folder.
pub async fn check_sync(host: &Host, window: &str, tool: BuildTool, build_dir: Option<&str>) -> IdeResult<()> {
    require_stand_ins(reported_launchers(host, window, tool, build_dir).await?)
}

/// Refuses a run whose program is a build tool or a JVM outside a temporary folder: a Maven or
/// Gradle configuration by the launcher the backend resolves for it too, any other by its command.
pub async fn check_run(host: &Host, window: &str, config: &RunConfig) -> IdeResult<()> {
    let tool = match config.kind.as_str() {
        "maven" => Some(BuildTool::Maven),
        "gradle" => Some(BuildTool::Gradle),
        _ => None,
    };
    // A Maven or Gradle configuration's bare `mvn` / `gradle` is replaced by the launcher the
    // backend resolves; any other launcher it names (a wrapper, a path) is started as it is.
    let replaced = tool.is_some() && matches!(config.command.as_str(), "mvn" | "gradle");
    if !replaced && is_guarded(&config.command) && !is_stand_in(&config.command) {
        return Err(refused(&config.command));
    }
    match tool {
        // The Maven of Settings › Maven replaces the command of a detected or generated goal too.
        Some(BuildTool::Maven) => require_stand_ins(reported_launchers(host, window, BuildTool::Maven, None).await?),
        Some(BuildTool::Gradle) if replaced => {
            if host.require_trust(window).is_err() {
                return Ok(());
            }
            // A bare `gradle` is not the build's wrapper: the backend starts the local distribution
            // of Settings › Gradle, else a version manager's install, else the `gradle` on `PATH`.
            let project = host.project(window)?;
            let gradle = project.settings.effective().languages.java.gradle;
            let home = gradle.home.trim();
            let local = gradle.distribution == ide_core::settings::GradleDistribution::Local
                && !home.is_empty()
                && !home.starts_with('~');
            let launcher = local.then(|| project.root.join(home).join("bin").join("gradle"));
            match launcher.map(|launcher| launcher.to_string_lossy().into_owned()) {
                Some(launcher) if is_stand_in(&launcher) => Ok(()),
                Some(launcher) => Err(refused(&launcher)),
                None => Err(refused("gradle")),
            }
        }
        Some(BuildTool::Gradle) | None => Ok(()),
    }
}

/// Refuses Build Project while the project has a Maven project or a Gradle build whose launcher
/// is not a stand-in in a temporary folder: the build runs them.
pub async fn check_build(host: &Host, window: &str) -> IdeResult<()> {
    for tool in [BuildTool::Maven, BuildTool::Gradle] {
        if let Some(launchers) = reported_launchers(host, window, tool, None).await?
            && !launchers.is_empty()
        {
            require_stand_ins(Some(launchers))?;
        }
    }
    Ok(())
}

/// Refuses a debug launch whose program is a build tool or a JVM outside a temporary folder.
pub fn check_debug(launch: &ide_dap::LaunchConfig) -> IdeResult<()> {
    match launch.program.as_deref() {
        Some(program) if is_guarded(program) && !is_stand_in(program) => Err(refused(program)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_is_a_stand_in_only_as_a_file_in_the_temporary_folder() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let mvn = folder.path().join("mvn");
        assert!(!is_stand_in(&mvn.to_string_lossy()), "a missing file");
        std::fs::write(&mvn, "#!/bin/sh\n").expect("the file is written");
        assert!(is_stand_in(&mvn.to_string_lossy()));
        assert!(!is_stand_in("mvn"), "a name looked up on PATH");
        assert!(!is_stand_in("/bin/sh"), "a file outside the temporary folder");
    }

    #[test]
    fn build_tools_and_jvms_are_guarded_by_name() {
        for command in ["mvn", "/usr/local/bin/mvn", "mvnw.cmd", "gradlew", "gradle.bat", "/jdk/bin/java", "java.exe"] {
            assert!(is_guarded(command), "{command}");
        }
        for command in ["sh", "/usr/bin/python3", "cargo", "javac-wrapper"] {
            assert!(!is_guarded(command), "{command}");
        }
        assert!(require_stand_ins(None).is_ok(), "a restricted project is refused by the backend itself");
        assert!(require_stand_ins(Some(Vec::new())).is_err(), "a build with no reported launcher");
        assert!(require_stand_ins(Some(vec!["mvn".to_owned()])).is_err());
    }
}
