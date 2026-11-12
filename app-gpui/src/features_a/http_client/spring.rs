//! Spring Web request mappings of a Java / Kotlin source: `@RequestMapping` / `@GetMapping`…
//! methods of `@RestController` / `@Controller` classes, with the class-level mapping applied.
//! Constants are resolved within the file. Offsets are UTF-16 code units.

use super::source::{Annotation, ClassDecl, Expr, Method, Param, SourceFile, Tok, TokKind, all_classes, parse_source};
use regex::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryParam {
    pub name: String,
    /// `defaultValue`, if any.
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub class_name: String,
    pub method_name: String,
    /// Offset of the method name (gutter line).
    pub offset: usize,
    /// The method declaration, annotations included.
    pub range: Range<usize>,
    /// Offset of the controller class name.
    pub class_offset: usize,
    pub http_method: String,
    /// Class × method paths (`/` joined, `{var}` kept), first one first.
    pub paths: Vec<String>,
    pub query: Vec<QueryParam>,
    pub headers: Vec<Header>,
    /// `consumes` (first).
    pub consumes: Option<String>,
    /// `produces` (first).
    pub produces: Option<String>,
    /// `@RequestBody` parameter type as written, if any.
    pub body_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controller {
    pub name: String,
    pub offset: usize,
    /// The class declaration, annotations included.
    pub range: Range<usize>,
    pub endpoints: Vec<Endpoint>,
}

/// The controller and endpoint whose declaration holds an offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpringAt<'a> {
    pub controller: Option<&'a Controller>,
    pub endpoint: Option<&'a Endpoint>,
}

const SHORTCUTS: [(&str, &str); 5] = [
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("DeleteMapping", "DELETE"),
    ("PatchMapping", "PATCH"),
];

const MEDIA_TYPES: [(&str, &str); 24] = [
    ("ALL", "*/*"),
    ("APPLICATION_JSON", "application/json"),
    ("APPLICATION_JSON_UTF8", "application/json;charset=UTF-8"),
    ("APPLICATION_XML", "application/xml"),
    ("APPLICATION_ATOM_XML", "application/atom+xml"),
    ("APPLICATION_FORM_URLENCODED", "application/x-www-form-urlencoded"),
    ("APPLICATION_OCTET_STREAM", "application/octet-stream"),
    ("APPLICATION_PDF", "application/pdf"),
    ("APPLICATION_PROBLEM_JSON", "application/problem+json"),
    ("APPLICATION_PROBLEM_XML", "application/problem+xml"),
    ("APPLICATION_NDJSON", "application/x-ndjson"),
    ("APPLICATION_STREAM_JSON", "application/stream+json"),
    ("APPLICATION_CBOR", "application/cbor"),
    ("APPLICATION_GRAPHQL_RESPONSE", "application/graphql-response+json"),
    ("IMAGE_GIF", "image/gif"),
    ("IMAGE_JPEG", "image/jpeg"),
    ("IMAGE_PNG", "image/png"),
    ("MULTIPART_FORM_DATA", "multipart/form-data"),
    ("MULTIPART_MIXED", "multipart/mixed"),
    ("TEXT_EVENT_STREAM", "text/event-stream"),
    ("TEXT_HTML", "text/html"),
    ("TEXT_MARKDOWN", "text/markdown"),
    ("TEXT_PLAIN", "text/plain"),
    ("TEXT_XML", "text/xml"),
];

static TEMPLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([A-Za-z0-9_]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)").expect("valid regex"));
static IS_MAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(java\.util\.)?(Map|MultiValueMap|HttpHeaders|LinkedMultiValueMap|HashMap)(?-u:\b)")
        .expect("valid regex")
});
static CONTROLLER_GUARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(?:[A-Za-z0-9_.]+\.)?(?:Rest)?Controller(?-u:\b)").expect("valid regex"));
static SLASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/{2,}").expect("valid regex"));

/// Evaluates string-valued annotation arguments within one file.
pub struct Constants {
    fields: HashMap<String, Expr>,
}

impl Constants {
    pub fn new(file: &SourceFile) -> Self {
        let mut fields = HashMap::new();
        let class_fields = all_classes(file).into_iter().flat_map(|c| c.fields.iter());
        for f in file.fields.iter().chain(class_fields) {
            if let Some(init) = &f.init {
                fields.entry(f.name.clone()).or_insert_with(|| init.clone());
            }
        }
        Self { fields }
    }

    /// Every element of an array (`{a, b}`, `[a, b]`, `arrayOf(a, b)`) or the single value;
    /// unresolvable ones are dropped.
    pub fn strings(&self, expr: Option<&Expr>, enums: bool) -> Vec<String> {
        let Some(expr) = expr.filter(|e| !e.is_empty()) else { return Vec::new() };
        let first = &expr[0];
        let inner: Option<&[Tok]> = if first.kind == TokKind::Punct && (first.text == "{" || first.text == "[") {
            Some(&expr[1..expr.len() - 1])
        } else if first.kind == TokKind::Id
            && matches!(first.text.as_str(), "arrayOf" | "listOf" | "setOf")
            && expr.get(1).is_some_and(|t| t.text == "(")
        {
            Some(&expr[2..expr.len() - 1])
        } else {
            None
        };
        match inner {
            None => self.value(expr, enums, &mut HashSet::new()).into_iter().collect(),
            Some(inner) => split_top(inner).iter().filter_map(|e| self.value(e, enums, &mut HashSet::new())).collect(),
        }
    }

    pub fn string(&self, expr: Option<&Expr>, enums: bool) -> Option<String> {
        self.strings(expr, enums).into_iter().next()
    }

    fn value(&self, expr: &[Tok], enums: bool, seen: &mut HashSet<String>) -> Option<String> {
        let mut out = String::new();
        for part in split_plus(expr) {
            out.push_str(&self.part(&part, enums, seen)?);
        }
        Some(out)
    }

    fn part(&self, part: &[Tok], enums: bool, seen: &mut HashSet<String>) -> Option<String> {
        // Kotlin templates of constants; other `${…}` are property placeholders, kept.
        if let [only] = part {
            if only.kind == TokKind::Str {
                let value = only.value.as_deref().unwrap_or_default();
                let replaced = TEMPLATE.replace_all(value, |caps: &Captures| {
                    let whole = caps.get(0).map_or("", |m| m.as_str());
                    let name = caps.get(1).or_else(|| caps.get(2)).map_or("", |m| m.as_str());
                    let init = if seen.contains(name) { None } else { self.fields.get(name) };
                    let resolved = init.and_then(|init| {
                        let mut inner = seen.clone();
                        inner.insert(name.to_owned());
                        self.value(init, false, &mut inner)
                    });
                    resolved.unwrap_or_else(|| whole.to_owned())
                });
                return Some(replaced.into_owned());
            }
            if only.kind == TokKind::Num {
                return Some(only.text.clone());
            }
        }
        if part.first().is_some_and(|t| t.text == "(") && part.last().is_some_and(|t| t.text == ")") {
            return self.value(&part[1..part.len() - 1], enums, seen);
        }
        // `NAME`, `Paths.NAME`, `MediaType.APPLICATION_JSON_VALUE`, `RequestMethod.GET`.
        let dotted = part
            .iter()
            .enumerate()
            .all(|(i, t)| if i % 2 == 1 { t.kind == TokKind::Punct && t.text == "." } else { t.kind == TokKind::Id });
        if !dotted {
            return None;
        }
        let names: Vec<&str> = part.iter().filter(|t| t.kind == TokKind::Id).map(|t| t.text.as_str()).collect();
        let last = *names.last()?;
        if names.len() >= 2 && names[names.len() - 2] == "MediaType" {
            let key = last.strip_suffix("_VALUE").unwrap_or(last);
            return MEDIA_TYPES.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_owned());
        }
        if let Some(init) = self.fields.get(last)
            && !seen.contains(last)
        {
            seen.insert(last.to_owned());
            return self.value(init, enums, seen);
        }
        enums.then(|| last.to_owned())
    }
}

fn split_at(expr: &[Tok], separator: &str, keep_empty_tail: bool) -> Vec<Expr> {
    let mut parts = Vec::new();
    let mut cur: Expr = Vec::new();
    let mut depth = 0i32;
    for t in expr {
        if t.kind == TokKind::Punct && matches!(t.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        }
        if t.kind == TokKind::Punct && matches!(t.text.as_str(), ")" | "]" | "}") {
            depth -= 1;
        }
        if depth == 0 && t.kind == TokKind::Punct && t.text == separator {
            parts.push(std::mem::take(&mut cur));
        } else {
            cur.push(t.clone());
        }
    }
    if keep_empty_tail || !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn split_top(expr: &[Tok]) -> Vec<Expr> {
    split_at(expr, ",", false)
}

fn split_plus(expr: &[Tok]) -> Vec<Expr> {
    split_at(expr, "+", true)
}

fn mapping(annotations: &[Annotation]) -> Option<(&Annotation, Option<&'static str>)> {
    annotations.iter().find_map(|ann| {
        if let Some((_, method)) = SHORTCUTS.iter().find(|(name, _)| *name == ann.name) {
            return Some((ann, Some(*method)));
        }
        (ann.name == "RequestMapping").then_some((ann, None))
    })
}

fn mapping_paths(ann: Option<&Annotation>, c: &Constants) -> Vec<String> {
    ann.map(|ann| c.strings(ann.arg("path").or_else(|| ann.arg("value")), false)).unwrap_or_default()
}

/// `base` and `path` joined with single slashes, without a trailing one (`/` for nothing).
pub fn join_paths(base: &str, path: &str) -> String {
    let joined = SLASHES.replace_all(&format!("/{base}/{path}"), "/").into_owned();
    if joined.len() > 1 { joined.strip_suffix('/').unwrap_or(&joined).to_owned() } else { "/".to_owned() }
}

fn annotation<'a>(p: &'a Param, name: &str) -> Option<&'a Annotation> {
    p.annotations.iter().find(|a| a.name == name)
}

/// Map-typed `@RequestParam` / `@RequestHeader` collect all of them: nothing to generate.
fn is_map(ty: &str) -> bool {
    IS_MAP.is_match(ty)
}

/// The first non-empty of `value` and `name`, else the parameter's own name.
fn param_name(c: &Constants, ann: &Annotation, p: &Param) -> String {
    c.string(ann.arg("value"), false)
        .filter(|s| !s.is_empty())
        .or_else(|| c.string(ann.arg("name"), false).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| p.name.clone())
}

/// `a=1` as its name and value: JS-style `split("=", 2)` keeps the first two pieces only.
fn condition(cond: &str) -> (&str, Option<&str>) {
    let mut pieces = cond.split('=');
    (pieces.next().unwrap_or_default(), pieces.next())
}

fn endpoint(cls: &ClassDecl, method: &Method, c: &Constants) -> Option<Endpoint> {
    let (ann, shortcut) = mapping(&method.annotations)?;
    let class_mapping = cls.annotations.iter().find(|a| a.name == "RequestMapping");
    let http_method = shortcut
        .map(str::to_owned)
        .or_else(|| c.string(ann.arg("method"), true))
        .or_else(|| class_mapping.and_then(|m| c.string(m.arg("method"), true)))
        .unwrap_or_else(|| "GET".to_owned());
    let bases = mapping_paths(class_mapping, c);
    let own = mapping_paths(Some(ann), c);
    let empty = [String::new()];
    let bases: &[String] = if bases.is_empty() { &empty } else { &bases };
    let own: &[String] = if own.is_empty() { &empty } else { &own };
    let paths = bases.iter().flat_map(|b| own.iter().map(move |p| join_paths(b, p))).collect();

    let mut query: Vec<QueryParam> = Vec::new();
    let mut headers: Vec<Header> = Vec::new();
    let mut body_type = None;
    for p in &method.params {
        let rp = annotation(p, "RequestParam");
        let rh = annotation(p, "RequestHeader");
        if let Some(rp) = rp.filter(|_| !is_map(&p.ty)) {
            query.push(QueryParam { name: param_name(c, rp, p), value: c.string(rp.arg("defaultValue"), false) });
        } else if let Some(rh) = rh.filter(|_| !is_map(&p.ty)) {
            let name = param_name(c, rh, p);
            let value = c.string(rh.arg("defaultValue"), false).unwrap_or_else(|| format!("{{{{{name}}}}}"));
            headers.push(Header { name, value });
        } else if annotation(p, "RequestBody").is_some() {
            body_type = Some(if p.ty.is_empty() { "Object".to_owned() } else { p.ty.clone() });
        }
    }
    let conditions = |key: &str| {
        let mut all = c.strings(class_mapping.and_then(|m| m.arg(key)), false);
        all.extend(c.strings(ann.arg(key), false));
        all
    };
    // `params = "a=1"` / `headers = "X-Api=1"` narrow the mapping: the request must carry them.
    for cond in conditions("params") {
        let (name, value) = condition(&cond);
        if !name.is_empty() && !cond.contains('!') && !query.iter().any(|q| q.name == name.trim()) {
            query.push(QueryParam { name: name.trim().to_owned(), value: value.map(|v| v.trim().to_owned()) });
        }
    }
    for cond in conditions("headers") {
        let (name, value) = condition(&cond);
        if let Some(value) = value.filter(|_| !name.is_empty() && !cond.contains('!')) {
            headers.push(Header { name: name.trim().to_owned(), value: value.trim().to_owned() });
        }
    }
    let first =
        |key: &str| c.string(ann.arg(key), false).or_else(|| class_mapping.and_then(|m| c.string(m.arg(key), false)));
    Some(Endpoint {
        class_name: cls.name.clone(),
        method_name: method.name.clone(),
        offset: method.name_start,
        range: method.start..method.end,
        class_offset: cls.name_start,
        http_method: http_method.to_uppercase(),
        paths,
        query,
        headers,
        consumes: first("consumes"),
        produces: first("produces"),
        body_type,
    })
}

/// The controllers of a parsed file, with their endpoints.
pub fn controllers_of(file: &SourceFile) -> Vec<Controller> {
    let constants = Constants::new(file);
    all_classes(file)
        .into_iter()
        .filter(|cls| cls.annotations.iter().any(|a| a.name == "RestController" || a.name == "Controller"))
        .map(|cls| Controller {
            name: cls.name.clone(),
            offset: cls.name_start,
            range: cls.start..cls.end,
            endpoints: cls.methods.iter().filter_map(|m| endpoint(cls, m, &constants)).collect(),
        })
        .collect()
}

/// Controllers of a Java / Kotlin source, with their endpoints.
pub fn spring_controllers(text: &str) -> Vec<Controller> {
    // Cheap guard: most files are not controllers.
    if !CONTROLLER_GUARD.is_match(text) {
        return Vec::new();
    }
    controllers_of(&parse_source(text))
}

/// The innermost controller and the endpoint whose declaration holds `offset`.
pub fn spring_at(controllers: &[Controller], offset: usize) -> SpringAt<'_> {
    let holds = |r: &Range<usize>| r.start <= offset && offset <= r.end;
    // Nested classes come after their outer one.
    let controller = controllers.iter().rev().find(|c| holds(&c.range));
    let endpoint = controller.and_then(|c| c.endpoints.iter().find(|e| holds(&e.range)));
    SpringAt { controller, endpoint }
}

#[cfg(test)]
pub(super) mod fixtures {
    pub const JAVA: &str = r#"package com.example.demo.web;

import com.example.demo.dto.CreateUser;
import org.springframework.http.MediaType;
import org.springframework.web.bind.annotation.*;

/** Users { not a brace } */
@RestController
@RequestMapping(UserController.BASE + "/users")
public class UserController {
    static final String BASE = "/api";
    private static final String BY_ID = "/{id}";
    private final UserService service;

    public UserController(UserService service) { this.service = service; }

    @GetMapping
    public List<User> list(@RequestParam(defaultValue = "0") int page, @RequestParam("q") String query,
                           @RequestParam Map<String, String> all) {
        return service.list(); // "}" in a comment
    }

    @GetMapping(BY_ID)
    public User get(@PathVariable Long id, @RequestHeader("X-Trace") String trace) {
        if (id == null) { throw new IllegalArgumentException("{"); }
        return service.get(id);
    }

    @PostMapping(value = {"", "/new"}, consumes = MediaType.APPLICATION_JSON_VALUE, produces = "application/json")
    public User create(@Valid @RequestBody CreateUser body) { return service.create(body); }

    @RequestMapping(path = "/{id:\\d+}/avatar", method = RequestMethod.PUT)
    public void avatar(@PathVariable("id") long id, @RequestBody byte[] data) {}

    @DeleteMapping(path = BY_ID, params = "force=true", headers = "X-Api=1")
    public void delete(@PathVariable Long id) {}

    @org.springframework.web.bind.annotation.PatchMapping("/{id}/name")
    public void rename(@PathVariable Long id, @RequestBody Rename rename) {}

    public void helper() {}

    record Rename(String name, boolean notify) {}
}
"#;

    pub const KOTLIN: &str = r#"package com.example.demo.web

import org.springframework.web.bind.annotation.*

const val API = "/api/v2"

@RestController
@RequestMapping(API)
class OrderController(private val service: OrderService) {
    companion object {
        const val ORDERS = "/orders"
    }

    @GetMapping("${ORDERS}/{id}", ORDERS + "/by-id/{id}")
    fun get(@PathVariable id: Long): Order = service.get(id)

    @PostMapping(ORDERS)
    suspend fun create(@RequestBody order: NewOrder, @RequestParam(required = false) dryRun: Boolean?): Order {
        return service.create(order)
    }

    @RequestMapping(value = [ORDERS + "/search"], method = [RequestMethod.POST, RequestMethod.GET])
    fun search(@RequestParam("q") query: String) = service.search(query)
}

data class NewOrder(val item: String, val quantity: Int, val tags: List<String> = emptyList())
"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::{JAVA, KOTLIN};
    use super::*;
    use crate::everywhere::open::byte_at_utf16;

    fn by_method<'a>(endpoints: &'a [Endpoint], name: &str) -> &'a Endpoint {
        endpoints.iter().find(|e| e.method_name == name).expect("endpoint")
    }

    fn q(name: &str, value: Option<&str>) -> QueryParam {
        QueryParam { name: name.to_owned(), value: value.map(str::to_owned) }
    }

    fn h(name: &str, value: &str) -> Header {
        Header { name: name.to_owned(), value: value.to_owned() }
    }

    fn java() -> Controller {
        spring_controllers(JAVA).into_iter().next().expect("controller")
    }

    fn at(text: &str, offset: usize) -> &str {
        &text[byte_at_utf16(text, offset)..]
    }

    #[test]
    fn finds_the_controller_and_its_mapped_methods_only() {
        let ctrl = java();
        assert_eq!(ctrl.name, "UserController");
        let names: Vec<&str> = ctrl.endpoints.iter().map(|e| e.method_name.as_str()).collect();
        assert_eq!(names, ["list", "get", "create", "avatar", "delete", "rename"]);
        assert!(at(JAVA, by_method(&ctrl.endpoints, "get").offset).starts_with("get("));
        assert!(at(JAVA, ctrl.offset).starts_with("UserController {"));
    }

    #[test]
    fn combines_class_and_method_paths_resolving_constants_of_the_file() {
        let eps = java().endpoints;
        assert_eq!(by_method(&eps, "list").paths, ["/api/users"]);
        assert_eq!(by_method(&eps, "get").paths, ["/api/users/{id}"]);
        assert_eq!(by_method(&eps, "create").paths, ["/api/users", "/api/users/new"]);
        assert_eq!(by_method(&eps, "rename").paths, ["/api/users/{id}/name"]);
    }

    #[test]
    fn reads_http_methods_from_shortcuts_and_request_mapping_method() {
        let methods: Vec<String> = java().endpoints.into_iter().map(|e| e.http_method).collect();
        assert_eq!(methods, ["GET", "GET", "POST", "PUT", "DELETE", "PATCH"]);
    }

    #[test]
    fn reads_request_parameters_headers_bodies_consumes_and_produces() {
        let eps = java().endpoints;
        assert_eq!(by_method(&eps, "list").query, [q("page", Some("0")), q("q", None)]);
        assert_eq!(by_method(&eps, "get").headers, [h("X-Trace", "{{X-Trace}}")]);
        let create = by_method(&eps, "create");
        assert_eq!(create.body_type.as_deref(), Some("CreateUser"));
        assert_eq!(create.consumes.as_deref(), Some("application/json"));
        assert_eq!(create.produces.as_deref(), Some("application/json"));
        let delete = by_method(&eps, "delete");
        assert_eq!(delete.query, [q("force", Some("true"))]);
        assert_eq!(delete.headers, [h("X-Api", "1")]);
    }

    #[test]
    fn finds_mappings_with_companion_and_top_level_constants_arrays_and_suspend_functions() {
        let ctrl = spring_controllers(KOTLIN).into_iter().next().expect("controller");
        assert_eq!(ctrl.name, "OrderController");
        let summary: Vec<(&str, &str, Vec<&str>)> = ctrl
            .endpoints
            .iter()
            .map(|e| (e.method_name.as_str(), e.http_method.as_str(), e.paths.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            summary,
            [
                ("get", "GET", vec!["/api/v2/orders/{id}", "/api/v2/orders/by-id/{id}"]),
                ("create", "POST", vec!["/api/v2/orders"]),
                ("search", "POST", vec!["/api/v2/orders/search"]),
            ]
        );
        let create = by_method(&ctrl.endpoints, "create");
        assert_eq!(create.body_type.as_deref(), Some("NewOrder"));
        assert_eq!(create.query, [q("dryRun", None)]);
        assert_eq!(by_method(&ctrl.endpoints, "search").query, [q("q", None)]);
    }

    #[test]
    fn ignores_classes_that_are_not_controllers_and_files_without_any() {
        assert!(spring_controllers(r#"@Service class A { @GetMapping("/x") void a() {} }"#).is_empty());
        assert!(spring_controllers("class A {}").is_empty());
        let found =
            spring_controllers("@FeignClient interface C { @GetMapping(\"/x\") String a(); }\n@Controller class B {}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "B");
        assert!(found[0].endpoints.is_empty());
    }

    #[test]
    fn uses_get_for_a_request_mapping_without_method_and_the_class_level_method_otherwise() {
        let c = spring_controllers(concat!(
            r#"@Controller @RequestMapping(value="/a", method=RequestMethod.POST) "#,
            r#"class A { @RequestMapping("/b") String b() { return ""; } }"#,
        ));
        assert_eq!(c[0].endpoints[0].http_method, "POST");
        assert_eq!(c[0].endpoints[0].paths, ["/a/b"]);
        let d = spring_controllers(r#"@RestController class D { @RequestMapping("/b") String b() { return ""; } }"#);
        assert_eq!(d[0].endpoints[0].http_method, "GET");
        assert_eq!(d[0].endpoints[0].paths, ["/b"]);
    }

    #[test]
    fn finds_the_endpoint_whose_declaration_annotations_included_holds_the_caret_and_its_controller() {
        let controllers = spring_controllers(JAVA);
        let offset = |needle: &str| JAVA[..JAVA.find(needle).expect("needle")].encode_utf16().count();
        let at = |needle: &str| spring_at(&controllers, offset(needle));
        assert_eq!(at("@GetMapping(BY_ID)").endpoint.map(|e| e.method_name.as_str()), Some("get"));
        assert_eq!(at("service.get(id)").endpoint.map(|e| e.method_name.as_str()), Some("get"));
        assert!(at("public void helper").endpoint.is_none());
        assert_eq!(at("public void helper").controller.map(|c| c.name.as_str()), Some("UserController"));
        assert_eq!(spring_at(&controllers, 0), SpringAt::default());
    }
}
