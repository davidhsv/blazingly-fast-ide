//! Build policies for configurations whose command is itself a build tool (Maven, Gradle).
//! Application and Spring Boot configurations build while their launch is prepared instead; this only
//! rewrites the command of an already chosen build-tool launch.

use ide_run::{BuildPolicy, RunConfig};

/// Lets a Maven build continue past compile errors.
const MAVEN_IGNORE_COMPILE_ERRORS: &str = "-Dmaven.compiler.failOnError=false";

/// `config` with its build policy applied to its command line.
pub fn apply_build_policy_to_command(config: &RunConfig) -> RunConfig {
    let (maven, gradle) = (config.kind == "maven", config.kind == "gradle");
    let has = |arg: &str| config.args.iter().any(|own| own == arg);
    match config.extras.build_policy {
        // Maven and Gradle both accept the clean task before their other goals.
        Some(BuildPolicy::Rebuild) if (maven || gradle) && !has("clean") => {
            let args = std::iter::once("clean".to_string()).chain(config.args.iter().cloned()).collect();
            RunConfig { args, ..config.clone() }
        }
        Some(BuildPolicy::MakeAndContinueOnErrors) if maven && !has(MAVEN_IGNORE_COMPILE_ERRORS) => {
            let mut next = config.clone();
            next.args.push(MAVEN_IGNORE_COMPILE_ERRORS.to_string());
            next
        }
        _ => config.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;

    fn cfg(kind: &str, args: &[&str], policy: BuildPolicy) -> RunConfig {
        let mut config = blank_config("m", "m", kind);
        config.command = "mvn".into();
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        config.extras.build_policy = Some(policy);
        config
    }

    #[test]
    fn puts_clean_first_for_maven_and_gradle_rebuild() {
        let maven = cfg("maven", &["-B", "spring-boot:run"], BuildPolicy::Rebuild);
        assert_eq!(apply_build_policy_to_command(&maven).args, ["clean", "-B", "spring-boot:run"]);
        let gradle = cfg("gradle", &["bootRun"], BuildPolicy::Rebuild);
        assert_eq!(apply_build_policy_to_command(&gradle).args, ["clean", "bootRun"]);
    }

    #[test]
    fn lets_maven_continue_on_compile_errors_only_when_asked() {
        let flag = MAVEN_IGNORE_COMPILE_ERRORS.to_string();
        let lenient = cfg("maven", &["-B", "spring-boot:run"], BuildPolicy::MakeAndContinueOnErrors);
        assert!(apply_build_policy_to_command(&lenient).args.contains(&flag));
        let strict = cfg("maven", &["-B", "spring-boot:run"], BuildPolicy::Make);
        assert!(!apply_build_policy_to_command(&strict).args.contains(&flag));
    }

    #[test]
    fn leaves_automatic_and_no_build_commands_alone() {
        for policy in [BuildPolicy::Automatic, BuildPolicy::NoBuild] {
            let config = cfg("maven", &["-B", "spring-boot:run"], policy);
            assert_eq!(apply_build_policy_to_command(&config), config);
        }
    }

    // Not from a spec: an explicit clean or flag is not added a second time.
    #[test]
    fn does_not_repeat_what_the_command_already_has() {
        let clean = cfg("gradle", &["clean", "build"], BuildPolicy::Rebuild);
        assert_eq!(apply_build_policy_to_command(&clean), clean);
        let lenient = cfg("maven", &[MAVEN_IGNORE_COMPILE_ERRORS], BuildPolicy::MakeAndContinueOnErrors);
        assert_eq!(apply_build_policy_to_command(&lenient), lenient);
    }
}
