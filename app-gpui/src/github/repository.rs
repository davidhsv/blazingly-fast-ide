//! The project's GitHub connection, like IntelliJ's `GHPRProjectViewModel` (repository selection,
//! auto-connect): gh availability, the GitHub remotes, the connected repository and the current user.
//!
//! Auto-connect: a project with one GitHub repository connects at once; with several, the
//! repository chosen last (saved per project) is used, else the selector is shown. The remotes come
//! from every git repository of the project (a folder holding several repositories offers each
//! one's), and gh runs in the repository of the connected remote ([`RepositoryState::connected_root`]).
//!
//! Nothing here calls the backend. A refresh goes [`RepositoryState::begin_refresh`] → list the
//! roots and their remotes → [`RepositoryState::remotes_listed`] → ask gh's availability in the
//! probe root → [`RepositoryState::availability_answered`]; a connect goes
//! [`RepositoryState::connect`] → find the repository and the current user →
//! [`RepositoryState::connected`]. A newer refresh or connect makes the answers of an older one void.

use super::data_model::{parse_github_remote, remote_of_repository};
use super::types::{Coordinates, GhRepository, GhUser};
use ide_github::GhAvailability;
use ide_vcs::Remote;

/// Storage key of the repository chosen last (`owner/name`), per project root.
pub fn selection_key(project_root: &str) -> String {
    format!("openidea.github.repository.v1:{project_root}")
}

/// A GitHub remote of the project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GhRemote {
    pub owner: String,
    pub name: String,
    /// Git remote name (`origin`).
    pub remote: String,
    pub url: String,
    /// Root of the project's git repository the remote belongs to (`None`: the primary one).
    pub root: Option<String>,
}

impl GhRemote {
    pub fn coordinates(&self) -> Coordinates {
        Coordinates::new(&self.owner, &self.name)
    }

    fn same(&self, other: Option<&Coordinates>) -> bool {
        other.is_some_and(|o| self.coordinates().same(o))
    }
}

/// The GitHub remotes among the `remotes` of the repository at `root`, one per GitHub repository.
pub fn github_remotes(remotes: &[Remote], root: Option<&str>) -> Vec<GhRemote> {
    let mut out: Vec<GhRemote> = Vec::new();
    for r in remotes {
        if let Some(c) = parse_github_remote(&r.url)
            && !out.iter().any(|o| o.same(Some(&c)))
        {
            out.push(GhRemote {
                owner: c.owner,
                name: c.name,
                remote: r.name.clone(),
                url: r.url.clone(),
                root: root.map(str::to_owned),
            });
        }
    }
    out
}

/// The remotes of one local repository (`root` `None`: the project's primary one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRemotes {
    pub root: Option<String>,
    pub remotes: Vec<Remote>,
}

/// The connected repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhConnection {
    pub coordinates: Coordinates,
    pub repository: GhRepository,
    pub current_user: GhUser,
    /// The local repository checkouts, diffs and new pull requests use (`None`: the primary one).
    pub root: Option<String>,
    /// That repository's GitHub remotes.
    pub remotes: Vec<GhRemote>,
}

/// A refresh that started: list the project's git roots and each one's remotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshTicket {
    seq: u64,
}

/// Ask gh's availability (`gh_availability`) in `probe`, then call
/// [`RepositoryState::availability_answered`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailabilityRequest {
    seq: u64,
    pub probe: Option<String>,
    found: Vec<GhRemote>,
    all: Vec<RootRemotes>,
    saved: Option<Coordinates>,
}

/// Find the repository (`findRepository`) and the current user, then call
/// [`RepositoryState::connected`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectRequest {
    seq: u64,
    pub coordinates: Coordinates,
    /// Where gh runs for these requests.
    pub root: Option<String>,
    remember: bool,
}

/// What a refresh ends with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshStep {
    /// Connect (the only repository, the current or the saved one).
    Connect(ConnectRequest),
    /// gh is not ready or no repository is chosen: the selector or the reason is shown.
    NotConnected { disconnected: Disconnected },
    /// A newer refresh started.
    Stale,
}

/// What a disconnect asks the caller to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Disconnected {
    /// Remove the saved choice ([`selection_key`]).
    pub clear_selection: bool,
    /// Drop the cached repository lists (collaborators, labels, …).
    pub reset_data: bool,
}

/// What a connect ends with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectOutcome {
    Connected {
        /// Store this `owner/name` under [`selection_key`].
        save_selection: Option<String>,
        /// Another repository than before: drop the cached repository lists.
        reset_data: bool,
    },
    /// "Could not connect to repository …" is in [`RepositoryState::connection_error`].
    Failed,
    Stale,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepositoryState {
    /// gh's state; `None` until checked.
    pub availability: Option<GhAvailability>,
    pub checking: bool,
    /// GitHub remotes of the project, one per repository.
    pub remotes: Vec<GhRemote>,
    /// gh's own choice among them (`gh repo view`), preselected in the selector.
    pub default_remote: Option<Coordinates>,
    pub connection: Option<GhConnection>,
    pub connecting: bool,
    pub connection_error: Option<String>,
    /// Bumped whenever the repository data is reloaded (IntelliJ's `dataReloadSignal`).
    pub revision: u64,
    /// GitHub remotes of every local repository (a GitHub repository can be a remote of several).
    local_remotes: Vec<GhRemote>,
    check_seq: u64,
    connect_seq: u64,
}

impl RepositoryState {
    /// The local repository of the connection, for gh and git commands (`None`: the primary one).
    pub fn connected_root(&self) -> Option<&str> {
        self.connection.as_ref().and_then(|c| c.root.as_deref())
    }

    /// The connected local repository's GitHub remotes (none while disconnected).
    pub fn connected_remotes(&self) -> &[GhRemote] {
        self.connection.as_ref().map(|c| c.remotes.as_slice()).unwrap_or_default()
    }

    pub fn begin_refresh(&mut self) -> RefreshTicket {
        self.check_seq += 1;
        self.checking = true;
        RefreshTicket { seq: self.check_seq }
    }

    /// Takes the remotes of every local repository (one entry with root `None` when the project
    /// lists no roots) and the saved choice; asks for gh's availability in the repository likely
    /// to be connected (gh resolves its repository in a local one). `None` when stale.
    pub fn remotes_listed(
        &mut self,
        ticket: &RefreshTicket,
        lists: Vec<RootRemotes>,
        saved_selection: Option<&str>,
    ) -> Option<AvailabilityRequest> {
        if ticket.seq != self.check_seq {
            return None;
        }
        let found: Vec<GhRemote> = lists.iter().flat_map(|l| github_remotes(&l.remotes, l.root.as_deref())).collect();
        let current = self.connection.as_ref().map(|c| c.coordinates.clone());
        let saved = saved_selection.and_then(Coordinates::parse);
        let probe = found
            .iter()
            .find(|r| r.same(current.as_ref()))
            .or_else(|| found.iter().find(|r| r.same(saved.as_ref())))
            .or(found.first())
            .and_then(|r| r.root.clone());
        Some(AvailabilityRequest { seq: ticket.seq, probe, found, all: lists, saved })
    }

    /// Takes gh's availability and decides what to connect to.
    pub fn availability_answered(
        &mut self,
        request: AvailabilityRequest,
        answer: Result<GhAvailability, String>,
    ) -> RefreshStep {
        if request.seq != self.check_seq {
            return RefreshStep::Stale;
        }
        let availability = answer.unwrap_or_else(|message| GhAvailability::Error { message });
        let mut local = request.found;
        let mut remotes: Vec<GhRemote> = Vec::new();
        for r in &local {
            if !remotes.iter().any(|o| o.same(Some(&r.coordinates()))) {
                remotes.push(r.clone());
            }
        }
        let default_remote = match &availability {
            GhAvailability::Ready { repo, .. } => Coordinates::parse(&repo.name_with_owner),
            _ => None,
        };
        if let Some(default) = &default_remote
            && !remotes.iter().any(|r| r.same(Some(default)))
        {
            let own = request
                .all
                .iter()
                .find(|l| l.root == request.probe)
                .and_then(|l| remote_of_repository(&l.remotes, default));
            let url = match (own, &availability) {
                (Some(own), _) => own.url.clone(),
                (None, GhAvailability::Ready { repo, .. }) => repo.url.clone(),
                (None, _) => String::new(),
            };
            let remote = GhRemote {
                owner: default.owner.clone(),
                name: default.name.clone(),
                remote: own.map(|o| o.name.clone()).unwrap_or_default(),
                url,
                root: request.probe.clone(),
            };
            remotes.insert(0, remote.clone());
            local.insert(0, remote);
        }
        let ready = matches!(availability, GhAvailability::Ready { .. });
        self.local_remotes = local;
        self.availability = Some(availability);
        self.default_remote = default_remote;
        self.checking = false;
        self.remotes = remotes;
        if !ready {
            return RefreshStep::NotConnected { disconnected: self.disconnect(false) };
        }
        let current = self.connection.as_ref().map(|c| c.coordinates.clone());
        let target = (if self.remotes.len() == 1 {
            self.remotes.first()
        } else {
            self.remotes
                .iter()
                .find(|r| r.same(current.as_ref()))
                .or_else(|| self.remotes.iter().find(|r| r.same(request.saved.as_ref())))
        })
        .cloned();
        // Only a choice among several repositories is remembered.
        let remember = self.remotes.len() > 1;
        match target {
            Some(t) => RefreshStep::Connect(self.connect(&t.coordinates(), Some(t.root), remember)),
            None => RefreshStep::NotConnected { disconnected: self.disconnect(false) },
        }
    }

    /// Connects to `repo` (the selector's "View Pull Requests"); gh and git then run in `root`
    /// (`Some`), else in the first local repository with it as a remote.
    pub fn connect(&mut self, repo: &Coordinates, root: Option<Option<String>>, remember: bool) -> ConnectRequest {
        self.connect_seq += 1;
        let root =
            root.unwrap_or_else(|| self.local_remotes.iter().find(|r| r.same(Some(repo))).and_then(|r| r.root.clone()));
        self.connecting = true;
        self.connection_error = None;
        ConnectRequest { seq: self.connect_seq, coordinates: repo.clone(), root, remember }
    }

    /// Takes the repository (`None`: not found) and the current user.
    pub fn connected(
        &mut self,
        request: ConnectRequest,
        answer: Result<(Option<GhRepository>, GhUser), String>,
    ) -> ConnectOutcome {
        if request.seq != self.connect_seq {
            return ConnectOutcome::Stale;
        }
        let name = request.coordinates.name_with_owner();
        let found = answer.and_then(|(repository, user)| match repository {
            Some(repository) => Ok((repository, user)),
            None => Err(format!("Repository {name} not found")),
        });
        match found {
            Ok((repository, current_user)) => {
                let reset_data = self
                    .connection
                    .as_ref()
                    .is_none_or(|p| !p.coordinates.same(&request.coordinates) || p.root != request.root);
                let remotes: Vec<GhRemote> =
                    self.local_remotes.iter().filter(|r| r.root == request.root).cloned().collect();
                self.connection = Some(GhConnection {
                    coordinates: request.coordinates,
                    repository,
                    current_user,
                    root: request.root,
                    remotes,
                });
                self.connecting = false;
                self.revision += 1;
                ConnectOutcome::Connected { save_selection: request.remember.then_some(name), reset_data }
            }
            Err(error) => {
                self.connecting = false;
                self.connection_error = Some(format!("Could not connect to repository {name}: {error}"));
                ConnectOutcome::Failed
            }
        }
    }

    /// Drops the connection; `forget` also clears the saved choice ("Change Repository or Account…").
    pub fn disconnect(&mut self, forget: bool) -> Disconnected {
        self.connect_seq += 1;
        let had = self.connection.is_some() || self.connecting;
        self.connection = None;
        self.connecting = false;
        Disconnected { clear_selection: forget, reset_data: had }
    }

    /// Another project: start over (the next refresh checks again).
    pub fn reset(&mut self) -> Disconnected {
        self.check_seq += 1;
        let d = self.disconnect(false);
        self.availability = None;
        self.remotes.clear();
        self.default_remote = None;
        self.connection_error = None;
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::user;

    fn remote(name: &str, url: &str) -> Remote {
        Remote { name: name.into(), url: url.into() }
    }

    #[test]
    fn recognizes_github_com_remotes() {
        for url in [
            "https://github.com/o/r.git",
            "https://user@github.com/o/r",
            "git@github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
            "ssh://git@github.com:22/o/r.git",
            "git://github.com/o/r",
        ] {
            assert_eq!(parse_github_remote(url), Some(Coordinates::new("o", "r")), "{url}");
        }
        assert_eq!(parse_github_remote("https://gitlab.com/o/r.git"), None);
        assert_eq!(parse_github_remote("/local/repo"), None);
        let found = github_remotes(
            &[
                remote("origin", "git@github.com:me/r.git"),
                remote("upstream", "https://github.com/org/r.git"),
                remote("dup", "https://github.com/ORG/r"),
                remote("other", "https://example.com/x/y"),
            ],
            None,
        );
        let names: Vec<String> = found.iter().map(|r| format!("{}={}/{}", r.remote, r.owner, r.name)).collect();
        assert_eq!(names, ["origin=me/r", "upstream=org/r"]);
    }

    fn ready(name_with_owner: &str) -> GhAvailability {
        GhAvailability::Ready {
            repo: ide_github::GhRepo {
                name_with_owner: name_with_owner.into(),
                url: format!("https://github.com/{name_with_owner}"),
                default_branch: None,
                merge_commit_allowed: true,
                squash_merge_allowed: true,
                rebase_merge_allowed: true,
                viewer_permission: String::new(),
            },
            login: Some("me".into()),
        }
    }

    #[test]
    fn offers_the_github_repositories_of_each_and_runs_gh_in_the_connected_one() {
        let mut s = RepositoryState::default();
        let ticket = s.begin_refresh();
        let root =
            |root: &str, url: &str| RootRemotes { root: Some(root.into()), remotes: vec![remote("origin", url)] };
        let lists = vec![
            root("/w/android", "git@github.com:org/android.git"),
            root("/w/ios", "https://github.com/org/ios.git"),
            root("/w/tools", "https://gitlab.com/org/tools.git"),
        ];
        let request = s.remotes_listed(&ticket, lists, None).expect("current refresh");
        // gh is asked in a repository, not in the folder that holds them.
        assert_eq!(request.probe.as_deref(), Some("/w/android"));
        let step = s.availability_answered(request, Ok(ready("org/android")));
        let listed: Vec<(&str, &str, Option<&str>)> =
            s.remotes.iter().map(|r| (r.owner.as_str(), r.name.as_str(), r.root.as_deref())).collect();
        assert_eq!(listed, [("org", "android", Some("/w/android")), ("org", "ios", Some("/w/ios"))]);
        // Several repositories: the selector is shown.
        assert!(matches!(step, RefreshStep::NotConnected { .. }));
        assert_eq!(s.connection, None);

        let ios = s.remotes[1].clone();
        let connect = s.connect(&ios.coordinates(), Some(ios.root.clone()), true);
        let repository = GhRepository { id: "R1".into(), ..GhRepository::default() };
        let outcome = s.connected(connect, Ok((Some(repository), user("me"))));
        assert_eq!(outcome, ConnectOutcome::Connected { save_selection: Some("org/ios".into()), reset_data: true });
        assert_eq!(s.connected_root(), Some("/w/ios"));
        let names: Vec<&str> = s.connected_remotes().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["ios"]);
    }

    // Not from a spec: an older connect's answer is dropped, and a missing repository fails.
    #[test]
    fn drops_answers_of_an_older_connect() {
        let mut s = RepositoryState::default();
        let old = s.connect(&Coordinates::new("o", "a"), Some(None), false);
        let new = s.connect(&Coordinates::new("o", "b"), Some(None), false);
        assert_eq!(s.connected(old, Ok((Some(GhRepository::default()), user("me")))), ConnectOutcome::Stale);
        assert_eq!(s.connected(new, Ok((None, user("me")))), ConnectOutcome::Failed);
        assert_eq!(
            s.connection_error.as_deref(),
            Some("Could not connect to repository o/b: Repository o/b not found")
        );
    }
}
