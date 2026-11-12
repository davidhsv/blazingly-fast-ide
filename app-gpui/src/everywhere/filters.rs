//! Per-tab result filters of Search Everywhere (IDEA's funnel): file types on Files, languages and symbol
//! kinds on Classes and Symbols, a file mask on Text. Stored as exclusions, so languages and kinds that
//! become known later show by default.

use super::model::SeTab;
use super::open::kind_name;
use ide_core::SymbolKind;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// The Files tab's filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LanguageFilter {
    /// Excluded language ids.
    pub languages: Vec<String>,
}

/// The filter of the Classes or the Symbols tab.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SymbolFilter {
    /// Excluded language ids.
    pub languages: Vec<String>,
    /// Excluded symbol kinds.
    pub kinds: Vec<String>,
}

/// The Text tab's filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TextFilter {
    /// Comma-separated globs (`*.ts, !*.test.*`); empty means every file.
    pub mask: String,
}

/// The filters of every tab that has one, in the shape they are stored. The default filters nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SeFilters {
    pub files: LanguageFilter,
    pub classes: SymbolFilter,
    pub symbols: SymbolFilter,
    pub text: TextFilter,
}

/// A tab with a filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterTab {
    Files,
    Classes,
    Symbols,
    Text,
}

pub const FILTER_TABS: [FilterTab; 4] = [FilterTab::Files, FilterTab::Classes, FilterTab::Symbols, FilterTab::Text];

/// The filter tab behind a dialog tab, if it has a filter.
pub fn filter_tab(tab: SeTab) -> Option<FilterTab> {
    match tab {
        SeTab::Files => Some(FilterTab::Files),
        SeTab::Classes => Some(FilterTab::Classes),
        SeTab::Symbols => Some(FilterTab::Symbols),
        SeTab::Text => Some(FilterTab::Text),
        SeTab::All | SeTab::Actions => None,
    }
}

fn field<'a>(raw: &'a Value, tab: &str, name: &str) -> Option<&'a Value> {
    raw.get(tab).and_then(|filter| filter.get(name))
}

/// The distinct strings of a stored list, in their order; anything else in it is dropped.
fn strings(value: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in value.and_then(Value::as_array).into_iter().flatten() {
        if let Some(text) = item.as_str()
            && !out.iter().any(|seen| seen == text)
        {
            out.push(text.to_owned());
        }
    }
    out
}

/// Filters from their stored value. Missing and malformed parts (written by an older or a newer version)
/// read as "nothing filtered".
pub fn sanitize_filters(raw: &Value) -> SeFilters {
    let symbols = |tab: &str| SymbolFilter {
        languages: strings(field(raw, tab, "languages")),
        kinds: strings(field(raw, tab, "kinds")),
    };
    SeFilters {
        files: LanguageFilter { languages: strings(field(raw, "files", "languages")) },
        classes: symbols("classes"),
        symbols: symbols("symbols"),
        text: TextFilter { mask: field(raw, "text", "mask").and_then(Value::as_str).unwrap_or_default().to_owned() },
    }
}

/// Whether `tab` currently filters anything out.
pub fn filter_active(filters: &SeFilters, tab: SeTab) -> bool {
    match tab {
        SeTab::Files => !filters.files.languages.is_empty(),
        SeTab::Classes => !filters.classes.languages.is_empty() || !filters.classes.kinds.is_empty(),
        SeTab::Symbols => !filters.symbols.languages.is_empty() || !filters.symbols.kinds.is_empty(),
        SeTab::Text => !filters.text.mask.trim().is_empty(),
        SeTab::All | SeTab::Actions => false,
    }
}

/// Symbol kinds the Classes tab offers, in IDEA's order.
pub const CLASS_KINDS: [&str; 6] = ["class", "interface", "struct", "enum", "trait", "typeAlias"];

/// Symbol kinds the Symbols tab offers, in IDEA's order.
pub const SYMBOL_KINDS: [&str; 17] = [
    "class",
    "interface",
    "struct",
    "enum",
    "trait",
    "typeAlias",
    "function",
    "method",
    "constructor",
    "field",
    "property",
    "variable",
    "constant",
    "enumMember",
    "module",
    "namespace",
    "macro",
];

/// The menu label of an offered symbol kind.
pub fn kind_label(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "class" => "Classes",
        "interface" => "Interfaces",
        "struct" => "Structs",
        "enum" => "Enums",
        "trait" => "Traits",
        "typeAlias" => "Type Aliases",
        "function" => "Functions",
        "method" => "Methods",
        "constructor" => "Constructors",
        "field" => "Fields",
        "property" => "Properties",
        "variable" => "Variables",
        "constant" => "Constants",
        "enumMember" => "Enum Members",
        "module" => "Modules",
        "namespace" => "Namespaces",
        "macro" => "Macros",
        _ => return None,
    })
}

/// The symbol kinds a tab's filter offers; none on the tabs that do not filter by kind.
pub fn kinds_for(tab: FilterTab) -> &'static [&'static str] {
    match tab {
        FilterTab::Classes => &CLASS_KINDS,
        FilterTab::Symbols => &SYMBOL_KINDS,
        FilterTab::Files | FilterTab::Text => &[],
    }
}

/// Kinds the menu does not list fall under the closest listed one.
fn kind_bucket(kind: &str) -> &str {
    match kind {
        "package" => "module",
        "typeParameter" => "typeAlias",
        other => other,
    }
}

/// Extension (lower case, without the dot) → language id, from each language's id and extensions. The first
/// language to claim an extension keeps it.
pub fn extension_index<'a, E: AsRef<str> + 'a>(
    languages: impl IntoIterator<Item = (&'a str, &'a [E])>,
) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for (id, extensions) in languages {
        for extension in extensions {
            index.entry(extension.as_ref().to_lowercase()).or_insert_with(|| id.to_owned());
        }
    }
    index
}

/// The language of `path` by the longest extension of its name that is known: `d.ts` before `ts`.
pub fn language_of<'a>(path: &str, by_ext: &'a HashMap<String, String>) -> Option<&'a str> {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    name.match_indices('.')
        .find_map(|(dot, _)| by_ext.get(&name[dot + 1..]).filter(|id| !id.is_empty()))
        .map(String::as_str)
}

/// What applying a tab's filter takes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedFilter {
    pub excluded_languages: Vec<String>,
    pub excluded_kinds: Vec<String>,
    /// Text tab: the include globs.
    pub mask: Option<String>,
}

/// The filter to apply on `tab`; `None` when the tab has none or it filters nothing.
pub fn resolve_filter(filters: &SeFilters, tab: SeTab) -> Option<ResolvedFilter> {
    if !filter_active(filters, tab) {
        return None;
    }
    let symbols = |filter: &SymbolFilter| ResolvedFilter {
        excluded_languages: filter.languages.clone(),
        excluded_kinds: filter.kinds.clone(),
        mask: None,
    };
    match filter_tab(tab)? {
        FilterTab::Text => {
            Some(ResolvedFilter { mask: Some(filters.text.mask.trim().to_owned()), ..ResolvedFilter::default() })
        }
        FilterTab::Files => {
            Some(ResolvedFilter { excluded_languages: filters.files.languages.clone(), ..ResolvedFilter::default() })
        }
        FilterTab::Classes => Some(symbols(&filters.classes)),
        FilterTab::Symbols => Some(symbols(&filters.symbols)),
    }
}

/// Whether a result passes the filter; `kind` is given for symbols. `languages` is the extension index
/// ([`extension_index`]): a file whose language is unknown, or any file while the index is not there yet,
/// is never filtered out by language.
pub fn passes_filter(
    filter: &ResolvedFilter,
    path: &str,
    kind: Option<SymbolKind>,
    languages: Option<&HashMap<String, String>>,
) -> bool {
    if let Some(kind) = kind {
        let bucket = kind_bucket(kind_name(kind));
        if filter.excluded_kinds.iter().any(|excluded| excluded == bucket) {
            return false;
        }
    }
    if filter.excluded_languages.is_empty() {
        return true;
    }
    let language = languages.and_then(|by_ext| language_of(path, by_ext));
    language.is_none_or(|language| filter.excluded_languages.iter().all(|excluded| excluded != language))
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        ActionMatchOptions, MergeOptions, SeItem, SettingsSearchEntry, SymbolMatch, TAB_LIMIT, match_actions,
        match_settings, merge_action_results, merge_symbols,
    };
    use super::*;
    use crate::platform::actions::ActionMeta;
    use crate::platform::i18n::I18n;
    use ide_core::{Pos, Range};
    use serde_json::json;

    fn by_ext() -> HashMap<String, String> {
        extension_index([("typescript", &["ts", "d.ts"][..]), ("rust", &["rs"][..])])
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn sym(name: &str, kind: SymbolKind, rel: &str) -> SymbolMatch {
        SymbolMatch {
            name: name.to_owned(),
            kind,
            container: None,
            path: format!("/p/{rel}"),
            rel_path: rel.to_owned(),
            range: Range::new(Pos::new(0, 0), Pos::new(0, 1)),
            score: 1,
            indices: Vec::new(),
        }
    }

    fn entries() -> Vec<SettingsSearchEntry> {
        let entry = |page_id: &str, title: &str, path: &[&str], keywords: &[&str]| SettingsSearchEntry {
            page_id: page_id.to_owned(),
            title: title.to_owned(),
            path: strs(path),
            keywords: strs(keywords),
        };
        vec![
            entry("editor.general", "General", &["Editor"], &[]),
            entry("editor.general", "Show line numbers", &["Editor", "General"], &[]),
            entry("appearance", "Appearance", &[], &["theme"]),
        ]
    }

    fn enabled(_: &ActionMeta) -> bool {
        true
    }

    fn kinds(items: &[SeItem]) -> Vec<&'static str> {
        items
            .iter()
            .map(|item| match item {
                SeItem::Action(_) => "action",
                SeItem::Setting(_) => "setting",
                SeItem::File(_) | SeItem::Symbol(_) | SeItem::Text(_) => "other",
            })
            .collect()
    }

    #[test]
    fn sanitizes_stored_filters_and_reports_which_tabs_are_filtered() {
        assert_eq!(sanitize_filters(&Value::Null), SeFilters::default());
        let raw = json!({ "files": { "languages": ["rust", 3, "rust"] }, "text": { "mask": "*.ts" }, "extra": 1 });
        let filters = sanitize_filters(&raw);
        assert_eq!(filters.files.languages, ["rust"]);
        assert_eq!(filters.symbols, SymbolFilter::default());
        assert!(filter_active(&filters, SeTab::Files));
        assert!(filter_active(&filters, SeTab::Text));
        assert!(!filter_active(&filters, SeTab::Classes));
        assert!(!filter_active(&filters, SeTab::All));
    }

    #[test]
    fn maps_paths_to_languages_by_their_longest_known_extension() {
        let by_ext = by_ext();
        assert_eq!(language_of("/p/a/main.RS", &by_ext), Some("rust"));
        assert_eq!(language_of("/p/types.d.ts", &by_ext), Some("typescript"));
        assert_eq!(language_of("/p/README", &by_ext), None);
    }

    #[test]
    fn keeps_unknown_file_types_and_buckets_unlisted_kinds() {
        let by_ext = by_ext();
        let filter =
            ResolvedFilter { excluded_languages: strs(&["rust"]), excluded_kinds: strs(&["module"]), mask: None };
        assert!(!passes_filter(&filter, "/p/a.rs", None, Some(&by_ext)));
        assert!(passes_filter(&filter, "/p/a.txt", None, Some(&by_ext)));
        assert!(!passes_filter(&filter, "/p/a.ts", Some(SymbolKind::Package), Some(&by_ext)));
        assert!(passes_filter(&filter, "/p/a.ts", Some(SymbolKind::Function), Some(&by_ext)));
    }

    #[test]
    fn filters_files_by_file_type() {
        let by_ext = by_ext();
        let filters = SeFilters { files: LanguageFilter { languages: strs(&["rust"]) }, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Files).unwrap();
        let found = ["/p/a.rs", "/p/b.ts", "/p/c.md"];
        let kept: Vec<&str> =
            found.into_iter().filter(|path| passes_filter(&filter, path, None, Some(&by_ext))).collect();
        assert_eq!(kept, ["/p/b.ts", "/p/c.md"]);
    }

    #[test]
    fn filters_symbols_by_kind_and_language() {
        let by_ext = by_ext();
        let found = [
            sym("run", SymbolKind::Function, "a.rs"),
            sym("Run", SymbolKind::Class, "b.ts"),
            sym("runner", SymbolKind::Variable, "c.ts"),
        ];
        let symbols = SymbolFilter { languages: strs(&["rust"]), kinds: strs(&["variable"]) };
        let filters = SeFilters { symbols, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Symbols).unwrap();
        let opts = MergeOptions {
            root: Some("/p"),
            include_non_project: false,
            types_only: false,
            exclude_types: false,
            limit: 4 * TAB_LIMIT + 1,
        };
        let merged = merge_symbols(&found, &[], "run", &opts);
        let names: Vec<&str> = merged
            .items
            .iter()
            .filter(|item| passes_filter(&filter, &item.path, Some(item.symbol_kind), Some(&by_ext)))
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(names, ["Run"]);
    }

    #[test]
    fn passes_the_text_tabs_file_mask_to_the_search() {
        let filters = SeFilters { text: TextFilter { mask: " *.ts, !*.test.* ".to_owned() }, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Text).unwrap();
        assert_eq!(filter.mask.as_deref(), Some("*.ts, !*.test.*"));
    }

    #[test]
    fn hides_disabled_actions_unless_included() {
        let i18n = I18n::default();
        let actions = [ActionMeta::new("a.on", "Reformat Code"), ActionMeta::new("a.off", "Reformat File")];
        let is_enabled = |action: &ActionMeta| action.id == "a.on";
        let keys = |include_disabled: bool| -> Vec<String> {
            let opts = ActionMatchOptions { is_enabled: &is_enabled, recent: &[], include_disabled };
            match_actions(&actions, "reformat", &opts, &i18n).into_iter().map(|item| item.key).collect()
        };
        assert_eq!(keys(false), ["a:a.on"]);
        assert_eq!(keys(true), ["a:a.on", "a:a.off"]);
    }

    #[test]
    fn matches_option_labels_page_titles_and_keywords() {
        let i18n = I18n::default();
        let entries = entries();
        let titles: Vec<String> =
            match_settings(&entries, "line num", &i18n).into_iter().map(|item| item.title).collect();
        assert_eq!(titles, ["Show line numbers"]);
        let pages: Vec<String> =
            match_settings(&entries, "theme", &i18n).into_iter().map(|item| item.page_id).collect();
        assert_eq!(pages, ["appearance"]);
        assert!(match_settings(&entries, "", &i18n).is_empty());
    }

    #[test]
    fn ranks_settings_below_equally_good_actions() {
        let i18n = I18n::default();
        let opts = ActionMatchOptions { is_enabled: &enabled, recent: &[], include_disabled: true };
        let actions = match_actions(&[ActionMeta::new("x", "Show Line Numbers")], "show line", &opts, &i18n);
        let merged = merge_action_results(actions, match_settings(&entries(), "show line", &i18n));
        assert_eq!(kinds(&merged), ["action", "setting"]);
    }

    #[test]
    fn lists_settings_with_actions_once_the_index_answers() {
        let i18n = I18n::default();
        let opts = ActionMatchOptions { is_enabled: &enabled, recent: &[], include_disabled: true };
        let actions = match_actions(&[ActionMeta::new("x", "Toggle Line Numbers")], "line numbers", &opts, &i18n);
        let merged = merge_action_results(actions, match_settings(&entries(), "line numbers", &i18n));
        assert_eq!(kinds(&merged), ["action", "setting"]);
    }

    // Not from a spec: a tab without an active filter resolves to none, and a filter passes every file while
    // the extension index is not there.
    #[test]
    fn inactive_filters_resolve_to_none_and_languages_need_the_index() {
        let filters = SeFilters { files: LanguageFilter { languages: strs(&["rust"]) }, ..SeFilters::default() };
        assert_eq!(resolve_filter(&filters, SeTab::Symbols), None);
        assert_eq!(resolve_filter(&filters, SeTab::All), None);
        let filter = resolve_filter(&filters, SeTab::Files).unwrap();
        assert!(passes_filter(&filter, "/p/a.rs", None, None));
    }

    // Not from a spec: every offered kind has a label and the filters keep their stored shape.
    #[test]
    fn offered_kinds_have_labels_and_filters_keep_their_stored_shape() {
        assert!(kinds_for(FilterTab::Symbols).iter().all(|kind| kind_label(kind).is_some()));
        assert_eq!(kinds_for(FilterTab::Classes), CLASS_KINDS);
        assert!(kinds_for(FilterTab::Text).is_empty());
        assert_eq!(kind_label("package"), None);
        assert_eq!(filter_tab(SeTab::Actions), None);
        let stored = serde_json::to_value(SeFilters::default()).unwrap();
        let shape = json!({
            "files": { "languages": [] },
            "classes": { "languages": [], "kinds": [] },
            "symbols": { "languages": [], "kinds": [] },
            "text": { "mask": "" },
        });
        assert_eq!(stored, shape);
        assert_eq!(sanitize_filters(&stored), SeFilters::default());
    }
}
