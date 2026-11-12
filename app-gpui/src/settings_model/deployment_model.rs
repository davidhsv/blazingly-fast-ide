//! The edits behind the Deployment settings pages: the server list, the mappings table and the
//! exclude names.

use ide_core::settings::{DeploymentMapping, DeploymentServer};

/// `.git;.idea; *.pyc` as names: trimmed, without blanks or repeats.
pub fn parse_excludes(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in text.split(';').map(str::trim).filter(|name| !name.is_empty()) {
        if !out.iter().any(|seen| seen == name) {
            out.push(name.to_owned());
        }
    }
    out
}

pub fn format_excludes(names: &[String]) -> String {
    names.join(";")
}

/// The cells of a mapping row an edit changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MappingPatch {
    pub local: Option<String>,
    pub deployment: Option<String>,
    pub web: Option<String>,
}

/// `mappings` with the row `index` patched.
pub fn update_mapping(mappings: &[DeploymentMapping], index: usize, patch: &MappingPatch) -> Vec<DeploymentMapping> {
    let mut out = mappings.to_vec();
    if let Some(row) = out.get_mut(index) {
        let cells =
            [(&mut row.local, &patch.local), (&mut row.deployment, &patch.deployment), (&mut row.web, &patch.web)];
        for (cell, value) in cells {
            if let Some(value) = value {
                cell.clone_from(value);
            }
        }
    }
    out
}

pub fn add_mapping(mappings: &[DeploymentMapping]) -> Vec<DeploymentMapping> {
    let mut out = mappings.to_vec();
    out.push(DeploymentMapping::default());
    out
}

pub fn remove_mapping(mappings: &[DeploymentMapping], index: usize) -> Vec<DeploymentMapping> {
    let mut out = mappings.to_vec();
    if index < out.len() {
        out.remove(index);
    }
    out
}

/// "Server 3": the first free "Server <n>" name.
pub fn next_server_name(servers: &[DeploymentServer]) -> String {
    (servers.len() + 1..)
        .map(|n| format!("Server {n}"))
        .find(|name| !servers.iter().any(|server| server.name == *name))
        .unwrap_or_default()
}

fn base36(mut n: u64) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

/// Makes the ids of new servers, run targets and SSH configurations: unique in a session by a
/// counter, and across sessions by the time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteIds {
    made: u64,
}

impl RemoteIds {
    /// `<prefix>-<time in base 36>-<count>`.
    pub fn next(&mut self, prefix: &str, now_ms: u64) -> String {
        self.made += 1;
        format!("{prefix}-{}-{}", base36(now_ms), self.made)
    }
}

/// A new server (SFTP, one empty mapping row), or a copy of `base`; either with the fresh `id`.
pub fn new_server(servers: &[DeploymentServer], base: Option<&DeploymentServer>, id: String) -> DeploymentServer {
    match base {
        Some(base) => DeploymentServer { id, name: format!("{} (copy)", base.name), ..base.clone() },
        None => DeploymentServer {
            id,
            name: next_server_name(servers),
            mappings: vec![DeploymentMapping::default()],
            ..DeploymentServer::default()
        },
    }
}

/// The default server after `servers` changed: kept when still there, else the first one (or none).
pub fn fix_default_server(servers: &[DeploymentServer], default_server: &str) -> String {
    let kept = servers.iter().find(|server| server.id == default_server);
    kept.or(servers.first()).map(|server| server.id.clone()).unwrap_or_default()
}

fn without_trailing_slashes(path: &str) -> &str {
    path.trim_end_matches('/')
}

/// The parent of an absolute remote folder (`/` stays `/`).
pub fn remote_parent(path: &str) -> &str {
    let trimmed = if path.len() > 1 { without_trailing_slashes(path) } else { path };
    match trimmed.rfind('/') {
        Some(slash) if slash > 0 => &trimmed[..slash],
        _ => "/",
    }
}

/// `path` relative to the folder `root` (`""` for `root` itself), for a chosen mapping folder;
/// `None` outside it.
pub fn folder_within<'p>(root: &str, path: &'p str) -> Option<&'p str> {
    let root = without_trailing_slashes(root);
    let path = without_trailing_slashes(path);
    if root.is_empty() {
        return None;
    }
    match path.strip_prefix(root)? {
        "" => Some(""),
        rest => rest.strip_prefix('/'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::settings::DeploymentType;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|&name| name.to_owned()).collect()
    }

    fn mapping(local: &str, deployment: &str, web: &str) -> DeploymentMapping {
        DeploymentMapping { local: local.to_owned(), deployment: deployment.to_owned(), web: web.to_owned() }
    }

    fn rows() -> Vec<DeploymentMapping> {
        vec![mapping("", "", ""), mapping("public", "htdocs", "/")]
    }

    fn server(id: &str, name: &str) -> DeploymentServer {
        DeploymentServer {
            id: id.to_owned(),
            name: name.to_owned(),
            mappings: vec![DeploymentMapping::default()],
            ..DeploymentServer::default()
        }
    }

    // exclude names › parses a ;-separated list
    #[test]
    fn parses_a_semicolon_separated_list() {
        assert_eq!(parse_excludes(".git; .idea;;*.pyc ;"), [".git", ".idea", "*.pyc"]);
        assert!(parse_excludes("").is_empty());
        assert_eq!(parse_excludes("a;a;b"), ["a", "b"]);
    }

    // exclude names › formats and round-trips
    #[test]
    fn formats_and_round_trips() {
        let list = names(&[".git", "*.log", "node_modules"]);
        assert_eq!(format_excludes(&list), ".git;*.log;node_modules");
        assert_eq!(parse_excludes(&format_excludes(&list)), list);
    }

    // mappings table › edits one cell
    #[test]
    fn edits_one_cell() {
        let rows = rows();
        let patch = MappingPatch { deployment: Some("www".to_owned()), ..MappingPatch::default() };
        let next = update_mapping(&rows, 1, &patch);
        assert_eq!(next[1], mapping("public", "www", "/"));
        assert_eq!(next[0], rows[0]);
        assert_eq!(rows[1].deployment, "htdocs");
    }

    // mappings table › adds and removes rows
    #[test]
    fn adds_and_removes_rows() {
        let rows = rows();
        assert_eq!(add_mapping(&rows).len(), 3);
        assert_eq!(add_mapping(&rows)[2], mapping("", "", ""));
        assert_eq!(remove_mapping(&rows, 0), [rows[1].clone()]);
        assert_eq!(remove_mapping(&rows, 5), rows);
    }

    // server list › names new servers and copies existing ones
    #[test]
    fn names_new_servers_and_copies_existing_ones() {
        let a = server("a", "Server 1");
        let mut ids = RemoteIds::default();
        assert_eq!(next_server_name(std::slice::from_ref(&a)), "Server 2");
        assert_eq!(next_server_name(&[server("a", "Server 2")]), "Server 3");
        let fresh = new_server(std::slice::from_ref(&a), None, ids.next("server", 1_700_000_000_000));
        assert_eq!(fresh.kind, DeploymentType::Sftp);
        assert_eq!(fresh.name, "Server 2");
        assert!(!fresh.id.is_empty());
        assert_eq!(fresh.mappings, [mapping("", "", "")]);
        let base =
            DeploymentServer { root_path: "/srv".to_owned(), excluded_paths: names(&["/srv/cache"]), ..a.clone() };
        let copy = new_server(std::slice::from_ref(&a), Some(&base), ids.next("server", 1_700_000_000_000));
        assert_eq!((copy.name.as_str(), copy.root_path.as_str()), ("Server 1 (copy)", "/srv"));
        assert_eq!(copy.excluded_paths, ["/srv/cache"]);
        assert_ne!(copy.id, "a");
        assert_ne!(copy.id, fresh.id);
    }

    // server list › copies a local server without changing its type or mappings
    #[test]
    fn copies_a_local_server_without_changing_its_type_or_mappings() {
        let local =
            DeploymentServer { kind: DeploymentType::Local, root_path: "/mounted/www".to_owned(), ..server("a", "S") };
        let copy = new_server(std::slice::from_ref(&local), Some(&local), "server-x-1".to_owned());
        assert_eq!(copy.kind, DeploymentType::Local);
        assert_eq!((copy.root_path.as_str(), copy.ssh_config.as_str()), ("/mounted/www", ""));
        assert_eq!(copy.mappings, local.mappings);
    }

    // server list › keeps the default server valid
    #[test]
    fn keeps_the_default_server_valid() {
        let servers = [server("a", "Server 1")];
        assert_eq!(fix_default_server(&servers, "a"), "a");
        assert_eq!(fix_default_server(&servers, "gone"), "a");
        assert_eq!(fix_default_server(&[], "a"), "");
    }

    // server list › walks up remote folders
    #[test]
    fn walks_up_remote_folders() {
        assert_eq!(remote_parent("/var/www/"), "/var");
        assert_eq!(remote_parent("/var"), "/");
    }

    // chosen mapping folders › are relative to the project or the server's root path
    #[test]
    fn are_relative_to_the_project_or_the_servers_root_path() {
        assert_eq!(folder_within("/work/shop", "/work/shop/web/public"), Some("web/public"));
        assert_eq!(folder_within("/var/www/", "/var/www/site"), Some("site"));
        assert_eq!(folder_within("/var/www", "/var/www/"), Some(""));
        assert_eq!(folder_within("/var/www", "/var/wwwroot"), None);
        assert_eq!(folder_within("/var/www", "/srv"), None);
        assert_eq!(folder_within("", "/srv"), None);
    }

    // Not from a spec: ids count up within a session and carry the time in base 36; the backend's
    // default server has no mapping row, the page's new server has one.
    #[test]
    fn remote_ids_are_unique_in_a_session() {
        let mut ids = RemoteIds::default();
        assert_eq!(ids.next("server", 36), "server-10-1");
        assert_eq!(ids.next("target", 35), "target-z-2");
        assert!(DeploymentServer::default().mappings.is_empty());
        assert_eq!(remote_parent("/"), "/");
    }
}
