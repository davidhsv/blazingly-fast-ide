//! GraphQL schemas and queries: which files make up a schema (read from graphql-config files —
//! `.graphqlrc*`, `graphql.config.*` — or, without one, every GraphQL file of the project), the
//! endpoints a configuration declares and their environment variables, the GraphQL tool window's
//! tree, what Execute GraphQL sends, and the schema file written from an introspection response.
//!
//! Nothing here reads files or sends requests: the caller reads the files and hands their text in,
//! and an introspection or query response comes back as its body.

use crate::everywhere::open::{byte_at_utf16, utf16_len};
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::dirname;
use ide_core::text::{Diagnostic, Pos, Range, Severity};
use regex::{Captures, Regex};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub const SCHEMA_EXTENSIONS: [&str; 3] = ["graphql", "graphqls", "gql"];

/// graphql-config file names, most specific first.
pub const CONFIG_NAMES: [&str; 9] = [
    "graphql.config.yml",
    "graphql.config.yaml",
    "graphql.config.json",
    "graphql.config.js",
    "graphql.config.ts",
    ".graphqlrc",
    ".graphqlrc.yml",
    ".graphqlrc.yaml",
    ".graphqlrc.json",
];

/// Written by "Add Schema Configuration".
pub const CONFIG_TEMPLATE: &str = "schema: schema.graphql\ndocuments: '**/*.graphql'\n";

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
    };
}

re!(KEY_VALUE, r#"^("[^"]*"|'[^']*'|[^\s:'"][^:]*?)\s*:(?:\s+(.*))?$"#);
re!(ENV_VARIABLE, r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::([^}]*))?\}");
re!(DYNAMIC_CONFIG, r"\.(js|ts)$");
re!(JSON_START, r"^\s*\{");
re!(ENDPOINTS_SECTION, r#"^\s*["']?endpoints["']?\s*:"#);
re!(URL, r"(?i)^https?://");
re!(
    DEFINITION,
    concat!(
        r"(?-u:\b)(extend[ \t]+)?(type|interface|enum|input|union|scalar)[ \t]+([_A-Za-z][A-Za-z0-9_]*)",
        r"|(?-u:\b)directive[ \t]+@([_A-Za-z][A-Za-z0-9_]*)",
    )
);
re!(CODE_NOISE, r#"(?s)""".*?"""|"(?:[^"\\\n]|\\.)*"|#[^\n]*"#);
re!(NEXT_DEFINITION, r"\n[ \t]*(?:extend|type|interface|enum|input|union|scalar|directive|schema)(?-u:\b)");
re!(IMPLEMENTS, r"(?-u:\b)implements(?-u:\b)");
re!(NAME, r"[_A-Za-z][A-Za-z0-9_]*");
re!(FIELD_TYPE, r":\s*\[*\s*([_A-Za-z][A-Za-z0-9_]*)");
re!(DEFINITION_HEADER, r"^(query|mutation|subscription|fragment)(?-u:\b)\s*([_A-Za-z][A-Za-z0-9_]*)?");
re!(SPREAD, r"\.\.\.\s*([_A-Za-z][A-Za-z0-9_]*)");
re!(JSON_STRING, r#""(?:[^"\\]|\\.)*""#);
re!(GLOB_CHARS, r"[*?{\[]");

/// A GraphQL endpoint of a configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GqlEndpoint {
    pub name: String,
    pub url: String,
    /// Header names and values, in the order the configuration lists them.
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphQLConfig {
    /// Globs (relative to the config's folder) or URLs of introspected schemas.
    pub schema: Vec<String>,
    pub endpoints: Vec<GqlEndpoint>,
    /// A JS/TS config: its contents are code and are not evaluated.
    pub dynamic: bool,
    /// Environment variables it uses (`${NAME}`, `${NAME:default}`).
    pub variables: Vec<String>,
}

/// Definitions counted the way the schema discovery summary reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchemaStats {
    /// Object, interface, union and scalar types.
    pub types: u32,
    pub inputs: u32,
    pub enums: u32,
    pub directives: u32,
}

/// A named type a definition refers to, checked against the whole schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeReference {
    pub name: String,
    /// The type declaring the field (or the union / implementing type).
    pub owner: String,
    pub line: u32,
    /// UTF-16 column.
    pub col: u32,
}

/// What a schema file defines and refers to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaAnalysis {
    pub stats: SchemaStats,
    /// Type names it defines.
    pub defined: Vec<String>,
    pub references: Vec<TypeReference>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaFile {
    pub path: String,
    pub analysis: SchemaAnalysis,
}

/// One schema: a config file's, or the project-wide default one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaProject {
    pub key: String,
    /// The config file; `None` for the default schema.
    pub config_path: Option<String>,
    /// The folder its globs are relative to.
    pub root: String,
    /// The config's local schema pointers (globs or files), relative to `root`.
    pub pointers: Vec<String>,
    pub files: Vec<SchemaFile>,
    /// Schema pointers that are URLs (introspected remotely, not read here).
    pub remote: Vec<String>,
    pub endpoints: Vec<GqlEndpoint>,
    pub dynamic: bool,
    /// Environment variables its config uses.
    pub variables: Vec<String>,
}

pub fn is_schema_file(name: &str) -> bool {
    match name.rfind('.') {
        Some(dot) if dot > 0 => SCHEMA_EXTENSIONS.contains(&name[dot + 1..].to_lowercase().as_str()),
        _ => false,
    }
}

pub fn is_config_file(name: &str) -> bool {
    CONFIG_NAMES.contains(&name)
}

// --- config parsing ------------------------------------------------------------------------------

struct YamlLine {
    indent: usize,
    text: String,
}

/// `#` comments cut off, outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && prev.is_none_or(char::is_whitespace) => return &line[..i],
            None => {}
        }
        prev = Some(c);
    }
    line
}

fn unquote(s: &str) -> String {
    let mut chars = s.chars();
    match (chars.next(), chars.next_back()) {
        (Some(first), Some(last)) if (first == '"' || first == '\'') && last == first => s[1..s.len() - 1].to_owned(),
        _ => s.to_owned(),
    }
}

fn key_of(text: &str) -> Option<(String, String)> {
    let caps = KEY_VALUE.captures(text)?;
    let key = unquote(caps.get(1).map_or("", |m| m.as_str()));
    Some((key, caps.get(2).map_or("", |m| m.as_str().trim()).to_owned()))
}

fn scalar(text: &str) -> Value {
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let inner = inner.trim();
        if inner.is_empty() {
            return Value::Array(Vec::new());
        }
        return Value::Array(inner.split(',').map(|s| Value::String(unquote(s.trim()))).collect());
    }
    Value::String(unquote(text))
}

fn is_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

struct YamlParser {
    lines: Vec<YamlLine>,
    i: usize,
}

impl YamlParser {
    fn block(&mut self, indent: usize) -> Value {
        let Some(line) = self.lines.get(self.i).filter(|l| l.indent >= indent) else {
            return Value::String(String::new());
        };
        let at = line.indent;
        if is_item(&line.text) { Value::Array(self.list(at)) } else { self.map(at) }
    }

    fn list(&mut self, indent: usize) -> Vec<Value> {
        let mut out = Vec::new();
        while let Some(line) = self.lines.get(self.i).filter(|l| l.indent == indent && is_item(&l.text)) {
            let rest = line.text[1..].trim().to_owned();
            if rest.is_empty() {
                self.i += 1;
                out.push(self.block(indent + 1));
            } else if key_of(&rest).is_some() {
                // "- key: value" opens a map whose further keys are indented past the dash.
                self.lines[self.i] = YamlLine { indent: indent + 2, text: rest };
                out.push(self.map(indent + 2));
            } else {
                self.i += 1;
                out.push(scalar(&rest));
            }
        }
        out
    }

    fn map(&mut self, indent: usize) -> Value {
        let mut out = Map::new();
        while let Some(line) = self.lines.get(self.i).filter(|l| l.indent == indent) {
            let Some((key, value)) = key_of(&line.text) else { break };
            self.i += 1;
            let value = if value.is_empty() { self.block(indent + 1) } else { scalar(&value) };
            out.insert(key, value);
        }
        Value::Object(out)
    }
}

/// The YAML a graphql-config uses: nested maps, `-` lists, scalars and flow lists.
pub fn parse_yaml(text: &str) -> Value {
    let lines = text
        .split('\n')
        .map(|raw| raw.strip_suffix('\r').unwrap_or(raw))
        .filter_map(|raw| {
            let indent = raw.find(|c: char| !c.is_whitespace())?;
            let text = strip_comment(raw).trim().to_owned();
            (!text.is_empty() && text != "---").then(|| YamlLine { indent: utf16_len(&raw[..indent]), text })
        })
        .collect();
    YamlParser { lines, i: 0 }.block(0)
}

fn as_strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        Some(Value::Array(items)) => items.iter().flat_map(|item| as_strings(Some(item))).collect(),
        // `schema: { "https://api/graphql": { headers } }`: the pointer is the key.
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn as_headers(v: Option<&Value>) -> Vec<(String, String)> {
    match v {
        Some(Value::Object(map)) => {
            map.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect()
        }
        _ => Vec::new(),
    }
}

fn as_endpoints(v: Option<&Value>) -> Vec<GqlEndpoint> {
    let Some(Value::Object(map)) = v else { return Vec::new() };
    map.iter()
        .filter_map(|(name, e)| match e {
            Value::String(url) => Some(GqlEndpoint { name: name.clone(), url: url.clone(), headers: Vec::new() }),
            Value::Object(o) => o.get("url").and_then(Value::as_str).map(|url| GqlEndpoint {
                name: name.clone(),
                url: url.to_owned(),
                headers: as_headers(o.get("headers")),
            }),
            _ => None,
        })
        .collect()
}

/// Reads a graphql-config file (graphql-config v3+, and the legacy `schemaPath`).
pub fn parse_config(name: &str, text: &str) -> GraphQLConfig {
    let mut variables: Vec<String> = Vec::new();
    for caps in ENV_VARIABLE.captures_iter(text) {
        let name = caps.get(1).map_or("", |m| m.as_str());
        if !variables.iter().any(|v| v == name) {
            variables.push(name.to_owned());
        }
    }
    let empty = GraphQLConfig { variables: variables.clone(), ..GraphQLConfig::default() };
    if DYNAMIC_CONFIG.is_match(name) {
        return GraphQLConfig { dynamic: true, ..empty };
    }
    let data = if name.ends_with(".json") || JSON_START.is_match(text) {
        serde_json::from_str::<Value>(text).ok()
    } else {
        Some(parse_yaml(text))
    };
    let Some(Value::Object(o)) = data else { return empty };
    let mut schema = as_strings(o.get("schema"));
    schema.extend(as_strings(o.get("schemaPath")));
    let endpoints = as_endpoints(o.get("extensions").and_then(|e| e.get("endpoints")));
    GraphQLConfig { schema, endpoints, dynamic: false, variables }
}

/// `text` with its `${NAME}` / `${NAME:default}` replaced from `values` (else the default, else "").
pub fn expand_variables(text: &str, values: &HashMap<String, String>) -> String {
    ENV_VARIABLE
        .replace_all(text, |caps: &Captures| {
            let name = caps.get(1).map_or("", |m| m.as_str());
            values.get(name).cloned().or_else(|| caps.get(2).map(|m| m.as_str().to_owned())).unwrap_or_default()
        })
        .into_owned()
}

/// Variables `endpoint` uses that have neither a value nor a default: it cannot run before they
/// are set. An empty value counts as not set.
pub fn missing_variables(endpoint: &GqlEndpoint, values: &HashMap<String, String>) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    let texts = std::iter::once(endpoint.url.as_str()).chain(endpoint.headers.iter().map(|(_, v)| v.as_str()));
    for text in texts {
        for caps in ENV_VARIABLE.captures_iter(text) {
            let name = caps.get(1).map_or("", |m| m.as_str());
            let set = values.get(name).is_some_and(|v| !v.is_empty());
            if !set && caps.get(2).is_none() && !missing.iter().any(|m| m == name) {
                missing.push(name.to_owned());
            }
        }
    }
    missing
}

/// A response size as the result header shows it: "286 bytes", "1.7 kb".
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 { format!("{bytes} bytes") } else { format!("{:.1} kb", bytes as f64 / 1024.0) }
}

/// `endpoint` with its URL and headers expanded.
pub fn expand_endpoint(endpoint: &GqlEndpoint, values: &HashMap<String, String>) -> GqlEndpoint {
    GqlEndpoint {
        name: endpoint.name.clone(),
        url: expand_variables(&endpoint.url, values),
        headers: endpoint.headers.iter().map(|(k, v)| (k.clone(), expand_variables(v, values))).collect(),
    }
}

/// An endpoint of a config file and the 0-based line declaring it (for its ▶ gutter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointLine {
    pub endpoint: String,
    pub line: u32,
}

/// The lines declaring the endpoints of a config file (YAML or JSON).
pub fn endpoint_lines(name: &str, text: &str) -> Vec<EndpointLine> {
    let endpoints = parse_config(name, text).endpoints;
    if endpoints.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let Some(section) = lines.iter().position(|l| ENDPOINTS_SECTION.is_match(l)) else { return Vec::new() };
    endpoints
        .into_iter()
        .filter_map(|e| {
            let key = Regex::new(&format!(r#"^\s*["']?{}["']?\s*:"#, regex::escape(&e.name))).ok()?;
            let at = lines.iter().enumerate().position(|(i, l)| i > section && key.is_match(l))?;
            Some(EndpointLine { endpoint: e.name, line: u32::try_from(at).unwrap_or(u32::MAX) })
        })
        .collect()
}

// --- schema files --------------------------------------------------------------------------------

/// `**`, `*`, `?` and `{a,b}` globs, matched against a `/`-separated relative path; `None` for a
/// pattern that does not make a valid expression.
pub fn glob_to_regex(pattern: &str) -> Option<Regex> {
    let glob: Vec<char> = pattern.strip_prefix("./").unwrap_or(pattern).chars().collect();
    let mut re = String::from("^");
    let mut i = 0;
    while i < glob.len() {
        let c = glob[i];
        if c == '*' && glob.get(i + 1) == Some(&'*') {
            i += 1;
            if glob.get(i + 1) == Some(&'/') {
                i += 1;
                re.push_str("(?:.*/)?");
            } else {
                re.push_str(".*");
            }
        } else if c == '*' {
            re.push_str("[^/]*");
        } else if c == '?' {
            re.push_str("[^/]");
        } else if c == '{' {
            match (i..glob.len()).find(|&k| glob[k] == '}') {
                None => re.push_str(r"\{"),
                Some(end) => {
                    let inner: String = glob[i + 1..end].iter().collect();
                    let alternatives: Vec<String> = inner.split(',').map(regex::escape).collect();
                    re.push_str(&format!("(?:{})", alternatives.join("|")));
                    i = end;
                }
            }
        } else {
            re.push_str(&regex::escape(&c.to_string()));
        }
        i += 1;
    }
    re.push('$');
    Regex::new(&re).ok()
}

pub fn is_url(pointer: &str) -> bool {
    URL.is_match(pointer)
}

const BUILTIN_TYPES: [&str; 5] = ["String", "Int", "Float", "Boolean", "ID"];

/// Comments and strings blanked out with spaces, byte offsets kept (so positions still match
/// `text`).
fn code_only(text: &str) -> String {
    CODE_NOISE
        .replace_all(text, |caps: &Captures| {
            let whole = caps.get(0).map_or("", |m| m.as_str());
            let blank = |c: char| if c == '\n' { "\n".to_owned() } else { " ".repeat(c.len_utf8()) };
            whole.chars().map(blank).collect::<String>()
        })
        .into_owned()
}

/// Definitions and type references of a schema file (a light parse, enough for discovery).
pub fn analyze_schema(text: &str) -> SchemaAnalysis {
    let code = code_only(text);
    let mut line_starts = vec![0];
    line_starts.extend(code.match_indices('\n').map(|(i, _)| i + 1));
    let position = |offset: usize| {
        let line = line_starts.partition_point(|&start| start <= offset).saturating_sub(1);
        let col = utf16_len(&text[line_starts[line]..offset]);
        (u32::try_from(line).unwrap_or(u32::MAX), u32::try_from(col).unwrap_or(u32::MAX))
    };
    let mut stats = SchemaStats::default();
    let mut defined: Vec<String> = Vec::new();
    let mut references = Vec::new();
    let mut refer = |name: &str, owner: &str, offset: usize| {
        let (line, col) = position(offset);
        references.push(TypeReference { name: name.to_owned(), owner: owner.to_owned(), line, col });
    };
    for caps in DEFINITION.captures_iter(&code) {
        let Some(whole) = caps.get(0) else { continue };
        if caps.get(4).is_some() {
            stats.directives += 1;
            continue;
        }
        let (Some(kind), Some(name)) = (caps.get(2).map(|m| m.as_str()), caps.get(3).map(|m| m.as_str())) else {
            continue;
        };
        if caps.get(1).is_none() && !defined.iter().any(|d| d == name) {
            defined.push(name.to_owned());
            match kind {
                "input" => stats.inputs += 1,
                "enum" => stats.enums += 1,
                _ => stats.types += 1,
            }
        }
        let after = whole.end();
        let brace = code[after..].find('{').map(|b| after + b);
        let next_at = NEXT_DEFINITION.find(&code[after..]).map_or(code.len(), |m| after + m.start());
        let header_end = match brace {
            Some(brace) if brace < next_at => brace,
            _ => next_at,
        };
        let header = &code[after..header_end];
        if kind == "union" || IMPLEMENTS.is_match(header) {
            let list = if kind == "union" {
                &header[header.find('=').map_or(0, |at| at + 1)..]
            } else {
                header.find("implements").map_or(header, |at| &header[at + "implements".len()..])
            };
            let list_at = after + header.len() - list.len();
            for r in NAME.find_iter(list) {
                refer(r.as_str(), name, list_at + r.start());
            }
        }
        if matches!(kind, "enum" | "scalar" | "union") || !code[header_end..].starts_with('{') {
            continue;
        }
        // Field and argument types: the name after each `:` (past `[`) in the block.
        let Some(close) = code[header_end..].find('}').map(|c| header_end + c) else { continue };
        let start = header_end + 1;
        for r in FIELD_TYPE.captures_iter(&code[start..close]) {
            if let Some(ty) = r.get(1) {
                refer(ty.as_str(), name, start + ty.start());
            }
        }
    }
    SchemaAnalysis { stats, defined, references }
}

/// A problem of a schema file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaProblem {
    pub path: String,
    pub diagnostic: Diagnostic,
}

/// Named types used but defined nowhere in the schema (the "Schema errors").
pub fn unresolved_types(project: &SchemaProject, i18n: &mut I18n) -> Vec<SchemaProblem> {
    let known: HashSet<&str> = BUILTIN_TYPES
        .into_iter()
        .chain(project.files.iter().flat_map(|f| f.analysis.defined.iter().map(String::as_str)))
        .collect();
    let mut out = Vec::new();
    for f in &project.files {
        for r in f.analysis.references.iter().filter(|r| !known.contains(r.name.as_str())) {
            let message = i18n.tx_built(
                &["The field type '", "' is not present when resolving type '", "'"],
                &[Param::from(r.name.as_str()), Param::from(r.owner.as_str())],
            );
            let len = u32::try_from(utf16_len(&r.name)).unwrap_or(u32::MAX);
            out.push(SchemaProblem {
                path: f.path.clone(),
                diagnostic: Diagnostic {
                    range: Range { start: Pos::new(r.line, r.col), end: Pos::new(r.line, r.col + len) },
                    severity: Severity::Error,
                    message,
                    source: Some("graphql".to_owned()),
                    code: None,
                    unnecessary: false,
                    deprecated: false,
                    server: None,
                },
            });
        }
    }
    out
}

pub fn schema_stats(project: &SchemaProject) -> SchemaStats {
    project.files.iter().fold(SchemaStats::default(), |mut total, f| {
        total.types += f.analysis.stats.types;
        total.inputs += f.analysis.stats.inputs;
        total.enums += f.analysis.stats.enums;
        total.directives += f.analysis.stats.directives;
        total
    })
}

fn relative_to<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    if root == "/" {
        return Some(path.strip_prefix('/').unwrap_or(path));
    }
    path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/'))
}

/// A config file and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: String,
    pub config: GraphQLConfig,
}

/// The schemas of a project: one per config file, its `schema` globs picking files under the
/// config's folder; with no config, every GraphQL file is in the default schema.
pub fn discover_schemas(root: &str, configs: &[ConfigFile], files: &[SchemaFile]) -> Vec<SchemaProject> {
    let sorted = |mut files: Vec<SchemaFile>| {
        files.sort_by(|a, b| collate::compare(&a.path, &b.path));
        files
    };
    if configs.is_empty() {
        return vec![SchemaProject {
            key: "default".to_owned(),
            config_path: None,
            root: root.to_owned(),
            files: sorted(files.to_vec()),
            ..SchemaProject::default()
        }];
    }
    let mut configs: Vec<&ConfigFile> = configs.iter().collect();
    configs.sort_by(|a, b| collate::compare(&a.path, &b.path));
    configs
        .into_iter()
        .map(|ConfigFile { path, config }| {
            let dir = match path.rfind('/') {
                Some(at) if at > 0 => path[..at].to_owned(),
                _ => "/".to_owned(),
            };
            let pointers: Vec<String> = config.schema.iter().filter(|p| !is_url(p)).cloned().collect();
            let globs: Vec<Regex> = pointers.iter().filter_map(|p| glob_to_regex(p)).collect();
            let picked = files
                .iter()
                .filter(|f| relative_to(&dir, &f.path).is_some_and(|rel| globs.iter().any(|g| g.is_match(rel))))
                .cloned()
                .collect();
            SchemaProject {
                key: path.clone(),
                config_path: Some(path.clone()),
                root: dir,
                pointers,
                files: sorted(picked),
                remote: config.schema.iter().filter(|p| is_url(p)).cloned().collect(),
                endpoints: config.endpoints.clone(),
                dynamic: config.dynamic,
                variables: config.variables.clone(),
            }
        })
        .collect()
}

/// The schema of a file: the one listing it, else the closest config folder holding it.
pub fn schema_of<'a>(projects: &'a [SchemaProject], path: &str) -> Option<&'a SchemaProject> {
    if let Some(listed) = projects.iter().find(|p| p.files.iter().any(|f| f.path == path)) {
        return Some(listed);
    }
    let dir = dirname(path);
    let holds = |p: &&SchemaProject| {
        dir == p.root || dir.strip_prefix(p.root.as_str()).is_some_and(|rest| rest.starts_with('/'))
    };
    // The first of the closest ones.
    projects.iter().rev().filter(|p| p.config_path.is_some()).filter(holds).max_by_key(|p| p.root.len())
}

// --- tool window tree ----------------------------------------------------------------------------

/// A node of the GraphQL tool window's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaNode {
    Project { key: String, project: SchemaProject },
    Summary { key: String, project: SchemaProject },
    File { key: String, file: SchemaFile },
    Remote { key: String, url: String },
    Errors { key: String, project: SchemaProject },
    Error { key: String, path: String, diagnostic: Diagnostic },
    NoErrors { key: String },
    Endpoints { key: String, project: SchemaProject },
    Endpoint { key: String, endpoint: GqlEndpoint },
    NoEndpoints { key: String, project: SchemaProject },
}

impl SchemaNode {
    pub fn key(&self) -> &str {
        match self {
            Self::Project { key, .. }
            | Self::Summary { key, .. }
            | Self::File { key, .. }
            | Self::Remote { key, .. }
            | Self::Errors { key, .. }
            | Self::Error { key, .. }
            | Self::NoErrors { key }
            | Self::Endpoints { key, .. }
            | Self::Endpoint { key, .. }
            | Self::NoEndpoints { key, .. } => key,
        }
    }

    /// The node's kind, as the tree styles it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Summary { .. } => "summary",
            Self::File { .. } => "file",
            Self::Remote { .. } => "remote",
            Self::Errors { .. } => "errors",
            Self::Error { .. } => "error",
            Self::NoErrors { .. } => "noErrors",
            Self::Endpoints { .. } => "endpoints",
            Self::Endpoint { .. } => "endpoint",
            Self::NoEndpoints { .. } => "noEndpoints",
        }
    }
}

pub fn project_nodes(projects: &[SchemaProject]) -> Vec<SchemaNode> {
    projects.iter().map(|p| SchemaNode::Project { key: p.key.clone(), project: p.clone() }).collect()
}

/// A schema's errors: unresolved types found by discovery, and the errors and warnings the
/// language server reports for its open files (the same problem only once).
pub fn schema_problems(
    project: &SchemaProject,
    diagnostics_for: &dyn Fn(&str) -> Vec<Diagnostic>,
    i18n: &mut I18n,
) -> Vec<SchemaProblem> {
    let mut problems = unresolved_types(project, i18n);
    let at = |path: &str, d: &Diagnostic| format!("{path}:{}:{}", d.range.start.line, d.range.start.col);
    let seen: HashSet<String> = problems.iter().map(|p| at(&p.path, &p.diagnostic)).collect();
    for f in &project.files {
        for diagnostic in diagnostics_for(&f.path) {
            let shown = matches!(diagnostic.severity, Severity::Error | Severity::Warning);
            if shown && !seen.contains(&at(&f.path, &diagnostic)) {
                problems.push(SchemaProblem { path: f.path.clone(), diagnostic });
            }
        }
    }
    problems
}

/// The children of a node; `None` for a leaf.
pub fn child_nodes(
    node: &SchemaNode,
    diagnostics_for: &dyn Fn(&str) -> Vec<Diagnostic>,
    i18n: &mut I18n,
) -> Option<Vec<SchemaNode>> {
    let k = node.key();
    Some(match node {
        SchemaNode::Project { project, .. } => vec![
            SchemaNode::Summary { key: format!("{k}#summary"), project: project.clone() },
            SchemaNode::Errors { key: format!("{k}#errors"), project: project.clone() },
            SchemaNode::Endpoints { key: format!("{k}#endpoints"), project: project.clone() },
        ],
        SchemaNode::Summary { project, .. } => project
            .files
            .iter()
            .map(|file| SchemaNode::File { key: format!("{k}:{}", file.path), file: file.clone() })
            .chain(project.remote.iter().map(|url| SchemaNode::Remote { key: format!("{k}:{url}"), url: url.clone() }))
            .collect(),
        SchemaNode::Errors { project, .. } => {
            let problems = schema_problems(project, diagnostics_for, i18n);
            if problems.is_empty() {
                return Some(vec![SchemaNode::NoErrors { key: format!("{k}:none") }]);
            }
            problems
                .into_iter()
                .enumerate()
                .map(|(i, SchemaProblem { path, diagnostic: d })| SchemaNode::Error {
                    key: format!("{k}:{i}:{path}:{}:{}", d.range.start.line, d.range.start.col),
                    path,
                    diagnostic: d,
                })
                .collect()
        }
        SchemaNode::Endpoints { project, .. } => {
            if project.endpoints.is_empty() {
                return Some(vec![SchemaNode::NoEndpoints { key: format!("{k}:none"), project: project.clone() }]);
            }
            project
                .endpoints
                .iter()
                .map(|e| SchemaNode::Endpoint { key: format!("{k}:{}", e.name), endpoint: e.clone() })
                .collect()
        }
        _ => return None,
    })
}

// --- query execution -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    Operation,
    Fragment,
    Other,
}

/// A top-level definition of an executable document; byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Definition {
    kind: DefinitionKind,
    name: Option<String>,
    from: usize,
    to: usize,
}

/// Top-level definitions of `doc`, found by brace matching outside comments and strings.
fn top_level_definitions(code: &str) -> Vec<Definition> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let from = i;
        let Some(open) = code[i..].find('{').map(|o| i + o) else { break };
        let mut depth = 0;
        let mut j = open;
        while j < bytes.len() {
            if bytes[j] == b'{' {
                depth += 1;
            } else if bytes[j] == b'}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        let to = (j + 1).min(bytes.len());
        let header = code[from..open].trim();
        let caps = DEFINITION_HEADER.captures(header);
        let keyword = caps.as_ref().and_then(|c| c.get(1)).map(|m| m.as_str());
        let kind = match keyword {
            _ if header.is_empty() => DefinitionKind::Operation,
            Some("fragment") => DefinitionKind::Fragment,
            Some(_) => DefinitionKind::Operation,
            None => DefinitionKind::Other,
        };
        let name = caps.as_ref().and_then(|c| c.get(2)).map(|m| m.as_str().to_owned());
        out.push(Definition { kind, name, from, to });
        i = to;
    }
    out
}

/// A part of a document, as UTF-16 offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub from: usize,
    pub to: usize,
}

/// What Execute GraphQL sends and the parts of the document it comes from (the rest is greyed out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryContext {
    pub text: String,
    pub ranges: Vec<TextRange>,
}

/// What Execute GraphQL sends: the selection; else the operation around the caret with the
/// fragments it uses; else the whole document.
pub fn query_context(doc: &str, selection: Option<TextRange>) -> QueryContext {
    let byte = |units: usize| byte_at_utf16(doc, units);
    if let Some(sel) = selection.filter(|s| s.to > s.from) {
        return QueryContext { text: doc[byte(sel.from)..byte(sel.to)].trim().to_owned(), ranges: vec![sel] };
    }
    let whole = QueryContext { text: doc.trim().to_owned(), ranges: vec![TextRange { from: 0, to: utf16_len(doc) }] };
    let code = code_only(doc);
    let defs = top_level_definitions(&code);
    let Some(caret) = selection.map(|s| byte(s.from)) else { return whole };
    let Some(op) = defs.iter().position(|d| d.kind == DefinitionKind::Operation && d.from <= caret && caret <= d.to)
    else {
        return whole;
    };
    let fragments: HashMap<&str, usize> = defs
        .iter()
        .enumerate()
        .filter(|(_, d)| d.kind == DefinitionKind::Fragment)
        .filter_map(|(i, d)| d.name.as_deref().map(|n| (n, i)))
        .collect();
    let mut used: HashSet<&str> = HashSet::new();
    // Depth-first, as the spreads are met.
    fn visit<'a>(
        code: &'a str,
        defs: &'a [Definition],
        fragments: &HashMap<&'a str, usize>,
        used: &mut HashSet<&'a str>,
        at: usize,
    ) {
        for s in SPREAD.captures_iter(&code[defs[at].from..defs[at].to]) {
            let Some(name) = s.get(1).map(|m| m.as_str()) else { continue };
            let Some(&fragment) = fragments.get(name) else { continue };
            if name == "on" || used.contains(name) {
                continue;
            }
            used.insert(name);
            visit(code, defs, fragments, used, fragment);
        }
    }
    visit(&code, &defs, &fragments, &mut used, op);
    let sent: Vec<&Definition> = defs
        .iter()
        .enumerate()
        .filter(|(i, d)| {
            *i == op || (d.kind == DefinitionKind::Fragment && used.contains(d.name.as_deref().unwrap_or("")))
        })
        .map(|(_, d)| d)
        .collect();
    QueryContext {
        text: sent.iter().map(|d| &doc[d.from..d.to]).collect::<Vec<_>>().join("\n\n"),
        ranges: sent
            .iter()
            .map(|d| TextRange { from: utf16_len(&doc[..d.from]), to: utf16_len(&doc[..d.to]) })
            .collect(),
    }
}

pub fn query_text(doc: &str, selection: Option<TextRange>) -> String {
    query_context(doc, selection).text
}

/// The parts of `[0, length)` outside `ranges`.
pub fn outside_ranges(length: usize, ranges: &[TextRange]) -> Vec<TextRange> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.from);
    let mut out = Vec::new();
    let mut pos = 0;
    for r in sorted {
        if r.from > pos {
            out.push(TextRange { from: pos, to: r.from });
        }
        pos = pos.max(r.to);
    }
    if pos < length {
        out.push(TextRange { from: pos, to: length });
    }
    out
}

/// Errors in a GraphQL response body (`errors` array), 0 when it is not a GraphQL response.
pub fn response_error_count(body: Option<&str>) -> usize {
    body.and_then(|b| serde_json::from_str::<Value>(b).ok())
        .and_then(|v| v.get("errors").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0)
}

/// JSON the HTTP client sends as written: it expands `{{name}}`, which a GraphQL selection
/// (`{ a { b }}`) can contain, so braces inside strings are escaped.
pub fn json_without_templates(value: &Value) -> String {
    let json = serde_json::to_string(value).unwrap_or_default();
    JSON_STRING
        .replace_all(&json, |caps: &Captures| {
            caps.get(0).map_or("", |m| m.as_str()).replace('{', "\\u007b").replace('}', "\\u007d")
        })
        .into_owned()
}

/// An `.http` request posting `query` (and `variables`) to `endpoint`, for the HTTP client to
/// execute.
pub fn http_request_text(endpoint: &GqlEndpoint, query: &str, variables: Option<&Map<String, Value>>) -> String {
    let mut headers: Vec<(String, String)> = vec![("Content-Type".to_owned(), "application/json".to_owned())];
    for (k, v) in &endpoint.headers {
        match headers.iter_mut().find(|(name, _)| name == k) {
            Some(existing) => existing.1.clone_from(v),
            None => headers.push((k.clone(), v.clone())),
        }
    }
    let mut body = Map::new();
    body.insert("query".to_owned(), Value::String(query.to_owned()));
    if let Some(variables) = variables {
        body.insert("variables".to_owned(), Value::Object(variables.clone()));
    }
    let mut lines = vec![format!("POST {}", endpoint.url)];
    lines.extend(headers.iter().map(|(k, v)| format!("{k}: {v}")));
    lines.push(String::new());
    lines.push(json_without_templates(&Value::Object(body)));
    lines.push(String::new());
    lines.join("\n")
}

/// The variables editor's text: empty, or a JSON object.
#[derive(Clone, Debug, PartialEq)]
pub enum Variables {
    Parsed(Option<Map<String, Value>>),
    Error(String),
}

pub fn parse_variables(text: &str) -> Variables {
    if text.trim().is_empty() {
        return Variables::Parsed(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Variables::Parsed(Some(map)),
        Ok(_) => Variables::Error("Variables must be a JSON object".to_owned()),
        Err(e) => Variables::Error(e.to_string()),
    }
}

/// Pretty-prints a JSON body; other bodies are shown as received.
pub fn format_body(body: Option<&str>) -> String {
    let Some(body) = body else { return String::new() };
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| body.to_owned())
}

// --- introspection -------------------------------------------------------------------------------

/// graphql-js `getIntrospectionQuery()` with descriptions.
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives { name description locations args { ...InputValue } }
  }
}
fragment FullType on __Type {
  kind name description
  fields(includeDeprecated: true) {
    name description args { ...InputValue } type { ...TypeRef } isDeprecated deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) { name description isDeprecated deprecationReason }
  possibleTypes { ...TypeRef }
}
fragment InputValue on __InputValue { name description type { ...TypeRef } defaultValue }
fragment TypeRef on __Type {
  kind name ofType { kind name ofType { kind name ofType { kind name ofType {
    kind name ofType { kind name ofType { kind name ofType { kind name } } } } } } }
}";

/// First line of a schema file written by "Get GraphQL Schema from Endpoint".
pub const GENERATED_HEADER: &str = "# This file was generated. Do not edit manually.";

/// Schema files are indented by four spaces.
const INDENT: &str = "    ";

/// Types are printed grouped by kind, each group by name.
const KIND_ORDER: [&str; 6] = ["INTERFACE", "UNION", "OBJECT", "ENUM", "SCALAR", "INPUT_OBJECT"];
const BUILTIN_DIRECTIVES: [&str; 4] = ["include", "skip", "deprecated", "specifiedBy"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypeRef {
    kind: String,
    name: Option<String>,
    of_type: Option<Box<TypeRef>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputValue {
    name: String,
    description: Option<String>,
    #[serde(rename = "type")]
    ty: TypeRef,
    default_value: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldDef {
    name: String,
    description: Option<String>,
    args: Option<Vec<InputValue>>,
    #[serde(rename = "type")]
    ty: TypeRef,
    is_deprecated: Option<bool>,
    deprecation_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnumValueDef {
    name: String,
    description: Option<String>,
    is_deprecated: Option<bool>,
    deprecation_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullType {
    kind: String,
    name: String,
    description: Option<String>,
    fields: Option<Vec<FieldDef>>,
    input_fields: Option<Vec<InputValue>>,
    interfaces: Option<Vec<TypeRef>>,
    enum_values: Option<Vec<EnumValueDef>>,
    possible_types: Option<Vec<TypeRef>>,
}

#[derive(Deserialize)]
struct RootName {
    name: String,
}

#[derive(Deserialize)]
struct DirectiveDef {
    name: String,
    description: Option<String>,
    locations: Vec<String>,
    args: Option<Vec<InputValue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntrospectionSchema {
    query_type: Option<RootName>,
    mutation_type: Option<RootName>,
    subscription_type: Option<RootName>,
    types: Vec<FullType>,
    directives: Option<Vec<DirectiveDef>>,
}

#[derive(Deserialize)]
struct IntrospectionData {
    #[serde(rename = "__schema")]
    schema: Option<IntrospectionSchema>,
}

#[derive(Deserialize)]
struct ResponseError {
    message: String,
}

#[derive(Deserialize)]
struct IntrospectionResponse {
    data: Option<IntrospectionData>,
    errors: Option<Vec<ResponseError>>,
}

fn field_name(f: &FieldDef) -> &str {
    &f.name
}

fn input_name(v: &InputValue) -> &str {
    &v.name
}

fn directive_name(d: &DirectiveDef) -> &str {
    &d.name
}

fn by_name<T>(list: Option<&[T]>, name: fn(&T) -> &str) -> Vec<&T> {
    let mut sorted: Vec<&T> = list.unwrap_or_default().iter().collect();
    sorted.sort_by(|a, b| collate::compare_code_units(name(a), name(b)));
    sorted
}

fn print_type_ref(t: &TypeRef) -> String {
    match (t.kind.as_str(), &t.of_type) {
        ("NON_NULL", Some(inner)) => format!("{}!", print_type_ref(inner)),
        ("LIST", Some(inner)) => format!("[{}]", print_type_ref(inner)),
        _ => t.name.clone().unwrap_or_default(),
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn print_description(text: Option<&str>, indent: &str) -> String {
    let Some(text) = text.filter(|t| !t.is_empty()) else { return String::new() };
    if !text.contains('\n') {
        return format!("{indent}{}\n", json_string(text));
    }
    let body = text
        .replace("\"\"\"", "\\\"\"\"")
        .split('\n')
        .map(|l| if l.is_empty() { String::new() } else { format!("{indent}{l}") })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{indent}\"\"\"\n{body}\n{indent}\"\"\"\n")
}

fn print_deprecated(is_deprecated: Option<bool>, reason: Option<&str>) -> String {
    if is_deprecated != Some(true) {
        return String::new();
    }
    match reason.filter(|r| !r.is_empty() && *r != "No longer supported") {
        Some(reason) => format!(" @deprecated(reason: {})", json_string(reason)),
        None => " @deprecated".to_owned(),
    }
}

fn print_input_value(v: &InputValue) -> String {
    let default = v.default_value.as_ref().map(|d| format!(" = {d}")).unwrap_or_default();
    format!("{}: {}{default}", v.name, print_type_ref(&v.ty))
}

fn print_args(list: Option<&[InputValue]>) -> String {
    match list {
        Some(list) if !list.is_empty() => {
            format!("({})", list.iter().map(print_input_value).collect::<Vec<_>>().join(", "))
        }
        _ => String::new(),
    }
}

fn print_block(lines: &[String]) -> String {
    if lines.is_empty() { String::new() } else { format!(" {{\n{}\n}}", lines.join("\n")) }
}

fn print_type(t: &FullType) -> String {
    let head = print_description(t.description.as_deref(), "");
    match t.kind.as_str() {
        "SCALAR" => format!("{head}scalar {}", t.name),
        "OBJECT" | "INTERFACE" => {
            let implements = match &t.interfaces {
                Some(list) if !list.is_empty() => {
                    format!(" implements {}", list.iter().map(print_type_ref).collect::<Vec<_>>().join(" & "))
                }
                _ => String::new(),
            };
            let fields: Vec<String> = by_name(t.fields.as_deref(), field_name)
                .into_iter()
                .map(|f| {
                    format!(
                        "{}{INDENT}{}{}: {}{}",
                        print_description(f.description.as_deref(), INDENT),
                        f.name,
                        print_args(f.args.as_deref()),
                        print_type_ref(&f.ty),
                        print_deprecated(f.is_deprecated, f.deprecation_reason.as_deref())
                    )
                })
                .collect();
            let keyword = if t.kind == "OBJECT" { "type" } else { "interface" };
            format!("{head}{keyword} {}{implements}{}", t.name, print_block(&fields))
        }
        "UNION" => {
            let members = t.possible_types.iter().flatten().map(print_type_ref).collect::<Vec<_>>().join(" | ");
            format!("{head}union {} = {members}", t.name)
        }
        "ENUM" => {
            let values: Vec<String> = t
                .enum_values
                .iter()
                .flatten()
                .map(|v| {
                    format!(
                        "{}{INDENT}{}{}",
                        print_description(v.description.as_deref(), INDENT),
                        v.name,
                        print_deprecated(v.is_deprecated, v.deprecation_reason.as_deref())
                    )
                })
                .collect();
            format!("{head}enum {}{}", t.name, print_block(&values))
        }
        "INPUT_OBJECT" => {
            let fields: Vec<String> = by_name(t.input_fields.as_deref(), input_name)
                .into_iter()
                .map(|v| {
                    format!("{}{INDENT}{}", print_description(v.description.as_deref(), INDENT), print_input_value(v))
                })
                .collect();
            format!("{head}input {}{}", t.name, print_block(&fields))
        }
        _ => String::new(),
    }
}

/// The schema file "Get GraphQL Schema from Endpoint" writes for an introspection response body
/// (`{"data":{"__schema":…}}`). Fails with the response's error messages when there is no schema.
pub fn introspection_to_sdl(body: &str) -> Result<String, String> {
    let response: IntrospectionResponse = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(schema) = response.data.and_then(|d| d.schema) else {
        let messages = response.errors.unwrap_or_default().into_iter().map(|e| e.message).collect::<Vec<_>>();
        let message = messages.join("\n");
        return Err(if message.is_empty() { "The response has no __schema".to_owned() } else { message });
    };
    let roots = [
        ("query", &schema.query_type),
        ("mutation", &schema.mutation_type),
        ("subscription", &schema.subscription_type),
    ];
    let ops: Vec<String> = roots
        .iter()
        .filter_map(|(op, root)| {
            root.as_ref().filter(|r| !r.name.is_empty()).map(|r| format!("{INDENT}{op}: {}", r.name))
        })
        .collect();
    let mut parts = vec![format!("schema {{\n{}\n}}", ops.join("\n"))];
    for d in by_name(schema.directives.as_deref(), directive_name) {
        if BUILTIN_DIRECTIVES.contains(&d.name.as_str()) {
            continue;
        }
        parts.push(format!(
            "{}directive @{}{} on {}",
            print_description(d.description.as_deref(), ""),
            d.name,
            print_args(d.args.as_deref()),
            d.locations.join(" | ")
        ));
    }
    let types: Vec<&FullType> = schema
        .types
        .iter()
        .filter(|t| !t.name.starts_with("__") && !(t.kind == "SCALAR" && BUILTIN_TYPES.contains(&t.name.as_str())))
        .collect();
    for kind in KIND_ORDER {
        let mut group: Vec<&FullType> = types.iter().copied().filter(|t| t.kind == kind).collect();
        group.sort_by(|a, b| collate::compare_code_units(&a.name, &b.name));
        parts.extend(group.into_iter().map(print_type));
    }
    parts.retain(|p| !p.is_empty());
    Ok(format!("{GENERATED_HEADER}\n\n{}\n", parts.join("\n\n")))
}

/// Where "Get GraphQL Schema from Endpoint" writes: the config's first schema pointer naming a
/// file, else `schema.graphql` next to the config.
pub fn introspection_target(project: &SchemaProject) -> String {
    let file = project.pointers.iter().find(|p| !GLOB_CHARS.is_match(p)).map_or("schema.graphql", String::as_str);
    let base = if project.root == "/" { "" } else { project.root.as_str() };
    format!("{base}/{}", file.strip_prefix("./").unwrap_or(file))
}

// --- environment variables dialog ----------------------------------------------------------------

/// The values of `names` as `NAME=value` lines, for the clipboard.
pub fn format_environment(names: &[&str], values: &HashMap<String, String>) -> String {
    names.iter().map(|n| format!("{n}={}", values.get(*n).map_or("", String::as_str))).collect::<Vec<_>>().join("\n")
}

/// Values of `names` found in pasted `NAME=value` lines; other lines are ignored.
pub fn parse_environment(text: &str, names: &[&str]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        let Some(at) = line.find('=').filter(|&at| at > 0) else { continue };
        let name = line[..at].trim();
        if names.contains(&name) {
            out.insert(name.to_owned(), line[at + 1..].trim().to_owned());
        }
    }
    out
}

// --- replacing a file's text in its editor -------------------------------------------------------

/// What replacing a file's text in its editor comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceOutcome {
    /// Replace the whole text of the file's editor.
    Replace { path: String, text: String },
    /// No editor showed the file in time.
    NoEditor,
}

/// Replacing a file's text once the tab just opened for it has created its editor.
///
/// The caller opens [`ReplaceFileText::path`], then reports whether an editor shows it each time
/// the editors change, and the time, until an outcome comes back.
#[derive(Clone, Debug)]
pub struct ReplaceFileText {
    path: String,
    text: String,
    deadline_ms: u64,
    done: bool,
}

impl ReplaceFileText {
    pub fn new(path: &str, text: &str, now_ms: u64, timeout_ms: u64) -> Self {
        let deadline_ms = now_ms.saturating_add(timeout_ms);
        Self { path: path.to_owned(), text: text.to_owned(), deadline_ms, done: false }
    }

    /// The file to open.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The editors changed (or the open finished): `has_editor` says whether one shows the file.
    pub fn editors_changed(&mut self, has_editor: bool, now_ms: u64) -> Option<ReplaceOutcome> {
        if self.done {
            return None;
        }
        if has_editor {
            self.done = true;
            return Some(ReplaceOutcome::Replace { path: self.path.clone(), text: self.text.clone() });
        }
        self.tick(now_ms)
    }

    /// Time passed: gives up once the timeout is over.
    pub fn tick(&mut self, now_ms: u64) -> Option<ReplaceOutcome> {
        if self.done || now_ms < self.deadline_ms {
            return None;
        }
        self.done = true;
        Some(ReplaceOutcome::NoEditor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn config(schema: &[&str]) -> GraphQLConfig {
        GraphQLConfig { schema: schema.iter().map(|s| (*s).to_owned()).collect(), ..GraphQLConfig::default() }
    }

    fn file(path: &str) -> SchemaFile {
        SchemaFile { path: path.to_owned(), analysis: SchemaAnalysis::default() }
    }

    fn files() -> Vec<SchemaFile> {
        vec![file("/p/schema.graphql"), file("/p/api/user.graphqls"), file("/p/web/query.graphql")]
    }

    fn paths(project: &SchemaProject) -> Vec<&str> {
        project.files.iter().map(|f| f.path.as_str()).collect()
    }

    fn range(from: usize, to: usize) -> TextRange {
        TextRange { from, to }
    }

    fn utf16_index(doc: &str, needle: &str) -> usize {
        utf16_len(&doc[..doc.find(needle).expect("needle")])
    }

    const SCHEMA: &str = r#"
"""
type NotAType
"""
type Query { a(x: In): Int, color: Color }
# type Commented
extend type Query { b: [Missing!]! }
enum Color { RED }
input In { x: Int, d: Date }
scalar Date
union U = Query | Gone
directive @auth on FIELD_DEFINITION
"#;

    #[test]
    fn reads_nested_maps_lists_and_flow_lists() {
        let parsed = parse_yaml(
            "# graphql-config\nschema:\n  - schema.graphql   # main\n  - 'types/**/*.graphql'\n\
             documents: [a.graphql, \"b.graphql\"]\nextensions:\n  endpoints:\n    default:\n      \
             url: http://localhost:4000/graphql\n      headers:\n        Authorization: Bearer x\n",
        );
        assert_eq!(
            parsed,
            json!({
                "schema": ["schema.graphql", "types/**/*.graphql"],
                "documents": ["a.graphql", "b.graphql"],
                "extensions": {
                    "endpoints": {
                        "default": {
                            "url": "http://localhost:4000/graphql",
                            "headers": { "Authorization": "Bearer x" },
                        },
                    },
                },
            })
        );
    }

    #[test]
    fn reads_lists_of_maps() {
        assert_eq!(
            parse_yaml("items:\n  - name: a\n    url: x\n  - b\n"),
            json!({ "items": [{ "name": "a", "url": "x" }, "b"] })
        );
    }

    #[test]
    fn reads_schema_pointers_and_endpoints_from_yaml() {
        let c = parse_config(
            "graphql.config.yml",
            "schema: schema.graphql\nextensions:\n  endpoints:\n    local: http://localhost/graphql\n",
        );
        assert_eq!(
            c,
            GraphQLConfig {
                schema: vec!["schema.graphql".to_owned()],
                endpoints: vec![GqlEndpoint {
                    name: "local".to_owned(),
                    url: "http://localhost/graphql".to_owned(),
                    headers: Vec::new()
                }],
                dynamic: false,
                variables: Vec::new(),
            }
        );
    }

    #[test]
    fn reads_json_a_graphqlrc_holding_json_and_the_legacy_schema_path() {
        assert_eq!(parse_config("graphql.config.json", r#"{"schema":["a.graphql"]}"#).schema, ["a.graphql"]);
        assert_eq!(parse_config(".graphqlrc", r#"{ "schemaPath": "s.graphqls" }"#).schema, ["s.graphqls"]);
    }

    #[test]
    fn takes_the_keys_of_a_schema_map_as_pointers() {
        assert_eq!(
            parse_config(".graphqlrc.json", r#"{"schema":{"https://api.dev/graphql":{"headers":{}}}}"#).schema,
            ["https://api.dev/graphql"]
        );
    }

    #[test]
    fn does_not_evaluate_js_ts_configs_and_survives_broken_ones() {
        assert!(parse_config("graphql.config.ts", "export default {}").dynamic);
        assert!(parse_config("graphql.config.json", "{").schema.is_empty());
    }

    #[test]
    fn matches_graphql_config_globs() {
        let matches = |glob: &str, path: &str| glob_to_regex(glob).is_some_and(|re| re.is_match(path));
        assert!(matches("**/*.graphql", "schema.graphql"));
        assert!(matches("**/*.graphql", "a/b/c.graphql"));
        assert!(matches("src/*.{graphql,gql}", "src/q.gql"));
        assert!(!matches("src/*.graphql", "src/a/q.graphql"));
        assert!(matches("./schema.graphql", "schema.graphql"));
    }

    #[test]
    fn counts_definitions_as_the_summary_does_not_comments_or_descriptions() {
        let a = analyze_schema(SCHEMA);
        assert_eq!(a.stats, SchemaStats { types: 3, inputs: 1, enums: 1, directives: 1 });
        let mut defined = a.defined.clone();
        defined.sort();
        assert_eq!(defined, ["Color", "Date", "In", "Query", "U"]);
    }

    #[test]
    fn reports_types_used_but_defined_nowhere_at_their_position() {
        let f = SchemaFile { path: "/p/s.graphql".to_owned(), analysis: analyze_schema(SCHEMA) };
        let project = discover_schemas("/p", &[], &[f]).remove(0);
        let errors: Vec<(String, Pos)> = unresolved_types(&project, &mut I18n::default())
            .into_iter()
            .map(|e| (e.diagnostic.message, e.diagnostic.range.start))
            .collect();
        assert_eq!(
            errors,
            [
                ("The field type 'Missing' is not present when resolving type 'Query'".to_owned(), Pos::new(6, 24)),
                ("The field type 'Gone' is not present when resolving type 'U'".to_owned(), Pos::new(10, 18)),
            ]
        );
        assert_eq!(schema_stats(&project).types, 3);
    }

    #[test]
    fn puts_every_file_in_the_default_schema_without_a_config() {
        let p = discover_schemas("/p", &[], &files()).remove(0);
        assert_eq!(p.config_path, None);
        assert_eq!(paths(&p), ["/p/api/user.graphqls", "/p/schema.graphql", "/p/web/query.graphql"]);
    }

    #[test]
    fn picks_a_configs_files_by_its_globs_relative_to_its_folder() {
        let configs = [ConfigFile {
            path: "/p/api/.graphqlrc.yml".to_owned(),
            config: config(&["*.graphqls", "https://x.dev/graphql"]),
        }];
        let p = discover_schemas("/p", &configs, &files()).remove(0);
        assert_eq!(p.root, "/p/api");
        assert_eq!(paths(&p), ["/p/api/user.graphqls"]);
        assert_eq!(p.remote, ["https://x.dev/graphql"]);
    }

    fn diag(severity: Severity) -> Diagnostic {
        Diagnostic {
            range: Range { start: Pos::new(1, 2), end: Pos::new(1, 4) },
            severity,
            message: "Unknown type".to_owned(),
            source: Some("graphql".to_owned()),
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        }
    }

    #[test]
    fn shows_the_sections_summary_errors_and_endpoints() {
        let mut i18n = I18n::default();
        let root = project_nodes(&discover_schemas("/p", &[], &[])).remove(0);
        let none = |_: &str| -> Vec<Diagnostic> { Vec::new() };
        let sections = child_nodes(&root, &none, &mut i18n).expect("sections");
        assert_eq!(sections.iter().map(SchemaNode::kind).collect::<Vec<_>>(), ["summary", "errors", "endpoints"]);
        assert_eq!(child_nodes(&sections[0], &none, &mut i18n), Some(Vec::new()));
        let kinds = |node: &SchemaNode, i18n: &mut I18n| {
            child_nodes(node, &none, i18n).map(|c| c.iter().map(SchemaNode::kind).collect::<Vec<_>>())
        };
        assert_eq!(kinds(&sections[1], &mut i18n), Some(vec!["noErrors"]));
        assert_eq!(kinds(&sections[2], &mut i18n), Some(vec!["noEndpoints"]));
    }

    #[test]
    fn lists_errors_and_warnings_of_the_schemas_files() {
        let mut i18n = I18n::default();
        let root = project_nodes(&discover_schemas("/p", &[], &files())).remove(0);
        let none = |_: &str| -> Vec<Diagnostic> { Vec::new() };
        let errors = child_nodes(&root, &none, &mut i18n).expect("sections").remove(1);
        let reported = |path: &str| {
            if path == "/p/schema.graphql" {
                vec![diag(Severity::Error), diag(Severity::Hint), diag(Severity::Warning)]
            } else {
                Vec::new()
            }
        };
        let found = child_nodes(&errors, &reported, &mut i18n).expect("errors");
        let severities: Vec<Option<Severity>> = found
            .iter()
            .map(|n| match n {
                SchemaNode::Error { diagnostic, .. } => Some(diagnostic.severity),
                _ => None,
            })
            .collect();
        assert_eq!(severities, [Some(Severity::Error), Some(Severity::Warning)]);
    }

    #[test]
    fn runs_the_selection_else_the_document() {
        assert_eq!(query_text("  { a }\n", None), "{ a }");
        assert_eq!(query_text("{ a } { b }", Some(range(6, 11))), "{ b }");
        assert_eq!(query_text("{ a }", Some(range(2, 2))), "{ a }");
    }

    #[test]
    fn runs_the_operation_around_the_caret_with_the_fragments_it_uses() {
        let doc = "# { not: \"this\" }\nquery A { ...F }\n\
                   query B($id: ID!) { node(id: $id) { ... on User { name } ...G } }\n\
                   fragment F on Query { x }\nfragment G on Node { id ...H }\nfragment H on Node { \"}\" }\n";
        let in_b = utf16_index(doc, "node(");
        assert_eq!(
            query_text(doc, Some(range(in_b, in_b))),
            "query B($id: ID!) { node(id: $id) { ... on User { name } ...G } }\n\nfragment G on Node { id ...H }\n\n\
             fragment H on Node { \"}\" }"
        );
        let in_a = utf16_index(doc, "...F");
        assert_eq!(query_text(doc, Some(range(in_a, in_a))), "query A { ...F }\n\nfragment F on Query { x }");
        // Outside every operation (in the comment): the whole document.
        assert_eq!(query_text(doc, Some(range(3, 3))), doc.trim());
    }

    #[test]
    fn reports_what_was_sent_so_the_rest_can_be_greyed_out() {
        let doc = "query A { a }\nquery B { b }\n";
        assert_eq!(query_context(doc, Some(range(2, 5))).ranges, [range(2, 5)]);
        let in_b = utf16_index(doc, "b }");
        assert_eq!(query_context(doc, Some(range(in_b, in_b))).ranges, [range(14, 27)]);
        assert_eq!(outside_ranges(utf16_len(doc), &[range(14, 27)]), [range(0, 14), range(27, 28)]);
    }

    #[test]
    fn counts_the_errors_of_a_graphql_response() {
        assert_eq!(response_error_count(Some(r#"{"errors":[{"message":"a"},{"message":"b"}]}"#)), 2);
        assert_eq!(response_error_count(Some(r#"{"data":{}}"#)), 0);
        assert_eq!(response_error_count(Some("<html>")), 0);
    }

    #[test]
    fn escapes_braces_in_strings_so_the_http_client_does_not_expand_them() {
        let value = json!({ "query": "{ a { b }}", "variables": { "o": { "p": 1 } } });
        let text = json_without_templates(&value);
        assert!(!text.contains("{{"));
        assert_eq!(serde_json::from_str::<Value>(&text).expect("valid JSON"), value);
    }

    #[test]
    fn reads_variables_as_a_json_object() {
        assert_eq!(parse_variables("  "), Variables::Parsed(None));
        let mut code = Map::new();
        code.insert("code".to_owned(), json!("VN"));
        assert_eq!(parse_variables(r#"{"code":"VN"}"#), Variables::Parsed(Some(code)));
        assert!(matches!(parse_variables("[1]"), Variables::Error(_)));
        assert!(matches!(parse_variables("{"), Variables::Error(_)));
    }

    #[test]
    fn posts_the_query_as_json_with_the_endpoints_headers() {
        let endpoint = GqlEndpoint {
            name: "x".to_owned(),
            url: "http://h/graphql".to_owned(),
            headers: headers(&[("Authorization", "Bearer t")]),
        };
        assert_eq!(
            http_request_text(&endpoint, "{ a }", None),
            "POST http://h/graphql\nContent-Type: application/json\nAuthorization: Bearer t\n\n\
             {\"query\":\"\\u007b a \\u007d\"}\n"
        );
    }

    #[test]
    fn uses_the_schema_listing_the_file_else_the_closest_config_folder() {
        let configs = [
            ConfigFile { path: "/p/.graphqlrc.yml".to_owned(), config: config(&["schema.graphql"]) },
            ConfigFile { path: "/p/api/.graphqlrc.yml".to_owned(), config: config(&["*.graphqls"]) },
        ];
        let projects = discover_schemas("/p", &configs, &files());
        let config_of = |path: &str| schema_of(&projects, path).and_then(|p| p.config_path.as_deref());
        assert_eq!(config_of("/p/schema.graphql"), Some("/p/.graphqlrc.yml"));
        assert_eq!(config_of("/p/api/ops/q.graphql"), Some("/p/api/.graphqlrc.yml"));
    }

    fn introspection_body() -> String {
        let r = |name: &str, kind: &str| json!({ "kind": kind, "name": name, "ofType": null });
        let non_null = |t: Value| json!({ "kind": "NON_NULL", "name": null, "ofType": t });
        let list = |t: Value| json!({ "kind": "LIST", "name": null, "ofType": t });
        json!({
            "data": {
                "__schema": {
                    "queryType": { "name": "Query" },
                    "mutationType": null,
                    "subscriptionType": null,
                    "directives": [{ "name": "include", "locations": ["FIELD"], "args": [] }],
                    "types": [
                        {
                            "kind": "OBJECT",
                            "name": "Query",
                            "description": "Root",
                            "interfaces": [],
                            "fields": [
                                {
                                    "name": "countries",
                                    "args": [
                                        { "name": "filter", "type": r("Filter", "INPUT_OBJECT"), "defaultValue": null },
                                    ],
                                    "type": non_null(list(non_null(r("Country", "OBJECT")))),
                                },
                                {
                                    "name": "old",
                                    "args": [],
                                    "type": r("String", "SCALAR"),
                                    "isDeprecated": true,
                                    "deprecationReason": "Use countries",
                                },
                            ],
                        },
                        {
                            "kind": "OBJECT",
                            "name": "Country",
                            "interfaces": [],
                            "fields": [{ "name": "code", "args": [], "type": non_null(r("ID", "SCALAR")) }],
                        },
                        {
                            "kind": "INPUT_OBJECT",
                            "name": "Filter",
                            "inputFields": [
                                { "name": "code", "type": r("String", "SCALAR"), "defaultValue": "\"VN\"" },
                            ],
                        },
                        { "kind": "ENUM", "name": "Color", "enumValues": [{ "name": "RED" }] },
                        { "kind": "SCALAR", "name": "String" },
                        { "kind": "OBJECT", "name": "__Type", "fields": [] },
                    ],
                },
            },
        })
        .to_string()
    }

    #[test]
    fn prints_the_schema_with_its_header_kinds_then_names_and_sorted_fields() {
        assert_eq!(
            introspection_to_sdl(&introspection_body()),
            Ok("# This file was generated. Do not edit manually.

schema {
    query: Query
}

type Country {
    code: ID!
}

\"Root\"
type Query {
    countries(filter: Filter): [Country!]!
    old: String @deprecated(reason: \"Use countries\")
}

enum Color {
    RED
}

input Filter {
    code: String = \"VN\"
}
"
            .to_owned())
        );
    }

    #[test]
    fn fails_on_a_response_without_a_schema() {
        let result = introspection_to_sdl(r#"{"errors":[{"message":"Introspection disabled"}]}"#);
        assert!(result.is_err_and(|e| e.contains("Introspection disabled")));
    }

    #[test]
    fn writes_to_the_configs_first_schema_file_else_schema_graphql() {
        let configs = [ConfigFile {
            path: "/p/graphql.config.yml".to_owned(),
            config: config(&["types/*.graphqls", "./api.graphql"]),
        }];
        let p = discover_schemas("/p", &configs, &[]).remove(0);
        assert_eq!(introspection_target(&p), "/p/api.graphql");
        let d = discover_schemas("/p", &[], &[]).remove(0);
        assert_eq!(introspection_target(&d), "/p/schema.graphql");
    }

    #[test]
    fn finds_each_endpoints_line_in_yaml_and_json_configs() {
        let yaml = "schema: s.graphql\nextensions:\n  endpoints:\n    countries:\n      url: https://c/\n    \
                    local: http://l/\n";
        let line = |endpoint: &str, line: u32| EndpointLine { endpoint: endpoint.to_owned(), line };
        assert_eq!(endpoint_lines("graphql.config.yml", yaml), [line("countries", 3), line("local", 5)]);
        let json = "{\n  \"extensions\": {\n    \"endpoints\": {\n      \"dev\": \"http://d/\"\n    }\n  }\n}";
        assert_eq!(endpoint_lines("graphql.config.json", json), [line("dev", 3)]);
        assert!(endpoint_lines("graphql.config.yml", "schema: s.graphql").is_empty());
    }

    #[test]
    fn lists_the_variables_a_config_uses_and_expands_endpoints_with_them() {
        let c = parse_config(
            "graphql.config.yml",
            "extensions:\n  endpoints:\n    api:\n      url: ${API_URL:http://localhost:4000}/graphql\n      \
             headers:\n        \
             Authorization: Bearer ${TOKEN}\n",
        );
        assert_eq!(c.variables, ["API_URL", "TOKEN"]);
        let api = &c.endpoints[0];
        assert_eq!(
            expand_endpoint(api, &values(&[("TOKEN", "t")])),
            GqlEndpoint {
                name: "api".to_owned(),
                url: "http://localhost:4000/graphql".to_owned(),
                headers: headers(&[("Authorization", "Bearer t")]),
            }
        );
        assert_eq!(expand_endpoint(api, &values(&[("API_URL", "https://x.dev")])).url, "https://x.dev/graphql");
    }

    #[test]
    fn copies_and_pastes_values_as_name_value_lines() {
        let names = ["API_TOKEN", "COUNTRIES_HOST"];
        assert_eq!(format_environment(&names, &values(&[("API_TOKEN", "t")])), "API_TOKEN=t\nCOUNTRIES_HOST=");
        assert_eq!(
            parse_environment("# comment\nAPI_TOKEN = a=b\r\nOTHER=x\nCOUNTRIES_HOST=https://h\n", &names),
            values(&[("API_TOKEN", "a=b"), ("COUNTRIES_HOST", "https://h")])
        );
    }

    #[test]
    fn needs_the_variables_that_have_neither_a_value_nor_a_default() {
        let endpoint = GqlEndpoint {
            name: "env".to_owned(),
            url: "${HOST:https://h}/graphql".to_owned(),
            headers: headers(&[("Authorization", "Bearer ${TOKEN}"), ("X-Team", "${TEAM}")]),
        };
        assert_eq!(missing_variables(&endpoint, &values(&[])), ["TOKEN", "TEAM"]);
        assert_eq!(missing_variables(&endpoint, &values(&[("TOKEN", "t")])), ["TEAM"]);
        assert!(missing_variables(&endpoint, &values(&[("TOKEN", "t"), ("TEAM", "a")])).is_empty());
        // An empty value counts as not set.
        assert_eq!(missing_variables(&endpoint, &values(&[("TOKEN", ""), ("TEAM", "a")])), ["TOKEN"]);
    }

    #[test]
    fn shows_sizes_in_bytes_and_kb() {
        assert_eq!(format_size(286), "286 bytes");
        assert_eq!(format_size(1741), "1.7 kb");
    }

    #[test]
    fn replaces_the_text_once_the_tab_just_opened_has_created_its_editor() {
        let mut replace = ReplaceFileText::new("/p/schema.graphql", "type Query { b: Int }", 0, 5000);
        assert_eq!(replace.path(), "/p/schema.graphql");
        assert_eq!(replace.editors_changed(false, 0), None);
        assert_eq!(
            replace.editors_changed(true, 10),
            Some(ReplaceOutcome::Replace {
                path: "/p/schema.graphql".to_owned(),
                text: "type Query { b: Int }".to_owned()
            })
        );
        assert_eq!(replace.tick(6000), None);
    }

    #[test]
    fn reports_when_no_editor_shows_the_file() {
        let mut replace = ReplaceFileText::new("/p/missing.graphql", "x", 0, 50);
        assert_eq!(replace.editors_changed(false, 0), None);
        assert_eq!(replace.tick(49), None);
        assert_eq!(replace.tick(50), Some(ReplaceOutcome::NoEditor));
    }
}
