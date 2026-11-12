//! IDEA-style "Application" run configurations for Java / Kotlin main classes (Spring Boot
//! applications get their own kind), and test configurations scoped to one class or method.

use crate::platform::project_roots::{RootFolder, is_test_path, root_for};
use crate::run::configs::blank_config;
use crate::run::java_launch::{CLASSPATH, TEST_CLASSPATH, jvm_module_dir};
use crate::run::jvm_targets::simple_class_name;
use ide_run::RunConfig;

/// What placing a JVM source file in the project reads: the run configurations, the build model's
/// module directories and the effective source roots.
#[derive(Clone, Copy, Debug, Default)]
pub struct JvmContext<'a> {
    pub configs: &'a [RunConfig],
    pub modules: &'a [&'a str],
    pub roots: &'a [RootFolder],
}

/// In a test source root, as IDEA's `getClasspathType` decides: the root holding the file (synced
/// from Gradle, detected or marked in Project Structure) wins; `src/test` only while no root is known
/// for the file.
fn is_test_source(rel_path: &str, roots: &[RootFolder]) -> bool {
    if root_for(roots, rel_path).is_some() {
        return is_test_path(roots, rel_path);
    }
    rel_path.starts_with("src/test/") || rel_path.contains("/src/test/")
}

/// Temporary "Application" configuration that runs `class_name` (a binary name).
pub fn application_config(class_name: &str, spring: bool, rel_path: &str, context: JvmContext) -> RunConfig {
    let module = jvm_module_dir(rel_path, context.configs, context.modules);
    let id = if module.is_empty() {
        format!("application:{class_name}")
    } else {
        format!("application:{module}:{class_name}")
    };
    let kind = if spring { "spring-boot" } else { "application" };
    let classpath = if is_test_source(rel_path, context.roots) { TEST_CLASSPATH } else { CLASSPATH };
    let mut config = blank_config(id, simple_class_name(class_name), kind);
    config.command = "java".to_string();
    config.args = vec!["-cp".to_string(), classpath.to_string(), class_name.to_string()];
    config.extras.module = Some(module);
    config.detected = true;
    let adapter = if rel_path.ends_with(".kt") { "kotlin-debug-adapter" } else { "java-debug" };
    config.debug_adapter = Some(adapter.to_string());
    config
}

/// Narrows a Maven / Gradle test configuration to a class (`method` unset) or one method: Surefire
/// `-Dtest=Class#method`, Gradle `--tests Class.method`. `file_config` is the file-level configuration
/// whose last argument is the file filter.
pub fn jvm_test_config(file_config: &RunConfig, class_name: &str, method: Option<&str>) -> Option<RunConfig> {
    let mut args = file_config.args.clone();
    args.pop();
    let simple = simple_class_name(class_name);
    let member = method.map(|method| format!("#{method}")).unwrap_or_default();
    match file_config.kind.as_str() {
        "maven" => {
            args.push(format!("-Dtest={class_name}{member}"));
            args.push("-Dsurefire.failIfNoSpecifiedTests=false".to_string());
        }
        "gradle" => {
            if args.last().is_some_and(|last| last == "--tests") {
                args.pop();
            }
            args.push("--tests".to_string());
            args.push(method.map_or_else(|| class_name.to_string(), |method| format!("{class_name}.{method}")));
        }
        _ => return None,
    }
    Some(RunConfig {
        id: format!("context:target:{}:{class_name}{member}", file_config.id),
        name: method.map_or_else(|| simple.to_string(), |method| format!("{simple}.{method}")),
        args,
        ..file_config.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::project_roots::FolderRole;
    use crate::run::configs::to_cwd;

    fn cfg(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(id, id, kind);
        config.command = if kind == "maven" { "./mvnw".to_string() } else { kind.to_string() };
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        config.detected = true;
        config
    }

    #[test]
    fn derives_the_module_directory() {
        assert_eq!(jvm_module_dir("src/main/java/a/App.java", &[], &[]), "");
        assert_eq!(jvm_module_dir("services/api/src/test/kotlin/a/T.kt", &[], &[]), "services/api");
        let mut maven = cfg("m", "maven", &[]);
        maven.cwd = to_cwd(Some("odd"));
        assert_eq!(jvm_module_dir("odd/App.java", &[maven], &[]), "odd");
    }

    #[test]
    fn takes_the_innermost_module_the_build_model_knows_like_ideas_file_index() {
        let modules = ["", "/w/billing", "/w/billing/modules/core", "legacy"];
        // `sourceSets.main.java.srcDirs = ['src']`: no `src/main/java` to guess from.
        assert_eq!(jvm_module_dir("legacy/src/a/App.java", &[], &modules), "legacy");
        let core = "/w/billing/modules/core/src/main/java/a/App.java";
        assert_eq!(jvm_module_dir(core, &[], &modules), "/w/billing/modules/core");
        // A module the model does not know yet (before the first sync): the source layout.
        assert_eq!(jvm_module_dir("services/api/src/main/java/a/App.java", &[], &modules), "services/api");
    }

    #[test]
    fn routes_kotlin_facade_and_file_jvm_name_application_configurations_to_kotlin() {
        for class_name in ["org.example.MainKt", "org.example.CustomFacade"] {
            let config = application_config(class_name, false, "src/main/kotlin/Main.kt", JvmContext::default());
            assert_eq!(config.debug_adapter.as_deref(), Some("kotlin-debug-adapter"));
            assert_eq!(config.args, ["-cp", "${classpath}", class_name]);
        }
    }

    #[test]
    fn creates_an_application_config_for_a_test_source_in_its_module() {
        let config = application_config("a.Tool", false, "m/src/test/java/a/Tool.java", JvmContext::default());
        assert_eq!(config.kind, "application");
        assert_eq!(config.extras.module.as_deref(), Some("m"));
        assert_eq!(config.cwd, None);
        assert_eq!(config.args, ["-cp", "${testClasspath}", "a.Tool"]);
        assert_eq!(config.id, "application:m:a.Tool");
    }

    #[test]
    fn scopes_maven_and_gradle_test_configs_to_a_class_or_method() {
        let mut file = cfg("f", "maven", &["test", "-Dtest=OrderTest"]);
        file.is_test = true;
        let nested = jvm_test_config(&file, "a.OrderTest$Nested", Some("fails")).expect("a Maven test");
        assert_eq!(nested.name, "Nested.fails");
        let expected = ["test", "-Dtest=a.OrderTest$Nested#fails", "-Dsurefire.failIfNoSpecifiedTests=false"];
        assert_eq!(nested.args, expected);
        let mut gradle = cfg("g", "gradle", &["test", "--tests", "*OrderTest"]);
        gradle.is_test = true;
        let class = jvm_test_config(&gradle, "a.OrderTest", None).expect("a Gradle test");
        assert_eq!(class.args, ["test", "--tests", "a.OrderTest"]);
    }

    // Not from a spec: a known source root decides the classpath, whatever the folder is called.
    #[test]
    fn a_marked_test_root_selects_the_test_classpath() {
        let root = |path: &str, kind| RootFolder { path: path.to_string(), kind, detected_from: None };
        let roots = [root("checks", FolderRole::Test), root("src/test/java", FolderRole::Source)];
        let context = JvmContext { roots: &roots, ..JvmContext::default() };
        assert_eq!(application_config("a.T", false, "checks/a/T.java", context).args[1], TEST_CLASSPATH);
        assert_eq!(application_config("a.T", false, "src/test/java/a/T.java", context).args[1], CLASSPATH);
        assert_eq!(jvm_test_config(&cfg("n", "npm", &["test"]), "a.T", None), None);
    }
}
