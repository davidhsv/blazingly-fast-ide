//! Plugin-owned decorations: highlights, underlines and after-text in editors, badges and colours in
//! the project tree. Each plugin replaces its whole set at once; sets of another project root, of
//! unknown styles or with positions outside the document are left out. Text a plugin adds is
//! plain text, never markup.
//!
//! [`PluginDecorations`] keeps the snapshots; [`PluginDecorations::catch_up`] asks for what was
//! published before a window opened, and a plugin's newer publication wins over that answer.

use ide_core::{Pos, Range};
use ide_host::plugins::DecorationSet;
use std::collections::HashMap;

/// The styles a plugin may use, with the theme token each draws in.
pub fn style_token(style: &str) -> Option<&'static str> {
    Some(match style {
        "highlight" => "accent",
        "search" => "fg-search-match",
        "added" => "vcs-added",
        "modified" => "vcs-modified",
        "removed" => "vcs-deleted",
        "warning" => "warning",
        "error" => "danger",
        "info" => "info",
        "underline" | "strikethrough" => "fg-editor",
        "muted" => "fg-muted",
        _ => return None,
    })
}

/// The UTF-16 offset of each line start of a document, to turn positions into offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineStarts {
    /// Start offset and UTF-16 length of each line.
    lines: Vec<(usize, usize)>,
}

impl LineStarts {
    pub fn new(text: &str) -> Self {
        let mut lines = Vec::new();
        let mut from = 0;
        for line in text.split('\n') {
            let len = line.encode_utf16().count();
            lines.push((from, len));
            from += len + 1;
        }
        Self { lines }
    }

    /// The offset of `pos`, `None` when the line or column is outside the document.
    pub fn offset(&self, pos: Pos) -> Option<usize> {
        let (from, len) = *self.lines.get(pos.line as usize)?;
        (pos.col as usize <= len).then_some(from + pos.col as usize)
    }

    /// The offsets of `range`, `None` when an end is outside the document.
    pub fn range(&self, range: Range) -> Option<(usize, usize)> {
        Some((self.offset(range.start)?, self.offset(range.end)?))
    }
}

/// One decoration of an editor, in UTF-16 offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorDecoration {
    pub from: usize,
    pub to: usize,
    pub style: String,
    /// Shown on hover.
    pub hover: Option<String>,
    /// Plain text drawn after `to`.
    pub after: Option<String>,
}

/// A file's decoration in the project tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeDecoration {
    /// The theme token of its colour.
    pub color: &'static str,
    pub badge: Option<String>,
    pub tooltip: Option<String>,
    pub faded: bool,
    pub error: bool,
}

/// A request for the decorations published before this window listened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatchUp {
    revision: u64,
    versions: HashMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct PluginDecorations {
    snapshots: HashMap<String, Vec<DecorationSet>>,
    versions: HashMap<String, u64>,
    revision: u64,
}

impl PluginDecorations {
    /// A plugin replaced its sets (`DecorationsChanged`); editors and trees draw again.
    pub fn set(&mut self, plugin_id: &str, sets: Vec<DecorationSet>) {
        *self.versions.entry(plugin_id.to_owned()).or_default() += 1;
        self.snapshots.insert(plugin_id.to_owned(), sets);
    }

    /// The plugins with sets.
    pub fn plugins(&self) -> impl Iterator<Item = &str> {
        self.snapshots.keys().map(String::as_str)
    }

    /// Asks for every plugin's sets (`plugin_decoration_sets`), on start and when the project changes.
    pub fn catch_up(&mut self) -> CatchUp {
        self.revision += 1;
        CatchUp { revision: self.revision, versions: self.versions.clone() }
    }

    /// The answer to [`PluginDecorations::catch_up`]: sets of plugins that published nothing since
    /// replace theirs, and those of plugins the answer omits are dropped. Returns the plugins whose
    /// sets changed; a superseded answer changes nothing.
    pub fn caught_up(&mut self, request: &CatchUp, all: HashMap<String, Vec<DecorationSet>>) -> Vec<String> {
        if request.revision != self.revision {
            return Vec::new();
        }
        let unchanged =
            |versions: &HashMap<String, u64>, plugin: &str| versions.get(plugin) == request.versions.get(plugin);
        let mut changed = Vec::new();
        let gone: Vec<String> =
            self.snapshots.keys().filter(|p| !all.contains_key(*p) && unchanged(&self.versions, p)).cloned().collect();
        for plugin in gone {
            self.set(&plugin, Vec::new());
            changed.push(plugin);
        }
        for (plugin, sets) in all {
            if unchanged(&self.versions, &plugin) {
                self.set(&plugin, sets);
                changed.push(plugin);
            }
        }
        changed
    }

    /// The decorations of the editor of `path` in project `root`, for a document of `text`.
    pub fn editor(&self, root: Option<&str>, path: &str, text: &str) -> Vec<EditorDecoration> {
        let lines = LineStarts::new(text);
        let mut out = Vec::new();
        for set in self.snapshots.values().flatten() {
            if set.tree || root != Some(&*set.root.to_string_lossy()) || set.path.to_string_lossy() != path {
                continue;
            }
            for item in &set.items {
                if style_token(&item.style).is_none() {
                    continue;
                }
                let Some((from, to)) = lines.range(item.range).filter(|(from, to)| from <= to) else { continue };
                if from == to && item.after.is_none() {
                    continue;
                }
                out.push(EditorDecoration {
                    from,
                    to,
                    style: item.style.clone(),
                    hover: item.hover.clone(),
                    after: item.after.clone(),
                });
            }
        }
        out.sort_by_key(|d| (d.from, d.to));
        out
    }

    /// The project tree decorations of `plugin_id` in project `root`, by path.
    pub fn tree(&self, root: Option<&str>, plugin_id: &str) -> HashMap<String, TreeDecoration> {
        let mut items = HashMap::new();
        for set in self.snapshots.get(plugin_id).into_iter().flatten() {
            if !set.tree || root != Some(&*set.root.to_string_lossy()) {
                continue;
            }
            let Some(file) = &set.file else { continue };
            let Some(color) = style_token(&file.style) else { continue };
            items.insert(
                set.path.to_string_lossy().into_owned(),
                TreeDecoration {
                    color,
                    badge: file.badge.clone(),
                    tooltip: file.tooltip.clone(),
                    faded: file.style == "muted",
                    error: file.style == "error",
                },
            );
        }
        items
    }

    /// Stops: every plugin's sets are withdrawn.
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.versions.clear();
    }
}

/// What an `EditorRanges` event does to an editor of `text`: the selections, or `None` when any
/// range is outside the document (the editor is left alone).
pub fn editor_ranges(text: &str, ranges: &[Range]) -> Option<Vec<(usize, usize)>> {
    if ranges.is_empty() {
        return None;
    }
    let lines = LineStarts::new(text);
    ranges.iter().map(|range| lines.range(*range)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_plugin::{Decoration, FileDecoration};

    const TEXT: &str = "😀text";

    fn range() -> Range {
        Range::new(Pos::new(0, 2), Pos::new(0, 4))
    }

    fn set(root: &str) -> DecorationSet {
        DecorationSet {
            root: root.into(),
            path: "/p/a.txt".into(),
            set_id: "test".into(),
            tree: false,
            items: vec![Decoration {
                range: range(),
                style: "highlight".into(),
                hover: Some("hover".into()),
                after: Some("<plain>".into()),
            }],
            file: None,
        }
    }

    #[test]
    fn renders_plain_annotations_and_withdraws_them() {
        let mut decorations = PluginDecorations::default();
        decorations.set("test", vec![set("/p")]);
        let shown = decorations.editor(Some("/p"), "/p/a.txt", TEXT);
        assert_eq!(
            shown,
            vec![EditorDecoration {
                from: 2,
                to: 4,
                style: "highlight".into(),
                hover: Some("hover".into()),
                after: Some("<plain>".into())
            }]
        );
        let covered: String = String::from_utf16_lossy(&TEXT.encode_utf16().collect::<Vec<_>>()[2..4]);
        assert_eq!(covered, "te");
        decorations.set("test", Vec::new());
        assert!(decorations.editor(Some("/p"), "/p/a.txt", TEXT).is_empty());
    }

    #[test]
    fn filters_project_roots_and_unknown_styles_and_replaces_tree_badges() {
        let mut decorations = PluginDecorations::default();
        decorations.set("test", vec![set("/other")]);
        assert!(decorations.editor(Some("/p"), "/p/a.txt", TEXT).is_empty());
        let mut unknown = set("/p");
        unknown.items[0].style = "unknown".into();
        decorations.set("test", vec![unknown]);
        assert!(decorations.editor(Some("/p"), "/p/a.txt", TEXT).is_empty());
        let (badge, tooltip) = (Some("M".to_owned()), Some("changed".to_owned()));
        let file = FileDecoration { style: "modified".into(), badge, tooltip };
        let tree = DecorationSet { tree: true, items: Vec::new(), file: Some(file), ..set("/p") };
        decorations.set("test", vec![tree]);
        let badge = decorations.tree(Some("/p"), "test").get("/p/a.txt").and_then(|d| d.badge.clone());
        assert_eq!(badge.as_deref(), Some("M"));
        decorations.set("test", Vec::new());
        assert!(decorations.tree(Some("/p"), "test").is_empty());
    }

    #[test]
    fn selects_utf_16_ranges_and_reveals_without_changing_text() {
        assert_eq!(editor_ranges(TEXT, &[range()]), Some(vec![(2, 4)]));
        assert_eq!(editor_ranges(TEXT, &[Range::new(Pos::new(1, 0), Pos::new(1, 1))]), None);
        assert_eq!(editor_ranges(TEXT, &[Range::new(Pos::new(0, 0), Pos::new(0, 9))]), None);
    }

    #[test]
    fn a_catch_up_answer_loses_to_a_newer_publication() {
        let mut decorations = PluginDecorations::default();
        decorations.set("stale", vec![set("/p")]);
        let request = decorations.catch_up();
        decorations.set("live", vec![set("/p")]);
        let all = HashMap::from([("live".to_owned(), Vec::new()), ("new".to_owned(), vec![set("/p")])]);
        let mut changed = decorations.caught_up(&request, all);
        changed.sort();
        assert_eq!(changed, vec!["new".to_owned(), "stale".to_owned()]);
        assert_eq!(decorations.editor(Some("/p"), "/p/a.txt", TEXT).len(), 2, "live kept its own, new arrived");
        let _newer = decorations.catch_up();
        let late = HashMap::from([("late".to_owned(), vec![set("/p")])]);
        assert_eq!(decorations.caught_up(&request, late), Vec::<String>::new(), "superseded");
    }
}
