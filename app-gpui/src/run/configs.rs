//! Run configuration helpers: labels and icons, grouping, overriding, validation, compounds and the
//! one-line environment field.

use crate::platform::command_line::format_args;
use crate::run::config_types::RunConfigTypes;
use ide_core::IpcPath;
use ide_run::{RunConfig, RunExtras};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::atomic::{AtomicU32, Ordering};

/// Display names of the built-in kinds, in the order the kind selector lists them.
pub const KIND_LABELS: [(&str, &str); 36] = [
    ("npm", "npm"),
    ("deno", "Deno"),
    ("bun", "Bun"),
    ("cargo", "Cargo"),
    ("gradle", "Gradle"),
    ("maven", "Maven"),
    ("go", "Go"),
    ("python", "Python"),
    ("pytest", "pytest"),
    ("phpunit", "PHPUnit"),
    ("composer", "Composer"),
    ("make", "Make"),
    ("cmake", "CMake"),
    ("just", "Just"),
    ("shell", "Shell Script"),
    ("program", "Program"),
    ("compound", "Compound"),
    ("application", "Application"),
    ("spring-boot", "Spring Boot"),
    ("node", "Node.js"),
    ("java-file", "Java Single File"),
    ("rustc", "Rust Single File"),
    ("kotlinc", "Kotlin Single File"),
    ("cc", "C/C++ Single File"),
    ("native", "Native Executable"),
    ("cmake-application", "CMake Application"),
    ("ruby", "Ruby"),
    ("php", "PHP"),
    ("perl", "Perl"),
    ("lua", "Lua"),
    ("r", "R"),
    ("swift", "Swift"),
    ("dart", "Dart"),
    ("elixir", "Elixir"),
    ("scala-cli", "Scala CLI"),
    ("kotlin", "Kotlin Script"),
];

/// The menu category of a built-in kind, for the Services "Add Service" menu (IDEA's template groups).
pub const KIND_CATEGORIES: [(&str, &str); 25] = [
    ("spring-boot", "Java"),
    ("application", "Java"),
    ("native", "Java"),
    ("java-file", "Java"),
    ("kotlin", "Java"),
    ("kotlinc", "Java"),
    ("scala-cli", "Java"),
    ("maven", "Build Tools"),
    ("gradle", "Build Tools"),
    ("cmake", "Build Tools"),
    ("make", "Build Tools"),
    ("just", "Build Tools"),
    ("npm", "JavaScript"),
    ("node", "JavaScript"),
    ("bun", "JavaScript"),
    ("deno", "JavaScript"),
    ("python", "Python"),
    ("pytest", "Python"),
    ("cargo", "Rust"),
    ("rustc", "Rust"),
    ("go", "Go"),
    ("cc", "C/C++"),
    ("cmake-application", "C/C++"),
    ("shell", "Shell"),
    ("program", "Shell"),
];

/// Category order of the Add Service menu; unlisted categories (plugins) sort after, then "Others".
pub const CATEGORY_ORDER: [&str; 10] =
    ["Java", "Build Tools", "JavaScript", "Python", "Rust", "Go", "C/C++", "Shell", "Servers", "Docker"];

/// A configuration of `kind` with nothing else set: no command, the system environment included.
pub fn blank_config(id: impl Into<String>, name: impl Into<String>, kind: impl Into<String>) -> RunConfig {
    RunConfig {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        command: String::new(),
        args: Vec::new(),
        cwd: None,
        env: BTreeMap::new(),
        detected: false,
        is_test: false,
        debug_adapter: None,
        allow_parallel: false,
        extras: RunExtras::default(),
    }
}

/// The working directory as the UI holds it (`/`-separated); `None` for the project root.
pub fn cwd_of(config: &RunConfig) -> Option<&str> {
    config.cwd.as_deref().and_then(|cwd| cwd.to_str())
}

/// A working directory in the form configurations store it.
pub fn to_cwd(cwd: Option<&str>) -> Option<IpcPath> {
    cwd.map(IpcPath::from)
}

/// The value of the type param `key`; `None` when the configuration does not set it.
pub fn param<'a>(config: &'a RunConfig, key: &str) -> Option<&'a str> {
    config.extras.params.get(key).map(String::as_str)
}

fn builtin_label(kind: &str) -> Option<&'static str> {
    KIND_LABELS.iter().find(|(id, _)| *id == kind).map(|(_, label)| *label)
}

/// The menu category of a kind: a registered type's group (Docker, Servers), else its language or
/// tool category, else "Others".
pub fn category_of(kind: &str, types: &RunConfigTypes) -> String {
    if let Some(group) = types.get(kind).and_then(|ty| ty.group.clone()) {
        return group;
    }
    KIND_CATEGORIES.iter().find(|(id, _)| *id == kind).map_or("Others", |(_, category)| *category).to_string()
}

/// The display name of a kind: its registered type's label, else the built-in one, else the kind
/// with a capital first letter.
pub fn kind_label(kind: &str, types: &RunConfigTypes) -> String {
    if let Some(ty) = types.get(kind) {
        return ty.label.clone();
    }
    if let Some(label) = builtin_label(kind) {
        return label.to_string();
    }
    let mut chars = kind.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Other".to_string(),
    }
}

/// The icon of a configuration of `kind`; tests share one icon whatever runs them.
pub fn config_icon(kind: &str, is_test: bool, types: &RunConfigTypes) -> String {
    if is_test {
        return "check".to_string();
    }
    let icon = match kind {
        "compound" => "gutter-run-all",
        "program" | "native" => "file-binary",
        "spring-boot" => "spring-boot",
        "tomcat" => "tomcat",
        "npm" | "bun" => "file-npm",
        "deno" => "file-ts",
        "cargo" => "file-cargo",
        "gradle" => "file-gradle",
        "maven" => "file-maven",
        "dotnet" | "dotnet-test" => "file-csharp",
        "go" => "file-go",
        "node" => "file-js",
        "application" => "class",
        "java-file" => "file-java",
        "rustc" => "file-rust",
        "cc" | "cmake-application" => "file-cpp",
        "shell" => "file-shell",
        "ruby" => "file-ruby",
        "php" | "phpunit" | "composer" => "file-php",
        "perl" => "file-perl",
        "lua" => "file-lua",
        "r" => "file-r",
        "swift" => "file-swift",
        "dart" => "file-dart",
        "elixir" => "file-elixir",
        "scala-cli" => "file-scala",
        "kotlinc" => "file-kotlin",
        "kotlin" => "file-kotlin-script",
        "python" | "pytest" => "file-python",
        _ => return types.get(kind).map_or("terminal", |ty| ty.icon.as_str()).to_string(),
    };
    icon.to_string()
}

/// The configurations of one kind.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigGroup {
    pub kind: String,
    pub label: String,
    pub configs: Vec<RunConfig>,
}

/// Groups by kind, keeping the backend's order (detection order) within and across groups.
pub fn group_configs(configs: &[RunConfig], types: &RunConfigTypes) -> Vec<ConfigGroup> {
    let mut groups: Vec<ConfigGroup> = Vec::new();
    for config in configs {
        match groups.iter_mut().find(|group| group.kind == config.kind) {
            Some(group) => group.configs.push(config.clone()),
            None => groups.push(ConfigGroup {
                kind: config.kind.clone(),
                label: kind_label(&config.kind, types),
                configs: vec![config.clone()],
            }),
        }
    }
    groups
}

/// Configurations persisted in `.openidea/run.toml` (everything not regenerated by detection).
pub fn user_configs(configs: &[RunConfig]) -> Vec<RunConfig> {
    configs.iter().filter(|config| !config.detected).cloned().collect()
}

/// Applies an edit. Editing a detected configuration produces a user copy with the same id, which then
/// overrides the detected one (the backend drops detected entries whose id a user configuration shares).
pub fn upsert_config(configs: &[RunConfig], edited: &RunConfig) -> Vec<RunConfig> {
    let next = RunConfig { detected: false, ..edited.clone() };
    let mut out = configs.to_vec();
    match out.iter_mut().find(|config| config.id == edited.id) {
        Some(existing) => *existing = next,
        None => out.push(next),
    }
    out
}

pub fn remove_config(configs: &[RunConfig], id: &str) -> Vec<RunConfig> {
    configs.iter().filter(|config| config.id != id).cloned().collect()
}

/// Merges detected and user configurations the way the backend does (user ids win, user configurations last).
pub fn merge_configs(detected: &[RunConfig], user: &[RunConfig]) -> Vec<RunConfig> {
    let ids: HashSet<&str> = user.iter().map(|config| config.id.as_str()).collect();
    detected.iter().filter(|config| !ids.contains(config.id.as_str())).chain(user).cloned().collect()
}

/// A fresh id for a user configuration.
pub fn uid() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    // A randomly keyed hasher is the standard library's source of per-call entropy.
    let random = RandomState::new().build_hasher().finish() as u32;
    format!("user:{random:08x}{}", COUNTER.fetch_add(1, Ordering::Relaxed) + 1)
}

/// `base`, or `base (2)`, `base (3)`… when a configuration already has that name.
pub fn unique_name(base: &str, configs: &[RunConfig]) -> String {
    let names: HashSet<&str> = configs.iter().map(|config| config.name.as_str()).collect();
    if !names.contains(base) {
        return base.to_string();
    }
    (2u32..).map(|i| format!("{base} ({i})")).find(|name| !names.contains(name.as_str())).unwrap_or_default()
}

/// A new, empty shell configuration.
pub fn new_config(configs: &[RunConfig]) -> RunConfig {
    blank_config(uid(), unique_name("Unnamed", configs), "shell")
}

/// A user copy of `config` under a new id and name.
pub fn duplicate_config(config: &RunConfig, configs: &[RunConfig]) -> RunConfig {
    RunConfig {
        id: uid(),
        name: unique_name(&format!("{} (copy)", config.name), configs),
        detected: false,
        ..config.clone()
    }
}

/// Field errors by field: `name`, `command`, `env`, `cwd`, `compound`, `beforeLaunch` or a type param key.
pub type ConfigErrors = BTreeMap<String, String>;

/// Why a Build Module task of `config` targeting the module `target` cannot run, if it cannot.
pub fn module_task_error(config: &RunConfig, target: &str) -> Option<&'static str> {
    match config.extras.module.as_deref() {
        None => Some("Select a module for the Build Module task"),
        Some(module) if module != target => {
            Some("The Build Module task uses a different module; remove it and add it again")
        }
        Some(_) => None,
    }
}

/// The errors of `config` among `all` configurations (which may include `config` itself).
pub fn validate_config(config: &RunConfig, all: &[RunConfig], types: &RunConfigTypes) -> ConfigErrors {
    let mut errors = ConfigErrors::new();
    let mut error = |field: &str, message: String| {
        errors.insert(field.to_string(), message);
    };
    let name = config.name.trim();
    if name.is_empty() {
        error("name", "Name is required".into());
    } else if all.iter().any(|other| other.id != config.id && other.name.trim() == name) {
        error("name", "Another configuration has this name".into());
    }
    if config.kind == "compound" {
        let children = &config.extras.compound;
        if children.is_empty() {
            error("compound", "Add at least one configuration".into());
        } else if compound_cycle(config, all) {
            error("compound", "A compound configuration can't include itself".into());
        } else if children.iter().any(|id| !all.iter().any(|other| &other.id == id)) {
            error("compound", "Some configurations no longer exist".into());
        }
    } else if config.command.trim().is_empty() && !types.get(&config.kind).is_some_and(|ty| ty.debug_only) {
        error("command", "Command is required".into());
    }
    for task in &config.extras.before_launch {
        let kind = task.kind.as_str();
        let runs_config = kind == "run" || (kind == "build" && !task.target.is_empty());
        if runs_config && (task.target == config.id || !all.iter().any(|other| other.id == task.target)) {
            error("beforeLaunch", "A before launch task runs a missing configuration or this one".into());
        } else if kind == "buildModule" {
            if let Some(message) = module_task_error(config, &task.target) {
                error("beforeLaunch", message.into());
            }
        } else if !matches!(kind, "build" | "buildArtifacts" | "dotnetBuild" | "run") && task.target.trim().is_empty() {
            error("beforeLaunch", "A before launch task is incomplete".into());
        }
    }
    for key in config.env.keys() {
        if key.trim().is_empty() {
            error("env", "Environment variable names can't be empty".into());
            break;
        }
        if key.chars().any(|c| c == '=' || c == '\0' || c.is_whitespace()) {
            error("env", format!("Invalid environment variable name: {key}"));
            break;
        }
    }
    if cwd_of(config).is_some_and(|cwd| cwd.contains('\0')) {
        error("cwd", "Invalid directory".into());
    }
    errors
}

/// Display command line, e.g. `pnpm run dev`.
pub fn command_line(command: &str, args: &[String]) -> String {
    let args = format_args(args);
    [command, args.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Whether compound `config` reaches itself through its children.
pub fn compound_cycle(config: &RunConfig, all: &[RunConfig]) -> bool {
    let mut by_id: HashMap<&str, &RunConfig> = all.iter().map(|other| (other.id.as_str(), other)).collect();
    by_id.insert(&config.id, config);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut pending = vec![config.id.as_str()];
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        for child in by_id.get(id).map_or(&[][..], |found| found.extras.compound.as_slice()) {
            if *child == config.id {
                return true;
            }
            pending.push(child);
        }
    }
    false
}

/// The configurations a compound starts (nested compounds flattened, each once).
pub fn compound_children(config: &RunConfig, all: &[RunConfig]) -> Vec<RunConfig> {
    fn walk<'a>(ids: &'a [String], all: &'a [RunConfig], seen: &mut HashSet<&'a str>, out: &mut Vec<RunConfig>) {
        for id in ids {
            if !seen.insert(id) {
                continue;
            }
            let Some(child) = all.iter().find(|other| &other.id == id) else { continue };
            if child.kind == "compound" {
                walk(&child.extras.compound, all, seen, out);
            } else {
                out.push(child.clone());
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::from([config.id.as_str()]);
    walk(&config.extras.compound, all, &mut seen, &mut out);
    out
}

/// IDEA's one-line environment field: `A=1;B=2` (`;` and `\` in values escaped).
pub fn format_env(env: &BTreeMap<String, String>) -> String {
    let escaped = |value: &str| value.replace('\\', "\\\\").replace(';', "\\;");
    env.iter().map(|(key, value)| format!("{key}={}", escaped(value))).collect::<Vec<_>>().join(";")
}

/// A variable read from the environment field or pasted text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
}

/// Parses `A=1;B=2`, or one `NAME=value` per line as pasted from a shell or a `.env` file (`export`
/// prefixes, comments and surrounding quotes dropped).
pub fn parse_env(text: &str) -> Vec<EnvEntry> {
    let multiline = text.trim().contains('\n');
    let mut entries: Vec<String> = Vec::new();
    if multiline {
        entries.extend(text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line).to_string()));
    } else {
        let mut current = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' if matches!(chars.peek(), Some(';' | '\\')) => current.extend(chars.next()),
                ';' => entries.push(std::mem::take(&mut current)),
                _ => current.push(ch),
            }
        }
        entries.push(current);
    }
    let mut out = Vec::new();
    for raw in &entries {
        let mut line = if multiline { raw.trim() } else { raw.trim_start() };
        if line.is_empty() || (multiline && line.starts_with('#')) {
            continue;
        }
        if multiline && let Some(rest) = line.strip_prefix("export") {
            let assignment = rest.trim_start();
            if assignment.len() < rest.len() {
                line = assignment;
            }
        }
        let Some(eq) = line.find('=').filter(|eq| *eq > 0) else { continue };
        let key = line[..eq].trim().to_string();
        let mut value = &line[eq + 1..];
        if multiline {
            value = value.trim();
            let quoted = value.len() > 1
                && value.chars().next().is_some_and(|quote| matches!(quote, '"' | '\'') && value.ends_with(quote));
            if quoted {
                value = &value[1..value.len() - 1];
            }
        }
        out.push(EnvEntry { key, value: value.to_string() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::command_line::parse_args;
    use crate::run::config_types::RunConfigType;

    fn cfg(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: "echo".into(), detected: true, ..blank_config(id, id, kind) }
    }

    fn detected() -> Vec<RunConfig> {
        vec![cfg("npm:dev", "npm"), RunConfig { is_test: true, ..cfg("npm:test", "npm") }]
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn user_configs_override_detected_ones_with_the_same_id() {
        let user = [RunConfig { name: "dev (custom)".into(), detected: false, ..cfg("npm:dev", "npm") }];
        let merged = merge_configs(&detected(), &user);
        let pairs: Vec<(&str, &str)> = merged.iter().map(|c| (c.id.as_str(), c.name.as_str())).collect();
        assert_eq!(pairs, [("npm:test", "npm:test"), ("npm:dev", "dev (custom)")]);
    }

    #[test]
    fn keeps_saved_cmake_names_when_generated_context_changes() {
        let id = "cmake:build:Debug:hello";
        let saved = RunConfig { name: "My hello (build)".into(), detected: false, ..cfg(id, "cmake-application") };
        for name in ["hello", "hello [Debug · build/]"] {
            let generated = RunConfig { name: name.into(), ..cfg(id, "cmake-application") };
            assert_eq!(merge_configs(&[generated], std::slice::from_ref(&saved)), std::slice::from_ref(&saved));
        }
    }

    #[test]
    fn editing_a_detected_config_creates_a_user_copy_with_the_same_id() {
        let detected = detected();
        let args = strings(&["run", "dev", "--port", "3001"]);
        let edited = upsert_config(&detected, &RunConfig { args: args.clone(), ..detected[0].clone() });
        assert_eq!(edited[0].id, "npm:dev");
        assert!(!edited[0].detected);
        assert_eq!(edited[0].args, args);
        assert_eq!(user_configs(&edited).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["npm:dev"]);
        assert!(edited[1].detected);
    }

    #[test]
    fn adds_duplicates_and_removes_user_configs() {
        let detected = detected();
        let added = new_config(&detected);
        assert!(added.id.starts_with("user:"));
        assert!(!added.detected);
        let mut list = upsert_config(&detected, &added);
        let copy = duplicate_config(&detected[0], &list);
        assert_eq!(copy.name, "npm:dev (copy)");
        assert!(!copy.detected);
        assert_ne!(copy.id, detected[0].id);
        list = upsert_config(&list, &copy);
        assert_eq!(user_configs(&list).len(), 2);
        list = remove_config(&list, &added.id);
        assert_eq!(user_configs(&list).iter().map(|c| c.id.clone()).collect::<Vec<_>>(), [copy.id]);
    }

    #[test]
    fn groups_by_kind_in_order() {
        let groups = group_configs(&[cfg("a", "npm"), cfg("b", "cargo"), cfg("c", "npm")], &RunConfigTypes::new());
        let shape: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|group| (group.label.as_str(), group.configs.iter().map(|c| c.id.as_str()).collect()))
            .collect();
        assert_eq!(shape, [("npm", vec!["a", "c"]), ("Cargo", vec!["b"])]);
    }

    #[test]
    fn requires_a_name_and_command() {
        let config = RunConfig { name: " ".into(), command: String::new(), ..cfg("x", "shell") };
        let expected = ConfigErrors::from([
            ("name".to_string(), "Name is required".to_string()),
            ("command".to_string(), "Command is required".to_string()),
        ]);
        assert_eq!(validate_config(&config, &[], &RunConfigTypes::new()), expected);
    }

    #[test]
    fn rejects_duplicate_names_and_bad_env_keys() {
        let types = RunConfigTypes::new();
        let all = [RunConfig { name: "Server".into(), ..cfg("a", "shell") }];
        let duplicate = RunConfig { name: "Server".into(), ..cfg("b", "shell") };
        assert!(validate_config(&duplicate, &all, &types)["name"].contains("Another"));
        let bad_env = RunConfig { env: BTreeMap::from([("BAD KEY".into(), "1".into())]), ..cfg("c", "shell") };
        assert!(validate_config(&bad_env, &[], &types)["env"].contains("Invalid"));
        let good_env = RunConfig { env: BTreeMap::from([("RUST_LOG".into(), "debug".into())]), ..cfg("d", "shell") };
        assert!(validate_config(&good_env, &[], &types).is_empty());
    }

    #[test]
    fn splits_with_quotes_and_escapes() {
        let parsed = parse_args(r#"run --name "hello world" 'it''s' a\ b """#, false);
        assert_eq!(parsed, ["run", "--name", "hello world", "its", "a b", ""]);
        assert!(parse_args("  ", false).is_empty());
    }

    #[test]
    fn round_trips_through_format_args() {
        let args = strings(&["test", "--", "it's here", "", "$HOME", "plain"]);
        assert_eq!(parse_args(&format_args(&args), false), args);
        assert_eq!(command_line("cargo", &strings(&["run", "--bin", "my app"])), "cargo run --bin 'my app'");
    }

    #[test]
    fn maps_kinds_to_their_language_categories_and_unlisted_ones_to_others() {
        let types = RunConfigTypes::new();
        assert_eq!(category_of("spring-boot", &types), "Java");
        assert_eq!(category_of("maven", &types), "Build Tools");
        assert_eq!(category_of("npm", &types), "JavaScript");
        assert_eq!(category_of("cargo", &types), "Rust");
        assert_eq!(category_of("godot", &types), "Others");
    }

    // The Tomcat half of this case is asserted next to the Tomcat type.
    #[test]
    fn prefers_a_registered_types_group_over_the_category_map() {
        let mut types = RunConfigTypes::new();
        let fake = RunConfigType::new("fake-server", "Fake", "server", |_, _| None).group("Servers");
        let registration = types.register(fake);
        assert_eq!(category_of("fake-server", &types), "Servers");
        assert_eq!(kind_label("fake-server", &types), "Fake");
        types.unregister(&registration);
        assert_eq!(category_of("fake-server", &types), "Others");
    }

    // Not from a spec: a kind without a label is capitalised; tests and unknown kinds get fallback icons.
    #[test]
    fn labels_and_icons_fall_back_for_unknown_kinds() {
        let types = RunConfigTypes::new();
        assert_eq!(kind_label("godot", &types), "Godot");
        assert_eq!(kind_label("", &types), "Other");
        assert_eq!(config_icon("cargo", true, &types), "check");
        assert_eq!(config_icon("godot", false, &types), "terminal");
        assert_eq!(config_icon("native", false, &types), "file-binary");
    }

    // Not from a spec: nested compounds are flattened once and a compound reaching itself is a cycle.
    #[test]
    fn compounds_flatten_children_and_detect_cycles() {
        let compound = |id: &str, children: &[&str]| {
            let mut config = cfg(id, "compound");
            config.extras.compound = strings(children);
            config
        };
        let nested = [compound("inner", &["b", "a"]), compound("loop", &["outer"])];
        let all = [vec![cfg("a", "shell"), cfg("b", "shell")], nested.to_vec()].concat();
        let outer = compound("outer", &["a", "inner", "missing"]);
        let children = compound_children(&outer, &all);
        assert_eq!(children.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(!compound_cycle(&outer, &all));
        assert!(compound_cycle(&compound("outer", &["loop"]), &all));
    }

    // Not from a spec: the one-line form escapes `;` and `\`, and pasted shell lines lose `export` and quotes.
    #[test]
    fn environment_text_round_trips_and_reads_pasted_lines() {
        let env = BTreeMap::from([("A".to_string(), "1;2".to_string()), ("B".to_string(), "c:\\x".to_string())]);
        let line = format_env(&env);
        assert_eq!(line, "A=1\\;2;B=c:\\\\x");
        let entry = |key: &str, value: &str| EnvEntry { key: key.into(), value: value.into() };
        assert_eq!(parse_env(&line), [entry("A", "1;2"), entry("B", "c:\\x")]);
        let pasted = "# comment\nexport NAME=\"my app\"\r\nPORT = 8080 \n=skipped\n";
        assert_eq!(parse_env(pasted), [entry("NAME", "my app"), entry("PORT", "8080")]);
    }
}
