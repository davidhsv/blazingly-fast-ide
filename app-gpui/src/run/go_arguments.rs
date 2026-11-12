//! Go test command lines: flags can occur before or after package targets; binary arguments start at
//! `-args`.

const BOOLEAN_FLAGS: [&str; 20] = [
    "-a",
    "-n",
    "-race",
    "-msan",
    "-asan",
    "-cover",
    "-linkshared",
    "-modcacherw",
    "-trimpath",
    "-v",
    "-work",
    "-x",
    "-json",
    "-c",
    "-i",
    "-short",
    "-failfast",
    "-benchmem",
    "-fullpath",
    "-buildvcs",
];

/// The parts of a `go test` command line (the arguments after `test`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoTestParts {
    /// Flags with their values.
    pub flags: Vec<String>,
    /// Packages or files.
    pub targets: Vec<String>,
    /// `-args` (or `--`) and what follows it, for the test binary.
    pub program: Vec<String>,
}

pub fn go_test_parts(args: &[String]) -> GoTestParts {
    let mut parts = GoTestParts::default();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-args" || arg == "--" {
            parts.program = args[i..].to_vec();
            break;
        }
        if arg.starts_with('-') {
            parts.flags.push(arg.clone());
            let name = arg.strip_prefix("-test.").map_or_else(|| arg.clone(), |flag| format!("-{flag}"));
            let takes_value = !arg.contains('=') && !BOOLEAN_FLAGS.contains(&name.as_str());
            if takes_value && let Some(value) = args.get(i + 1) {
                parts.flags.push(value.clone());
                i += 1;
            }
        } else {
            parts.targets.push(arg.clone());
        }
        i += 1;
    }
    parts
}

/// The name of a flag that selects tests (`run`, `skip`, `bench`, `fuzz`) and whether its value is
/// attached with `=`. After `-args` only the `-test.` spelling reaches the test binary.
fn selector(arg: &str, program: bool) -> Option<bool> {
    let flag = arg.strip_prefix('-')?;
    let flag = match flag.strip_prefix("test.") {
        Some(flag) => flag,
        None if program => return None,
        None => flag,
    };
    let (name, attached) = flag.split_once('=').map_or((flag, false), |(name, _)| (name, true));
    matches!(name, "run" | "skip" | "bench" | "fuzz").then_some(attached)
}

/// `flags` without the ones that select tests.
pub fn without_go_selection(flags: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut program = false;
    let mut i = 0;
    while i < flags.len() {
        let arg = &flags[i];
        if arg == "--" {
            out.extend_from_slice(&flags[i..]);
            break;
        }
        if arg == "-args" {
            program = true;
        }
        match selector(arg, program) {
            Some(attached) => i += usize::from(!attached),
            None => out.push(arg.clone()),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    // Not from a spec: flags keep their values, targets stay apart and `-args` starts the program part.
    #[test]
    fn splits_flags_targets_and_program_arguments() {
        let parts = go_test_parts(&strings(&["test", "-race", "-count", "1", "-run=^A$", "./...", "-args", "-x"]));
        assert_eq!(parts.flags, ["-race", "-count", "1", "-run=^A$"]);
        assert_eq!(parts.targets, ["./..."]);
        assert_eq!(parts.program, ["-args", "-x"]);
    }

    // Not from a spec: test selection is dropped with its value, in both spellings.
    #[test]
    fn drops_test_selection_flags() {
        let flags = strings(&["-race", "-run", "^A$", "-test.bench=.", "-v", "-args", "-run", "x", "-test.skip", "y"]);
        assert_eq!(without_go_selection(&flags), ["-race", "-v", "-args", "-run", "x"]);
        assert_eq!(without_go_selection(&strings(&["-fuzz", "F", "--", "-run", "x"])), ["--", "-run", "x"]);
    }
}
