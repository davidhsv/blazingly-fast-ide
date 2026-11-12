//! Moving along a diff's chain to the next file that has a textual change, without opening the
//! binary, unreadable or unchanged files on the way.
//!
//! [`ChainSearch`] walks the chain one file at a time: [`ChainSearch::step`] says what the
//! application has to fetch for the file under inspection, and the answer moves the search on.

use super::diff_input::{DiffData, SideLoad, chain_neighbour, side_file, side_load};
use super::diff_options::{HighlightPolicy, IgnorePolicy, idea_chunks};
use super::doc::{Direction, Doc};
use ide_vfs::FileProbe;

/// What a [`ChainSearch`] needs next.
#[derive(Clone, Debug, PartialEq)]
pub enum ChainStep {
    /// Probe the files on disk and answer with [`ChainSearch::probed`].
    Probe { paths: Vec<String> },
    /// Load both sides and answer with [`ChainSearch::loaded`].
    Load { left: SideLoad, right: SideLoad },
    /// The search is over: the file to open, or none when no textual change remains.
    Done(Option<Box<DiffData>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Probe,
    Load,
    Found,
}

/// The search for the neighbour of a diff, in one direction of its chain, that has changes to show
/// under the viewer's whitespace policy.
#[derive(Clone, Debug)]
pub struct ChainSearch {
    direction: Direction,
    ignore: IgnorePolicy,
    /// The file under inspection; `None` once the chain is exhausted.
    candidate: Option<DiffData>,
    stage: Stage,
}

/// The files of a diff that are on disk.
fn disk_paths(data: &DiffData) -> Vec<String> {
    [side_file(&data.left, &data.path), side_file(&data.right, &data.path)]
        .into_iter()
        .flatten()
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `text` with every line break as `\n`, as the viewer holds it.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

impl ChainSearch {
    pub fn new(data: &DiffData, direction: Direction, ignore: IgnorePolicy) -> Self {
        let mut search = Self { direction, ignore, candidate: None, stage: Stage::Probe };
        search.inspect(chain_neighbour(data, direction));
        search
    }

    fn inspect(&mut self, candidate: Option<DiffData>) {
        // A file with no side on disk has nothing to probe.
        let on_disk = candidate.as_ref().is_some_and(|data| !disk_paths(data).is_empty());
        self.stage = if on_disk { Stage::Probe } else { Stage::Load };
        self.candidate = candidate;
    }

    /// Moves on to the next file of the chain.
    fn skip(&mut self) {
        let next = self.candidate.as_ref().and_then(|data| chain_neighbour(data, self.direction));
        self.inspect(next);
    }

    pub fn step(&self) -> ChainStep {
        let Some(data) = &self.candidate else { return ChainStep::Done(None) };
        match self.stage {
            Stage::Probe => ChainStep::Probe { paths: disk_paths(data) },
            Stage::Load => {
                ChainStep::Load { left: side_load(&data.path, &data.left), right: side_load(&data.path, &data.right) }
            }
            Stage::Found => ChainStep::Done(Some(Box::new(data.clone()))),
        }
    }

    /// The probes of the paths asked for; `None` when one failed (a stale chain can hold a file that
    /// was deleted or is no longer readable). A binary or oversized file is skipped before its text
    /// is loaded.
    pub fn probed(&mut self, probes: Option<&[FileProbe]>) {
        if self.stage != Stage::Probe {
            return;
        }
        match probes {
            Some(probes) if !probes.iter().any(|probe| probe.binary || probe.too_large) => self.stage = Stage::Load,
            _ => self.skip(),
        }
    }

    /// The texts of the left and the right side; `None` when one could not be loaded.
    pub fn loaded(&mut self, texts: Option<(&str, &str)>) {
        if self.stage != Stage::Load {
            return;
        }
        let changed = texts.is_some_and(|(left, right)| {
            if left.contains('\0') || right.contains('\0') {
                return false;
            }
            let (a, b) = (Doc::new(&normalized(left)), Doc::new(&normalized(right)));
            !idea_chunks(&a, &b, self.ignore, HighlightPolicy::None).chunks.is_empty()
        });
        if changed {
            self.stage = Stage::Found;
        } else {
            self.skip();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_input::{DiffSide, Reveal, with_chain};

    fn diff(name: &str, text: &str) -> DiffData {
        DiffData::new(&format!("/repo/{name}"), DiffSide::text("before\n", "Before"), DiffSide::text(text, "After"))
    }

    fn changed(name: &str) -> DiffData {
        diff(name, "after\n")
    }

    fn chain(items: &[DiffData], index: usize) -> DiffData {
        with_chain(items, index).expect("the item exists")
    }

    /// Runs a search to its end. `probe` answers for one path (`None`: the probe fails); the probes
    /// made are counted.
    fn search(
        data: &DiffData,
        direction: Direction,
        ignore: IgnorePolicy,
        probe: impl Fn(&str) -> Option<FileProbe>,
        probes: &mut usize,
    ) -> Option<DiffData> {
        let text = |load: &SideLoad| match load {
            SideLoad::Text(text) => Some(text.clone()),
            SideLoad::WorkingText { .. } => Some("after\n".to_owned()),
            _ => None,
        };
        let mut search = ChainSearch::new(data, direction, ignore);
        loop {
            match search.step() {
                ChainStep::Probe { paths } => {
                    *probes += paths.len();
                    let found: Option<Vec<FileProbe>> = paths.iter().map(|path| probe(path)).collect();
                    search.probed(found.as_deref());
                }
                ChainStep::Load { left, right } => {
                    let texts = text(&left).zip(text(&right));
                    search.loaded(texts.as_ref().map(|(left, right)| (left.as_str(), right.as_str())));
                }
                ChainStep::Done(found) => return found.map(|data| *data),
            }
        }
    }

    fn text_file(_: &str) -> Option<FileProbe> {
        Some(FileProbe { size: 6, binary: false, too_large: false })
    }

    fn neighbour(data: &DiffData, direction: Direction, ignore: IgnorePolicy) -> Option<DiffData> {
        search(data, direction, ignore, text_file, &mut 0)
    }

    #[test]
    fn skips_unchanged_and_binary_revisions_in_either_direction() {
        let items = [changed("a"), diff("binary", "\0binary"), diff("unchanged", "before\n"), changed("z")];
        for (direction, start, path, reveal) in
            [(Direction::Next, 0, "/repo/z", Reveal::First), (Direction::Previous, 3, "/repo/a", Reveal::Last)]
        {
            let next = neighbour(&chain(&items, start), direction, IgnorePolicy::None).expect("a changed file remains");
            assert_eq!((next.path.as_str(), next.reveal), (path, Some(reveal)));
            assert_eq!(neighbour(&next, direction, IgnorePolicy::None), None);
        }
    }

    #[test]
    fn skips_binary_oversized_and_unreadable_working_files_before_loading_their_text() {
        let probe = |path: &str| {
            if path.ends_with("unreadable") {
                return None;
            }
            Some(FileProbe { size: 6, binary: path.ends_with("binary"), too_large: path.ends_with("large") })
        };
        let working = |name: &str| DiffData { right: DiffSide::worktree(), ..changed(name) };
        let items = [changed("a"), working("binary"), working("large"), working("unreadable"), changed("z")];
        let mut probes = 0;
        let next = search(&chain(&items, 0), Direction::Next, IgnorePolicy::None, probe, &mut probes);
        assert_eq!(next.map(|data| data.path), Some("/repo/z".to_owned()));
        assert_eq!(probes, 3);
    }

    #[test]
    fn uses_the_viewer_whitespace_policy_and_stops_when_no_textual_changes_remain() {
        let data = chain(&[changed("a"), diff("space", " before \n")], 0);
        let spaced = neighbour(&data, Direction::Next, IgnorePolicy::None);
        assert_eq!(spaced.map(|data| data.path), Some("/repo/space".to_owned()));
        assert_eq!(neighbour(&data, Direction::Next, IgnorePolicy::Whitespace), None);
        assert_eq!(neighbour(&changed("single"), Direction::Next, IgnorePolicy::None), None);
    }

    #[test]
    fn normalizes_line_endings_like_the_viewer_when_skipping_unchanged_files() {
        let data = chain(&[changed("a"), diff("crlf", "before\r\n"), changed("z")], 0);
        let next = neighbour(&data, Direction::Next, IgnorePolicy::None);
        assert_eq!(next.map(|data| data.path), Some("/repo/z".to_owned()));
    }
}
