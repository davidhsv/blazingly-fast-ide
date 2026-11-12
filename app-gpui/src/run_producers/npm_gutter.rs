//! Gutter run icons on the `scripts` entries of a `package.json` (like IDEA's package.json gutter):
//! Run / Debug launch the npm run configuration the project detected for the script, or a temporary
//! one when it has none.
//!
//! [`ScriptMenuFlow`] does no I/O: it hands out the next [`ScriptMenuStep`] and continues with its
//! answer.

use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use crate::run::configs::{blank_config, cwd_of, to_cwd};
use crate::run::targets::MAX_SCAN;
use crate::run_producers::context_run::ContextShortcuts;
use ide_run::RunConfig;
use serde_json::Value;

/// One `scripts` entry: the script name and its 0-based line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptEntry {
    pub name: String,
    pub line: u32,
}

/// The text stopped being JSON.
struct Broken;

/// A small JSON scanner that tracks lines: script values may contain braces, colons and escapes.
struct Scanner {
    text: Vec<char>,
    at: usize,
    line: u32,
}

impl Scanner {
    fn peek(&self) -> Option<char> {
        self.text.get(self.at).copied()
    }

    /// Whether the next character is `expected`.
    fn is_at(&self, expected: char) -> bool {
        self.peek() == Some(expected)
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                '\n' => self.line += 1,
                ' ' | '\t' | '\r' | '\u{feff}' => {}
                _ => break,
            }
            self.at += 1;
        }
    }

    /// The JSON string at the cursor, with its escapes decoded; moves past it.
    fn string(&mut self) -> Result<String, Broken> {
        if !self.is_at('"') {
            return Err(Broken);
        }
        self.at += 1;
        fn push(units: &mut Vec<u16>, c: char) {
            units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
        }
        let mut units: Vec<u16> = Vec::new();
        loop {
            let c = self.peek().ok_or(Broken)?;
            self.at += 1;
            match c {
                '"' => return Ok(String::from_utf16_lossy(&units)),
                '\\' => {
                    let escaped = self.peek().ok_or(Broken)?;
                    self.at += 1;
                    match escaped {
                        'u' => {
                            let hex: String = self.text.get(self.at..self.at + 4).ok_or(Broken)?.iter().collect();
                            if !hex.chars().all(|digit| digit.is_ascii_hexdigit()) {
                                return Err(Broken);
                            }
                            units.push(u16::from_str_radix(&hex, 16).map_err(|_| Broken)?);
                            self.at += 4;
                        }
                        'n' => push(&mut units, '\n'),
                        't' => push(&mut units, '\t'),
                        'r' => push(&mut units, '\r'),
                        'b' => push(&mut units, '\u{8}'),
                        'f' => push(&mut units, '\u{c}'),
                        '\n' => {
                            self.line += 1;
                            push(&mut units, '\n');
                        }
                        other => push(&mut units, other),
                    }
                }
                '\n' => {
                    self.line += 1;
                    push(&mut units, '\n');
                }
                other => push(&mut units, other),
            }
        }
    }

    /// The length of the number or literal at the cursor.
    fn scalar_len(&self) -> Option<usize> {
        let rest = &self.text[self.at.min(self.text.len())..];
        let digits = |from: usize| rest[from..].iter().take_while(|c| c.is_ascii_digit()).count();
        let sign = usize::from(rest.first() == Some(&'-'));
        let whole = digits(sign);
        if whole > 0 {
            let mut len = sign + whole;
            if rest.get(len) == Some(&'.') && digits(len + 1) > 0 {
                len += 1 + digits(len + 1);
            }
            if matches!(rest.get(len), Some('e' | 'E')) {
                let signed = usize::from(matches!(rest.get(len + 1), Some('-' | '+')));
                let exponent = digits((len + 1 + signed).min(rest.len()));
                if exponent > 0 {
                    len += 1 + signed + exponent;
                }
            }
            return Some(len);
        }
        let starts_with = |word: &str| rest.iter().copied().take(word.len()).eq(word.chars());
        ["true", "false", "null"].into_iter().find(|word| starts_with(word)).map(str::len)
    }

    /// Skips one JSON value (objects and arrays recursively).
    fn value(&mut self) -> Result<(), Broken> {
        self.skip_space();
        let (object, close) = match self.peek() {
            Some('"') => return self.string().map(|_| ()),
            Some('{') => (true, '}'),
            Some('[') => (false, ']'),
            _ => {
                self.at += self.scalar_len().ok_or(Broken)?;
                return Ok(());
            }
        };
        self.at += 1;
        self.skip_space();
        if self.is_at(close) {
            self.at += 1;
            return Ok(());
        }
        loop {
            if object {
                self.skip_space();
                self.string()?;
                self.skip_space();
                if !self.is_at(':') {
                    return Err(Broken);
                }
                self.at += 1;
            }
            self.value()?;
            self.skip_space();
            if self.is_at(',') {
                self.at += 1;
            } else if self.is_at(close) {
                self.at += 1;
                return Ok(());
            } else {
                return Err(Broken);
            }
        }
    }

    /// The entries of the `scripts` object the cursor is in.
    fn scripts(&mut self, entries: &mut Vec<ScriptEntry>) -> Result<(), Broken> {
        loop {
            self.skip_space();
            let line = self.line;
            let name = self.string()?;
            self.skip_space();
            if !self.is_at(':') {
                return Err(Broken);
            }
            self.at += 1;
            self.value()?;
            entries.push(ScriptEntry { name, line });
            self.skip_space();
            if !self.is_at(',') {
                return Ok(());
            }
            self.at += 1;
        }
    }

    /// The members of the top-level object, collecting the entries of its `scripts`.
    fn document(&mut self, entries: &mut Vec<ScriptEntry>) -> Result<(), Broken> {
        self.skip_space();
        if !self.is_at('{') {
            return Ok(());
        }
        self.at += 1;
        self.skip_space();
        if self.is_at('}') {
            return Ok(());
        }
        loop {
            self.skip_space();
            let key = self.string()?;
            self.skip_space();
            if !self.is_at(':') {
                return Ok(());
            }
            self.at += 1;
            self.skip_space();
            if key == "scripts" && self.is_at('{') {
                self.at += 1;
                self.skip_space();
                if !self.is_at('}') {
                    self.scripts(entries)?;
                }
                // The closing `}` of `scripts`.
                self.at += 1;
            } else {
                self.value()?;
            }
            self.skip_space();
            if !self.is_at(',') {
                return Ok(());
            }
            self.at += 1;
        }
    }
}

/// The top-level `scripts` entries of `package.json` text, in document order. Entries read so far
/// survive malformed text, so a trailing comma keeps the earlier scripts visible.
pub fn script_entries(text: &str) -> Vec<ScriptEntry> {
    let mut entries = Vec::new();
    let mut scanner = Scanner { text: text.chars().collect(), at: 0, line: 0 };
    // Whatever was read before the text broke stays.
    let _ = scanner.document(&mut entries);
    entries
}

/// What the script gutter reads of a `package.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageJson {
    pub name: Option<String>,
    /// The `packageManager` declaration (`pnpm@9.1.0`).
    pub package_manager: Option<String>,
}

/// The manifest in `text`; `None` when it is not JSON, or is an empty value.
pub fn parse_package_json(text: &str) -> Option<PackageJson> {
    let value: Value = serde_json::from_str(text).ok()?;
    let empty = match &value {
        Value::Null | Value::Bool(false) => true,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        _ => false,
    };
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    (!empty).then(|| PackageJson { name: field("name"), package_manager: field("packageManager") })
}

const PACKAGE_MANAGERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];

/// The package manager of a package: the one it declares, then by lockfile, in the order backend
/// detection uses.
pub fn script_package_manager<S: AsRef<str>>(package: Option<&PackageJson>, files: &[S]) -> &'static str {
    let declared = package.and_then(|package| package.package_manager.as_deref()).unwrap_or_default();
    let declared = declared.split('@').next().unwrap_or_default();
    if let Some(manager) = PACKAGE_MANAGERS.into_iter().find(|manager| *manager == declared) {
        return manager;
    }
    let has = |name: &str| files.iter().any(|file| file.as_ref() == name);
    if has("pnpm-lock.yaml") {
        "pnpm"
    } else if has("yarn.lock") {
        "yarn"
    } else if has("bun.lock") || has("bun.lockb") {
        "bun"
    } else {
        "npm"
    }
}

/// A working directory as detection writes it: without a leading `./` or trailing slashes.
fn normal_cwd(config: &RunConfig) -> &str {
    let cwd = cwd_of(config).unwrap_or_default();
    let cwd = cwd.strip_prefix('.').map_or(cwd, |rest| rest.strip_prefix('/').unwrap_or(rest));
    cwd.trim_end_matches('/')
}

/// Backend ids for a script: the root package's, the member's by its `name` or its directory.
pub fn script_config_ids(name: &str, package_name: Option<&str>, dir: &str) -> Vec<String> {
    let mut ids = vec![format!("npm:{name}")];
    for owner in [package_name.unwrap_or_default(), dir] {
        let id = format!("npm:{owner}:{name}");
        if !owner.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The project's configuration for the script in the package at `dir` (relative to the project root):
/// the detected id first (a user override keeps it), then `npm run <name>` args in the same working
/// directory.
pub fn script_config_for<'c>(
    configs: &'c [RunConfig],
    name: &str,
    dir: &str,
    package_name: Option<&str>,
) -> Option<&'c RunConfig> {
    let ids = script_config_ids(name, package_name, dir);
    let in_dir = |config: &&RunConfig| normal_cwd(config) == dir;
    let runs = |config: &&RunConfig| {
        let args: Vec<&str> = config.args.iter().take(2).map(String::as_str).collect();
        config.kind == "npm" && args == ["run", name]
    };
    let by_id = configs.iter().filter(in_dir).find(|config| ids.contains(&config.id));
    by_id.or_else(|| configs.iter().filter(in_dir).find(runs))
}

/// What the backend would detect: test scripts render results in the test tree.
fn is_test_script(name: &str) -> bool {
    matches!(name, "test" | "vitest" | "jest") || name.starts_with("test:")
}

/// What choosing a row of a script's popup does.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptCommand {
    /// Run the project's configuration with this id.
    RunConfig(String),
    /// Run this configuration as a temporary one.
    RunTemporary(Box<RunConfig>),
    Debug(Box<RunConfig>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScriptMenuItem {
    pub label: String,
    pub icon: &'static str,
    pub shortcut: Option<String>,
    pub run: ScriptCommand,
}

/// The popup of a script's gutter icon.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptMenu {
    /// The configuration's name, as in "Run '<label>'".
    pub label: String,
    pub config: RunConfig,
    /// The project has this configuration (else it is a temporary one).
    pub matched: bool,
}

impl ScriptMenu {
    /// The Run and Debug rows.
    pub fn items(&self, shortcuts: ContextShortcuts, i18n: &mut I18n) -> [ScriptMenuItem; 2] {
        let label = |verb: &str, i18n: &mut I18n| i18n.tx_built(&[verb, "'"], &[Param::from(self.label.as_str())]);
        let run = if self.matched {
            ScriptCommand::RunConfig(self.config.id.clone())
        } else {
            ScriptCommand::RunTemporary(Box::new(self.config.clone()))
        };
        let debug = ScriptCommand::Debug(Box::new(self.config.clone()));
        let (run_shortcut, debug_shortcut) = (shortcuts.run.map(str::to_string), shortcuts.debug.map(str::to_string));
        [
            ScriptMenuItem { label: label("Run '", i18n), icon: "run", shortcut: run_shortcut, run },
            ScriptMenuItem { label: label("Debug '", i18n), icon: "debug", shortcut: debug_shortcut, run: debug },
        ]
    }
}

/// What making a script's popup needs next.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptMenuStep {
    /// Read the root package's manifest (workspace detection uses its manager, even for a member's
    /// first script), then [`ScriptMenuFlow::root_package_read`] (`None` when it cannot be read).
    ReadRootPackage(String),
    /// List the files (and links to files) of this directory, then [`ScriptMenuFlow::dir_read`] (none
    /// when it cannot be read).
    ReadDir(String),
    Done(Box<ScriptMenu>),
}

/// Makes the popup of the script `name` of the `package.json` at `path`. Start it with the
/// configurations as they are after a reload: detection reads the disk, so a script typed moments ago
/// is already there.
#[derive(Clone, Debug)]
pub struct ScriptMenuFlow {
    name: String,
    path: String,
    root: Option<String>,
    /// The package's directory, relative to the project root.
    dir: String,
    package: Option<PackageJson>,
    root_package: Option<PackageJson>,
}

impl ScriptMenuFlow {
    pub fn start(
        name: &str,
        path: &str,
        content: &str,
        root: Option<&str>,
        configs: &[RunConfig],
    ) -> (Self, ScriptMenuStep) {
        let below = root.and_then(|root| path.strip_prefix(root)?.strip_prefix('/'));
        let dir = below.unwrap_or(path).strip_suffix("/package.json").unwrap_or_default().to_string();
        let package = parse_package_json(utf16::slice(content, 0, MAX_SCAN));
        let package_name = package.as_ref().and_then(|package| package.name.as_deref());
        let matched = script_config_for(configs, name, &dir, package_name);
        // Reuse detection for sibling scripts, including a workspace's inherited manager.
        let sibling = |config: &&RunConfig| {
            let manager = PACKAGE_MANAGERS.contains(&config.command.as_str());
            config.kind == "npm" && config.detected && normal_cwd(config) == dir && manager
        };
        let known = matched.or_else(|| configs.iter().find(sibling)).map(|config| config.command.clone());
        let matched = matched.cloned();
        let flow = Self {
            name: name.to_string(),
            path: path.to_string(),
            root: root.map(str::to_string),
            dir,
            package,
            root_package: None,
        };
        let step = match (known, &flow.root) {
            (Some(manager), _) => flow.done(&manager, matched),
            (None, Some(root)) if !flow.dir.is_empty() => {
                ScriptMenuStep::ReadRootPackage(format!("{root}/package.json"))
            }
            (None, _) => flow.read_dir(),
        };
        (flow, step)
    }

    fn read_dir(&self) -> ScriptMenuStep {
        let own = || self.path.strip_suffix("package.json").unwrap_or(&self.path).to_string();
        let dir = match (&self.root_package, &self.root) {
            (Some(_), Some(root)) => format!("{root}/"),
            _ => own(),
        };
        ScriptMenuStep::ReadDir(dir)
    }

    pub fn root_package_read(&mut self, text: Option<&str>) -> ScriptMenuStep {
        self.root_package = text.and_then(parse_package_json);
        self.read_dir()
    }

    pub fn dir_read<S: AsRef<str>>(&self, files: &[S]) -> ScriptMenuStep {
        let package = self.root_package.as_ref().or(self.package.as_ref());
        self.done(script_package_manager(package, files), None)
    }

    fn done(&self, manager: &str, matched: Option<RunConfig>) -> ScriptMenuStep {
        let menu = match matched {
            Some(config) => ScriptMenu { label: config.name.clone(), config, matched: true },
            None => {
                let label = format!("{manager} run {}", self.name);
                let owner = if self.dir.is_empty() { "root" } else { self.dir.as_str() };
                let config = RunConfig {
                    command: manager.to_string(),
                    args: vec!["run".to_string(), self.name.clone()],
                    cwd: to_cwd(Some(self.dir.as_str()).filter(|dir| !dir.is_empty())),
                    detected: true,
                    is_test: is_test_script(&self.name),
                    debug_adapter: Some("js-debug".to_string()),
                    ..blank_config(format!("context:npm:{owner}:{}", self.name), label.as_str(), "npm")
                };
                ScriptMenu { label, config, matched: false }
            }
        };
        ScriptMenuStep::Done(Box::new(menu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(id: &str, command: &str, args: &[&str], cwd: Option<&str>) -> RunConfig {
        RunConfig {
            command: command.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            cwd: to_cwd(cwd),
            detected: true,
            ..blank_config(id, id, "npm")
        }
    }

    fn named(config: RunConfig, name: &str) -> RunConfig {
        RunConfig { name: name.to_string(), ..config }
    }

    fn found(text: &str) -> Vec<(String, u32)> {
        script_entries(text).into_iter().map(|entry| (entry.name, entry.line)).collect()
    }

    fn entries(expected: &[(&str, u32)]) -> Vec<(String, u32)> {
        expected.iter().map(|(name, line)| (name.to_string(), *line)).collect()
    }

    #[test]
    fn lists_scripts_with_their_lines_in_document_order() {
        let text = [
            "{",
            "  \"name\": \"app\",",
            "  \"scripts\": {",
            "    \"dev\": \"vite\",",
            "    \"build\": \"tsc && vite build\",",
            "    \"test\": \"vitest run\"",
            "  }",
            "}",
        ]
        .join("\n");
        assert_eq!(found(&text), entries(&[("dev", 3), ("build", 4), ("test", 5)]));
    }

    #[test]
    fn parses_names_with_escapes_and_values_containing_braces_or_colons() {
        let text = [
            "{",
            "  \"scripts\": {",
            "    \"say \\\"hi\\\"\": \"echo \\\"{\\\" done\",",
            "    \"a\\u0041b\": \"node -e \\\"x:{y}\\\"\"",
            "  }",
            "}",
        ]
        .join("\n");
        assert_eq!(found(&text), entries(&[("say \"hi\"", 2), ("aAb", 3)]));
    }

    #[test]
    fn ignores_scripts_nested_in_other_objects() {
        let text = [
            "{",
            "  \"a\": {",
            "    \"scripts\": {",
            "      \"inner\": \"x\"",
            "    }",
            "  },",
            "  \"scripts\": {",
            "    \"outer\": \"y\"",
            "  }",
            "}",
        ]
        .join("\n");
        assert_eq!(found(&text), entries(&[("outer", 7)]));
    }

    #[test]
    fn tracks_lines_through_object_array_and_scalar_values() {
        let text = [
            "{",
            "  \"scripts\": {",
            "    \"a\": 1,",
            "    \"b\": true,",
            "    \"c\": null,",
            "    \"d\": [1, 2],",
            "    \"e\": { \"x\": 1 },",
            "    \"f\": \"ok\"",
            "  }",
            "}",
        ]
        .join("\n");
        assert_eq!(found(&text), entries(&[("a", 2), ("b", 3), ("c", 4), ("d", 5), ("e", 6), ("f", 7)]));
    }

    #[test]
    fn returns_nothing_without_a_scripts_key() {
        assert!(script_entries("{\n  \"name\": \"x\"\n}\n").is_empty());
    }

    #[test]
    fn returns_an_empty_scripts_object_as_no_entries() {
        assert!(script_entries("{\n  \"scripts\": {}\n}\n").is_empty());
    }

    #[test]
    fn ignores_a_scripts_value_that_is_not_an_object() {
        assert!(script_entries("{\n  \"scripts\": \"dev\"\n}\n").is_empty());
    }

    #[test]
    fn keeps_entries_parsed_before_malformed_text() {
        let text = ["{", "  \"scripts\": {", "    \"a\": \"x\",", "    \"b\": "].join("\n");
        assert_eq!(found(&text), entries(&[("a", 2)]));
    }

    #[test]
    fn tolerates_a_trailing_comma() {
        assert_eq!(found("{\n  \"scripts\": {\n    \"a\": \"x\",\n  },\n}\n"), entries(&[("a", 2)]));
    }

    #[test]
    fn skips_a_bom() {
        assert_eq!(found("\u{feff}{\n  \"scripts\": {\n    \"dev\": \"vite\"\n  }\n}\n"), entries(&[("dev", 2)]));
    }

    #[test]
    fn tracks_crlf_lines_after_non_ascii_and_astral_characters() {
        let text = [
            "{",
            "  \"description\": \"中文 🚀 café\",",
            "  \"scripts\": {",
            "    \"构建🚀\": \"echo 完成\",",
            "    \"test\": \"echo ok\"",
            "  }",
            "}",
        ]
        .join("\r\n");
        assert_eq!(found(&text), entries(&[("构建🚀", 3), ("test", 4)]));
    }

    #[test]
    fn decodes_escaped_unicode_and_backslashes_in_keys_and_values() {
        let text = r#"{"scripts":{"构建🚀":"echo \"ok\" \\path","say\\hi":"echo ok"}}"#;
        assert_eq!(found(text), entries(&[("构建🚀", 0), ("say\\hi", 0)]));
    }

    #[test]
    fn produces_no_entries_for_scripts_nested_in_an_object_or_array() {
        assert!(script_entries(r#"{"config":{"scripts":{"nested":"echo no"}}}"#).is_empty());
        assert!(script_entries(r#"{"config":[{"scripts":{"nested":"echo no"}}]}"#).is_empty());
    }

    #[test]
    fn skips_long_numeric_metadata_before_scripts() {
        let text = r#"{"metadata":1234567890123456,"scripts":{"dev":"echo ok"}}"#;
        assert_eq!(found(text), entries(&[("dev", 0)]));
    }

    fn declaring(manager: &str) -> PackageJson {
        PackageJson { name: None, package_manager: Some(manager.to_string()) }
    }

    #[test]
    fn prefers_the_declared_manager_over_lockfiles() {
        for manager in ["npm", "pnpm", "yarn", "bun"] {
            let package = declaring(&format!("{manager}@1.0.0"));
            let lockfiles = ["pnpm-lock.yaml", "yarn.lock", "bun.lock"];
            assert_eq!(script_package_manager(Some(&package), &lockfiles), manager);
        }
    }

    #[test]
    fn uses_the_lockfile_when_no_manager_is_declared() {
        let cases = [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lock", "bun"),
            ("bun.lockb", "bun"),
            ("package-lock.json", "npm"),
        ];
        for (file, manager) in cases {
            assert_eq!(script_package_manager(Some(&PackageJson::default()), &[file]), manager, "{file}");
            assert_eq!(script_package_manager(Some(&declaring("unknown@1")), &[file]), manager, "{file}");
        }
    }

    #[test]
    fn uses_backend_lockfile_precedence_and_defaults_to_npm() {
        assert_eq!(script_package_manager(None, &["bun.lock", "yarn.lock", "pnpm-lock.yaml"]), "pnpm");
        assert_eq!(script_package_manager(None, &["bun.lock", "yarn.lock"]), "yarn");
        assert_eq!(script_package_manager::<&str>(None, &[]), "npm");
    }

    const LOCKFILE: [&str; 1] = ["pnpm-lock.yaml"];

    /// The labels of the popup's rows.
    fn labels(step: &ScriptMenuStep) -> Vec<String> {
        let ScriptMenuStep::Done(menu) = step else { panic!("the popup, not {step:?}") };
        let items = menu.items(ContextShortcuts::default(), &mut I18n::default());
        items.into_iter().map(|item| item.label).collect()
    }

    /// The popup of the script `new` of the package at `path` in the project at `/project`.
    fn start(path: &str, content: &str, configs: &[RunConfig]) -> (ScriptMenuFlow, ScriptMenuStep) {
        ScriptMenuFlow::start("new", path, content, Some("/project"), configs)
    }

    #[test]
    fn uses_the_package_lockfile_for_an_undetected_script() {
        let (flow, step) = start("/project/package.json", "{}", &[]);
        assert_eq!(step, ScriptMenuStep::ReadDir("/project/".to_string()));
        assert_eq!(labels(&flow.dir_read(&LOCKFILE)), ["Run 'pnpm run new'", "Debug 'pnpm run new'"]);
    }

    #[test]
    fn inherits_the_backends_workspace_manager_from_a_sibling_script() {
        let configs = [cfg("x", "yarn", &[], Some("packages/web"))];
        let (_, step) = start("/project/packages/web/package.json", r#"{"packageManager":"pnpm@9"}"#, &configs);
        // Nothing is read from the disk.
        assert_eq!(labels(&step), ["Run 'yarn run new'", "Debug 'yarn run new'"]);
    }

    #[test]
    fn does_not_inherit_an_unrelated_packages_manager_or_a_custom_executable() {
        let configs = [cfg("x", "yarn", &[], Some("other")), cfg("x", "echo", &[], None)];
        let (flow, step) = start("/project/package.json", "{}", &configs);
        assert_eq!(step, ScriptMenuStep::ReadDir("/project/".to_string()));
        assert_eq!(labels(&flow.dir_read(&LOCKFILE)), ["Run 'pnpm run new'", "Debug 'pnpm run new'"]);
    }

    #[test]
    fn uses_root_detection_for_a_workspace_members_first_script() {
        let (mut flow, step) = start("/project/packages/web/package.json", "{}", &[]);
        assert_eq!(step, ScriptMenuStep::ReadRootPackage("/project/package.json".to_string()));
        let step = flow.root_package_read(Some(r#"{"packageManager":"yarn@4"}"#));
        assert_eq!(step, ScriptMenuStep::ReadDir("/project/".to_string()));
        assert_eq!(labels(&flow.dir_read(&LOCKFILE)), ["Run 'yarn run new'", "Debug 'yarn run new'"]);
    }

    #[test]
    fn uses_local_detection_for_a_polyglot_package_without_a_root_manifest() {
        let (mut flow, _) = start("/project/web/package.json", "{}", &[]);
        assert_eq!(flow.root_package_read(None), ScriptMenuStep::ReadDir("/project/web/".to_string()));
        assert_eq!(labels(&flow.dir_read(&LOCKFILE)), ["Run 'pnpm run new'", "Debug 'pnpm run new'"]);
    }

    #[test]
    fn preserves_a_matching_user_configuration() {
        let configs = [named(cfg("npm:new", "custom", &[], None), "Custom new")];
        let (_, step) = start("/project/package.json", "{}", &configs);
        // Nothing is read from the disk.
        assert_eq!(labels(&step), ["Run 'Custom new'", "Debug 'Custom new'"]);
        let ScriptMenuStep::Done(menu) = step else { panic!("the popup") };
        let [run, _] = menu.items(ContextShortcuts::default(), &mut I18n::default());
        assert_eq!(run.run, ScriptCommand::RunConfig("npm:new".to_string()));
    }

    #[test]
    fn renders_a_native_button_and_opens_its_menu_on_keyboard_style_activation_after_unicode_text() {
        let text = "{\r\n  \"description\": \"中文 🚀\",\r\n  \"scripts\": {\r\n    \"dev\": \"echo ok\"\r\n  }\r\n}";
        // The icon sits on the script's line, whatever came before it.
        assert_eq!(found(text), entries(&[("dev", 3)]));
        let (flow, step) = ScriptMenuFlow::start("dev", "/project/package.json", text, Some("/project"), &[]);
        assert_eq!(step, ScriptMenuStep::ReadDir("/project/".to_string()));
        let step = flow.dir_read(&LOCKFILE);
        assert_eq!(labels(&step), ["Run 'pnpm run dev'", "Debug 'pnpm run dev'"]);
        let ScriptMenuStep::Done(menu) = step else { panic!("the popup") };
        assert_eq!((menu.config.id.as_str(), menu.config.is_test), ("context:npm:root:dev", false));
        assert_eq!(menu.config.debug_adapter.as_deref(), Some("js-debug"));
    }

    fn root_dev() -> RunConfig {
        named(cfg("npm:dev", "pnpm", &["run", "dev"], None), "pnpm run dev")
    }

    #[test]
    fn matches_the_root_packages_detected_id() {
        let configs = [root_dev()];
        assert_eq!(script_config_for(&configs, "dev", "", None), Some(&configs[0]));
    }

    #[test]
    fn matches_a_workspace_member_by_its_package_name_or_directory() {
        let configs = [named(cfg("npm:web:dev", "pnpm", &["run", "dev"], Some("web")), "web: pnpm run dev")];
        assert_eq!(script_config_for(&configs, "dev", "web", Some("web")), Some(&configs[0]));
        assert_eq!(script_config_for(&configs, "dev", "web", None), Some(&configs[0]));
    }

    #[test]
    fn falls_back_to_npm_run_args_in_the_same_directory() {
        let configs = [named(cfg("user-cfg", "npm", &["run", "dev"], Some("./web/")), "my dev")];
        assert_eq!(script_config_for(&configs, "dev", "web", None), Some(&configs[0]));
    }

    #[test]
    fn matches_an_unnamed_nested_members_overridden_config_by_its_full_relative_directory() {
        let configs = [cfg("npm:packages/web:dev", "custom", &[], Some("packages/web"))];
        assert_eq!(script_config_for(&configs, "dev", "packages/web", None), Some(&configs[0]));
    }

    #[test]
    fn prefers_an_id_match_over_an_args_match() {
        let configs = [cfg("user-cfg", "npm", &["run", "dev"], None), root_dev()];
        assert_eq!(script_config_for(&configs, "dev", "", None), Some(&configs[1]));
    }

    #[test]
    fn finds_nothing_in_another_directory() {
        assert_eq!(script_config_for(&[root_dev()], "dev", "web", None), None);
    }

    #[test]
    fn derives_the_backend_configuration_ids() {
        assert_eq!(script_config_ids("dev", None, ""), ["npm:dev"]);
        let ids = script_config_ids("dev", Some("web"), "packages/web");
        assert_eq!(ids, ["npm:dev", "npm:web:dev", "npm:packages/web:dev"]);
    }
}
