//! The inspection controls offered wherever a problem is shown (the submenu of an inspection's
//! fixes, Problems, Inspection Results): edit the inspection's profile setting, disable it,
//! change its severity. Every change is written to the profile in effect, at the level where
//! that profile is stored.

pub use super::build_tool_recovery::SETTINGS_ACTION;
use super::inspections::{InspectionRef, effective_profile_id};
use crate::platform::i18n::{I18n, Param};
use crate::settings_model::inspections_model::{
    INSPECTION_LEVELS, ProfileLevel, ProfileOverrides, profile_location, set_level, set_option,
};
use ide_core::settings::{CustomInspectionProfile, InspectionLevel, InspectionSettings};
use ide_workspace::settings::SettingsScope;
use serde_json::Value;

/// The argument of [`SETTINGS_ACTION`] that opens Editor › Inspections with the inspection `id`
/// selected.
pub fn inspection_settings_page(id: &str) -> String {
    format!("editor.inspections#{id}")
}

/// A change to one inspection of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionChange<'a> {
    Disable,
    /// `None` restores the inspection's default level.
    Severity(Option<InspectionLevel>),
    Option {
        key: &'a str,
        value: bool,
    },
}

/// A settings value to store.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectionWrite {
    pub scope: SettingsScope,
    pub key: &'static str,
    pub value: Value,
}

fn changed(overrides: &ProfileOverrides, id: &str, change: InspectionChange<'_>) -> ProfileOverrides {
    match change {
        InspectionChange::Disable => {
            let mut next = overrides.clone();
            next.entry(id.to_owned()).or_default().enabled = Some(false);
            next
        }
        InspectionChange::Severity(level) => set_level(overrides, id, level),
        InspectionChange::Option { key, value } => set_option(overrides, id, key, value),
    }
}

/// `value` without the object members that are `null`: an override only spells what it changes.
fn without_nulls(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields.into_iter().filter(|(_, field)| !field.is_null()).map(|(k, v)| (k, without_nulls(v))).collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

fn stored<T: serde::Serialize>(value: &T) -> Value {
    // Overrides and profiles hold only JSON-representable values (string keys, booleans, names).
    without_nulls(serde_json::to_value(value).unwrap_or(Value::Null))
}

/// The write that applies `change` to inspection `id` in the profile in effect. `None` when
/// that profile is a copy `settings` does not hold (a chosen copy that is gone is not in effect:
/// `Default` is, and takes the write).
pub fn update_inspection(
    settings: &InspectionSettings,
    has_project: bool,
    id: &str,
    change: InspectionChange<'_>,
) -> Option<InspectionWrite> {
    let (level, copy) = profile_location(effective_profile_id(settings, has_project));
    let scope = match level {
        ProfileLevel::Project => SettingsScope::Project,
        ProfileLevel::Ide => SettingsScope::Global,
    };
    let Some(copy) = copy else {
        let (key, overrides) = match level {
            ProfileLevel::Project => ("inspections.project", &settings.project),
            ProfileLevel::Ide => ("inspections.default", &settings.default),
        };
        return Some(InspectionWrite { scope, key, value: stored(&changed(overrides, id, change)) });
    };
    let (key, copies) = match level {
        ProfileLevel::Project => ("inspections.projectProfiles", &settings.project_profiles),
        ProfileLevel::Ide => ("inspections.ideProfiles", &settings.ide_profiles),
    };
    let profile = copies.get(copy)?;
    let mut next = copies.clone();
    let updated =
        CustomInspectionProfile { name: profile.name.clone(), overrides: changed(&profile.overrides, id, change) };
    next.insert(copy.to_owned(), updated);
    Some(InspectionWrite { scope, key, value: stored(&next) })
}

/// What choosing a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectionCommand {
    /// Run [`SETTINGS_ACTION`] with [`inspection_settings_page`] of this inspection.
    EditSettings(String),
    /// Store [`update_inspection`] of the inspection with [`InspectionChange::Disable`].
    Disable(String),
    /// Store [`update_inspection`] of the inspection with [`InspectionChange::Severity`].
    SetSeverity(String, Option<InspectionLevel>),
}

/// A menu row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlItem {
    pub id: String,
    /// English source text, translated by the menu unless `verbatim`.
    pub label: String,
    /// The label is final: it holds a name and is translated already.
    pub verbatim: bool,
    pub icon: Option<&'static str>,
    pub checked: Option<bool>,
    pub run: Option<InspectionCommand>,
    pub submenu: Vec<ControlItem>,
}

fn row(id: &str, label: &str, run: InspectionCommand) -> ControlItem {
    ControlItem {
        id: id.to_owned(),
        label: label.to_owned(),
        verbatim: false,
        icon: None,
        checked: None,
        run: Some(run),
        submenu: Vec::new(),
    }
}

/// The submenu of an inspection's fix in the intentions popup (its right-arrow menu).
pub fn intention_controls(inspection: &InspectionRef) -> Vec<ControlItem> {
    let edit = InspectionCommand::EditSettings(inspection.id.clone());
    vec![
        ControlItem { icon: Some("settings"), ..row("inspection.edit", "Edit inspection profile setting", edit) },
        row("inspection.disable", "Disable inspection", InspectionCommand::Disable(inspection.id.clone())),
    ]
}

/// How a level is spelled in settings and in row ids.
fn level_key(level: InspectionLevel) -> &'static str {
    match level {
        InspectionLevel::Error => "error",
        InspectionLevel::Warning => "warning",
        InspectionLevel::WeakWarning => "weakWarning",
        InspectionLevel::ServerProblem => "serverProblem",
        InspectionLevel::Typo => "typo",
        InspectionLevel::None => "none",
    }
}

/// The rows of the Problems / Inspection Results context menu for a problem's inspection.
/// `profile` holds the overrides in effect: the inspection's current level is checked.
pub fn problem_controls(inspection: &InspectionRef, profile: &ProfileOverrides, i18n: &mut I18n) -> Vec<ControlItem> {
    let id = &inspection.id;
    let current = profile.get(id).and_then(|entry| entry.severity);
    let severity = |row_id: &str, label: &str, level: Option<InspectionLevel>| ControlItem {
        checked: Some(current == level),
        ..row(row_id, label, InspectionCommand::SetSeverity(id.clone(), level))
    };
    let levels = INSPECTION_LEVELS
        .iter()
        .map(|&(level, label)| severity(&format!("severity.{}", level_key(level)), label, Some(level)));
    let disable = ControlItem {
        label: i18n.tx_built(&["Disable Inspection '", "'"], &[Param::from(inspection.name.as_str())]),
        verbatim: true,
        ..row("inspection.disable", "", InspectionCommand::Disable(id.clone()))
    };
    let change_severity = ControlItem {
        id: "inspection.severity".to_owned(),
        label: "Change Severity".to_owned(),
        verbatim: false,
        icon: None,
        checked: None,
        run: None,
        submenu: std::iter::once(severity("severity.default", "Default", None)).chain(levels).collect(),
    };
    let edit = ControlItem {
        icon: Some("settings"),
        ..row("inspection.edit", "Edit Inspection Profile Setting…", InspectionCommand::EditSettings(id.clone()))
    };
    vec![disable, change_severity, edit]
}

#[cfg(test)]
mod tests {
    use super::super::inspections::diagnostic_inspection;
    use super::*;
    use crate::settings_model::inspections_model::{SPELLING_INSPECTION, inspection_option};
    use serde_json::json;

    fn settings(raw: Value) -> InspectionSettings {
        serde_json::from_value(raw).expect("the backend takes the settings")
    }

    fn inspection(id: &str, name: &str) -> Option<InspectionRef> {
        Some(InspectionRef { id: id.to_owned(), name: name.to_owned() })
    }

    // inspection controls › names the inspection a diagnostic belongs to
    #[test]
    fn names_the_inspection_a_diagnostic_belongs_to() {
        assert_eq!(diagnostic_inspection("spelling", None), inspection(SPELLING_INSPECTION, "Typo"));
        assert_eq!(diagnostic_inspection("ruff", Some("F401")), inspection("lsp:ruff:F401", "ruff F401"));
        assert_eq!(diagnostic_inspection("pyright", None), inspection("lsp:pyright", "pyright"));
        assert_eq!(diagnostic_inspection("", None), None);
        // A built-in check reported per code: PHPStan's identifiers.
        assert_eq!(
            diagnostic_inspection("phpstan", Some("constant.notFound")),
            inspection("builtin:phpstan:constant.notFound", "PHPStan constant.notFound")
        );
        assert_eq!(diagnostic_inspection("phpstan", None), inspection("builtin:phpstan", "PHPStan"));
        assert_eq!(diagnostic_inspection("build", Some("E0308")), inspection("builtin:build", "Build problems"));
    }

    // inspection controls › writes to the profile in effect, at its level
    #[test]
    fn writes_to_the_profile_in_effect_at_its_level() {
        let global_default = settings(json!({ "default": { "x": { "severity": "error" } } }));
        assert_eq!(
            update_inspection(&global_default, false, SPELLING_INSPECTION, InspectionChange::Disable),
            Some(InspectionWrite {
                scope: SettingsScope::Global,
                key: "inspections.default",
                value: json!({ "x": { "severity": "error" }, "builtin:spelling": { "enabled": false } }),
            })
        );

        let project_default = settings(json!({ "profile": "project" }));
        let weak = InspectionChange::Severity(Some(InspectionLevel::WeakWarning));
        assert_eq!(
            update_inspection(&project_default, true, "lsp:ruff:F401", weak),
            Some(InspectionWrite {
                scope: SettingsScope::Project,
                key: "inspections.project",
                value: json!({ "lsp:ruff:F401": { "severity": "weakWarning" } }),
            })
        );

        let copy = settings(json!({
            "profile": "ide:strict",
            "ideProfiles": { "strict": { "name": "Strict", "overrides": {} } },
        }));
        let option = InspectionChange::Option { key: "processCode", value: false };
        assert_eq!(
            update_inspection(&copy, true, SPELLING_INSPECTION, option),
            Some(InspectionWrite {
                scope: SettingsScope::Global,
                key: "inspections.ideProfiles",
                value: json!({ "strict": {
                    "name": "Strict",
                    "overrides": { "builtin:spelling": { "options": { "processCode": false } } },
                } }),
            })
        );
    }

    // inspection controls › drops entries that end up empty
    #[test]
    fn drops_entries_that_end_up_empty() {
        let only_severity: ProfileOverrides =
            serde_json::from_value(json!({ "a": { "severity": "error" } })).expect("the backend takes the overrides");
        assert!(changed(&only_severity, "a", InspectionChange::Severity(None)).is_empty());
    }

    // inspection controls › reads options with their defaults
    #[test]
    fn reads_options_with_their_defaults() {
        assert!(inspection_option(&ProfileOverrides::new(), SPELLING_INSPECTION, "processCode"));
        let off = changed(
            &ProfileOverrides::new(),
            SPELLING_INSPECTION,
            InspectionChange::Option { key: "processCode", value: false },
        );
        assert!(!inspection_option(&off, SPELLING_INSPECTION, "processCode"));
    }

    // inspection controls › offers IDEA's menu rows, opening Settings on the inspection
    #[test]
    fn offers_ideas_menu_rows_opening_settings_on_the_inspection() {
        let inspection = InspectionRef { id: "lsp:ruff:F401".to_owned(), name: "ruff F401".to_owned() };
        let rows = intention_controls(&inspection);
        let labels = |rows: &[ControlItem]| rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&rows), ["Edit inspection profile setting", "Disable inspection"]);
        assert_eq!(rows[0].run, Some(InspectionCommand::EditSettings("lsp:ruff:F401".to_owned())));
        assert_eq!(
            (SETTINGS_ACTION, inspection_settings_page(&inspection.id).as_str()),
            ("file.settings", "editor.inspections#lsp:ruff:F401")
        );

        let profile: ProfileOverrides = serde_json::from_value(json!({ "lsp:ruff:F401": { "severity": "error" } }))
            .expect("the backend takes the overrides");
        let menu = problem_controls(&inspection, &profile, &mut I18n::default());
        assert_eq!(
            labels(&menu),
            ["Disable Inspection 'ruff F401'", "Change Severity", "Edit Inspection Profile Setting…"]
        );
        let checked: Vec<&str> =
            menu[1].submenu.iter().filter(|row| row.checked == Some(true)).map(|row| row.label.as_str()).collect();
        assert_eq!(checked, ["Error"]);
        assert_eq!(menu[1].submenu.len(), 7);
        assert_eq!(menu[1].submenu[3].id, "severity.weakWarning");
    }

    // Not from a spec: with a chosen copy that is gone `Default` takes the write, and nulls are
    // not stored.
    #[test]
    fn a_missing_copy_leaves_the_write_to_default_and_nulls_are_not_stored() {
        let gone = settings(json!({ "profile": "ide:gone" }));
        let write = update_inspection(&gone, true, "x", InspectionChange::Disable).expect("Default is written");
        assert_eq!((write.key, write.value), ("inspections.default", json!({ "x": { "enabled": false } })));
        assert_eq!(without_nulls(json!({ "a": null, "b": [{ "c": null, "d": 1 }] })), json!({ "b": [{ "d": 1 }] }));
    }
}
