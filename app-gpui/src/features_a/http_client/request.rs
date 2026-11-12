//! Spring endpoints as `.http` requests: the server address from `application.properties` /
//! `application.yml`, `{var}` as `{{var}}`, query parameters, headers and a JSON skeleton of the
//! `@RequestBody` type, and the text appended to a request file.
//!
//! Nothing here reads files. The configuration files and body types are looked up through the
//! paths these functions name; the caller reads them and hands the text back.

use super::source::{ClassDecl, SourceFile, all_classes, parse_source};
use super::spring::Endpoint;
use crate::everywhere::open::utf16_len;
use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
    };
}

re!(PROPERTY, r"^((?:\\.|[^=:\s])+)\s*[=:\s]\s*(.*)$");
re!(ESCAPE, r"\\(.)");
re!(YAML_COMMENT, r"\s+#.*$");
re!(YAML_ENTRY, r#"^(\s*)("[^"]*"|'[^']*'|[^:]+?)\s*:(?:\s+(.*))?$"#);
re!(QUOTED_VALUE, r#"^"(.*)"$|^'(.*)'$"#);
re!(PLACEHOLDER, r"\$\{[^}:]*(?::([^}]*))?\}");
re!(DIGITS, r"^[0-9]+$");
re!(SLASHES, r"/{2,}");
re!(
    NUMBER,
    concat!(
        r"^(byte|short|int|long|float|double|Byte|Short|Integer|Long|Float|Double|Int|BigDecimal|BigInteger|Number",
        r"|UByte|UShort|UInt|ULong)\??$",
    )
);
re!(BOOLEAN, r"^(boolean|Boolean)\??$");
re!(
    STRING,
    concat!(
        r"^(String|CharSequence|char|Character|Char|UUID|LocalDate|LocalDateTime|LocalTime|Instant|OffsetDateTime",
        r"|ZonedDateTime|Date|Duration|URI|URL)\??$",
    )
);
re!(
    COLLECTION,
    concat!(
        r"^(List|ArrayList|LinkedList|Set|HashSet|TreeSet|LinkedHashSet|Collection|Iterable|MutableList|MutableSet",
        r"|Flux|Array|.*Array)(<.*>)?\??$|\[\]$|\.\.\.$",
    )
);
re!(MAP, r"^(Map|HashMap|LinkedHashMap|TreeMap|MutableMap|MultiValueMap|JsonNode|ObjectNode)(<.*>)?\??$");
re!(WRAPPER, r"^(?:[A-Za-z0-9_.]*\.)?(Mono|Optional|HttpEntity|RequestEntity|Deferred|CompletableFuture)<(.*)>$");
re!(QUALIFIER, r"^(?:[a-z_$][A-Za-z0-9_$]*\.)+");
re!(GENERIC_TAIL, r"<.*$");
re!(PATH_VARIABLE, r"\{\*?([A-Za-z0-9_.-]+)(?::[^{}]*(?:\{[^{}]*\}[^{}]*)*)?\}");
re!(TYPE_NAME, r"^[A-Z][A-Za-z0-9_]*$");

/// A flat `key → value` configuration.
pub type Config = HashMap<String, String>;

/// Flat `key → value` of a `.properties` file (`=`, `:` or space separated; `#` / `!` comments).
pub fn parse_properties(text: &str) -> Config {
    let mut out = Config::new();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut i = 0;
    while i < lines.len() {
        let mut line = lines[i].trim().to_owned();
        i += 1;
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        while line.ends_with('\\') && i < lines.len() {
            line.pop();
            line.push_str(lines[i].trim());
            i += 1;
        }
        if let Some(caps) = PROPERTY.captures(&line) {
            let raw_key = caps.get(1).map_or("", |m| m.as_str());
            if !out.contains_key(raw_key) {
                let value = caps.get(2).map_or("", |m| m.as_str()).trim().to_owned();
                out.insert(ESCAPE.replace_all(raw_key, "$1").into_owned(), value);
            }
        }
    }
    out
}

/// Flat dotted keys of a YAML file's first document (block mappings and scalars only; enough for
/// `server.*`).
pub fn parse_yaml(text: &str) -> Config {
    let mut out = Config::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut started = false;
    for raw in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if raw.starts_with("---") {
            if started {
                break;
            }
            continue;
        }
        let line = YAML_COMMENT.replacen(raw, 1, "");
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("- ") {
            continue;
        }
        let Some(caps) = YAML_ENTRY.captures(&line) else { continue };
        started = true;
        let indent = caps.get(1).map_or(0, |m| m.as_str().encode_utf16().count());
        while stack.last().is_some_and(|(at, _)| *at >= indent) {
            stack.pop();
        }
        let key = caps.get(2).map_or("", |m| m.as_str());
        let key = key.strip_prefix(['"', '\'']).unwrap_or(key);
        let key = key.strip_suffix(['"', '\'']).unwrap_or(key).to_owned();
        let path = stack.iter().map(|(_, k)| k.as_str()).chain([key.as_str()]).collect::<Vec<_>>().join(".");
        let value = caps.get(3).map(|m| m.as_str().trim()).filter(|v| !v.is_empty());
        match value {
            Some(value) if value != "|" && value != ">" => {
                let unquoted = QUOTED_VALUE.captures(value).map_or_else(
                    || value.to_owned(),
                    |c| format!("{}{}", c.get(1).map_or("", |m| m.as_str()), c.get(2).map_or("", |m| m.as_str())),
                );
                out.entry(path).or_insert(unquoted);
            }
            _ => stack.push((indent, key)),
        }
    }
    out
}

/// `${PORT:8081}` as `8081`; a placeholder without a default is unknown.
fn resolved(value: Option<&String>) -> Option<String> {
    let value = value?;
    let mut unknown = false;
    let out = PLACEHOLDER.replace_all(value, |caps: &Captures| match caps.get(1) {
        Some(default) => default.as_str().to_owned(),
        None => {
            unknown = true;
            String::new()
        }
    });
    let out = out.into_owned();
    (!unknown).then_some(out)
}

/// `http://localhost:<server.port><context path>` from Spring Boot configuration (the first file
/// with a value wins).
pub fn host_from_config(configs: &[Config]) -> String {
    let get = |keys: &[&str]| {
        configs.iter().find_map(|c| keys.iter().find_map(|k| resolved(c.get(*k)).filter(|v| !v.is_empty())))
    };
    let port = get(&["server.port"]);
    let ssl = get(&["server.ssl.enabled"]).is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || get(&["server.ssl.key-store"]).is_some();
    let scheme = if ssl { "https" } else { "http" };
    let port = match port {
        Some(port) if DIGITS.is_match(&port) && port != "0" => port.parse::<u64>().map_or(port, |n| n.to_string()),
        _ => "8080".to_owned(),
    };
    let context = [
        get(&["server.servlet.context-path", "server.context-path", "spring.webflux.base-path"]).unwrap_or_default(),
        get(&["spring.mvc.servlet.path"]).unwrap_or_default(),
    ]
    .join("/");
    let context = SLASHES.replace_all(&context, "/");
    let context = context.strip_suffix('/').unwrap_or(&context);
    let ctx = if !context.is_empty() && !context.starts_with('/') { format!("/{context}") } else { context.to_owned() };
    format!("{scheme}://localhost:{port}{ctx}")
}

/// Unwraps `Mono<T>`, `Optional<T>`, `HttpEntity<T>`…, qualified names and nullability.
pub fn bare_type(ty: &str) -> String {
    let mut t = ty.trim();
    t = t.strip_suffix('?').unwrap_or(t);
    let mut t = t.to_owned();
    while let Some(inner) = WRAPPER.captures(&t).and_then(|c| c.get(2)).map(|m| m.as_str().trim().to_owned()) {
        t = inner;
    }
    QUALIFIER.replace(&t, "").into_owned()
}

/// Where the classes a body skeleton expands come from.
pub trait ClassLookup {
    fn class(&self, name: &str) -> Option<&ClassDecl>;
}

impl ClassLookup for [ClassDecl] {
    fn class(&self, name: &str) -> Option<&ClassDecl> {
        self.iter().find(|c| c.name == name)
    }
}

impl ClassLookup for HashMap<String, ClassDecl> {
    fn class(&self, name: &str) -> Option<&ClassDecl> {
        self.get(name)
    }
}

/// Placeholder value for a field / body of `ty`; classes found by `lookup` get their properties
/// (one level deep for nested ones).
pub fn skeleton(ty: &str, lookup: &(impl ClassLookup + ?Sized)) -> Value {
    skeleton_at(ty, lookup, 0)
}

fn skeleton_at(ty: &str, lookup: &(impl ClassLookup + ?Sized), depth: usize) -> Value {
    let t = bare_type(ty);
    if NUMBER.is_match(&t) {
        return Value::from(0);
    }
    if BOOLEAN.is_match(&t) {
        return Value::Bool(false);
    }
    if STRING.is_match(&t) {
        return Value::String(String::new());
    }
    if COLLECTION.is_match(&t) {
        return Value::Array(Vec::new());
    }
    if MAP.is_match(&t) || t == "Object" || t == "Any" || t.is_empty() {
        return Value::Object(Map::new());
    }
    let Some(cls) = lookup.class(&GENERIC_TAIL.replace(&t, "")).filter(|_| depth <= 1) else {
        return Value::Object(Map::new());
    };
    let mut obj = Map::new();
    for p in &cls.ctor_params {
        obj.insert(p.name.clone(), skeleton_at(&p.ty, lookup, depth + 1));
    }
    for f in cls.fields.iter().filter(|f| !f.is_static) {
        if !obj.contains_key(&f.name) {
            obj.insert(f.name.clone(), skeleton_at(&f.ty, lookup, depth + 1));
        }
    }
    Value::Object(obj)
}

fn path_variables(path: &str) -> String {
    PATH_VARIABLE.replace_all(path, "{{${1}}}").into_owned()
}

/// How an endpoint is rendered.
#[derive(Clone, Debug, Default)]
pub struct RenderOptions<'a> {
    pub host: &'a str,
    /// Path to use (one of the endpoint's paths); the first by default.
    pub path: Option<&'a str>,
    /// JSON body of the `@RequestBody` type.
    pub body: Option<Value>,
}

/// One `### Name` request block, ending with a line break.
pub fn render_request(e: &Endpoint, opts: &RenderOptions) -> String {
    let path = path_variables(opts.path.or(e.paths.first().map(String::as_str)).unwrap_or("/"));
    let query = e
        .query
        .iter()
        .map(|q| format!("{}={}", q.name, q.value.clone().unwrap_or_else(|| format!("{{{{{}}}}}", q.name))))
        .collect::<Vec<_>>()
        .join("&");
    let query = if query.is_empty() { String::new() } else { format!("?{query}") };
    let mut lines = vec![
        format!("### {}.{}", e.class_name, e.method_name),
        format!("{} {}{path}{query}", e.http_method, opts.host),
    ];
    let mut headers: Vec<(String, String)> = e.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect();
    let content_type = e.consumes.clone().or_else(|| e.body_type.as_ref().map(|_| "application/json".to_owned()));
    if let Some(produces) = &e.produces {
        headers.insert(0, ("Accept".to_owned(), produces.clone()));
    }
    if let Some(ct) = content_type.as_ref().filter(|_| e.body_type.is_some() || e.consumes.is_some()) {
        headers.push(("Content-Type".to_owned(), ct.clone()));
    }
    lines.extend(headers.iter().map(|(name, value)| format!("{name}: {value}")));
    if e.body_type.is_some() {
        lines.push(String::new());
        let body = opts.body.clone().unwrap_or_else(|| Value::Object(Map::new()));
        let form = content_type.as_deref() == Some("application/x-www-form-urlencoded");
        match body {
            Value::Object(fields) if form => {
                lines.push(fields.keys().map(|k| format!("{k}=")).collect::<Vec<_>>().join("&"));
            }
            body => lines.push(serde_json::to_string_pretty(&body).unwrap_or_default()),
        }
    }
    format!("{}\n", lines.join("\n"))
}

/// Text appended to a request file, where it goes and where the caret lands (UTF-16 offsets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Append {
    pub insert: String,
    pub from: usize,
    /// The first request line, in the new text.
    pub caret: usize,
}

/// Text to append to a request file holding `existing`, and the offset (in the new text) of the
/// first request line.
pub fn append_requests(existing: &str, blocks: &[String]) -> Append {
    let from = utf16_len(existing);
    let sep = if existing.trim().is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let insert = format!("{sep}{}", blocks.join("\n"));
    let first_line = blocks.first().and_then(|b| b.find('\n').map(|at| utf16_len(&b[..at]) + 1)).unwrap_or(0);
    Append { insert, from, caret: from + sep.len() + first_line }
}

/// `p` without its last segment (either separator).
fn dirname(p: &str) -> &str {
    p.rfind(['/', '\\']).map_or(p, |at| &p[..at])
}

/// The Spring Boot configuration files for `source_path` (its module's `src/main/resources`), most
/// specific first.
pub fn config_candidates(source_path: &str, project_root: Option<&str>) -> Vec<String> {
    let norm = source_path.replace('\\', "/");
    let mut roots: Vec<String> = norm.rfind("/src/main/").map(|at| norm[..at].to_owned()).into_iter().collect();
    if let Some(root) = project_root
        && !roots.iter().any(|r| r == root)
    {
        let root = root.replace('\\', "/");
        roots.push(root.strip_suffix('/').unwrap_or(&root).to_owned());
    }
    roots
        .iter()
        .flat_map(|r| {
            ["application.properties", "application.yml", "application.yaml"]
                .map(|f| format!("{r}/src/main/resources/{f}"))
        })
        .collect()
}

/// The server address from the configuration files that exist, in candidate order (`None` for a
/// missing or unreadable file).
pub fn host_from_files<'a>(files: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> String {
    let configs: Vec<Config> = files
        .into_iter()
        .filter_map(|(path, text)| {
            let text = text?;
            Some(if path.ends_with(".properties") { parse_properties(text) } else { parse_yaml(text) })
        })
        .collect();
    host_from_config(&configs)
}

/// A source file a lookup has read.
#[derive(Clone, Debug)]
struct Origin {
    path: String,
    file: SourceFile,
}

/// A class being looked for: the files that may declare it, tried in order.
#[derive(Clone, Debug)]
struct Load {
    name: String,
    candidates: Vec<String>,
    next: usize,
    /// Loading a body type (its property types are looked up from its file next).
    body: bool,
}

/// The body types of endpoints: classes of the controller's file, then `<Type>.java|.kt` beside it
/// or where its import points (same source root), and their property types one level down.
///
/// Reads one file at a time: [`BodyLookup::pending`] names it and [`BodyLookup::answer`] takes its
/// text (`None` when it is missing). When nothing is pending, [`BodyLookup::finish`] gives the
/// classes found.
#[derive(Clone, Debug)]
pub struct BodyLookup {
    known: HashMap<String, ClassDecl>,
    origin: Origin,
    ext: [&'static str; 2],
    names: VecDeque<String>,
    props: VecDeque<String>,
    props_from: Option<Origin>,
    load: Option<Load>,
}

fn type_name(ty: &str) -> String {
    GENERIC_TAIL.replace(&bare_type(ty), "").into_owned()
}

fn source_root(path: &str, file: &SourceFile) -> String {
    let mut root = dirname(path);
    let depth = if file.package_name.is_empty() { 0 } else { file.package_name.split('.').count() };
    for _ in 0..depth {
        root = dirname(root);
    }
    root.to_owned()
}

impl BodyLookup {
    pub fn new(source_path: &str, file: &SourceFile, endpoints: &[&Endpoint]) -> Self {
        let mut names = VecDeque::new();
        for e in endpoints {
            if let Some(ty) = &e.body_type {
                let name = type_name(ty);
                if !names.contains(&name) {
                    names.push_back(name);
                }
            }
        }
        let mut lookup = Self {
            known: HashMap::new(),
            origin: Origin { path: source_path.replace('\\', "/"), file: file.clone() },
            ext: if source_path.ends_with(".kt") { [".kt", ".java"] } else { [".java", ".kt"] },
            names,
            props: VecDeque::new(),
            props_from: None,
            load: None,
        };
        lookup.add(file);
        lookup.advance();
        lookup
    }

    fn add(&mut self, file: &SourceFile) {
        for c in all_classes(file) {
            self.known.entry(c.name.clone()).or_insert_with(|| c.clone());
        }
    }

    /// The files that may declare `name` as seen from `from` (its import, then its directory);
    /// `None` when it is known or not a class name.
    fn start_load(&self, name: &str, from: &Origin, body: bool) -> Option<Load> {
        if self.known.contains_key(name) || !TYPE_NAME.is_match(name) {
            return None;
        }
        let suffix = format!(".{name}");
        let imported = from.file.imports.iter().find(|i| i.ends_with(&suffix));
        let dir = dirname(&from.path).to_owned();
        let mut dirs = Vec::new();
        if let Some(imported) = imported {
            let package: Vec<&str> = imported.split('.').collect();
            dirs.push(format!("{}/{}", source_root(&from.path, &from.file), package[..package.len() - 1].join("/")));
        }
        dirs.push(dir);
        let candidates = dirs.iter().flat_map(|d| self.ext.map(|x| format!("{d}/{name}{x}"))).collect();
        Some(Load { name: name.to_owned(), candidates, next: 0, body })
    }

    fn begin_props(&mut self, name: &str, from: Origin) {
        self.props = self
            .known
            .get(name)
            .map(|c| {
                let fields = c.fields.iter().filter(|f| !f.is_static).map(|f| f.ty.as_str());
                c.ctor_params.iter().map(|p| p.ty.as_str()).chain(fields).map(type_name).collect()
            })
            .unwrap_or_default();
        self.props_from = Some(from);
    }

    fn advance(&mut self) {
        while self.load.is_none() {
            if let Some(prop) = self.props.pop_front() {
                let from = self.props_from.clone().unwrap_or_else(|| self.origin.clone());
                self.load = self.start_load(&prop, &from, false);
                continue;
            }
            let Some(name) = self.names.pop_front() else { return };
            let origin = self.origin.clone();
            match self.start_load(&name, &origin, true) {
                Some(load) => self.load = Some(load),
                None => self.begin_props(&name, origin),
            }
        }
    }

    /// The file to read next; `None` when the lookup is done.
    pub fn pending(&self) -> Option<&str> {
        self.load.as_ref().and_then(|l| l.candidates.get(l.next)).map(String::as_str)
    }

    /// The text of the pending file, `None` when it is missing or unreadable.
    pub fn answer(&mut self, text: Option<&str>) {
        let Some(mut load) = self.load.take() else { return };
        match text {
            Some(text) => {
                let path = load.candidates[load.next].clone();
                let file = parse_source(text);
                self.add(&file);
                if load.body {
                    self.begin_props(&load.name, Origin { path, file });
                }
            }
            None => {
                load.next += 1;
                if load.next < load.candidates.len() {
                    self.load = Some(load);
                } else if load.body {
                    let origin = self.origin.clone();
                    self.begin_props(&load.name, origin);
                }
            }
        }
        self.advance();
    }

    /// The classes found, by name.
    pub fn finish(self) -> HashMap<String, ClassDecl> {
        self.known
    }
}

#[cfg(test)]
mod tests {
    use super::super::spring::fixtures::{JAVA, KOTLIN};
    use super::super::spring::spring_controllers;
    use super::*;
    use serde_json::json;

    fn run(mut lookup: BodyLookup, files: &HashMap<&str, &str>) -> HashMap<String, ClassDecl> {
        while let Some(path) = lookup.pending().map(str::to_owned) {
            lookup.answer(files.get(path.as_str()).copied());
        }
        lookup.finish()
    }

    fn endpoint<'a>(endpoints: &'a [Endpoint], name: &str) -> &'a Endpoint {
        endpoints.iter().find(|e| e.method_name == name).expect("endpoint")
    }

    #[test]
    fn renders_requests_with_path_variables_query_and_json_body() {
        let eps = spring_controllers(JAVA).remove(0).endpoints;
        let host = "http://localhost:8080";
        let opts = RenderOptions { host, ..RenderOptions::default() };
        assert_eq!(
            render_request(endpoint(&eps, "list"), &opts),
            "### UserController.list\nGET http://localhost:8080/api/users?page=0&q={{q}}\n"
        );
        assert_eq!(
            render_request(endpoint(&eps, "get"), &opts),
            "### UserController.get\nGET http://localhost:8080/api/users/{{id}}\nX-Trace: {{X-Trace}}\n"
        );
        assert_eq!(
            render_request(endpoint(&eps, "avatar"), &opts),
            "### UserController.avatar\nPUT http://localhost:8080/api/users/{{id}}/avatar\n\
             Content-Type: application/json\n\n{}\n"
        );
        let file = parse_source(JAVA);
        let nested: Vec<ClassDecl> = file.classes.iter().flat_map(|c| c.classes.iter().cloned()).collect();
        let rename = endpoint(&eps, "rename");
        let body = skeleton(rename.body_type.as_deref().expect("body type"), nested.as_slice());
        assert_eq!(
            render_request(rename, &RenderOptions { host, path: None, body: Some(body) }),
            [
                "### UserController.rename",
                "PATCH http://localhost:8080/api/users/{{id}}/name",
                "Content-Type: application/json",
                "",
                "{",
                "  \"name\": \"\",",
                "  \"notify\": false",
                "}",
                "",
            ]
            .join("\n")
        );
        assert_eq!(
            render_request(endpoint(&eps, "create"), &RenderOptions { host, path: Some("/api/users/new"), body: None }),
            [
                "### UserController.create",
                "POST http://localhost:8080/api/users/new",
                "Accept: application/json",
                "Content-Type: application/json",
                "",
                "{}",
                "",
            ]
            .join("\n")
        );
    }

    #[test]
    fn builds_the_body_from_a_data_class_of_the_file() {
        let file = parse_source(KOTLIN);
        let eps = spring_controllers(KOTLIN).remove(0).endpoints;
        let refs: Vec<&Endpoint> = eps.iter().collect();
        let lookup = BodyLookup::new("/p/src/main/kotlin/com/example/demo/web/OrderController.kt", &file, &refs);
        let classes = run(lookup, &HashMap::new());
        assert_eq!(skeleton("NewOrder", &classes), json!({ "item": "", "quantity": 0, "tags": [] }));
    }

    #[test]
    fn reads_server_port_and_the_context_path_from_properties() {
        let props = parse_properties(
            "# comment\nserver.port=9090\nserver.servlet.context-path = /shop/\nspring.application.name: demo\n",
        );
        assert_eq!(host_from_config(&[props]), "http://localhost:9090/shop");
    }

    #[test]
    fn reads_nested_and_flattened_keys_from_yamls_first_document() {
        let yml = parse_yaml(
            "spring:\n  application:\n    name: demo # x\nserver:\n  port: ${PORT:8181}\n  servlet:\n    \
             context-path: \"/api\"\n---\nserver:\n  port: 1\n",
        );
        assert_eq!(host_from_config(&[yml]), "http://localhost:8181/api");
        assert_eq!(host_from_config(&[parse_yaml("server.port: 7070\n")]), "http://localhost:7070");
    }

    #[test]
    fn defaults_to_localhost_8080_and_keeps_unknown_placeholders_out() {
        assert_eq!(host_from_config(&[]), "http://localhost:8080");
        assert_eq!(host_from_config(&[parse_properties("server.port=${PORT}\n")]), "http://localhost:8080");
        assert_eq!(host_from_config(&[parse_properties("server.port=0\n")]), "http://localhost:8080");
        assert_eq!(
            host_from_config(&[parse_properties("server.port=8443\nserver.ssl.enabled=true\n")]),
            "https://localhost:8443"
        );
        assert_eq!(
            host_from_config(&[parse_yaml("spring:\n  webflux:\n    base-path: /rx\n")]),
            "http://localhost:8080/rx"
        );
    }

    #[test]
    fn looks_in_the_modules_src_main_resources() {
        let files: HashMap<&str, &str> = HashMap::from([
            ("/ws/api/src/main/resources/application.yml", "server:\n  port: 8001\n"),
            ("/ws/src/main/resources/application.properties", "server.port=9000\n"),
        ]);
        let host = |source: &str| {
            let candidates = config_candidates(source, Some("/ws"));
            host_from_files(candidates.iter().map(|p| (p.as_str(), files.get(p.as_str()).copied())))
        };
        assert_eq!(host("/ws/api/src/main/java/a/A.java"), "http://localhost:8001");
        assert_eq!(host("/ws/other/A.java"), "http://localhost:9000");
    }

    #[test]
    fn follows_the_import_to_the_same_source_root_then_the_controllers_directory() {
        let files: HashMap<&str, &str> = HashMap::from([
            (
                "/p/src/main/java/com/example/demo/dto/CreateUser.java",
                "package com.example.demo.dto;\npublic class CreateUser {\n  public static final int MAX = 3;\n  \
                 private String name;\n  private int age;\n  private Address address;\n  \
                 private List<String> roles;\n}\n",
            ),
            (
                "/p/src/main/java/com/example/demo/dto/Address.java",
                "package com.example.demo.dto;\npublic record Address(String city, String zip) {}\n",
            ),
        ]);
        let file = parse_source(JAVA);
        let eps = spring_controllers(JAVA).remove(0).endpoints;
        let refs: Vec<&Endpoint> = eps.iter().collect();
        let lookup = BodyLookup::new("/p/src/main/java/com/example/demo/web/UserController.java", &file, &refs);
        let classes = run(lookup, &files);
        assert_eq!(
            skeleton("CreateUser", &classes),
            json!({ "name": "", "age": 0, "address": { "city": "", "zip": "" }, "roles": [] })
        );
        assert_eq!(skeleton("Mono<CreateUser>", &classes)["name"], json!(""));
        assert_eq!(skeleton("List<CreateUser>", &classes), json!([]));
        assert_eq!(skeleton("Unknown", &classes), json!({}));
    }

    #[test]
    fn starts_an_empty_file_with_the_request() {
        let block = "### A.a\nGET http://localhost:8080/a\n".to_owned();
        assert_eq!(
            append_requests("", std::slice::from_ref(&block)),
            Append { insert: block.clone(), from: 0, caret: 8 }
        );
    }

    #[test]
    fn separates_from_existing_requests_with_a_blank_line_before_the_separator() {
        let block = "### A.a\nGET http://localhost:8080/a\n".to_owned();
        let existing = "### X\nGET http://x\n";
        let r = append_requests(existing, &[block.clone(), block.clone()]);
        let text = format!("{existing}{}", r.insert);
        assert_eq!(text, format!("{existing}\n{block}\n{block}"));
        assert!(text[r.caret..].starts_with("GET http://localhost:8080/a"));
        assert!(append_requests("GET http://x", std::slice::from_ref(&block)).insert.starts_with("\n\n###"));
        assert!(append_requests("GET http://x\n\n", std::slice::from_ref(&block)).insert.starts_with("###"));
    }
}
