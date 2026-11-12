//! Request-path and controller-method ranking of the project's endpoints for Search Everywhere.

use super::model::{SymbolHits, SymbolItem};
use super::open::utf16_len;
use super::rank::{MatchOptions, match_text};
use crate::platform::collate;
use ide_core::SymbolKind;

/// An endpoint of the project, as an endpoint provider (a framework plugin) reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointEntry {
    /// `GET`, `POST`, …; `*` for any method.
    pub method: String,
    pub framework: Option<String>,
    /// What handles the request when it is not a controller method (`Menus#search`).
    pub handler: Option<String>,
    pub plugin_id: Option<String>,
    /// Class-level and method-level mapping joined; `{var}` kept.
    pub url: String,
    pub controller: String,
    pub method_name: String,
    pub path: String,
    pub rel: String,
    /// 0-based line of the handler.
    pub line: u32,
    /// UTF-16 column of the handler.
    pub col: u32,
}

const HTTP_METHODS: [&str; 8] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"];

/// `path` without the slashes around it.
fn strip(path: &str) -> &str {
    path.trim_matches('/')
}

/// What answers the endpoint: its handler, else `Controller.method`.
fn qualified(route: &EndpointEntry) -> String {
    match &route.handler {
        Some(handler) => handler.clone(),
        None => {
            let parts = [route.controller.as_str(), route.method_name.as_str()];
            parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(".")
        }
    }
}

/// Byte offsets in `text` of the characters behind the last place `needle` appears in it, whatever the case
/// of `text`; `needle` is lower case. Empty when it does not appear.
fn last_occurrence(text: &str, needle: &str) -> Vec<usize> {
    let mut lowered = String::with_capacity(text.len());
    // Per byte of `lowered`, the offset in `text` of the character it was lowercased from.
    let mut sources = Vec::with_capacity(text.len());
    for (offset, c) in text.char_indices() {
        lowered.extend(c.to_lowercase());
        sources.resize(lowered.len(), offset);
    }
    let Some(at) = lowered.rfind(needle) else { return Vec::new() };
    let mut offsets = sources[at..at + needle.len()].to_vec();
    offsets.dedup();
    offsets
}

fn item(route: &EndpointEntry, indices: Vec<usize>, score: i32) -> SymbolItem {
    let display = format!("{} {}", route.method, route.url);
    SymbolItem {
        key: format!("r:{}:{}:{}:{}", route.path, route.line, route.method, route.url),
        name: display.clone(),
        display,
        symbol_kind: SymbolKind::Method,
        container: Some(qualified(route)),
        path: route.path.clone(),
        rel: route.rel.clone(),
        line: route.line,
        col: route.col,
        indices,
        lsp: false,
        score: Some(score),
    }
}

/// Routes matching `query`: a request path (`/deeplink/menu/search`, with or without the servlet context path
/// in front), an optional `POST ` prefix, or the controller / method name. Scores: exact path, then a path
/// suffix (a context path in front of it), then a substring, then the symbol names, then a fuzzy path hit.
/// The matched path is highlighted in the displayed `METHOD /url`.
pub fn match_routes(routes: &[EndpointEntry], query: &str, limit: usize) -> SymbolHits {
    let text = query.trim();
    if text.is_empty() || routes.is_empty() {
        return SymbolHits::default();
    }
    let mut words = text.split_whitespace();
    let head = words.next().unwrap_or_default().to_uppercase();
    let method = HTTP_METHODS.contains(&head.as_str()).then_some(head);
    let rest = if method.is_some() { words.collect::<Vec<_>>().join(" ") } else { text.to_owned() };
    let want_path = strip(&rest).to_lowercase();
    let want_name = rest.to_lowercase();

    let mut hits: Vec<SymbolItem> = Vec::new();
    for route in routes {
        if method.as_ref().is_some_and(|method| route.method != "*" && route.method != *method) {
            continue;
        }
        let target = strip(&route.url).to_lowercase();
        let mut score = 0;
        let mut indices = Vec::new();
        if !want_path.is_empty() {
            let nested = want_path.contains('/')
                && (target.ends_with(&format!("/{want_path}")) || want_path.ends_with(&format!("/{target}")));
            let long_enough = utf16_len(&want_path) >= 3 && utf16_len(&target) >= 3;
            if target == want_path {
                score = 1000;
            } else if nested {
                score = 900;
            } else if long_enough && (target.contains(&want_path) || want_path.contains(&target)) {
                score = 700;
            }
            if score != 0 {
                indices = last_occurrence(&format!("{} {}", route.method, route.url), &want_path);
            }
        }
        if score == 0 && !want_name.is_empty() {
            if route.method_name.to_lowercase() == want_name {
                score = 800;
            } else if qualified(route).to_lowercase().contains(&want_name) {
                score = 600;
            } else if utf16_len(&want_name) >= 3 && target.contains(&want_name) {
                score = 500;
            } else if let Some(fuzzy) = match_text(&target, &want_name, MatchOptions::default()) {
                score = 300 + fuzzy.score;
            }
        }
        if score != 0 {
            hits.push(item(route, indices, score));
        }
    }
    // Stable: routes that tie on everything keep the order they were reported in.
    hits.sort_by(|a, b| {
        b.score.cmp(&a.score).then_with(|| collate::compare(&a.rel, &b.rel)).then_with(|| a.line.cmp(&b.line))
    });
    let has_more = hits.len() > limit;
    hits.truncate(limit);
    SymbolHits { items: hits, has_more }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> EndpointEntry {
        EndpointEntry {
            method: "POST".to_owned(),
            url: "/deeplink/menu/search".to_owned(),
            controller: "DeepLinkController".to_owned(),
            method_name: "searchMenus".to_owned(),
            path: "/p/tg-tenant-server/src/main/java/DeepLinkController.java".to_owned(),
            rel: "tg-tenant-server/src/main/java/DeepLinkController.java".to_owned(),
            line: 45,
            col: 4,
            ..EndpointEntry::default()
        }
    }

    fn routes() -> Vec<EndpointEntry> {
        let detail = EndpointEntry {
            method: "GET".to_owned(),
            url: "/deeplink/menu/{menuId}".to_owned(),
            method_name: "getMenuDetail".to_owned(),
            line: 52,
            col: 4,
            ..route()
        };
        vec![route(), detail]
    }

    /// The container of the best match.
    fn top(routes: &[EndpointEntry], query: &str) -> Option<String> {
        match_routes(routes, query, 5).items.into_iter().next().and_then(|item| item.container)
    }

    #[test]
    fn finds_any_method_plugin_routes_and_displays_an_optional_handler_or_framework() {
        let entry = EndpointEntry {
            method: "*".to_owned(),
            controller: "Rails".to_owned(),
            method_name: String::new(),
            framework: Some("Rails".to_owned()),
            handler: None,
            ..route()
        };
        assert_eq!(top(std::slice::from_ref(&entry), "GET /deeplink/menu/search").as_deref(), Some("Rails"));
        let handled = EndpointEntry { handler: Some("Menus#search".to_owned()), ..entry };
        assert_eq!(top(&[handled], "Menus#search").as_deref(), Some("Menus#search"));
    }

    #[test]
    fn finds_a_controller_method_by_the_request_path_it_answers() {
        for query in [
            "/deeplink/menu/search",
            "deeplink/menu/search",
            "/tg-tenant/deeplink/menu/search",
            "POST /tg-tenant/deeplink/menu/search",
            "post /deeplink/menu/search",
        ] {
            let found = match_routes(&routes(), query, 5);
            let first = found.items.first().expect(query);
            assert_eq!(first.container.as_deref(), Some("DeepLinkController.searchMenus"), "{query}");
            assert_eq!(first.display, "POST /deeplink/menu/search", "{query}");
        }
    }

    #[test]
    fn honours_an_http_method_prefix() {
        assert!(match_routes(&routes(), "GET /deeplink/menu/x", 5).items.is_empty());
        assert_eq!(top(&routes(), "GET /deeplink/menu").as_deref(), Some("DeepLinkController.getMenuDetail"));
    }

    #[test]
    fn finds_it_by_controller_or_method_name_too() {
        assert_eq!(top(&routes(), "searchMenus").as_deref(), Some("DeepLinkController.searchMenus"));
        assert_eq!(
            top(&routes(), "DeepLinkController.getMenuDetail").as_deref(),
            Some("DeepLinkController.getMenuDetail")
        );
        assert_eq!(match_routes(&routes(), "deeplink", 5).items.len(), 2);
    }

    #[test]
    fn says_nothing_for_an_empty_or_unrelated_query() {
        assert!(match_routes(&routes(), "", 5).items.is_empty());
        assert!(match_routes(&routes(), "   ", 5).items.is_empty());
        assert!(match_routes(&routes(), "unrelated-thing", 5).items.is_empty());
    }

    // Not from a spec: the matched path is highlighted in the displayed route, a route found by name has no
    // highlight, and results beyond the limit are reported.
    #[test]
    fn highlights_the_path_and_reports_overflow() {
        let found = match_routes(&routes(), "menu/search", 1);
        assert_eq!(found.items[0].indices, (15..26).collect::<Vec<usize>>());
        assert_eq!(found.items[0].score, Some(900));
        let key = "r:/p/tg-tenant-server/src/main/java/DeepLinkController.java:45:POST:/deeplink/menu/search";
        assert_eq!(found.items[0].key, key);
        assert!(match_routes(&routes(), "searchMenus", 5).items[0].indices.is_empty());
        let both = match_routes(&routes(), "deeplink", 1);
        assert_eq!((both.items.len(), both.has_more), (1, true));
        assert_eq!(both.items[0].line, 45);
    }

    // Not from a spec: highlights are byte offsets into the displayed route, also when its characters take
    // several bytes or lowercase to more than one character.
    #[test]
    fn highlights_are_byte_offsets_into_the_displayed_route() {
        let cafe = EndpointEntry { url: "/Café/Menü".to_owned(), ..route() };
        let found = match_routes(std::slice::from_ref(&cafe), "café/menü", 5);
        assert_eq!(found.items[0].display, "POST /Café/Menü");
        assert_eq!(found.items[0].indices, [6, 7, 8, 9, 11, 12, 13, 14, 15]);
        assert_eq!(last_occurrence("İx/İx", "x"), [6]);
        assert_eq!(last_occurrence("POST /a", "zzz"), Vec::<usize>::new());
    }
}
