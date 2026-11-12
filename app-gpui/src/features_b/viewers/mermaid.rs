//! Mermaid diagrams of the Markdown preview: which diagrams to draw, one at a time, and what each
//! diagram block shows meanwhile. The drawing itself (Mermaid producing SVG) is the caller's: it
//! takes [`MermaidJob`]s from [`Mermaid::next_job`] and answers with [`Mermaid::rendered`].
//!
//! Results are cached by theme and source, so edits elsewhere in the file do not draw diagrams
//! again; while a changed diagram draws, the block keeps showing the drawing before the edit. Each
//! applied SVG gets fresh ids ([`reid`]): two copies of one cached drawing in a document would
//! otherwise share marker ids, and references would resolve against a hidden copy.

use crate::platform::css_color::{resolve_token_color, to_rgb};
use crate::theme::tokens::{Theme, ThemeAttributes};
use regex::{Captures, Regex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

const CACHE_LIMIT: usize = 64;

/// The identity of the theme a diagram is drawn in: it changes whenever the colours the theme
/// tokens resolve to may change (built-in theme, contrast, palette theme, plugin theme, accent and
/// UI font).
pub fn theme_key(attributes: &ThemeAttributes, accent: &str, font: &str) -> String {
    let attribute = |name: &str| attributes.get(name).unwrap_or("").to_owned();
    format!(
        "{}|{}|{}|{}|{accent}|{font}",
        attribute("data-theme"),
        attribute("data-contrast"),
        attribute("data-color-theme"),
        attribute("data-plugin-theme"),
    )
}

/// What Mermaid is initialized with before drawing in a theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidConfig {
    pub start_on_load: bool,
    pub security_level: &'static str,
    pub suppress_error_rendering: bool,
    pub theme: &'static str,
    pub dark_mode: bool,
    /// Mermaid's theme variables (`background`, `primaryColor`…), as hex or `rgb()`.
    pub theme_variables: Vec<(&'static str, String)>,
    pub font_family: Option<String>,
}

/// Diagram colours from the IDE tokens. Mermaid parses only hex and `rgb()`, so tokens a theme
/// writes as `hsl()`, `color-mix()` or `var()` are resolved to RGB.
pub fn mermaid_config(theme: &Theme) -> MermaidConfig {
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(?:#[0-9a-f]{3,8}|rgba?\([^)]*\))$"));
    let token = |names: &[&str]| {
        names.iter().find_map(|name| {
            let value = theme.resolved(name.trim_start_matches('-')).unwrap_or_default();
            let value = value.trim();
            if PLAIN.is_match(value) {
                return Some(value.to_owned());
            }
            if value.is_empty() { None } else { resolve_token_color(name, theme).map(to_rgb) }
        })
    };
    let variables: [(&'static str, &[&str]); 12] = [
        ("background", &["--bg-editor", "--bg-app"]),
        ("mainBkg", &["--bg-elev"]),
        ("primaryColor", &["--bg-elev"]),
        ("primaryBorderColor", &["--border-strong"]),
        ("primaryTextColor", &["--fg-strong", "--fg"]),
        ("secondaryColor", &["--bg-popup"]),
        ("tertiaryColor", &["--bg-hover"]),
        ("lineColor", &["--fg-secondary"]),
        ("textColor", &["--fg"]),
        ("edgeLabelBackground", &["--bg-editor", "--bg-app"]),
        ("errorBkgColor", &["--bg-elev"]),
        ("errorTextColor", &["--danger"]),
    ];
    let font = theme.resolved("font-ui").map(|font| font.trim().to_owned()).filter(|font| !font.is_empty());
    MermaidConfig {
        start_on_load: false,
        security_level: "strict",
        suppress_error_rendering: true,
        theme: "base",
        dark_mode: !theme.attributes().is_light(),
        theme_variables: variables.iter().filter_map(|(name, tokens)| Some((*name, token(tokens)?))).collect(),
        font_family: font,
    }
}

/// A drawing, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidResult {
    Svg(String),
    Error(String),
}

/// What a diagram block shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramView {
    /// Its source only, until the drawing is ready.
    Pending,
    /// The drawing before the last edit, until the new one is ready.
    Stale(String),
    Drawn(String),
    /// The syntax error, next to the source.
    Failed(String),
}

impl DiagramView {
    /// Whether the block shows a drawing (and supports pan and zoom).
    pub fn is_drawn(&self) -> bool {
        matches!(self, DiagramView::Drawn(_) | DiagramView::Stale(_))
    }
}

/// One drawing for the caller to make with Mermaid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidJob {
    /// The unique id Mermaid draws under (`openidea-mermaid-<n>`).
    pub id: String,
    pub source: String,
    /// Initialize Mermaid with [`mermaid_config`] of the current theme first: the theme changed
    /// since the last drawing.
    pub configure: bool,
}

/// A diagram block's new view after a drawing finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramUpdate {
    pub pass: u64,
    pub index: usize,
    pub view: DiagramView,
}

/// One rendering of a preview's diagrams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidPass {
    pub id: u64,
    /// Whether some diagram draws later (it changes the layout when done).
    pub is_async: bool,
    /// What each diagram block shows now, in document order.
    pub views: Vec<DiagramView>,
}

#[derive(Debug)]
struct Queued {
    pass: u64,
    index: usize,
    key: String,
    source: String,
}

/// The drawings of every preview: the cache, and the queue drawn one at a time (Mermaid's render
/// is not re-entrant across configuration changes).
#[derive(Debug, Default)]
pub struct Mermaid {
    cache: HashMap<String, MermaidResult>,
    order: VecDeque<String>,
    configured: String,
    seq: u64,
    applied: u64,
    passes: u64,
    live: HashSet<u64>,
    queue: VecDeque<Queued>,
    in_flight: Option<(String, Queued)>,
}

impl Mermaid {
    pub fn new() -> Self {
        Self::default()
    }

    fn remember(&mut self, key: String, result: MermaidResult) {
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= CACHE_LIMIT
                && let Some(oldest) = self.order.pop_front()
            {
                self.cache.remove(&oldest);
            }
            self.order.push_back(key.clone());
        }
        self.cache.insert(key, result);
    }

    /// The view of a finished drawing, its SVG with fresh ids.
    fn view_of(&mut self, result: &MermaidResult) -> DiagramView {
        match result {
            MermaidResult::Svg(svg) => {
                self.applied += 1;
                DiagramView::Drawn(reid(svg, &format!("c{}-", self.applied)))
            }
            MermaidResult::Error(error) => DiagramView::Failed(error.clone()),
        }
    }

    /// Renders a preview's diagram `sources` in the theme `theme_key`. `previous` holds the last
    /// drawing per block of the same preview (updated in place: the caller keeps it with the
    /// preview); it stands in while a changed diagram draws. Cached drawings apply at once.
    pub fn render(&mut self, sources: &[&str], theme_key: &str, previous: &mut Vec<Option<String>>) -> MermaidPass {
        self.passes += 1;
        let pass = self.passes;
        previous.resize(sources.len(), None);
        let mut views = Vec::with_capacity(sources.len());
        let mut is_async = false;
        for (index, source) in sources.iter().enumerate() {
            let source = source.trim_end_matches('\n');
            let key = format!("{theme_key}\n{source}");
            match self.cache.get(&key).cloned() {
                Some(result) => {
                    let view = self.view_of(&result);
                    if let DiagramView::Drawn(svg) = &view {
                        previous[index] = Some(svg.clone());
                    }
                    views.push(view);
                }
                None => {
                    is_async = true;
                    views.push(previous[index].clone().map_or(DiagramView::Pending, DiagramView::Stale));
                    self.queue.push_back(Queued { pass, index, key, source: source.to_owned() });
                }
            }
        }
        if is_async {
            self.live.insert(pass);
        }
        MermaidPass { id: pass, is_async, views }
    }

    /// The preview re-rendered or went away: its queued drawings are skipped.
    pub fn cancel(&mut self, pass: u64) {
        self.live.remove(&pass);
        self.queue.retain(|queued| queued.pass != pass);
    }

    /// The next drawing to make, once the one before has been answered.
    pub fn next_job(&mut self) -> Option<MermaidJob> {
        if self.in_flight.is_some() {
            return None;
        }
        let queued = self.queue.pop_front()?;
        self.seq += 1;
        let id = format!("openidea-mermaid-{}", self.seq);
        let theme = queued.key.split('\n').next().unwrap_or("").to_owned();
        let configure = self.configured != theme;
        self.configured = theme;
        let job = MermaidJob { id: id.clone(), source: queued.source.clone(), configure };
        self.in_flight = Some((id, queued));
        Some(job)
    }

    /// The answer of job `id`: the SVG, or Mermaid's error message. Returns the block's new view
    /// when its preview still shows it; the caller stores a drawn SVG as that block's `previous`.
    pub fn rendered(&mut self, id: &str, answer: Result<String, String>) -> Option<DiagramUpdate> {
        let (_, queued) = self.in_flight.take_if(|(pending, _)| *pending == id)?;
        let result = match answer {
            Ok(svg) => MermaidResult::Svg(svg),
            Err(message) => {
                let message = message.trim();
                MermaidResult::Error(if message.is_empty() { "Invalid diagram".to_owned() } else { message.to_owned() })
            }
        };
        self.remember(queued.key, result.clone());
        if !self.live.contains(&queued.pass) {
            return None;
        }
        if !self.queue.iter().any(|other| other.pass == queued.pass) {
            self.live.remove(&queued.pass);
        }
        let view = self.view_of(&result);
        Some(DiagramUpdate { pass: queued.pass, index: queued.index, view })
    }

    /// Whether every drawing of `pass` is in place (or the pass was cancelled).
    pub fn is_done(&self, pass: u64) -> bool {
        !self.live.contains(&pass)
    }
}

// ---------------------------------------------------------------------------
// Fresh ids

static TAG: LazyLock<Regex> = LazyLock::new(|| pattern(r#"<[a-zA-Z](?:[^>"']|"[^"]*"|'[^']*')*>"#));
static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(\s)([\w:-]+)(\s*=\s*)(?:"([^"]*)"|'([^']*)')"#));
static MARKUP: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?s)(<style\b[^>]*>)(.*?)(</style\s*>)|<[a-zA-Z](?:[^>"']|"[^"]*"|'[^']*')*>"#));
static URL_ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(?:style|fill|stroke|filter|clip-path|mask|marker(?:-start|-mid|-end)?|cursor)$"));
static WORD: LazyLock<Regex> = LazyLock::new(|| pattern(r"\S+"));

/// The ids of one SVG and the prefix they get.
struct Ids<'a> {
    known: HashSet<String>,
    /// Longest first, so `#a-10` is not read as `#a-1` followed by `0`.
    sorted: Vec<String>,
    prefix: &'a str,
}

impl Ids<'_> {
    fn rename(&self, id: &str) -> String {
        if self.known.contains(id) { format!("{}{id}", self.prefix) } else { id.to_owned() }
    }

    /// `#id` selectors in a selector list, where the id is followed by the end or a combinator.
    fn selectors(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while let Some(found) = text[i..].find('#') {
            let at = i + found;
            out.push_str(&text[i..=at]);
            let rest = &text[at + 1..];
            let id = self.sorted.iter().find(|id| {
                rest.strip_prefix(id.as_str()).is_some_and(|after| {
                    after.chars().next().is_none_or(|c| c.is_whitespace() || ".:>+~[,".contains(c))
                })
            });
            match id {
                Some(id) => {
                    out.push_str(self.prefix);
                    out.push_str(id);
                    i = at + 1 + id.len();
                }
                None => i = at + 1,
            }
        }
        out.push_str(&text[i..]);
        out
    }

    /// CSS with the selectors of its rules renamed; declarations (which may hold hex colours like
    /// `#dad`) are left alone.
    fn css(&self, css: &str) -> String {
        let mut out = String::with_capacity(css.len());
        let mut start = 0;
        for (i, c) in css.char_indices() {
            match c {
                '{' => {
                    out.push_str(&self.selectors(&css[start..i]));
                    out.push('{');
                    start = i + 1;
                }
                '}' => {
                    out.push_str(&css[start..=i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push_str(&css[start..]);
        out
    }

    /// `url(#id)` references, quoted or not.
    fn urls(&self, value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut rest = value;
        while let Some(at) = rest.find("url(") {
            out.push_str(&rest[..at + 4]);
            let after = &rest[at + 4..];
            let body = after.trim_start();
            let space = &after[..after.len() - body.len()];
            let quote = body.chars().next().filter(|c| *c == '"' || *c == '\'');
            let quoted = quote.map_or(body, |q| &body[q.len_utf8()..]);
            let reference = quoted.strip_prefix('#').and_then(|id_and_tail| {
                let len = id_and_tail.find(|c: char| c == '\'' || c == '"' || c == ')' || c.is_whitespace());
                let len = len.unwrap_or(id_and_tail.len());
                let tail = &id_and_tail[len..];
                let closed = match quote {
                    Some(q) => tail.strip_prefix(q).is_some_and(|tail| tail.trim_start().starts_with(')')),
                    None => tail.trim_start().starts_with(')'),
                };
                (len > 0 && closed).then_some((&id_and_tail[..len], tail))
            });
            match reference {
                Some((id, tail)) => {
                    out.push_str(space);
                    out.extend(quote);
                    out.push('#');
                    out.push_str(&self.rename(id));
                    rest = tail;
                }
                None => rest = after,
            }
        }
        out.push_str(rest);
        out
    }

    fn attribute(&self, found: &Captures<'_>) -> String {
        let group = |n: usize| found.get(n).map_or("", |m| m.as_str());
        let (name, quote) = (group(2), if found.get(4).is_some() { '"' } else { '\'' });
        let value = found.get(4).or_else(|| found.get(5)).map_or("", |m| m.as_str());
        let value = if name == "id" {
            self.rename(value)
        } else if name == "href" || name == "xlink:href" {
            match value.strip_prefix('#') {
                Some(id) => format!("#{}", self.rename(id)),
                None => value.to_owned(),
            }
        } else if name == "aria-labelledby" || name == "aria-describedby" {
            WORD.replace_all(value, |word: &Captures<'_>| self.rename(&word[0])).into_owned()
        } else if URL_ATTRIBUTE.is_match(name) {
            self.urls(value)
        } else {
            value.to_owned()
        };
        format!("{}{name}{}{quote}{value}{quote}", group(1), group(3))
    }
}

/// `svg` with every id given `prefix`, and the references to them (`href="#…"`, `url(#…)`,
/// `aria-labelledby`, `#id` selectors of its `<style>`s) renamed with them. Labels and attributes
/// that only mention an id are left alone.
pub fn reid(svg: &str, prefix: &str) -> String {
    let mut known = HashSet::new();
    for tag in TAG.find_iter(svg) {
        for attribute in ATTRIBUTE.captures_iter(tag.as_str()) {
            if attribute.get(2).is_some_and(|name| name.as_str() == "id")
                && let Some(value) = attribute.get(4).or_else(|| attribute.get(5))
            {
                known.insert(value.as_str().to_owned());
            }
        }
    }
    if known.is_empty() {
        return svg.to_owned();
    }
    let mut sorted: Vec<String> = known.iter().cloned().collect();
    sorted.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let ids = Ids { known, sorted, prefix };
    MARKUP
        .replace_all(svg, |found: &Captures<'_>| match found.get(2) {
            Some(css) => {
                let open = found.get(1).map_or("", |m| m.as_str());
                let close = found.get(3).map_or("", |m| m.as_str());
                format!("{open}{}{close}", ids.urls(&ids.css(css.as_str())))
            }
            None => ATTRIBUTE.replace_all(&found[0], |attribute: &Captures<'_>| ids.attribute(attribute)).into_owned(),
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::viewers::render::{Node, has_mermaid, mermaid_sources, render_markdown};
    use crate::theme::tokens::Tokens;

    const SEQ: &str = "```mermaid\nsequenceDiagram\n    A->B:test\n```\n";

    /// The drawing Mermaid makes of `text` under `id`, as the spec's stand-in for Mermaid draws it.
    fn drawing(id: &str, text: &str) -> Result<String, String> {
        if text.contains("bad") {
            return Err("Parse error on line 2:\nbad\n^".to_owned());
        }
        let first = text.split('\n').next().unwrap_or("");
        let template = concat!(
            r#"<svg id="ID"><style>#ID{fill:#dad;}#ID .cluster rect{fill:#2e2f30;}"#,
            r#"#ID .flowchart-link{fill:none;}#ID [id$="-arrow"] path{fill:#9fa2a8;}</style>"#,
            r#"<defs><marker id="ID-arrow" orient="auto"><path d="M0,0L8,4"/></marker></defs>"#,
            r#"<g class="cluster" id="dad"><rect/></g>"#,
            r#"<path class="flowchart-link" d="M0,4" marker-end="url(#ID-arrow)"/><text>FIRST</text></svg>"#,
        );
        Ok(template.replace("ID", id).replace("FIRST", first))
    }

    /// Draws every queued job as the stand-in does; returns the jobs drawn.
    fn draw_all(mermaid: &mut Mermaid) -> (Vec<MermaidJob>, Vec<DiagramUpdate>) {
        let (mut jobs, mut updates) = (Vec::new(), Vec::new());
        while let Some(job) = mermaid.next_job() {
            updates.extend(mermaid.rendered(&job.id, drawing(&job.id, &job.source)));
            jobs.push(job);
        }
        (jobs, updates)
    }

    fn sources(md: &str) -> Vec<String> {
        mermaid_sources(&render_markdown(md)).into_iter().map(str::to_owned).collect()
    }

    fn render(mermaid: &mut Mermaid, md: &str, key: &str, previous: &mut Vec<Option<String>>) -> MermaidPass {
        let owned = sources(md);
        let sources: Vec<&str> = owned.iter().map(String::as_str).collect();
        mermaid.render(&sources, key, previous)
    }

    /// The svg drawn for one source, answered with `svg`.
    fn rendered_svg(svg: &str) -> String {
        let mut mermaid = Mermaid::new();
        render(&mut mermaid, SEQ, "k", &mut Vec::new());
        let job = mermaid.next_job().expect("a job");
        match mermaid.rendered(&job.id, Ok(svg.to_owned())).map(|update| update.view) {
            Some(DiagramView::Drawn(svg)) => svg,
            other => panic!("not drawn: {other:?}"),
        }
    }

    fn ids_of(svg: &str) -> Vec<String> {
        TAG.find_iter(svg)
            .flat_map(|tag| {
                ATTRIBUTE
                    .captures_iter(tag.as_str())
                    .filter(|a| &a[2] == "id")
                    .map(|a| a.get(4).or_else(|| a.get(5)).map_or("", |m| m.as_str()).to_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn attribute_values(svg: &str, name: &str) -> Vec<String> {
        TAG.find_iter(svg)
            .flat_map(|tag| {
                ATTRIBUTE
                    .captures_iter(tag.as_str())
                    .filter(|a| &a[2] == name)
                    .map(|a| a.get(4).or_else(|| a.get(5)).map_or("", |m| m.as_str()).to_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn styles(svg: &str) -> Vec<String> {
        static STYLE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?s)<style>(.*?)</style>"));
        STYLE.captures_iter(svg).map(|found| found[1].to_owned()).collect()
    }

    #[test]
    fn emits_a_placeholder_holding_the_escaped_source_other_code_blocks_unchanged() {
        let md =
            format!("# T\n\n{SEQ}\n```js\nlet a = 1;\n```\n\n```Mermaid title\ngraph TD\n  a[\"<b>x</b>\"]-->b\n```\n");
        let blocks = render_markdown(&md);
        let diagrams = mermaid_sources(&blocks);
        assert_eq!(diagrams.len(), 2);
        assert_eq!(blocks[1].line, 2);
        assert_eq!(blocks[1].nodes, [Node::Mermaid { source: "sequenceDiagram\n    A->B:test".into() }]);
        assert!(diagrams[1].contains("a[\"<b>x</b>\"]"));
        assert!(blocks.iter().any(|block| {
            block.nodes.iter().any(|node| matches!(node, Node::Code { language: Some(l), .. } if l == "js"))
        }));
    }

    #[test]
    fn loads_mermaid_only_for_previews_with_a_diagram() {
        let mut mermaid = Mermaid::new();
        let plain = render_markdown("# Plain\n\n```js\n1\n```\n");
        assert!(!has_mermaid(&plain));
        let pass = mermaid.render(&mermaid_sources(&plain), "k", &mut Vec::new());
        assert!(!pass.is_async);
        assert!(mermaid.is_done(pass.id));
        assert_eq!(mermaid.next_job(), None);

        assert!(has_mermaid(&render_markdown(SEQ)));
        let pass = render(&mut mermaid, SEQ, "k", &mut Vec::new());
        assert_eq!(pass.views, [DiagramView::Pending]);
        let (jobs, updates) = draw_all(&mut mermaid);
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].configure);
        assert!(mermaid.is_done(pass.id));
        let DiagramView::Drawn(svg) = &updates[0].view else { panic!("not drawn: {updates:?}") };
        assert!(svg.starts_with("<svg"));
        let config = mermaid_config(&Tokens::parse(":root { --fg: #abcdef; }").theme(&ThemeAttributes::dark()));
        assert!(!config.start_on_load);
        assert_eq!((config.security_level, config.theme), ("strict", "base"));
    }

    #[test]
    fn shows_syntax_errors_inline_next_to_the_source_without_throwing() {
        let mut mermaid = Mermaid::new();
        let md = "```mermaid\nsequenceDiagram\nbad\n```\n";
        assert!(sources(md)[0].contains("bad"));
        render(&mut mermaid, md, "k", &mut Vec::new());
        let (_, updates) = draw_all(&mut mermaid);
        let DiagramView::Failed(error) = &updates[0].view else { panic!("no error: {updates:?}") };
        assert!(error.contains("Parse error on line 2"));
        assert!(!error.contains("<svg"));
    }

    #[test]
    fn renders_with_unique_ids_reuses_results_and_keeps_the_old_diagram_while_a_new_one_renders() {
        let mut mermaid = Mermaid::new();
        render(&mut mermaid, SEQ, "k", &mut Vec::new());
        let (first, _) = draw_all(&mut mermaid);

        // Same source again (an edit elsewhere): drawn at once from the cache.
        let mut previous = Vec::new();
        let again = render(&mut mermaid, SEQ, "k", &mut previous);
        assert!(!again.is_async);
        assert!(matches!(&again.views[0], DiagramView::Drawn(svg) if svg.contains("<svg")));
        assert_eq!(mermaid.next_job(), None);

        // Edited diagram: the previous drawing stands in until the new one is ready.
        let next = render(&mut mermaid, &SEQ.replace("test", "edited"), "k", &mut previous);
        assert!(next.is_async);
        assert!(matches!(&next.views[0], DiagramView::Stale(svg) if svg.contains("<text>sequenceDiagram</text>")));
        let (second, updates) = draw_all(&mut mermaid);
        assert_eq!(second.len(), 1);
        assert!(updates[0].view.is_drawn());
        assert_ne!(first[0].id, second[0].id);
    }

    #[test]
    fn applies_a_cached_diagram_with_fresh_svg_ids_so_copies_dont_share_markers() {
        let mut mermaid = Mermaid::new();
        render(&mut mermaid, SEQ, "k", &mut Vec::new());
        let (_, updates) = draw_all(&mut mermaid);
        let DiagramView::Drawn(a) = updates[0].view.clone() else { panic!("not drawn") };
        let second = render(&mut mermaid, SEQ, "k", &mut Vec::new());
        assert!(!second.is_async);
        assert_eq!(mermaid.next_job(), None);
        let DiagramView::Drawn(b) = second.views[0].clone() else { panic!("not drawn") };
        let (ids_a, ids_b) = (ids_of(&a), ids_of(&b));
        assert_eq!(ids_a.len(), 3);
        assert_eq!(ids_b.len(), 3);
        assert!(ids_a.iter().all(|id| !ids_b.contains(id)));
        for svg in [&a, &b] {
            let own = ids_of(svg);
            let root = &own[0];
            let reference = &attribute_values(svg, "marker-end")[0];
            let target = reference.trim_start_matches("url(#").trim_end_matches(')');
            assert!(own.iter().any(|id| id == target), "{reference}");
            let css = &styles(svg)[0];
            assert!(css.contains(&format!("#{root}{{fill:#dad;}}")), "{css}");
            assert!(css.contains(&format!("#{root} .cluster rect{{fill:#2e2f30;}}")), "{css}");
            assert!(css.contains(&format!("#{root} .flowchart-link{{fill:none;}}")), "{css}");
            assert!(own.iter().any(|id| id.ends_with("-arrow")));
        }
    }

    #[test]
    fn rewrites_style_selectors_followed_by() {
        for tail in ["", ".node", ":hover", ">g", "+g", "~g", "[role]", ", rect", " .cluster rect", "\nrect"] {
            let svg = rendered_svg(&format!(
                r#"<svg id="openidea-mermaid-1"><style>#openidea-mermaid-1{tail}{{fill:none;}}</style></svg>"#
            ));
            let id = &ids_of(&svg)[0];
            assert_eq!(styles(&svg), [format!("#{id}{tail}{{fill:none;}}")], "{tail:?}");
        }
    }

    #[test]
    fn rewrites_fragment_links_quoted_urls_and_marker_ids_while_preserving_suffix_selectors() {
        let id = "openidea-mermaid-1";
        let marker = format!("{id}_flowchart-pointEnd");
        let template = concat!(
            r#"<svg id="ID" aria-labelledby="title desc" aria-describedby="desc">"#,
            r#"<style>#ID [id$="-pointEnd"]{fill:#dad;}#MARKER{stroke:#dad;}</style>"#,
            r#"<title id="title">Title</title><desc id="desc">Description</desc>"#,
            r#"<marker id="MARKER"/><g id="dad"/>"#,
            r##"<use href="#MARKER"/><use xlink:href="#MARKER"/>"##,
            r##"<path marker-end="url(#MARKER)"/>"##,
            r##"<path marker-end="url('#MARKER')"/>"##,
            r##"<path marker-end='url("#MARKER")'/>"##,
            r##"<path style="marker-end: url( '#MARKER' );"/>"##,
            r##"<style>#ID path{marker-end:url("#MARKER");}</style></svg>"##,
        );
        let svg = rendered_svg(&template.replace("MARKER", &marker).replace("ID", id));
        let ids = ids_of(&svg);
        let root = &ids[0];
        let marker_id = ids.iter().find(|i| i.ends_with("_flowchart-pointEnd")).expect("the marker");
        assert_eq!(*marker_id, format!("{root}_flowchart-pointEnd"));
        let mut uses = attribute_values(&svg, "href");
        uses.extend(attribute_values(&svg, "xlink:href"));
        assert_eq!(uses, [format!("#{marker_id}"), format!("#{marker_id}")]);
        assert_eq!(
            attribute_values(&svg, "marker-end"),
            [format!("url(#{marker_id})"), format!("url('#{marker_id}')"), format!("url(\"#{marker_id}\")")]
        );
        assert_eq!(attribute_values(&svg, "style"), [format!("marker-end: url( '#{marker_id}' );")]);
        assert_eq!(
            styles(&svg),
            [
                format!("#{root} [id$=\"-pointEnd\"]{{fill:#dad;}}#{marker_id}{{stroke:#dad;}}"),
                format!("#{root} path{{marker-end:url(\"#{marker_id}\");}}"),
            ]
        );
        let title = ids.iter().find(|i| i.ends_with("title")).expect("the title");
        let desc = ids.iter().find(|i| i.ends_with("desc")).expect("the description");
        assert_eq!(attribute_values(&svg, "aria-labelledby"), [format!("{title} {desc}")]);
        assert_eq!(attribute_values(&svg, "aria-describedby"), std::slice::from_ref(desc));
    }

    #[test]
    fn keeps_ids_with_a_shared_prefix_distinct_and_leaves_unknown_references_unchanged() {
        let short = "openidea-mermaid-1";
        let long = "openidea-mermaid-10";
        let template = concat!(
            r#"<svg id="SHORT"><g id="LONG"/>"#,
            r#"<style>#SHORT>g,#LONG:hover{fill:none;}#LONG0{fill:none;}</style>"#,
            r##"<use href="#SHORT"/><use href="#LONG"/><use href="#LONG0"/>"##,
            r##"<path marker-end="url('#LONG')"/></svg>"##,
        );
        let svg = rendered_svg(&template.replace("SHORT", short).replace("LONG", long));
        let ids = ids_of(&svg);
        let (root, long_id) = (&ids[0], &ids[1]);
        assert_eq!(*long_id, format!("{root}0"));
        assert_eq!(attribute_values(&svg, "href"), [format!("#{root}"), format!("#{long_id}"), format!("#{long}0")]);
        assert_eq!(styles(&svg), [format!("#{root}>g,#{long_id}:hover{{fill:none;}}#{long}0{{fill:none;}}")]);
        assert_eq!(attribute_values(&svg, "marker-end"), [format!("url('#{long_id}')")]);
    }

    #[test]
    fn leaves_ids_in_ordinary_labels_and_unrelated_attributes_untouched() {
        let id = "openidea-mermaid-1";
        let label = format!(r##"#{id} {id} id="{id}" href="#{id}" url(#{id})"##);
        let template = concat!(
            r#"<svg id="ID"><text>LABEL</text>"#,
            r##"<g data-id="ID" data-label="#ID" title="url(#ID)"/>"##,
            r##"<a href="https://example.com/#ID">Link</a></svg>"##,
        );
        let svg = rendered_svg(&template.replace("LABEL", &label).replace("ID", id));
        assert!(svg.contains(&format!("<text>{label}</text>")));
        assert_eq!(attribute_values(&svg, "data-id"), [id]);
        assert_eq!(attribute_values(&svg, "data-label"), [format!("#{id}")]);
        assert_eq!(attribute_values(&svg, "title"), [format!("url(#{id})")]);
        assert_eq!(attribute_values(&svg, "href"), [format!("https://example.com/#{id}")]);
    }

    #[test]
    fn re_renders_when_switching_between_built_in_palette_themes_of_the_same_light_dark_base() {
        let one = theme_key(&ThemeAttributes::dark().with("data-color-theme", "one-dark"), "", "Inter");
        let solarized = theme_key(&ThemeAttributes::dark().with("data-color-theme", "solarized-dark"), "", "Inter");
        assert_ne!(one, solarized);
        let mut mermaid = Mermaid::new();
        render(&mut mermaid, SEQ, &one, &mut Vec::new());
        let (first, _) = draw_all(&mut mermaid);
        render(&mut mermaid, SEQ, &solarized, &mut Vec::new());
        let (second, _) = draw_all(&mut mermaid);
        assert_eq!(first.len() + second.len(), 2);
        assert!(second[0].configure);
    }

    #[test]
    fn re_renders_when_switching_between_plugin_themes_of_the_same_light_dark_base() {
        let attributes = ThemeAttributes::dark().with("data-theme", "dark");
        let night = theme_key(&attributes.clone().with("data-plugin-theme", "acme-themes/night"), "", "");
        let dusk = theme_key(&attributes.with("data-plugin-theme", "acme-themes/dusk"), "", "");
        assert_ne!(night, dusk);
        let mut mermaid = Mermaid::new();
        render(&mut mermaid, SEQ, &night, &mut Vec::new());
        draw_all(&mut mermaid);
        let pass = render(&mut mermaid, SEQ, &dusk, &mut Vec::new());
        assert!(pass.is_async);
        assert_eq!(draw_all(&mut mermaid).0.len(), 1);
    }

    #[test]
    fn passes_colours_a_theme_writes_as_hsl_or_color_mix_to_mermaid_as_rgb() {
        let tokens = Tokens::parse(concat!(
            ":root { --bg-editor: hsl(220, 13%, 18%); ",
            "--fg: color-mix(in srgb, hsl(220, 14%, 71%) 90%, black); --danger: #e06c75; }",
        ));
        let config = mermaid_config(&tokens.theme(&ThemeAttributes::dark()));
        let variable =
            |name: &str| config.theme_variables.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
        assert_eq!(variable("background").as_deref(), Some("rgb(40, 44, 52)"));
        assert!(variable("textColor").is_some_and(|value| value.starts_with("rgb(")), "{config:?}");
        assert_eq!(variable("errorTextColor").as_deref(), Some("#e06c75"));
        assert!(config.dark_mode);
    }

    #[test]
    fn skips_superseded_renders() {
        let mut mermaid = Mermaid::new();
        let pass = render(&mut mermaid, &SEQ.replace("test", "stale"), "k", &mut Vec::new());
        mermaid.cancel(pass.id);
        assert!(mermaid.is_done(pass.id));
        assert_eq!(mermaid.next_job(), None);
        assert_eq!(pass.views, [DiagramView::Pending]);
    }
}
