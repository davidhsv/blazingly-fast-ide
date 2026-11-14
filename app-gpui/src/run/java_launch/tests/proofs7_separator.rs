use super::*;

/// The hosts of the spec: the separator the backend reports for the host's paths, and those paths.
const HOSTS: [(&str, &str, [&str; 2]); 5] = [
    ("Linux", ":", ["/p/classes", "/lib/a;b.jar"]),
    ("Windows", ";", [r"C:\p\classes", r"D:\lib\a.jar"]),
    ("Mac", ":", ["/p/classes", "/lib/a.jar"]),
    ("Windows", ";", [r"C:\p\classes", r"D:\lib\a.jar"]),
    ("Linux", ":", ["/p/classes", "/lib/a.jar"]),
];

// src/features/debugger/__tests__/javaSeparator.test.ts › Java launch on $host from $client › joins %s and module
// paths using the host separator
/// The class path that replaces `${classpath}` or `${testClasspath}`, and the module path put before the arguments,
/// are the host's paths joined with the separator the host reports, whatever the separator of the machine the IDE
/// runs on; a `;` inside a path of a `:` host stays part of the path.
#[test]
fn joins_the_classpath_and_module_paths_using_the_host_separator() {
    for (host, separator, paths) in HOSTS {
        let info = JavaLaunchInfo {
            class_paths: strings(&paths),
            module_paths: strings(&paths),
            path_list_separator: separator.into(),
            ..JavaLaunchInfo::default()
        };
        let joined = paths.join(separator);
        for placeholder in [CLASSPATH, TEST_CLASSPATH] {
            let launch = RunConfig { args: strings(&["-cp", placeholder, "Main"]), ..config() };
            let resolved = resolve_java_command(&launch, &info);
            assert_eq!(resolved.args, ["--module-path", joined.as_str(), "-cp", joined.as_str(), "Main"], "{host}");
        }
    }
}
