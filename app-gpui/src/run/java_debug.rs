//! Debugging Maven / Gradle run configurations (IDEA debugs those too): the build tool starts the JVM
//! with a JDWP agent that waits for a debugger (`spring-boot:run` → `-Dspring-boot.run.jvmArguments`,
//! Surefire → `-Dmaven.surefire.debug`, Gradle → `--debug-jvm`); when the agent reports its port, the
//! Java debugger attaches. Output and the test tree stay in the Run tab.
//!
//! Maven goals that run the application inside Maven's JVM (`tomcat7:run`, `jetty:run`, `exec:java`)
//! get the agent through `MAVEN_OPTS`; any other configuration set to the Java debugger (a script such
//! as `catalina.sh run`) through `JAVA_TOOL_OPTIONS`, which every JVM reads. A plugin's run detector
//! names the arguments that start its JVM with a waiting agent (`debugArgs`, e.g. Bazel's
//! `--java_debug`).

use crate::run::build_command::apply_build_policy_to_command;
use crate::run::configs::{cwd_of, param};
use crate::run::gradle_run::{DELEGATED_DEBUG, DELEGATED_MAIN};
use crate::run::java_launch::{JDWP_AGENT, is_jvm_application, jdwp_listening_port};
use ide_core::RunId;
use ide_run::RunConfig;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

const JVM_ADAPTERS: [&str; 2] = ["java-debug", "kotlin-debug-adapter"];

fn is_maven_test_goal(arg: &str) -> bool {
    matches!(arg, "test" | "verify" | "integration-test" | "surefire:test" | "failsafe:integration-test")
}

/// The task name of a Gradle task path (`:app:bootRun` → `bootRun`).
fn gradle_task(arg: &str) -> &str {
    arg.rsplit(':').next().unwrap_or(arg)
}

fn is_gradle_jvm_task(arg: &str) -> bool {
    matches!(gradle_task(arg), "test" | "bootRun" | "run" | "integrationTest" | "check")
}

/// A plugin goal that runs the application in Maven's own JVM (`tomcat7:run`, `jetty:run-war`,
/// `exec:java`).
fn is_maven_in_process_goal(arg: &str) -> bool {
    let word = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    let Some((plugin, goal)) = arg.rsplit_once(':') else { return false };
    matches!(goal, "run" | "run-war" | "run-exploded" | "java") && plugin.split(':').all(word)
}

/// The command is Maven or its wrapper, whatever the configuration's kind.
fn is_maven_command(config: &RunConfig) -> bool {
    let name = config.command.rsplit('/').next().unwrap_or_default();
    config.kind == "maven" || matches!(name, "mvn" | "mvnw" | "mvn.cmd" | "mvnw.cmd")
}

fn is_gradle_command(config: &RunConfig) -> bool {
    let name = config.command.rsplit('/').next().unwrap_or_default();
    config.kind == "gradle" || matches!(name, "gradle" | "gradlew" | "gradle.bat" | "gradlew.bat")
}

fn jvm_adapter(config: &RunConfig) -> bool {
    config.debug_adapter.as_deref().is_some_and(|adapter| JVM_ADAPTERS.contains(&adapter))
}

/// `env` with `value` added to the options in `key`.
fn with_option(env: &BTreeMap<String, String>, key: &str, value: &str) -> BTreeMap<String, String> {
    let mut env = env.clone();
    let current = env.get(key).map(|current| current.trim()).unwrap_or_default();
    let next = if current.is_empty() { value.to_string() } else { format!("{current} {value}") };
    env.insert(key.to_string(), next);
    env
}

/// `args` with `value` added to the system property `key`.
fn with_property(args: &[String], key: &str, value: &str) -> Vec<String> {
    let prefix = format!("-D{key}=");
    let mut out = args.to_vec();
    match out.iter_mut().find(|arg| arg.starts_with(&prefix)) {
        Some(arg) => {
            let current = arg[prefix.len()..].trim().to_string();
            *arg = if current.is_empty() { format!("{prefix}{value}") } else { format!("{prefix}{current} {value}") };
        }
        None => out.push(format!("{prefix}{value}")),
    }
    out
}

/// The `debugArgs` of a plugin's run detector that declared an attaching debugger.
fn plugin_debug_args(config: &RunConfig) -> Option<Vec<String>> {
    let raw = param(config, "debugArgs").filter(|raw| !raw.is_empty() && jvm_adapter(config))?;
    let args: Vec<String> = serde_json::from_str(raw).ok()?;
    (!args.is_empty()).then_some(args)
}

/// `config` with `arg` appended, unless it has it.
fn with_arg(config: RunConfig, arg: &str) -> RunConfig {
    if config.args.iter().any(|own| own == arg) {
        return config;
    }
    let mut next = config;
    next.args.push(arg.to_string());
    next
}

/// The configuration with a waiting JDWP agent, or `None` when it starts no debuggable JVM.
/// `jdwp_port`: pin a Surefire / Failsafe fork to this port — its "Listening for transport" line stays
/// buffered until the tests run, which `suspend=y` prevents, so the port must be known without it.
pub fn jdwp_launch(config: &RunConfig, jdwp_port: Option<u32>) -> Option<RunConfig> {
    static BOOT_PLUGIN_RUN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r":spring-boot-maven-plugin:[^:]*:run$").expect("the Spring Boot goal pattern is valid")
    });
    let mut config = apply_build_policy_to_command(config);
    if config.kind == "tomcat" {
        config.extras.params.insert("tomcatDebug".to_string(), "true".to_string());
        return Some(config);
    }
    let has = |args: &[String], test: &dyn Fn(&str) -> bool| args.iter().any(|arg| test(arg));
    if config.kind == "plugin" {
        let extra = plugin_debug_args(&config)?;
        if !extra.iter().all(|arg| config.args.contains(arg)) {
            config.args.extend(extra);
        }
        return Some(config);
    }
    // A single source file (`java Hi.java`): the agent goes before the file. Compiled first
    // (`javac … && java -cp …`): through `JDK_JAVA_OPTIONS`, which only the `java` launcher reads.
    if config.kind == "java-file" {
        if config.args.is_empty() {
            config.env = with_option(&config.env, "JDK_JAVA_OPTIONS", JDWP_AGENT);
        } else {
            config.args.insert(0, JDWP_AGENT.to_string());
        }
        return Some(config);
    }
    if is_maven_command(&config) {
        if has(&config.args, &|arg| arg == "spring-boot:run" || BOOT_PLUGIN_RUN.is_match(arg)) {
            config.args = with_property(&config.args, "spring-boot.run.jvmArguments", JDWP_AGENT);
            return Some(config);
        }
        if config.is_test || has(&config.args, &is_maven_test_goal) {
            let failsafe = has(&config.args, &|arg| {
                arg.contains("integration-test") || arg.contains("verify") || arg.contains("failsafe")
            });
            let agent = match jdwp_port.filter(|port| *port != 0) {
                Some(port) => JDWP_AGENT.replace("address=127.0.0.1:0", &format!("address=127.0.0.1:{port}")),
                None => JDWP_AGENT.to_string(),
            };
            config.args = with_property(&config.args, "maven.surefire.debug", &agent);
            if failsafe {
                config.args = with_property(&config.args, "maven.failsafe.debug", &agent);
            }
            return Some(config);
        }
        if jvm_adapter(&config) || has(&config.args, &is_maven_in_process_goal) {
            config.env = with_option(&config.env, "MAVEN_OPTS", JDWP_AGENT);
            return Some(config);
        }
        return None;
    }
    if is_gradle_command(&config) {
        // A delegated Application: its init script adds the agent (port 0) to the JavaExec JVM. Gradle
        // Test forks test workers and BootRun forks the application: their init scripts give those a
        // dynamic JDWP port.
        let scripted = config.is_test
            || has(&config.args, &|arg| {
                arg.starts_with(DELEGATED_MAIN) || matches!(gradle_task(arg), "test" | "integrationTest" | "bootRun")
            });
        if scripted {
            return Some(with_arg(config, DELEGATED_DEBUG));
        }
        if !has(&config.args, &is_gradle_jvm_task) {
            return None;
        }
        return Some(with_arg(config, "--debug-jvm"));
    }
    // Application / Spring Boot configurations are launched by java-debug itself.
    if jvm_adapter(&config) && !is_jvm_application(&config.kind) {
        config.env = with_option(&config.env, "JAVA_TOOL_OPTIONS", JDWP_AGENT);
        return Some(config);
    }
    None
}

/// A Maven test launch: its Surefire / Failsafe fork holds the "Listening" line, so its JDWP port is
/// pinned to a free port asked from the backend before [`AgentDebug::start`].
pub fn needs_pinned_port(config: &RunConfig) -> bool {
    is_maven_command(config) && (config.is_test || config.args.iter().any(|arg| is_maven_test_goal(arg)))
}

/// A debugger attach to a JVM waiting on a JDWP port (the `debug.attach` action's argument).
#[derive(Clone, Debug, PartialEq)]
pub struct AttachRequest {
    pub name: String,
    /// The application's module (a delegated run's working directory is the Gradle build).
    pub module: String,
    pub host_name: String,
    pub port: u32,
    /// The run whose process the debugger belongs to.
    pub process: Option<RunId>,
    pub run_config: RunConfig,
    pub show: Option<bool>,
}

/// Runs a configuration with a JDWP agent and attaches the Java debugger to every JVM it starts.
#[derive(Clone, Debug)]
pub struct AgentDebug {
    config: RunConfig,
    show: Option<bool>,
    pinned: Option<u32>,
    started: bool,
    process: Option<RunId>,
    /// Ports reported before the session was known.
    waiting: Vec<u32>,
    attached: Vec<u32>,
}

impl AgentDebug {
    /// The debug run of `config` and what to start for it (the tab still shows `config`); `None` when
    /// it starts no debuggable JVM. `pinned`: the free port of [`needs_pinned_port`], if one was got.
    pub fn start(config: &RunConfig, pinned: Option<u32>, show: Option<bool>) -> Option<(Self, RunConfig)> {
        let launch = jdwp_launch(config, pinned)?;
        let debug = Self {
            config: config.clone(),
            show,
            pinned,
            started: false,
            process: None,
            waiting: Vec::new(),
            attached: Vec::new(),
        };
        Some((debug, launch))
    }

    fn attach(&mut self, port: u32) -> Option<AttachRequest> {
        if self.attached.contains(&port) {
            return None;
        }
        self.attached.push(port);
        let module = self.config.extras.module.as_deref().or_else(|| cwd_of(&self.config)).unwrap_or_default();
        Some(AttachRequest {
            name: self.config.name.clone(),
            module: module.to_string(),
            host_name: "127.0.0.1".to_string(),
            port,
            process: self.process,
            run_config: self.config.clone(),
            show: self.show,
        })
    }

    /// An output line of the run: the attach to make when an agent reports its port.
    pub fn on_line(&mut self, line: &str) -> Option<AttachRequest> {
        let port = jdwp_listening_port(line)?;
        if self.started {
            return self.attach(port);
        }
        self.waiting.push(port);
        None
    }

    /// The run's session started with this process id: the attaches that waited for it. The pinned
    /// port needs no output line: the debugger retries the attach while the fork starts.
    pub fn session_started(&mut self, process: Option<RunId>) -> Vec<AttachRequest> {
        self.started = true;
        self.process = process;
        let ports: Vec<u32> = self.pinned.into_iter().chain(std::mem::take(&mut self.waiting)).collect();
        ports.into_iter().filter_map(|port| self.attach(port)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use ide_run::BuildPolicy;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn cfg(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(id, id, kind);
        config.command = if kind == "maven" { "./mvnw".to_string() } else { kind.to_string() };
        config.args = strings(args);
        config.detected = true;
        config
    }

    fn launch(config: &RunConfig) -> RunConfig {
        jdwp_launch(config, None).expect("a debuggable launch")
    }

    /// The agent listening on a free port, at the end of `text`.
    fn ends_with_agent(text: &str) -> bool {
        text.ends_with("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0")
    }

    #[test]
    fn adds_jdwp_to_a_script_explicitly_assigned_the_kotlin_debugger() {
        let mut script = cfg("kotlin-script", "shell", &[]);
        script.command = "./run.sh".into();
        script.debug_adapter = Some("kotlin-debug-adapter".into());
        assert!(launch(&script).env["JAVA_TOOL_OPTIONS"].contains("-agentlib:jdwp="));
    }

    #[test]
    fn adds_a_waiting_jdwp_agent_to_spring_boot_run_surefire_and_gradle() {
        let boot = launch(&cfg("b", "maven", &["spring-boot:run"]));
        let jvm_arguments = boot.args[1].strip_prefix("-Dspring-boot.run.jvmArguments=-agentlib:jdwp=");
        let waits = |agent: &str| agent.contains("suspend=y") && agent.ends_with("address=127.0.0.1:0");
        assert!(jvm_arguments.is_some_and(waits));
        let merged = launch(&cfg("b", "maven", &["spring-boot:run", "-Dspring-boot.run.jvmArguments=-Xmx1g"]));
        assert!(merged.args[1].starts_with("-Dspring-boot.run.jvmArguments=-Xmx1g -agentlib:jdwp="));
        let mut test = cfg("t", "maven", &["test", "-Dtest=A#b"]);
        test.is_test = true;
        assert!(launch(&test).args.iter().any(|arg| arg.starts_with("-Dmaven.surefire.debug=-agentlib:jdwp=")));
        // A fork's "Listening for transport" line stays buffered until tests run — which suspend=y
        // prevents — so a Maven test's port is pinned and attached without the line.
        let mut plain = cfg("t2", "maven", &["test"]);
        plain.is_test = true;
        let pinned = jdwp_launch(&plain, Some(50123)).expect("a debuggable launch");
        assert!(pinned.args.iter().any(|arg| arg.contains("address=127.0.0.1:50123")));
        let mut verify = cfg("t3", "maven", &["verify"]);
        verify.is_test = true;
        let failsafe = jdwp_launch(&verify, Some(50124)).expect("a debuggable launch");
        let pinned_failsafe =
            |arg: &String| arg.starts_with("-Dmaven.failsafe.debug=") && arg.contains("address=127.0.0.1:50124");
        assert!(failsafe.args.iter().any(pinned_failsafe));
        assert!(launch(&cfg("g", "gradle", &["bootRun"])).args.contains(&DELEGATED_DEBUG.to_string()));
        let mut gradle_test = cfg("gt", "gradle", &["test"]);
        gradle_test.is_test = true;
        let gradle_test = launch(&gradle_test);
        assert!(gradle_test.args.contains(&DELEGATED_DEBUG.to_string()));
        assert!(!gradle_test.args.contains(&"--debug-jvm".to_string()));
        assert_eq!(jdwp_launch(&cfg("p", "maven", &["package"]), None), None);
        assert_eq!(jdwp_launch(&cfg("n", "npm", &["test"]), None), None);
    }

    #[test]
    fn debugs_mavens_own_jvm_for_the_in_process_goals() {
        let goals = [
            "tomcat7:run",
            "org.apache.tomcat.maven:tomcat7-maven-plugin:2.2:run",
            "jetty:run",
            "jetty:run-war",
            "exec:java",
        ];
        for goal in goals {
            let mut config = cfg("m", "maven", &["clean", goal]);
            config.env.insert("MAVEN_OPTS".into(), "-Xmx1g".into());
            let out = launch(&config);
            assert_eq!(out.args, ["clean", goal]);
            assert!(out.env["MAVEN_OPTS"].starts_with("-Xmx1g -agentlib:jdwp="), "{goal}");
            assert!(ends_with_agent(&out.env["MAVEN_OPTS"]), "{goal}");
        }
    }

    #[test]
    fn honours_rebuild_on_a_maven_debug_command() {
        let mut config = cfg("r", "maven", &["spring-boot:run"]);
        config.extras.build_policy = Some(BuildPolicy::Rebuild);
        let out = launch(&config);
        assert_eq!(out.args[0], "clean");
        assert!(out.args.last().is_some_and(|arg| arg.starts_with("-Dspring-boot.run.jvmArguments=-agentlib:jdwp=")));
    }

    #[test]
    fn keeps_plain_maven_builds_out_of_the_debugger_unless_set_to_the_java_debugger() {
        assert_eq!(jdwp_launch(&cfg("p", "maven", &["exec:exec"]), None), None);
        let mut chosen = cfg("p", "maven", &["package"]);
        chosen.debug_adapter = Some("java-debug".into());
        assert!(ends_with_agent(&launch(&chosen).env["MAVEN_OPTS"]));
    }

    #[test]
    fn starts_a_configuration_set_to_the_java_debugger_with_an_agent_in_java_tool_options() {
        let cases: [(&str, &str, &[&str]); 3] = [
            ("shell", "C:\\apache-tomcat-9.0.98\\bin\\catalina.bat", &["run"]),
            ("shell", "/opt/tomcat9/bin/catalina.sh", &["run"]),
            ("program", "java", &["-jar", "app.war"]),
        ];
        for (kind, command, args) in cases {
            let mut config = cfg("tomcat9", kind, args);
            config.command = command.into();
            config.debug_adapter = Some("java-debug".into());
            let out = launch(&config);
            assert_eq!((out.command.as_str(), &out.args), (command, &strings(args)));
            assert!(ends_with_agent(&out.env["JAVA_TOOL_OPTIONS"]));
            config.env.insert("JAVA_TOOL_OPTIONS".into(), "-Dfile.encoding=UTF-8".into());
            let merged = launch(&config);
            assert!(merged.env["JAVA_TOOL_OPTIONS"].starts_with("-Dfile.encoding=UTF-8 -agentlib:jdwp="));
        }
    }

    #[test]
    fn debugs_a_single_java_file_the_agent_before_the_file_or_for_java_only_on_a_javac_line() {
        let mut file = cfg("f", "java-file", &["Hi.java"]);
        file.command = "/bin/java".into();
        let source = launch(&file);
        assert_eq!(source.args.len(), 2);
        assert!(ends_with_agent(&source.args[0]));
        assert_eq!(source.args[1], "Hi.java");
        let line = "mkdir -p /tmp/c && /bin/javac -g -d /tmp/c Test.java && /bin/java -cp /tmp/c Test";
        let mut compiled = cfg("f", "java-file", &[]);
        compiled.command = line.into();
        compiled.env.insert("JDK_JAVA_OPTIONS".into(), "-Xmx1g".into());
        let compiled = launch(&compiled);
        assert_eq!(compiled.command, line);
        assert!(compiled.args.is_empty());
        assert!(compiled.env["JDK_JAVA_OPTIONS"].starts_with("-Xmx1g -agentlib:jdwp="));
        assert!(!compiled.env.contains_key("JAVA_TOOL_OPTIONS"));
    }

    #[test]
    fn leaves_application_configurations_and_other_debuggers_alone() {
        for kind in ["application", "spring-boot"] {
            let mut config = cfg("a", kind, &[]);
            config.command = "java".into();
            config.debug_adapter = Some("java-debug".into());
            assert_eq!(jdwp_launch(&config, None), None);
        }
        let mut script = cfg("s", "shell", &[]);
        script.command = "./run.sh".into();
        script.debug_adapter = Some("debugpy".into());
        assert_eq!(jdwp_launch(&script, None), None);
        let mut gradle = cfg("g", "gradle", &["build"]);
        gradle.debug_adapter = Some("java-debug".into());
        assert_eq!(jdwp_launch(&gradle, None), None);
    }

    fn detected(debug_args: Option<&str>, debug_adapter: Option<&str>) -> RunConfig {
        let mut config = cfg("plugin:acme.bazel:test", "plugin", &["test", "//a:t"]);
        config.command = "bazelisk".into();
        config.is_test = true;
        config.debug_adapter = debug_adapter.map(str::to_string);
        if let Some(debug_args) = debug_args {
            config.extras.params.insert("debugArgs".into(), debug_args.into());
        }
        config
    }

    #[test]
    fn plugin_detectors_start_the_command_with_the_detectors_debug_arguments_once() {
        let debug = launch(&detected(Some("[\"--java_debug\"]"), Some("java-debug")));
        assert_eq!(debug.args, ["test", "//a:t", "--java_debug"]);
        assert_eq!(launch(&debug).args, ["test", "//a:t", "--java_debug"]);
    }

    #[test]
    fn plugin_detectors_need_both_a_jvm_debugger_and_debug_arguments() {
        assert_eq!(jdwp_launch(&detected(None, Some("java-debug")), None), None);
        assert_eq!(jdwp_launch(&detected(Some("[\"--java_debug\"]"), None), None), None);
        assert_eq!(jdwp_launch(&detected(Some("not json"), Some("java-debug")), None), None);
        assert_eq!(jdwp_launch(&detected(Some("[]"), Some("java-debug")), None), None);
    }

    // Not from a spec: ports reported before the session is known attach once it is, each port once.
    #[test]
    fn attaches_to_every_reported_port_once() {
        let mut test = cfg("t", "maven", &["test"]);
        test.is_test = true;
        test.extras.module = Some("app".into());
        assert!(needs_pinned_port(&test));
        let (mut debug, launch) = AgentDebug::start(&test, Some(50123), Some(false)).expect("a debug run");
        assert!(launch.args.iter().any(|arg| arg.contains("address=127.0.0.1:50123")));
        assert_eq!(debug.on_line("Listening for transport dt_socket at address: 5005"), None);
        let attaches = debug.session_started(Some(RunId(7)));
        assert_eq!(attaches.iter().map(|attach| attach.port).collect::<Vec<_>>(), [50123, 5005]);
        let first = &attaches[0];
        assert_eq!((first.module.as_str(), first.process, first.show), ("app", Some(RunId(7)), Some(false)));
        assert_eq!(debug.on_line("Listening for transport dt_socket at address: 5005"), None);
        let later = debug.on_line("Listening for transport dt_socket at address: 5006");
        assert_eq!(later.map(|attach| attach.port), Some(5006));
        assert!(!needs_pinned_port(&cfg("g", "gradle", &["test"])));
    }
}
