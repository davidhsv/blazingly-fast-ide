//! File templates ("New ▸ Rust File", "Kotlin Class"…): naming rules, package inference and body
//! rendering.
//!
//! Body variables: `$NAME$` (the entered name, styled per template), `$PACKAGE$`, `$FILE_NAME$`,
//! `$DATE$`, `$TIME$`, `$YEAR$`, `$USER$`, `$PROJECT_NAME$`, `$END$` (the caret); IDEA's `${NAME}`
//! spelling works too. A line mentioning `$PACKAGE$` is dropped, together with one following blank
//! line, when the package is empty. `#parse("File Header")` inserts an include or code template,
//! indented like the directive; a line whose include is empty disappears.

use super::files_builtins::{builtin_code_templates, builtin_includes};
use super::live_model::{format_date, kebab_case, pascal_case, snake_case};
use chrono::{Datelike, Local, NaiveDateTime};
use regex::{Captures, Regex};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

/// How the entered name becomes the file's name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NameStyle {
    #[default]
    Asis,
    Snake,
    Pascal,
    Kebab,
}

/// How the package (or namespace) of a new file is inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageStyle {
    /// Java / Kotlin: the path below a source root.
    Jvm,
    /// Go: the package of the directory.
    Go,
    /// PHP: the PSR-4 namespace of the directory.
    Php,
}

/// One body of a template ("Class", "Interface", "JUnit 5 Test").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTemplateKind {
    pub id: String,
    pub label: String,
    pub body: String,
    /// Overrides the template's extension (Stylesheet › CSS / SCSS / Less).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

/// A value the New File dialog asks for, besides the name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatePrompt {
    pub name: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTemplate {
    pub id: String,
    /// Menu label, e.g. "Rust File".
    pub name: String,
    pub icon: String,
    /// Without the dot; empty for "File…" (the name carries the extension).
    #[serde(default)]
    pub extension: String,
    #[serde(default)]
    pub name_style: NameStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_style: Option<PackageStyle>,
    /// Dotted names (`com.acme.Foo`) create package directories.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dotted_packages: bool,
    #[serde(default)]
    pub kinds: Vec<FileTemplateKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Prefilled name for fixed-name files (`Dockerfile`, `.editorconfig`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_name: Option<String>,
    /// Exact filename supplied by a plugin; skips the name prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<TemplatePrompt>,
    /// Infer the package style from the extension, by the built-in templates' rules.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub infer_package: bool,
    /// Reformat according to style: run Reformat Code on the new file (unset: on, as IDEA).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reformat: Option<bool>,
}

/// A fragment for `#parse` (Includes tab: "File Header") or a Code tab template.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateFragment {
    /// The name `#parse` refers to, e.g. `File Header`.
    pub name: String,
    pub body: String,
    /// What uses it (Code tab).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The fragments `#parse` looks its names up in, the first list that knows a name winning.
#[derive(Debug, Clone, Copy)]
pub struct Includes<'a> {
    lists: &'a [&'a [TemplateFragment]],
}

impl<'a> Includes<'a> {
    pub fn new(lists: &'a [&'a [TemplateFragment]]) -> Self {
        Self { lists }
    }

    /// The built-in includes and code templates.
    pub fn builtin() -> Includes<'static> {
        static LISTS: LazyLock<[&'static [TemplateFragment]; 2]> =
            LazyLock::new(|| [builtin_includes(), builtin_code_templates()]);
        Includes { lists: LISTS.as_slice() }
    }

    /// The body of the include or code template called `name`.
    pub fn resolve(&self, name: &str) -> Option<&'a str> {
        self.lists.iter().find_map(|list| {
            // IDEA names carry the extension (`File Header.java`); ours may omit it.
            let with_extension =
                |f: &&TemplateFragment| name.strip_prefix(f.name.as_str()).is_some_and(|rest| rest.starts_with('.'));
            let hit = list.iter().find(|f| f.name == name).or_else(|| list.iter().find(with_extension));
            hit.map(|f| f.body.as_str())
        })
    }
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

fn without_final_newline(text: &str) -> &str {
    text.strip_suffix('\n').unwrap_or(text)
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn expand_includes_at(body: &str, includes: Includes<'_>, depth: usize) -> String {
    static PARSE_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r#"^([ \t]*)#parse\(\s*"([^"]+)"\s*\)[ \t]*$"#));
    static PARSE_INLINE: LazyLock<Regex> = LazyLock::new(|| pattern(r#"#parse\(\s*"([^"]+)"\s*\)"#));
    if depth > 4 || !body.contains("#parse") {
        return body.to_owned();
    }
    let mut out: Vec<String> = Vec::new();
    for line in body.split('\n') {
        let Some(directive) = PARSE_LINE.captures(line) else {
            let replaced = PARSE_INLINE.replace_all(line, |found: &Captures<'_>| match includes.resolve(&found[1]) {
                Some(include) => expand_includes_at(without_final_newline(include), includes, depth + 1),
                None => found[0].to_owned(),
            });
            out.push(replaced.into_owned());
            continue;
        };
        let Some(include) = includes.resolve(&directive[2]) else {
            out.push(line.to_owned());
            continue;
        };
        let include = normalize_newlines(include);
        let text = expand_includes_at(without_final_newline(&include), includes, depth + 1);
        if text.is_empty() {
            continue;
        }
        let indent = &directive[1];
        out.extend(text.split('\n').map(|l| if l.is_empty() { String::new() } else { format!("{indent}{l}") }));
    }
    out.join("\n")
}

/// Expands `#parse("name")` directives (nested up to a few levels); unknown names stay as written.
pub fn expand_includes(body: &str, includes: Includes<'_>) -> String {
    expand_includes_at(body, includes, 0)
}

/// The template as [`plan_file`] sees it for `kind`: with the kind's extension when it has its own.
pub fn with_kind<'t>(template: &'t FileTemplate, kind: Option<&FileTemplateKind>) -> Cow<'t, FileTemplate> {
    match kind.and_then(|kind| kind.extension.as_deref()) {
        Some(extension) if extension != template.extension => {
            Cow::Owned(FileTemplate { extension: extension.to_owned(), ..template.clone() })
        }
        _ => Cow::Borrowed(template),
    }
}

fn or_original(styled: String, name: &str) -> String {
    if styled.is_empty() { name.to_owned() } else { styled }
}

pub fn style_name(name: &str, style: NameStyle) -> String {
    match style {
        NameStyle::Snake => or_original(snake_case(name), name),
        NameStyle::Pascal => {
            let mut chars = name.chars();
            let is_pascal =
                chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric());
            if is_pascal { name.to_owned() } else { or_original(pascal_case(name), name) }
        }
        NameStyle::Kebab => or_original(kebab_case(name), name),
        NameStyle::Asis => name.to_owned(),
    }
}

/// Where a new file goes and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Absolute path of the new file.
    pub path: String,
    /// Directory the file goes into (may not exist yet).
    pub directory: String,
    /// The `$NAME$` value.
    pub name: String,
    pub file_name: String,
}

fn is_ascii_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Device names Windows reserves, whatever the extension.
fn is_reserved_stem(stem: &str) -> bool {
    let stem = stem.to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|device| {
            stem.strip_prefix(device).is_some_and(|n| n.len() == 1 && n.chars().all(|c| ('1'..='9').contains(&c)))
        })
}

/// Whether a plugin's exact filename is one every platform can create in a directory.
fn is_safe_file_name(file_name: &str) -> bool {
    let stem = file_name.split('.').next().unwrap_or_default().trim_end();
    !file_name.is_empty()
        && file_name.len() <= 255
        && file_name.trim() == file_name
        && !file_name.ends_with('.')
        && !file_name.contains(['\\', '/', '<', '>', ':', '"', '|', '?', '*'])
        && !file_name.chars().any(|c| (c as u32) < 32 || (127..=159).contains(&(c as u32)))
        && !is_reserved_stem(stem)
}

/// `com.acme.Name`: several `.`-separated words.
fn is_dotted_name(part: &str) -> bool {
    let is_word = |word: &str| !word.is_empty() && word.chars().all(|c| is_ascii_word(c) || c == '$');
    part.contains('.') && part.split('.').all(is_word)
}

/// Resolves the entered name to a file path: strips a typed extension, allows `sub/dir/name` (and
/// `com.acme.Name` for JVM templates) and applies the template's naming style. A template with an
/// exact filename ignores `input`. `None` for a name no file can have.
pub fn plan_file(template: &FileTemplate, directory: &str, input: &str) -> Option<PlannedFile> {
    let base = directory.trim_end_matches('/');
    if let Some(file_name) = template.file_name.as_deref() {
        if !is_safe_file_name(file_name) {
            return None;
        }
        let name = match file_name.rfind('.') {
            Some(dot) if dot > 0 => &file_name[..dot],
            _ => file_name,
        };
        return Some(PlannedFile {
            path: format!("{base}/{file_name}"),
            directory: base.to_owned(),
            name: name.to_owned(),
            file_name: file_name.to_owned(),
        });
    }
    let slashed = input.trim().replace('\\', "/");
    let mut raw = slashed.trim_matches('/');
    if raw.is_empty() {
        return None;
    }
    let ext = if template.extension.is_empty() { String::new() } else { format!(".{}", template.extension) };
    if !ext.is_empty()
        && let Some(cut) = raw.len().checked_sub(ext.len())
        && raw.is_char_boundary(cut)
        && raw[cut..].to_lowercase() == ext.to_lowercase()
    {
        raw = &raw[..cut];
    }
    let mut parts: Vec<&str> = raw.split('/').filter(|part| !part.is_empty()).collect();
    if template.dotted_packages
        && let [only] = parts[..]
        && is_dotted_name(only)
    {
        parts = only.split('.').collect();
    }
    if parts.iter().any(|part| matches!(*part, "." | "..")) {
        return None;
    }
    let last = parts.pop()?;
    let name = style_name(last, template.name_style);
    if name.is_empty() || name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return None;
    }
    let directory = std::iter::once(base).chain(parts).collect::<Vec<_>>().join("/");
    let file_name = format!("{name}{ext}");
    Some(PlannedFile { path: format!("{directory}/{file_name}"), directory, name, file_name })
}

const JVM_ROOTS: [&str; 4] = ["java", "kotlin", "scala", "groovy"];

/// The package the folders `segments` spell (`-` becoming `_`); none when one is not an identifier.
fn to_package(segments: &[&str]) -> String {
    let clean: Vec<String> = segments.iter().map(|segment| segment.replace('-', "_")).collect();
    let is_identifier = |segment: &String| {
        let mut chars = segment.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            && chars.all(|c| is_ascii_word(c) || c == '$')
    };
    if clean.iter().all(is_identifier) { clean.join(".") } else { String::new() }
}

/// Java/Kotlin package for a directory: the path below the nearest source or test root
/// (`source_roots`: project-relative, `""` being the content root), as IDEA does. Without a
/// containing root, conventions apply (`src/main/kotlin/com/acme` → `com.acme`, also
/// `src/commonMain/kotlin/…`, `src/test/java/…`); a plain `src/com/acme` falls back to below `src`.
pub fn infer_jvm_package<S: AsRef<str>>(directory: &str, project_root: Option<&str>, source_roots: &[S]) -> String {
    let mut rel = directory.trim_end_matches('/');
    if let Some(root) = project_root.filter(|root| !root.is_empty()) {
        let trimmed = root.trim_end_matches('/');
        if rel == root || rel.strip_prefix(trimmed).is_some_and(|rest| rest.starts_with('/')) {
            rel = &rel[trimmed.len().min(rel.len())..];
        }
    }
    let segments: Vec<&str> = rel.split('/').filter(|segment| !segment.is_empty()).collect();
    let rel_path = segments.join("/");
    let contains = |root: &str| {
        root.is_empty() || rel_path == root || rel_path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
    };
    // The innermost root: the longest, the first listed among equals.
    let mut nearest: Option<&str> = None;
    for root in source_roots {
        let root: &str = root.as_ref();
        if contains(root) && nearest.is_none_or(|best| root.encode_utf16().count() > best.encode_utf16().count()) {
            nearest = Some(root);
        }
    }
    if let Some(root) = nearest {
        let depth = root.split('/').filter(|segment| !segment.is_empty()).count();
        return to_package(&segments[depth.min(segments.len())..]);
    }
    let Some(src) = segments.iter().rposition(|segment| *segment == "src") else {
        return String::new();
    };
    let language_root = (src + 1..segments.len()).rev().find(|&i| JVM_ROOTS.contains(&segments[i]));
    match language_root {
        Some(root) => to_package(&segments[root + 1..]),
        None => {
            let below = &segments[src + 1..];
            // `src/main` without a language root has no package.
            let skip = usize::from(matches!(below.first(), Some(&("main" | "test"))));
            to_package(&below[skip..])
        }
    }
}

/// Go package for a directory: the package of an existing sibling file when known, `main` under
/// `cmd/`, otherwise the sanitized directory name.
pub fn infer_go_package(directory: &str, project_root: Option<&str>, sibling_package: Option<&str>) -> String {
    if let Some(sibling) = sibling_package.filter(|sibling| !sibling.is_empty()) {
        return sibling.to_owned();
    }
    let segments: Vec<&str> = directory.split('/').filter(|segment| !segment.is_empty()).collect();
    let root_depth = project_root.unwrap_or_default().split('/').filter(|segment| !segment.is_empty()).count();
    let below = segments.get(root_depth..).unwrap_or_default();
    if below.split_last().is_some_and(|(_, parents)| parents.contains(&"cmd")) {
        return "main".to_owned();
    }
    let base: String = segments
        .last()
        .copied()
        .unwrap_or("main")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) { "main".to_owned() } else { base }
}

/// The package a Go source file declares.
pub fn go_package_of(text: &str) -> Option<&str> {
    static PACKAGE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?m)^\s*package\s+([A-Za-z_][A-Za-z0-9_]*)"));
    PACKAGE.captures(text).and_then(|found| found.get(1)).map(|name| name.as_str())
}

/// The parts of a JSON document the PSR-4 maps are read from: objects keep the order their
/// entries are written in, which decides between mappings of the same depth.
enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    Text(String),
    Other,
}

impl Json {
    fn field(&self, name: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries.iter().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AnyValue;

        impl<'de> Visitor<'de> for AnyValue {
            type Value = Json;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }

            fn visit_bool<E>(self, _: bool) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_str<E>(self, text: &str) -> Result<Json, E> {
                Ok(Json::Text(text.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Json::Array(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Json>()? {
                    entries.push(entry);
                }
                Ok(Json::Object(entries))
            }
        }

        deserializer.deserialize_any(AnyValue)
    }
}

/// `autoload.psr-4` and `autoload-dev.psr-4` of a composer.json: `(namespace prefix, folder)`, the
/// folder relative to the composer.json. Nothing for text that is not JSON.
pub fn composer_psr4(json: &str) -> Vec<(String, String)> {
    let Ok(composer) = serde_json::from_str::<Json>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        let Some(Json::Object(map)) = composer.field(section).and_then(|autoload| autoload.field("psr-4")) else {
            continue;
        };
        for (prefix, value) in map {
            let folders = match value {
                Json::Array(folders) => folders.as_slice(),
                single => std::slice::from_ref(single),
            };
            for folder in folders {
                let Json::Text(folder) = folder else { continue };
                let folder = folder.replace('\\', "/");
                let folder = folder.strip_prefix("./").unwrap_or(&folder).trim_end_matches('/');
                out.push((prefix.clone(), folder.to_owned()));
            }
        }
    }
    out
}

/// PHP namespace for `directory` (relative to the composer.json folder): the PSR-4 prefix of the
/// longest mapped folder that contains it, then the folders below it (`App\` + `src/` → `src/Http`
/// gives `App\Http`). Outside every mapping: no namespace.
pub fn infer_php_namespace(directory: &str, psr4: &[(String, String)]) -> String {
    let rel: Vec<&str> = directory.split('/').filter(|segment| !segment.is_empty()).collect();
    let mut best: Option<(&str, usize)> = None;
    for (prefix, folder) in psr4 {
        let segments: Vec<&str> = folder.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        if !rel.starts_with(&segments) {
            continue;
        }
        if best.is_none_or(|(_, depth)| segments.len() > depth) {
            best = Some((prefix.as_str(), segments.len()));
        }
    }
    let Some((prefix, depth)) = best else {
        return String::new();
    };
    let namespace = prefix.split('\\').filter(|part| !part.is_empty());
    let parts: Vec<&str> = namespace.chain(rel[depth..].iter().copied()).collect();
    // A PHP label: letters, digits, `_` and anything outside ASCII, not starting with a digit.
    let is_label = |part: &&str| {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii())
            && chars.all(|c| is_ascii_word(c) || !c.is_ascii())
    };
    if parts.iter().all(is_label) { parts.join("\\") } else { String::new() }
}

/// What a file template's body is rendered with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTemplateVars {
    pub name: String,
    pub file_name: String,
    pub package_name: Option<String>,
    pub project_name: Option<String>,
    pub user: Option<String>,
    /// The moment `$DATE$`, `$TIME$` and `$YEAR$` show; the local time when unset.
    pub now: Option<NaiveDateTime>,
    /// Values the dialog asked for. The names above win over them.
    pub variables: BTreeMap<String, String>,
}

/// A rendered body: its text and where the caret goes (UTF-16 offset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub caret: u32,
}

/// Renders a body; the caret is at `$END$`, else at the end. Values are inserted as they are:
/// a `$` in one is never read as a variable.
pub fn render_file_template(body: &str, vars: &FileTemplateVars, includes: Includes<'_>) -> Rendered {
    static PACKAGE_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\$PACKAGE(_NAME)?\$|\$\{PACKAGE(_NAME)?\}"));
    let now = vars.now.unwrap_or_else(|| Local::now().naive_local());
    let package = vars.package_name.clone().unwrap_or_default();
    let mut values = vars.variables.clone();
    values.extend(
        [
            ("NAME", vars.name.clone()),
            ("FILE_NAME", vars.file_name.clone()),
            ("PACKAGE", package.clone()),
            ("PACKAGE_NAME", package.clone()),
            ("PROJECT_NAME", vars.project_name.clone().unwrap_or_default()),
            ("USER", vars.user.clone().unwrap_or_default()),
            ("DATE", format_date(now, None)),
            ("TIME", format_date(now, Some("HH:mm"))),
            ("YEAR", now.year().to_string()),
        ]
        .map(|(name, value)| (name.to_owned(), value)),
    );
    let expanded = expand_includes(&normalize_newlines(body), includes);
    let mut lines: Vec<&str> = expanded.split('\n').collect();
    if package.is_empty() {
        let mut kept = Vec::with_capacity(lines.len());
        let mut i = 0;
        while i < lines.len() {
            if PACKAGE_LINE.is_match(lines[i]) {
                // The blank line after the package statement goes with it.
                i += if lines.get(i + 1).is_some_and(|next| next.trim().is_empty()) { 2 } else { 1 };
                continue;
            }
            kept.push(lines[i]);
            i += 1;
        }
        lines = kept;
    }
    let src: Vec<char> = lines.join("\n").chars().collect();
    let mut text = String::new();
    let mut caret: Option<u32> = None;
    let mut i = 0;
    while i < src.len() {
        if src[i] == '$' {
            if src.get(i + 1) == Some(&'$') {
                text.push('$');
                i += 2;
                continue;
            }
            let braced = src.get(i + 1) == Some(&'{');
            let (start, closer) = if braced { (i + 2, '}') } else { (i + 1, '$') };
            let close = src.get(start..).and_then(|rest| rest.iter().position(|&c| c == closer)).map(|at| start + at);
            if let Some(close) = close {
                let key: String = src[start..close].iter().collect();
                if key == "END" {
                    caret.get_or_insert(text.encode_utf16().count() as u32);
                    i = close + 1;
                    continue;
                }
                if let Some(value) = values.get(&key) {
                    text.push_str(value);
                    i = close + 1;
                    continue;
                }
            }
        }
        text.push(src[i]);
        i += 1;
    }
    let caret = caret.unwrap_or(text.encode_utf16().count() as u32);
    Rendered { text, caret }
}

/// Whether "Reformat according to style" is on (IDEA's default, unless the user turned it off).
pub fn reformats_on_create(template: &FileTemplate) -> bool {
    template.reformat != Some(false)
}

fn is_space(unit: u16) -> bool {
    b" \t\r\n".iter().any(|&space| unit == u16::from(space))
}

fn is_blank(unit: u16) -> bool {
    unit == u16::from(b' ') || unit == u16::from(b'\t')
}

/// Where the caret (`$END$`) goes after Reformat Code turned `before` into `after`: behind the
/// same number of non-whitespace characters, then on the same line below them (past the new
/// indentation), since formatters mostly change whitespace and may replace the whole text in one
/// edit. Offsets are UTF-16.
pub fn caret_after_reformat(before: &str, caret: u32, after: &str) -> u32 {
    const NEWLINE: u16 = b'\n' as u16;
    let before: Vec<u16> = before.encode_utf16().collect();
    let after: Vec<u16> = after.encode_utf16().collect();
    let caret = (caret as usize).min(before.len());
    let tokens = before[..caret].iter().filter(|&&unit| !is_space(unit)).count();
    let gap_start = before[..caret].iter().rposition(|&unit| !is_space(unit)).map_or(0, |last| last + 1);
    let gap = &before[gap_start..caret];
    let newlines = gap.iter().filter(|&&unit| unit == NEWLINE).count();
    let mut p = 0;
    let mut seen = 0;
    while p < after.len() && seen < tokens {
        if !is_space(after[p]) {
            seen += 1;
        }
        p += 1;
    }
    if tokens > 0 && p >= after.len() {
        return after.len() as u32;
    }
    if newlines == 0 {
        // Same line as the token before: keep the spaces typed between them, as far as they still exist.
        let kept = after[p..].iter().take(gap.len()).take_while(|&&unit| is_blank(unit)).count();
        return (p + kept) as u32;
    }
    let mut q = p;
    for _ in 0..newlines {
        let Some(newline) = after[q..].iter().position(|&unit| unit == NEWLINE).map(|at| q + at) else {
            break;
        };
        if !after[q..newline].iter().all(|&unit| is_blank(unit) || unit == u16::from(b'\r')) {
            break;
        }
        q = newline + 1;
    }
    if q == p {
        return p as u32;
    }
    (q + after[q..].iter().take_while(|&&unit| is_blank(unit)).count()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::files_builtins::builtin_file_templates;
    use chrono::NaiveDate;

    fn tpl(id: &str) -> &'static FileTemplate {
        builtin_file_templates().iter().find(|t| t.id == id).expect("a built-in template")
    }

    fn kind<'t>(template: &'t FileTemplate, id: &str) -> &'t FileTemplateKind {
        template.kinds.iter().find(|k| k.id == id).expect("a kind of the template")
    }

    fn path(template: &FileTemplate, directory: &str, input: &str) -> Option<String> {
        plan_file(template, directory, input).map(|plan| plan.path)
    }

    fn vars(name: &str, file_name: &str) -> FileTemplateVars {
        FileTemplateVars { name: name.to_owned(), file_name: file_name.to_owned(), ..FileTemplateVars::default() }
    }

    fn in_package(name: &str, file_name: &str, package: &str) -> FileTemplateVars {
        FileTemplateVars { package_name: Some(package.to_owned()), ..vars(name, file_name) }
    }

    fn render(body: &str, vars: &FileTemplateVars) -> Rendered {
        render_file_template(body, vars, Includes::builtin())
    }

    const NO_ROOTS: [&str; 0] = [];

    // package inference › uses the path below JVM source roots
    #[test]
    fn uses_the_path_below_jvm_source_roots() {
        let infer = |directory: &str| infer_jvm_package(directory, Some("/p"), &NO_ROOTS);
        assert_eq!(infer("/p/src/main/kotlin/com/acme"), "com.acme");
        assert_eq!(infer("/p/app/src/test/java/org/x/util"), "org.x.util");
        assert_eq!(infer("/p/shared/src/commonMain/kotlin/io/k"), "io.k");
        assert_eq!(infer("/p/src/com/acme"), "com.acme");
        assert_eq!(infer("/p/src/main/java"), "");
        assert_eq!(infer("/p/docs"), "");
        assert_eq!(infer("/p/src/main/java/my-app/x"), "my_app.x");
        assert_eq!(infer("/p/src/main/java/1bad"), "");
    }

    // package inference › counts from the nearest source root when roots are known
    #[test]
    fn counts_from_the_nearest_source_root_when_roots_are_known() {
        // A custom root (`<sourceDirectory>gen</sourceDirectory>`, Mark Directory as Sources Root).
        assert_eq!(infer_jvm_package("/p/gen/com/acme", Some("/p"), &["gen"]), "com.acme");
        assert_eq!(infer_jvm_package("/p/gen", Some("/p"), &["gen"]), "");
        // Innermost root wins; conventions apply outside every root.
        assert_eq!(infer_jvm_package("/p/src/x/y/z", Some("/p"), &["src", "src/x"]), "y.z");
        assert_eq!(infer_jvm_package("/p/src/main/java/a/b", Some("/p"), &["lib"]), "a.b");
        assert_eq!(infer_jvm_package("/p/gens/a", Some("/p"), &["gen"]), "");
    }

    // package inference › infers Go packages
    #[test]
    fn infers_go_packages() {
        assert_eq!(infer_go_package("/p/internal/store", Some("/p"), None), "store");
        assert_eq!(infer_go_package("/p/cmd/server", Some("/p"), None), "main");
        assert_eq!(infer_go_package("/p/pkg/http-util", Some("/p"), None), "httputil");
        assert_eq!(infer_go_package("/p/pkg/x", Some("/p"), Some("xsvc")), "xsvc");
    }

    // file naming › applies styles and extensions
    #[test]
    fn applies_styles_and_extensions() {
        assert_eq!(style_name("UserService", NameStyle::Snake), "user_service");
        assert_eq!(style_name("user service", NameStyle::Pascal), "UserService");
        assert_eq!(path(tpl("rust"), "/p/src", "HttpClient").as_deref(), Some("/p/src/http_client.rs"));
        assert_eq!(path(tpl("tsx"), "/p/src", "user-card.tsx").as_deref(), Some("/p/src/UserCard.tsx"));
        let java = plan_file(tpl("java"), "/p/src/main/java", "com.acme.Foo").expect("a dotted name is planned");
        assert_eq!(java.path, "/p/src/main/java/com/acme/Foo.java");
        assert_eq!(java.directory, "/p/src/main/java/com/acme");
        assert_eq!(java.name, "Foo");
        assert_eq!(path(tpl("file"), "/p", "notes/todo.txt").as_deref(), Some("/p/notes/todo.txt"));
        assert_eq!(plan_file(tpl("file"), "/p", "  "), None);
        assert_eq!(plan_file(tpl("file"), "/p", "../x"), None);
    }

    // file naming › lets a kind pick the extension (Stylesheet › SCSS)
    #[test]
    fn lets_a_kind_pick_the_extension() {
        let css = tpl("stylesheet");
        let planned = |id: &str, input: &str| path(&with_kind(css, Some(kind(css, id))), "/p/web", input);
        assert_eq!(planned("css", "main").as_deref(), Some("/p/web/main.css"));
        assert_eq!(planned("scss", "main").as_deref(), Some("/p/web/main.scss"));
        assert_eq!(planned("less", "main.less").as_deref(), Some("/p/web/main.less"));
        assert!(matches!(with_kind(css, Some(kind(css, "css"))), Cow::Borrowed(_)));
    }

    // file naming › prefills fixed names (Dockerfile, .editorconfig, .gitignore, go.mod)
    #[test]
    fn prefills_fixed_names() {
        let fixed = [("dockerfile", "Dockerfile"), ("editorconfig", ".editorconfig"), ("gitignore", ".gitignore")];
        for (id, name) in fixed.into_iter().chain([("go-mod", "go")]) {
            let t = tpl(id);
            assert_eq!(t.default_name.as_deref(), Some(name));
            let ext = if t.extension.is_empty() { String::new() } else { format!(".{}", t.extension) };
            assert_eq!(path(t, "/p", name), Some(format!("/p/{name}{ext}")));
        }
    }

    // file naming › offers IDEA's common file kinds
    #[test]
    fn offers_ideas_common_file_kinds() {
        let ids: Vec<&str> = builtin_file_templates().iter().map(|t| t.id.as_str()).collect();
        for id in ["stylesheet", "dockerfile", "editorconfig", "json", "yaml", "toml", "shell", "gitignore"] {
            assert!(ids.contains(&id), "{id}");
        }
        let unique: std::collections::BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len());
    }

    fn psr4() -> Vec<(String, String)> {
        composer_psr4(
            r#"{
                "autoload": { "psr-4": { "App\\": "src/", "Legacy\\": ["src/legacy/", "lib"] } },
                "autoload-dev": { "psr-4": { "App\\Tests\\": "./tests" } }
            }"#,
        )
    }

    // PHP namespaces › reads autoload and autoload-dev PSR-4 maps
    #[test]
    fn reads_autoload_and_autoload_dev_psr4_maps() {
        let pairs = psr4();
        let pairs: Vec<(&str, &str)> = pairs.iter().map(|(prefix, dir)| (prefix.as_str(), dir.as_str())).collect();
        assert_eq!(
            pairs,
            [("App\\", "src"), ("Legacy\\", "src/legacy"), ("Legacy\\", "lib"), ("App\\Tests\\", "tests")]
        );
        assert!(composer_psr4("{ not json").is_empty());
    }

    // PHP namespaces › uses the longest mapped folder and adds the folders below it
    #[test]
    fn uses_the_longest_mapped_folder_and_adds_the_folders_below_it() {
        let psr4 = psr4();
        assert_eq!(infer_php_namespace("/src/Http", &psr4), "App\\Http");
        assert_eq!(infer_php_namespace("/src", &psr4), "App");
        assert_eq!(infer_php_namespace("/src/legacy/Billing", &psr4), "Legacy\\Billing");
        assert_eq!(infer_php_namespace("/tests/Service", &psr4), "App\\Tests\\Service");
        assert_eq!(infer_php_namespace("/config", &psr4), "");
        assert_eq!(infer_php_namespace("/src/my-dir", &psr4), "");
        assert_eq!(infer_php_namespace("/x", &[(String::new(), String::new())]), "x");
    }

    // PHP namespaces › writes the namespace into PHP classes, and leaves it out without one
    #[test]
    fn writes_the_namespace_into_php_classes_and_leaves_it_out_without_one() {
        let class = tpl("php-class");
        let cls = render(&class.kinds[0].body, &in_package("UserController", "UserController.php", "App\\Http"));
        assert_eq!(cls.text, "<?php\n\nnamespace App\\Http;\n\nclass UserController\n{\n    \n}\n");
        let bare = render(&class.kinds[0].body, &vars("Foo", "Foo.php"));
        assert_eq!(bare.text, "<?php\n\nclass Foo\n{\n    \n}\n");
        let mailer = in_package("MailerTest", "MailerTest.php", "App\\Tests\\Service");
        let test = render(&kind(class, "test").body, &mailer);
        assert_eq!(
            test.text,
            "<?php\n\nnamespace App\\Tests\\Service;\n\nuse PHPUnit\\Framework\\TestCase;\n\n\
             final class MailerTest extends TestCase\n{\n\
             \x20   public function testSomething(): void\n    {\n        \n    }\n}\n"
        );
        assert_eq!(render(&tpl("php").kinds[0].body, &vars("bootstrap", "bootstrap.php")).text, "<?php\n\n\n");
    }

    // rendering › substitutes variables and places the caret
    #[test]
    fn substitutes_variables_and_places_the_caret() {
        let r = render(&tpl("java").kinds[0].body, &in_package("Foo", "Foo.java", "com.acme"));
        assert_eq!(r.text, "package com.acme;\n\npublic class Foo {\n    \n}\n");
        assert_eq!(&r.text[r.caret as usize..], "\n}\n");
    }

    // rendering › drops the package line when there is no package
    #[test]
    fn drops_the_package_line_when_there_is_no_package() {
        let r = render(&tpl("kotlin").kinds[0].body, &vars("Foo", "Foo.kt"));
        assert_eq!(r.text, "class Foo {\n    \n}\n");
    }

    // rendering › caret defaults to the end and $$ is a dollar
    #[test]
    fn caret_defaults_to_the_end_and_double_dollar_is_a_dollar() {
        let now = NaiveDate::from_ymd_opt(2030, 2, 1).and_then(|date| date.and_hms_opt(0, 0, 0));
        let r = render("a$$b $YEAR$", &FileTemplateVars { now, ..FileTemplateVars::default() });
        assert_eq!(r, Rendered { text: "a$b 2030".to_owned(), caret: 8 });
    }

    fn fragments() -> Vec<TemplateFragment> {
        let fragment = |name: &str, body: &str| TemplateFragment {
            name: name.to_owned(),
            body: body.to_owned(),
            description: None,
        };
        vec![
            fragment("File Header", "/**\n * @author $USER$\n */\n"),
            fragment("Empty", ""),
            fragment("Method", "@Test\nvoid test() {\n    $END$\n}\n"),
        ]
    }

    // includes › expands #parse lines with the directive's indent and drops empty ones
    #[test]
    fn expands_parse_lines_with_the_directives_indent_and_drops_empty_ones() {
        let fragments = fragments();
        let lists = [fragments.as_slice()];
        let includes = Includes::new(&lists);
        assert_eq!(
            expand_includes("#parse(\"File Header\")\nclass A {\n    #parse(\"Method\")\n}", includes),
            "/**\n * @author $USER$\n */\nclass A {\n    @Test\n    void test() {\n        $END$\n    }\n}"
        );
        assert_eq!(expand_includes("a\n#parse(\"Empty\")\nb", includes), "a\nb");
        assert_eq!(expand_includes("#parse(\"Missing\")", includes), "#parse(\"Missing\")");
    }

    // includes › resolves IDEA names with an extension
    #[test]
    fn resolves_idea_names_with_an_extension() {
        let fragments = fragments();
        let lists = [fragments.as_slice()];
        assert_eq!(
            expand_includes("#parse(\"File Header.java\")", Includes::new(&lists)),
            "/**\n * @author $USER$\n */"
        );
    }

    // includes › renders includes with the template variables and ${VAR} syntax
    #[test]
    fn renders_includes_with_the_template_variables_and_braced_syntax() {
        let fragments = fragments();
        let lists = [fragments.as_slice()];
        let vars = FileTemplateVars { user: Some("ann".to_owned()), ..vars("Foo", "Foo.java") };
        let r = render_file_template("#parse(\"File Header\")\nclass ${NAME} {}", &vars, Includes::new(&lists));
        assert_eq!(r.text, "/**\n * @author ann\n */\nclass Foo {}");
    }

    // includes › builds the built-in test templates from code templates
    #[test]
    fn builds_the_built_in_test_templates_from_code_templates() {
        let r = render(&kind(tpl("java"), "test").body, &vars("FooTest", "FooTest.java"));
        assert!(r.text.contains("class FooTest {\n\n    @Test\n    void test() {\n        \n    }\n}\n"), "{}", r.text);
        assert!(r.text.starts_with("import org.junit"));
    }

    // reformat according to style › is on unless the user turned it off
    #[test]
    fn is_on_unless_the_user_turned_it_off() {
        let with = |reformat: Option<bool>| FileTemplate { reformat, ..FileTemplate::default() };
        assert!(reformats_on_create(&with(None)));
        assert!(reformats_on_create(&with(Some(true))));
        assert!(!reformats_on_create(&with(Some(false))));
    }

    /// The caret `|` marks in `before` moved by a reformat to `after`, and where `after` marks it.
    fn moved(before: &str, after: &str) -> (u32, u32) {
        let at = |text: &str| (text.replace('|', ""), text.find('|').unwrap_or_default() as u32);
        let (before, caret) = at(before);
        let (after, wanted) = at(after);
        (caret_after_reformat(&before, caret, &after), wanted)
    }

    // reformat according to style › keeps the caret inside the re-indented body
    #[test]
    fn keeps_the_caret_inside_the_re_indented_body() {
        let (got, want) = moved("class Foo {\n  |\n}\n", "class Foo {\n    |\n}\n");
        assert_eq!(got, want);
    }

    // reformat according to style › follows the tokens when whitespace around them changes
    #[test]
    fn follows_the_tokens_when_whitespace_around_them_changes() {
        let (got, want) = moved("fn main(){let x=1;|}", "fn main() {\n    let x = 1;|\n}");
        assert_eq!(got, want);
    }

    // reformat according to style › keeps spaces typed after a token on its line
    #[test]
    fn keeps_spaces_typed_after_a_token_on_its_line() {
        let (got, want) = moved("return |;", "return |;");
        assert_eq!(got, want);
    }

    // reformat according to style › stays at the start of the text and clamps past the end
    #[test]
    fn stays_at_the_start_of_the_text_and_clamps_past_the_end() {
        assert_eq!(caret_after_reformat("  a", 0, "a"), 0);
        assert_eq!(caret_after_reformat("a b", 3, "a"), 1);
    }

    // plugin template names and values › keeps exact filename %s
    #[test]
    fn keeps_exact_filenames() {
        for file_name in [".dockerignore", ".gitattributes", "Dockerfile", "Order.java"] {
            let template = FileTemplate { file_name: Some(file_name.to_owned()), ..tpl("java").clone() };
            assert_eq!(path(&template, "/p", "ignored"), Some(format!("/p/{file_name}")));
        }
    }

    // plugin template names and values › rejects unsafe filename %j
    #[test]
    fn rejects_unsafe_filenames() {
        let unsafe_names = [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "/absolute",
            "C:evil",
            "a?",
            " a",
            "a.",
            "NUL.txt",
            "com1",
            "a\n",
        ];
        for file_name in unsafe_names {
            let template = FileTemplate { file_name: Some(file_name.to_owned()), ..tpl("file").clone() };
            assert_eq!(plan_file(&template, "/p", ""), None, "{file_name:?}");
        }
    }

    // plugin template names and values › inserts variables literally and preserves UTF-16 caret offsets
    #[test]
    fn inserts_variables_literally_and_preserves_utf16_caret_offsets() {
        let variables =
            BTreeMap::from([("ENTITY".to_owned(), "😀$NAME$".to_owned()), ("NAME".to_owned(), "bad".to_owned())]);
        let vars = FileTemplateVars { variables, ..vars("Repo", "Repo.java") };
        let r = render("$ENTITY$ ${ENTITY} $NAME$ $END$x", &vars);
        assert_eq!(r.text, "😀$NAME$ 😀$NAME$ Repo x");
        assert_eq!(r.caret as usize, "😀$NAME$ 😀$NAME$ Repo ".encode_utf16().count());
    }

    // Not from a spec: a Go file's package is read from its first package clause.
    #[test]
    fn reads_the_package_a_go_file_declares() {
        assert_eq!(go_package_of("// Doc\n  package service\n"), Some("service"));
        assert_eq!(go_package_of("func main() {}"), None);
    }
}
