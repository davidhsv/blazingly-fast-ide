//! Unity projects: an asset created in the IDE and added to Git takes its `.meta` file along.
//!
//! Unity writes the `.meta` file of a new asset on its own, some time after the asset appears.
//! Only files the IDE created and whose addition succeeded are paired, for the project and
//! repository they were created in: [`UnityMetadataStaging`] remembers them until their metadata
//! shows up in a status, and [`MetadataAddition`] follows one addition from the file operation to
//! the end of its `git add`.
//!
//! Nothing here runs git. A method that returns paths asks the caller to stage them (titled by
//! [`stage_title`]) and to call [`UnityMetadataStaging::stage_finished`] when that ended.

use super::store::VcsState;
use crate::platform::i18n::{I18n, Param};
use crate::platform::paths::{ipc_path, is_same_or_inside};
use crate::vcs::changes::{ChangeKind, FileStatus, root_of, strip_slash};
use std::collections::{HashMap, HashSet};

/// What the pairing reads of the window's project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectState {
    pub root: Option<String>,
    /// The project is a Unity project.
    pub unity: bool,
    /// Another project is being opened in the window.
    pub opening: bool,
}

fn path_key(path: &str) -> String {
    strip_slash(&ipc_path(path)).to_owned()
}

fn untracked(file: Option<&FileStatus>) -> bool {
    file.is_some_and(|file| {
        file.unstaged == Some(ChangeKind::Untracked) && file.staged.is_none() && !file.path.ends_with('/')
    })
}

/// The status is a complete, successful snapshot of the repository.
fn ready(vcs: &VcsState) -> bool {
    vcs.available && vcs.status_loaded && !vcs.loading && vcs.error.as_deref().is_none_or(str::is_empty)
}

fn selected_repo(vcs: &VcsState) -> Option<&str> {
    vcs.repo.as_deref().or(vcs.roots.first().map(String::as_str)).filter(|repo| !repo.is_empty())
}

fn status_by_path(vcs: &VcsState) -> HashMap<String, &FileStatus> {
    vcs.status.iter().map(|file| (path_key(&file.path), file)).collect()
}

fn push_unique(out: &mut Vec<String>, path: &str) {
    if !out.iter().any(|known| known == path) {
        out.push(path.to_owned());
    }
}

/// Title of the operation that stages `count` files.
pub fn stage_title(count: usize, i18n: &mut I18n) -> String {
    if count == 1 {
        i18n.tx("Add 1 file to Git")
    } else {
        i18n.tx_built(&["Add ", " files to Git"], &[Param::from(count)])
    }
}

/// An asset whose metadata is waited for.
#[derive(Debug)]
struct PendingMetadata {
    repo: String,
    /// The asset's own addition succeeded: its metadata may be added without asking again.
    confirmed: bool,
    /// The asset was seen staged: once it no longer is, the user took it back.
    seen_added: bool,
    /// The addition that registered it.
    addition: u64,
}

#[derive(Debug, Default)]
pub struct UnityMetadataStaging {
    /// Asset path → what is known of it, in the order the assets were registered.
    pending: Vec<(String, PendingMetadata)>,
    /// Bumped whenever everything pending is dropped; an addition started before is void.
    generation: u64,
    additions: u64,
    disposed: bool,
    /// A `git add` of metadata files is running.
    staging: bool,
}

impl UnityMetadataStaging {
    fn clear(&mut self) {
        self.generation += 1;
        self.pending.clear();
    }

    /// The metadata files to stage now: those of confirmed assets that showed up untracked. Assets
    /// that were unstaged, deleted, moved to another repository, or whose metadata is not a new
    /// file are forgotten.
    pub fn drain(&mut self, vcs: &VcsState) -> Vec<String> {
        if self.disposed || self.pending.is_empty() || !ready(vcs) {
            return Vec::new();
        }
        let status = status_by_path(vcs);
        let staging = self.staging;
        let mut files = Vec::new();
        self.pending.retain_mut(|(asset, entry)| {
            let meta_path = format!("{asset}.meta");
            let source = status.get(asset.as_str()).copied();
            let meta = status.get(meta_path.as_str()).copied();
            let added = source.is_some_and(|source| source.staged == Some(ChangeKind::Added));
            entry.seen_added |= added;
            let repo = Some(entry.repo.as_str());
            let gone = ((entry.confirmed || entry.seen_added) && !added)
                || source.is_some_and(|source| source.unstaged == Some(ChangeKind::Deleted))
                || root_of(asset, &vcs.roots) != repo
                || root_of(&meta_path, &vcs.roots) != repo
                || meta.is_some_and(|meta| !untracked(Some(meta)));
            if gone {
                return false;
            }
            match meta {
                // Consent is consumed before the addition runs: a failed one must not be retried on
                // every refresh.
                Some(meta) if entry.confirmed && !staging => {
                    files.push(meta.path.clone());
                    false
                }
                _ => true,
            }
        });
        self.staging |= !files.is_empty();
        files
    }

    /// The `git add` [`UnityMetadataStaging::drain`] asked for ended: what to stage next.
    pub fn stage_finished(&mut self, vcs: &VcsState) -> Vec<String> {
        self.staging = false;
        self.drain(vcs)
    }

    /// Follows the repository state. Another project or selected repository voids everything
    /// pending; otherwise returns the metadata files to stage.
    pub fn vcs_changed(&mut self, prev: &VcsState, now: &VcsState) -> Vec<String> {
        // The first refresh discovers the primary repository; that is not a user switch.
        let switched = selected_repo(prev).is_some_and(|before| selected_repo(now) != Some(before));
        if now.root != prev.root || !now.available || switched {
            self.clear();
            return Vec::new();
        }
        self.drain(now)
    }

    /// Follows the window's project: another project, or one being opened, voids everything pending.
    pub fn project_changed(&mut self, prev: &ProjectState, now: &ProjectState) {
        if now.root != prev.root || now.unity != prev.unity || (now.opening && !prev.opening) {
            self.clear();
        }
    }

    /// Starts following the addition of the files at `paths`, which the IDE just created.
    pub fn begin<S: AsRef<str>>(&mut self, paths: &[S], project: &ProjectState) -> MetadataAddition {
        self.additions += 1;
        let root = project.root.as_deref().filter(|_| project.unity);
        MetadataAddition {
            id: self.additions,
            started: self.generation,
            assets_dir: root.map(|root| format!("{}/Assets", path_key(root))),
            requested: paths.iter().map(|path| path_key(path.as_ref())).collect(),
            attempt: Vec::new(),
            assets: None,
        }
    }

    /// Stops pairing for good.
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.clear();
    }
}

/// One addition of files the IDE created, from the file operation to the end of its `git add`.
#[derive(Debug)]
pub struct MetadataAddition {
    id: u64,
    /// The staging's generation when the addition started.
    started: u64,
    /// The project's `Assets` folder; `None` outside Unity projects.
    assets_dir: Option<String>,
    /// The created paths.
    requested: HashSet<String>,
    /// Assets this addition registered as pending.
    attempt: Vec<String>,
    /// Asset → repository of the created files that take their metadata along; settled when the
    /// files to add are first looked at.
    assets: Option<Vec<(String, String)>>,
}

impl MetadataAddition {
    /// Nothing voided the addition since it started.
    pub fn current(&self, staging: &UnityMetadataStaging, project: &ProjectState) -> bool {
        !staging.disposed && staging.generation == self.started && !project.opening
    }

    fn pair(&self, files: &[String], vcs: &VcsState, status: &HashMap<String, &FileStatus>) -> Vec<(String, String)> {
        let mut paired: Vec<(String, String)> = Vec::new();
        let Some(assets_dir) = self.assets_dir.as_deref() else { return paired };
        for file in files {
            let path = path_key(file);
            let is_asset = self.requested.contains(&path)
                && path != assets_dir
                && is_same_or_inside(&path, assets_dir)
                && !path.to_lowercase().ends_with(".meta")
                && untracked(status.get(&path).copied());
            let Some(repo) = root_of(&path, &vcs.roots).filter(|_| is_asset) else { continue };
            let known = paired.iter().any(|(known, _)| *known == path);
            if !known && root_of(&format!("{path}.meta"), &vcs.roots) == Some(repo) {
                paired.push((path, repo.to_owned()));
            }
        }
        paired
    }

    /// The files to add for `files` (untracked files the operation covers): without assets and
    /// metadata that are no longer new files, with the metadata of the created assets that exists
    /// by now.
    pub fn include(&mut self, files: &[String], vcs: &VcsState) -> Vec<String> {
        let status = status_by_path(vcs);
        let mut paired = match self.assets.take() {
            Some(paired) => paired,
            None => self.pair(files, vcs, &status),
        };
        let is_paired = |path: &str| paired.iter().any(|(asset, _)| asset == path);
        let mut out: Vec<String> = Vec::new();
        for file in files {
            let path = path_key(file);
            let pairs = is_paired(&path) || is_paired(path.strip_suffix(".meta").unwrap_or(&path));
            if !pairs || untracked(status.get(&path).copied()) {
                push_unique(&mut out, file);
            }
        }
        paired.retain(|(asset, repo)| {
            let meta_path = format!("{asset}.meta");
            if !untracked(status.get(asset).copied()) || root_of(asset, &vcs.roots) != Some(repo.as_str()) {
                out.retain(|file| file != asset && *file != meta_path);
                return false;
            }
            if let Some(meta) = status.get(&meta_path).copied()
                && untracked(Some(meta))
                && root_of(&path_key(&meta.path), &vcs.roots) == Some(repo.as_str())
            {
                push_unique(&mut out, &meta.path);
            }
            true
        });
        self.assets = Some(paired);
        out
    }

    /// Whether the status has to be refreshed before [`MetadataAddition::before_stage`]: created
    /// assets are among the files, and their metadata may have appeared meanwhile.
    pub fn needs_refresh(&self) -> bool {
        self.assets.as_ref().is_some_and(|assets| !assets.is_empty())
    }

    /// The files to stage for `files`, right before staging them (after the refresh
    /// [`MetadataAddition::needs_refresh`] asked for). Assets whose metadata is not among them are
    /// registered as pending. Empty when the addition was voided or the status is not usable.
    pub fn before_stage(
        &mut self,
        files: &[String],
        staging: &mut UnityMetadataStaging,
        vcs: &VcsState,
        project: &ProjectState,
    ) -> Vec<String> {
        if (self.needs_refresh() && !ready(vcs)) || !self.current(staging, project) {
            return Vec::new();
        }
        let out = self.include(files, vcs);
        let selected: HashSet<String> = out.iter().map(|file| path_key(file)).collect();
        for (asset, repo) in self.assets.iter().flatten() {
            // Cancellation is observed while the addition is in flight, but metadata is not added
            // until it succeeded. Metadata already among the files is finished.
            if selected.contains(&format!("{asset}.meta")) {
                continue;
            }
            let entry = PendingMetadata { repo: repo.clone(), confirmed: false, seen_added: false, addition: self.id };
            self.attempt.push(asset.clone());
            match staging.pending.iter_mut().find(|(known, _)| known == asset) {
                Some(slot) => slot.1 = entry,
                None => staging.pending.push((asset.clone(), entry)),
            }
        }
        out
    }

    /// The `git add` ended (`ok`: it succeeded). The assets it registered become confirmed, or are
    /// forgotten when it failed or the addition was voided. Returns whether the caller refreshes
    /// the status and then calls [`MetadataAddition::settled`].
    pub fn finished(&mut self, ok: bool, staging: &mut UnityMetadataStaging, project: &ProjectState) -> bool {
        let confirmed = ok && self.current(staging, project);
        for asset in &self.attempt {
            let own = |(known, entry): &(String, PendingMetadata)| known == asset && entry.addition == self.id;
            let Some(index) = staging.pending.iter().position(own) else { continue };
            if confirmed {
                staging.pending[index].1.confirmed = true;
            } else {
                staging.pending.remove(index);
            }
        }
        confirmed && !self.attempt.is_empty()
    }

    /// The refresh [`MetadataAddition::finished`] asked for ended: the metadata files to stage now.
    pub fn settled(&self, staging: &mut UnityMetadataStaging, vcs: &VcsState, project: &ProjectState) -> Vec<String> {
        if self.current(staging, project) { staging.drain(vcs) } else { Vec::new() }
    }
}
