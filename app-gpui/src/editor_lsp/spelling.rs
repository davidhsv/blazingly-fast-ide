//! Proofreading › Typo: misspelt words of comments, strings and declared names (the backend's
//! speller extracts and checks them off the UI thread), reported as diagnostics of the
//! [`SPELLING_PRODUCER`] at the Typo level. Quick fixes: "Change to…" (comments, literals, text:
//! the word in place) or "Rename to…" (a declared name: through Rename, so references follow),
//! "Save 'word' to project / application dictionary", and the inspection's controls (→).
//!
//! The speller's answers come in as arguments; requests and settings writes go out as values.

use super::intentions::{IntentionItem, IntentionRun};
use super::lsp::{Debounce, Inflight};
use super::positions::range_to_offsets;
use super::rename::Span;
use crate::editor::buffer::{self, Assoc, map_offset};
use crate::platform::i18n::{I18n, Param};
use crate::platform_more::inspection_controls::{ControlItem, intention_controls};
use crate::platform_more::inspections::InspectionRef;
use crate::settings_model::inspections_model::{ProfileOverrides, SPELLING_INSPECTION, inspection_option, is_enabled};
use ide_core::inspection_settings::SpellingSettings;
use ide_core::{Diagnostic, Range, RequestId, Severity, TextChange};
use ropey::Rope;

/// The diagnostics producer of the Typo inspection.
pub const SPELLING_PRODUCER: &str = "spelling";
/// The delay of the check after an edit.
pub const CHECK_DELAY_MS: u64 = 800;
/// The delay of the first check of an opened editor.
pub const OPEN_CHECK_DELAY_MS: u64 = 200;
/// The id of a divider row in a fix's → menu.
pub const MENU_SEPARATOR: &str = "separator";

/// Where a misspelt word is (the speller's `TypoContext`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypoContext {
    /// A part of a declared name.
    Code,
    Literal,
    Comment,
    /// Plain text and Markdown.
    Prose,
}

/// A misspelt word the speller reported (UTF-16 range).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typo {
    pub range: Range,
    pub word: String,
    pub context: TypoContext,
}

/// What the Typo inspection's options include (IDEA's "Process code / literals / comments").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellScope {
    pub code: bool,
    pub literals: bool,
    pub comments: bool,
}

impl Default for SpellScope {
    fn default() -> Self {
        Self { code: true, literals: true, comments: true }
    }
}

/// Whether the Typo inspection runs under `profile`.
pub fn spelling_enabled(profile: &ProfileOverrides) -> bool {
    is_enabled(profile, SPELLING_INSPECTION)
}

/// What the Typo inspection's options under `profile` have checked.
pub fn spell_scope(profile: &ProfileOverrides) -> SpellScope {
    let on = |key: &str| inspection_option(profile, SPELLING_INSPECTION, key);
    SpellScope { code: on("processCode"), literals: on("processLiterals"), comments: on("processComments") }
}

/// Letters and digits only: what can replace one word of an identifier ("me sage" or "me-sage"
/// would break the code).
pub fn is_name_part(word: &str) -> bool {
    !word.is_empty() && word.chars().all(char::is_alphanumeric)
}

/// The diagnostics of the speller's typos, for [`super::diagnostics::DiagnosticStore`] under
/// [`SPELLING_PRODUCER`].
pub fn typo_diagnostics(typos: &[Typo], i18n: &mut I18n) -> Vec<Diagnostic> {
    typos
        .iter()
        .map(|typo| Diagnostic {
            range: typo.range,
            severity: Severity::Info,
            message: i18n.tx_built(&["Typo: In word '", "'"], &[Param::from(typo.word.as_str())]),
            source: Some("Spelling".into()),
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        })
        .collect()
}

/// A typo of the last check, at UTF-16 offsets of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedTypo {
    pub from: u32,
    pub to: u32,
    pub word: String,
    pub context: TypoContext,
}

/// Everything that decides what a check reports: rechecked when it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellConfig {
    pub words: SpellingSettings,
    pub enabled: bool,
    pub scope: SpellScope,
}

impl SpellConfig {
    pub fn new(words: &SpellingSettings, profile: &ProfileOverrides) -> Self {
        Self { words: words.clone(), enabled: spelling_enabled(profile), scope: spell_scope(profile) }
    }
}

/// A timer to start: report `ticket` to [`SpellChecker::fire`] once `delay_ms` elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckTimer {
    pub ticket: u64,
    pub delay_ms: u64,
}

/// What a due check does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStep {
    /// The inspection is off: clear the file's [`SPELLING_PRODUCER`] diagnostics, and cancel the
    /// check still running.
    Clear { cancel: Option<RequestId> },
    /// Ask the speller (once the editor is synced) with request `id`; `cancel` is the check it
    /// supersedes.
    Check { id: RequestId, scope: SpellScope, cancel: Option<RequestId> },
}

/// The Typo inspection of one editor: checks after edits (debounced) and after dictionary and
/// profile changes, and keeps the last check's typos for the quick fixes.
#[derive(Debug, Clone, Default)]
pub struct SpellChecker {
    debounce: Debounce,
    inflight: Inflight,
    config: SpellConfig,
    typos: Vec<PlacedTypo>,
}

impl SpellChecker {
    /// The editor opened with `config` in effect: the first check comes soon.
    pub fn open(config: SpellConfig) -> (Self, CheckTimer) {
        let mut checker = Self { config, ..Self::default() };
        let timer = checker.schedule(OPEN_CHECK_DELAY_MS);
        (checker, timer)
    }

    fn schedule(&mut self, delay_ms: u64) -> CheckTimer {
        CheckTimer { ticket: self.debounce.schedule(), delay_ms }
    }

    /// The last check's typos, mapped through later edits.
    pub fn typos(&self) -> &[PlacedTypo] {
        &self.typos
    }

    /// The document changed by `changes`: the typos follow, and a check is scheduled.
    pub fn edited(&mut self, changes: &[TextChange]) -> CheckTimer {
        self.typos = std::mem::take(&mut self.typos)
            .into_iter()
            .map(|typo| PlacedTypo {
                from: map_offset(typo.from, changes, Assoc::After),
                to: map_offset(typo.to, changes, Assoc::Before),
                ..typo
            })
            .filter(|typo| typo.to > typo.from)
            .collect();
        self.schedule(CHECK_DELAY_MS)
    }

    /// Settings changed: the file is checked again right away when the dictionaries, the
    /// inspection's state or its options changed.
    pub fn settings_changed(&mut self, config: SpellConfig) -> Option<CheckTimer> {
        if config == self.config {
            return None;
        }
        self.config = config;
        Some(self.schedule(0))
    }

    /// The timer of `ticket` elapsed; `id` is the request id a check would use.
    pub fn fire(&mut self, ticket: u64, id: RequestId) -> Option<CheckStep> {
        if !self.debounce.fire(ticket) {
            return None;
        }
        let cancel = self.inflight.cancel();
        if !self.config.enabled {
            self.typos.clear();
            return Some(CheckStep::Clear { cancel });
        }
        self.inflight.begin(id);
        Some(CheckStep::Check { id, scope: self.config.scope, cancel })
    }

    /// The speller answered `id` for `doc` (the text now): the diagnostics to publish, `None`
    /// when a newer check superseded it.
    pub fn answered(&mut self, id: RequestId, typos: &[Typo], doc: &Rope, i18n: &mut I18n) -> Option<Vec<Diagnostic>> {
        if !self.inflight.finish(id) {
            return None;
        }
        self.typos = typos
            .iter()
            .map(|typo| {
                let range = range_to_offsets(doc, typo.range);
                PlacedTypo { from: range.start, to: range.end, word: typo.word.clone(), context: typo.context }
            })
            .collect();
        Some(typo_diagnostics(typos, i18n))
    }

    /// The editor went away: the check to cancel. The caller clears the file's diagnostics.
    pub fn close(&mut self) -> Option<RequestId> {
        self.debounce.cancel();
        self.inflight.cancel()
    }

    /// The typo at `pos` whose word is still there.
    pub fn typo_at(&self, doc: &Rope, pos: u32) -> Option<&PlacedTypo> {
        self.typos
            .iter()
            .find(|typo| typo.from <= pos && pos <= typo.to && buffer::slice(doc, typo.from, typo.to) == typo.word)
    }
}

/// A dictionary of accepted words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dictionary {
    Project,
    Application,
}

/// A settings write: `value` (JSON) at `key` of the `scope` settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryWrite {
    pub scope: &'static str,
    pub key: &'static str,
    pub value: String,
}

/// Adds `word` (lowercased) to a dictionary of `words`; `None` when it is there already.
pub fn save_to_dictionary(words: &SpellingSettings, word: &str, dictionary: Dictionary) -> Option<DictionaryWrite> {
    let word = word.to_lowercase();
    let (list, scope, key) = match dictionary {
        Dictionary::Project => (&words.project_words, "project", "spelling.projectWords"),
        Dictionary::Application => (&words.ide_words, "global", "spelling.ideWords"),
    };
    if list.contains(&word) {
        return None;
    }
    let mut next = list.clone();
    next.push(word);
    next.sort();
    let value = serde_json::to_string(&next).ok()?;
    Some(DictionaryWrite { scope, key, value })
}

/// What a row of the Typo fixes runs ([`IntentionRun::Local`] ids and → menu ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellingCommand {
    /// Ask the speller for suggestions of the typo, then [`change_to`].
    ChangeTo(PlacedTypo),
    /// Store [`save_to_dictionary`] of the word.
    Save(String, Dictionary),
}

/// Where the fixes are offered.
#[derive(Debug, Clone, Copy)]
pub struct FixContext<'a> {
    /// The inspection profile in effect.
    pub profile: &'a ProfileOverrides,
    /// The document cannot be edited (library sources, stubs).
    pub read_only: bool,
    pub has_project: bool,
}

const CHANGE_ID: &str = "spelling.change";
const SAVE_ID: &str = "spelling.save";
const SAVE_PROJECT_ID: &str = "spelling.saveProject";
const SAVE_IDE_ID: &str = "spelling.saveIde";

fn menu_row(id: &str, label: &str) -> ControlItem {
    ControlItem {
        id: id.to_owned(),
        label: label.to_owned(),
        verbatim: false,
        icon: None,
        checked: None,
        run: None,
        submenu: Vec::new(),
    }
}

/// The Typo quick fixes at the caret (⌥↵), or none. Dictionary saves write settings, so they are
/// offered in read-only documents too (library sources, stubs); only the correction needs a
/// writable file. One row saves to the project dictionary (the application's without a project);
/// its → menu offers each level, a divider ([`MENU_SEPARATOR`]) and the inspection's controls.
/// Rows of the → menu without a `run` are run through [`spelling_command`].
pub fn spelling_intentions(
    checker: &SpellChecker,
    doc: &Rope,
    head: u32,
    selection_empty: bool,
    context: &FixContext<'_>,
    i18n: &mut I18n,
) -> Vec<IntentionItem> {
    let FixContext { profile, read_only, has_project } = *context;
    if !selection_empty || !spelling_enabled(profile) {
        return Vec::new();
    }
    let Some(typo) = checker.typo_at(doc, head) else { return Vec::new() };
    let mut submenu = Vec::new();
    if has_project {
        submenu.push(menu_row(SAVE_PROJECT_ID, "Save to project-level dictionary"));
    }
    submenu.push(menu_row(SAVE_IDE_ID, "Save to application-level dictionary"));
    submenu.push(menu_row(MENU_SEPARATOR, ""));
    submenu.extend(intention_controls(&InspectionRef { id: SPELLING_INSPECTION.into(), name: "Typo".into() }));
    let mut items = Vec::new();
    if !read_only {
        let label = if typo.context == TypoContext::Code { "Typo: Rename to…" } else { "Typo: Change to…" };
        items.push(IntentionItem {
            icon: Some("bulb-fix"),
            emphasis: true,
            submenu: Some(submenu.clone()),
            ..IntentionItem::new(CHANGE_ID, label, IntentionRun::Local(CHANGE_ID.into()))
        });
    }
    let label = i18n.tx_built(&["Save '", "' to dictionary"], &[Param::from(typo.word.as_str())]);
    items.push(IntentionItem {
        icon: Some("bulb-fix"),
        submenu: Some(submenu),
        ..IntentionItem::new(SAVE_ID, &label, IntentionRun::Local(SAVE_ID.into()))
    });
    items
}

/// What the row `id` of [`spelling_intentions`] (or of its → menu) does for the typo at `head`.
pub fn spelling_command(
    checker: &SpellChecker,
    doc: &Rope,
    head: u32,
    id: &str,
    has_project: bool,
) -> Option<SpellingCommand> {
    let typo = checker.typo_at(doc, head)?;
    let word = typo.word.clone();
    Some(match id {
        CHANGE_ID => SpellingCommand::ChangeTo(typo.clone()),
        SAVE_ID if has_project => SpellingCommand::Save(word, Dictionary::Project),
        SAVE_ID | SAVE_IDE_ID => SpellingCommand::Save(word, Dictionary::Application),
        SAVE_PROJECT_ID => SpellingCommand::Save(word, Dictionary::Project),
        _ => return None,
    })
}

/// What "Change to…" shows once the speller suggested `suggestions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeTo {
    /// "No suggestions for 'word'" at the caret (English source text).
    Hint(String),
    /// A chooser of replacements at the typo; choosing one runs [`correction`].
    Choose { title: String, suggestions: Vec<String> },
}

/// The suggestions chooser of `typo`. A part of a declared name only becomes another name part.
pub fn change_to(typo: &PlacedTypo, suggestions: Vec<String>, i18n: &mut I18n) -> ChangeTo {
    let rename = typo.context == TypoContext::Code;
    let suggestions: Vec<String> = suggestions.into_iter().filter(|s| !rename || is_name_part(s)).collect();
    let word = Param::from(typo.word.as_str());
    if suggestions.is_empty() {
        return ChangeTo::Hint(format!("No suggestions for '{}'", typo.word));
    }
    let title = if rename {
        i18n.tx_built(&["Rename '", "' to"], &[word])
    } else {
        i18n.tx_built(&["Change '", "' to"], &[word])
    };
    ChangeTo::Choose { title, suggestions }
}

/// How a chosen replacement is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correction {
    /// A declared name: rename that part of it through Rename, so references follow (IDEA's
    /// "Typo: Rename to").
    RenamePart { part: Span, replacement: String },
    /// Comments, literals and text: the word changes in place, the caret after it.
    Replace { change: TextChange, caret: u32 },
    /// The word is no longer there.
    Stale,
}

/// The correction of `typo` to `replacement` in `doc`.
pub fn correction(typo: &PlacedTypo, replacement: &str, doc: &Rope) -> Correction {
    if typo.context == TypoContext::Code {
        let part = Span { from: typo.from, to: typo.to };
        return Correction::RenamePart { part, replacement: replacement.to_owned() };
    }
    if buffer::slice(doc, typo.from, typo.to) != typo.word {
        return Correction::Stale;
    }
    let caret = typo.from + replacement.encode_utf16().count() as u32;
    Correction::Replace { change: TextChange { from: typo.from, to: typo.to, insert: replacement.to_owned() }, caret }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::diagnostics::{DiagnosticStore, diagnostic_marks, to_lint_diagnostics};
    use crate::platform_more::inspections::StyledDiagnostic;
    use ide_core::Pos;
    use ide_core::inspection_settings::InspectionOverride;
    use std::collections::BTreeMap;

    const PATH: &str = "/p/a.rs";
    const ALL: SpellScope = SpellScope { code: true, literals: true, comments: true };

    fn comment_typo() -> Typo {
        Typo { range: Range::new(Pos::new(0, 3), Pos::new(0, 9)), word: "mesage".into(), context: TypoContext::Comment }
    }

    fn profile(entry: InspectionOverride) -> ProfileOverrides {
        ProfileOverrides::from([(SPELLING_INSPECTION.to_owned(), entry)])
    }

    fn options(pairs: &[(&str, bool)]) -> ProfileOverrides {
        let options: BTreeMap<String, bool> = pairs.iter().map(|(key, on)| ((*key).to_owned(), *on)).collect();
        profile(InspectionOverride { options: Some(options), ..InspectionOverride::default() })
    }

    /// An editor of `text` checked once under `profile`, the speller answering `typos`: its
    /// checker, the scope it was asked for (`None`: not asked) and the diagnostics.
    fn checked(
        text: &str,
        profile: &ProfileOverrides,
        typos: &[Typo],
    ) -> (SpellChecker, Option<SpellScope>, DiagnosticStore) {
        let doc = Rope::from_str(text);
        let mut i18n = I18n::default();
        let mut store = DiagnosticStore::default();
        let (mut checker, timer) = SpellChecker::open(SpellConfig::new(&SpellingSettings::default(), profile));
        assert_eq!(timer.delay_ms, OPEN_CHECK_DELAY_MS);
        let scope = match checker.fire(timer.ticket, RequestId(1)) {
            Some(CheckStep::Check { id, scope, .. }) => {
                let diagnostics = checker.answered(id, typos, &doc, &mut i18n).expect("the current check");
                store.set_file_diagnostics(PATH, SPELLING_PRODUCER, diagnostics);
                Some(scope)
            }
            Some(CheckStep::Clear { .. }) => {
                store.set_file_diagnostics(PATH, SPELLING_PRODUCER, Vec::new());
                None
            }
            None => panic!("the first check is due"),
        };
        (checker, scope, store)
    }

    fn labels(items: &[IntentionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn renames_a_name_part_only_to_words_that_fit_in_an_identifier() {
        assert!(["message", "menage", "Mesa", "ünïcode1"].into_iter().all(is_name_part));
        assert!(!["me sage", "me-sage", "mes_age", ""].into_iter().any(is_name_part));
    }

    #[test]
    fn reports_typos_at_the_typo_level_and_offers_fixes_at_the_caret() {
        let none = ProfileOverrides::new();
        let doc = Rope::from_str("// mesage here");
        let (mut checker, scope, store) = checked("// mesage here", &none, &[comment_typo()]);
        assert_eq!(scope, Some(ALL));
        let shown = store.diagnostics_for(PATH, &none);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].diagnostic.message, "Typo: In word 'mesage'");
        assert_eq!(shown[0].diagnostic.severity, Severity::Info);
        assert!(shown[0].typo);
        let lint = to_lint_diagnostics(&doc, &shown);
        assert_eq!(lint[0].mark_class.as_deref(), Some("cm-lsp-typo"));
        assert!(diagnostic_marks(&lint).is_empty());

        let mut i18n = I18n::default();
        let fixes = FixContext { profile: &none, read_only: false, has_project: true };
        let items = spelling_intentions(&checker, &doc, 5, true, &fixes, &mut i18n);
        assert_eq!(labels(&items), ["Typo: Change to…", "Save 'mesage' to dictionary"]);
        let menu: Vec<&str> = items[0]
            .submenu
            .iter()
            .flatten()
            .map(|row| if row.id == MENU_SEPARATOR { "-" } else { row.label.as_str() })
            .collect();
        assert_eq!(
            menu,
            [
                "Save to project-level dictionary",
                "Save to application-level dictionary",
                "-",
                "Edit inspection profile setting",
                "Disable inspection"
            ]
        );
        assert!(spelling_intentions(&checker, &doc, 12, true, &fixes, &mut i18n).is_empty());

        let Some(SpellingCommand::Save(word, dictionary)) = spelling_command(&checker, &doc, 5, &items[1].id, true)
        else {
            panic!("the row saves the word")
        };
        let write = save_to_dictionary(&SpellingSettings::default(), &word, dictionary).expect("a new word");
        let value = r#"["mesage"]"#.to_owned();
        let expected = DictionaryWrite { scope: "project", key: "spelling.projectWords", value };
        assert_eq!(write, expected);
        // The dictionary change re-checks the file.
        let words = SpellingSettings { project_words: vec!["mesage".into()], ..SpellingSettings::default() };
        let timer = checker.settings_changed(SpellConfig::new(&words, &none)).expect("a re-check");
        assert!(matches!(checker.fire(timer.ticket, RequestId(2)), Some(CheckStep::Check { .. })));
    }

    #[test]
    fn does_not_run_while_the_inspection_is_disabled() {
        let off = profile(InspectionOverride { enabled: Some(false), ..InspectionOverride::default() });
        let (checker, scope, _) = checked("// mesage", &off, &[comment_typo()]);
        assert_eq!(scope, None);
        let doc = Rope::from_str("// mesage");
        let fixes = FixContext { profile: &off, read_only: false, has_project: true };
        assert!(spelling_intentions(&checker, &doc, 5, true, &fixes, &mut I18n::default()).is_empty());
    }

    #[test]
    fn renames_declared_names_instead_of_replacing_one_occurrence() {
        let none = ProfileOverrides::new();
        let range = Range::new(Pos::new(0, 0), Pos::new(0, 6));
        let typo = Typo { range, context: TypoContext::Code, ..comment_typo() };
        let text = "mesage = 1\nprint(mesage)";
        let (checker, _, _) = checked(text, &none, &[typo]);
        let doc = Rope::from_str(text);
        let fixes = FixContext { profile: &none, read_only: false, has_project: true };
        let items = spelling_intentions(&checker, &doc, 2, true, &fixes, &mut I18n::default());
        assert_eq!(items[0].label, "Typo: Rename to…");
        let placed = checker.typo_at(&doc, 2).expect("the typo");
        assert_eq!(
            correction(placed, "message", &doc),
            Correction::RenamePart { part: Span { from: 0, to: 6 }, replacement: "message".into() }
        );
    }

    #[test]
    fn in_a_read_only_document_saves_to_dictionaries_but_changes_nothing_in_place() {
        let none = ProfileOverrides::new();
        let (checker, scope, _) = checked("// mesage here", &none, &[comment_typo()]);
        assert!(scope.is_some());
        let doc = Rope::from_str("// mesage here");
        let fixes = FixContext { profile: &none, read_only: true, has_project: true };
        let items = spelling_intentions(&checker, &doc, 5, true, &fixes, &mut I18n::default());
        assert_eq!(labels(&items), ["Save 'mesage' to dictionary"]);
        let command = spelling_command(&checker, &doc, 5, &items[0].id, true);
        assert_eq!(command, Some(SpellingCommand::Save("mesage".into(), Dictionary::Project)));
        let write = save_to_dictionary(&SpellingSettings::default(), "mesage", Dictionary::Project);
        assert_eq!(write.map(|write| (write.scope, write.value)), Some(("project", r#"["mesage"]"#.to_owned())));
    }

    #[test]
    fn checks_only_what_the_typo_options_include_and_re_checks_when_they_change() {
        let first = options(&[("processCode", false), ("processLiterals", false)]);
        let (mut checker, scope, _) = checked("// mesage", &first, &[comment_typo()]);
        assert_eq!(scope, Some(SpellScope { code: false, literals: false, comments: true }));
        let next = options(&[("processComments", false)]);
        let config = SpellConfig::new(&SpellingSettings::default(), &next);
        let timer = checker.settings_changed(config).expect("a re-check");
        let Some(CheckStep::Check { scope, cancel, .. }) = checker.fire(timer.ticket, RequestId(2)) else {
            panic!("checked again")
        };
        assert_eq!(scope, SpellScope { code: true, literals: true, comments: false });
        assert_eq!(cancel, None);
    }

    #[test]
    fn keeps_lint_conversion_of_plain_info_diagnostics_unchanged() {
        let doc = Rope::from_str("abc");
        let plain = Diagnostic {
            range: Range::new(Pos::new(0, 0), Pos::new(0, 1)),
            severity: Severity::Info,
            message: "m".into(),
            source: None,
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        };
        let out = to_lint_diagnostics(&doc, &[StyledDiagnostic { diagnostic: plain, typo: false }]);
        assert_eq!(out[0].mark_class, None);
    }
}
