//! The project's Spring request paths, scanned once per project (and workspace members) and kept
//! until a mapping source changes.
//!
//! A scan is asked for with a ticket; the caller fetches the mapping sources and hands them back
//! with that ticket. An answer for a scan that was superseded or invalidated is dropped.

use super::spring::spring_controllers;
use crate::everywhere::routes::EndpointEntry;
use crate::platform::paths::{ipc_path, is_same_or_inside};
use crate::platform::virtual_docs::relative_path;
use ide_host::ops::routes::RouteSource;
use std::sync::Arc;

/// Whether `path` is a source a mapping can live in (a Java or Kotlin file).
pub fn is_route_source(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".java", ".kt", ".kts"].iter().any(|ext| lower.ends_with(ext))
}

/// What an index is built for: the project root and, for a workspace, its member folders.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    root: String,
    members: Option<Vec<String>>,
}

/// The answer to a lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteLookup {
    /// The routes, shared with every other lookup of the same index.
    Ready(Arc<Vec<EndpointEntry>>),
    /// Fetch the mapping sources and call [`RouteIndex::scanned`] with this ticket.
    Scan { ticket: u64 },
    /// A scan for the same index is under way: wait for its ticket's answer.
    Pending { ticket: u64 },
}

/// The request-path index of one project.
#[derive(Debug, Default)]
pub struct RouteIndex {
    cached: Option<(Key, Arc<Vec<EndpointEntry>>)>,
    scanning: Option<(Key, u64)>,
    tickets: u64,
}

impl RouteIndex {
    /// The routes of `root`; `members` are the workspace's member folders when the open project is
    /// a workspace rooted there.
    pub fn routes(&mut self, root: Option<&str>, members: Option<&[String]>) -> RouteLookup {
        let Some(root) = root else { return RouteLookup::Ready(Arc::default()) };
        let key = Key { root: root.to_owned(), members: members.map(<[String]>::to_vec) };
        if let Some((cached, routes)) = &self.cached
            && *cached == key
        {
            return RouteLookup::Ready(Arc::clone(routes));
        }
        if let Some((scanning, ticket)) = &self.scanning
            && *scanning == key
        {
            return RouteLookup::Pending { ticket: *ticket };
        }
        self.tickets += 1;
        self.scanning = Some((key, self.tickets));
        RouteLookup::Scan { ticket: self.tickets }
    }

    /// The mapping sources a scan fetched; the routes, or `None` when the scan is no longer wanted.
    pub fn scanned(&mut self, ticket: u64, sources: &[RouteSource]) -> Option<Arc<Vec<EndpointEntry>>> {
        let (key, _) = self.scanning.take_if(|(_, t)| *t == ticket)?;
        let routes = Arc::new(scan(&key, sources));
        self.cached = Some((key, Arc::clone(&routes)));
        Some(routes)
    }

    /// Forgets the index, so the next lookup rescans the project.
    pub fn clear(&mut self) {
        self.cached = None;
        self.scanning = None;
    }

    /// Files were created, modified or removed: a change to a mapping source forgets the index.
    /// Returns whether it did.
    pub fn files_changed<'a>(&mut self, paths: impl IntoIterator<Item = &'a str>) -> bool {
        let stale = paths.into_iter().any(is_route_source);
        if stale {
            self.clear();
        }
        stale
    }
}

/// The member folder holding `path`: the longest member that is it or contains it.
fn member_of<'a>(members: &'a [String], path: &str) -> Option<&'a str> {
    members
        .iter()
        .filter(|m| path == m.as_str() || path.strip_prefix(m.as_str()).is_some_and(|rest| rest.starts_with('/')))
        .max_by_key(|m| m.len())
        .map(String::as_str)
}

/// 0-based line and UTF-16 column of a UTF-16 offset.
fn line_col(text: &str, offset: usize) -> (u32, u32) {
    let (mut line, mut col, mut at) = (0u32, 0u32, 0usize);
    for c in text.chars() {
        if at >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += u32::try_from(c.len_utf16()).unwrap_or(1);
        }
        at += c.len_utf16();
    }
    (line, col)
}

fn scan(key: &Key, sources: &[RouteSource]) -> Vec<EndpointEntry> {
    let mut routes = Vec::new();
    for source in sources {
        let path = ipc_path(&source.path.to_string_lossy());
        let folder = match &key.members {
            Some(members) => member_of(members, &path),
            None => Some(key.root.as_str()),
        };
        let Some(folder) = folder else { continue };
        let folder = ipc_path(folder);
        let base = folder.strip_suffix('/').unwrap_or(&folder);
        if !is_same_or_inside(&path, base) {
            continue;
        }
        for controller in spring_controllers(&source.text) {
            for endpoint in &controller.endpoints {
                let (line, col) = line_col(&source.text, endpoint.offset);
                // A mapping without a path answers nothing to look up by URL; the method itself is
                // still reachable through the symbol index.
                for url in &endpoint.paths {
                    routes.push(EndpointEntry {
                        method: endpoint.http_method.clone(),
                        url: url.clone(),
                        controller: controller.name.clone(),
                        method_name: endpoint.method_name.clone(),
                        path: path.clone(),
                        rel: relative_path(base, &path).into_owned(),
                        line,
                        col,
                        ..EndpointEntry::default()
                    });
                }
            }
        }
    }
    routes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IpcPath;
    use std::path::PathBuf;

    fn source(method: &str, url: &str, path: &str) -> RouteSource {
        RouteSource {
            path: IpcPath(PathBuf::from(path)),
            text: format!(
                "@RestController\n@RequestMapping(\"/deeplink\")\nclass DeepLinkController {{\n    \
                 @PostMapping(\"{url}\")\n    \
                 String {method}() {{ return \"ok\"; }}\n}}"
            ),
        }
    }

    fn deep_link(path: &str) -> RouteSource {
        source("searchMenus", "/menu/search", path)
    }

    fn members(list: &[&str]) -> Vec<String> {
        list.iter().map(|m| (*m).to_owned()).collect()
    }

    fn scan_now(
        index: &mut RouteIndex,
        root: &str,
        members: Option<&[String]>,
        sources: &[RouteSource],
    ) -> Arc<Vec<EndpointEntry>> {
        match index.routes(Some(root), members) {
            RouteLookup::Ready(routes) => routes,
            RouteLookup::Scan { ticket } => index.scanned(ticket, sources).expect("current scan"),
            RouteLookup::Pending { .. } => panic!("no scan should be pending"),
        }
    }

    #[test]
    fn is_invalidated_by_a_change_to_a_mapping_source() {
        for (path, expected) in
            [("/p/A.java", true), ("/p/B.kt", true), ("/p/C.kts", true), ("/p/pom.xml", false), ("/p/notes.md", false)]
        {
            assert_eq!(is_route_source(path), expected, "{path}");
        }
    }

    #[test]
    fn includes_linked_members_with_member_relative_paths_and_excludes_files_outside_every_member() {
        let mut index = RouteIndex::default();
        let ws = members(&["/p", "/linked", "/linked/nested"]);
        let sources = [
            deep_link("/p/DeepLinkController.java"),
            deep_link("/linked/src/Controller.kt"),
            deep_link("/linked/nested/src/NestedController.java"),
            deep_link("/linked-other/Outside.java"),
            deep_link("/workspace/State.java"),
        ];
        let routes = scan_now(&mut index, "/workspace", Some(&ws), &sources);
        let found: Vec<(&str, &str, u32, u32)> =
            routes.iter().map(|r| (r.path.as_str(), r.rel.as_str(), r.line, r.col)).collect();
        assert_eq!(
            found,
            [
                ("/p/DeepLinkController.java", "DeepLinkController.java", 4, 11),
                ("/linked/src/Controller.kt", "src/Controller.kt", 4, 11),
                ("/linked/nested/src/NestedController.java", "src/NestedController.java", 4, 11),
            ]
        );
    }

    #[test]
    fn rescans_when_members_are_added_or_removed_without_changing_the_workspace_root() {
        let mut index = RouteIndex::default();
        let sources = [deep_link("/p/DeepLinkController.java"), deep_link("/linked/Controller.java")];
        let mut scans = 0;
        let mut lookup = |index: &mut RouteIndex, ws: &[&str]| {
            let ws = members(ws);
            match index.routes(Some("/workspace"), Some(&ws)) {
                RouteLookup::Ready(routes) => routes,
                RouteLookup::Scan { ticket } => {
                    scans += 1;
                    index.scanned(ticket, &sources).expect("current scan")
                }
                RouteLookup::Pending { .. } => panic!("no scan should be pending"),
            }
        };
        assert_eq!(lookup(&mut index, &["/p"]).len(), 1);
        let both = lookup(&mut index, &["/p", "/linked"]);
        assert_eq!(both.len(), 2);
        assert!(Arc::ptr_eq(&lookup(&mut index, &["/p", "/linked"]), &both));
        let linked = lookup(&mut index, &["/linked"]);
        assert_eq!(linked.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["/linked/Controller.java"]);
        assert!(lookup(&mut index, &[]).is_empty());
        assert_eq!(scans, 4);
    }

    #[test]
    fn scans_once_per_root_with_method_coordinates_and_rescans_for_another_root() {
        let mut index = RouteIndex::default();
        let sources = [deep_link("/p/DeepLinkController.java")];
        assert_eq!(index.routes(None, None), RouteLookup::Ready(Arc::default()));
        let RouteLookup::Scan { ticket } = index.routes(Some("/p"), None) else { panic!("expected a scan") };
        let first = index.scanned(ticket, &sources).expect("current scan");
        assert_eq!(
            *first,
            [EndpointEntry {
                method: "POST".to_owned(),
                url: "/deeplink/menu/search".to_owned(),
                controller: "DeepLinkController".to_owned(),
                method_name: "searchMenus".to_owned(),
                path: "/p/DeepLinkController.java".to_owned(),
                rel: "DeepLinkController.java".to_owned(),
                line: 4,
                col: 11,
                ..EndpointEntry::default()
            }]
        );
        let RouteLookup::Ready(again) = index.routes(Some("/p"), None) else { panic!("expected the cached routes") };
        assert!(Arc::ptr_eq(&again, &first));
        let RouteLookup::Scan { ticket } = index.routes(Some("/other"), None) else { panic!("expected a rescan") };
        assert!(index.scanned(ticket, &sources).expect("current scan").is_empty());
    }

    #[test]
    fn matches_sources_by_path_segments_whatever_separator_the_host_used() {
        let mut index = RouteIndex::default();
        let sources = [
            deep_link(r"D:\work\demo\src\DeepLinkController.java"),
            source("other", "/menu/search", "D:/work/demo2/DeepLinkController.java"),
        ];
        let routes = scan_now(&mut index, "D:/work/demo", None, &sources);
        let found: Vec<(&str, &str, &str)> =
            routes.iter().map(|r| (r.method_name.as_str(), r.path.as_str(), r.rel.as_str())).collect();
        assert_eq!(found, [("searchMenus", "D:/work/demo/src/DeepLinkController.java", "src/DeepLinkController.java")]);
    }

    #[test]
    fn invalidates_on_created_modified_or_removed_mapping_sources() {
        let mut index = RouteIndex::default();
        let first = scan_now(&mut index, "/p", None, &[deep_link("/p/DeepLinkController.java")]);
        assert_eq!(first[0].method_name, "searchMenus");
        assert!(!index.files_changed(["/p/pom.xml"]));
        assert!(matches!(index.routes(Some("/p"), None), RouteLookup::Ready(_)));
        assert!(index.files_changed(["/p/A.java"]));
        let sources = [source("listMenus", "/menu/list", "/p/DeepLinkController.java")];
        let changed = scan_now(&mut index, "/p", None, &sources);
        assert_eq!(changed[0].url, "/deeplink/menu/list");
    }

    #[test]
    fn drops_the_answer_of_a_scan_that_was_superseded() {
        let mut index = RouteIndex::default();
        let RouteLookup::Scan { ticket: old } = index.routes(Some("/p"), None) else { panic!("expected a scan") };
        assert_eq!(index.routes(Some("/p"), None), RouteLookup::Pending { ticket: old });
        let RouteLookup::Scan { ticket } = index.routes(Some("/other"), None) else { panic!("expected a scan") };
        assert!(index.scanned(old, &[deep_link("/p/DeepLinkController.java")]).is_none());
        assert!(index.scanned(ticket, &[]).is_some());
    }
}
