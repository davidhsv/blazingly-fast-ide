//! Gutter markers of document indexers for the files open in editors: asked in bounded batches, no
//! file reads, no polling while nothing changes.
//!
//! [`PluginMarkers`] calls nothing: watching a file or a change of the navigation providers returns
//! a [`MarkerEffect`] (a timer, then a fetch), and [`PluginMarkers::answered`] takes the result. An
//! answer from before the navigation epoch changed (another project, new providers) or from before
//! the last watcher left is dropped.

use ide_plugin::indexing::FileMarkers;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Delay before the visible files' markers are asked for.
pub const REFRESH_DELAY_MS: u64 = 200;
/// Most paths per `plugin_navigation_markers` call.
pub const MARKER_BATCH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerRequest {
    /// The navigation epoch when it was made.
    pub epoch: u64,
    session: u64,
    /// The calls to make, in order.
    pub batches: Vec<Vec<PathBuf>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerEffect {
    /// Call [`PluginMarkers::refresh`] after this many milliseconds.
    Schedule(u64),
}

/// The markers of the visible files.
#[derive(Debug, Default)]
pub struct PluginMarkers {
    entries: HashMap<PathBuf, FileMarkers>,
    /// Watched paths in the order they were first watched, with their watcher counts.
    visible: Vec<(PathBuf, usize)>,
    timer: bool,
    running: bool,
    again: bool,
    session: u64,
}

impl PluginMarkers {
    pub fn entry(&self, path: &Path) -> Option<&FileMarkers> {
        self.entries.get(path)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn schedule(&mut self) -> Vec<MarkerEffect> {
        self.again = true;
        if self.timer || self.running {
            return Vec::new();
        }
        self.timer = true;
        vec![MarkerEffect::Schedule(REFRESH_DELAY_MS)]
    }

    /// An editor shows `path`; several editors of one file share a watch.
    pub fn watch(&mut self, path: &Path) -> Vec<MarkerEffect> {
        if path.as_os_str().is_empty() {
            return Vec::new();
        }
        match self.visible.iter_mut().find(|(p, _)| p == path) {
            Some((_, count)) => *count += 1,
            None => self.visible.push((path.to_owned(), 1)),
        }
        self.schedule()
    }

    /// An editor of `path` closed. When the last watch goes, everything is forgotten and an answer
    /// in flight is dropped.
    pub fn unwatch(&mut self, path: &Path) {
        if let Some(at) = self.visible.iter().position(|(p, _)| p == path) {
            self.visible[at].1 -= 1;
            if self.visible[at].1 == 0 {
                self.visible.remove(at);
            }
        }
        if self.visible.is_empty() {
            self.session += 1;
            self.timer = false;
            self.again = false;
            self.entries.clear();
        }
    }

    /// The navigation providers changed (indexes updated, another project).
    pub fn providers_changed(&mut self) -> Vec<MarkerEffect> {
        if self.visible.is_empty() { Vec::new() } else { self.schedule() }
    }

    /// The timer fired: the markers of every watched path, in batches.
    pub fn refresh(&mut self, epoch: u64) -> Option<MarkerRequest> {
        self.timer = false;
        if self.running {
            return None;
        }
        self.running = true;
        self.again = false;
        let paths: Vec<PathBuf> = self.visible.iter().map(|(p, _)| p.clone()).collect();
        let batches = paths.chunks(MARKER_BATCH).map(<[PathBuf]>::to_vec).collect();
        Some(MarkerRequest { epoch, session: self.session, batches })
    }

    /// The results of `request`'s batches (`Err`: navigation is unavailable, during a project close
    /// or reload), with the navigation epoch now. Returns whether the entries changed and what to
    /// schedule next.
    pub fn answered(
        &mut self,
        request: &MarkerRequest,
        epoch: u64,
        results: Result<Vec<FileMarkers>, String>,
    ) -> (bool, Vec<MarkerEffect>) {
        self.running = false;
        let current = request.epoch == epoch && request.session == self.session;
        let mut changed = false;
        if current {
            let mut entries: HashMap<PathBuf, FileMarkers> = HashMap::new();
            if let Ok(results) = results {
                for result in results {
                    if !self.visible.iter().any(|(p, _)| *p == result.path) {
                        continue;
                    }
                    match entries.get_mut(&result.path) {
                        Some(previous) => previous.markers.extend(result.markers),
                        None => {
                            entries.insert(result.path.clone(), result);
                        }
                    }
                }
            }
            self.entries = entries;
            changed = true;
        }
        let next = if self.again { self.schedule() } else { Vec::new() };
        (changed, next)
    }

    /// Drops the markers of `paths` and the markers pointing into them; `None`: all.
    pub fn invalidate(&mut self, paths: Option<&[String]>) -> bool {
        let before = self.entries.len();
        /// Whether `path` is one of `paths`.
        fn named(paths: &[String], path: &Path) -> bool {
            paths.iter().any(|q| Path::new(q) == path)
        }
        self.entries.retain(|path, entry| {
            let Some(paths) = paths else { return false };
            !named(paths, path) && !entry.markers.iter().any(|m| m.targets.iter().any(|t| named(paths, &t.path)))
        });
        before != self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Location, Pos, Range};
    use ide_plugin::indexing::IndexedMarker;

    #[test]
    fn batches_visible_paths_shares_watchers_and_does_no_polling_while_idle() {
        let mut markers = PluginMarkers::default();
        let mut scheduled = Vec::new();
        for i in 0..130 {
            scheduled.extend(markers.watch(Path::new(&format!("/p/{i}.java"))));
        }
        scheduled.extend(markers.watch(Path::new("/p/0.java")));
        assert_eq!(scheduled, vec![MarkerEffect::Schedule(REFRESH_DELAY_MS)], "one timer for the burst");
        let request = markers.refresh(1).expect("a request");
        assert_eq!(request.batches.iter().map(Vec::len).collect::<Vec<_>>(), vec![64, 64, 2]);
        let (changed, next) = markers.answered(&request, 1, Ok(Vec::new()));
        assert!(changed);
        assert_eq!(next, Vec::new(), "nothing is asked again while idle");
    }

    #[test]
    fn discards_in_flight_results_after_the_project_provider_epoch_changes() {
        let mut markers = PluginMarkers::default();
        markers.watch(Path::new("/old/A.xml"));
        let request = markers.refresh(1).expect("a request");
        markers.providers_changed();
        let result = FileMarkers { path: "/old/A.xml".into(), version: Some(1), markers: Vec::new() };
        let (changed, next) = markers.answered(&request, 2, Ok(vec![result]));
        assert!(!changed);
        assert_eq!(next, vec![MarkerEffect::Schedule(REFRESH_DELAY_MS)], "asked again for the new epoch");
        assert!(markers.is_empty());
    }

    #[test]
    fn invalidates_both_changed_files_and_markers_targeting_them() {
        let mut markers = PluginMarkers::default();
        markers.watch(Path::new("/p/A.xml"));
        let request = markers.refresh(1).expect("a request");
        let range = Range::new(Pos::new(0, 0), Pos::new(0, 1));
        let marker = IndexedMarker {
            range,
            targets: vec![Location::file("/p/A.java".into(), range)],
            plugin_id: "p".into(),
            icon: None,
        };
        let entry = FileMarkers { path: "/p/A.xml".into(), version: Some(1), markers: vec![marker] };
        markers.answered(&request, 1, Ok(vec![entry]));
        assert_eq!(markers.len(), 1);
        assert!(markers.invalidate(Some(&["/p/A.java".to_owned()][..])));
        assert!(markers.is_empty());
    }
}
