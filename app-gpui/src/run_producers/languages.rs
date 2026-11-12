//! Single-file script languages: which language a file is, the tools that can run it, what to install
//! when none is found, and how a compile-and-run line is written for the platform shell.

use crate::platform::command_line::format_args;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptLanguage {
    Js,
    Ts,
    Python,
    Shell,
    Bat,
    Ruby,
    Php,
    Perl,
    Lua,
    R,
    Go,
    Rust,
    Java,
    Kotlin,
    KotlinScript,
    Swift,
    C,
    Cpp,
    Dart,
    Elixir,
    Scala,
    Shebang,
}

impl ScriptLanguage {
    /// The language's id; an interpreted language's configurations are of the kind with that id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Js => "js",
            Self::Ts => "ts",
            Self::Python => "python",
            Self::Shell => "shell",
            Self::Bat => "bat",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::Perl => "perl",
            Self::Lua => "lua",
            Self::R => "r",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::KotlinScript => "kotlin-script",
            Self::Swift => "swift",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Dart => "dart",
            Self::Elixir => "elixir",
            Self::Scala => "scala",
            Self::Shebang => "shebang",
        }
    }
}

/// The file name of a `/`-separated path.
pub(crate) fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// The command of a script's `#!` line: `#!/usr/bin/env -S deno run` is `["deno", "run"]`,
/// `#!/bin/zsh -f` is `["/bin/zsh", "-f"]`. `None` without one, or when `env` names no program.
pub fn shebang(content: Option<&str>) -> Option<Vec<String>> {
    let first = content?.split('\n').next()?;
    let line = first.split(['\r', '\u{2028}', '\u{2029}']).next()?;
    let mut words = line.strip_prefix("#!")?.split_whitespace();
    let command = words.next()?;
    let rest: Vec<&str> = words.collect();
    if command == "env" || command.ends_with("/env") {
        let args = match rest.split_first() {
            Some((first, split)) if *first == "-S" => strings(split),
            _ => rest.iter().filter(|arg| !arg.starts_with('-')).map(|arg| arg.to_string()).collect(),
        };
        return (!args.is_empty()).then_some(args);
    }
    Some(std::iter::once(command).chain(rest).map(str::to_string).collect())
}

/// The name an interpreter is known by: its file name, without `.exe`.
pub fn interpreter_name(command: &str) -> &str {
    let name = base_name(command);
    name.strip_suffix(".exe").unwrap_or(name)
}

/// The language a file runs as: by its extension, or by its `#!` line when it has none.
pub fn script_language(path: &str, content: Option<&str>) -> Option<ScriptLanguage> {
    let name = base_name(path);
    let ends = |extensions: &[&str]| extensions.iter().any(|extension| name.ends_with(extension));
    let lower = name.to_ascii_lowercase();
    let language = if ends(&[".js", ".cjs", ".mjs"]) {
        ScriptLanguage::Js
    } else if ends(&[".ts", ".cts", ".mts"]) && !ends(&[".d.ts", ".d.cts", ".d.mts"]) {
        ScriptLanguage::Ts
    } else if ends(&[".py", ".pyw"]) {
        ScriptLanguage::Python
    } else if ends(&[".sh", ".bash", ".zsh"]) {
        ScriptLanguage::Shell
    } else if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        ScriptLanguage::Bat
    } else if ends(&[".rb"]) {
        ScriptLanguage::Ruby
    } else if ends(&[".php"]) {
        ScriptLanguage::Php
    } else if ends(&[".pl"]) {
        ScriptLanguage::Perl
    } else if ends(&[".lua"]) {
        ScriptLanguage::Lua
    } else if ends(&[".r", ".R"]) {
        ScriptLanguage::R
    } else if ends(&[".go"]) {
        ScriptLanguage::Go
    } else if ends(&[".rs"]) {
        ScriptLanguage::Rust
    } else if ends(&[".java"]) {
        ScriptLanguage::Java
    } else if ends(&[".kt"]) {
        ScriptLanguage::Kotlin
    } else if ends(&[".kts"]) && !ends(&[".gradle.kts"]) {
        ScriptLanguage::KotlinScript
    } else if ends(&[".swift"]) {
        ScriptLanguage::Swift
    } else if ends(&[".c"]) {
        ScriptLanguage::C
    } else if ends(&[".cpp", ".cc", ".cxx", ".c++"]) {
        ScriptLanguage::Cpp
    } else if ends(&[".dart"]) {
        ScriptLanguage::Dart
    } else if ends(&[".exs"]) {
        ScriptLanguage::Elixir
    } else if ends(&[".scala", ".sc"]) {
        ScriptLanguage::Scala
    } else if !name.contains('.') && shebang(content).is_some() {
        ScriptLanguage::Shebang
    } else {
        return None;
    };
    Some(language)
}

/// Tools a file of `language` may run with, in preference order.
pub fn tools_for(language: ScriptLanguage, content: Option<&str>) -> Vec<String> {
    let tools: &[&str] = match language {
        ScriptLanguage::Js => &["node", "deno", "bun"],
        ScriptLanguage::Ts => &["node", "tsx", "ts-node", "deno", "bun"],
        ScriptLanguage::Python => &["python3", "python"],
        ScriptLanguage::Shell | ScriptLanguage::Shebang => {
            let command = shebang(content).unwrap_or_default();
            let named = command.first().map(|first| interpreter_name(first).to_string());
            let mut tools: Vec<String> = named.into_iter().collect();
            if language == ScriptLanguage::Shell {
                for shell in ["bash", "zsh", "sh"] {
                    if !tools.iter().any(|tool| tool == shell) {
                        tools.push(shell.to_string());
                    }
                }
            }
            return tools;
        }
        ScriptLanguage::Bat => &[],
        ScriptLanguage::Ruby => &["ruby"],
        ScriptLanguage::Php => &["php"],
        ScriptLanguage::Perl => &["perl"],
        ScriptLanguage::Lua => &["lua"],
        ScriptLanguage::R => &["Rscript"],
        ScriptLanguage::Go => &["go"],
        ScriptLanguage::Rust => &["rustc"],
        ScriptLanguage::Java => &["java"],
        ScriptLanguage::Kotlin => &["kotlinc"],
        ScriptLanguage::KotlinScript => &["kotlin", "kotlinc"],
        ScriptLanguage::Swift => &["swift"],
        ScriptLanguage::C => &["clang", "gcc", "cc"],
        ScriptLanguage::Cpp => &["clang++", "g++", "c++"],
        ScriptLanguage::Dart => &["dart"],
        ScriptLanguage::Elixir => &["elixir"],
        ScriptLanguage::Scala => &["scala-cli"],
    };
    strings(tools)
}

/// What to install when the runtime of `language` is missing.
fn install_hint(language: ScriptLanguage) -> &'static str {
    match language {
        ScriptLanguage::Js => "Install Node.js from https://nodejs.org (or `brew install node`).",
        ScriptLanguage::Python => {
            "Install Python (`brew install python`) or choose an interpreter in Settings › Python Interpreter."
        }
        ScriptLanguage::Ruby => "Install Ruby (`brew install ruby`).",
        ScriptLanguage::Php => {
            "Install PHP (`brew install php`) or choose an interpreter in Settings › Languages & Frameworks › PHP."
        }
        ScriptLanguage::Perl => "Install Perl (`brew install perl`).",
        ScriptLanguage::Lua => "Install Lua (`brew install lua`).",
        ScriptLanguage::R => "Install R (`brew install r`) so that `Rscript` is on the PATH.",
        ScriptLanguage::Go => "Install Go from https://go.dev/dl (or `brew install go`).",
        ScriptLanguage::Rust => "Install Rust with rustup: https://rustup.rs",
        ScriptLanguage::Java => concat!(
            "Install a JDK 11+ (`brew install openjdk`), or download one in ",
            "Settings › Languages & Frameworks › Java (Project JDK › Download JDK…).",
        ),
        ScriptLanguage::Kotlin => concat!(
            "Install the Kotlin compiler (`brew install kotlin`, or ",
            "https://kotlinlang.org/docs/command-line.html) so that `kotlinc` is on the PATH.",
        ),
        ScriptLanguage::KotlinScript => "Install Kotlin (`brew install kotlin`).",
        ScriptLanguage::Swift => "Install the Xcode command line tools (`xcode-select --install`).",
        ScriptLanguage::C => "Install a C compiler: `xcode-select --install` (clang) or `brew install gcc`.",
        ScriptLanguage::Cpp => "Install a C++ compiler: `xcode-select --install` (clang++) or `brew install gcc`.",
        ScriptLanguage::Dart => "Install the Dart SDK (`brew install dart-sdk`).",
        ScriptLanguage::Elixir => "Install Elixir (`brew install elixir`).",
        ScriptLanguage::Scala => "Install Scala CLI (`brew install Virtuslab/scala-cli/scala-cli`).",
        ScriptLanguage::Shell => "Install the shell named in the script's #! line.",
        ScriptLanguage::Shebang => "Install the interpreter named in the script's #! line.",
        ScriptLanguage::Ts | ScriptLanguage::Bat => "",
    }
}

/// How the missing runtime of `language` is called in a message, when not by its first tool.
fn runtime_name(language: ScriptLanguage) -> Option<&'static str> {
    Some(match language {
        ScriptLanguage::Js => "Node.js",
        ScriptLanguage::Ts => "TypeScript",
        ScriptLanguage::Python => "Python",
        ScriptLanguage::R => "R",
        ScriptLanguage::Go => "Go",
        ScriptLanguage::Rust => "rustc",
        ScriptLanguage::Java => "Java",
        ScriptLanguage::Kotlin => "The Kotlin compiler",
        ScriptLanguage::KotlinScript => "Kotlin",
        ScriptLanguage::C => "A C compiler",
        ScriptLanguage::Cpp => "A C++ compiler",
        ScriptLanguage::Scala => "Scala CLI",
        _ => return None,
    })
}

/// Why a file of `language` cannot run: which runtime was not found, where it was looked for and how
/// to install it.
pub fn not_found_text(language: ScriptLanguage, tools: &[String]) -> String {
    let what = runtime_name(language).or(tools.first().map(String::as_str)).unwrap_or(language.id());
    let looked: Vec<String> = tools.iter().map(|tool| format!("`{tool}`")).collect();
    format!("{what} was not found (looked for {}). {}", looked.join(", "), install_hint(language)).trim().to_string()
}

/// How Node.js runs TypeScript itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeStripping {
    /// Without a flag.
    Native,
    /// Behind `--experimental-strip-types`.
    Flag,
}

/// The leading `major.minor` of a version string.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let digits = |text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let major = digits(version);
    let rest = version[major..].strip_prefix('.')?;
    let minor = digits(rest);
    Some((version[..major].parse().ok()?, rest[..minor].parse().ok()?))
}

/// Node's built-in TypeScript support: unflagged on 23.6+ and 22.18+, behind
/// `--experimental-strip-types` on 22.6–22.17 and 23.0–23.5, none before.
pub fn node_type_stripping(version: Option<&str>) -> Option<TypeStripping> {
    let (major, minor) = major_minor(version?)?;
    if major > 23 || (major == 23 && minor >= 6) || (major == 22 && minor >= 18) {
        Some(TypeStripping::Native)
    } else if (major == 22 && minor >= 6) || major == 23 {
        Some(TypeStripping::Flag)
    } else {
        None
    }
}

/// One word for the platform shell: POSIX `sh` quoting, or `cmd`'s double quotes.
pub fn shell_word(word: &str, windows: bool) -> String {
    if !windows {
        return format_args(&[word]);
    }
    if word.is_empty() {
        return "\"\"".to_string();
    }
    let special = |c: char| c.is_whitespace() || "\"&|<>^(),;=%!'".contains(c);
    if word.contains(special) { format!("\"{}\"", word.replace('"', "\"\"")) } else { word.to_string() }
}

/// A command for the platform shell; `cmd` runs a batch file (`kotlinc.bat`) with `call`, so that
/// `&&` sees its exit code.
pub fn shell_line(words: &[String], windows: bool) -> String {
    let line = words.iter().map(|word| shell_word(word, windows)).collect::<Vec<_>>().join(" ");
    let first = words.first().map(|word| word.to_ascii_lowercase()).unwrap_or_default();
    if windows && (first.ends_with(".bat") || first.ends_with(".cmd")) { format!("call {line}") } else { line }
}

/// A path for the platform shell (backslashes for `cmd`).
pub fn native_path(path: &str, windows: bool) -> String {
    if windows { path.replace('/', "\\") } else { path.to_string() }
}

/// Creates `dir` (with parents) if missing, as a shell command.
pub fn mkdir_line(dir: &str, windows: bool) -> String {
    if !windows {
        return format!("mkdir -p {}", shell_word(dir, false));
    }
    let dir = shell_word(&native_path(dir, true), true);
    format!("(if not exist {dir} mkdir {dir})")
}

/// A short stable hash of a path, so that the compiled binaries of same-named files don't collide.
pub fn path_hash(path: &str) -> String {
    let mut hash = path.encode_utf16().fold(5381u32, |hash, unit| hash.wrapping_mul(33) ^ u32::from(unit));
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit(hash % 36, 36).unwrap_or('0'));
        hash /= 36;
        if hash == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_stripping_versions() {
        assert_eq!(node_type_stripping(Some("22.5.1")), None);
        assert_eq!(node_type_stripping(Some("22.6.0")), Some(TypeStripping::Flag));
        assert_eq!(node_type_stripping(Some("22.17.9")), Some(TypeStripping::Flag));
        assert_eq!(node_type_stripping(Some("22.18.0")), Some(TypeStripping::Native));
        assert_eq!(node_type_stripping(Some("23.5.0")), Some(TypeStripping::Flag));
        assert_eq!(node_type_stripping(Some("23.6.0")), Some(TypeStripping::Native));
        assert_eq!(node_type_stripping(Some("v?")), None);
    }

    #[test]
    fn cmd_quoting_double_quotes_only_where_needed_never_posix_single_quotes() {
        assert_eq!(shell_word(r"C:\Program Files\x.exe", true), r#""C:\Program Files\x.exe""#);
        assert_eq!(shell_word(r"C:\tools\x.exe", true), r"C:\tools\x.exe");
        assert_eq!(shell_word("a&b", true), "\"a&b\"");
        assert_eq!(shell_word("it's", true), "\"it's\"");
        assert_eq!(shell_word("", true), "\"\"");
        assert_eq!(shell_word("it's", false), r"'it'\''s'");
    }

    // Not from a spec: a `#!` line names its program directly or through `env`, and a file is told by
    // its extension before its `#!` line.
    #[test]
    fn reads_shebang_lines_and_tells_languages_by_extension() {
        assert_eq!(shebang(Some("#!/usr/bin/env node")), Some(strings(&["node"])));
        assert_eq!(shebang(Some("#!/usr/bin/env -S deno run -A\n")), Some(strings(&["deno", "run", "-A"])));
        assert_eq!(shebang(Some("#! /bin/zsh -f\r\necho")), Some(strings(&["/bin/zsh", "-f"])));
        assert_eq!(shebang(Some("#!/usr/bin/env -i")), None);
        assert_eq!(shebang(Some("echo\n#!/bin/sh")), None);
        assert_eq!(script_language("/a/types.d.ts", None), None);
        assert_eq!(script_language("/a/build.gradle.kts", None), None);
        assert_eq!(script_language("/a/RUN.CMD", None), Some(ScriptLanguage::Bat));
        assert_eq!(script_language("/a/tool", Some("#!/bin/sh")), Some(ScriptLanguage::Shebang));
        assert_eq!(script_language("/a/notes", Some("just text")), None);
        assert_eq!(tools_for(ScriptLanguage::Shell, Some("#!/bin/zsh")), ["zsh", "bash", "sh"]);
        let lua = "lua was not found (looked for `lua`). Install Lua (`brew install lua`).";
        assert_eq!(not_found_text(ScriptLanguage::Lua, &strings(&["lua"])), lua);
    }

    // Not from a spec: the hash is the same number the stored binaries of earlier runs are named by.
    #[test]
    fn hashes_paths_to_base_36() {
        assert_eq!(path_hash(""), "45h");
        assert_ne!(path_hash("/a/x.rs"), path_hash("/b/x.rs"));
        assert!(path_hash("/a/x.rs").chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
    }
}
