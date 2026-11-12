//! The Editor › Inspections tree: language → language server → the diagnostic codes seen,
//! plugins' problems under Plugins, plus the checks the IDE runs itself; the enabled / severity
//! state against a profile's overrides; and the profiles of the gear menu.
//!
//! Profiles are the backend's [`InspectionSettings`]: `Default` and its copies at IDE level,
//! `Project Default` and its copies with the project.

use super::page_spec::PageSpec;
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use ide_core::settings::{CustomInspectionProfile, InspectionLevel, InspectionOverride, InspectionSettings};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// A profile's overrides: inspection id → what differs from the inspection's defaults.
pub type ProfileOverrides = BTreeMap<String, InspectionOverride>;
type CustomProfiles = BTreeMap<String, CustomInspectionProfile>;

/// A checkbox in an inspection's Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectionOptionSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub default: bool,
}

/// A check the IDE runs itself (not a language server).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinInspection {
    pub id: &'static str,
    /// The producer key its diagnostics arrive under.
    pub producer: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub description: &'static str,
    /// The highlighting level when the profile sets none (else as reported).
    pub default_level: Option<InspectionLevel>,
    /// `false`: off until a profile turns it on.
    pub default_enabled: bool,
    /// Each diagnostic code is an inspection of its own (`<id>:<code>`), as a language server's are.
    pub by_code: bool,
    pub options: &'static [InspectionOptionSpec],
}

/// Turned on, the Java language server's compiler reports it.
pub const JAVA_SERIAL_VERSION_INSPECTION: &str = "java:missing-serial-version";
/// The Typo inspection.
pub const SPELLING_INSPECTION: &str = "builtin:spelling";

const fn builtin(
    id: &'static str,
    producer: &'static str,
    name: &'static str,
    group: &'static str,
    default_level: Option<InspectionLevel>,
    description: &'static str,
) -> BuiltinInspection {
    BuiltinInspection {
        id,
        producer,
        name,
        group,
        description,
        default_level,
        default_enabled: true,
        by_code: false,
        options: &[],
    }
}

const SPELLING_OPTIONS: [InspectionOptionSpec; 3] = [
    InspectionOptionSpec { key: "processCode", label: "Process code", default: true },
    InspectionOptionSpec { key: "processLiterals", label: "Process literals", default: true },
    InspectionOptionSpec { key: "processComments", label: "Process comments", default: true },
];

/// The built-in checks, in the order the page's search lists them.
pub static BUILTIN_INSPECTIONS: [BuiltinInspection; 9] = [
    builtin(
        "builtin:merge-conflict-markers",
        "merge-conflict-markers",
        "Unresolved merge conflict markers",
        "Version control",
        Some(InspectionLevel::Error),
        "Reports the <<<<<<<, ======= and >>>>>>> lines a merge leaves in a file, whatever its language and \
         whether or not a language server is installed.",
    ),
    builtin(
        "builtin:dependency-versions",
        "dependency-versions",
        "Newer dependency version available",
        "Cargo",
        Some(InspectionLevel::WeakWarning),
        "Reports newer crate versions that Cargo.toml requirements do not already allow.",
    ),
    builtin(
        "builtin:syntax",
        "syntax",
        "Syntax errors",
        "General",
        Some(InspectionLevel::Error),
        "Reports code the built-in parser cannot read, in files no language server checks (none is configured \
         for the language, or it is not installed, stopped or crashed). A running language server reports syntax \
         errors itself.",
    ),
    builtin(
        "builtin:file-references",
        "file-references",
        "Unresolved file references",
        "General",
        None,
        "Reports paths in HTML attributes, CSS url() and @import that do not name an existing file, resolved \
         relative to the file, the project root and its resource roots.",
    ),
    builtin(
        "builtin:build",
        "build",
        "Build problems",
        "General",
        None,
        "Compiler errors and warnings of the last Build Project, shown at their source positions.",
    ),
    BuiltinInspection {
        default_enabled: false,
        ..builtin(
            JAVA_SERIAL_VERSION_INSPECTION,
            "jdt-serial-version",
            "Serializable class without 'serialVersionUID'",
            "Java",
            Some(InspectionLevel::Warning),
            "Reports classes that implement Serializable but declare no serialVersionUID field. Alt+Enter on the \
             class name adds a default or a generated ID. The Java language server checks it, and restarts when \
             this is changed.",
        )
    },
    BuiltinInspection {
        by_code: true,
        ..builtin(
            "builtin:phpstan",
            "phpstan",
            "PHPStan",
            "PHP",
            Some(InspectionLevel::Warning),
            "Errors the project's vendor/bin/phpstan reports in a PHP file when it is saved, shown at their \
             lines. PHPStan reads phpstan.neon, phpstan.neon.dist or phpstan.dist.neon from the project root, or \
             the configuration file chosen in Settings › Languages & Frameworks › PHP. Each rule PHPStan has \
             reported is listed below by its identifier and can be turned off or re-graded on its own.",
        )
    },
    builtin(
        "builtin:sql",
        "sql-inspections",
        "Unresolved database references",
        "SQL",
        None,
        "Reports tables, columns and aliases in query consoles that the data source's schema does not contain.",
    ),
    BuiltinInspection {
        options: &SPELLING_OPTIONS,
        ..builtin(
            SPELLING_INSPECTION,
            "spelling",
            "Typo",
            "Proofreading",
            Some(InspectionLevel::Typo),
            "Reports misspelt words in comments, string literals and the names declarations introduce (camelCase \
             and snake_case parts checked separately), and in plain text and Markdown. Agent nouns of known words \
             (checkpointer) and, in names, two known words run together (filepath) are accepted. Words can be \
             saved to the project or application dictionary (Editor › Natural Languages › Spelling). Correcting \
             a name renames the symbol and its references.",
        )
    },
];

/// IDEA's highlighting levels with their labels, in the order the level menu lists them.
pub const INSPECTION_LEVELS: [(InspectionLevel, &str); 6] = [
    (InspectionLevel::Error, "Error"),
    (InspectionLevel::Warning, "Warning"),
    (InspectionLevel::WeakWarning, "Weak Warning"),
    (InspectionLevel::ServerProblem, "Server Problem"),
    (InspectionLevel::Typo, "Typo"),
    (InspectionLevel::None, "No highlighting"),
];

/// The label of keeping the level the server or plugin assigns.
pub const AS_REPORTED: &str = "As reported";

fn builtin_by_id(id: &str) -> Option<&'static BuiltinInspection> {
    BUILTIN_INSPECTIONS.iter().find(|builtin| builtin.id == id)
}

fn builtin_by_producer(producer: &str) -> Option<&'static BuiltinInspection> {
    BUILTIN_INSPECTIONS.iter().find(|builtin| builtin.producer == producer)
}

/// Whether inspection `id` is on when a profile doesn't say.
pub fn enabled_by_default(id: &str) -> bool {
    builtin_by_id(id).is_none_or(|builtin| builtin.default_enabled)
}

/// The inspection id of everything a producer reports.
pub fn producer_inspection_id(producer: &str) -> String {
    builtin_by_producer(producer).map_or_else(|| format!("lsp:{producer}"), |builtin| builtin.id.to_owned())
}

pub fn code_inspection_id(base: &str, code: &str) -> String {
    format!("{base}:{code}")
}

/// A diagnostic code met at runtime, offered as an inspection of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenCode {
    pub producer: String,
    pub code: String,
    pub source: String,
    /// A message it was reported with.
    pub sample: String,
}

/// A language and the language servers configured for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageServers {
    pub name: String,
    pub servers: Vec<String>,
}

/// A row of the inspection tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionNode {
    /// Unique in the tree (a server serving several languages appears under each).
    pub key: String,
    /// The inspection id; `None` for a language or category group.
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub children: Vec<InspectionNode>,
}

const OTHER: &str = "Other";
const PLUGINS: &str = "Plugins";
const PLUGIN_PREFIX: &str = "plugin:";

/// What a code node says: whose problems it stands for, the tool that reports them when that is
/// not the owner itself, and a message it was seen with.
fn code_description(owner: &str, seen: &SeenCode, i18n: &mut I18n) -> String {
    let named_source = !seen.source.is_empty() && seen.source != owner;
    let (owner, code) = (Param::from(owner), Param::from(seen.code.as_str()));
    let what = if named_source {
        let source = Param::from(seen.source.as_str());
        i18n.tx_built(&["", " problems with code ", " (", ")."], &[owner, code, source])
    } else {
        i18n.tx_built(&["", " problems with code ", "."], &[owner, code])
    };
    if seen.sample.is_empty() {
        what
    } else {
        i18n.tx_built(&["", "\n\nExample: ", ""], &[Param::from(what), Param::from(seen.sample.as_str())])
    }
}

/// The codes of an inspection as nodes of their own, by code (digit runs as numbers).
fn code_nodes(group: &str, id: &str, owner: &str, codes: &[SeenCode], i18n: &mut I18n) -> Vec<InspectionNode> {
    let mut sorted: Vec<&SeenCode> = codes.iter().collect();
    sorted.sort_by(|a, b| collate::compare_numeric(&a.code, &b.code));
    sorted
        .into_iter()
        .map(|seen| {
            let code_id = code_inspection_id(id, &seen.code);
            InspectionNode {
                key: format!("{group}/{code_id}"),
                id: Some(code_id),
                name: seen.code.clone(),
                description: code_description(owner, seen, i18n),
                children: Vec::new(),
            }
        })
        .collect()
}

/// The node of a language server (or of a plugin, named `plugin`) with the codes seen from it.
fn server_node(
    group: &str,
    server: &str,
    codes: &[SeenCode],
    languages: &str,
    plugin: Option<&str>,
    i18n: &mut I18n,
) -> InspectionNode {
    let id = producer_inspection_id(server);
    let description = match plugin {
        Some(plugin) => i18n.tx_built(
            &[
                "Problems the ",
                " plugin reports. Severity \"As reported\" keeps the level the plugin assigns; a code below can \
                 override it.",
            ],
            &[Param::from(plugin)],
        ),
        None if !languages.is_empty() => i18n.tx_built(
            &[
                "Problems the ",
                " language server reports in ",
                " files. Severity \"As reported\" keeps the level the server assigns; a code below can override it.",
            ],
            &[Param::from(server), Param::from(languages)],
        ),
        None => i18n.tx_built(
            &[
                "Problems the ",
                " language server reports. Severity \"As reported\" keeps the level the server assigns; a code \
                 below can override it.",
            ],
            &[Param::from(server)],
        ),
    };
    InspectionNode {
        key: format!("{group}/{id}"),
        children: code_nodes(group, &id, plugin.unwrap_or(server), codes, i18n),
        id: Some(id),
        name: plugin.unwrap_or(server).to_owned(),
        description,
    }
}

/// The codes known per producer, producers and codes in the order they were first met.
#[derive(Default)]
struct KnownCodes(Vec<(String, Vec<SeenCode>)>);

impl KnownCodes {
    fn producer(&mut self, producer: &str) -> &mut Vec<SeenCode> {
        let at = self.0.iter().position(|(known, _)| known == producer).unwrap_or_else(|| {
            self.0.push((producer.to_owned(), Vec::new()));
            self.0.len() - 1
        });
        &mut self.0[at].1
    }

    fn add(&mut self, seen: SeenCode) {
        let codes = self.producer(&seen.producer.clone());
        if !codes.iter().any(|known| known.code == seen.code) {
            codes.push(seen);
        }
    }

    fn of(&self, producer: &str) -> &[SeenCode] {
        self.0.iter().find(|(known, _)| known == producer).map_or(&[], |(_, codes)| codes.as_slice())
    }
}

/// The inspection tree: one group per language that has language servers, the built-in checks
/// under their category, servers known only from seen diagnostics or stored overrides under
/// "Other", and plugins (`plugin:<id>`, named by `names`) under "Plugins". Groups are by name.
/// Codes with stored `overrides` stay listed even when they were not seen in this session.
pub fn build_inspection_tree(
    languages: &[LanguageServers],
    seen: &[SeenCode],
    overrides: &[&ProfileOverrides],
    names: &BTreeMap<String, String>,
    i18n: &mut I18n,
) -> Vec<InspectionNode> {
    // `lsp:<producer>[:<code>]`; a plugin's producer is `plugin:<id>`.
    static LSP_ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^lsp:(plugin:[^:]+|[^:]+)(?::(.+))?$").expect("the pattern is valid"));
    let mut codes = KnownCodes::default();
    for code in seen {
        codes.add(code.clone());
    }
    let unseen = |producer: &str, code: &str| SeenCode {
        producer: producer.to_owned(),
        code: code.to_owned(),
        ..SeenCode::default()
    };
    for id in overrides.iter().flat_map(|profile| profile.keys()) {
        if let Some(found) = LSP_ID.captures(id) {
            match found.get(2) {
                Some(code) => codes.add(unseen(&found[1], code.as_str())),
                None => {
                    codes.producer(&found[1]);
                }
            }
        }
        let by_code = BUILTIN_INSPECTIONS.iter().filter(|builtin| builtin.by_code);
        let coded = by_code.filter_map(|builtin| Some((builtin, id.strip_prefix(builtin.id)?.strip_prefix(':')?)));
        for (builtin, code) in coded.take(1) {
            codes.add(unseen(builtin.producer, code));
        }
    }
    let served_by = |server: &str| -> Vec<&str> {
        let serving = languages.iter().filter(|language| language.servers.iter().any(|s| s == server));
        serving.map(|language| language.name.as_str()).collect()
    };

    let mut groups: Vec<InspectionNode> = Vec::new();
    let mut add = |group: &str, node: InspectionNode| {
        let at = groups.iter().position(|known| known.name == group).unwrap_or_else(|| {
            let (key, name) = (format!("group:{group}"), group.to_owned());
            groups.push(InspectionNode { key, name, ..InspectionNode::default() });
            groups.len() - 1
        });
        groups[at].children.push(node);
    };
    for language in languages {
        for server in &language.servers {
            let served = served_by(server).join(", ");
            add(&language.name, server_node(&language.name, server, codes.of(server), &served, None, i18n));
        }
    }
    for builtin in &BUILTIN_INSPECTIONS {
        let own_codes: &[SeenCode] = if builtin.by_code { codes.of(builtin.producer) } else { &[] };
        let node = InspectionNode {
            key: format!("{}/{}", builtin.group, builtin.id),
            id: Some(builtin.id.to_owned()),
            name: builtin.name.to_owned(),
            description: builtin.description.to_owned(),
            children: code_nodes(builtin.group, builtin.id, builtin.name, own_codes, i18n),
        };
        add(builtin.group, node);
    }
    for (producer, list) in &codes.0 {
        if !served_by(producer).is_empty() || builtin_by_producer(producer).is_some() {
            continue;
        }
        match producer.strip_prefix(PLUGIN_PREFIX) {
            Some(plugin_id) => {
                let name = names.get(producer).map_or(plugin_id, String::as_str);
                add(PLUGINS, server_node(PLUGINS, producer, list, "", Some(name), i18n));
            }
            None => add(OTHER, server_node(OTHER, producer, list, "", None, i18n)),
        }
    }
    for group in &mut groups {
        let count = group.children.len();
        group.description = format!("{count} inspection{}.", if count == 1 { "" } else { "s" });
    }
    groups.sort_by(|a, b| collate::compare(&a.name, &b.name));
    groups
}

/// The nodes whose name, id or description (or an ancestor's name) hold every word of the query;
/// ancestors of matches are kept.
pub fn filter_inspection_tree(nodes: &[InspectionNode], query: &str) -> Vec<InspectionNode> {
    fn walk(nodes: &[InspectionNode], trail: &str, terms: &[String]) -> Vec<InspectionNode> {
        let mut out = Vec::new();
        for node in nodes {
            let id = node.id.as_deref().unwrap_or_default();
            let hay = format!("{trail} {} {id} {}", node.name, node.description).to_lowercase();
            if terms.iter().all(|term| hay.contains(term.as_str())) {
                out.push(node.clone());
                continue;
            }
            let children = walk(&node.children, &format!("{trail} {}", node.name), terms);
            if !children.is_empty() {
                out.push(InspectionNode { children, ..node.clone() });
            }
        }
        out
    }
    let terms: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_owned).collect();
    if terms.is_empty() { nodes.to_vec() } else { walk(nodes, "", &terms) }
}

/// The inspection ids of a node and its descendants.
pub fn inspection_ids(node: &InspectionNode) -> Vec<&str> {
    node.id.as_deref().into_iter().chain(node.children.iter().flat_map(inspection_ids)).collect()
}

pub fn is_enabled(overrides: &ProfileOverrides, id: &str) -> bool {
    overrides.get(id).and_then(|entry| entry.enabled).unwrap_or_else(|| enabled_by_default(id))
}

/// The checkbox of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    On,
    Off,
    Mixed,
}

/// The checkbox of a node: an inspection that is off is off; otherwise mixed when some
/// descendant is off.
pub fn check_state(overrides: &ProfileOverrides, node: &InspectionNode) -> CheckState {
    if node.id.as_deref().is_some_and(|id| !is_enabled(overrides, id)) {
        return CheckState::Off;
    }
    let ids = inspection_ids(node);
    match ids.iter().filter(|id| is_enabled(overrides, id)).count() {
        on if on == ids.len() => CheckState::On,
        0 => CheckState::Off,
        _ => CheckState::Mixed,
    }
}

/// `overrides` with the entry of `id` changed; an entry left without anything is dropped.
fn with_override(
    overrides: &ProfileOverrides,
    id: &str,
    change: impl FnOnce(&mut InspectionOverride),
) -> ProfileOverrides {
    let mut next = overrides.clone();
    let entry = next.entry(id.to_owned()).or_default();
    change(entry);
    if *entry == InspectionOverride::default() {
        next.remove(id);
    }
    next
}

/// Turns a node and all its descendants on or off; an inspection back at its default has no
/// override.
pub fn set_enabled(overrides: &ProfileOverrides, node: &InspectionNode, enabled: bool) -> ProfileOverrides {
    inspection_ids(node).into_iter().fold(overrides.clone(), |acc, id| {
        with_override(&acc, id, |entry| entry.enabled = (enabled != enabled_by_default(id)).then_some(enabled))
    })
}

/// Sets the highlighting level of `id`; `None` restores the default ("As reported").
pub fn set_level(overrides: &ProfileOverrides, id: &str, level: Option<InspectionLevel>) -> ProfileOverrides {
    with_override(overrides, id, |entry| entry.severity = level)
}

/// Sets option `key` of inspection `id`; back at its default it is not stored.
pub fn set_option(overrides: &ProfileOverrides, id: &str, key: &str, value: bool) -> ProfileOverrides {
    let spec = builtin_by_id(id).and_then(|builtin| builtin.options.iter().find(|option| option.key == key));
    let fallback = spec.is_none_or(|option| option.default);
    with_override(overrides, id, |entry| {
        let mut options = entry.options.take().unwrap_or_default();
        if value == fallback {
            options.remove(key);
        } else {
            options.insert(key.to_owned(), value);
        }
        entry.options = Some(options).filter(|options| !options.is_empty());
    })
}

/// The value of option `key` of inspection `id` under a profile: its default when unset.
pub fn inspection_option(overrides: &ProfileOverrides, id: &str, key: &str) -> bool {
    let stored = overrides.get(id).and_then(|entry| entry.options.as_ref()).and_then(|options| options.get(key));
    let spec = builtin_by_id(id).and_then(|builtin| builtin.options.iter().find(|option| option.key == key));
    stored.copied().unwrap_or_else(|| spec.is_none_or(|option| option.default))
}

/// The keys of the node of `id` and of its ancestors in `nodes`, outermost first; `None` when
/// there is no such inspection.
pub fn path_to_inspection(nodes: &[InspectionNode], id: &str) -> Option<Vec<String>> {
    nodes.iter().find_map(|node| {
        if node.id.as_deref() == Some(id) {
            return Some(vec![node.key.clone()]);
        }
        let below = path_to_inspection(&node.children, id)?;
        Some(std::iter::once(node.key.clone()).chain(below).collect())
    })
}

/// The search texts of the page: the built-in checks and the levels (servers come with the
/// language list).
pub fn inspection_keywords() -> Vec<&'static str> {
    let fixed = [
        "inspections",
        "inspection profile",
        "code analysis",
        "lint",
        "linter",
        "diagnostics",
        "severity",
        "highlighting level",
        "language server",
        "copy profile",
        "rename profile",
        "delete profile",
        "spelling",
        "proofreading",
        "process code",
        "process literals",
        "process comments",
    ];
    let builtins = BUILTIN_INSPECTIONS.iter().map(|builtin| builtin.name);
    fixed.into_iter().chain(builtins).chain(INSPECTION_LEVELS.iter().map(|(_, label)| *label)).collect()
}

/// Editor › Inspections.
pub fn inspections_page() -> PageSpec {
    PageSpec::new("editor.inspections", "Inspections", 25).group("Editor").keywords(&inspection_keywords())
}

/// Editor › Natural Languages.
pub fn natural_languages_page() -> PageSpec {
    PageSpec::new("editor.naturalLanguages", "Natural Languages", 26).group("Editor").keywords(&[
        "natural languages",
        "English",
        "grammar",
        "proofreading",
    ])
}

/// Editor › Natural Languages › Spelling.
pub fn spelling_page() -> PageSpec {
    PageSpec::new("editor.naturalLanguages.spelling", "Spelling", 1).group("Editor").keywords(&[
        "spelling",
        "typo",
        "dictionary",
        "accepted words",
        "project dictionary",
        "application dictionary",
        "custom dictionaries",
        "hunspell",
    ])
}

// ---------------------------------------------------------------------------
// Profile management (gear menu)

/// Where copies of a profile are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileLevel {
    Ide,
    Project,
}

impl ProfileLevel {
    fn prefix(self) -> &'static str {
        match self {
            Self::Ide => "ide",
            Self::Project => "project",
        }
    }

    /// The id of this level's default profile.
    fn default_profile(self) -> &'static str {
        match self {
            Self::Ide => "default",
            Self::Project => "project",
        }
    }
}

/// Where the profile `id` is stored: its level, and its key among that level's copies (`None`
/// for the two defaults).
pub fn profile_location(id: &str) -> (ProfileLevel, Option<&str>) {
    match id.split_once(':').filter(|(_, key)| !key.is_empty()) {
        _ if id == "project" => (ProfileLevel::Project, None),
        Some(("ide", key)) => (ProfileLevel::Ide, Some(key)),
        Some(("project", key)) => (ProfileLevel::Project, Some(key)),
        _ => (ProfileLevel::Ide, None),
    }
}

pub fn custom_profile_id(level: ProfileLevel, key: &str) -> String {
    format!("{}:{key}", level.prefix())
}

fn copies(settings: &InspectionSettings, level: ProfileLevel) -> &CustomProfiles {
    match level {
        ProfileLevel::Ide => &settings.ide_profiles,
        ProfileLevel::Project => &settings.project_profiles,
    }
}

fn copies_mut(settings: &mut InspectionSettings, level: ProfileLevel) -> &mut CustomProfiles {
    match level {
        ProfileLevel::Ide => &mut settings.ide_profiles,
        ProfileLevel::Project => &mut settings.project_profiles,
    }
}

/// The overrides of the profile `id`; none when it does not exist.
pub fn overrides_of(settings: &InspectionSettings, id: &str) -> ProfileOverrides {
    let copy = |level: ProfileLevel, key: &str| copies(settings, level).get(key).map(|copy| copy.overrides.clone());
    match (id, profile_location(id)) {
        ("default", _) => settings.default.clone(),
        ("project", _) => settings.project.clone(),
        (_, (level, Some(key))) => copy(level, key).unwrap_or_default(),
        _ => ProfileOverrides::new(),
    }
}

/// A storage key not used yet for a copy named `name` among `existing`.
fn new_profile_key(name: &str, existing: &CustomProfiles) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "profile" } else { slug };
    if !existing.contains_key(slug) {
        return slug.to_owned();
    }
    (2u32..).map(|i| format!("{slug}-{i}")).find(|key| !existing.contains_key(key)).unwrap_or_default()
}

/// The settings with the overrides of the profile `id` replaced; unchanged when it does not exist.
pub fn with_profile_overrides(
    settings: &InspectionSettings,
    id: &str,
    overrides: ProfileOverrides,
) -> InspectionSettings {
    let mut next = settings.clone();
    match profile_location(id) {
        (ProfileLevel::Project, None) => next.project = overrides,
        (ProfileLevel::Ide, None) => next.default = overrides,
        (level, Some(key)) => {
            if let Some(copy) = copies_mut(&mut next, level).get_mut(key) {
                copy.overrides = overrides;
            }
        }
    }
    next
}

/// The settings with a copy of the profile `from` named `name` at `level`, made the chosen one.
pub fn copy_profile(settings: &InspectionSettings, from: &str, name: &str, level: ProfileLevel) -> InspectionSettings {
    let mut next = settings.clone();
    let key = new_profile_key(name, copies(settings, level));
    let copy = CustomInspectionProfile { name: name.trim().to_owned(), overrides: overrides_of(settings, from) };
    next.profile = custom_profile_id(level, &key);
    copies_mut(&mut next, level).insert(key, copy);
    next
}

/// Renames a copied profile (the two defaults keep their names).
pub fn rename_profile(settings: &InspectionSettings, id: &str, name: &str) -> InspectionSettings {
    let mut next = settings.clone();
    if let (level, Some(key)) = profile_location(id)
        && let Some(copy) = copies_mut(&mut next, level).get_mut(key)
    {
        copy.name = name.trim().to_owned();
    }
    next
}

/// Deletes a copied profile; when it was the chosen one, its level's default is.
pub fn delete_profile(settings: &InspectionSettings, id: &str) -> InspectionSettings {
    let mut next = settings.clone();
    if let (level, Some(key)) = profile_location(id) {
        copies_mut(&mut next, level).remove(key);
        if next.profile == id {
            next.profile = level.default_profile().to_owned();
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn languages() -> Vec<LanguageServers> {
        let language = |name: &str, servers: &[&str]| LanguageServers {
            name: name.to_owned(),
            servers: servers.iter().map(|&server| server.to_owned()).collect(),
        };
        vec![
            language("Python", &["basedpyright", "ruff"]),
            language("Markdown", &[]),
            language("TypeScript", &["typescript-language-server"]),
            language("JavaScript", &["typescript-language-server"]),
        ]
    }

    fn seen_code(producer: &str, code: &str, source: &str, sample: &str) -> SeenCode {
        SeenCode {
            producer: producer.to_owned(),
            code: code.to_owned(),
            source: source.to_owned(),
            sample: sample.to_owned(),
        }
    }

    fn seen() -> Vec<SeenCode> {
        vec![seen_code("ruff", "F401", "Ruff", "`os` imported but unused"), seen_code("custom-ls", "X1", "custom", "")]
    }

    fn overrides(stored: serde_json::Value) -> ProfileOverrides {
        serde_json::from_value(stored).expect("the backend takes the overrides")
    }

    fn tree_of(languages: &[LanguageServers], seen: &[SeenCode], stored: &ProfileOverrides) -> Vec<InspectionNode> {
        build_inspection_tree(languages, seen, &[stored], &BTreeMap::new(), &mut I18n::default())
    }

    fn tree() -> Vec<InspectionNode> {
        tree_of(&languages(), &seen(), &overrides(json!({ "lsp:ruff:E501": { "severity": "typo" } })))
    }

    fn find<'n>(nodes: &'n [InspectionNode], key: &str) -> Option<&'n InspectionNode> {
        nodes.iter().find_map(|node| if node.key == key { Some(node) } else { find(&node.children, key) })
    }

    fn node<'n>(nodes: &'n [InspectionNode], key: &str) -> &'n InspectionNode {
        find(nodes, key).unwrap_or_else(|| panic!("no node {key}"))
    }

    fn child_ids(node: &InspectionNode) -> Vec<&str> {
        node.children.iter().filter_map(|child| child.id.as_deref()).collect()
    }

    fn enabled(enabled: bool) -> InspectionOverride {
        InspectionOverride { enabled: Some(enabled), ..InspectionOverride::default() }
    }

    fn one(id: &str, entry: InspectionOverride) -> ProfileOverrides {
        ProfileOverrides::from([(id.to_owned(), entry)])
    }

    // plugin inspections › list each plugin by name under Plugins, from seen codes and stored overrides
    #[test]
    fn list_each_plugin_by_name_under_plugins_from_seen_codes_and_stored_overrides() {
        let stored = overrides(json!({
            "lsp:plugin:acme.license:old-code": { "enabled": false },
            "lsp:plugin:acme.other": { "severity": "warning" },
        }));
        let seen = [seen_code("plugin:acme.license", "missing-header", "", "No licence header")];
        let names = BTreeMap::from([("plugin:acme.license".to_owned(), "License Header".to_owned())]);
        let tree = build_inspection_tree(&[], &seen, &[&stored], &names, &mut I18n::default());
        let plugins = node(&tree, "group:Plugins");
        let listed: Vec<(Option<&str>, &str)> =
            plugins.children.iter().map(|child| (child.id.as_deref(), child.name.as_str())).collect();
        assert_eq!(
            listed,
            [(Some("lsp:plugin:acme.license"), "License Header"), (Some("lsp:plugin:acme.other"), "acme.other")]
        );
        assert_eq!(
            child_ids(&plugins.children[0]),
            ["lsp:plugin:acme.license:missing-header", "lsp:plugin:acme.license:old-code"]
        );
        assert!(plugins.children[0].description.contains("License Header plugin"));
        assert_eq!(find(&tree, "group:Other"), None);
    }

    // PHPStan inspections › list each identifier below PHPStan, from seen codes and stored overrides
    #[test]
    fn list_each_identifier_below_phpstan_from_seen_codes_and_stored_overrides() {
        let php = [LanguageServers { name: "PHP".to_owned(), servers: vec!["intelephense".to_owned()] }];
        let seen = [
            seen_code("phpstan", "return.type", "PHPStan", "Should return int but returns string."),
            seen_code("build", "E0308", "rustc", ""),
        ];
        let stored = overrides(json!({
            "builtin:phpstan:constant.notFound": { "enabled": false },
            "builtin:build:E1": { "enabled": false },
        }));
        let tree = tree_of(&php, &seen, &stored);
        assert_eq!(child_ids(node(&tree, "group:PHP")), ["lsp:intelephense", "builtin:phpstan"]);
        let phpstan = node(&tree, "PHP/builtin:phpstan");
        let listed: Vec<(Option<&str>, &str)> =
            phpstan.children.iter().map(|child| (child.id.as_deref(), child.name.as_str())).collect();
        assert_eq!(
            listed,
            [
                (Some("builtin:phpstan:constant.notFound"), "constant.notFound"),
                (Some("builtin:phpstan:return.type"), "return.type"),
            ]
        );
        assert_eq!(
            phpstan.children[1].description,
            "PHPStan problems with code return.type.\n\nExample: Should return int but returns string."
        );
        assert_eq!(
            path_to_inspection(&tree, "builtin:phpstan:return.type"),
            Some(vec![
                "group:PHP".to_owned(),
                "PHP/builtin:phpstan".to_owned(),
                "PHP/builtin:phpstan:return.type".to_owned(),
            ])
        );
        // Other built-in checks stay single inspections.
        assert!(node(&tree, "General/builtin:build").children.is_empty());
        let off = one("builtin:phpstan:constant.notFound", enabled(false));
        assert_eq!(check_state(&off, phpstan), CheckState::Mixed);
    }

    // inspections tree › groups servers by language with seen and configured codes, built-ins by category
    #[test]
    fn groups_servers_by_language_with_seen_and_configured_codes_built_ins_by_category() {
        let tree = tree();
        let names: Vec<&str> = tree.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Cargo",
                "General",
                "Java",
                "JavaScript",
                "Other",
                "PHP",
                "Proofreading",
                "Python",
                "SQL",
                "TypeScript",
                "Version control",
            ]
        );
        let python = node(&tree, "group:Python");
        assert_eq!(child_ids(python), ["lsp:basedpyright", "lsp:ruff"]);
        assert_eq!(child_ids(&python.children[1]), ["lsp:ruff:E501", "lsp:ruff:F401"]);
        assert!(node(&tree, "Python/lsp:ruff:F401").description.contains("imported but unused"));
        assert_eq!(child_ids(node(&tree, "group:Other")), ["lsp:custom-ls"]);
        assert!(child_ids(node(&tree, "group:General")).contains(&"builtin:file-references"));
        // A server of several languages appears under each, as the same inspection.
        let shared = node(&tree, "JavaScript/lsp:typescript-language-server");
        assert!(shared.description.contains("TypeScript, JavaScript"), "{}", shared.description);
    }

    // inspections tree › filters by name, code and group, keeping ancestors
    #[test]
    fn filters_by_name_code_and_group_keeping_ancestors() {
        let tree = tree();
        let out = filter_inspection_tree(&tree, "f401");
        assert_eq!(out.iter().map(|group| group.name.as_str()).collect::<Vec<_>>(), ["Python"]);
        assert_eq!(inspection_ids(&out[0]), ["lsp:ruff", "lsp:ruff:F401"]);
        assert_eq!(child_ids(&filter_inspection_tree(&tree, "python ruff")[0]), ["lsp:ruff"]);
        assert_eq!(filter_inspection_tree(&tree, "").len(), tree.len());
    }

    // inspections tree › derives tri-state checkboxes and toggles whole subtrees
    #[test]
    fn derives_tri_state_checkboxes_and_toggles_whole_subtrees() {
        let tree = tree();
        let python = node(&tree, "group:Python");
        let ruff = node(&tree, "Python/lsp:ruff");
        let mut o = set_enabled(&ProfileOverrides::new(), node(&tree, "Python/lsp:ruff:F401"), false);
        assert_eq!(o, one("lsp:ruff:F401", enabled(false)));
        assert_eq!(check_state(&o, ruff), CheckState::Mixed);
        assert_eq!(check_state(&o, python), CheckState::Mixed);
        o = set_enabled(&o, ruff, false);
        assert_eq!(check_state(&o, ruff), CheckState::Off);
        o = set_enabled(&o, python, true);
        assert!(o.is_empty());
        assert_eq!(check_state(&o, python), CheckState::On);
    }

    // inspections tree › keeps an inspection that is off by default off until turned on
    #[test]
    fn keeps_an_inspection_that_is_off_by_default_off_until_turned_on() {
        let tree = tree();
        let serial = node(&tree, &format!("Java/{JAVA_SERIAL_VERSION_INSPECTION}"));
        let none = ProfileOverrides::new();
        assert_eq!(check_state(&none, serial), CheckState::Off);
        let mut o = set_enabled(&none, serial, true);
        assert_eq!(o, one(JAVA_SERIAL_VERSION_INSPECTION, enabled(true)));
        assert_eq!(check_state(&o, serial), CheckState::On);
        o = set_enabled(&o, serial, false);
        assert!(o.is_empty());
        // Turning a whole group on or off only records what differs from each default.
        let java = node(&tree, "group:Java");
        assert_eq!(set_enabled(&none, java, true), one(JAVA_SERIAL_VERSION_INSPECTION, enabled(true)));
    }

    // inspections tree › keeps severity and enabled overrides independent
    #[test]
    fn keeps_severity_and_enabled_overrides_independent() {
        let tree = tree();
        let mut o = set_level(&ProfileOverrides::new(), "lsp:ruff", Some(InspectionLevel::Error));
        o = set_enabled(&o, node(&tree, "Python/lsp:ruff"), false);
        let both = InspectionOverride { severity: Some(InspectionLevel::Error), ..enabled(false) };
        assert_eq!(o.get("lsp:ruff"), Some(&both));
        o = set_level(&o, "lsp:ruff", None);
        assert_eq!(o.get("lsp:ruff"), Some(&enabled(false)));
    }

    // inspections tree › registers as Editor › Inspections
    #[test]
    fn registers_as_editor_inspections() {
        let page = inspections_page();
        assert_eq!((page.id, page.group), ("editor.inspections", Some("Editor")));
        assert!(page.keywords.contains(&"Weak Warning"));
    }

    fn base() -> InspectionSettings {
        let stored = json!({ "default": { "a": { "enabled": false } }, "project": { "b": { "severity": "error" } } });
        serde_json::from_value(stored).expect("the backend takes the settings")
    }

    // inspection profile management › copies at either level and selects the copy
    #[test]
    fn copies_at_either_level_and_selects_the_copy() {
        let base = base();
        let s1 = copy_profile(&base, "default", "Strict", ProfileLevel::Ide);
        assert_eq!(s1.profile, "ide:strict");
        let strict = CustomInspectionProfile { name: "Strict".to_owned(), overrides: one("a", enabled(false)) };
        assert_eq!(s1.ide_profiles.get("strict"), Some(&strict));
        assert_eq!(s1.default, base.default);
        let s2 = copy_profile(&s1, "project", "Team", ProfileLevel::Project);
        assert_eq!(s2.profile, "project:team");
        let error = InspectionOverride { severity: Some(InspectionLevel::Error), ..InspectionOverride::default() };
        assert_eq!(s2.project_profiles.get("team").map(|team| &team.overrides), Some(&one("b", error.clone())));
        // Copies are independent of their source.
        let s3 = with_profile_overrides(&s2, "project:team", ProfileOverrides::new());
        assert_eq!(s3.project_profiles.get("team").map(|team| team.overrides.len()), Some(0));
        assert_eq!(s3.project, one("b", error));
    }

    // inspection profile management › renames and deletes copies only
    #[test]
    fn renames_and_deletes_copies_only() {
        let s1 = copy_profile(&base(), "default", "Strict", ProfileLevel::Ide);
        let renamed = rename_profile(&s1, "ide:strict", " Lenient ");
        assert_eq!(renamed.ide_profiles.get("strict").map(|copy| copy.name.as_str()), Some("Lenient"));
        assert_eq!(rename_profile(&s1, "default", "X"), s1);
        let s2 = delete_profile(&s1, "ide:strict");
        assert!(s2.ide_profiles.is_empty());
        assert_eq!(s2.profile, "default");
        assert_eq!(delete_profile(&s1, "project"), s1);
        let team = copy_profile(&s1, "default", "Team", ProfileLevel::Project);
        assert_eq!(delete_profile(&team, "project:team").profile, "project");
    }

    // inspection profile management › registers Natural Languages › Spelling under Editor
    #[test]
    fn registers_natural_languages_spelling_under_editor() {
        let (natural, spelling) = (natural_languages_page(), spelling_page());
        assert_eq!(spelling.group, Some("Editor"));
        assert!(spelling.id.strip_prefix(natural.id).is_some_and(|rest| rest.starts_with('.')));
        assert!(spelling.keywords.contains(&"accepted words"));
    }

    // inspection options and focus (issue #42) › stores Typo options apart from defaults and keeps them through
    // validation
    #[test]
    fn stores_typo_options_apart_from_defaults_and_keeps_them_through_validation() {
        let off = set_option(&ProfileOverrides::new(), SPELLING_INSPECTION, "processCode", false);
        let stored = json!({ "builtin:spelling": { "options": { "processCode": false } } });
        assert_eq!(off, overrides(stored.clone()));
        let settings: InspectionSettings =
            serde_json::from_value(json!({ "default": stored })).expect("the backend takes the settings");
        assert_eq!(settings.default, off);
        assert!(!inspection_option(&off, SPELLING_INSPECTION, "processCode"));
        assert!(inspection_option(&off, SPELLING_INSPECTION, "processComments"));
        assert!(set_option(&off, SPELLING_INSPECTION, "processCode", true).is_empty());
    }

    // inspection options and focus (issue #42) › opens Settings on an inspection: page#id, ancestors to expand
    #[test]
    fn opens_settings_on_an_inspection_ancestors_to_expand() {
        let tree = tree_of(&[], &[], &ProfileOverrides::new());
        let path = path_to_inspection(&tree, SPELLING_INSPECTION);
        assert_eq!(path, Some(vec!["group:Proofreading".to_owned(), "Proofreading/builtin:spelling".to_owned()]));
        assert_eq!(path_to_inspection(&tree, "nope"), None);
    }

    // Not from a spec: a code reported by another tool than its server names the tool, groups
    // count their inspections, and a second copy of a name gets a key of its own.
    #[test]
    fn describes_codes_counts_groups_and_keys_copies() {
        let tree = tree();
        let ruff = node(&tree, "Python/lsp:ruff:F401");
        assert!(ruff.description.starts_with("ruff problems with code F401 (Ruff)."), "{}", ruff.description);
        assert_eq!(node(&tree, "group:Python").description, "2 inspections.");
        assert_eq!(node(&tree, "group:SQL").description, "1 inspection.");
        assert_eq!(producer_inspection_id("spelling"), SPELLING_INSPECTION);
        let s1 = copy_profile(&base(), "default", "Strict!", ProfileLevel::Ide);
        let s2 = copy_profile(&s1, "ide:strict", "strict", ProfileLevel::Ide);
        assert_eq!(s2.profile, "ide:strict-2");
        assert_eq!(overrides_of(&s2, "ide:strict-2"), one("a", enabled(false)));
        assert!(overrides_of(&s2, "ide:gone").is_empty());
        assert_eq!(profile_location("project:"), (ProfileLevel::Ide, None));
        assert_eq!(copy_profile(&base(), "default", "!!", ProfileLevel::Project).profile, "project:profile");
    }
}
