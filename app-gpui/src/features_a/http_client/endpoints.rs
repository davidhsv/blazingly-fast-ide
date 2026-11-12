//! The project's HTTP endpoints in `.http` files: request URLs completed from what the endpoint
//! providers report, and an endpoint rendered as a request block.
//!
//! Providers answer per project root; the caller asks every registered provider and hands the
//! answers back, a failed provider contributing nothing.

use super::highlight::METHODS;
use crate::everywhere::open::{byte_at_utf16, utf16_len};
use crate::everywhere::routes::EndpointEntry;
use regex::{Captures, Regex};
use std::collections::HashSet;
use std::sync::LazyLock;

static ENDPOINT_VARIABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([A-Za-z0-9_.-]+)\}").expect("valid regex"));
static REQUEST_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^\s*({})\s+(\S*)$", METHODS.join("|"))).expect("valid regex"));
static SEPARATOR_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*###.*$").expect("valid regex"));
static REQUEST_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^\s*(?:{})\s+|^\s*https?://", METHODS.join("|"))).expect("valid regex"));
static ORIGIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:https?://[^/\s]+|\{\{[^}]+\}\})").expect("valid regex"));

/// `{var}` path variables as `{{var}}`; `{{var}}` stays as it is.
pub fn endpoint_url(path: &str) -> String {
    ENDPOINT_VARIABLE
        .replace_all(path, |caps: &Captures| {
            let whole = caps.get(0).map_or("", |m| m.as_str());
            let end = caps.get(0).map_or(0, |m| m.end());
            if path[end..].starts_with('}') {
                whole.to_owned()
            } else {
                format!("{{{{{}}}}}", caps.get(1).map_or("", |m| m.as_str()))
            }
        })
        .into_owned()
}

/// An endpoint as a request block on the default local server.
pub fn render_endpoint(e: &EndpointEntry) -> String {
    let title = e.handler.clone().unwrap_or_else(|| format!("{} {}", e.method, e.url));
    let method = if e.method == "*" { "GET" } else { e.method.as_str() };
    format!("### {title}\n{method} http://localhost:8080{}\n", endpoint_url(&e.url))
}

/// Where a request URL is being completed: UTF-16 offset of the completed part, the request's
/// method and the path typed so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionTarget {
    pub from: usize,
    pub method: String,
    pub prefix: String,
}

/// Only the URL of the first request line in a request block is completed; `pos` is a UTF-16
/// offset into `text`.
pub fn completion_target(text: &str, pos: usize) -> Option<CompletionTarget> {
    let before = &text[..byte_at_utf16(text, pos)];
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    let caps = REQUEST_LINE.captures(&before[start..])?;
    let block = SEPARATOR_LINE.split(&before[..start]).last().unwrap_or_default();
    if block.split('\n').any(|l| REQUEST_START.is_match(l)) {
        return None;
    }
    let method = caps.get(1)?.as_str();
    let url = caps.get(2).map_or("", |m| m.as_str());
    let origin = ORIGIN
        .find(url)
        .map(|m| m.as_str())
        .filter(|origin| url[origin.len()..].is_empty() || url[origin.len()..].starts_with('/'))
        .unwrap_or_default();
    if !url.is_empty() && origin.is_empty() && !url.starts_with('/') {
        return None;
    }
    Some(CompletionTarget {
        from: utf16_len(before) - utf16_len(url) + utf16_len(origin),
        method: method.to_owned(),
        prefix: url[origin.len()..].to_owned(),
    })
}

/// A completion option for a request URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointCompletion {
    pub label: String,
    pub detail: Option<String>,
}

/// The endpoints that complete `target`: same method (or any), URL starting with the prefix, each
/// URL once.
pub fn endpoint_completions(target: &CompletionTarget, endpoints: &[EndpointEntry]) -> Vec<EndpointCompletion> {
    let mut seen = HashSet::new();
    endpoints
        .iter()
        .filter(|e| (e.method == "*" || e.method == target.method) && e.url.starts_with(&target.prefix))
        .filter(|e| seen.insert(e.url.as_str()))
        .map(|e| EndpointCompletion {
            label: endpoint_url(&e.url),
            detail: e.handler.clone().or_else(|| e.framework.clone()),
        })
        .collect()
}

/// The registered endpoint providers, by id.
#[derive(Debug, Default)]
pub struct EndpointProviders {
    ids: Vec<String>,
}

impl EndpointProviders {
    /// Registers a provider; a provider with the same id is replaced.
    pub fn register(&mut self, id: &str) {
        if !self.ids.iter().any(|i| i == id) {
            self.ids.push(id.to_owned());
        }
    }

    pub fn unregister(&mut self, id: &str) {
        self.ids.retain(|i| i != id);
    }

    /// The providers to ask for the endpoints of `root`; none without a project.
    pub fn to_ask(&self, root: Option<&str>) -> Vec<String> {
        if root.is_some() { self.ids.clone() } else { Vec::new() }
    }

    /// The endpoints the providers answered: failed providers and providers unregistered meanwhile
    /// contribute nothing.
    pub fn collect(&self, answers: Vec<(String, Result<Vec<EndpointEntry>, String>)>) -> Vec<EndpointEntry> {
        answers
            .into_iter()
            .filter(|(id, _)| self.ids.contains(id))
            .filter_map(|(_, answer)| answer.ok())
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> EndpointEntry {
        EndpointEntry {
            method: "GET".to_owned(),
            url: "/users/{id}".to_owned(),
            method_name: "users.show".to_owned(),
            handler: Some("users.show".to_owned()),
            path: "/p/routes.txt".to_owned(),
            ..EndpointEntry::default()
        }
    }

    #[test]
    fn completes_only_request_urls_preserving_origins_and_variables() {
        for url in ["/us", "http://localhost:8000/us", "https://example.com/us", "{{host}}/us"] {
            let text = format!("### Users\nGET {url}");
            let result = completion_target(&text, utf16_len(&text)).expect("target");
            assert_eq!(&text[result.from..], "/us", "{url}");
            assert_eq!(result.method, "GET");
        }
        let rejected =
            ["GET /users\nX-Test: /us", "POST /users\n\nGET /us", "# GET /us", "@host = /us", "GET /users HTTP/1.1"];
        for text in rejected {
            assert_eq!(completion_target(text, utf16_len(text)), None, "{text}");
        }
        assert_eq!(completion_target("GET /old\n### Next\nPOST /us", 28).map(|t| t.method), Some("POST".to_owned()));
    }

    #[test]
    fn renders_optional_handlers_and_any_method_routes_into_http_blocks() {
        assert_eq!(render_endpoint(&route()), "### users.show\nGET http://localhost:8080/users/{{id}}\n");
        let any = EndpointEntry { method: "*".to_owned(), handler: None, ..route() };
        assert_eq!(render_endpoint(&any), "### * /users/{id}\nGET http://localhost:8080/users/{{id}}\n");
    }

    #[test]
    fn scopes_providers_and_removes_their_completion_entries_on_disposal() {
        let mut providers = EndpointProviders::default();
        providers.register("test.routes");
        providers.register("test.failed");
        let answer = |root: &str, id: &str| -> Result<Vec<EndpointEntry>, String> {
            match id {
                "test.routes" if root == "/p" => {
                    Ok(vec![route(), EndpointEntry { method: "POST".to_owned(), ..route() }])
                }
                "test.routes" => Ok(Vec::new()),
                _ => Err("provider unavailable".to_owned()),
            }
        };
        let search = |providers: &EndpointProviders, root: &str| {
            let answers = providers.to_ask(Some(root)).into_iter().map(|id| (id.clone(), answer(root, &id))).collect();
            providers.collect(answers)
        };
        let target = completion_target("GET {{host}}/us", 15).expect("target");
        let labels = |endpoints: &[EndpointEntry]| {
            endpoint_completions(&target, endpoints).into_iter().map(|c| c.label).collect::<Vec<_>>()
        };
        assert_eq!(labels(&search(&providers, "/p")), ["/users/{{id}}"]);
        assert!(search(&providers, "/other").is_empty());
        assert!(providers.to_ask(None).is_empty());
        providers.unregister("test.routes");
        assert!(labels(&search(&providers, "/p")).is_empty());
    }
}
