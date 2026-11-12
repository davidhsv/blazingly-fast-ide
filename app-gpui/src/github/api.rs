//! GitHub API access through gh, like IntelliJ's `GithubApiRequestExecutor`: the backend's
//! `gh_api` command runs `gh api` in the connected local repository with gh's authentication and
//! answers with the response body. This module builds the [`GhApiRequest`]s and reads the answers.
//!
//! GraphQL documents are sent with every fragment they spread (`...name`), transitively, like
//! IntelliJ's `CachingGraphQLQueryLoader`; fragments a document defines itself are not added again.
//! Requests address the connected repository explicitly ([`repo_path`], [`repo_variables`]), not
//! gh's `{owner}/{repo}` placeholders, since a project can have several GitHub remotes.

use super::graphql::fragment;
use super::types::{Coordinates, TimelineItem};
use ide_github::GhApiRequest;
use regex::Regex;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Documents

static FRAGMENT_DEFINITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^fragment\s+(\S+)\s+on\s+.*\{$").expect("valid regex"));
/// `...name`; `... on User {` is an inline fragment, not a spread.
static SPREAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\.\.\.([A-Za-z_][0-9A-Za-z_]*)").expect("valid regex"));

struct Block {
    body: String,
    /// Fragments spread in the block and not defined in it, in order of first spread.
    dependencies: Vec<String>,
    /// Fragments the block defines itself.
    defines: Vec<String>,
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|n| n == name) {
        list.push(name.to_owned());
    }
}

fn read_block(text: &str) -> Block {
    let mut dependencies = Vec::new();
    let mut defines = Vec::new();
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim();
        lines.push(line);
        if let Some(def) = FRAGMENT_DEFINITION.captures(line) {
            push_unique(&mut defines, &def[1]);
        }
        if let Some(spread) = SPREAD.captures(line) {
            push_unique(&mut dependencies, &spread[1]);
        }
    }
    dependencies.retain(|d| !defines.contains(d));
    Block { body: lines.join("\n").trim().to_owned(), dependencies, defines }
}

/// A document with the fragments it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDocument {
    /// The fragments (dependencies before what spreads them), then the document.
    pub text: String,
    /// Names of the added fragments, in discovery order.
    pub fragments: Vec<String>,
}

/// `doc` with every fragment it spreads, transitively; an error names a fragment that does not exist.
pub fn resolve_document(doc: &str) -> Result<ResolvedDocument, String> {
    fn visit(names: &[String], defines: &[String], found: &mut Vec<(String, Block)>) -> Result<(), String> {
        for name in names {
            if found.iter().any(|(n, _)| n == name) || defines.contains(name) {
                continue;
            }
            let text = fragment(name).ok_or_else(|| format!("Couldn't find file for fragment {name}"))?;
            let block = read_block(text);
            let dependencies = block.dependencies.clone();
            found.push((name.clone(), block));
            visit(&dependencies, defines, found)?;
        }
        Ok(())
    }
    let block = read_block(doc);
    let mut found: Vec<(String, Block)> = Vec::new();
    visit(&block.dependencies, &block.defines, &mut found)?;
    let fragments = found.iter().map(|(n, _)| n.clone()).collect();
    let mut parts: Vec<&str> = found.iter().rev().map(|(_, b)| b.body.as_str()).collect();
    parts.push(&block.body);
    Ok(ResolvedDocument { text: parts.join("\n"), fragments })
}

/// `doc` preceded by the fragments it needs, ready to send.
pub fn with_fragments(doc: &str) -> Result<String, String> {
    resolve_document(doc).map(|r| r.text)
}

/// Names of the fragments [`with_fragments`] adds, in discovery order.
pub fn fragments_of(doc: &str) -> Result<Vec<String>, String> {
    resolve_document(doc).map(|r| r.fragments)
}

// ---------------------------------------------------------------------------
// Requests

/// A GraphQL response with `errors` (IntelliJ's `GithubConfusingException`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphQlError {
    pub message: String,
    pub errors: Vec<GraphQlErrorEntry>,
}

impl std::fmt::Display for GraphQlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GraphQlError {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct GraphQlErrorEntry {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlErrorEntry>,
}

/// `data` of a GraphQL response body; an error for `errors` (gh usually fails the call first),
/// for no data, and for a body that does not have the expected shape.
pub fn graphql_data<T: DeserializeOwned>(text: &str) -> Result<T, GraphQlError> {
    let response: GraphQlResponse<T> = serde_json::from_str(text)
        .map_err(|e| GraphQlError { message: format!("unexpected GitHub answer: {e}"), errors: Vec::new() })?;
    if !response.errors.is_empty() {
        let message = response.errors.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join("\n");
        return Err(GraphQlError { message, errors: response.errors });
    }
    response.data.ok_or_else(|| GraphQlError { message: "GitHub returned no data".into(), errors: Vec::new() })
}

/// The `gh_api` request running the document `doc` with `variables` (`None` values are left out).
pub fn graphql_request(doc: &str, variables: &[(&str, Option<Value>)]) -> Result<GhApiRequest, String> {
    let variables: Map<String, Value> =
        variables.iter().filter_map(|(k, v)| v.clone().map(|v| ((*k).to_owned(), v))).collect();
    let body = serde_json::json!({ "query": with_fragments(doc)?, "variables": variables });
    Ok(GhApiRequest {
        method: "POST".into(),
        endpoint: "graphql".into(),
        body: Some(body.to_string()),
        paginate: false,
    })
}

/// A REST request (`endpoint` relative to the API root, e.g. `repos/o/r/pulls/1/files`). `paginate`
/// follows every `Link` page; [`parse_rest`] concatenates array pages.
pub fn rest_request(method: &str, endpoint: &str, body: Option<&Value>, paginate: bool) -> GhApiRequest {
    GhApiRequest {
        method: method.to_owned(),
        endpoint: endpoint.to_owned(),
        body: body.map(Value::to_string),
        paginate,
    }
}

/// Parses a REST body: `--slurp` pages (an array of arrays) are flattened; an empty body
/// (`204 No Content`) is `None`.
pub fn parse_rest(text: &str, paginated: bool) -> serde_json::Result<Option<Value>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let json: Value = serde_json::from_str(text)?;
    Ok(Some(match json {
        Value::Array(pages) if paginated && pages.iter().all(Value::is_array) => Value::Array(
            pages.into_iter().flat_map(|p| if let Value::Array(items) = p { items } else { Vec::new() }).collect(),
        ),
        other => other,
    }))
}

/// `repos/owner/name`, the prefix of a repository's REST endpoints.
pub fn repo_path(repo: &Coordinates) -> String {
    format!("repos/{}/{}", repo.owner, repo.name)
}

/// The `$repoOwner` / `$repoName` variables of the repository documents.
pub fn repo_variables(repo: &Coordinates) -> [(&'static str, Option<Value>); 2] {
    [("repoOwner", Some(Value::from(repo.owner.as_str()))), ("repoName", Some(Value::from(repo.name.as_str())))]
}

// ---------------------------------------------------------------------------
// Timeline items

/// Timeline item types IntelliJ drops (`GHPRTimelineItem.IGNORED_TYPES`).
const IGNORED_TIMELINE_TYPES: [&str; 7] = [
    "ReferencedEvent",
    "MentionedEvent",
    "CommentDeletedEvent",
    "LockedEvent",
    "UnlockedEvent",
    "SubscribedEvent",
    "UnsubscribedEvent",
];

/// Item types the timeline document selects fields for.
const TIMELINE_TYPES: [&str; 24] = [
    "PullRequestCommit",
    "PullRequestReview",
    "IssueComment",
    "RenamedTitleEvent",
    "ClosedEvent",
    "ReopenedEvent",
    "MergedEvent",
    "AssignedEvent",
    "UnassignedEvent",
    "LabeledEvent",
    "UnlabeledEvent",
    "ReviewRequestedEvent",
    "ReviewRequestRemovedEvent",
    "ReviewDismissedEvent",
    "ReadyForReviewEvent",
    "ConvertToDraftEvent",
    "BaseRefChangedEvent",
    "BaseRefForcePushedEvent",
    "HeadRefForcePushedEvent",
    "HeadRefDeletedEvent",
    "HeadRefRestoredEvent",
    "CrossReferencedEvent",
    "ConnectedEvent",
    "DisconnectedEvent",
];

/// The timeline's nodes as items: IntelliJ's ignored types dropped, types the document selects no
/// fields for kept as [`TimelineItem::Unknown`].
pub fn timeline_items(nodes: Vec<Value>) -> serde_json::Result<Vec<TimelineItem>> {
    let mut out = Vec::new();
    for node in nodes {
        let type_name = node.get("__typename").and_then(Value::as_str).unwrap_or_default().to_owned();
        if IGNORED_TIMELINE_TYPES.contains(&type_name.as_str()) {
            continue;
        }
        if TIMELINE_TYPES.contains(&type_name.as_str()) {
            out.push(serde_json::from_value(node)?);
        } else {
            out.push(TimelineItem::Unknown { type_name });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::graphql::{FRAGMENTS, QUERIES, query};
    use std::collections::HashSet;

    fn definitions(text: &str) -> Vec<String> {
        static DEF: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^\s*fragment\s+(\S+)\s+on\s").expect("valid regex"));
        text.split('\n').filter_map(|l| DEF.captures(l).map(|c| c[1].to_owned())).collect()
    }

    fn spreads(text: &str) -> HashSet<String> {
        static SP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^\s*\.\.\.([A-Za-z_][0-9A-Za-z_]*)").expect("valid regex"));
        text.split('\n').filter_map(|l| SP.captures(l).map(|c| c[1].to_owned())).collect()
    }

    fn doc(name: &str) -> &'static str {
        query(name).expect("a known query")
    }

    #[test]
    fn has_every_intellij_fragment_and_query() {
        assert_eq!(FRAGMENTS.len(), 39);
        assert_eq!(QUERIES.len(), 40);
        for (name, text) in FRAGMENTS {
            assert_eq!(definitions(text), [name], "{name}");
        }
    }

    #[test]
    fn sends_every_query_with_every_fragment_it_needs_once() {
        for (name, doc) in QUERIES {
            let text = with_fragments(doc).expect(name);
            let defined = definitions(&text);
            assert_eq!(defined.iter().collect::<HashSet<_>>().len(), defined.len(), "{name}");
            for s in spreads(&text) {
                assert!(defined.contains(&s), "{name}: {s}");
            }
            // Only fragments that are spread somewhere are added.
            let spread = spreads(&text);
            for d in fragments_of(doc).expect(name) {
                assert!(spread.contains(&d), "{name}: {d}");
            }
        }
    }

    #[test]
    fn resolves_fragments_transitively_like_ghgql_query_loader() {
        let names = fragments_of(doc("issueSearch")).expect("resolves");
        for expected in [
            "pageInfo",
            "pullRequestInfoShort",
            "rates",
            "nodeInfo",
            "actor",
            "actorInfo",
            "userInfo",
            "user",
            "labelInfo",
            "pullRequestReviewer",
            "teamInfo",
            "pullRequestReview",
            "reactions",
            "reaction",
            "enterpriseUserAccountInfo",
            "organizationInfo",
        ] {
            assert!(names.iter().any(|n| n == expected), "{expected}");
        }
        assert!(!names.iter().any(|n| n == "repository"));
        // Fragments go first, the operation last.
        let text = with_fragments(doc("issueSearch")).expect("resolves");
        let text = text.trim();
        assert!(text.starts_with("fragment"));
        assert!(text.contains("\nquery issueSearch"));
        assert!(text.ends_with('}'));
    }

    #[test]
    fn does_not_add_fragments_a_document_defines_itself() {
        let timeline = doc("pullRequestTimeline");
        let text = with_fragments(timeline).expect("resolves");
        assert_eq!(definitions(&text).iter().filter(|d| *d == "gitRefName").count(), 1);
        let names = fragments_of(timeline).expect("resolves");
        assert!(!names.iter().any(|n| n == "gitRefName"));
        assert!(!names.iter().any(|n| n == "issueCommentInfo"));
    }

    #[test]
    fn treats_inline_fragments_as_selections_not_spreads() {
        let names = fragments_of(doc("findPendingReview")).expect("resolves");
        assert!(names.iter().any(|n| n == "pullRequestPendingReview"));
        assert!(names.iter().any(|n| n == "nodeInfo"));
        assert_eq!(fragments_of("query { node(id: \"x\") { ... on User { id } } }"), Ok(Vec::new()));
    }

    #[test]
    fn fails_on_an_unknown_fragment() {
        let err = with_fragments("query {\n  viewer {\n    ...nope\n  }\n}").expect_err("unknown fragment");
        assert!(err.contains("nope"));
    }

    #[test]
    fn returns_data_and_throws_graphql_errors() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct A {
            a: i32,
        }
        assert_eq!(graphql_data::<A>(r#"{"data":{"a":1}}"#), Ok(A { a: 1 }));
        let err = graphql_data::<A>(r#"{"errors":[{"message":"bad"},{"message":"worse"}]}"#).expect_err("errors");
        assert_eq!(err.errors.len(), 2);
        assert_eq!(graphql_data::<A>(r#"{"errors":[{"message":"bad"}]}"#).map_err(|e| e.message), Err("bad".into()));
    }

    #[test]
    fn flattens_slurped_rest_pages_and_maps_empty_bodies_to_null() {
        assert_eq!(parse_rest("[[1,2],[3]]", true).ok().flatten(), Some(serde_json::json!([1, 2, 3])));
        assert_eq!(parse_rest(r#"[{"a":1}]"#, false).ok().flatten(), Some(serde_json::json!([{ "a": 1 }])));
        assert_eq!(parse_rest("  ", false).ok(), Some(None));
    }

    #[test]
    fn sends_documents_with_their_fragments_through_gh() {
        let mut variables = repo_variables(&Coordinates::new("o", "r")).to_vec();
        variables.push(("cursor", None));
        let req = graphql_request(doc("findRepository"), &variables).expect("resolves");
        assert_eq!((req.method.as_str(), req.endpoint.as_str(), req.paginate), ("POST", "graphql", false));
        let body: Value = serde_json::from_str(req.body.as_deref().unwrap_or_default()).expect("JSON body");
        assert_eq!(body["variables"], serde_json::json!({ "repoOwner": "o", "repoName": "r" }));
        let defined = definitions(body["query"].as_str().unwrap_or_default());
        for name in ["repository", "repositoryOwnerName", "rates"] {
            assert!(defined.iter().any(|d| d == name), "{name}");
        }
    }

    #[test]
    fn drops_intellijs_ignored_types_and_marks_unknown_ones() {
        let nodes = ["IssueComment", "MentionedEvent", "SubscribedEvent", "AutoMergeEnabledEvent"]
            .map(|t| serde_json::json!({ "__typename": t }))
            .to_vec();
        let items = timeline_items(nodes).expect("reads");
        assert_eq!(items.iter().map(TimelineItem::type_name).collect::<Vec<_>>(), ["IssueComment", "Unknown"]);
        assert_eq!(items[1], TimelineItem::Unknown { type_name: "AutoMergeEnabledEvent".into() });
    }
}
