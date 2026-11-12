//! JVM run configuration types: Application and Spring Boot
//! (`java [VM options] -cp ${classpath} <main class> [program arguments]`), and the VM options Maven /
//! Gradle configurations hand to the JVM the build forks (`spring-boot:run`, tests, `bootRun`) rather
//! than to the build tool's own JVM.

use crate::platform::command_line::{format_args, parse_args};
use crate::run::command_name::command_name;
use crate::run::config_types::{
    CommandLine, FieldControl, FieldOption, OptionsSource, Params, RunConfigType, RunConfigTypeField, TypeEnv,
};
use crate::run::configs::param;
use crate::run::java_launch::{CLASSPATH, ClasspathKind, TEST_CLASSPATH, parse_java_args};
use ide_run::RunConfig;

pub const PROFILES_ARG: &str = "--spring.profiles.active=";
/// The module of the project root in the `module` param (`RunConfig.module` is `""` for it).
pub const ROOT_MODULE: &str = ".";
const VM_OPTIONS_PLACEHOLDER: &str = "e.g. -Xmx1g -Dkey=value --add-opens java.base/java.lang=ALL-UNNAMED";
/// What the Update action does by default, and nothing when the IDE loses focus.
pub const UPDATE_POLICY_DEFAULTS: [(&str, &str); 2] = [("onUpdate", "ask"), ("onDeactivation", "none")];

fn value<'a>(params: &'a Params, key: &str) -> &'a str {
    params.get(key).map_or("", String::as_str)
}

/// VM options / program arguments as typed; on Windows a backslash is a path separator.
pub fn split_jvm_args(text: &str, windows: bool) -> Vec<String> {
    parse_args(text, windows)
}

fn vm_options_field(hint: Option<&str>) -> RunConfigTypeField {
    let field = RunConfigTypeField::new("vmOptions", "VM options")
        .control(FieldControl::Textarea)
        .rows(2)
        .mono()
        .placeholder(VM_OPTIONS_PLACEHOLDER);
    match hint {
        Some(hint) => field.hint(hint),
        None => field,
    }
}

fn profiles_field(hint: &str) -> RunConfigTypeField {
    RunConfigTypeField::new("activeProfiles", "Active profiles").mono().placeholder("e.g. dev,local").hint(hint)
}

/// The fields choosing what the Update action and losing focus do to a running application.
pub fn update_policy_fields(tomcat: bool) -> Vec<RunConfigTypeField> {
    let incremental = [
        FieldOption::new("resources", "Update resources"),
        FieldOption::new("classes", "Update classes and resources"),
    ];
    let mut on_update = vec![FieldOption::new("ask", "Show chooser")];
    on_update.extend(incremental.iter().cloned());
    if tomcat {
        on_update.push(FieldOption::new("redeploy", "Redeploy"));
    }
    on_update.push(FieldOption::new("restart", "Restart"));
    let mut on_deactivation = vec![FieldOption::new("none", "Do nothing")];
    on_deactivation.extend(incremental);
    vec![
        RunConfigTypeField::new("onUpdate", "On 'Update' action").select(on_update),
        RunConfigTypeField::new("onDeactivation", "On frame deactivation")
            .select(on_deactivation)
            .hint("When all project windows lose focus to another application"),
    ]
}

/// Main classes of the JVM configurations, offered while typing.
fn main_classes(configs: &[RunConfig]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for config in configs.iter().filter(|config| matches!(config.kind.as_str(), "application" | "spring-boot")) {
        let stored = param(config, "mainClass").filter(|main| !main.is_empty()).map(str::to_string);
        if let Some(main) = stored.or_else(|| parse_java_args(&config.args).main_class)
            && !out.contains(&main)
        {
            out.push(main);
        }
    }
    out
}

/// `java` in a JDK home (the `jre` param); the home itself when the path names `bin/java` already.
pub fn jre_java(home: &str, windows: bool) -> String {
    let lower = home.to_ascii_lowercase().replace('\\', "/");
    if lower.ends_with("/bin/java") || lower.ends_with("/bin/java.exe") {
        return home.to_string();
    }
    let java = if windows { "java.exe" } else { "java" };
    format!("{}/bin/{java}", home.trim_end_matches(['/', '\\']))
}

fn jvm_command_line(spring: bool, params: &Params, env: &TypeEnv) -> CommandLine {
    let main = value(params, "mainClass").trim();
    let profiles = if spring { value(params, "activeProfiles").trim() } else { "" };
    let (jre, executable) = (value(params, "jre"), value(params, "executable"));
    let command = if !jre.is_empty() {
        jre_java(jre, env.windows)
    } else if !executable.is_empty() {
        executable.to_string()
    } else {
        "java".to_string()
    };
    let classpath = Some(value(params, "classpath")).filter(|classpath| !classpath.is_empty()).unwrap_or(CLASSPATH);
    let mut args = split_jvm_args(value(params, "vmOptions"), env.windows);
    args.extend(["-cp".to_string(), classpath.to_string()]);
    if !main.is_empty() {
        args.push(main.to_string());
    }
    if !profiles.is_empty() {
        args.push(format!("{PROFILES_ARG}{profiles}"));
    }
    args.extend(split_jvm_args(value(params, "programArgs"), env.windows));
    CommandLine::new(command, args)
}

/// The form's values of a configuration detected or saved before the fields existed:
/// `java [vm] -cp <cp> Main [args]`.
fn jvm_params_from(spring: bool, config: &RunConfig) -> Option<Params> {
    if !command_name(&config.command).eq_ignore_ascii_case("java") {
        return None;
    }
    let parsed = parse_java_args(&config.args);
    let classpath = match parsed.classpath {
        Some(ClasspathKind::Runtime) => CLASSPATH.to_string(),
        Some(ClasspathKind::Test) => TEST_CLASSPATH.to_string(),
        None => parsed.explicit_classpath?,
    };
    let main_class = parsed.main_class?;
    let mut rest = parsed.program_args.as_slice();
    let mut params = Params::new();
    if spring && let Some(profiles) = rest.first().and_then(|first| first.strip_prefix(PROFILES_ARG)) {
        if !profiles.is_empty() {
            params.insert("activeProfiles".to_string(), profiles.to_string());
        }
        rest = &rest[1..];
    }
    if config.command != "java" {
        params.insert("executable".to_string(), config.command.clone());
    }
    if let Some(module) = &config.extras.module {
        let module = if module.is_empty() { ROOT_MODULE } else { module.as_str() };
        params.insert("module".to_string(), module.to_string());
    }
    params.insert("mainClass".to_string(), main_class);
    params.insert("vmOptions".to_string(), format_args(&parsed.vm_args));
    params.insert("programArgs".to_string(), format_args(rest));
    params.insert("classpath".to_string(), classpath);
    Some(params)
}

fn jvm_application_type(spring: bool) -> RunConfigType {
    let (id, label, icon) =
        if spring { ("spring-boot", "Spring Boot", "spring-boot") } else { ("application", "Application", "class") };
    let description = if spring { "Run a Spring Boot application" } else { "Run a Java main class" };
    let mut fields = vec![
        RunConfigTypeField::new("mainClass", "Main class")
            .mono()
            .required()
            .placeholder("e.g. com.example.Application")
            .suggestions(|configs, _| main_classes(configs)),
        vm_options_field(None),
        RunConfigTypeField::new("programArgs", "Program arguments").mono(),
    ];
    if spring {
        fields.push(profiles_field("Comma separated; passed as --spring.profiles.active"));
        fields.extend(update_policy_fields(false));
    }
    fields.extend([
        // IDEA's JRE: the module's JDK unless one installed JDK is chosen.
        RunConfigTypeField::new("jre", "JRE")
            .select(vec![FieldOption::new("", "Default (the module's JDK)")])
            .load_options(OptionsSource::InstalledJdks),
        // IDEA's "Use classpath of module": the module whose classpath and build the run uses.
        RunConfigTypeField::new("module", "Use classpath of module")
            .select(vec![FieldOption::new("", "Find by main class")])
            .load_options(OptionsSource::BuildModules),
        RunConfigTypeField::new("classpath", "Classpath").select(vec![
            FieldOption::new(CLASSPATH, "Runtime classpath of the module"),
            FieldOption::new(TEST_CLASSPATH, "Test classpath of the module"),
        ]),
    ]);
    let mut defaults = vec![("classpath", CLASSPATH)];
    if spring {
        defaults.extend(UPDATE_POLICY_DEFAULTS);
    }
    RunConfigType::new(id, label, icon, move |params, env| Some(jvm_command_line(spring, params, env)))
        .description(description)
        .defaults(&defaults)
        .debug_adapter("java-debug")
        .fields(fields)
        .params_from(move |config| jvm_params_from(spring, config))
}

pub fn application_type() -> RunConfigType {
    jvm_application_type(false)
}

pub fn spring_boot_type() -> RunConfigType {
    jvm_application_type(true)
}

/// One value for a Maven property holding JVM arguments (`spring-boot.run.jvmArguments`, `argLine`):
/// the plugins split it on whitespace, honouring "…" and '…'.
pub fn join_property_args(args: &[String]) -> String {
    let quoted = |arg: &String| {
        if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"' || c == '\'') {
            arg.clone()
        } else if arg.contains('"') {
            format!("'{arg}'")
        } else {
            format!("\"{arg}\"")
        }
    };
    args.iter().map(quoted).collect::<Vec<_>>().join(" ")
}

fn words(tasks: &str) -> impl Iterator<Item = &str> {
    tasks.split_whitespace()
}

fn is_spring_boot_run(goal: &str) -> bool {
    // `group:spring-boot-maven-plugin:version:run`.
    let plugin = goal.strip_suffix(":run").and_then(|rest| rest.rsplit_once(':')).map(|(plugin, _)| plugin);
    goal == "spring-boot:run" || plugin.is_some_and(|plugin| plugin.ends_with(":spring-boot-maven-plugin"))
}

fn is_maven_test_goal(goal: &str) -> bool {
    matches!(goal, "test" | "verify" | "integration-test" | "surefire:test" | "failsafe:integration-test")
}

fn gradle_task(task: &str) -> &str {
    task.rsplit(':').next().unwrap_or(task)
}

fn is_boot_run(task: &str) -> bool {
    gradle_task(task) == "bootRun"
}

fn is_gradle_jvm_task(task: &str) -> bool {
    matches!(gradle_task(task), "test" | "bootRun" | "run" | "integrationTest" | "check")
}

/// The VM options and profiles fields of a Maven configuration: shown for the goals that fork a JVM.
pub fn maven_jvm_fields() -> Vec<RunConfigTypeField> {
    let forks = |p: &Params| words(value(p, "tasks")).any(|goal| is_spring_boot_run(goal) || is_maven_test_goal(goal));
    let boots = |p: &Params| words(value(p, "tasks")).any(is_spring_boot_run);
    vec![
        vm_options_field(Some("Passed to the JVM that spring-boot:run or the tests start, not to Maven"))
            .visible(forks),
        profiles_field("Comma separated; passed as -Dspring-boot.run.profiles").visible(boots),
    ]
}

/// Arguments a Maven configuration adds for the JVM its goals fork.
pub fn maven_jvm_args(params: &Params, env: &TypeEnv) -> Vec<String> {
    let goals: Vec<&str> = words(value(params, "tasks")).collect();
    let vm = split_jvm_args(value(params, "vmOptions"), env.windows);
    let mut out = Vec::new();
    if goals.iter().any(|goal| is_spring_boot_run(goal)) {
        if !vm.is_empty() {
            out.push(format!("-Dspring-boot.run.jvmArguments={}", join_property_args(&vm)));
        }
        let profiles = value(params, "activeProfiles").trim();
        if !profiles.is_empty() {
            out.push(format!("-Dspring-boot.run.profiles={profiles}"));
        }
    } else if !vm.is_empty() && goals.iter().any(|goal| is_maven_test_goal(goal)) {
        out.push(format!("-DargLine={}", join_property_args(&vm)));
    }
    out
}

/// The VM options and profiles fields of a Gradle configuration.
pub fn gradle_jvm_fields() -> Vec<RunConfigTypeField> {
    let forks = |p: &Params| words(value(p, "tasks")).any(is_gradle_jvm_task);
    let boots = |p: &Params| words(value(p, "tasks")).any(is_boot_run);
    vec![
        vm_options_field(Some("Passed to the JVMs the tasks start (bootRun, run, test), not to Gradle")).visible(forks),
        profiles_field("Comma separated; passed as -Dspring.profiles.active").visible(boots),
    ]
}

/// VM options (and profiles, as `-Dspring.profiles.active`) of a Gradle configuration's forked JVMs.
pub fn gradle_jvm_args(params: &Params, env: &TypeEnv) -> Vec<String> {
    let tasks: Vec<&str> = words(value(params, "tasks")).collect();
    if !tasks.iter().any(|task| is_gradle_jvm_task(task)) {
        return Vec::new();
    }
    let mut args = split_jvm_args(value(params, "vmOptions"), env.windows);
    let profiles = value(params, "activeProfiles").trim();
    if tasks.iter().any(|task| is_boot_run(task)) && !profiles.is_empty() {
        args.push(format!("-Dspring.profiles.active={profiles}"));
    }
    args
}

/// What launching a Gradle configuration needs for its VM options. Gradle has no command-line switch
/// for them: an init script adds them to the JavaExec / Test tasks. `Ok(None)`: nothing to add;
/// `Ok(Some(args))`: have the backend write the script for `args`, then [`with_jvm_args_script`].
pub fn gradle_jvm_args_script(config: &RunConfig, env: &TypeEnv) -> Result<Option<Vec<String>>, String> {
    let jvm_args = gradle_jvm_args(&config.extras.params, env);
    if jvm_args.is_empty() {
        return Ok(None);
    }
    if config.extras.target.as_deref().is_some_and(|target| !target.is_empty()) {
        return Err("VM options of Gradle configurations are not applied on run targets".to_string());
    }
    Ok(Some(jvm_args))
}

/// `config` with the init script that carries its VM options.
pub fn with_jvm_args_script(config: &RunConfig, script: &str) -> RunConfig {
    let args = [vec!["--init-script".to_string(), script.to_string()], config.args.clone()].concat();
    RunConfig { args, ..config.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::config_types::RunConfigTypes;
    use crate::run::configs::blank_config;
    use crate::run::java_debug::jdwp_launch;
    use crate::run::java_launch::join_java_args;
    use crate::run::types::{form_params, new_config_of_type, register_builtin_types, with_params};

    const REPORTER_VM: &str = "-Ddemo.flag=on --add-opens java.base/java.lang=ALL-UNNAMED";
    const SCRIPT: &str = "/tmp/openidea-gradle/jvm-args-1.init.gradle";

    fn params(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn cfg(kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config("c", "c", kind);
        config.command = "java".into();
        config.args = strings(args);
        config.debug_adapter = Some("java-debug".into());
        config
    }

    fn builtin_types() -> RunConfigTypes {
        let mut types = RunConfigTypes::new();
        register_builtin_types(&mut types);
        types
    }

    fn line(ty: &RunConfigType, pairs: &[(&str, &str)], env: &TypeEnv) -> CommandLine {
        ty.line(&params(pairs), env).expect("a command line")
    }

    #[test]
    fn puts_vm_options_before_the_main_class_and_program_arguments_after_it() {
        for ty in [application_type(), spring_boot_type()] {
            let pairs = [
                ("mainClass", "com.example.Application"),
                ("vmOptions", REPORTER_VM),
                ("programArgs", "--server.port=8081 'two words'"),
                ("classpath", "${classpath}"),
            ];
            let found = line(&ty, &pairs, &TypeEnv::default());
            let expected = strings(&[
                "-Ddemo.flag=on",
                "--add-opens",
                "java.base/java.lang=ALL-UNNAMED",
                "-cp",
                "${classpath}",
                "com.example.Application",
                "--server.port=8081",
                "two words",
            ]);
            assert_eq!(found, CommandLine::new("java", expected));
            let parsed = parse_java_args(&found.args);
            assert_eq!(parsed.vm_args, ["-Ddemo.flag=on", "--add-opens", "java.base/java.lang=ALL-UNNAMED"]);
            assert_eq!(parsed.main_class.as_deref(), Some("com.example.Application"));
            assert_eq!(parsed.program_args, ["--server.port=8081", "two words"]);
        }
    }

    #[test]
    fn reads_vm_options_over_several_lines_and_with_quoted_spaces() {
        let pairs = [("mainClass", "a.Main"), ("vmOptions", "-Xmx1g\n-Dapp.name=\"My App\"\n  -Dempty=\"\"")];
        let found = line(&application_type(), &pairs, &TypeEnv::default());
        assert_eq!(found.args[..3], ["-Xmx1g", "-Dapp.name=My App", "-Dempty="]);
        let vm_args = parse_java_args(&found.args).vm_args;
        assert_eq!(join_java_args(&vm_args), "-Xmx1g \"-Dapp.name=My App\" -Dempty=");
    }

    #[test]
    fn keeps_windows_paths_as_typed_on_windows() {
        let typed = r#"-javaagent:C:\tools\agent.jar "-Dlog=C:\Program Files\x.log""#;
        assert_eq!(parse_args(typed, true), [r"-javaagent:C:\tools\agent.jar", r"-Dlog=C:\Program Files\x.log"]);
        assert_eq!(parse_args(r#"-Dq=\"x\" "a \"b\"""#, true), ["-Dq=\"x\"", "a \"b\""]);
        // Elsewhere a backslash still escapes.
        assert_eq!(parse_args(r"a\ b", false), ["a b"]);
    }

    #[test]
    fn passes_spring_boots_active_profiles_before_the_program_arguments() {
        let env = TypeEnv::default();
        let pairs = [("mainClass", "a.App"), ("activeProfiles", "dev,local"), ("programArgs", "--x")];
        let boot = line(&spring_boot_type(), &pairs, &env);
        assert_eq!(boot.args, ["-cp", "${classpath}", "a.App", "--spring.profiles.active=dev,local", "--x"]);
        // Application has no profiles field.
        let plain = line(&application_type(), &[("mainClass", "a.App"), ("activeProfiles", "dev")], &env);
        assert_eq!(plain.args, ["-cp", "${classpath}", "a.App"]);
    }

    #[test]
    fn uses_the_test_classpath_when_chosen() {
        let pairs = [("mainClass", "a.T"), ("classpath", "${testClasspath}")];
        let found = line(&application_type(), &pairs, &TypeEnv::default());
        assert_eq!(found.args, ["-cp", "${testClasspath}", "a.T"]);
    }

    #[test]
    fn opens_a_detected_spring_boot_configuration_in_the_form() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let mut detected = cfg("spring-boot", &["-cp", "${classpath}", "com.example.DemoApplication"]);
        detected.detected = true;
        let expected = params(&[
            ("mainClass", "com.example.DemoApplication"),
            ("vmOptions", ""),
            ("programArgs", ""),
            ("classpath", "${classpath}"),
        ]);
        assert_eq!(form_params(&detected, types.get("spring-boot"), &env), Some(expected));
    }

    #[test]
    fn splits_an_older_configurations_arguments_into_vm_options_main_class_profiles_and_program_arguments() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let ty = types.get("spring-boot");
        let args = ["-Xmx1g", "-Dname=a b", "-cp", "${classpath}", "a.App", "--spring.profiles.active=dev", "x y"];
        let old = cfg("spring-boot", &args);
        let found = form_params(&old, ty, &env).expect("the form applies");
        let expected = params(&[
            ("mainClass", "a.App"),
            ("vmOptions", "-Xmx1g '-Dname=a b'"),
            ("programArgs", "'x y'"),
            ("classpath", "${classpath}"),
            ("activeProfiles", "dev"),
        ]);
        assert_eq!(found, expected);
        // Editing VM options keeps the rest of the command line.
        let mut edited = found.clone();
        edited.insert("vmOptions".into(), REPORTER_VM.into());
        let next = with_params(&old, edited, ty, &env);
        let expected = [
            "-Ddemo.flag=on",
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "-cp",
            "${classpath}",
            "a.App",
            "--spring.profiles.active=dev",
            "x y",
        ];
        assert_eq!(next.args, expected);
        assert_eq!(param(&next, "vmOptions"), Some(REPORTER_VM));
    }

    #[test]
    fn keeps_an_explicit_classpath_and_edits_other_command_lines_as_a_command() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let explicit = cfg("application", &["-cp", "lib/*", "a.Main"]);
        let found = form_params(&explicit, types.get("application"), &env).expect("the form applies");
        assert_eq!(value(&found, "classpath"), "lib/*");
        let mut script = cfg("spring-boot", &["a.Main"]);
        script.command = "./run.sh".into();
        assert_eq!(form_params(&script, types.get("spring-boot"), &env), None);
        assert_eq!(form_params(&cfg("spring-boot", &["a.Main"]), types.get("spring-boot"), &env), None);
    }

    #[test]
    fn prefers_the_stored_params() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let mut config = cfg("spring-boot", &["-Xss2m", "-cp", "${classpath}", "a.App"]);
        config.extras.params = params(&[("mainClass", "a.App"), ("vmOptions", "-Xss2m")]);
        assert_eq!(form_params(&config, types.get("spring-boot"), &env), Some(config.extras.params.clone()));
    }

    #[test]
    fn applies_the_templates_vm_options_to_a_new_configuration() {
        let mut template = cfg("spring-boot", &[]);
        template.id = "template:spring-boot".into();
        template.extras.params = params(&[("classpath", "${classpath}"), ("vmOptions", "-Xmx512m")]);
        let config = new_config_of_type(&spring_boot_type(), &[], Some(&template), None, &TypeEnv::default());
        assert_eq!(config.kind, "spring-boot");
        assert_eq!(config.debug_adapter.as_deref(), Some("java-debug"));
        assert_eq!(param(&config, "vmOptions"), Some("-Xmx512m"));
        assert_eq!(config.args, ["-Xmx512m", "-cp", "${classpath}"]);
    }

    #[test]
    fn passes_spring_boot_runs_vm_options_and_profiles_to_the_application_not_to_maven() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let maven = types.get("maven").expect("the Maven type");
        let vm_options = format!("{REPORTER_VM} '-Dname=a b'");
        let pairs = [("tasks", "spring-boot:run"), ("vmOptions", vm_options.as_str()), ("activeProfiles", "dev")];
        let found = line(maven, &pairs, &env);
        let jvm_arguments = concat!(
            "-Dspring-boot.run.jvmArguments=-Ddemo.flag=on --add-opens java.base/java.lang=ALL-UNNAMED ",
            "\"-Dname=a b\"",
        );
        assert_eq!(found.args, ["spring-boot:run", jvm_arguments, "-Dspring-boot.run.profiles=dev"]);
        // Debugging adds the JDWP agent to the same property.
        let mut config = cfg("maven", &[]);
        config.command = "mvn".into();
        config.args = found.args;
        let debug = jdwp_launch(&config, None).expect("a debug launch");
        assert!(debug.args[1].starts_with("-Dspring-boot.run.jvmArguments=-Ddemo.flag=on "));
        assert!(debug.args[1].contains(" -agentlib:jdwp="));
    }

    #[test]
    fn passes_test_goals_vm_options_to_surefire_and_nothing_for_other_goals() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let maven = types.get("maven").expect("the Maven type");
        let test = line(maven, &[("tasks", "test"), ("vmOptions", "-Xmx1g")], &env);
        assert_eq!(test.args, ["test", "-DargLine=-Xmx1g"]);
        assert_eq!(line(maven, &[("tasks", "package"), ("vmOptions", "-Xmx1g")], &env).args, ["package"]);
        let vm = maven.fields.iter().find(|field| field.key == "vmOptions").expect("the VM options field");
        assert!(vm.is_visible(&params(&[("tasks", "clean spring-boot:run")])));
        assert!(!vm.is_visible(&params(&[("tasks", "package")])));
    }

    #[test]
    fn quotes_property_values_the_plugins_split() {
        let args = strings(&["-Xmx1g", "-Dp=C:\\Program Files\\x", "say \"hi\"", ""]);
        assert_eq!(join_property_args(&args), "-Xmx1g \"-Dp=C:\\Program Files\\x\" 'say \"hi\"' \"\"");
    }

    #[test]
    fn hands_gradles_vm_options_to_an_init_script_for_the_forked_jvms() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let pairs = [("tasks", "bootRun"), ("vmOptions", REPORTER_VM), ("activeProfiles", "dev"), ("wrapper", "true")];
        let form = params(&pairs);
        let opens = "java.base/java.lang=ALL-UNNAMED";
        let jvm_args = strings(&["-Ddemo.flag=on", "--add-opens", opens, "-Dspring.profiles.active=dev"]);
        assert_eq!(gradle_jvm_args(&form, &env), jvm_args);
        assert!(gradle_jvm_args(&params(&[("tasks", "build"), ("vmOptions", "-Xmx1g")]), &env).is_empty());
        let config = with_params(&cfg("gradle", &[]), form, types.get("gradle"), &env);
        // Gradle's own command line is unchanged.
        assert_eq!(config.args, ["bootRun"]);
        assert_eq!(gradle_jvm_args_script(&config, &env), Ok(Some(jvm_args.clone())));
        let launched = with_jvm_args_script(&config, SCRIPT);
        assert_eq!(launched.args, ["--init-script", SCRIPT, "bootRun"]);
        // Debugging keeps them: the BootRun init script gives the forked JVM a dynamic port.
        let debug = jdwp_launch(&config, None).expect("a debug launch");
        assert_eq!(gradle_jvm_args_script(&debug, &env), Ok(Some(jvm_args)));
        let debug = with_jvm_args_script(&debug, SCRIPT);
        assert_eq!(debug.args, ["--init-script", SCRIPT, "bootRun", "-Dopenidea.run.debug=true"]);
    }

    #[test]
    fn leaves_gradle_configurations_without_vm_options_alone() {
        let (types, env) = (builtin_types(), TypeEnv::default());
        let plain = with_params(&cfg("gradle", &[]), params(&[("tasks", "bootRun")]), types.get("gradle"), &env);
        assert_eq!(gradle_jvm_args_script(&plain, &env), Ok(None));
        let mut remote = cfg("gradle", &[]);
        remote.extras.target = Some("ssh-1".into());
        let form = params(&[("tasks", "bootRun"), ("vmOptions", "-Xmx1g")]);
        let remote = with_params(&remote, form, types.get("gradle"), &env);
        assert!(gradle_jvm_args_script(&remote, &env).is_err_and(|message| message.contains("run targets")));
    }

    #[test]
    fn the_build_wrapper_follows_the_host_that_runs_the_build() {
        let types = builtin_types();
        let wrappers = |windows: bool| {
            let env = TypeEnv { windows, ..TypeEnv::default() };
            ["gradle", "maven"].map(|kind| {
                let ty = types.get(kind).expect("a build tool type");
                line(ty, &[("wrapper", "true"), ("tasks", "test")], &env).command
            })
        };
        assert_eq!(wrappers(true), ["gradlew.bat", "mvnw.cmd"]);
        assert_eq!(wrappers(false), ["./gradlew", "./mvnw"]);
    }

    // Not from a spec: a JDK home becomes its launcher, and a path to the launcher is kept.
    #[test]
    fn the_jre_param_names_a_jdk_home_or_its_launcher() {
        assert_eq!(jre_java("/jdks/21/", false), "/jdks/21/bin/java");
        assert_eq!(jre_java("C:\\jdks\\21", true), "C:\\jdks\\21/bin/java.exe");
        assert_eq!(jre_java("C:\\jdks\\21\\bin\\JAVA.exe", true), "C:\\jdks\\21\\bin\\JAVA.exe");
        let pairs = [("mainClass", "a.Main"), ("jre", "/jdks/21")];
        assert_eq!(line(&application_type(), &pairs, &TypeEnv::default()).command, "/jdks/21/bin/java");
    }
}
