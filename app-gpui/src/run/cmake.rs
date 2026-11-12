//! CMake configure and build configurations: they are builds, shown in the Build tool window instead
//! of a Run tab.

use crate::run::build_task::BuildRequest;
use ide_run::RunConfig;
use serde_json::json;

/// The action that builds the project, or one CMake build of it.
pub const BUILD_PROJECT_ACTION: &str = "build.buildProject";

/// Whether `config` configures or builds a CMake build directory.
pub fn is_cmake_build(config: &RunConfig) -> bool {
    config.kind == "cmake" && matches!(config.args.first().map(String::as_str), Some("-S" | "--build"))
}

/// The selected target's build, including saved configurations and CTest.
pub fn cmake_build_id(config: Option<&RunConfig>, configs: &[RunConfig]) -> Option<String> {
    let config = config?;
    if is_cmake_build(config) {
        if config.args.first().is_some_and(|arg| arg == "--build") {
            return Some(config.id.clone());
        }
        // Configure: the build of the same build directory.
        let build_id = format!("{}:build", config.id.strip_suffix(":configure")?);
        return configs.iter().find(|c| c.id == build_id && is_cmake_build(c)).map(|c| c.id.clone());
    }
    config
        .extras
        .before_launch
        .iter()
        .filter(|task| task.kind == "run" || task.kind == "build")
        .filter_map(|task| configs.iter().find(|c| c.id == task.target))
        .find(|c| is_cmake_build(c))
        .map(|c| c.id.clone())
}

/// The build of the CMake configuration `id`; `activate` shows the Build tool window.
pub fn build_cmake(id: &str, activate: bool) -> BuildRequest {
    BuildRequest { action: BUILD_PROJECT_ACTION.to_string(), arg: json!({ "cmake": id, "activate": activate }) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::config_types::RunConfigTypes;
    use crate::run::configs::{blank_config, validate_config};
    use ide_run::BeforeLaunchTask;

    fn config(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(id, id, kind);
        config.command = "cmake".into();
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        config.detected = true;
        config
    }

    fn with_task(mut config: RunConfig, kind: &str, target: &str) -> RunConfig {
        config.extras.before_launch = vec![BeforeLaunchTask { kind: kind.into(), target: target.into() }];
        config
    }

    /// Configure, build, a target build and the application built by it.
    fn configs() -> Vec<RunConfig> {
        let target = ["--build", "release", "--target", "hello", "--config", "Release"];
        vec![
            config("cmake:build:configure", "cmake", &["-S", ".", "-B", "build"]),
            config("cmake:build:build", "cmake", &["--build", "build"]),
            config("cmake:release:hello:build", "cmake", &target),
            with_task(config("hello", "cmake-application", &[]), "build", "cmake:release:hello:build"),
        ]
    }

    #[test]
    fn selects_the_target_build_configures_build_directory_and_ctests_prerequisite() {
        let configs = configs();
        let (configure, build, target, app) = (&configs[0], &configs[1], &configs[2], &configs[3]);
        assert_eq!(cmake_build_id(Some(configure), &configs).as_deref(), Some(build.id.as_str()));
        assert_eq!(cmake_build_id(Some(target), &configs).as_deref(), Some(target.id.as_str()));
        assert_eq!(cmake_build_id(Some(app), &configs).as_deref(), Some(target.id.as_str()));
        let ctest = with_task(config("ctest", "cmake", &["--test-dir", "build"]), "build", &build.id);
        assert_eq!(cmake_build_id(Some(&ctest), &configs).as_deref(), Some(build.id.as_str()));
        assert_eq!(cmake_build_id(Some(&config("script", "shell", &[])), &configs), None);
        assert_eq!(cmake_build_id(None, &configs), None);
        assert!(!is_cmake_build(&config("test", "cmake", &["--test-dir", "build"])));
    }

    #[test]
    fn validates_target_build_prerequisites_while_allowing_a_generic_project_build() {
        let (configs, types) = (configs(), RunConfigTypes::new());
        let app = configs[3].clone();
        assert!(!validate_config(&app, &configs, &types).contains_key("beforeLaunch"));
        assert!(validate_config(&app, &[], &types).contains_key("beforeLaunch"));
        let own = with_task(app.clone(), "build", &app.id);
        assert!(validate_config(&own, &configs, &types).contains_key("beforeLaunch"));
        let generic = with_task(app, "build", "");
        assert!(!validate_config(&generic, &configs, &types).contains_key("beforeLaunch"));
    }

    // Not from a spec: the build request names the configuration and whether to show the Build window.
    #[test]
    fn a_cmake_build_is_a_project_build_of_that_configuration() {
        let request = build_cmake("cmake:build:build", true);
        assert_eq!(request.action, "build.buildProject");
        assert_eq!(request.arg, json!({ "cmake": "cmake:build:build", "activate": true }));
    }
}
