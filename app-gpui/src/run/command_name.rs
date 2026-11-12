//! Executable identity for form matching; the original command is kept for launching.

/// The name a command is recognised by: its file name, and on Windows without its extension and case.
pub fn command_name(command: &str) -> String {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let lower = name.to_ascii_lowercase();
    match [".exe", ".cmd", ".bat", ".com"].iter().find(|extension| lower.ends_with(**extension)) {
        Some(extension) => lower[..lower.len() - extension.len()].to_string(),
        None => name.to_string(),
    }
}

/// `args` with `extra` forwarded to a package script. pnpm forwards `--` to the script, where it ends
/// the test runner's option parsing; the other package managers need it to pass arguments on.
pub fn append_script_args(command: &str, args: &[String], extra: &[String]) -> Vec<String> {
    let separated = args.iter().any(|arg| arg == "--");
    let needs_separator = !separated && matches!(command_name(command).as_str(), "npm" | "yarn" | "bun");
    let separator = needs_separator.then(|| "--".to_string());
    args.iter().cloned().chain(separator).chain(extra.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a Windows launcher is recognised by its base name, other commands as written.
    #[test]
    fn names_ignore_folders_and_windows_extensions() {
        assert_eq!(command_name("C:\\tools\\pnpm.CMD"), "pnpm");
        assert_eq!(command_name("/opt/bin/Gradle"), "Gradle");
        assert_eq!(command_name("java"), "java");
        let args = ["run".to_string(), "test".to_string()];
        let file = ["a.test.ts".to_string()];
        assert_eq!(append_script_args("npm", &args, &file), ["run", "test", "--", "a.test.ts"]);
        assert_eq!(append_script_args("/opt/bin/pnpm", &args, &file), ["run", "test", "a.test.ts"]);
    }
}
