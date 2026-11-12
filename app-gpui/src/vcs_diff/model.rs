//! What version control shows about a file outside its own views: the colour, tooltip and folder
//! highlighting of the project tree and the tabs, together with the small text helpers of the Git
//! views (stash subjects, progress lines, remote web pages, counts and timestamps).

use crate::platform::date_format::{CivilDateTime, DateFormatOptions, format_day, format_timestamp};
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::{EffectiveKind, FileStatus, effective_kind, strip_slash};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Name of the theme token colouring files of `kind`.
pub fn kind_color(kind: EffectiveKind) -> &'static str {
    match kind {
        EffectiveKind::Modified | EffectiveKind::Renamed => "vcs-modified",
        EffectiveKind::Added => "vcs-added",
        EffectiveKind::Deleted => "vcs-deleted",
        EffectiveKind::Untracked => "vcs-untracked",
        EffectiveKind::Ignored => "vcs-ignored",
        EffectiveKind::Conflicted => "vcs-conflict",
    }
}

/// English name of `kind`.
pub fn kind_label(kind: EffectiveKind) -> &'static str {
    match kind {
        EffectiveKind::Modified => "Modified",
        EffectiveKind::Renamed => "Renamed",
        EffectiveKind::Added => "Added",
        EffectiveKind::Deleted => "Deleted",
        EffectiveKind::Untracked => "Unversioned",
        EffectiveKind::Ignored => "Ignored",
        EffectiveKind::Conflicted => "Merge conflict",
    }
}

/// Folder colour precedence: the earlier kind wins when a folder holds several.
const FOLDER_PRECEDENCE: [EffectiveKind; 6] = [
    EffectiveKind::Conflicted,
    EffectiveKind::Modified,
    EffectiveKind::Renamed,
    EffectiveKind::Added,
    EffectiveKind::Deleted,
    EffectiveKind::Untracked,
];

/// How a path is shown by the project tree, editor tabs and file lists. A field that is `None` says
/// nothing, leaving what another provider said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDecoration {
    /// Name of the theme token holding the text colour, such as `vcs-modified`.
    pub color: Option<String>,
    /// Colour tinting the editor tab.
    pub tint: Option<String>,
    /// Short badge text, such as a problem count.
    pub badge: Option<String>,
    pub tooltip: Option<String>,
    /// Underlined as a file with errors.
    pub error: Option<bool>,
    /// Dimmed (ignored, excluded).
    pub faded: Option<bool>,
    /// The folders above the path are decorated too.
    pub propagate: Option<bool>,
    /// The path is a folder whose whole content shares the decoration.
    pub subtree: Option<bool>,
}

impl FileDecoration {
    /// Takes over every field `top` says something about.
    fn overlay(&mut self, top: &FileDecoration) {
        fn take<T: Clone>(slot: &mut Option<T>, value: &Option<T>) {
            if value.is_some() {
                slot.clone_from(value);
            }
        }
        take(&mut self.color, &top.color);
        take(&mut self.tint, &top.tint);
        take(&mut self.badge, &top.badge);
        take(&mut self.tooltip, &top.tooltip);
        take(&mut self.error, &top.error);
        take(&mut self.faded, &top.faded);
        take(&mut self.propagate, &top.propagate);
        take(&mut self.subtree, &top.subtree);
    }
}

/// Decorations for the files of `status`, by absolute path, in the order folders pick their colour
/// from: the most significant kind first (conflict, then modified…). `propagate`: folders holding
/// changed files are highlighted.
pub fn status_decorations(status: &[FileStatus], propagate: bool, i18n: &mut I18n) -> Vec<(String, FileDecoration)> {
    let mut entries: Vec<(String, FileDecoration, usize)> = Vec::new();
    for fs in status {
        let Some(kind) = effective_kind(fs) else { continue };
        let label = kind_label(kind);
        let tooltip = match &fs.renamed_from {
            Some(from) if !from.is_empty() => {
                i18n.tx_built(&["", " (from ", ")"], &[Param::Text(label.to_owned()), Param::Text(from.clone())])
            }
            _ => label.to_owned(),
        };
        let mut decoration =
            FileDecoration { color: Some(kind_color(kind).to_owned()), tooltip: Some(tooltip), ..Default::default() };
        if kind == EffectiveKind::Ignored {
            decoration.faded = Some(true);
        } else if propagate {
            decoration.propagate = Some(true);
        }
        // An untracked or ignored folder is one `dir/` entry: its whole content is decorated.
        if fs.path.ends_with('/') {
            decoration.subtree = Some(true);
        }
        let rank = FOLDER_PRECEDENCE.iter().position(|k| *k == kind).unwrap_or(FOLDER_PRECEDENCE.len());
        entries.push((strip_slash(&fs.path).to_owned(), decoration, rank));
    }
    entries.sort_by_key(|(_, _, rank)| *rank);
    // A path listed twice keeps its first place and its last decoration.
    let mut out: Vec<(String, FileDecoration)> = Vec::with_capacity(entries.len());
    let mut index: HashMap<String, usize> = HashMap::new();
    for (path, decoration, _) in entries {
        match index.get(&path) {
            Some(at) => out[*at].1 = decoration,
            None => {
                index.insert(path.clone(), out.len());
                out.push((path, decoration));
            }
        }
    }
    out
}

/// What a provider's decorations make of the folders above them.
#[derive(Clone, Debug, Default)]
struct FolderDecoration {
    color: Option<String>,
    error: Option<bool>,
}

#[derive(Clone, Debug, Default)]
struct ProviderDecorations {
    paths: HashMap<String, FileDecoration>,
    /// Folders holding decorated descendants.
    folders: HashMap<String, FolderDecoration>,
    /// Folders whose decoration covers everything below them.
    subtrees: HashMap<String, FileDecoration>,
}

/// The decorations of every provider (version control status, problems), merged per path. Later
/// providers win per field.
#[derive(Clone, Debug, Default)]
pub struct FileDecorations {
    /// In the order the providers first reported.
    providers: Vec<(String, ProviderDecorations)>,
}

/// The folders above `path`, nearest first, without the root.
fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.rmatch_indices('/').map(|(i, _)| i).filter(|i| *i > 0).map(move |i| &path[..i])
}

impl FileDecorations {
    /// The order providers are merged in; one not listed comes first.
    const PROVIDER_ORDER: [&'static str; 2] = ["vcs", "problems"];

    /// Replaces all decorations of `provider`. A folder takes the colour of the first decoration
    /// below it that propagates.
    pub fn set(&mut self, provider: &str, decorations: Vec<(String, FileDecoration)>) {
        let mut next = ProviderDecorations::default();
        for (path, d) in &decorations {
            if d.subtree == Some(true) {
                let covering = FileDecoration {
                    color: d.color.clone(),
                    tooltip: d.tooltip.clone(),
                    faded: d.faded,
                    error: d.error,
                    ..Default::default()
                };
                next.subtrees.insert(path.clone(), covering);
            }
            if d.propagate != Some(true) {
                continue;
            }
            for dir in ancestors(path) {
                let folder = next.folders.entry(dir.to_owned()).or_default();
                if folder.color.is_none() {
                    folder.color.clone_from(&d.color);
                }
                if folder.error != Some(true) {
                    folder.error = d.error;
                }
            }
        }
        next.paths = decorations.into_iter().collect();
        match self.providers.iter_mut().find(|(id, _)| id == provider) {
            Some((_, present)) => *present = next,
            None => self.providers.push((provider.to_owned(), next)),
        }
    }

    /// The decoration of `path`: its own, else what its folder content gives it, else what a
    /// decorated folder above it says about everything below.
    pub fn get(&self, path: &str) -> Option<FileDecoration> {
        let rank = |id: &str| Self::PROVIDER_ORDER.iter().position(|known| *known == id).map_or(0, |at| at + 1);
        let mut providers: Vec<&(String, ProviderDecorations)> = self.providers.iter().collect();
        providers.sort_by_key(|(id, _)| rank(id));
        let mut out: Option<FileDecoration> = None;
        for (_, provider) in providers {
            if let Some(own) = provider.paths.get(path) {
                out.get_or_insert_with(FileDecoration::default).overlay(own);
            } else if let Some(folder) = provider.folders.get(path) {
                // A folder entry states both of its fields, set or not.
                let merged = out.get_or_insert_with(FileDecoration::default);
                merged.color.clone_from(&folder.color);
                merged.error = folder.error;
            } else if let Some(covering) = ancestors(path).find_map(|dir| provider.subtrees.get(dir)) {
                // So does the decoration a folder gives its content.
                let merged = out.get_or_insert_with(FileDecoration::default);
                merged.color.clone_from(&covering.color);
                merged.tooltip.clone_from(&covering.tooltip);
                merged.faded = covering.faded;
                merged.error = covering.error;
            }
        }
        out
    }
}

/// A stash subject split into its branch and the text the stash list shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashLabel {
    pub branch: Option<String>,
    pub text: String,
}

/// Splits a stash subject (`On main: msg`, `WIP on main: 1a2b3c subject`) into the branch and the
/// user-facing text; any other subject is the text as it is.
pub fn stash_label(message: &str) -> StashLabel {
    static SUBJECT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)^(?:WIP on|On) ([^:]+): ?(.*)$").expect("the pattern is valid"));
    match SUBJECT.captures(message) {
        None => StashLabel { branch: None, text: message.to_owned() },
        Some(found) => {
            let text = found[2].trim();
            StashLabel {
                branch: Some(found[1].to_owned()),
                text: if text.is_empty() { message.to_owned() } else { text.to_owned() },
            }
        }
    }
}

/// The fraction a git progress line (`Receiving objects:  45% (9/20)`) reports.
pub fn progress_fraction(line: &str) -> Option<f64> {
    static PERCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]{1,3})%").expect("the pattern is valid"));
    let n: u32 = PERCENT.captures(line)?[1].parse().ok()?;
    (n <= 100).then(|| f64::from(n) / 100.0)
}

/// Web page of a git remote URL (`git@github.com:o/r.git`, `ssh://git@host:22/o/r`,
/// `https://user@host/o/r.git`); `None` for a local path.
pub fn remote_web_url(url: &str) -> Option<String> {
    static FULL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:ssh|git|https?)://(?:[^@/]+@)?([^/:]+)(?::[0-9]+)?/(.+)$").expect("the pattern is valid")
    });
    // `host:path`, the path not starting with a slash.
    static SCP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:[^@/]+@)?([^:/]+):([^/\n].*)$").expect("the pattern is valid"));
    let trimmed = url.trim().trim_end_matches('/');
    let u = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let found = FULL.captures(u).or_else(|| SCP.captures(u))?;
    Some(format!("https://{}/{}", &found[1], found[2].trim_start_matches('/')))
}

/// Page of the commit `sha` on the remote's web UI.
pub fn commit_web_url(remote_url: &str, sha: &str) -> Option<String> {
    let base = remote_web_url(remote_url)?;
    // Bitbucket names the page differently.
    let page = if base.contains("bitbucket.org/") { "commits" } else { "commit" };
    Some(format!("{base}/{page}/{sha}"))
}

/// "N file(s)", worded for the locale.
pub fn file_count(n: usize, i18n: &mut I18n) -> String {
    let unit = if n == 1 { " file" } else { " files" };
    i18n.tx_built(&["", unit], &[Param::from(n)])
}

/// A list timestamp as the date format settings say; empty without a time. `ms` and `now_ms` are
/// Unix times, shown `utc_offset_minutes` east of UTC.
pub fn format_time(
    ms: Option<f64>,
    now_ms: f64,
    utc_offset_minutes: i32,
    options: &DateFormatOptions,
    i18n: &mut I18n,
) -> String {
    match ms {
        Some(ms) if ms != 0.0 && !ms.is_nan() => {
            format_timestamp(ms, now_ms, utc_offset_minutes, options).localized(i18n)
        }
        _ => String::new(),
    }
}

/// The short date of a Unix time; empty without one.
pub fn format_date(ms: Option<f64>, utc_offset_minutes: i32, options: &DateFormatOptions) -> String {
    match ms {
        Some(ms) if ms != 0.0 && !ms.is_nan() => {
            format_day(CivilDateTime::from_unix_ms(ms, utc_offset_minutes), options, true)
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::changes::ChangeKind;

    fn fs(
        rel: &str,
        staged: Option<ChangeKind>,
        unstaged: Option<ChangeKind>,
        renamed_from: Option<&str>,
    ) -> FileStatus {
        FileStatus {
            path: format!("/repo/{rel}"),
            rel_path: rel.to_owned(),
            staged,
            unstaged,
            renamed_from: renamed_from.map(str::to_owned),
        }
    }

    #[test]
    fn maps_staged_and_unstaged_pairs_like_ideas_change_lists() {
        use ChangeKind::{Added, Conflicted, Deleted, Ignored, Modified, Renamed, Untracked};
        let kind = |staged, unstaged| effective_kind(&fs("a", staged, unstaged, None));
        assert_eq!(kind(None, Some(Modified)), Some(EffectiveKind::Modified));
        assert_eq!(kind(Some(Modified), None), Some(EffectiveKind::Modified));
        assert_eq!(kind(Some(Added), Some(Modified)), Some(EffectiveKind::Added));
        assert_eq!(kind(None, Some(Untracked)), Some(EffectiveKind::Untracked));
        assert_eq!(kind(None, Some(Deleted)), Some(EffectiveKind::Deleted));
        assert_eq!(kind(Some(Deleted), None), Some(EffectiveKind::Deleted));
        assert_eq!(kind(Some(Renamed), None), Some(EffectiveKind::Renamed));
        assert_eq!(kind(None, Some(Conflicted)), Some(EffectiveKind::Conflicted));
        assert_eq!(kind(None, Some(Ignored)), Some(EffectiveKind::Ignored));
        // Added then deleted in the worktree: nothing to show.
        assert_eq!(kind(Some(Added), Some(Deleted)), None);
        assert_eq!(kind(None, None), None);
    }

    #[test]
    fn colours_files_with_the_vcs_tokens_and_propagates_to_folders() {
        let mut i18n = I18n::default();
        let decorations = status_decorations(
            &[
                fs("src/app/Main.kt", None, Some(ChangeKind::Modified), None),
                fs("src/app/New.kt", None, Some(ChangeKind::Untracked), None),
                fs("build/", None, Some(ChangeKind::Ignored), None),
                fs("docs/Moved.md", Some(ChangeKind::Renamed), None, Some("docs/Old.md")),
            ],
            true,
            &mut i18n,
        );
        let get = |path: &str| decorations.iter().find(|(p, _)| p == path).map(|(_, d)| d.clone()).expect(path);
        let main = get("/repo/src/app/Main.kt");
        assert_eq!((main.color.as_deref(), main.propagate), (Some("vcs-modified"), Some(true)));
        let new = get("/repo/src/app/New.kt");
        assert_eq!((new.color.as_deref(), new.propagate), (Some("vcs-untracked"), Some(true)));
        // Directory entries lose the trailing slash; ignored entries are faded and don't propagate.
        let build = get("/repo/build");
        assert_eq!((build.color.as_deref(), build.faded), (Some("vcs-ignored"), Some(true)));
        assert_eq!(build.propagate, None);
        assert!(get("/repo/docs/Moved.md").tooltip.is_some_and(|tooltip| tooltip.contains("docs/Old.md")));
    }

    #[test]
    fn gives_folders_the_most_significant_colour() {
        let mut i18n = I18n::default();
        let mut store = FileDecorations::default();
        store.set(
            "vcs",
            status_decorations(
                &[
                    fs("src/a/New.kt", None, Some(ChangeKind::Untracked), None),
                    fs("src/a/Changed.kt", None, Some(ChangeKind::Modified), None),
                    fs("src/b/Conflict.kt", None, Some(ChangeKind::Conflicted), None),
                    fs("lib/Only.kt", None, Some(ChangeKind::Untracked), None),
                ],
                true,
                &mut i18n,
            ),
        );
        let color = |store: &FileDecorations, path: &str| store.get(path).and_then(|d| d.color);
        assert_eq!(color(&store, "/repo/src/a").as_deref(), Some("vcs-modified"));
        assert_eq!(color(&store, "/repo/src").as_deref(), Some("vcs-conflict"));
        assert_eq!(color(&store, "/repo/lib").as_deref(), Some("vcs-untracked"));
        assert_eq!(color(&store, "/repo/src/a/New.kt").as_deref(), Some("vcs-untracked"));
        store.set("vcs", Vec::new());
        assert_eq!(store.get("/repo/src"), None);
    }

    #[test]
    fn splits_gits_stash_subjects_into_branch_and_message() {
        let label =
            |branch: Option<&str>, text: &str| StashLabel { branch: branch.map(str::to_owned), text: text.to_owned() };
        assert_eq!(stash_label("On main: wip make"), label(Some("main"), "wip make"));
        let wip = stash_label("WIP on feature/x: 1a2b3c4 Add thing");
        assert_eq!(wip, label(Some("feature/x"), "1a2b3c4 Add thing"));
        assert_eq!(stash_label("custom subject"), label(None, "custom subject"));
    }

    // Not from a spec: everything below an untracked folder shares its decoration.
    #[test]
    fn decorates_the_content_of_a_folder_entry() {
        let mut i18n = I18n::default();
        let mut store = FileDecorations::default();
        store.set("vcs", status_decorations(&[fs("new/", None, Some(ChangeKind::Untracked), None)], true, &mut i18n));
        let inside = store.get("/repo/new/deep/file.txt").expect("the folder covers its content");
        assert_eq!(inside.color.as_deref(), Some("vcs-untracked"));
        assert_eq!(inside.subtree, None);
        assert_eq!(store.get("/repo/other.txt"), None);
    }

    // Not from a spec: progress lines, remote pages and counts.
    #[test]
    fn reads_progress_and_remote_urls() {
        assert_eq!(progress_fraction("Receiving objects:  45% (9/20)"), Some(0.45));
        assert_eq!(progress_fraction("done."), None);
        assert_eq!(progress_fraction("900%"), None);
        assert_eq!(remote_web_url("git@github.com:o/r.git").as_deref(), Some("https://github.com/o/r"));
        assert_eq!(remote_web_url("ssh://git@host:22/o/r").as_deref(), Some("https://host/o/r"));
        assert_eq!(remote_web_url("https://user@host/o/r.git/").as_deref(), Some("https://host/o/r"));
        assert_eq!(remote_web_url("/local/path"), None);
        assert_eq!(
            commit_web_url("git@bitbucket.org:o/r.git", "abc").as_deref(),
            Some("https://bitbucket.org/o/r/commits/abc")
        );
        let github = commit_web_url("https://github.com/o/r", "abc");
        assert_eq!(github.as_deref(), Some("https://github.com/o/r/commit/abc"));
        let mut i18n = I18n::default();
        assert_eq!(file_count(1, &mut i18n), "1 file");
        assert_eq!(file_count(3, &mut i18n), "3 files");
    }
}
