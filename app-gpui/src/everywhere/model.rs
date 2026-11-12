//! Search Everywhere result model: item types, per-section merging and ranking, and the flat row list
//! (section headers and "more" rows) the dialog renders.
//!
//! Highlight indices are byte offsets into the text they belong to, one per matched character, as
//! [`crate::ui::matching::index_runs`] takes them. Lines and columns are 0-based, columns in UTF-16 code
//! units.

use super::open::{rel_dir, symbol_display_name, utf16_len};
use super::rank::{MatchOptions, WORD_ALIGNED, by_score, match_text};
pub use crate::find::ipc::LineMatch;
use crate::platform::actions::ActionMeta;
use crate::platform::i18n::I18n;
use crate::platform::virtual_docs::basename;
use ide_core::ipc_path::to_ipc;
use ide_core::{Location, Range, SymbolKind};
use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};

/// A tab of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeTab {
    All,
    Classes,
    Files,
    Symbols,
    Actions,
    Text,
}

/// The tabs in the order the dialog shows them.
pub const TABS: [SeTab; 6] = [SeTab::All, SeTab::Classes, SeTab::Files, SeTab::Symbols, SeTab::Actions, SeTab::Text];

impl SeTab {
    /// The id the tab is stored and addressed by (`/files`).
    pub const fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Classes => "classes",
            Self::Files => "files",
            Self::Symbols => "symbols",
            Self::Actions => "actions",
            Self::Text => "text",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        TABS.into_iter().find(|tab| tab.id() == id)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Classes => "Classes",
            Self::Files => "Files",
            Self::Symbols => "Symbols",
            Self::Actions => "Actions",
            Self::Text => "Text",
        }
    }

    /// The section a tab other than All searches.
    const fn section(self) -> Option<SectionId> {
        match self {
            Self::All => None,
            Self::Classes => Some(SectionId::Classes),
            Self::Files => Some(SectionId::Files),
            Self::Symbols => Some(SectionId::Symbols),
            Self::Actions => Some(SectionId::Actions),
            Self::Text => Some(SectionId::Text),
        }
    }
}

/// A group of results from one source. The order of the variants is the order sections are listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SectionId {
    Recent,
    RecentActions,
    Classes,
    Files,
    Symbols,
    Endpoints,
    Actions,
    Text,
}

pub const SECTION_ORDER: [SectionId; 8] = [
    SectionId::Recent,
    SectionId::RecentActions,
    SectionId::Classes,
    SectionId::Files,
    SectionId::Symbols,
    SectionId::Endpoints,
    SectionId::Actions,
    SectionId::Text,
];

impl SectionId {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::RecentActions => "recentActions",
            Self::Classes => "classes",
            Self::Files => "files",
            Self::Symbols => "symbols",
            Self::Endpoints => "endpoints",
            Self::Actions => "actions",
            Self::Text => "text",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Recent => "Recent Files",
            Self::RecentActions => "Recent Actions",
            Self::Classes => "Classes",
            Self::Files => "Files",
            Self::Symbols => "Symbols",
            Self::Endpoints => "Endpoints",
            Self::Actions => "Actions",
            Self::Text => "Text",
        }
    }
}

/// Sections a tab queries, for a query with text and for an empty one.
pub fn sections_for(tab: SeTab, empty_query: bool) -> Vec<SectionId> {
    if empty_query {
        return match tab {
            SeTab::All => vec![SectionId::Recent, SectionId::RecentActions],
            SeTab::Files => vec![SectionId::Recent],
            SeTab::Actions => vec![SectionId::RecentActions],
            SeTab::Classes | SeTab::Symbols | SeTab::Text => Vec::new(),
        };
    }
    tab.section().map_or_else(|| MIXED.to_vec(), |section| vec![section])
}

pub const ALL_TAB_LIMIT: usize = 6;
pub const TAB_LIMIT: usize = 50;
pub const MORE_STEP_ALL: usize = 15;
pub const MORE_STEP_TAB: usize = 50;
pub const RECENT_LIMIT: usize = 15;

/// A symbol found by the index. Mirrors `ide_index::SymbolMatch`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolMatch {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub path: String,
    pub rel_path: String,
    pub range: Range,
    pub score: u32,
    /// Matched char indices within `name`.
    pub indices: Vec<u32>,
}

/// A symbol a language server reports for the workspace. Mirrors `ide_lsp::types::WorkspaceSymbol`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItem {
    pub key: String,
    pub path: String,
    pub rel: String,
    pub name: String,
    pub dir: String,
    /// Matched characters of `name`.
    pub name_indices: Vec<usize>,
    /// Where to open the file, when the query named a line.
    pub line: Option<u32>,
    pub col: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolItem {
    pub key: String,
    pub name: String,
    pub display: String,
    pub symbol_kind: SymbolKind,
    pub container: Option<String>,
    pub path: String,
    pub rel: String,
    pub line: u32,
    pub col: u32,
    /// Matched characters of `name`.
    pub indices: Vec<usize>,
    /// From the language server rather than the index.
    pub lsp: bool,
    /// Relevance computed with the item (endpoints: how well the request path matched).
    pub score: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionItem {
    pub key: String,
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub enabled: bool,
    /// Matched characters of `title`.
    pub indices: Vec<usize>,
    pub score: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextItem {
    pub key: String,
    pub path: String,
    pub rel: String,
    pub line: u32,
    pub col: u32,
    pub end_col: u32,
    /// The line with its indentation collapsed to at most two spaces.
    pub text: String,
    /// Matched characters of `text`.
    pub indices: Vec<usize>,
}

/// A settings page or option, listed among actions ("Settings: Editor | General").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingItem {
    pub key: String,
    pub page_id: String,
    pub title: String,
    pub path: Vec<String>,
    /// Matched characters of `title`.
    pub indices: Vec<usize>,
    pub score: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeItem {
    File(FileItem),
    Symbol(SymbolItem),
    Action(ActionItem),
    Text(TextItem),
    Setting(SettingItem),
}

impl SeItem {
    pub fn key(&self) -> &str {
        match self {
            Self::File(item) => &item.key,
            Self::Symbol(item) => &item.key,
            Self::Action(item) => &item.key,
            Self::Text(item) => &item.key,
            Self::Setting(item) => &item.key,
        }
    }
}

/// A row of the dialog's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeRow {
    /// A section header.
    Section {
        key: String,
        section: SectionId,
        title: &'static str,
    },
    /// A `/` command that switches to a tab.
    Command {
        key: String,
        name: &'static str,
        tab: SeTab,
        label: &'static str,
    },
    /// "Show more" of a section.
    More {
        key: String,
        section: SectionId,
    },
    Item {
        item: SeItem,
        section: SectionId,
    },
}

impl SeRow {
    pub fn key(&self) -> &str {
        match self {
            Self::Section { key, .. } | Self::Command { key, .. } | Self::More { key, .. } => key,
            Self::Item { item, .. } => item.key(),
        }
    }

    pub fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }

    /// The section a result or a "more" row belongs to.
    pub fn group(&self) -> Option<SectionId> {
        match self {
            Self::More { section, .. } | Self::Item { section, .. } => Some(*section),
            Self::Section { .. } | Self::Command { .. } => None,
        }
    }
}

/// The results of one section and whether more are there or coming.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionState {
    pub items: Vec<SeItem>,
    pub has_more: bool,
    pub loading: bool,
}

pub type Sections = BTreeMap<SectionId, SectionState>;

/// Byte offsets in `text` of the characters at the char indices `indices`; indices past the end are
/// dropped.
fn char_offsets(text: &str, indices: impl IntoIterator<Item = usize>) -> Vec<usize> {
    let offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    indices.into_iter().filter_map(|index| offsets.get(index).copied()).collect()
}

/// A file result. `indices` are the matched char indices within `rel_path`, as the index reports them;
/// those in the name are kept.
pub fn file_item(path: &str, rel_path: &str, indices: &[u32], line: Option<u32>, col: Option<u32>) -> FileItem {
    let name = basename(rel_path).into_owned();
    let name_start = rel_path.chars().count().saturating_sub(name.chars().count());
    let in_name = indices.iter().filter_map(|&index| (index as usize).checked_sub(name_start));
    let name_indices = char_offsets(&name, in_name);
    FileItem {
        key: format!("f:{path}"),
        path: path.to_owned(),
        rel: rel_path.to_owned(),
        name,
        dir: rel_dir(rel_path).to_owned(),
        name_indices,
        line,
        col,
    }
}

fn symbol_key(path: &str, start: ide_core::Pos, name: &str) -> String {
    format!("s:{path}:{}:{}:{name}", start.line, start.col)
}

pub fn symbol_item(found: &SymbolMatch) -> SymbolItem {
    SymbolItem {
        key: symbol_key(&found.path, found.range.start, &found.name),
        name: found.name.clone(),
        display: symbol_display_name(&found.name, found.kind),
        symbol_kind: found.kind,
        container: found.container.clone(),
        path: found.path.clone(),
        rel: found.rel_path.clone(),
        line: found.range.start.line,
        col: found.range.start.col,
        indices: char_offsets(&found.name, found.indices.iter().map(|&index| index as usize)),
        lsp: false,
        score: None,
    }
}

fn is_exact(name: &str, query: &str) -> bool {
    name.to_lowercase() == query.trim().to_lowercase()
}

/// How [`merge_symbols`] narrows and cuts its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeOptions<'a> {
    pub root: Option<&'a str>,
    /// Keep language-server symbols outside the project.
    pub include_non_project: bool,
    /// Keep only types (the Classes section).
    pub types_only: bool,
    /// Drop types (the Symbols section of the All tab, which lists them under Classes).
    pub exclude_types: bool,
    pub limit: usize,
}

/// The symbols of a section and whether more matched than its limit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolHits {
    pub items: Vec<SymbolItem>,
    pub has_more: bool,
}

/// Index results keep the backend's order. Language-server symbols the index does not know are scored
/// here and appended, or put ahead when they clearly match better (an exact name).
pub fn merge_symbols(
    index: &[SymbolMatch],
    lsp: &[WorkspaceSymbol],
    query: &str,
    opts: &MergeOptions<'_>,
) -> SymbolHits {
    let keep = |kind: SymbolKind| {
        if opts.types_only { kind.is_type_like() } else { !(opts.exclude_types && kind.is_type_like()) }
    };
    let root = opts.root.filter(|root| !root.is_empty());
    let mut seen: HashSet<(String, u32, String)> = HashSet::new();
    let mut scored: Vec<(SymbolItem, i32)> = Vec::new();
    for (position, found) in index.iter().enumerate() {
        if !keep(found.kind) || !seen.insert((found.path.clone(), found.range.start.line, found.name.clone())) {
            continue;
        }
        // The backend's order is authoritative within each tier (exact name, other).
        let tier = if is_exact(&found.name, query) { 4000 } else { 3000 };
        scored.push((symbol_item(found), tier - i32::try_from(position).unwrap_or(i32::MAX)));
    }
    for symbol in lsp {
        if !keep(symbol.kind) {
            continue;
        }
        let path = to_ipc(&symbol.location.path);
        let below_root = root.and_then(|root| path.strip_prefix(root)).filter(|rest| rest.starts_with('/'));
        let in_project = below_root.is_some() || root == Some(path.as_str());
        if !in_project && !opts.include_non_project {
            continue;
        }
        let start = symbol.location.range.start;
        let dedupe = (path.clone(), start.line, symbol.name.clone());
        if seen.contains(&dedupe) {
            continue;
        }
        let Some(hit) = match_text(&symbol.name, query, MatchOptions::default()) else { continue };
        seen.insert(dedupe);
        let rel = match below_root {
            Some(rest) => rest[1..].to_owned(),
            None if in_project => String::new(),
            None => path.clone(),
        };
        // Exact language-server hits follow exact index hits and beat fuzzy ones.
        let score = if is_exact(&symbol.name, query) { 3500 } else { hit.score };
        let item = SymbolItem {
            key: symbol_key(&path, start, &symbol.name),
            name: symbol.name.clone(),
            display: symbol_display_name(&symbol.name, symbol.kind),
            symbol_kind: symbol.kind,
            container: symbol.container.clone(),
            path,
            rel,
            line: start.line,
            col: start.col,
            indices: hit.indices,
            lsp: true,
            score: None,
        };
        scored.push((item, score));
    }
    let mut items: Vec<SymbolItem> = by_score(scored, |entry| entry.1).into_iter().map(|entry| entry.0).collect();
    let has_more = items.len() > opts.limit;
    items.truncate(opts.limit);
    SymbolHits { items, has_more }
}

/// What [`match_actions`] needs to know about the actions it is given.
#[derive(Clone, Copy)]
pub struct ActionMatchOptions<'a> {
    /// Whether an action can run where the dialog was opened.
    pub is_enabled: &'a dyn Fn(&ActionMeta) -> bool,
    /// Ids of recently used actions, most recent first.
    pub recent: &'a [String],
    /// List actions that cannot run too (greyed, ranked lower).
    pub include_disabled: bool,
}

/// Actions matching `query` by title, best first. An action whose title does not match is still found,
/// ranked lower and without highlights, by its category, description, keywords (the English title of a
/// translated action among them) or id. Hidden actions are never listed.
pub fn match_actions<'a>(
    actions: impl IntoIterator<Item = &'a ActionMeta>,
    query: &str,
    opts: &ActionMatchOptions<'_>,
    i18n: &I18n,
) -> Vec<ActionItem> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let lowered = query.to_lowercase();
    let mut out = Vec::new();
    for action in actions {
        if action.hidden {
            continue;
        }
        let enabled = (opts.is_enabled)(action);
        if !enabled && !opts.include_disabled {
            continue;
        }
        let shown = action.localized(i18n);
        let (mut score, indices) = match match_text(&shown.title, query, WORD_ALIGNED) {
            Some(hit) => (hit.score, hit.indices),
            None => {
                let extra = shown.category.iter().chain(&shown.description).chain(&shown.keywords);
                let best = extra.filter_map(|text| match_text(text, query, WORD_ALIGNED)).map(|hit| hit.score).max();
                let id_hit = action.id.to_lowercase().contains(&lowered).then_some(0);
                let Some(best) = best.max(id_hit) else { continue };
                (best - 400, Vec::new())
            }
        };
        if !enabled {
            score -= 150;
        }
        if let Some(rank) = opts.recent.iter().rposition(|id| *id == action.id) {
            score += 40 - i32::try_from(rank.saturating_mul(3)).map_or(30, |boost| boost.min(30));
        }
        out.push(ActionItem {
            key: format!("a:{}", action.id),
            id: action.id.clone(),
            title: shown.title,
            category: shown.category,
            icon: action.icon.clone(),
            enabled,
            indices,
            score,
        });
    }
    by_score(out, |item| item.score)
}

/// A searchable settings entry: a page, or an option label on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsSearchEntry {
    pub page_id: String,
    /// Page title or option label, in English.
    pub title: String,
    /// Breadcrumb of the page ("Editor", "General").
    pub path: Vec<String>,
    /// Extra words that match this entry (page keywords).
    pub keywords: Vec<String>,
}

/// Settings rank below equally good actions, as IDEA's option matches.
const SETTING_PENALTY: i32 = 120;

/// Settings entries matching `query` by title (translated or English), else by breadcrumb or keywords.
pub fn match_settings(entries: &[SettingsSearchEntry], query: &str, i18n: &I18n) -> Vec<SettingItem> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let title = i18n.tx(&entry.title);
        let (score, indices) = if let Some(hit) = match_text(&title, query, WORD_ALIGNED) {
            (hit.score, hit.indices)
        } else if title != entry.title
            && let Some(english) = match_text(&entry.title, query, WORD_ALIGNED)
        {
            // The English title still matches; its indices do not apply to the translated text.
            (english.score, Vec::new())
        } else {
            let extra = entry.path.iter().map(|part| i18n.tx(part)).chain(entry.keywords.iter().cloned());
            let best = extra.filter_map(|text| match_text(&text, query, WORD_ALIGNED)).map(|hit| hit.score).max();
            let Some(best) = best else { continue };
            (best - 400, Vec::new())
        };
        let key = format!("set:{}:{}", entry.page_id, entry.title);
        if !seen.insert(key.clone()) {
            continue;
        }
        out.push(SettingItem {
            key,
            page_id: entry.page_id.clone(),
            title,
            path: entry.path.clone(),
            indices,
            score: score - SETTING_PENALTY,
        });
    }
    by_score(out, |item| item.score)
}

/// Actions and settings in one relevance order; ties keep actions first.
pub fn merge_action_results(actions: Vec<ActionItem>, settings: Vec<SettingItem>) -> Vec<SeItem> {
    let settings = settings.into_iter().map(SeItem::Setting);
    let items: Vec<SeItem> = actions.into_iter().map(SeItem::Action).chain(settings).collect();
    by_score(items, |item| match item {
        SeItem::Action(action) => action.score,
        SeItem::Setting(setting) => setting.score,
        SeItem::File(_) | SeItem::Symbol(_) | SeItem::Text(_) => 0,
    })
}

/// An action as a result that no query matched (the recent actions).
pub fn action_item(action: &ActionMeta, enabled: bool, i18n: &I18n) -> ActionItem {
    let shown = action.localized(i18n);
    ActionItem {
        key: format!("a:{}", action.id),
        id: action.id.clone(),
        title: shown.title,
        category: shown.category,
        icon: action.icon.clone(),
        enabled,
        indices: Vec::new(),
        score: 0,
    }
}

/// The text results of one file. A hint of indentation shows nesting without wasting the row, like IDEA.
pub fn text_items(path: &str, rel: &str, matches: &[LineMatch]) -> Vec<TextItem> {
    let mut out = Vec::with_capacity(matches.len());
    for found in matches {
        let body = found.text.trim_start();
        let ws = utf16_len(&found.text[..found.text.len() - body.len()]);
        let indent = ws.min(2);
        // UTF-16 units the collapsed indentation removed in front of the body.
        let lead = ws - indent;
        let text = format!("{}{}", " ".repeat(indent), body.trim_end());
        // Per UTF-16 unit of `text`, the byte offset of the character it belongs to.
        let unit_offsets: Vec<usize> =
            text.char_indices().flat_map(|(offset, c)| std::iter::repeat_n(offset, c.len_utf16())).collect();
        let mut indices = Vec::new();
        for &(start, end) in &found.ranges {
            for unit in (start as usize).max(ws)..end as usize {
                if let Some(&offset) = unit_offsets.get(unit - lead) {
                    indices.push(offset);
                }
            }
        }
        indices.dedup();
        let (start, end) = found.ranges.first().copied().unwrap_or((0, 0));
        out.push(TextItem {
            key: format!("t:{path}:{}:{start}", found.line),
            path: path.to_owned(),
            rel: rel.to_owned(),
            line: found.line,
            col: start + found.text_offset,
            end_col: end + found.text_offset,
            text,
            indices,
        });
    }
    out
}

/// Sections the All tab mixes into one relevance-ordered list.
const MIXED: [SectionId; 6] = [
    SectionId::Classes,
    SectionId::Files,
    SectionId::Symbols,
    SectionId::Endpoints,
    SectionId::Actions,
    SectionId::Text,
];

/// Weight of a text hit: a literal substring in a line ranks below name matches that start a word and
/// above scattered name subsequences.
pub const TEXT_WEIGHT: i32 = 350;

/// Comparable match degree of an item's name against the query (IDEA's `MatchingDegree`).
pub fn item_weight(item: &SeItem, query: &str) -> i32 {
    let name = |text: &str| match_text(text, query, MatchOptions::default()).map(|hit| hit.score);
    match item {
        SeItem::File(file) => name(&file.name).unwrap_or_else(|| name(&file.rel).unwrap_or(0) - 200),
        SeItem::Symbol(symbol) => {
            let qualified = || {
                let container = symbol.container.as_deref().filter(|container| !container.is_empty())?;
                name(&format!("{container}.{}", symbol.name))
            };
            symbol.score.or_else(|| name(&symbol.name)).or_else(qualified).unwrap_or(0)
        }
        SeItem::Action(action) => action.score,
        SeItem::Setting(setting) => setting.score,
        SeItem::Text(_) => TEXT_WEIGHT,
    }
}

/// A result of the All tab with the weight it is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mixed<'a> {
    pub item: &'a SeItem,
    pub section: SectionId,
    pub weight: i32,
}

/// The All tab's results interleaved by weight. Within a section the source's order wins (the index
/// already ranks by recency and more), so weights are clamped to never increase; ties keep the section
/// order.
pub fn mix_items<'a>(sections: &'a Sections, query: &str) -> Vec<Mixed<'a>> {
    let mut out = Vec::new();
    for section in MIXED {
        let mut cap = i32::MAX;
        for item in sections.get(&section).into_iter().flat_map(|state| &state.items) {
            cap = cap.min(item_weight(item, query));
            out.push(Mixed { item, section, weight: cap });
        }
    }
    // Stable: equal weights stay in section order, then in the source's order.
    out.sort_by_key(|mixed| Reverse(mixed.weight));
    out
}

fn more_row(section: SectionId) -> SeRow {
    SeRow::More { key: format!("+{}", section.id()), section }
}

/// The text the All tab mixes its results by, when there is any.
fn mix_query(mix: Option<&str>) -> Option<&str> {
    mix.map(str::trim).filter(|query| !query.is_empty())
}

/// Flat rows the dialog renders. With `mix` (the All tab's query) results are interleaved by relevance
/// and the "more" rows follow them; otherwise sections keep their fixed order, under a header when
/// `headers` is set.
pub fn build_rows(sections: &Sections, headers: bool, mix: Option<&str>) -> Vec<SeRow> {
    let mut rows = Vec::new();
    if let Some(query) = mix_query(mix) {
        let mixed = mix_items(sections, query);
        rows.extend(mixed.into_iter().map(|mixed| SeRow::Item { item: mixed.item.clone(), section: mixed.section }));
        let more = MIXED.into_iter().filter(|section| sections.get(section).is_some_and(|state| state.has_more));
        rows.extend(more.map(more_row));
        return rows;
    }
    for section in SECTION_ORDER {
        let Some(state) = sections.get(&section).filter(|state| !state.items.is_empty()) else { continue };
        if headers {
            rows.push(SeRow::Section { key: format!("#{}", section.id()), section, title: section.title() });
        }
        rows.extend(state.items.iter().map(|item| SeRow::Item { item: item.clone(), section }));
        if state.has_more {
            rows.push(more_row(section));
        }
    }
    rows
}

/// Where Next / Previous Group moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupDirection {
    Next,
    Previous,
}

/// Next / Previous Group (PageDown / PageUp, ⌘↓ / ⌘↑): the first item of the next section, or of the
/// current (then the previous) one; `None` with fewer than two sections.
pub fn group_jump(rows: &[SeRow], active: usize, direction: GroupDirection) -> Option<usize> {
    // Groups start where the section changes (the All tab may show no header rows).
    let mut starts = Vec::new();
    let mut last = None;
    for (index, row) in rows.iter().enumerate() {
        let Some(section) = row.group() else { continue };
        if last != Some(section) {
            starts.push(index);
        }
        last = Some(section);
    }
    if starts.len() < 2 {
        return None;
    }
    let (&first, &final_start) = (starts.first()?, starts.last()?);
    let before = |limit: usize| starts.iter().rev().copied().find(|&start| start < limit).unwrap_or(first);
    Some(match direction {
        GroupDirection::Next => starts.iter().copied().find(|&start| start > active).unwrap_or(final_start),
        GroupDirection::Previous => {
            let current = before(active.saturating_add(1));
            if current < active { current } else { before(active) }
        }
    })
}

/// A `/` command: the tab it switches to under the name typed for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeCommand {
    pub name: &'static str,
    pub tab: SeTab,
    pub label: &'static str,
}

/// One command per tab but All, in tab order.
pub const COMMANDS: [ScopeCommand; 5] = [
    ScopeCommand { name: "/classes", tab: SeTab::Classes, label: SeTab::Classes.label() },
    ScopeCommand { name: "/files", tab: SeTab::Files, label: SeTab::Files.label() },
    ScopeCommand { name: "/symbols", tab: SeTab::Symbols, label: SeTab::Symbols.label() },
    ScopeCommand { name: "/actions", tab: SeTab::Actions, label: SeTab::Actions.label() },
    ScopeCommand { name: "/text", tab: SeTab::Text, label: SeTab::Text.label() },
];

/// What a query starting with `/` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    /// The commands matching what is typed so far, as rows to choose from.
    List(Vec<SeRow>),
    /// Switch to `tab` and search `rest` there.
    Run { tab: SeTab, rest: String },
}

/// `/` or `/fi` lists the matching scope commands; `/files foo` switches to Files and searches `foo`.
/// Anything else (an absolute path, say) is a query.
pub fn parse_command(query: &str) -> Option<ParsedCommand> {
    let typed = query.strip_prefix('/')?;
    let (word, rest) = match typed.find(char::is_whitespace) {
        Some(end) => (&typed[..end], Some(typed[end..].trim_start())),
        None => (typed, None),
    };
    let name = format!("/{}", word.to_lowercase());
    if let Some(rest) = rest {
        let command = COMMANDS.iter().find(|command| command.name == name)?;
        return Some(ParsedCommand::Run { tab: command.tab, rest: rest.to_owned() });
    }
    let rows: Vec<SeRow> = COMMANDS
        .iter()
        .filter(|command| command.name.starts_with(&name))
        .map(|command| SeRow::Command {
            key: format!("cmd:{}", command.tab.id()),
            name: command.name,
            tab: command.tab,
            label: command.label,
        })
        .collect();
    (!rows.is_empty()).then_some(ParsedCommand::List(rows))
}

/// Whether the top result is final: every section above the first one with items has finished. Enter
/// typed before that waits (IDEA opens the first result of the new query, never a stale row of the
/// previous one). With `mix`, any section can still outrank the top row; only a slow text search is not
/// waited for once the top row beats every text hit.
pub fn top_result_settled(sections: &Sections, mix: Option<&str>) -> bool {
    if let Some(query) = mix_query(mix) {
        let beats_text = mix_items(sections, query).first().is_some_and(|top| top.weight > TEXT_WEIGHT);
        return MIXED.into_iter().all(|section| {
            sections.get(&section).is_none_or(|state| !state.loading) || (section == SectionId::Text && beats_text)
        });
    }
    for section in SECTION_ORDER {
        let Some(state) = sections.get(&section) else { continue };
        if state.loading {
            return false;
        }
        if !state.items.is_empty() {
            return true;
        }
    }
    true
}

/// Keeps the selection on the same item while results stream in: the index of `key` if it is still
/// there, otherwise the first selectable row, or the selectable row at or above the previous index once
/// the user has navigated.
pub fn stable_index(rows: &[SeRow], key: Option<&str>, prev_index: Option<usize>, user_moved: bool) -> Option<usize> {
    if let Some(key) = key.filter(|key| !key.is_empty())
        && let Some(index) = rows.iter().position(|row| row.key() == key)
    {
        return Some(index);
    }
    if user_moved
        && let Some(prev) = prev_index
        && !rows.is_empty()
    {
        let from = prev.min(rows.len() - 1);
        if let Some(index) = (0..=from).rev().find(|&index| !rows[index].is_section()) {
            return Some(index);
        }
    }
    rows.iter().position(|row| !row.is_section())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{IpcPath, Pos};
    use std::path::PathBuf;

    fn range(line: u32) -> Range {
        Range::new(Pos::new(line, 0), Pos::new(line, 1))
    }

    fn sym(name: &str, kind: SymbolKind, path: &str, line: u32) -> SymbolMatch {
        SymbolMatch {
            name: name.to_owned(),
            kind,
            container: None,
            path: path.to_owned(),
            rel_path: path.replacen("/p/", "", 1),
            range: range(line),
            score: 0,
            indices: vec![0],
        }
    }

    fn ws(name: &str, kind: SymbolKind, path: &str, line: u32) -> WorkspaceSymbol {
        let location = Location { path: IpcPath(PathBuf::from(path)), range: range(line), uri: None };
        WorkspaceSymbol { name: name.to_owned(), kind, container: None, location }
    }

    const OPTS: MergeOptions<'static> = MergeOptions {
        root: Some("/p"),
        include_non_project: false,
        types_only: false,
        exclude_types: false,
        limit: 10,
    };

    fn names(hits: &SymbolHits) -> Vec<&str> {
        hits.items.iter().map(|item| item.name.as_str()).collect()
    }

    fn enabled(_: &ActionMeta) -> bool {
        true
    }

    /// Enabled unless the action needs the editor focused.
    fn enabled_outside_editor(action: &ActionMeta) -> bool {
        action.when.as_deref() != Some("editorFocus")
    }

    fn actions() -> Vec<ActionMeta> {
        let edit_configs = ActionMeta::new("run.editConfigs", "Edit Configurations…");
        vec![
            ActionMeta::new("code.optimizeImports", "Optimize Imports").category("Code"),
            ActionMeta { keywords: vec!["run config".to_owned()], ..edit_configs },
            ActionMeta { hidden: true, ..ActionMeta::new("internal.thing", "Optimize Secret") },
            ActionMeta::new("code.reformat", "Reformat Code").when("editorFocus"),
        ]
    }

    fn matched(list: &[ActionMeta], query: &str, recent: &[String]) -> Vec<ActionItem> {
        let opts = ActionMatchOptions { is_enabled: &enabled_outside_editor, recent, include_disabled: true };
        match_actions(list, query, &opts, &I18n::default())
    }

    fn ids(items: &[ActionItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    fn file(rel: &str) -> SeItem {
        SeItem::File(file_item(&format!("/p/{rel}"), rel, &[], None, None))
    }

    fn state(items: Vec<SeItem>, has_more: bool, loading: bool) -> SectionState {
        SectionState { items, has_more, loading }
    }

    fn row_kinds(rows: &[SeRow]) -> Vec<&'static str> {
        rows.iter()
            .map(|row| match row {
                SeRow::Section { .. } => "section",
                SeRow::Command { .. } => "command",
                SeRow::More { .. } => "more",
                SeRow::Item { item: SeItem::File(_), .. } => "file",
                SeRow::Item { item: SeItem::Symbol(_), .. } => "symbol",
                SeRow::Item { item: SeItem::Action(_), .. } => "action",
                SeRow::Item { item: SeItem::Text(_), .. } => "text",
                SeRow::Item { item: SeItem::Setting(_), .. } => "setting",
            })
            .collect()
    }

    #[test]
    fn keeps_index_order_and_appends_unknown_lsp_symbols_deduplicated() {
        let index =
            [sym("orderCount", SymbolKind::Method, "/p/A.kt", 3), sym("ordinal", SymbolKind::Field, "/p/B.kt", 1)];
        let lsp =
            [ws("orderCount", SymbolKind::Method, "/p/A.kt", 3), ws("ordersTotal", SymbolKind::Function, "/p/C.kt", 9)];
        let hits = merge_symbols(&index, &lsp, "ord", &OPTS);
        assert_eq!(names(&hits), ["orderCount", "ordinal", "ordersTotal"]);
        assert!(hits.items[2].lsp);
        assert_eq!(hits.items[2].indices, [0, 1, 2]);
        assert_eq!(hits.items[2].rel, "C.kt");
    }

    #[test]
    fn puts_exact_lsp_hits_before_fuzzy_index_hits() {
        let index = [sym("orderCount", SymbolKind::Method, "/p/A.kt", 3)];
        let lsp = [ws("ord", SymbolKind::Function, "/p/C.kt", 9)];
        assert_eq!(names(&merge_symbols(&index, &lsp, "ord", &OPTS)), ["ord", "orderCount"]);
    }

    #[test]
    fn filters_non_project_lsp_symbols_unless_included() {
        let lsp = [ws("orderLib", SymbolKind::Function, "/usr/lib/x.kt", 1)];
        assert!(merge_symbols(&[], &lsp, "ord", &OPTS).items.is_empty());
        let everywhere = MergeOptions { include_non_project: true, ..OPTS };
        assert_eq!(merge_symbols(&[], &lsp, "ord", &everywhere).items[0].rel, "/usr/lib/x.kt");
    }

    #[test]
    fn applies_type_filters_and_reports_overflow() {
        let index = [
            sym("Order", SymbolKind::Class, "/p/A.kt", 0),
            sym("order", SymbolKind::Variable, "/p/A.kt", 2),
            sym("OrderKind", SymbolKind::Enum, "/p/A.kt", 5),
        ];
        let types = merge_symbols(&index, &[], "ord", &MergeOptions { types_only: true, ..OPTS });
        assert_eq!(names(&types), ["Order", "OrderKind"]);
        let others = merge_symbols(&index, &[], "ord", &MergeOptions { exclude_types: true, ..OPTS });
        assert_eq!(names(&others), ["order"]);
        let limited = merge_symbols(&index, &[], "ord", &MergeOptions { limit: 2, ..OPTS });
        assert_eq!(limited.items.len(), 2);
        assert!(limited.has_more);
    }

    #[test]
    fn decorates_callables_with_parentheses() {
        let hits = merge_symbols(&[sym("run", SymbolKind::Function, "/p/A.kt", 0)], &[], "run", &OPTS);
        assert_eq!(hits.items[0].display, "run()");
    }

    #[test]
    fn matches_titles_with_highlight_indices_skipping_hidden_actions() {
        let found = matched(&actions(), "opt", &[]);
        assert_eq!(ids(&found), ["code.optimizeImports"]);
        assert_eq!(found[0].indices, [0, 1, 2]);
    }

    #[test]
    fn falls_back_to_keywords_category_without_indices() {
        let found = matched(&actions(), "run config", &[]);
        assert_eq!(found[0].id, "run.editConfigs");
        assert!(found[0].indices.is_empty());
    }

    #[test]
    fn ranks_disabled_actions_lower_but_keeps_them_greyed() {
        let list = [actions().remove(3), ActionMeta::new("x.code", "Code Cleanup")];
        let found: Vec<(String, bool)> =
            matched(&list, "code", &[]).into_iter().map(|item| (item.id, item.enabled)).collect();
        assert_eq!(found, [("x.code".to_owned(), true), ("code.reformat".to_owned(), false)]);
    }

    #[test]
    fn boosts_recently_used_actions() {
        let two = [ActionMeta::new("a.one", "Show Diff Left"), ActionMeta::new("a.two", "Show Diff Right")];
        assert_eq!(matched(&two, "show diff", &["a.two".to_owned()])[0].id, "a.two");
    }

    #[test]
    fn rejects_scattered_letters_but_keeps_substrings_and_word_starts() {
        let list = [
            ActionMeta::new("a.mute", "Mute Breakpoints"),
            ActionMeta::new("a.step", "Smart Step Into"),
            ActionMeta::new("a.perf", "Toggle Performance Overlay (Dev)"),
            ActionMeta::new("a.split", "Split Right"),
            ActionMeta::new("a.menu", "Main Menu"),
        ];
        assert_eq!(ids(&matched(&list, "main", &[])), ["a.menu"]);
        assert_eq!(ids(&matched(&list, "split", &[])), ["a.split"]);
        assert_eq!(ids(&matched(&list, "tpo", &[])), ["a.perf"]);
        assert_eq!(ids(&matched(&list, "ssi", &[])), ["a.step"]);
    }

    #[test]
    fn returns_nothing_for_an_empty_query() {
        assert!(matched(&actions(), "  ", &[]).is_empty());
    }

    #[test]
    fn file_item_splits_name_and_directory_highlights() {
        let item = file_item(
            "/p/src/orders/OrderController.kt",
            "src/orders/OrderController.kt",
            &[4, 11, 12, 16],
            Some(41),
            None,
        );
        assert_eq!(item.name, "OrderController.kt");
        assert_eq!(item.dir, "src/orders");
        assert_eq!(item.name_indices, [0, 1, 5]);
        assert_eq!(item.line, Some(41));
    }

    #[test]
    fn text_items_collapses_indentation_and_shifts_ranges() {
        let found =
            LineMatch { line: 4, text: "\t\t  val order = 1".to_owned(), ranges: vec![(8, 13)], text_offset: 0 };
        let items = text_items("/p/a.kt", "a.kt", &[found]);
        assert_eq!(items[0].text, "  val order = 1");
        assert_eq!(items[0].indices, [6, 7, 8, 9, 10]);
        assert_eq!((items[0].col, items[0].end_col), (8, 13));
    }

    #[test]
    fn build_rows_orders_sections_adds_headers_and_more_rows() {
        let opts = ActionMatchOptions { is_enabled: &enabled, recent: &[], include_disabled: true };
        let found = match_actions(&[ActionMeta::new("a", "Order")], "ord", &opts, &I18n::default());
        let sections = Sections::from([
            (SectionId::Actions, state(found.into_iter().map(SeItem::Action).collect(), false, false)),
            (SectionId::Files, state(vec![file("O.kt")], true, false)),
            (SectionId::Classes, state(Vec::new(), false, true)),
        ]);
        let rows = build_rows(&sections, true, None);
        assert_eq!(row_kinds(&rows), ["section", "file", "more", "section", "action"]);
        assert_eq!(row_kinds(&build_rows(&sections, false, None)), ["file", "more", "action"]);
    }

    #[test]
    fn build_rows_interleaves_the_all_tab_by_relevance_keeping_each_sources_order() {
        let upgrade = symbol_item(&sym("upgrade", SymbolKind::Function, "/p/m.py", 3));
        let line = LineMatch { line: 0, text: "upgrade".to_owned(), ranges: vec![(0, 3)], text_offset: 0 };
        let text = text_items("/p/a.md", "a.md", &[line]).into_iter().map(SeItem::Text).collect();
        let sections = Sections::from([
            (SectionId::Files, state(vec![file("src/upload_gray.py"), file("src/upgrade.py")], true, false)),
            (SectionId::Symbols, state(vec![SeItem::Symbol(upgrade)], false, false)),
            (SectionId::Text, state(text, false, false)),
        ]);
        let rows = build_rows(&sections, false, Some("upg"));
        let order: Vec<String> = rows
            .iter()
            .map(|row| match row {
                SeRow::More { section, .. } => format!("more:{}", section.id()),
                other => other.group().map_or("", SectionId::id).to_owned(),
            })
            .collect();
        assert_eq!(order, ["symbols", "files", "files", "text", "more:files"]);
        // The index ranked upload_gray first; mixing must not reorder within files.
        let files: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                SeRow::Item { item: SeItem::File(file), .. } => Some(file.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(files, ["upload_gray.py", "upgrade.py"]);
    }

    #[test]
    fn stable_index_keeps_the_selected_key_while_rows_stream_in() {
        let mut sections = Sections::from([(SectionId::Files, state(vec![file("A.kt"), file("B.kt")], false, false))]);
        let before = build_rows(&sections, true, None);
        let key = before[2].key().to_owned();
        sections.insert(SectionId::Classes, state(vec![file("C.kt")], false, false));
        let after = build_rows(&sections, true, None);
        let kept = stable_index(&after, Some(&key), Some(2), true).unwrap();
        assert_eq!(after[kept].key(), key);
        // Without a user selection the first selectable row wins.
        assert_eq!(stable_index(&after, None, None, false), Some(1));
        // A vanished key after navigation clamps to the previous index.
        assert_eq!(stable_index(&after, Some("gone"), Some(3), true), Some(3));
    }

    #[test]
    fn sections_for_picks_sources_per_tab_and_empty_query() {
        assert_eq!(sections_for(SeTab::All, true), [SectionId::Recent, SectionId::RecentActions]);
        assert!(sections_for(SeTab::Classes, true).is_empty());
        let all = [
            SectionId::Classes,
            SectionId::Files,
            SectionId::Symbols,
            SectionId::Endpoints,
            SectionId::Actions,
            SectionId::Text,
        ];
        assert_eq!(sections_for(SeTab::All, false), all);
        assert_eq!(sections_for(SeTab::Text, false), [SectionId::Text]);
    }

    #[test]
    fn waits_while_a_section_above_the_first_result_is_loading() {
        let sections = Sections::from([
            (SectionId::Classes, state(Vec::new(), false, true)),
            (SectionId::Files, state(vec![file("a.py"), file("b.py")], false, false)),
        ]);
        assert!(!top_result_settled(&sections, None));
    }

    #[test]
    fn is_settled_once_the_first_section_with_items_is_done_even_if_later_ones_load() {
        let sections = Sections::from([
            (SectionId::Classes, state(Vec::new(), false, false)),
            (SectionId::Files, state(vec![file("a.py")], false, false)),
            (SectionId::Text, state(Vec::new(), false, true)),
        ]);
        assert!(top_result_settled(&sections, None));
    }

    #[test]
    fn with_mixing_waits_for_every_source_but_a_text_search_the_top_row_already_beats() {
        let loading = || state(Vec::new(), false, true);
        let strong = || state(vec![file("upgrade.py")], false, false);
        let symbols_loading = Sections::from([(SectionId::Files, strong()), (SectionId::Symbols, loading())]);
        assert!(!top_result_settled(&symbols_loading, Some("upg")));
        let text_loading = Sections::from([(SectionId::Files, strong()), (SectionId::Text, loading())]);
        assert!(top_result_settled(&text_loading, Some("upg")));
        let weak = state(vec![file("xuxpxg.py")], false, false);
        let weak_top = Sections::from([(SectionId::Files, weak), (SectionId::Text, loading())]);
        assert!(!top_result_settled(&weak_top, Some("upg")));
    }

    #[test]
    fn is_settled_when_everything_finished_empty() {
        let sections = Sections::from([(SectionId::Files, state(Vec::new(), false, false))]);
        assert!(top_result_settled(&sections, None));
    }

    #[test]
    fn lists_scope_commands_matching_the_typed_prefix() {
        let Some(ParsedCommand::List(rows)) = parse_command("/") else { panic!("`/` lists the commands") };
        let listed: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                SeRow::Command { name, .. } => Some(*name),
                _ => None,
            })
            .collect();
        assert_eq!(listed, ["/classes", "/files", "/symbols", "/actions", "/text"]);
        let Some(ParsedCommand::List(rows)) = parse_command("/FI") else { panic!("`/FI` lists a command") };
        assert_eq!(rows.iter().map(SeRow::key).collect::<Vec<_>>(), ["cmd:files"]);
    }

    #[test]
    fn runs_a_complete_command_followed_by_a_space() {
        let files = ParsedCommand::Run { tab: SeTab::Files, rest: "upg".to_owned() };
        assert_eq!(parse_command("/files upg"), Some(files));
        assert_eq!(parse_command("/text "), Some(ParsedCommand::Run { tab: SeTab::Text, rest: String::new() }));
    }

    #[test]
    fn leaves_paths_and_other_queries_alone() {
        assert_eq!(parse_command("/Users/me/a.py"), None);
        assert_eq!(parse_command("/fil x"), None);
        assert_eq!(parse_command("upg"), None);
    }

    /// Classes with rows a and b, Files with c and a "more" row, Actions with d, each under its header.
    fn grouped_rows() -> Vec<SeRow> {
        let header = |section: SectionId| SeRow::Section { key: format!("#{}", section.id()), section, title: "" };
        vec![
            header(SectionId::Classes),
            grouped_item("a", SectionId::Classes),
            grouped_item("b", SectionId::Classes),
            header(SectionId::Files),
            grouped_item("c", SectionId::Files),
            SeRow::More { key: "more:files".to_owned(), section: SectionId::Files },
            header(SectionId::Actions),
            grouped_item("d", SectionId::Actions),
        ]
    }

    fn grouped_item(key: &str, section: SectionId) -> SeRow {
        let SeItem::File(item) = file(key) else { unreachable!("`file` builds a file item") };
        SeRow::Item { item: SeItem::File(FileItem { key: key.to_owned(), ..item }), section }
    }

    #[test]
    fn moves_to_the_first_item_of_the_next_group() {
        let rows = grouped_rows();
        assert_eq!(group_jump(&rows, 1, GroupDirection::Next), Some(4));
        assert_eq!(group_jump(&rows, 5, GroupDirection::Next), Some(7));
        assert_eq!(group_jump(&rows, 7, GroupDirection::Next), Some(7));
    }

    #[test]
    fn moves_to_the_start_of_the_current_group_then_the_previous_one() {
        let rows = grouped_rows();
        assert_eq!(group_jump(&rows, 5, GroupDirection::Previous), Some(4));
        assert_eq!(group_jump(&rows, 4, GroupDirection::Previous), Some(1));
        assert_eq!(group_jump(&rows, 2, GroupDirection::Previous), Some(1));
    }

    #[test]
    fn finds_groups_without_header_rows() {
        let flat = [
            grouped_item("a", SectionId::Classes),
            grouped_item("b", SectionId::Classes),
            grouped_item("c", SectionId::Files),
        ];
        assert_eq!(group_jump(&flat, 0, GroupDirection::Next), Some(2));
        assert_eq!(group_jump(&flat, 2, GroupDirection::Previous), Some(0));
    }

    #[test]
    fn is_none_with_a_single_group() {
        let rows = [grouped_item("a", SectionId::Files), grouped_item("b", SectionId::Files)];
        assert_eq!(group_jump(&rows, 0, GroupDirection::Next), None);
    }

    // Not from a spec: highlights of text with characters of several bytes are byte offsets, converted from
    // the char indices of the index and the UTF-16 columns of the text search.
    #[test]
    fn highlights_are_byte_offsets_whatever_unit_they_arrive_in() {
        let item = file_item("/p/src/Café.kt", "src/Café.kt", &[4, 7, 8], None, None);
        assert_eq!(item.name_indices, [0, 3, 5]);
        let found = LineMatch { line: 0, text: "    é😀x = 1".to_owned(), ranges: vec![(5, 8)], text_offset: 10 };
        let items = text_items("/p/a.kt", "a.kt", &[found]);
        assert_eq!(items[0].text, "  é😀x = 1");
        assert_eq!(items[0].indices, [4, 8]);
        assert_eq!((items[0].col, items[0].end_col), (15, 18));
        assert_eq!(items[0].key, "t:/p/a.kt:0:5");
    }

    // Not from a spec: a symbol weighs by its name, else with its container, unless it carries a score; a
    // file found by its path only weighs less.
    #[test]
    fn item_weight_uses_what_the_item_offers() {
        let mut symbol = symbol_item(&sym("list", SymbolKind::Method, "/p/A.kt", 3));
        assert_eq!(item_weight(&SeItem::Symbol(symbol.clone()), "list"), 1000 + 5 - 4);
        symbol.container = Some("Orders".to_owned());
        assert_eq!(item_weight(&SeItem::Symbol(symbol.clone()), "orders.list"), 1000 - 11);
        assert_eq!(item_weight(&SeItem::Symbol(symbol.clone()), "zzz"), 0);
        symbol.score = Some(900);
        assert_eq!(item_weight(&SeItem::Symbol(symbol), "zzz"), 900);
        assert_eq!(item_weight(&file("src/Order.kt"), "src"), 800 + 5 - 12 - 200);
    }

    // Not from a spec: tab ids round-trip and an action listed without a query has no score or highlights.
    #[test]
    fn tab_ids_round_trip_and_unmatched_actions_have_no_score() {
        for tab in TABS {
            assert_eq!(SeTab::from_id(tab.id()), Some(tab));
        }
        assert_eq!(SeTab::from_id("nope"), None);
        let item = action_item(&ActionMeta::new("a.one", "Show Diff"), false, &I18n::default());
        assert_eq!((item.key.as_str(), item.title.as_str()), ("a:a.one", "Show Diff"));
        assert_eq!((item.enabled, item.score, item.indices.len()), (false, 0, 0));
    }
}
