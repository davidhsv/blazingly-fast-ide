//! The Remote Host tool window's trees, one per server: the resolved root and mappings, the loaded
//! and expanded folders, and Download to Project through the backend-resolved mappings.
//!
//! A tree belongs to what it was listed from: the server's connection, root path and mappings and
//! the project. When any of them changes the tree starts over, and answers for the old one (which
//! carry the old generation) are dropped.

use super::actions::{Notice, NoticeBody};
use super::model::{local_path_for_remote, remote_name};
use super::types::{DeploymentRoot, ResolvedMapping};
use crate::platform::collate;
use crate::platform::i18n::I18n;
use crate::platform_more::remote_settings::deployment_connection;
use crate::workbench::notifications::NotificationLevel;
use ide_core::IdeError;
use ide_core::settings::{DeploymentMapping, DeploymentServer};
use ide_ssh::{FsConnection, RemoteEntry};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// What a tree was listed from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeSource {
    connection: FsConnection,
    root_path: String,
    mappings: Vec<DeploymentMapping>,
    project_root: Option<String>,
}

impl TreeSource {
    fn of(server: &DeploymentServer, project_root: Option<&str>) -> Self {
        Self {
            connection: deployment_connection(server),
            root_path: server.root_path.clone(),
            mappings: server.mappings.clone(),
            project_root: project_root.map(str::to_owned),
        }
    }
}

/// One server's tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteTree {
    source: Option<TreeSource>,
    generation: u64,
    pub root: Option<String>,
    pub mappings: Vec<ResolvedMapping>,
    /// The root could not be resolved or listed.
    pub error: Option<String>,
    /// Entries of the loaded folders: folders first, then by name.
    pub listings: HashMap<String, Vec<RemoteEntry>>,
    pub folder_errors: HashMap<String, String>,
    pub expanded: HashSet<String>,
    /// Entries Download to Project maps once the root is listed again.
    downloading: Option<Vec<RemoteEntry>>,
}

/// A request of the Remote Host window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRequest {
    /// `deploy_server_root(server)`; answer with [`RemoteHost::root_resolved`].
    ServerRoot { server: String, generation: u64 },
    /// `remote_list(connection, path)`; answer with [`RemoteHost::listed`].
    List { server: String, connection: FsConnection, path: String, generation: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostEffect {
    Request(HostRequest),
    Notify(Notice),
    /// Download these project paths from the server (`downloadPaths` of the actions: coverage,
    /// then `deploy_download`).
    Download {
        server: String,
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteHost {
    trees: HashMap<String, RemoteTree>,
    generations: u64,
}

/// Folders first, then names in the collation order.
fn sort_entries(mut entries: Vec<RemoteEntry>) -> Vec<RemoteEntry> {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => collate::compare(&a.name, &b.name),
    });
    entries
}

impl RemoteHost {
    pub fn tree(&self, server: &str) -> Option<&RemoteTree> {
        self.trees.get(server)
    }

    /// The server's tree, started over when it was listed from something else.
    fn current_tree(&mut self, server: &DeploymentServer, project_root: Option<&str>) -> &mut RemoteTree {
        let source = TreeSource::of(server, project_root);
        let fresh = self.trees.get(&server.id).is_none_or(|tree| tree.source.as_ref() != Some(&source));
        if fresh {
            self.generations += 1;
            let tree = RemoteTree { source: Some(source), generation: self.generations, ..RemoteTree::default() };
            self.trees.insert(server.id.clone(), tree);
        }
        self.trees.entry(server.id.clone()).or_default()
    }

    fn list_request(server: &DeploymentServer, tree: &RemoteTree, path: &str) -> HostRequest {
        HostRequest::List {
            server: server.id.clone(),
            connection: deployment_connection(server),
            path: path.to_owned(),
            generation: tree.generation,
        }
    }

    /// Resolves the server root and mappings (again with `force`), then lists the root; `None`
    /// when the root is listed already.
    pub fn ensure_root(
        &mut self,
        server: &DeploymentServer,
        project_root: Option<&str>,
        force: bool,
    ) -> Option<HostRequest> {
        let tree = self.current_tree(server, project_root);
        if !force && tree.root.as_ref().is_some_and(|root| tree.listings.contains_key(root)) {
            return None;
        }
        tree.error = None;
        match &tree.root {
            Some(root) if !force => Some(Self::list_request(server, tree, root)),
            _ => Some(HostRequest::ServerRoot { server: server.id.clone(), generation: tree.generation }),
        }
    }

    fn tree_of(&mut self, server: &DeploymentServer, generation: u64) -> Option<&mut RemoteTree> {
        self.trees.get_mut(&server.id).filter(|tree| tree.generation == generation)
    }

    /// The answer of [`HostRequest::ServerRoot`]: lists the root.
    pub fn root_resolved(
        &mut self,
        server: &DeploymentServer,
        generation: u64,
        result: Result<DeploymentRoot, IdeError>,
        i18n: &I18n,
    ) -> Vec<HostEffect> {
        let Some(tree) = self.tree_of(server, generation) else { return Vec::new() };
        match result {
            Ok(resolved) => {
                tree.mappings = resolved.mappings;
                let request = Self::list_request(server, tree, &resolved.root);
                tree.root = Some(resolved.root);
                vec![HostEffect::Request(request)]
            }
            Err(error) => {
                tree.error = Some(i18n.tx_template(&error.message));
                tree.downloading = None;
                Vec::new()
            }
        }
    }

    /// Lists a folder (expanding or reloading it).
    pub fn load_folder(&mut self, server: &DeploymentServer, project_root: Option<&str>, path: &str) -> HostRequest {
        let tree = self.current_tree(server, project_root);
        Self::list_request(server, tree, path)
    }

    /// The answer of [`HostRequest::List`]. A failure of the root is the tree's error, of another
    /// folder that folder's.
    pub fn listed(
        &mut self,
        server: &DeploymentServer,
        generation: u64,
        path: &str,
        result: Result<Vec<RemoteEntry>, IdeError>,
        i18n: &I18n,
    ) -> Vec<HostEffect> {
        let Some(tree) = self.tree_of(server, generation) else { return Vec::new() };
        match result {
            Ok(entries) => {
                tree.folder_errors.remove(path);
                tree.listings.insert(path.to_owned(), sort_entries(entries));
            }
            Err(error) if tree.root.as_deref() == Some(path) => tree.error = Some(i18n.tx_template(&error.message)),
            Err(error) => {
                tree.folder_errors.insert(path.to_owned(), i18n.tx_template(&error.message));
            }
        }
        if tree.root.as_deref() != Some(path) {
            return Vec::new();
        }
        match tree.downloading.take() {
            Some(entries) if tree.error.is_none() => Self::download_mapped(server, &tree.mappings, &entries, i18n),
            _ => Vec::new(),
        }
    }

    /// Reloads the root and the expanded folders.
    pub fn refresh_all(&mut self, server: &DeploymentServer, project_root: Option<&str>) -> Vec<HostRequest> {
        let source = TreeSource::of(server, project_root);
        let Some(tree) = self.trees.get_mut(&server.id).filter(|tree| tree.source.as_ref() == Some(&source)) else {
            return self.ensure_root(server, project_root, true).into_iter().collect();
        };
        let Some(root) = tree.root.clone() else {
            return self.ensure_root(server, project_root, true).into_iter().collect();
        };
        tree.listings.clear();
        tree.folder_errors.clear();
        tree.error = None;
        let mut folders = vec![root];
        folders.extend(tree.expanded.iter().cloned());
        folders.iter().map(|path| Self::list_request(server, tree, path)).collect()
    }

    /// Shows or hides a folder's children; an expanded folder not loaded yet is listed.
    pub fn set_expanded(
        &mut self,
        server: &DeploymentServer,
        project_root: Option<&str>,
        path: &str,
        expanded: bool,
    ) -> Option<HostRequest> {
        let tree = self.current_tree(server, project_root);
        if !expanded {
            tree.expanded.remove(path);
            return None;
        }
        tree.expanded.insert(path.to_owned());
        (!tree.listings.contains_key(path)).then(|| Self::list_request(server, tree, path))
    }

    /// Download to Project: resolves the root and mappings again, then downloads the mapped
    /// entries to their project paths; unmapped ones are reported. `None` without a project.
    pub fn download_to_project(
        &mut self,
        server: &DeploymentServer,
        project_root: Option<&str>,
        entries: Vec<RemoteEntry>,
    ) -> Option<HostRequest> {
        project_root?;
        let request = self.ensure_root(server, project_root, true)?;
        if let Some(tree) = self.trees.get_mut(&server.id) {
            tree.downloading = Some(entries);
        }
        Some(request)
    }

    fn download_mapped(
        server: &DeploymentServer,
        mappings: &[ResolvedMapping],
        entries: &[RemoteEntry],
        i18n: &I18n,
    ) -> Vec<HostEffect> {
        let mut locals = Vec::new();
        let mut unmapped = Vec::new();
        for entry in entries {
            match local_path_for_remote(&entry.path, mappings) {
                Some(local) => locals.push(local),
                None => unmapped.push(remote_name(&entry.path)),
            }
        }
        let mut effects = Vec::new();
        if !unmapped.is_empty() {
            effects.push(HostEffect::Notify(Notice {
                level: NotificationLevel::Warning,
                title: i18n.tx("Not mapped to a project path"),
                body: Some(NoticeBody::Text(unmapped.join(", "))),
                key: Some("deployment.unmapped".to_owned()),
                actions: Vec::new(),
            }));
        }
        if !locals.is_empty() {
            effects.push(HostEffect::Download { server: server.id.clone(), paths: locals });
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IpcPath;
    use ide_core::settings::DeploymentType;
    use std::path::PathBuf;

    fn sftp_server() -> DeploymentServer {
        DeploymentServer {
            id: "existing".to_owned(),
            ssh_config: "ssh".to_owned(),
            root_path: "/srv".to_owned(),
            ..DeploymentServer::default()
        }
    }

    fn resolved(root: &str, mappings: Vec<ResolvedMapping>) -> Result<DeploymentRoot, IdeError> {
        Ok(DeploymentRoot { root: root.to_owned(), mappings })
    }

    fn mapping(local: &str, deployment: &str) -> ResolvedMapping {
        ResolvedMapping { local: IpcPath::from(PathBuf::from(local)), deployment: deployment.to_owned() }
    }

    fn generation_of(request: &HostRequest) -> u64 {
        match request {
            HostRequest::ServerRoot { generation, .. } | HostRequest::List { generation, .. } => *generation,
        }
    }

    /// Runs `ensure_root` to the end: the root resolved as `root` and listed empty. Returns the
    /// list request, or `None` when nothing was asked.
    fn ensure(
        host: &mut RemoteHost,
        server: &DeploymentServer,
        answer: Result<DeploymentRoot, IdeError>,
        roots_asked: &mut usize,
    ) -> Option<HostRequest> {
        let i18n = I18n::default();
        let request = host.ensure_root(server, Some("/project"), false)?;
        let generation = generation_of(&request);
        let list = match request {
            HostRequest::ServerRoot { .. } => {
                *roots_asked += 1;
                match host.root_resolved(server, generation, answer, &i18n).pop() {
                    Some(HostEffect::Request(list)) => list,
                    other => panic!("the root is listed: {other:?}"),
                }
            }
            list => list,
        };
        if let HostRequest::List { path, .. } = &list {
            host.listed(server, generation, path, Ok(Vec::new()), &i18n);
        }
        Some(list)
    }

    fn list_of(request: Option<HostRequest>) -> Option<(FsConnection, String)> {
        match request? {
            HostRequest::List { connection, path, .. } => Some((connection, path)),
            HostRequest::ServerRoot { .. } => None,
        }
    }

    // reloads a server's tree when changing its filesystem type or root
    #[test]
    fn reloads_a_servers_tree_when_changing_its_filesystem_type_or_root() {
        let mut host = RemoteHost::default();
        let mut roots = 0;
        let server = sftp_server();
        let sftp = FsConnection::Sftp { ssh: "ssh".to_owned() };
        let listed = ensure(&mut host, &server, resolved("/srv", Vec::new()), &mut roots);
        assert_eq!(list_of(listed), Some((sftp, "/srv".to_owned())));
        assert_eq!(ensure(&mut host, &server, resolved("/srv", Vec::new()), &mut roots), None);
        assert_eq!(roots, 1);
        let local = DeploymentServer { kind: DeploymentType::Local, ..server };
        let listed = ensure(&mut host, &local, resolved("/srv", Vec::new()), &mut roots);
        assert_eq!(list_of(listed), Some((FsConnection::Local, "/srv".to_owned())));
        let mounted = DeploymentServer { root_path: "/mounted".to_owned(), ..local };
        let listed = ensure(&mut host, &mounted, resolved("/mounted", Vec::new()), &mut roots);
        assert_eq!(list_of(listed), Some((FsConnection::Local, "/mounted".to_owned())));
        assert_eq!(host.tree("existing").and_then(|tree| tree.root.as_deref()), Some("/mounted"));
    }

    // refreshes resolved mappings when the local mapping changes
    #[test]
    fn refreshes_resolved_mappings_when_the_local_mapping_changes() {
        let mut host = RemoteHost::default();
        let mut roots = 0;
        let server = DeploymentServer { id: "local".to_owned(), kind: DeploymentType::Local, ..Default::default() };
        ensure(&mut host, &server, resolved("/srv", Vec::new()), &mut roots);
        let mappings = vec![mapping("/project/src", "/srv")];
        let alias = DeploymentMapping { local: "/alias".to_owned(), deployment: "/".to_owned(), web: String::new() };
        let changed = DeploymentServer { mappings: vec![alias], ..server };
        ensure(&mut host, &changed, resolved("/srv", mappings.clone()), &mut roots);
        assert_eq!(roots, 2);
        assert_eq!(host.tree("local").map(|tree| tree.mappings.clone()), Some(mappings));
    }

    // downloads using backend-resolved folders instead of the configured alias
    #[test]
    fn downloads_using_backend_resolved_folders_instead_of_the_configured_alias() {
        let i18n = I18n::default();
        let mut host = RemoteHost::default();
        let alias = DeploymentMapping { local: "/alias".to_owned(), deployment: ".".to_owned(), web: String::new() };
        let server = DeploymentServer {
            id: "local".to_owned(),
            kind: DeploymentType::Local,
            mappings: vec![alias],
            ..Default::default()
        };
        let entry = RemoteEntry {
            path: "/srv/a.txt".to_owned(),
            name: "a.txt".to_owned(),
            is_dir: false,
            is_symlink: false,
            size: 5,
            mtime: None,
            permissions: "rw-r--r--".to_owned(),
        };
        let mut download = |mappings: Vec<ResolvedMapping>| -> Vec<HostEffect> {
            let request = host.download_to_project(&server, Some("/project"), vec![entry.clone()]);
            let Some(HostRequest::ServerRoot { generation, .. }) = request else {
                panic!("the root is resolved again");
            };
            let Some(HostEffect::Request(HostRequest::List { path, .. })) =
                host.root_resolved(&server, generation, resolved("/srv", mappings), &i18n).pop()
            else {
                panic!("the root is listed");
            };
            host.listed(&server, generation, &path, Ok(Vec::new()), &i18n)
        };
        let effects = download(vec![mapping("/project/src", "/srv")]);
        let expected = HostEffect::Download { server: "local".to_owned(), paths: vec!["/project/src/a.txt".into()] };
        assert_eq!(effects, [expected]);
        let effects = download(Vec::new());
        assert!(!effects.iter().any(|effect| matches!(effect, HostEffect::Download { .. })));
        let titles: Vec<&str> = effects
            .iter()
            .filter_map(|effect| match effect {
                HostEffect::Notify(notice) => Some(notice.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["Not mapped to a project path"]);
    }

    // Not from a spec: folders sort first, a failed folder keeps its own error, and an answer for
    // a tree that started over is dropped.
    #[test]
    fn sorts_folders_first_and_drops_stale_listings() {
        let i18n = I18n::default();
        let mut host = RemoteHost::default();
        let server = sftp_server();
        let first = host.ensure_root(&server, None, false);
        let old = first.as_ref().map_or(0, generation_of);
        host.root_resolved(&server, old, resolved("/srv", Vec::new()), &i18n);
        let entry = |name: &str, is_dir: bool| RemoteEntry {
            path: format!("/srv/{name}"),
            name: name.to_owned(),
            is_dir,
            is_symlink: false,
            size: 0,
            mtime: None,
            permissions: String::new(),
        };
        let listing = vec![entry("b.txt", false), entry("a", true), entry("a.txt", false)];
        host.listed(&server, old, "/srv", Ok(listing), &i18n);
        let names: Vec<&str> = host
            .tree("existing")
            .map_or(Vec::new(), |tree| tree.listings["/srv"].iter().map(|entry| entry.name.as_str()).collect());
        assert_eq!(names, ["a", "a.txt", "b.txt"]);
        let denied = IdeError::new(ide_core::error::ErrorKind::Io, "denied");
        host.listed(&server, old, "/srv/x", Err(denied), &i18n);
        let error = host.tree("existing").and_then(|tree| tree.folder_errors.get("/srv/x").cloned());
        assert_eq!(error.as_deref(), Some("denied"));
        let moved = DeploymentServer { root_path: "/other".to_owned(), ..server.clone() };
        host.ensure_root(&moved, None, false);
        assert!(host.listed(&moved, old, "/srv", Ok(Vec::new()), &i18n).is_empty());
        assert!(host.tree("existing").is_some_and(|tree| tree.listings.is_empty()));
    }
}
