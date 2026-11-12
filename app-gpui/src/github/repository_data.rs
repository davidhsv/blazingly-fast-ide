//! The connected repository's cached lists (IntelliJ's `GHPRRepositoryDataService`):
//! collaborators, contributors, assignees, labels, mentionable users and teams, each loaded
//! incrementally once ([`BatchesLoader`]) until the repository data is reset, and the pull request
//! template (the first non-blank one).
//!
//! REST lists are paged by number, 100 per page (a shorter page is the last); GraphQL ones by
//! cursor. This module builds the page requests and reads the answers; the caller sends them.

use super::api::{graphql_data, parse_rest, repo_path, repo_variables, rest_request};
use super::batches::{BatchesLoader, BatchesState, Page, merge_batches};
use super::data_model::has_permission;
use super::new_comments::document_request;
use super::types::{Connection, Coordinates, GhLabel, GhRepository, GhReviewer, GhTeam, GhUser, Permission};
use ide_github::GhApiRequest;
use serde::Deserialize;
use serde_json::Value;

const REST_PAGE_SIZE: usize = 100;

/// A collaborator's permissions (REST `permissions`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CollaboratorPermissions {
    pub admin: bool,
    pub maintain: bool,
    pub push: bool,
    pub triage: bool,
    pub pull: bool,
}

/// A REST collaborator (IntelliJ's `GHCollaborator`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collaborator {
    pub user: GhUser,
    pub permissions: CollaboratorPermissions,
}

#[derive(Deserialize)]
struct RestUser {
    login: String,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    avatar_url: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    name: Option<String>,
    permissions: Option<CollaboratorPermissions>,
}

impl RestUser {
    fn user(&self) -> GhUser {
        GhUser {
            typename: "User".into(),
            id: self.node_id.clone(),
            login: self.login.clone(),
            url: self.html_url.clone(),
            avatar_url: self.avatar_url.clone(),
            name: self.name.clone(),
        }
    }
}

#[derive(Deserialize)]
struct RestLabel {
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    url: String,
    name: String,
    #[serde(default)]
    color: String,
}

/// The REST lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestList {
    Collaborators,
    Contributors,
    /// Users issues and pull requests can be assigned to.
    Assignees,
    Labels,
}

impl RestList {
    fn segment(self) -> &'static str {
        match self {
            Self::Collaborators => "collaborators",
            Self::Contributors => "contributors",
            Self::Assignees => "assignees",
            Self::Labels => "labels",
        }
    }
}

/// Page `page` (1-based) of a REST list of `repo`.
pub fn rest_page_request(repo: &Coordinates, list: RestList, page: u32) -> GhApiRequest {
    let endpoint = format!("{}/{}?per_page={REST_PAGE_SIZE}&page={page}", repo_path(repo), list.segment());
    rest_request("GET", &endpoint, None, false)
}

fn rest_items<T: for<'de> Deserialize<'de>>(text: &str) -> Result<Vec<T>, String> {
    match parse_rest(text, false).map_err(|e| e.to_string())? {
        None => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
    }
}

fn rest_page<T, R>(page: u32, items: Vec<R>, map: impl Fn(R) -> T) -> Page<T, u32> {
    let next = (items.len() == REST_PAGE_SIZE).then_some(page + 1);
    Page { items: items.into_iter().map(map).collect(), next }
}

/// A page of users (contributors, assignees).
pub fn parse_users_page(page: u32, text: &str) -> Result<Page<GhUser, u32>, String> {
    Ok(rest_page(page, rest_items::<RestUser>(text)?, |u| u.user()))
}

/// A page of collaborators (missing permissions read as pull only).
pub fn parse_collaborators_page(page: u32, text: &str) -> Result<Page<Collaborator, u32>, String> {
    Ok(rest_page(page, rest_items::<RestUser>(text)?, |u| Collaborator {
        user: u.user(),
        permissions: u.permissions.unwrap_or(CollaboratorPermissions { pull: true, ..Default::default() }),
    }))
}

pub fn parse_labels_page(page: u32, text: &str) -> Result<Page<GhLabel, u32>, String> {
    Ok(rest_page(page, rest_items::<RestLabel>(text)?, |l| GhLabel {
        id: l.node_id,
        url: l.url,
        name: l.name,
        color: l.color,
    }))
}

fn cursor_value(cursor: Option<&str>) -> Option<Value> {
    Some(cursor.map_or(Value::Null, Value::from))
}

/// A page of the repository's mentionable users (`findMentionableUsers`).
pub fn mentionable_users_request(repo: &Coordinates, cursor: Option<&str>) -> Result<GhApiRequest, String> {
    let [owner, name] = repo_variables(repo);
    document_request("findMentionableUsers", &[owner, name, ("cursor", cursor_value(cursor))])
}

/// A page of the owner's teams (`findOrganizationTeams`); `None` for a repository a user owns (teams
/// exist for organizations only): its page is empty.
pub fn teams_request(repo: &GhRepository, cursor: Option<&str>) -> Option<Result<GhApiRequest, String>> {
    (repo.owner.typename == "Organization").then(|| {
        document_request(
            "findOrganizationTeams",
            &[("organization", Some(Value::from(repo.owner.login.as_str()))), ("cursor", cursor_value(cursor))],
        )
    })
}

fn connection_page<T>(c: Option<Connection<T>>) -> Page<T, Option<String>> {
    match c {
        None => Page { items: Vec::new(), next: None },
        Some(c) => {
            let next = c.page_info.end_cursor.filter(|_| c.page_info.has_next_page).map(Some);
            Page { items: c.nodes, next }
        }
    }
}

pub fn parse_mentionable_users_page(text: &str) -> Result<Page<GhUser, Option<String>>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repo {
        mentionable_users: Connection<GhUser>,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repo>,
    }
    let data = graphql_data::<Data>(text).map_err(|e| e.message)?;
    Ok(connection_page(data.repository.map(|r| r.mentionable_users)))
}

/// A teams page; listing teams needs `read:org`, which gh may lack: a failure is an empty page.
pub fn parse_teams_page(answer: Result<String, String>) -> Page<GhTeam, Option<String>> {
    #[derive(Deserialize)]
    struct Org {
        teams: Connection<GhTeam>,
    }
    #[derive(Deserialize)]
    struct Data {
        organization: Option<Org>,
    }
    let teams = answer.ok().and_then(|t| graphql_data::<Data>(&t).ok()).and_then(|d| d.organization).map(|o| o.teams);
    connection_page(teams)
}

// ---------------------------------------------------------------------------
// The pull request template

/// The first non-blank pull request template (`getPullRequestTemplates`), loaded once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateCache {
    /// `Some` once answered (`Some(None)`: no template, or the request failed).
    template: Option<Option<String>>,
    loading: bool,
}

impl TemplateCache {
    /// The template once known.
    pub fn template(&self) -> Option<Option<&str>> {
        self.template.as_ref().map(Option::as_deref)
    }

    /// The request to send, unless the template is known or loading.
    pub fn load(&mut self, repo: &Coordinates) -> Option<GhApiRequest> {
        if self.template.is_some() || self.loading {
            return None;
        }
        let request = document_request("getPullRequestTemplates", &repo_variables(repo)).ok();
        self.loading = request.is_some();
        if request.is_none() {
            self.template = Some(None);
        }
        request
    }

    pub fn loaded(&mut self, answer: Result<String, String>) {
        #[derive(Deserialize)]
        struct Template {
            body: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repo {
            pull_request_templates: Option<Vec<Template>>,
        }
        #[derive(Deserialize)]
        struct Data {
            repository: Option<Repo>,
        }
        let templates = answer
            .ok()
            .and_then(|t| graphql_data::<Data>(&t).ok())
            .and_then(|d| d.repository)
            .and_then(|r| r.pull_request_templates)
            .unwrap_or_default();
        self.loading = false;
        self.template = Some(templates.into_iter().filter_map(|t| t.body).find(|b| !b.trim().is_empty()));
    }
}

// ---------------------------------------------------------------------------
// The lists

/// Every cached list of the connected repository; [`RepositoryData::reset`] drops them (another
/// repository, a disconnect, "reload repository data").
#[derive(Clone, Debug)]
pub struct RepositoryData {
    pub collaborators: BatchesLoader<Collaborator, u32>,
    pub contributors: BatchesLoader<GhUser, u32>,
    pub assignees: BatchesLoader<GhUser, u32>,
    pub labels: BatchesLoader<GhLabel, u32>,
    pub mentionable_users: BatchesLoader<GhUser, Option<String>>,
    pub teams: BatchesLoader<GhTeam, Option<String>>,
    pub template: TemplateCache,
}

impl Default for RepositoryData {
    fn default() -> Self {
        Self {
            collaborators: BatchesLoader::new(1),
            contributors: BatchesLoader::new(1),
            assignees: BatchesLoader::new(1),
            labels: BatchesLoader::new(1),
            mentionable_users: BatchesLoader::new(None),
            teams: BatchesLoader::new(None),
            template: TemplateCache::default(),
        }
    }
}

/// Which list gives the author candidates and the potential reviewers: users with write access
/// can list collaborators, others only contributors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsersSource {
    Collaborators,
    Contributors,
}

pub fn users_source(permission: Option<Permission>) -> UsersSource {
    if has_permission(permission, Permission::Write) { UsersSource::Collaborators } else { UsersSource::Contributors }
}

impl RepositoryData {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Users that can be chosen as authors in the list filter.
    pub fn author_candidates(&self, permission: Option<Permission>) -> BatchesState<GhUser> {
        match users_source(permission) {
            UsersSource::Collaborators => {
                let s = self.collaborators.state();
                BatchesState {
                    items: s.items.iter().map(|c| c.user.clone()).collect(),
                    loading: s.loading,
                    done: s.done,
                    error: s.error.clone(),
                }
            }
            UsersSource::Contributors => self.contributors.state().clone(),
        }
    }

    /// Potential reviewers (`getBatchedPotentialReviewers`): teams, then collaborators with push
    /// access (else contributors).
    pub fn potential_reviewers(&self, permission: Option<Permission>) -> BatchesState<GhReviewer> {
        let t = self.teams.state();
        let teams = BatchesState {
            items: t.items.iter().cloned().map(GhReviewer::Team).collect(),
            loading: t.loading,
            done: t.done,
            error: t.error.clone(),
        };
        let users = match users_source(permission) {
            UsersSource::Collaborators => {
                let s = self.collaborators.state();
                BatchesState {
                    items: s
                        .items
                        .iter()
                        .filter(|c| c.permissions.push)
                        .map(|c| GhReviewer::Actor(c.user.clone()))
                        .collect(),
                    loading: s.loading,
                    done: s.done,
                    error: s.error.clone(),
                }
            }
            UsersSource::Contributors => {
                let s = self.contributors.state();
                BatchesState {
                    items: s.items.iter().cloned().map(GhReviewer::Actor).collect(),
                    loading: s.loading,
                    done: s.done,
                    error: s.error.clone(),
                }
            }
        };
        merge_batches(&[&teams, &users])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::batches::{LoadStep, PageOutcome};
    use crate::github::new_comments::fixtures::variables;
    use serde_json::json;

    // Not from a spec: REST pages are numbered, 100 per page; collaborators without permissions pull.
    #[test]
    fn pages_rest_lists_by_number() {
        let repo = Coordinates::new("o", "r");
        assert_eq!(rest_page_request(&repo, RestList::Labels, 2).endpoint, "repos/o/r/labels?per_page=100&page=2");
        let users: Vec<Value> =
            (0..100).map(|i| json!({ "login": format!("u{i}"), "node_id": format!("U{i}") })).collect();
        let page = parse_users_page(1, &Value::from(users).to_string()).expect("users");
        assert_eq!((page.items.len(), page.next), (100, Some(2)));
        let page =
            parse_collaborators_page(2, r#"[{"login":"a","permissions":{"push":true}},{"login":"b"}]"#).expect("users");
        assert_eq!(page.next, None);
        assert!(page.items[0].permissions.push && !page.items[1].permissions.push && page.items[1].permissions.pull);
        let labels =
            parse_labels_page(1, r#"[{"node_id":"L1","name":"bug","color":"d73a4a","url":""}]"#).expect("labels");
        assert_eq!(labels.items[0].name, "bug");
    }

    // Not from a spec: potential reviewers are the teams, then collaborators who can push.
    #[test]
    fn offers_teams_then_pushers_as_potential_reviewers() {
        let mut data = RepositoryData::default();
        let LoadStep::Fetch(ticket) = data.collaborators.load() else { panic!("loads") };
        let page =
            parse_collaborators_page(1, r#"[{"login":"a","permissions":{"push":true}},{"login":"b"}]"#).expect("users");
        assert_eq!(data.collaborators.page_loaded(&ticket, Ok(page)), PageOutcome::Done);
        let LoadStep::Fetch(ticket) = data.teams.load() else { panic!("loads") };
        let teams = parse_teams_page(Err("read:org missing".into()));
        assert_eq!(data.teams.page_loaded(&ticket, Ok(teams)), PageOutcome::Done);
        let reviewers = data.potential_reviewers(Some(Permission::Write));
        let keys: Vec<&str> = reviewers.items.iter().map(GhReviewer::key).collect();
        assert_eq!(keys, ["a"]);
        assert_eq!(data.author_candidates(Some(Permission::Write)).items.len(), 2);
        data.reset();
        assert!(data.collaborators.state().items.is_empty());
    }

    // Not from a spec: the first non-blank template, asked for once.
    #[test]
    fn caches_the_first_non_blank_template() {
        let repo = Coordinates::new("o", "r");
        let mut cache = TemplateCache::default();
        let request = cache.load(&repo).expect("asked");
        assert_eq!(variables(&request), json!({ "repoOwner": "o", "repoName": "r" }));
        assert_eq!(cache.load(&repo), None);
        cache.loaded(Ok(
            r###"{"data":{"repository":{"pullRequestTemplates":[{"body":"  "},{"body":"## Why"}]}}}"###.into()
        ));
        assert_eq!(cache.template(), Some(Some("## Why")));
        assert_eq!(cache.load(&repo), None);
    }
}
