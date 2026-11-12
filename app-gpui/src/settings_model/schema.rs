//! The settings as the dialog edits them: the backend's [`Settings`] as a JSON value, read and
//! written one dotted key at a time.
//!
//! The shape, the defaults and what a stored value may be are the backend's
//! (`ide_core::settings`): a value is valid when [`parse_settings`] takes it. Nothing here
//! repairs a malformed value; the store refuses it when it is written.

use ide_core::settings::Settings;
use serde::Deserialize;
use serde_json::{Map, Value};

/// `settings` as the JSON value the dialog reads and stages edits against.
pub fn settings_value(settings: &Settings) -> Value {
    // The settings types hold only JSON-representable values (string keys, finite numbers).
    serde_json::to_value(settings).unwrap_or(Value::Null)
}

/// The JSON value of the default settings.
pub fn default_settings() -> Value {
    settings_value(&Settings::default())
}

/// The settings a JSON value spells, missing fields at their defaults. `Err` says what the
/// backend refuses (a value of the wrong type).
pub fn parse_settings(raw: &Value) -> Result<Settings, String> {
    Settings::deserialize(raw).map_err(|error| error.to_string())
}

fn child<'v>(value: &'v Value, segment: &str) -> Option<&'v Value> {
    match value {
        Value::Object(fields) => fields.get(segment),
        Value::Array(items) => segment.parse::<usize>().ok().and_then(|index| items.get(index)),
        _ => None,
    }
}

/// The value at the dotted `key` of `source`; `None` when a segment is missing.
pub fn get_at<'v>(source: &'v Value, key: &str) -> Option<&'v Value> {
    key.split('.').try_fold(source, child)
}

fn fields_of(source: Option<&Value>) -> Map<String, Value> {
    source.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn set_in(source: Option<&Value>, key: &str, value: Value, remove_null: bool) -> Value {
    let mut fields = fields_of(source);
    match key.split_once('.') {
        Some((head, rest)) => {
            let inner = set_in(fields.get(head), rest, value, remove_null);
            fields.insert(head.to_owned(), inner);
        }
        None if remove_null && value.is_null() => {
            fields.remove(key);
        }
        None => {
            fields.insert(key.to_owned(), value);
        }
    }
    Value::Object(fields)
}

/// A copy of `source` with the dotted `key` set. What is not an object on the way becomes one.
pub fn set_at(source: &Value, key: &str, value: Value) -> Value {
    set_in(Some(source), key, value, false)
}

/// Like [`set_at`], but `null` removes the key, as a settings write of `null` does.
pub fn put_at(source: &Value, key: &str, value: Value) -> Value {
    set_in(Some(source), key, value, true)
}

/// Like [`put_at`] on a value that may be missing.
pub fn put_at_opt(source: Option<&Value>, key: &str, value: Value) -> Value {
    set_in(source, key, value, true)
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // 13 and 13.0 are the same setting.
        (Value::Number(a), Value::Number(b)) => a == b || a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b)),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(key, a)| b.get(key).is_some_and(|b| same(a, b)))
        }
        _ => a == b,
    }
}

/// Whether two settings values are the same. A missing value and `null` are the same: writing
/// `null` removes a key.
pub fn deep_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.filter(|value| !value.is_null()), b.filter(|value| !value.is_null())) {
        (Some(a), Some(b)) => same(a, b),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::LOCALES;
    use crate::platform::keymap::default_preset;
    use ide_core::settings::{AgentSettings, CMakeProfile, FileIconThemeOptions, NotificationGroupSettings};
    use serde_json::json;

    fn parsed(raw: Value) -> Settings {
        parse_settings(&raw).expect("the backend takes the value")
    }

    fn refused(raw: Value) -> bool {
        parse_settings(&raw).is_err()
    }

    // validateSettings › defaults focus synchronization on and preserves an explicit opt-out
    #[test]
    fn defaults_focus_synchronization_on_and_preserves_an_explicit_opt_out() {
        assert!(parsed(json!({ "files": {} })).files.sync_on_focus);
        assert!(!parsed(json!({ "files": { "syncOnFocus": false } })).files.sync_on_focus);
    }

    // validateSettings › fills defaults for missing or malformed values
    //
    // The backend fills what is missing. Where the webview repaired a malformed value, the
    // backend either keeps it (a string or number of the right type, whatever it says) or
    // refuses the whole value (a wrong type).
    #[test]
    fn fills_defaults_for_missing_values_and_refuses_malformed_ones() {
        assert_eq!(parsed(json!({})), Settings::default());
        assert!(refused(json!("garbage")));
        let kept = parsed(json!({
            "appearance": { "theme": "neon", "accent": "red", "uiFontSize": 400, "locale": "xx" },
            "editor": { "tabSize": 2, "fontFamily": "  " },
            "files": { "exclude": ["**/a"] },
            "keymapOverrides": { "file.save": ["Ctrl+S"] },
        }));
        assert_eq!(kept.appearance.theme, "neon");
        assert_eq!(kept.appearance.accent, "red");
        assert_eq!(kept.appearance.ui_font_size, 400.0);
        assert_eq!(kept.appearance.locale, "xx");
        assert_eq!(kept.appearance.zoom, 100);
        assert_eq!((kept.editor.font_size, kept.editor.tab_size, kept.editor.soft_wrap), (13.0, 2, true));
        assert_eq!(kept.editor.font_family, "  ");
        assert_eq!(kept.files.exclude, ["**/a"]);
        assert_eq!(kept.keymap, default_preset(cfg!(target_os = "macos")));
        assert_eq!(kept.keymap_overrides["file.save"], ["Ctrl+S"]);
        assert_eq!(kept.general, Settings::default().general);
        for malformed in [
            json!({ "editor": { "fontSize": "big" } }),
            json!({ "editor": { "tabSize": 2.6 } }),
            json!({ "editor": { "softWrap": "yes" } }),
            json!({ "files": { "exclude": ["**/a", 3, "", null] } }),
            json!({ "keymap": 12 }),
            json!({ "keymapOverrides": { "file.save": ["Ctrl+S", 1], "bad": "Meta+S" } }),
            json!({ "general": null }),
        ] {
            assert!(refused(malformed.clone()), "{malformed}");
        }
    }

    // validateSettings › keeps valid values, including dark-hc
    #[test]
    fn keeps_valid_values_including_dark_hc() {
        let v = parsed(json!({ "appearance": { "theme": "dark-hc", "accent": "#C9A080", "locale": "zh" } }));
        assert_eq!(v.appearance.theme, "dark-hc");
        // The accent is kept as written; the webview lower-cased it.
        assert_eq!(v.appearance.accent, "#C9A080");
        assert_eq!(v.appearance.locale, "zh");
        for locale in LOCALES {
            assert_eq!(parsed(json!({ "appearance": { "locale": locale.id } })).appearance.locale, locale.id);
        }
    }

    // validateSettings › reads and writes dotted keys immutably
    #[test]
    fn reads_and_writes_dotted_keys_immutably() {
        let src = json!({ "editor": { "fontSize": 13 } });
        let out = set_at(&src, "editor.fontSize", json!(14));
        assert_eq!(src["editor"]["fontSize"], 13);
        assert_eq!(get_at(&out, "editor.fontSize"), Some(&json!(14)));
        assert_eq!(get_at(&out, "editor.nope.deeper"), None);
    }

    // agent chat settings › defaults every meter but the cost to on and keeps stored values
    #[test]
    fn defaults_every_meter_but_the_cost_to_on_and_keeps_stored_values() {
        let defaults =
            AgentSettings { show_context_usage: true, show_work_time: true, show_usage_limits: true, show_cost: false };
        assert_eq!(parsed(json!({})).agents, defaults);
        let stored = parsed(json!({ "agents": { "showUsageLimits": false, "showCost": true } })).agents;
        assert_eq!(stored, AgentSettings { show_usage_limits: false, show_cost: true, ..defaults });
        // A value of the wrong type is refused, not replaced by its default.
        assert!(refused(json!({ "agents": { "showWorkTime": "no" } })));
    }

    // appearance & behavior settings › defaults automatic update checks to on and respects an explicit opt-out
    #[test]
    fn defaults_automatic_update_checks_and_respects_an_explicit_choice() {
        assert!(!parsed(json!({})).general.auto_check_updates);
        assert!(parsed(json!({ "general": { "autoCheckUpdates": true } })).general.auto_check_updates);
        assert!(refused(json!({ "general": { "autoCheckUpdates": "off" } })));
    }

    // View › Appearance toggles › shows the toolbar and the status bar unless settings hide them
    #[test]
    fn shows_the_toolbar_and_the_status_bar_unless_settings_hide_them() {
        let defaults = parsed(json!({})).appearance;
        assert!(defaults.show_main_toolbar && defaults.show_status_bar);
        let hidden = parsed(json!({ "appearance": { "showMainToolbar": false } })).appearance;
        assert!(!hidden.show_main_toolbar && hidden.show_status_bar);
        assert!(refused(json!({ "appearance": { "showStatusBar": "no" } })));
    }

    // appearance & behavior settings › keeps valid file icon theme options and drops or clamps the rest
    //
    // The backend keeps the options as stored: numbers are not clamped and an entry with a
    // field of the wrong type is refused.
    #[test]
    fn keeps_valid_file_icon_theme_options() {
        assert!(parsed(json!({})).appearance.file_icon_theme_options.is_empty());
        let stored = json!({
            "p/t": {
                "groups": { "tests": true },
                "choices": { "arrows": "plus" },
                "hideFiles": true,
                "hideUiIcons": true,
                "monochrome": "#546E7A",
                "saturation": 50,
                "opacity": 1,
                "iconSize": 20,
                "rowHeight": 100,
            },
        });
        let appearance = parsed(json!({ "appearance": { "fileIconThemeOptions": stored } })).appearance;
        let options = appearance.file_icon_theme_options;
        let theme: &FileIconThemeOptions = options.get("p/t").expect("the theme's options");
        assert_eq!(theme.groups.get("tests"), Some(&true));
        assert_eq!(theme.choices.get("arrows").map(String::as_str), Some("plus"));
        assert!(theme.hide_files && !theme.hide_folders && !theme.hollow_folders);
        assert!(theme.hide_ui_icons && !theme.hide_psi_icons);
        assert_eq!(theme.monochrome.as_deref(), Some("#546E7A"));
        assert_eq!((theme.saturation, theme.opacity), (Some(50), Some(1)));
        assert_eq!((theme.icon_size, theme.row_height, theme.accent.as_deref()), (Some(20), Some(100), None));
        for malformed in [
            json!({ "p/t": { "groups": { "bad": "yes" } } }),
            json!({ "p/t": { "hollowFolders": "x" } }),
            json!({ "p/t": { "iconSize": 20.4 } }),
            json!({ "p/u": 5 }),
        ] {
            assert!(refused(json!({ "appearance": { "fileIconThemeOptions": malformed } })));
        }
    }

    // appearance & behavior settings › defaults Show tool window badges to on and honours the stored value
    #[test]
    fn defaults_show_tool_window_badges_to_on_and_honours_the_stored_value() {
        assert!(parsed(json!({})).appearance.show_tool_window_badges);
        let off = parsed(json!({ "appearance": { "showToolWindowBadges": false } })).appearance;
        assert!(!off.show_tool_window_badges);
        assert!(refused(json!({ "appearance": { "showToolWindowBadges": 1 } })));
    }

    // appearance & behavior settings › keeps the UI fallback font empty by default and trims a stored one
    #[test]
    fn keeps_the_ui_fallback_font_empty_by_default() {
        assert_eq!(parsed(json!({})).appearance.ui_font_fallback, "");
        // Stored as written: the backend does not trim it.
        let stored = parsed(json!({ "appearance": { "uiFontFallback": " 思源黑体 " } })).appearance;
        assert_eq!(stored.ui_font_fallback, " 思源黑体 ");
        assert!(refused(json!({ "appearance": { "uiFontFallback": 5 } })));
    }

    // appearance & behavior settings › normalises notification groups
    #[test]
    fn reads_notification_groups() {
        let stored = json!({ "groups": { "vcs": { "display": "weird", "log": false } } });
        let notifications = parsed(json!({ "notifications": stored })).notifications;
        assert!(notifications.balloons && !notifications.sound);
        // An unknown display type is kept as written.
        let vcs = NotificationGroupSettings { display: "weird".to_owned(), log: false };
        assert_eq!(notifications.groups.get("vcs"), Some(&vcs));
        assert!(refused(json!({ "notifications": { "sound": 1 } })));
    }

    // loads an old attach setting as ask
    #[test]
    fn keeps_an_old_attach_setting_as_stored() {
        assert_eq!(parsed(json!({})).general.open_project_in, "ask");
        // The backend does not migrate the retired value.
        assert_eq!(parsed(json!({ "general": { "openProjectIn": "attach" } })).general.open_project_in, "attach");
    }

    // CMake settings › preserves absent and explicitly empty profiles
    #[test]
    fn preserves_absent_and_explicitly_empty_profiles() {
        assert_eq!(parsed(json!({})).cmake.profiles, None);
        assert_eq!(settings_value(&Settings::default())["cmake"], json!({}));
        assert_eq!(parsed(json!({ "cmake": { "profiles": [] } })).cmake.profiles, Some(Vec::new()));
    }

    // CMake settings › preserves argument arrays and drops malformed argument values
    #[test]
    fn preserves_argument_arrays_and_refuses_malformed_argument_values() {
        let cmake_args = ["-DMESSAGE=two words", "-DQUOTE=\"hello\"", ""];
        let build_args = ["-j", "8", "--", "LABEL=it's a build"];
        let stored = json!({ "cmake": { "profiles": [{ "cmakeArgs": cmake_args, "buildArgs": build_args }] } });
        let profiles = parsed(stored).cmake.profiles.unwrap_or_default();
        assert_eq!(profiles[0].cmake_args, cmake_args);
        assert_eq!(profiles[0].build_args, build_args);
        assert!(refused(json!({ "cmake": { "profiles": [{ "cmakeArgs": ["-DFOO=bar", 42, null] }] } })));
    }

    // CMake settings › retains custom types and generators while normalizing malformed fields
    #[test]
    fn retains_custom_types_and_generators() {
        let custom = json!({
            "name": "Custom",
            "buildType": "Coverage",
            "generator": "Custom generator",
            "env": { "KEEP": "yes" },
        });
        let profiles = parsed(json!({ "cmake": { "profiles": [custom] } })).cmake.profiles;
        let expected = CMakeProfile {
            name: "Custom".to_owned(),
            build_type: "Coverage".to_owned(),
            generator: "Custom generator".to_owned(),
            env: [("KEEP".to_owned(), "yes".to_owned())].into(),
            ..CMakeProfile::default()
        };
        assert_eq!(profiles, Some(vec![expected]));
        assert!(refused(json!({ "cmake": { "profiles": [{ "buildArgs": 5 }] } })));
        assert!(refused(json!({ "cmake": { "profiles": [{ "env": { "DROP": 5 } }] } })));
        assert!(refused(json!({ "cmake": { "profiles": [null] } })));
    }

    // MCP server settings › defaults to off and only offers global scope
    #[test]
    fn the_mcp_server_defaults_to_off() {
        assert!(!Settings::default().mcp_server.enabled);
        assert!(!parsed(json!({})).mcp_server.enabled);
        assert!(parsed(json!({ "mcpServer": { "enabled": true } })).mcp_server.enabled);
    }

    // Not from a spec: `put_at` removes a key written as null, paths go through arrays by index,
    // and equality takes 13 and 13.0, and null and a missing value, as the same.
    #[test]
    fn puts_removes_and_compares_values() {
        let src = json!({ "a": { "b": 1, "c": [{ "d": 2 }] } });
        assert_eq!(put_at(&src, "a.b", Value::Null), json!({ "a": { "c": [{ "d": 2 }] } }));
        assert_eq!(set_at(&src, "a.b", Value::Null)["a"]["b"], Value::Null);
        assert_eq!(put_at(&src, "a.b.x", json!(true))["a"]["b"], json!({ "x": true }));
        assert_eq!(put_at_opt(None, "x.y", json!(1)), json!({ "x": { "y": 1 } }));
        assert_eq!(get_at(&src, "a.c.0.d"), Some(&json!(2)));
        assert_eq!(get_at(&src, "a.c.1"), None);
        assert!(deep_equal(Some(&json!(13)), Some(&json!(13.0))));
        assert!(deep_equal(Some(&Value::Null), None));
        assert!(deep_equal(Some(&json!({ "a": [1, "x"] })), Some(&json!({ "a": [1.0, "x"] }))));
        assert!(!deep_equal(Some(&json!({ "a": 1 })), Some(&json!({ "a": 1, "b": 2 }))));
        assert!(!deep_equal(Some(&json!([1])), Some(&json!({ "0": 1 }))));
        assert_eq!(parse_settings(&default_settings()), Ok(Settings::default()));
    }
}
