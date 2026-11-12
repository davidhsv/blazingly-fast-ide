//! The Markdown editor's layouts and the live preview's state: the layout per file (Editor only,
//! Editor and Preview, Preview only, as IDEA's split editor), what the editor publishes to the
//! preview (text, scroll position, caret moves, font zoom), the preview's scrolling (following the
//! editor side by side; on its own, following navigation in the hidden editor and reporting the
//! line on screen), the `<details>` the reader toggled, its images and its font size.

use super::file_kinds::is_markdown_path;
use super::render::{BlockTop, LinkTarget, preview_line_at_offset, preview_offset_for_line, resolve_link};
use crate::platform::i18n::I18n;
use crate::platform::storage::UiStorage;
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// Layouts

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownMode {
    Editor,
    Split,
    Preview,
}

impl MarkdownMode {
    pub fn id(self) -> &'static str {
        match self {
            MarkdownMode::Editor => "editor",
            MarkdownMode::Split => "split",
            MarkdownMode::Preview => "preview",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [MarkdownMode::Editor, MarkdownMode::Split, MarkdownMode::Preview].into_iter().find(|mode| mode.id() == id)
    }

    /// Whether the preview shows (and follows the editor side by side).
    pub fn shows_preview(self) -> bool {
        self != MarkdownMode::Editor
    }
}

/// The storage key of the layout files open in when not switched yet.
pub const DEFAULT_MODE_KEY: &str = "openidea.markdown.defaultMode";

/// The layout actions: id, title and the layout they switch to.
pub const MODE_ACTIONS: [(&str, &str, MarkdownMode); 3] = [
    ("markdown.editorOnly", "Editor Only", MarkdownMode::Editor),
    ("markdown.editorAndPreview", "Editor and Preview", MarkdownMode::Split),
    ("markdown.previewOnly", "Preview Only", MarkdownMode::Preview),
];

/// The layout of each Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownModes {
    by_path: HashMap<String, MarkdownMode>,
    /// The layout of files not switched yet: the last one chosen.
    fallback: MarkdownMode,
}

impl MarkdownModes {
    pub fn load(storage: &UiStorage) -> Self {
        let fallback = storage.get(DEFAULT_MODE_KEY).and_then(MarkdownMode::from_id).unwrap_or(MarkdownMode::Split);
        Self { by_path: HashMap::new(), fallback }
    }

    pub fn mode_of(&self, path: &str) -> MarkdownMode {
        self.by_path.get(path).copied().unwrap_or(self.fallback)
    }

    pub fn set_mode(&mut self, path: &str, mode: MarkdownMode, storage: &mut UiStorage) {
        self.by_path.insert(path.to_owned(), mode);
        self.fallback = mode;
        storage.set(DEFAULT_MODE_KEY, mode.id().to_owned());
    }

    /// Runs layout action `action_id` for `path` (the action's argument, else the active editor's
    /// file); `false` when it is not a layout action or not a Markdown file.
    pub fn run_action(&mut self, action_id: &str, path: Option<&str>, storage: &mut UiStorage) -> bool {
        let Some((_, _, mode)) = MODE_ACTIONS.iter().find(|(id, _, _)| *id == action_id) else { return false };
        match path.filter(|path| is_markdown_path(path)) {
            Some(path) => {
                self.set_mode(path, *mode, storage);
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// What the editor publishes

/// The live state of one Markdown file, published by its editor for the preview.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveMarkdown {
    pub text: String,
    /// 0-based first visible source line of the editor that scrolled last.
    pub top_line: f64,
    /// Bumped when the editor scrolls, so equal lines still resync.
    pub scroll_seq: u64,
    /// The editor's font zoom in px; the preview scales its text by the same amount.
    pub font_zoom: f32,
    /// The preview's own scroll offset, kept so Preview only survives the preview being rebuilt.
    pub preview_top: f64,
    /// 0-based caret line, set when the caret moves without an edit (navigation).
    pub caret_line: u32,
    /// Bumped with `caret_line`, so going to the same place again still scrolls.
    pub caret_seq: u64,
}

/// Edits reach the preview this long after typing pauses.
pub const TEXT_DELAY_MS: u64 = 120;

/// The live state of every Markdown file open in an editor.
#[derive(Debug, Default)]
pub struct LiveSources {
    files: HashMap<String, LiveMarkdown>,
    /// Edited text waiting for typing to pause: the time it is due and the text.
    pending: HashMap<String, (u64, String)>,
}

impl LiveSources {
    pub fn get(&self, path: &str) -> Option<&LiveMarkdown> {
        self.files.get(path)
    }

    fn entry(&mut self, path: &str) -> &mut LiveMarkdown {
        self.files.entry(path.to_owned()).or_default()
    }

    /// An editor opened the file.
    pub fn opened(&mut self, path: &str, text: &str, font_zoom: f32) {
        let live = self.entry(path);
        live.text = text.to_owned();
        live.font_zoom = font_zoom;
    }

    /// The document changed: published at [`LiveSources::due`] once typing pauses.
    pub fn edited(&mut self, path: &str, text: String, now_ms: u64) {
        self.pending.insert(path.to_owned(), (now_ms + TEXT_DELAY_MS, text));
    }

    /// When the next pending edit is due.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.values().map(|(due, _)| *due).min()
    }

    /// Publishes the edits due at `now_ms`; returns the files whose text changed.
    pub fn due(&mut self, now_ms: u64) -> Vec<String> {
        let ready: Vec<String> =
            self.pending.iter().filter(|(_, (due, _))| *due <= now_ms).map(|(path, _)| path.clone()).collect();
        for path in &ready {
            if let Some((_, text)) = self.pending.remove(path) {
                self.entry(path).text = text;
            }
        }
        ready
    }

    /// The editor closed: its latest edit is published at once, for a preview that outlives it.
    pub fn closed(&mut self, path: &str, text: &str) {
        self.pending.remove(path);
        self.entry(path).text = text.to_owned();
    }

    pub fn scrolled(&mut self, path: &str, top_line: f64) {
        let live = self.entry(path);
        live.top_line = top_line;
        live.scroll_seq += 1;
    }

    /// The caret moved without an edit.
    pub fn caret_moved(&mut self, path: &str, caret_line: u32) {
        let live = self.entry(path);
        live.caret_line = caret_line;
        live.caret_seq += 1;
    }

    pub fn font_zoomed(&mut self, path: &str, font_zoom: f32) {
        self.entry(path).font_zoom = font_zoom;
    }

    /// The preview's own scroll offset, recorded as it scrolls.
    pub fn preview_scrolled(&mut self, path: &str, top: f64) {
        self.entry(path).preview_top = top;
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Preview scrolling

/// What the preview does after an event.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewEffect {
    ScrollTo(f64),
    /// The source line Preview only reports as on screen (Structure's Autoscroll from Source);
    /// `None` when the editor is back or the preview is gone.
    ShownLine(Option<u32>),
}

/// The scrolling of one preview of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewScroll {
    /// Editor and Preview: the preview follows the editor's scrolling.
    sync: bool,
    seen_caret: u64,
    /// The offset navigation scrolled to: its scroll event is not the reader's.
    scrolled_to: Option<f64>,
    reporting: bool,
}

/// The preview's blocks as laid out, and its full height.
#[derive(Debug, Clone, Copy)]
pub struct Layout<'a> {
    pub blocks: &'a [BlockTop],
    pub height: f64,
}

impl PreviewScroll {
    /// The preview is shown (again). On its own it goes back to where it was and reports the line
    /// on screen; side by side the editor's scroll position places it ([`PreviewScroll::editor_scrolled`]).
    pub fn mount(sync: bool, live: Option<&LiveMarkdown>, layout: Layout<'_>) -> (Self, Vec<PreviewEffect>) {
        let mut scroll =
            Self { sync, seen_caret: live.map_or(0, |live| live.caret_seq), scrolled_to: None, reporting: false };
        let mut effects = Vec::new();
        let mut top = 0.0;
        if !sync && let Some(saved) = live.map(|live| live.preview_top).filter(|top| *top > 0.0) {
            top = saved;
            effects.push(PreviewEffect::ScrollTo(saved));
        }
        effects.extend(scroll.start_reporting(layout, top));
        (scroll, effects)
    }

    fn start_reporting(&mut self, layout: Layout<'_>, scroll_top: f64) -> Option<PreviewEffect> {
        if self.sync {
            return None;
        }
        self.reporting = true;
        Some(PreviewEffect::ShownLine(Some(preview_line_at_offset(layout.blocks, scroll_top) as u32)))
    }

    /// Switched between Editor and Preview and Preview only.
    pub fn set_sync(&mut self, sync: bool, layout: Layout<'_>, scroll_top: f64) -> Vec<PreviewEffect> {
        if sync == self.sync {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if self.reporting {
            self.reporting = false;
            effects.push(PreviewEffect::ShownLine(None));
        }
        self.sync = sync;
        effects.extend(self.start_reporting(layout, scroll_top));
        effects
    }

    /// Editor and Preview: the editor scrolled (or the text or the layout changed).
    pub fn editor_scrolled(&self, live: &LiveMarkdown, layout: Layout<'_>) -> Option<PreviewEffect> {
        if !self.sync {
            return None;
        }
        if live.top_line <= 0.0 {
            return Some(PreviewEffect::ScrollTo(0.0));
        }
        Some(PreviewEffect::ScrollTo(preview_offset_for_line(layout.blocks, live.top_line, layout.height)))
    }

    /// Preview only: go where the hidden editor's caret was sent (Structure, Go to Symbol…).
    pub fn caret_moved(&mut self, live: &LiveMarkdown, layout: Layout<'_>) -> Vec<PreviewEffect> {
        if live.caret_seq == self.seen_caret {
            return Vec::new();
        }
        self.seen_caret = live.caret_seq;
        if self.sync {
            return Vec::new();
        }
        let top = preview_offset_for_line(layout.blocks, f64::from(live.caret_line), layout.height);
        self.scrolled_to = Some(top);
        vec![PreviewEffect::ScrollTo(top), PreviewEffect::ShownLine(Some(live.caret_line))]
    }

    /// The preview scrolled to `scroll_top` (after the frame): Preview only reports the line on
    /// screen, unless navigation made the scroll.
    pub fn scrolled(&mut self, layout: Layout<'_>, scroll_top: f64) -> Option<PreviewEffect> {
        if self.sync {
            return None;
        }
        if self.scrolled_to.is_some_and(|own| (scroll_top - own).abs() < 1.0) {
            return None;
        }
        self.scrolled_to = None;
        Some(PreviewEffect::ShownLine(Some(preview_line_at_offset(layout.blocks, scroll_top) as u32)))
    }

    /// The preview goes away.
    pub fn unmount(self) -> Option<PreviewEffect> {
        self.reporting.then_some(PreviewEffect::ShownLine(None))
    }
}

// ---------------------------------------------------------------------------
// Details

/// The `<details>` the reader opened or closed, by position: each edit renders them as written,
/// and these choices are applied on top.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetailsToggles {
    path: String,
    by_index: HashMap<usize, bool>,
}

impl DetailsToggles {
    /// The preview shows `path`: another file's choices are forgotten.
    pub fn show(&mut self, path: &str) {
        if self.path != path {
            self.path = path.to_owned();
            self.by_index.clear();
        }
    }

    /// The reader clicked the summary of the `index`-th `<details>`, open or not before.
    pub fn toggled(&mut self, index: usize, was_open: bool) {
        self.by_index.insert(index, !was_open);
    }

    /// Whether each `<details>` shows open, given how each is written.
    pub fn apply(&self, written: &[bool]) -> Vec<bool> {
        written.iter().enumerate().map(|(i, open)| self.by_index.get(&i).copied().unwrap_or(*open)).collect()
    }
}

// ---------------------------------------------------------------------------
// Links

/// What a click on a link of the preview does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkClick {
    OpenExternal(String),
    /// Scroll the preview to the heading of this `#id`.
    ScrollToAnchor(String),
    OpenFile {
        path: String,
        line: Option<u32>,
    },
}

pub fn link_click(file: &str, href: &str) -> Option<LinkClick> {
    Some(match resolve_link(file, href)? {
        LinkTarget::External { url } => LinkClick::OpenExternal(url),
        LinkTarget::Anchor { id } => LinkClick::ScrollToAnchor(id),
        LinkTarget::File { path, anchor: Some(anchor), .. } if path == file => LinkClick::ScrollToAnchor(anchor),
        LinkTarget::File { path, line, .. } => LinkClick::OpenFile { path, line },
    })
}

// ---------------------------------------------------------------------------
// Images

/// How an image of the preview is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageView {
    /// Shown from this URL (`data:`).
    Shown(String),
    /// Its local file is being read.
    Loading,
    /// Not loaded: a remote image (with this tooltip), or a local one that could not be read.
    Blocked(Option<String>),
}

const IMAGE_CACHE_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    Pending,
    Done(String),
}

/// Local images read as `data:` URLs by absolute path, shared by all previews. A failed read is
/// forgotten, so the next render tries again.
#[derive(Debug, Default)]
pub struct PreviewImages {
    reads: HashMap<String, Read>,
    order: VecDeque<String>,
    failed: HashSet<String>,
}

impl PreviewImages {
    /// A render of the Markdown file `file` shows an image of `source`: the local file to read
    /// (`ide_vfs::read_data_url`), if it is not read or being read already.
    pub fn request(&mut self, file: &str, source: &str) -> Option<String> {
        if source.to_ascii_lowercase().starts_with("data:image/") {
            return None;
        }
        let Some(LinkTarget::File { path, .. }) = resolve_link(file, source) else { return None };
        if self.reads.contains_key(&path) {
            return None;
        }
        if self.reads.len() >= IMAGE_CACHE_LIMIT
            && let Some(oldest) = self.order.pop_front()
        {
            self.reads.remove(&oldest);
        }
        self.failed.remove(&path);
        self.reads.insert(path.clone(), Read::Pending);
        self.order.push_back(path.clone());
        Some(path)
    }

    /// The answer of reading `path`: its `data:` URL, or the error.
    pub fn read(&mut self, path: &str, answer: Result<String, String>) {
        if !self.reads.contains_key(path) {
            return;
        }
        match answer {
            Ok(url) => {
                self.reads.insert(path.to_owned(), Read::Done(url));
            }
            Err(_) => {
                self.reads.remove(path);
                self.order.retain(|other| other != path);
                self.failed.insert(path.to_owned());
            }
        }
    }

    pub fn view(&self, file: &str, source: &str, i18n: &I18n) -> ImageView {
        if source.to_ascii_lowercase().starts_with("data:image/") {
            return ImageView::Shown(source.to_owned());
        }
        match resolve_link(file, source) {
            Some(LinkTarget::File { path, .. }) => match self.reads.get(&path) {
                Some(Read::Done(url)) => ImageView::Shown(url.clone()),
                Some(Read::Pending) => ImageView::Loading,
                None if self.failed.contains(&path) => ImageView::Blocked(None),
                None => ImageView::Loading,
            },
            _ => ImageView::Blocked(Some(i18n.tx("Remote images are not loaded"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Font size

/// The editor's default font size, which the preview follows when nothing is set.
pub const DEFAULT_EDITOR_FONT_SIZE: f64 = 13.0;

/// A stored preview font size within IDEA's 8..72, or `None` (follow the editor font).
fn preview_size(value: Option<&serde_json::Value>) -> Option<f64> {
    let value = value?.as_f64().filter(|v| v.is_finite())?;
    Some((value + 0.5).floor().clamp(8.0, 72.0))
}

/// The Markdown preview's font size in px: Languages & Frameworks › Markdown, else the editor
/// font size (never the UI font), as IDEA defaults it. `settings` is the project's settings JSON.
pub fn markdown_preview_font_size(settings: Option<&serde_json::Value>) -> f64 {
    let settings = settings.unwrap_or(&serde_json::Value::Null);
    let editor = settings.pointer("/editor/fontSize").and_then(serde_json::Value::as_f64).filter(|size| *size > 0.0);
    preview_size(settings.pointer("/languages/markdown/previewFontSize")).or(editor).unwrap_or(DEFAULT_EDITOR_FONT_SIZE)
}

/// The accessible name of the preview.
pub fn preview_label(i18n: &I18n) -> String {
    i18n.tx("Markdown preview")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::viewers::render::render_markdown;
    use serde_json::json;

    /// Each source line is 10px tall, as in the spec's fake layout.
    const LINE: f64 = 10.0;

    fn source() -> String {
        (0..40)
            .map(|i| match i % 10 {
                0 => format!("# H{}", i / 10),
                5 => "text".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tops(text: &str) -> Vec<BlockTop> {
        render_markdown(text)
            .iter()
            .map(|block| BlockTop { line: f64::from(block.line), top: f64::from(block.line) * LINE })
            .collect()
    }

    struct World {
        live: LiveSources,
        blocks: Vec<BlockTop>,
        shown: Option<u32>,
        scroll_top: f64,
    }

    const PATH: &str = "/p/README.md";

    impl World {
        fn new() -> Self {
            let mut live = LiveSources::default();
            let text = source();
            live.opened(PATH, &text, 0.0);
            Self { live, blocks: tops(&text), shown: None, scroll_top: 0.0 }
        }

        fn layout(&self) -> Layout<'_> {
            Layout { blocks: &self.blocks, height: 40.0 * LINE }
        }

        fn apply(&mut self, effects: impl IntoIterator<Item = PreviewEffect>) {
            for effect in effects {
                match effect {
                    PreviewEffect::ScrollTo(top) => {
                        self.scroll_top = top;
                        self.live.preview_scrolled(PATH, top);
                    }
                    PreviewEffect::ShownLine(line) => self.shown = line,
                }
            }
        }

        fn mount(&mut self, sync: bool) -> PreviewScroll {
            let (scroll, effects) = PreviewScroll::mount(sync, self.live.get(PATH), self.layout());
            self.apply(effects);
            scroll
        }

        fn go_to(&mut self, scroll: &mut PreviewScroll, line: u32) {
            self.live.caret_moved(PATH, line);
            let live = self.live.get(PATH).cloned().unwrap_or_default();
            let effects = scroll.caret_moved(&live, Layout { blocks: &self.blocks, height: 40.0 * LINE });
            self.apply(effects);
        }

        fn scroll(&mut self, scroll: &mut PreviewScroll, top: f64) {
            self.scroll_top = top;
            self.live.preview_scrolled(PATH, top);
            let effect = scroll.scrolled(Layout { blocks: &self.blocks, height: 40.0 * LINE }, top);
            self.apply(effect);
        }
    }

    #[test]
    fn scrolls_to_where_the_hidden_editors_caret_is_sent_again_on_the_same_place() {
        let mut world = World::new();
        let mut scroll = world.mount(false);
        assert_eq!(world.shown, Some(0));
        world.go_to(&mut scroll, 20);
        assert_eq!(world.scroll_top, 200.0);
        assert_eq!(world.shown, Some(20));
        world.scroll_top = 0.0;
        world.go_to(&mut scroll, 20);
        assert_eq!(world.scroll_top, 200.0);
    }

    #[test]
    fn reports_the_line_on_screen_as_the_user_scrolls_not_after_navigating() {
        let mut world = World::new();
        let mut scroll = world.mount(false);
        world.go_to(&mut scroll, 30);
        let top = world.scroll_top;
        world.scroll(&mut scroll, top);
        assert_eq!(world.shown, Some(30));
        world.scroll(&mut scroll, 157.0);
        assert_eq!(world.shown, Some(15));
    }

    #[test]
    fn reports_the_restored_line_when_shown_again_after_a_remount() {
        let mut world = World::new();
        world.live.preview_scrolled(PATH, 205.0);
        world.mount(false);
        assert_eq!(world.scroll_top, 205.0);
        assert_eq!(world.shown, Some(20));
    }

    #[test]
    fn leaves_the_split_preview_to_the_editors_scroll_sync() {
        let mut world = World::new();
        let mut scroll = world.mount(true);
        world.go_to(&mut scroll, 20);
        assert_eq!(world.scroll_top, 0.0);
        world.scroll(&mut scroll, 157.0);
        assert_eq!(world.shown, None);
        // The editor's own scrolling places the split preview.
        world.live.scrolled(PATH, 10.0);
        let live = world.live.get(PATH).cloned().unwrap_or_default();
        assert_eq!(scroll.editor_scrolled(&live, world.layout()), Some(PreviewEffect::ScrollTo(100.0)));
    }

    #[test]
    fn stops_standing_in_for_the_editor_when_the_editor_comes_back_or_the_preview_closes() {
        let mut world = World::new();
        let mut scroll = world.mount(false);
        world.go_to(&mut scroll, 20);
        assert_eq!(world.shown, Some(20));
        let top = world.scroll_top;
        let effects = scroll.set_sync(true, world.layout(), top);
        world.apply(effects);
        assert_eq!(world.shown, None);
        let effects = scroll.set_sync(false, world.layout(), top);
        world.apply(effects);
        assert!(world.shown.is_some());
        let effect = scroll.unmount();
        world.apply(effect);
        assert_eq!(world.shown, None);
    }

    #[test]
    fn restores_the_previews_own_scroll_after_a_remount() {
        let mut world = World::new();
        let mut scroll = world.mount(false);
        world.scroll(&mut scroll, 300.0);
        assert_eq!(world.live.get(PATH).map(|live| live.preview_top), Some(300.0));
        let effect = scroll.unmount();
        world.apply(effect);
        world.scroll_top = 0.0;
        world.mount(false);
        assert_eq!(world.scroll_top, 300.0);
    }

    #[test]
    fn keeps_what_the_reader_opened_and_closed_across_edits() {
        use crate::features_b::viewers::render::details_written_open;
        let doc = concat!(
            "<details>\n<summary>One</summary>\n\nFirst\n\n</details>\n\n",
            "<details open>\n<summary>Two</summary>\n\nSecond\n\n</details>\n",
        );
        let mut toggles = DetailsToggles::default();
        toggles.show("/p/NOTES.md");
        let written = details_written_open(&render_markdown(doc));
        assert_eq!(toggles.apply(&written), [false, true]);
        toggles.toggled(0, false);
        toggles.toggled(1, true);
        let edited = details_written_open(&render_markdown(&format!("{doc}\nMore\n")));
        assert_eq!(toggles.apply(&edited), [true, false]);
        toggles.show("/p/OTHER.md");
        assert_eq!(toggles.apply(&edited), [false, true]);
    }

    #[test]
    fn scales_the_markdown_preview_with_its_editor_and_resets_with_it() {
        let mut live = LiveSources::default();
        live.opened(PATH, "# Title", 0.0);
        assert_eq!(live.get(PATH).map(|live| live.font_zoom), Some(0.0));
        live.font_zoomed(PATH, 2.0);
        assert_eq!(live.get(PATH).map(|live| live.font_zoom), Some(2.0));
        live.font_zoomed(PATH, 0.0);
        assert_eq!(live.get(PATH).map(|live| live.font_zoom), Some(0.0));
    }

    #[test]
    fn follows_the_editor_font_size_until_markdown_preview_font_size_is_set_never_the_ui_font() {
        assert_eq!(markdown_preview_font_size(None), 13.0);
        let editor = json!({ "appearance": { "uiFontSize": 11 }, "editor": { "fontSize": 15 } });
        assert_eq!(markdown_preview_font_size(Some(&editor)), 15.0);
        let set = json!({
            "appearance": { "uiFontSize": 11 },
            "editor": { "fontSize": 15 },
            "languages": { "markdown": { "previewFontSize": 20 } }
        });
        assert_eq!(markdown_preview_font_size(Some(&set)), 20.0);
    }

    #[test]
    fn keeps_a_set_size_within_ideas_8_72() {
        let size = |v: serde_json::Value| {
            markdown_preview_font_size(Some(
                &json!({ "editor": { "fontSize": 14 }, "languages": { "markdown": { "previewFontSize": v } } }),
            ))
        };
        assert_eq!(size(json!(3)), 8.0);
        assert_eq!(size(json!(200)), 72.0);
        assert_eq!(size(json!(15.6)), 16.0);
        assert_eq!(size(serde_json::Value::Null), 14.0);
        assert_eq!(size(json!("big")), 14.0);
    }

    // Not from a spec: layouts are remembered per file, the last one chosen is the default.
    #[test]
    fn remembers_the_layout_per_file_and_the_last_one_chosen() {
        let mut storage = UiStorage::default();
        let mut modes = MarkdownModes::load(&storage);
        assert_eq!(modes.mode_of("/p/a.md"), MarkdownMode::Split);
        assert!(modes.run_action("markdown.previewOnly", Some("/p/a.md"), &mut storage));
        assert!(!modes.run_action("markdown.previewOnly", Some("/p/a.ts"), &mut storage));
        assert_eq!(modes.mode_of("/p/b.md"), MarkdownMode::Preview);
        assert_eq!(MarkdownModes::load(&storage).mode_of("/p/c.md"), MarkdownMode::Preview);
    }

    // Not from a spec: data URLs show at once, local files are read once, remote ones are blocked.
    #[test]
    fn loads_local_images_once_and_blocks_remote_ones() {
        let i18n = I18n::default();
        let mut images = PreviewImages::default();
        let file = "/repo/README.md";
        assert_eq!(images.request(file, "data:image/png;base64,AA"), None);
        let inline = "data:image/png;base64,AA";
        assert_eq!(images.view(file, inline, &i18n), ImageView::Shown(inline.into()));
        assert_eq!(images.request(file, "img/a.png").as_deref(), Some("/repo/img/a.png"));
        assert_eq!(images.request(file, "img/a.png"), None);
        assert_eq!(images.view(file, "img/a.png", &i18n), ImageView::Loading);
        images.read("/repo/img/a.png", Ok("data:image/png;base64,BB".into()));
        assert_eq!(images.view(file, "img/a.png", &i18n), ImageView::Shown("data:image/png;base64,BB".into()));
        images.request(file, "b.png");
        images.read("/repo/b.png", Err("missing".into()));
        assert_eq!(images.view(file, "b.png", &i18n), ImageView::Blocked(None));
        assert_eq!(images.request(file, "b.png").as_deref(), Some("/repo/b.png"));
        assert_eq!(
            images.view(file, "https://x.dev/a.png", &i18n),
            ImageView::Blocked(Some("Remote images are not loaded".into()))
        );
    }
}
