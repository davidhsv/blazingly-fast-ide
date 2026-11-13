//! Build, Execution, Deployment: CMake, Gradle, Maven, Console, Debugger (with Data Views and
//! Stepping), Python Debugger, Run/Debug, and Deployment with its Options.

use super::{custom, hint, number, select, select_owned, toggle};
use crate::settings_model::page_spec::{OptionDetail, PageSpec};
use crate::settings_model::run_targets::BUILD_GROUP;

/// The encodings of process output.
const CONSOLE_ENCODINGS: [&str; 14] = [
    "UTF-8",
    "UTF-16LE",
    "UTF-16BE",
    "ISO-8859-1",
    "windows-1250",
    "windows-1251",
    "windows-1252",
    "GBK",
    "GB18030",
    "Big5",
    "Shift_JIS",
    "EUC-JP",
    "EUC-KR",
    "KOI8-R",
];

/// A build tool page: its settings belong to the project, whose values show first. The page is a
/// view of its own.
fn build_tool(id: &'static str, title: &'static str, order: f64, keywords: &[&'static str]) -> PageSpec {
    PageSpec::new(id, title, order).group(BUILD_GROUP).project_scope().project_first().keywords(keywords)
}

fn console() -> PageSpec {
    let encodings = CONSOLE_ENCODINGS.iter().map(|&encoding| (encoding, encoding)).collect();
    PageSpec::new("build.console", "Console", 360)
        .group(BUILD_GROUP)
        .project_scope()
        .keywords(&["run output", "encoding", "fold", "buffer"])
        .section(
            None,
            vec![
                custom("Override console cycle buffer size", &["KB"]),
                toggle("console.softWrap", "Use soft wraps in console"),
                select_owned("console.encoding", "Default encoding", encodings),
            ],
        )
        .section(None, vec![custom("Fold console lines that contain", &[])])
        .detail("console.encoding", hint("Encoding of the output of processes started from Run configurations."))
}

fn debugger() -> PageSpec {
    PageSpec::new("build.debugger", "Debugger", 361)
        .group(BUILD_GROUP)
        .project_scope()
        .keywords(&["debug", "breakpoint"])
        .section(
            None,
            vec![
                toggle("debugger.showOnBreakpoint", "Show debug window on breakpoint hit"),
                toggle("debugger.focusAppOnBreakpoint", "Focus application on breakpoint"),
                toggle("debugger.hideOnTermination", "Hide debug window on process termination"),
            ],
        )
        .section(
            Some("Breakpoints"),
            vec![
                toggle("debugger.breakpointsAnyFile", "Allow placing breakpoints in any file"),
                toggle("debugger.runToCursorInlay", "Show inlay Run to Cursor popup"),
            ],
        )
        .detail(
            "debugger.breakpointsAnyFile",
            hint("Otherwise line breakpoints can only be set in files of languages that have a debugger."),
        )
}

fn data_views() -> PageSpec {
    PageSpec::new("build.debugger.dataViews", "Data Views", 0)
        .project_scope()
        .keywords(&["variables", "inline", "tooltip"])
        .section(
            None,
            vec![
                toggle("debugger.inlineValues", "Show values inline"),
                toggle("debugger.sortValues", "Sort values alphabetically"),
                toggle("debugger.valueTooltip", "Show value tooltip on mouse hover"),
                toggle("debugger.valueTooltipOnSelection", "Show value tooltip on code selection"),
                custom("Value tooltip delay (ms)", &[]),
            ],
        )
        .detail(
            "debugger.inlineValues",
            hint("Values of local variables are shown after the lines that use them while suspended."),
        )
        .detail(
            "debugger.valueTooltip",
            hint("Hovering a variable while suspended evaluates it in the selected frame."),
        )
}

fn stepping() -> PageSpec {
    PageSpec::new("build.debugger.stepping", "Stepping", 1)
        .project_scope()
        .keywords(&["step into", "library", "just my code"])
        .section(None, vec![toggle("debugger.skipLibraryCode", "Do not step into library scripts")])
        .detail(
            "debugger.skipLibraryCode",
            hint("Python: only your code (justMyCode). JavaScript: skips node_modules and Node.js internals."),
        )
}

fn python_debugger() -> PageSpec {
    PageSpec::new("build.pythonDebugger", "Python Debugger", 362)
        .group(BUILD_GROUP)
        .project_scope()
        .keywords(&["debugpy", "python", "gevent", "subprocess"])
        .section(
            None,
            vec![
                toggle("debugger.python.attachSubprocess", "Attach to subprocess automatically while debugging"),
                toggle("debugger.python.gevent", "Gevent compatible"),
            ],
        )
        .detail("debugger.python.gevent", hint("Enable when debugging code that uses gevent monkey-patching."))
}

fn run_debug() -> PageSpec {
    PageSpec::new("build.run", "Run/Debug", 363)
        .group(BUILD_GROUP)
        .project_scope()
        .keywords(&["run configuration", "rerun", "stop", "terminate"])
        .section(
            None,
            vec![
                toggle("run.confirmRerun", "Confirm rerun with process termination"),
                toggle("run.confirmTerminate", "Confirm closing a tab with a running process"),
                toggle("run.showOnStart", "Show Run/Debug window when a process is started"),
                number(
                    "run.terminationTimeoutMs",
                    "Time limit for Run/Debug process termination",
                    0.0,
                    600_000.0,
                    Some("ms"),
                ),
            ],
        )
        .detail(
            "run.terminationTimeoutMs",
            OptionDetail {
                step: Some(500.0),
                hint: Some(
                    "After a configuration's stop command, the process gets this long to exit before it is \
                     interrupted. Press Stop again to kill a process that doesn't exit.",
                ),
                ..OptionDetail::default()
            },
        )
}

fn deployment() -> PageSpec {
    PageSpec::new("build.deployment", "Deployment", 365)
        .group(BUILD_GROUP)
        .project_scope()
        .project_first()
        .keywords(&[
            "local",
            "mounted",
            "sftp",
            "ssh",
            "server",
            "upload",
            "remote host",
            "mappings",
            "web server",
            "deploy",
        ])
        .section(
            None,
            vec![custom(
                "Deployment servers",
                &[
                    "Connection",
                    "Mappings",
                    "Excluded Paths",
                    "Root path",
                    "Web server URL",
                    "SSH configuration",
                    "Test Connection",
                    "Use as Default",
                    "Local path",
                    "Deployment path",
                    "Web path",
                ],
            )],
        )
}

fn deployment_options() -> PageSpec {
    PageSpec::new("build.deployment.options", "Options", 0)
        .project_scope()
        .project_first()
        .keywords(&["deployment", "upload", "exclude", "timestamps", "automatic upload", "sftp"])
        .section(
            None,
            vec![
                custom("Exclude items by name", &["exclude"]),
                select(
                    "deployment.autoUpload",
                    "Upload changed files automatically to the default server",
                    &[("never", "Never"), ("onExplicitSave", "On explicit save action"), ("always", "Always")],
                ),
                toggle("deployment.deleteRemote", "Delete remote files when local are deleted"),
                toggle("deployment.preserveTimestamps", "Preserve files timestamps"),
            ],
        )
}

/// The pages in the order they are registered; Run Targets comes from its own module.
pub(super) fn pages() -> Vec<PageSpec> {
    vec![
        build_tool(
            "build.cmake",
            "CMake",
            357.0,
            &[
                "profile",
                "generator",
                "Ninja",
                "Build type",
                "CMake arguments",
                "Build directory",
                "Build arguments",
                "Environment variables",
            ],
        ),
        build_tool(
            "build.gradle",
            "Gradle",
            358.0,
            &[
                "Build and run using",
                "delegate",
                "JavaExec",
                "Gradle JVM",
                "JDK",
                "JAVA_HOME",
                "distribution",
                "wrapper",
                "gradlew",
                "local installation",
                "Gradle home",
                "Gradle user home",
                "GRADLE_USER_HOME",
                "mise",
                "asdf",
            ],
        ),
        build_tool(
            "build.maven",
            "Maven",
            359.0,
            &[
                "JRE",
                "runner",
                "JDK",
                "JAVA_HOME",
                "maven home",
                "mvn",
                "mvnw",
                "wrapper",
                "settings.xml",
                "user settings",
                "local repository",
                "m2",
                "thread count",
                "threads",
                "-T",
                "parallel",
            ],
        ),
        console(),
        debugger(),
        data_views(),
        stepping(),
        python_debugger(),
        run_debug(),
        deployment(),
        deployment_options(),
    ]
}
