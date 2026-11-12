//! Inspection profiles applied to diagnostics. Every diagnostic producer is an inspection: a
//! language server (`lsp:<server>`), optionally refined per diagnostic code
//! (`lsp:<server>:<code>`), or a check the IDE runs itself. The profile in effect turns
//! inspections off and remaps their severity; the editor, the Problems view and Inspect Code
//! all filter through [`apply_profile`].
//!
//! The profiles are the backend's [`InspectionSettings`]; the catalogue of built-in checks and
//! the edits of a profile are [`crate::settings_model::inspections_model`]'s.

use crate::platform::collate;
use crate::settings_model::inspections_model::{
    BUILTIN_INSPECTIONS, BuiltinInspection, ProfileLevel, ProfileOverrides, SeenCode, code_inspection_id,
    custom_profile_id, enabled_by_default, overrides_of, producer_inspection_id, profile_location,
};
use ide_core::settings::{CustomInspectionProfile, InspectionLevel, InspectionOverride, InspectionSettings};
use ide_core::{Diagnostic, Severity};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Id of the IDE-level default profile.
pub const DEFAULT_PROFILE: &str = "default";
/// Id of the profile stored with the project.
pub const PROJECT_PROFILE: &str = "project";

/// The editor severity a highlighting level is shown with.
pub fn level_severity(level: InspectionLevel) -> Severity {
    match level {
        InspectionLevel::Error => Severity::Error,
        InspectionLevel::Warning | InspectionLevel::ServerProblem => Severity::Warning,
        InspectionLevel::WeakWarning | InspectionLevel::Typo => Severity::Info,
        InspectionLevel::None => Severity::Hint,
    }
}

fn builtin_by_producer(producer: &str) -> Option<&'static BuiltinInspection> {
    BUILTIN_INSPECTIONS.iter().find(|builtin| builtin.producer == producer)
}

fn copies(settings: &InspectionSettings, level: ProfileLevel) -> &BTreeMap<String, CustomInspectionProfile> {
    match level {
        ProfileLevel::Ide => &settings.ide_profiles,
        ProfileLevel::Project => &settings.project_profiles,
    }
}

fn custom_of<'s>(settings: &'s InspectionSettings, id: &str) -> Option<&'s CustomInspectionProfile> {
    let (level, key) = profile_location(id);
    copies(settings, level).get(key?)
}

fn normalized_overrides(overrides: &ProfileOverrides) -> ProfileOverrides {
    overrides
        .iter()
        .filter(|(id, _)| !id.is_empty())
        .map(|(id, entry)| {
            let options = entry.options.clone().filter(|options| !options.is_empty());
            (id.clone(), InspectionOverride { options, ..entry.clone() })
        })
        .filter(|(_, entry)| *entry != InspectionOverride::default())
        .collect()
}

fn normalized_copies(
    profiles: &BTreeMap<String, CustomInspectionProfile>,
) -> BTreeMap<String, CustomInspectionProfile> {
    profiles
        .iter()
        .filter(|(key, _)| !key.trim().is_empty())
        .map(|(key, profile)| {
            let name = Some(profile.name.trim()).filter(|name| !name.is_empty()).unwrap_or(key.as_str());
            let copy =
                CustomInspectionProfile { name: name.to_owned(), overrides: normalized_overrides(&profile.overrides) };
            (key.clone(), copy)
        })
        .collect()
}

/// `settings` as the rest of this module expects them: a chosen profile that is not a profile
/// id becomes `Default`, overrides that change nothing are dropped, and a copy without a name
/// is named by its key. Values of the wrong type never get this far: the store refuses them.
pub fn normalized(settings: &InspectionSettings) -> InspectionSettings {
    let chosen = settings.profile.as_str();
    let known = chosen == PROJECT_PROFILE || profile_location(chosen).1.is_some();
    InspectionSettings {
        profile: if known { chosen } else { DEFAULT_PROFILE }.to_owned(),
        default: normalized_overrides(&settings.default),
        project: normalized_overrides(&settings.project),
        ide_profiles: normalized_copies(&settings.ide_profiles),
        project_profiles: normalized_copies(&settings.project_profiles),
    }
}

/// Whether the profile `id` exists (project-level ones only with a project open).
pub fn profile_exists(settings: &InspectionSettings, id: &str, has_project: bool) -> bool {
    if id == DEFAULT_PROFILE {
        return true;
    }
    if profile_location(id).0 == ProfileLevel::Project && !has_project {
        return false;
    }
    id == PROJECT_PROFILE || custom_of(settings, id).is_some()
}

/// The profile chosen, if it exists here; else `Default`.
pub fn effective_profile_id(settings: &InspectionSettings, has_project: bool) -> &str {
    if profile_exists(settings, &settings.profile, has_project) { settings.profile.as_str() } else { DEFAULT_PROFILE }
}

/// The English name of the profile `id`: a copy's own name, or the id of one that is gone.
pub fn profile_label(settings: &InspectionSettings, id: &str) -> String {
    match id {
        DEFAULT_PROFILE => "Default".to_owned(),
        PROJECT_PROFILE => "Project Default".to_owned(),
        _ => custom_of(settings, id).map_or_else(|| id.to_owned(), |copy| copy.name.clone()),
    }
}

/// A profile to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEntry {
    pub id: String,
    pub label: String,
    pub level: ProfileLevel,
}

/// The profiles to choose from: project level first (with a project), then IDE level; each
/// level's default, then its copies by name.
pub fn list_profiles(settings: &InspectionSettings, has_project: bool) -> Vec<ProfileEntry> {
    let level_entries = |level: ProfileLevel, default_id: &str| {
        let mut named: Vec<ProfileEntry> = copies(settings, level)
            .iter()
            .map(|(key, copy)| ProfileEntry { id: custom_profile_id(level, key), label: copy.name.clone(), level })
            .collect();
        named.sort_by(|a, b| collate::compare(&a.label, &b.label));
        let default = ProfileEntry { id: default_id.to_owned(), label: profile_label(settings, default_id), level };
        std::iter::once(default).chain(named)
    };
    let project = has_project.then(|| level_entries(ProfileLevel::Project, PROJECT_PROFILE));
    project.into_iter().flatten().chain(level_entries(ProfileLevel::Ide, DEFAULT_PROFILE)).collect()
}

/// Why a profile cannot be given a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileNameError {
    Empty,
    /// Another profile of the level has the name.
    Taken(String),
}

impl std::fmt::Display for ProfileNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Profile name cannot be empty"),
            Self::Taken(name) => write!(f, "A profile named '{name}' already exists"),
        }
    }
}

/// Checks a proposed profile name: names are unique per level. `own_id` is the profile being
/// renamed, which may keep its name.
pub fn validate_profile_name(
    name: &str,
    level: ProfileLevel,
    settings: &InspectionSettings,
    own_id: Option<&str>,
) -> Result<(), ProfileNameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProfileNameError::Empty);
    }
    let taken = list_profiles(settings, true)
        .iter()
        .any(|profile| profile.level == level && Some(profile.id.as_str()) != own_id && profile.label == name);
    if taken { Err(ProfileNameError::Taken(name.to_owned())) } else { Ok(()) }
}

/// `<name> copy`, numbered when a profile of `level` has that name.
pub fn copy_profile_name(name: &str, level: ProfileLevel, settings: &InspectionSettings) -> String {
    let base = format!("{name} copy");
    let free = |candidate: &String| validate_profile_name(candidate, level, settings, None).is_ok();
    if free(&base) {
        return base;
    }
    let numbered = (2u32..).map(|n| format!("{base} {n}")).find(free);
    numbered.unwrap_or(base)
}

/// The inspection a diagnostic belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionRef {
    pub id: String,
    pub name: String,
}

/// The inspection of a diagnostic of `producer`: its code's own when it has one, except for a
/// built-in check that is one inspection whatever the code.
pub fn diagnostic_inspection(producer: &str, code: Option<&str>) -> Option<InspectionRef> {
    if producer.is_empty() {
        return None;
    }
    let code = code.filter(|code| !code.is_empty());
    let builtin = builtin_by_producer(producer);
    if let Some(builtin) = builtin.filter(|builtin| !builtin.by_code || code.is_none()) {
        return Some(InspectionRef { id: builtin.id.to_owned(), name: builtin.name.to_owned() });
    }
    let base = producer_inspection_id(producer);
    let name = builtin.map_or(producer, |builtin| builtin.name);
    Some(match code {
        Some(code) => InspectionRef { id: code_inspection_id(&base, code), name: format!("{name} {code}") },
        None => InspectionRef { id: base, name: name.to_owned() },
    })
}

/// The producer of a diagnostic: the server that reported it, else the producer of the event
/// it came with, else what it names as its source.
pub fn producer_of<'d>(diagnostic: &'d Diagnostic, fallback: Option<&'d str>) -> &'d str {
    diagnostic.server.as_deref().or(fallback).or(diagnostic.source.as_deref()).unwrap_or("")
}

/// A diagnostic as a profile shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledDiagnostic {
    pub diagnostic: Diagnostic,
    /// Shown at the Typo level: drawn as a typo, not as a weak warning.
    pub typo: bool,
}

impl StyledDiagnostic {
    fn plain(diagnostic: &Diagnostic) -> Self {
        Self { diagnostic: diagnostic.clone(), typo: false }
    }
}

/// `items` of `producer` under `profile`: disabled inspections dropped, severity overrides
/// applied (a code's own override wins over its producer's, which wins over a built-in check's
/// default level). `None` when the profile changes nothing: `items` are shown as they are.
pub fn apply_profile(
    profile: &ProfileOverrides,
    producer: &str,
    items: &[Diagnostic],
) -> Option<Vec<StyledDiagnostic>> {
    let base = producer_inspection_id(producer);
    let own = profile.get(&base);
    if !own.and_then(|own| own.enabled).unwrap_or_else(|| enabled_by_default(&base)) {
        return Some(Vec::new());
    }
    let fallback = builtin_by_producer(producer).and_then(|builtin| builtin.default_level);
    let mut changed = false;
    let mut shown = Vec::with_capacity(items.len());
    for item in items {
        let code = item.code.as_deref().filter(|code| !code.is_empty());
        let by_code = code.and_then(|code| profile.get(&code_inspection_id(&base, code)));
        if by_code.is_some_and(|entry| entry.enabled == Some(false)) {
            changed = true;
            continue;
        }
        let level = by_code.and_then(|entry| entry.severity).or(own.and_then(|own| own.severity)).or(fallback);
        let severity = level.map_or(item.severity, level_severity);
        let typo = level == Some(InspectionLevel::Typo);
        changed |= typo || severity != item.severity;
        shown.push(StyledDiagnostic { diagnostic: Diagnostic { severity, ..item.clone() }, typo });
    }
    changed.then_some(shown)
}

/// Applies `profile` to diagnostics of mixed producers (Inspect Code results): they come back
/// grouped by producer, in the order the producers first appear.
pub fn apply_profile_mixed(profile: &ProfileOverrides, items: &[Diagnostic]) -> Vec<StyledDiagnostic> {
    let mut groups: Vec<(&str, Vec<Diagnostic>)> = Vec::new();
    for item in items {
        let producer = producer_of(item, None);
        match groups.iter_mut().find(|(grouped, _)| *grouped == producer) {
            Some((_, group)) => group.push(item.clone()),
            None => groups.push((producer, vec![item.clone()])),
        }
    }
    groups
        .iter()
        .flat_map(|(producer, group)| {
            apply_profile(profile, producer, group)
                .unwrap_or_else(|| group.iter().map(StyledDiagnostic::plain).collect())
        })
        .collect()
}

/// The profile in effect and its overrides.
pub fn active_profile(settings: &InspectionSettings, has_project: bool) -> (&str, ProfileOverrides) {
    let id = effective_profile_id(settings, has_project);
    (id, overrides_of(settings, id))
}

/// Tells when the profile in effect changes in content, whatever made the settings change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileWatch {
    last: ProfileOverrides,
}

impl ProfileWatch {
    /// Starts from the overrides in effect now.
    pub fn new(current: ProfileOverrides) -> Self {
        Self { last: current }
    }

    /// The settings or the project changed. Returns whether the overrides in effect differ
    /// from the ones seen last: what shows diagnostics then applies the profile again.
    pub fn update(&mut self, settings: &InspectionSettings, has_project: bool) -> bool {
        let (_, next) = active_profile(settings, has_project);
        if next == self.last {
            return false;
        }
        self.last = next;
        true
    }
}

/// Where the diagnostic codes seen are stored, for every window to read.
pub const SEEN_CODES_KEY: &str = "openidea.inspections.seenCodes";
/// The codes kept; the oldest go first.
pub const SEEN_CODES_LIMIT: usize = 500;
/// How long a change waits before it is stored, in milliseconds.
pub const SEEN_CODES_SAVE_DELAY_MS: u64 = 1000;
/// UTF-16 code units of a message kept as the sample of its code.
const SAMPLE_UNITS: usize = 200;

/// The diagnostic codes met at runtime, each offered as an inspection of its own in Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeenCodes {
    codes: Vec<SeenCode>,
}

fn sample(message: &str) -> String {
    let mut units = 0;
    message
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= SAMPLE_UNITS
        })
        .collect()
}

impl SeenCodes {
    /// The codes `stored` under [`SEEN_CODES_KEY`] holds; entries that are not codes are skipped.
    pub fn parse(stored: Option<&str>) -> Self {
        let mut seen = Self::default();
        let entries = stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        for entry in entries.as_ref().and_then(Value::as_array).into_iter().flatten() {
            let text = |name: &str| entry.get(name).and_then(Value::as_str);
            if let (Some(producer), Some(code)) = (text("producer"), text("code")) {
                seen.insert(SeenCode {
                    producer: producer.to_owned(),
                    code: code.to_owned(),
                    source: text("source").unwrap_or("").to_owned(),
                    sample: text("sample").unwrap_or("").to_owned(),
                });
            }
        }
        seen
    }

    fn contains(&self, producer: &str, code: &str) -> bool {
        self.codes.iter().any(|seen| seen.producer == producer && seen.code == code)
    }

    fn insert(&mut self, seen: SeenCode) -> bool {
        if self.contains(&seen.producer, &seen.code) {
            return false;
        }
        self.codes.push(seen);
        true
    }

    pub fn codes(&self) -> &[SeenCode] {
        &self.codes
    }

    /// Records the codes of a producer's diagnostics. Returns whether any was new: the list is
    /// then stored, [`SEEN_CODES_SAVE_DELAY_MS`] after the first unsaved change.
    pub fn note(&mut self, producer: &str, items: &[Diagnostic]) -> bool {
        if producer.is_empty() {
            return false;
        }
        let mut added = false;
        for item in items {
            let Some(code) = item.code.as_deref().filter(|code| !code.is_empty()) else { continue };
            added |= self.insert(SeenCode {
                producer: producer.to_owned(),
                code: code.to_owned(),
                source: item.source.clone().unwrap_or_default(),
                sample: sample(&item.message),
            });
        }
        if self.codes.len() > SEEN_CODES_LIMIT {
            self.codes.drain(..self.codes.len() - SEEN_CODES_LIMIT);
        }
        added
    }

    /// The codes seen in any window: what `stored` holds now, then this window's own that are
    /// not stored yet.
    pub fn merged_with(&self, stored: Option<&str>) -> Self {
        let mut merged = Self::parse(stored);
        for seen in &self.codes {
            merged.insert(seen.clone());
        }
        merged
    }

    /// What to store under [`SEEN_CODES_KEY`].
    pub fn serialized(&self) -> String {
        let entries: Vec<Value> = self
            .codes
            .iter()
            .map(|seen| {
                json!({ "producer": seen.producer, "code": seen.code, "source": seen.source, "sample": seen.sample })
            })
            .collect();
        Value::Array(entries).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Range;

    fn diag(code: Option<&str>, severity: Severity, server: Option<&str>) -> Diagnostic {
        Diagnostic {
            range: Range::default(),
            severity,
            message: format!("m {}", code.unwrap_or("null")),
            source: Some("Ruff".to_owned()),
            code: code.map(str::to_owned),
            unnecessary: false,
            deprecated: false,
            server: server.map(str::to_owned),
        }
    }

    fn warning(code: &str) -> Diagnostic {
        diag(Some(code), Severity::Warning, None)
    }

    fn settings(raw: Value) -> InspectionSettings {
        serde_json::from_value(raw).expect("the backend takes the settings")
    }

    fn profile(raw: Value) -> ProfileOverrides {
        serde_json::from_value(raw).expect("the backend takes the overrides")
    }

    fn shown(profile: &ProfileOverrides, producer: &str, items: &[Diagnostic]) -> Vec<(Option<String>, Severity)> {
        let styled = apply_profile(profile, producer, items).expect("the profile changes the diagnostics");
        styled.into_iter().map(|styled| (styled.diagnostic.code, styled.diagnostic.severity)).collect()
    }

    fn code(code: &str) -> Option<String> {
        Some(code.to_owned())
    }

    // inspection profiles › validates leniently
    #[test]
    fn validates_leniently() {
        let raw = json!({
            "profile": "project",
            "default": { "lsp:ruff": { "enabled": false }, "empty": {}, "x": { "severity": "typo" } },
        });
        let valid = normalized(&settings(raw));
        let expected = json!({
            "profile": "project",
            "default": { "lsp:ruff": { "enabled": false }, "x": { "severity": "typo" } },
        });
        assert_eq!(valid, settings(expected));
        assert!(valid.project.is_empty() && valid.ide_profiles.is_empty() && valid.project_profiles.is_empty());
        // A value of the wrong type or an unknown level is refused where the settings are stored.
        for refused in [json!({ "default": { "lsp:ruff": { "severity": "loud" } } }), json!({ "project": 3 })] {
            assert!(serde_json::from_value::<InspectionSettings>(refused).is_err());
        }
        assert_eq!(InspectionSettings::default().profile, "default");
        assert_eq!(normalized(&settings(json!({ "profile": "nonsense" }))).profile, "default");
        assert_eq!(effective_profile_id(&settings(json!({ "profile": "project" })), false), "default");
    }

    // inspection profiles › maps built-in producers to their inspections
    #[test]
    fn maps_built_in_producers_to_their_inspections() {
        assert_eq!(producer_inspection_id("file-references"), "builtin:file-references");
        assert_eq!(producer_inspection_id("ruff"), "lsp:ruff");
    }

    // inspection profiles › drops disabled inspections and remaps severities
    #[test]
    fn drops_disabled_inspections_and_remaps_severities() {
        let items = [warning("F401"), diag(Some("E501"), Severity::Error, None), diag(None, Severity::Info, None)];
        assert_eq!(apply_profile(&ProfileOverrides::new(), "ruff", &items), None);
        let off = profile(json!({ "lsp:ruff": { "enabled": false } }));
        assert_eq!(apply_profile(&off, "ruff", &items), Some(Vec::new()));
        let mixed = profile(json!({
            "lsp:ruff": { "severity": "error" },
            "lsp:ruff:F401": { "severity": "weakWarning" },
            "lsp:ruff:E501": { "enabled": false },
        }));
        assert_eq!(shown(&mixed, "ruff", &items), [(code("F401"), Severity::Info), (None, Severity::Error)]);
        let unhighlighted = profile(json!({ "lsp:ruff": { "severity": "none" } }));
        assert_eq!(shown(&unhighlighted, "ruff", &[warning("x")]), [(code("x"), Severity::Hint)]);
    }

    // inspection profiles › groups mixed results by reporting server
    #[test]
    fn groups_mixed_results_by_reporting_server() {
        let off = profile(json!({ "lsp:ruff": { "enabled": false } }));
        let by_server = |code: &str, server: &str| diag(Some(code), Severity::Warning, Some(server));
        let items = [by_server("a", "ruff"), by_server("b", "ty")];
        let out = apply_profile_mixed(&off, &items);
        assert_eq!(out.iter().map(|styled| styled.diagnostic.code.as_deref()).collect::<Vec<_>>(), [Some("b")]);
    }

    // inspection profiles › follows the project's choice of profile and notifies on content changes
    #[test]
    fn follows_the_projects_choice_of_profile_and_notifies_on_content_changes() {
        let mut watch = ProfileWatch::new(ProfileOverrides::new());
        let chosen = settings(json!({
            "profile": "project",
            "default": { "a": { "enabled": false } },
            "project": { "b": { "enabled": false } },
        }));
        let mut calls = 0;
        calls += u32::from(watch.update(&chosen, false));
        assert_eq!(active_profile(&chosen, false), ("default", profile(json!({ "a": { "enabled": false } }))));
        calls += u32::from(watch.update(&chosen, true));
        assert_eq!(active_profile(&chosen, true), ("project", profile(json!({ "b": { "enabled": false } }))));
        // The same settings read again are no change.
        calls += u32::from(watch.update(&chosen, true));
        assert_eq!(calls, 2);
    }

    // inspection profiles › gives the Typo inspection its default level and marks typo-level diagnostics
    #[test]
    fn gives_the_typo_inspection_its_default_level_and_marks_typo_level_diagnostics() {
        let typo = Diagnostic { source: Some("Spelling".to_owned()), ..diag(None, Severity::Info, None) };
        let none = ProfileOverrides::new();
        let styled = apply_profile(&none, "spelling", std::slice::from_ref(&typo)).expect("typos are marked");
        assert_eq!((styled[0].diagnostic.severity, styled[0].typo), (Severity::Info, true));
        let as_error = profile(json!({ "builtin:spelling": { "severity": "error" } }));
        let styled = apply_profile(&as_error, "spelling", std::slice::from_ref(&typo)).expect("the level changes");
        assert_eq!((styled[0].diagnostic.severity, styled[0].typo), (Severity::Error, false));
        let off = profile(json!({ "builtin:spelling": { "enabled": false } }));
        assert_eq!(apply_profile(&off, "spelling", std::slice::from_ref(&typo)), Some(Vec::new()));
        // Any inspection can be shown at the Typo level.
        let ruff_typos = profile(json!({ "lsp:ruff": { "severity": "typo" } }));
        assert!(apply_profile(&ruff_typos, "ruff", &[warning("x")]).is_some_and(|styled| styled[0].typo));
    }

    // inspection profiles › shows PHPStan results as warnings, each identifier turned off or re-graded on its own
    #[test]
    fn shows_phpstan_results_as_warnings_each_identifier_turned_off_or_re_graded_on_its_own() {
        let phpstan = |code: Option<&str>| Diagnostic {
            source: Some("PHPStan".to_owned()),
            ..diag(code, Severity::Warning, Some("phpstan"))
        };
        let items = [phpstan(Some("constant.notFound")), phpstan(Some("return.type")), phpstan(None)];
        assert_eq!(producer_inspection_id("phpstan"), "builtin:phpstan");
        // Already warnings: the default level changes nothing.
        assert_eq!(apply_profile(&ProfileOverrides::new(), "phpstan", &items), None);
        let graded = profile(json!({
            "builtin:phpstan:constant.notFound": { "enabled": false },
            "builtin:phpstan:return.type": { "severity": "error" },
        }));
        assert_eq!(
            shown(&graded, "phpstan", &items),
            [(code("return.type"), Severity::Error), (None, Severity::Warning)]
        );
        let off = profile(json!({ "builtin:phpstan": { "enabled": false } }));
        assert_eq!(apply_profile(&off, "phpstan", &items), Some(Vec::new()));
    }

    // inspection profiles › lists, labels and resolves copied profiles
    #[test]
    fn lists_labels_and_resolves_copied_profiles() {
        let s = normalized(&settings(json!({
            "profile": "ide:strict",
            "default": { "a": { "enabled": false } },
            "ideProfiles": {
                "strict": { "name": "Strict", "overrides": { "b": { "severity": "error" } } },
                "bad": {},
            },
            "projectProfiles": { "team": { "name": "Team", "overrides": {} } },
        })));
        assert_eq!(s.ide_profiles["bad"], CustomInspectionProfile { name: "bad".to_owned(), ..Default::default() });
        let listed = |has_project: bool| -> Vec<(String, String, ProfileLevel)> {
            list_profiles(&s, has_project).into_iter().map(|p| (p.id, p.label, p.level)).collect()
        };
        let entry = |id: &str, label: &str, level: ProfileLevel| (id.to_owned(), label.to_owned(), level);
        assert_eq!(
            listed(true),
            [
                entry("project", "Project Default", ProfileLevel::Project),
                entry("project:team", "Team", ProfileLevel::Project),
                entry("default", "Default", ProfileLevel::Ide),
                entry("ide:bad", "bad", ProfileLevel::Ide),
                entry("ide:strict", "Strict", ProfileLevel::Ide),
            ]
        );
        let ids: Vec<String> = listed(false).into_iter().map(|(id, _, _)| id).collect();
        assert_eq!(ids, ["default", "ide:bad", "ide:strict"]);
        assert_eq!(effective_profile_id(&s, false), "ide:strict");
        let with_profile = |id: &str| InspectionSettings { profile: id.to_owned(), ..s.clone() };
        assert_eq!(effective_profile_id(&with_profile("project:team"), false), "default");
        assert_eq!(effective_profile_id(&with_profile("ide:gone"), true), "default");
        assert_eq!(overrides_of(&s, "ide:strict"), profile(json!({ "b": { "severity": "error" } })));
        assert_eq!(profile_label(&s, "project"), "Project Default");
        let taken = validate_profile_name("Strict", ProfileLevel::Ide, &s, None);
        assert!(taken.is_err_and(|error| error.to_string().contains("already exists")));
        assert_eq!(validate_profile_name("Strict", ProfileLevel::Project, &s, None), Ok(()));
        assert_eq!(validate_profile_name("Strict", ProfileLevel::Ide, &s, Some("ide:strict")), Ok(()));
        let blank = validate_profile_name("  ", ProfileLevel::Ide, &s, None);
        assert!(blank.is_err_and(|error| error.to_string().contains("empty")));
        assert_eq!(copy_profile_name("Strict", ProfileLevel::Ide, &s), "Strict copy");
    }

    // Not from a spec: a copy's name is numbered when taken, and the codes seen are kept per
    // producer, capped, and merged with what other windows stored.
    #[test]
    fn numbers_copy_names_and_remembers_the_codes_seen() {
        let s = settings(json!({ "ideProfiles": {
            "a": { "name": "Strict copy" },
            "b": { "name": "Strict copy 2" },
        } }));
        assert_eq!(copy_profile_name("Strict", ProfileLevel::Ide, &s), "Strict copy 3");

        let mut seen = SeenCodes::parse(Some(r#"[{ "producer": "ruff", "code": "F401" }, 3, { "code": "x" }]"#));
        assert_eq!(seen.codes().len(), 1);
        assert!(!seen.note("ruff", &[warning("F401"), diag(None, Severity::Info, None)]));
        assert!(!seen.note("", &[warning("E501")]));
        assert!(seen.note("ruff", &[warning("E501")]));
        assert_eq!(seen.codes()[1].sample, "m E501");
        let stored = r#"[{ "producer": "ty", "code": "T1", "source": "ty", "sample": "s" }]"#;
        let merged = seen.merged_with(Some(stored));
        let codes: Vec<&str> = merged.codes().iter().map(|seen| seen.code.as_str()).collect();
        assert_eq!(codes, ["T1", "F401", "E501"]);
        assert_eq!(SeenCodes::parse(Some(&merged.serialized())), merged);
        let many: Vec<Diagnostic> = (0..SEEN_CODES_LIMIT + 2).map(|n| warning(&format!("C{n}"))).collect();
        assert!(seen.note("big", &many));
        assert_eq!(seen.codes().len(), SEEN_CODES_LIMIT);
        assert_eq!(seen.codes()[0].code, "C2");
        assert_eq!(sample(&"é".repeat(300)).chars().count(), 200);
    }
}
