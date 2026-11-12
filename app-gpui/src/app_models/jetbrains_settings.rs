//! Import Settings… with a JetBrains `settings.zip`: the archive's colour schemes and the editor
//! and general options OpenIDEA has become settings writes, listed for confirmation first. Only
//! keys present in the archive are written (JetBrains leaves defaults out). Keymaps, code styles
//! and the rest are listed as not imported.

use crate::platform::i18n::I18n;
use crate::theme::color_scheme::{CustomColorScheme, custom_schemes};
use crate::theme::icls::{import_scheme, parse_icls};
use ide_workspace::settings::JetBrainsSettings;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// A setting the confirmation lists, with its imported value (both translated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRow {
    pub label: String,
    pub value: String,
}

/// What importing the archive does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JetBrainsImportPlan {
    /// Global settings writes, in order.
    pub writes: Vec<(String, Value)>,
    pub rows: Vec<ImportRow>,
    /// Categories present in the archive that are not imported (translated).
    pub skipped: Vec<String>,
    /// `.icls` files that could not be read.
    pub invalid: Vec<String>,
}

/// The editor settings the import builds on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorState {
    pub font_size: Option<f64>,
    /// `editor.customSchemes` as stored.
    pub custom_schemes: Value,
    /// `editor.colorOverrides` as stored.
    pub color_overrides: Map<String, Value>,
}

const USER_PREFIX: &str = "_@user_";

static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(/?)([A-Za-z_][\w.:-]*)((?:[^>/]|/[^>])*)(/?)>").expect("a valid pattern"));
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([A-Za-z_][\w.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("a valid pattern"));

fn unescape(raw: &str) -> String {
    raw.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn attributes(raw: &str) -> HashMap<String, String> {
    ATTRIBUTE
        .captures_iter(raw)
        .map(|c| {
            let value = c.get(2).or_else(|| c.get(3)).map_or("", |m| m.as_str());
            (c[1].to_owned(), unescape(value))
        })
        .collect()
}

/// `<option name value>` children of the root's `<component name>`; empty when the file or the
/// component is absent. With `tag`, the `name` of that child element instead, under key "".
fn options(xml: Option<&String>, component: &str, tag: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(xml) = xml else { return out };
    // Depth below the root element; `inside` while in the component looked for.
    let (mut depth, mut inside) = (0usize, false);
    for c in TAG.captures_iter(xml) {
        let (closing, name, attrs, empty) = (&c[1] == "/", &c[2], &c[3], &c[4] == "/");
        if closing {
            depth = depth.saturating_sub(1);
            if depth <= 1 {
                inside = false;
            }
            continue;
        }
        let attrs = attributes(attrs);
        if depth == 1 && name == "component" && attrs.get("name").map(String::as_str) == Some(component) {
            inside = !empty;
        } else if depth == 2 && inside {
            let option = attrs.get("name");
            match (tag, option, attrs.get("value")) {
                (Some(tag), Some(option), _) if name == tag => {
                    out.insert(String::new(), option.clone());
                }
                (None, Some(option), Some(value)) if name == "option" && !option.is_empty() => {
                    out.insert(option.clone(), value.clone());
                }
                _ => {}
            }
        }
        if !empty {
            depth += 1;
        }
    }
    out
}

fn flag(value: Option<&String>) -> Option<bool> {
    value.map(|value| value == "true")
}

fn positive(value: Option<&String>) -> Option<f64> {
    value.and_then(|value| value.trim().parse::<f64>().ok()).filter(|n| n.is_finite() && *n > 0.0)
}

/// A number as JSON writes it: whole numbers without a fraction.
fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 { Value::from(n as i64) } else { Value::from(n) }
}

struct Planner<'a> {
    plan: JetBrainsImportPlan,
    i18n: &'a I18n,
}

impl Planner<'_> {
    fn on_off(&self, on: bool) -> String {
        self.i18n.tx(if on { "On" } else { "Off" })
    }

    /// Writes `key` when the archive has it, and lists it as `label` (shown as `shown`, or the
    /// value itself).
    fn add(&mut self, key: &str, value: Option<Value>, label: &str, shown: Option<String>) {
        let Some(value) = value else { return };
        let shown = shown.unwrap_or_else(|| match &value {
            Value::Bool(on) => self.on_off(*on),
            Value::String(text) => text.clone(),
            other => other.to_string(),
        });
        self.plan.writes.push((key.to_owned(), value));
        let label = self.i18n.tx(label);
        self.plan.rows.push(ImportRow { label, value: shown });
    }
}

/// The plan for importing `bundle` over `editor`. `unique` gives each newly imported colour
/// scheme its id suffix (a fresh UUID).
pub fn plan_jetbrains_import(
    bundle: &JetBrainsSettings,
    editor: &EditorState,
    mut unique: impl FnMut() -> String,
    i18n: &I18n,
) -> JetBrainsImportPlan {
    let mut planner = Planner { plan: JetBrainsImportPlan::default(), i18n };
    let mut customs = custom_schemes(&editor.custom_schemes);
    let mut overrides = editor.color_overrides.clone();
    let mut overrides_changed = false;
    // Name in the .icls → scheme id.
    let mut imported: HashMap<String, String> = HashMap::new();
    for (file, text) in &bundle.files {
        let Some(name) = file.strip_prefix("colors/") else { continue };
        let Ok(parsed) = parse_icls(text) else {
            planner.plan.invalid.push(name.to_owned());
            continue;
        };
        let original = parsed.scheme.name.clone();
        let shown = original.strip_prefix(USER_PREFIX).unwrap_or(&original).to_owned();
        // Overwrites an earlier import of the same scheme instead of adding "Name (2)".
        let next = import_scheme(&customs, CustomColorScheme { name: shown.clone(), ..parsed.scheme }, true, &unique());
        customs = next.customs;
        overrides_changed |= overrides.remove(&next.id).is_some();
        let label = i18n.tx("Color Scheme");
        let value = customs.get(&next.id).map_or(shown, |scheme| scheme.name.clone());
        planner.plan.rows.push(ImportRow { label, value });
        imported.insert(original, next.id);
    }
    if !imported.is_empty() {
        let schemes = serde_json::to_value(&customs).unwrap_or(Value::Null);
        planner.plan.writes.push(("editor.customSchemes".to_owned(), schemes));
        if overrides_changed {
            planner.plan.writes.push(("editor.colorOverrides".to_owned(), Value::Object(overrides)));
        }
        let global = options(
            bundle.files.get("options/colors.scheme.xml"),
            "EditorColorsManagerImpl",
            Some("global_color_scheme"),
        );
        if let Some(id) = global.get("").and_then(|name| imported.get(name)) {
            let shown = customs.get(id).map(|scheme| scheme.name.clone());
            planner.add("editor.colorScheme", Some(Value::from(id.as_str())), "Current color scheme", shown);
        }
    }

    let font = options(bundle.files.get("options/editor-font.xml"), "DefaultFont", None);
    let family = font.get("FONT_FAMILY").map(|family| family.trim()).filter(|family| !family.is_empty());
    planner.add("editor.fontFamily", family.map(Value::from), "Font", None);
    let size = positive(font.get("FONT_SIZE"));
    planner.add("editor.fontSize", size.map(number), "Size", None);
    let line_height = positive(font.get("LINE_SPACING"))
        .zip(size.or(editor.font_size))
        .map(|(spacing, base)| number((base * spacing).round()));
    planner.add("editor.lineHeight", line_height, "Line height", None);
    planner.add("editor.ligatures", flag(font.get("USE_LIGATURES")).map(Value::from), "Enable ligatures", None);

    let settings = options(bundle.files.get("options/editor.xml"), "EditorSettings", None);
    match settings.get("STRIP_TRAILING_SPACES").map(String::as_str) {
        Some(strip @ ("Changed" | "Whole")) => {
            let whole = strip == "Whole";
            planner.plan.writes.push(("files.trimTrailingWhitespace".to_owned(), Value::Bool(true)));
            let shown = i18n.tx(if whole { "All lines" } else { "Modified lines" });
            let scope = Value::from(if whole { "all" } else { "modified" });
            planner.add("editor.general.stripTrailingScope", Some(scope), "Remove trailing spaces on:", Some(shown));
        }
        Some("None") => {
            planner.add("files.trimTrailingWhitespace", Some(Value::Bool(false)), "Remove trailing spaces on:", None);
        }
        _ => {}
    }
    let newline = flag(settings.get("IS_ENSURE_NEWLINE_AT_EOF")).map(Value::from);
    planner.add("files.ensureFinalNewline", newline, "Ensure every saved file ends with a line break", None);
    let sticky = flag(settings.get("SHOW_STICKY_LINES")).map(Value::from);
    planner.add("editor.stickyScroll", sticky, "Show sticky lines while scrolling", None);
    let whitespace = flag(settings.get("IS_WHITESPACES_SHOWN")).map(Value::from);
    planner.add("editor.showWhitespace", whitespace, "Show whitespaces", None);
    // A comma-separated list of places: MAIN_EDITOR, CONSOLE, PREVIEW.
    let wraps = settings.get("USE_SOFT_WRAPS").map(|places| Value::Bool(places.split(',').any(|p| p == "MAIN_EDITOR")));
    planner.add("editor.softWrap", wraps, "Soft-wrap files", None);

    let general = options(bundle.files.get("options/ide.general.xml"), "GeneralSettings", None);
    let confirm = flag(general.get("confirmExit")).map(Value::from);
    planner.add("general.confirmExit", confirm, "Confirm before exiting the IDE", None);

    for (prefix, label) in [
        ("keymaps/", "Keymap"),
        ("codestyles/", "Code Style"),
        ("templates/", "Live templates"),
        ("inspection/", "Inspections"),
        ("installed.txt", "Plugins"),
    ] {
        if bundle.entries.iter().any(|entry| entry.starts_with(prefix)) {
            planner.plan.skipped.push(i18n.tx(label));
        }
    }
    planner.plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn icls(name: &str) -> String {
        format!(
            r#"<scheme name="{name}" version="142" parent_scheme="Darcula"><attributes><option name="DEFAULT_KEYWORD"><value><option name="FOREGROUND" value="cc856b" /></value></option></attributes></scheme>"#
        )
    }

    fn component(name: &str, options: &[(&str, &str)]) -> String {
        let options: String = options.iter().map(|(k, v)| format!(r#"<option name="{k}" value="{v}" />"#)).collect();
        format!(r#"<application><component name="{name}">{options}</component></application>"#)
    }

    fn editor() -> EditorState {
        EditorState { font_size: Some(13.0), custom_schemes: json!({}), color_overrides: Map::new() }
    }

    fn bundle(entries: &[&str], files: Vec<(&str, String)>) -> JetBrainsSettings {
        JetBrainsSettings {
            entries: entries.iter().map(|&e| e.to_owned()).collect(),
            files: files.into_iter().map(|(name, text)| (name.to_owned(), text)).collect(),
        }
    }

    /// A WebStorm export, trimmed to what the import reads.
    fn webstorm() -> JetBrainsSettings {
        bundle(
            &[
                "IntelliJ IDEA Global Settings",
                "colors/_@user_Darcula.icls",
                "options/editor-font.xml",
                "options/colors.scheme.xml",
                "options/editor.xml",
                "options/ide.general.xml",
                "options/vcs.xml",
                "keymaps/macOS.xml",
                "codestyles/Default.xml",
                "inspection/Default.xml",
                "installed.txt",
            ],
            vec![
                ("colors/_@user_Darcula.icls", icls("_@user_Darcula")),
                (
                    "options/colors.scheme.xml",
                    r#"<application><component name="EditorColorsManagerImpl"><global_color_scheme name="_@user_Darcula" /></component></application>"#.to_owned(),
                ),
                (
                    "options/editor-font.xml",
                    component(
                        "DefaultFont",
                        &[("FONT_SIZE", "17"), ("FONT_FAMILY", "JetBrains Maple Mono"), ("USE_LIGATURES", "true")],
                    ),
                ),
                (
                    "options/editor.xml",
                    component(
                        "EditorSettings",
                        &[
                            ("USE_SOFT_WRAPS", "CONSOLE"),
                            ("STRIP_TRAILING_SPACES", "Whole"),
                            ("IS_ENSURE_NEWLINE_AT_EOF", "true"),
                            ("SHOW_STICKY_LINES", "false"),
                        ],
                    ),
                ),
                (
                    "options/ide.general.xml",
                    component("GeneralSettings", &[("showTipsOnStartup", "false"), ("confirmExit", "false")]),
                ),
            ],
        )
    }

    fn plan(bundle: &JetBrainsSettings, editor: &EditorState) -> JetBrainsImportPlan {
        plan_jetbrains_import(bundle, editor, || "test-id".to_owned(), &I18n::default())
    }

    fn writes(plan: &JetBrainsImportPlan) -> BTreeMap<String, Value> {
        plan.writes.iter().cloned().collect()
    }

    fn row(label: &str, value: &str) -> ImportRow {
        ImportRow { label: label.to_owned(), value: value.to_owned() }
    }

    // planJetBrainsImport › maps a WebStorm export to the settings it has, and lists what is left out
    #[test]
    fn maps_a_webstorm_export_to_the_settings_it_has_and_lists_what_is_left_out() {
        let plan = plan(&webstorm(), &editor());
        let writes = writes(&plan);
        let schemes = writes["editor.customSchemes"].as_object().expect("schemes").clone();
        let (id, scheme) = schemes.iter().next().expect("one scheme");
        assert_eq!((&scheme["name"], &scheme["parent"]), (&json!("Darcula"), &json!("dark")));
        let expected: BTreeMap<String, Value> = [
            ("editor.customSchemes", Value::Object(schemes.clone())),
            ("editor.colorScheme", json!(id)),
            ("editor.fontFamily", json!("JetBrains Maple Mono")),
            ("editor.fontSize", json!(17)),
            ("editor.ligatures", json!(true)),
            ("files.trimTrailingWhitespace", json!(true)),
            ("editor.general.stripTrailingScope", json!("all")),
            ("files.ensureFinalNewline", json!(true)),
            ("editor.stickyScroll", json!(false)),
            ("editor.softWrap", json!(false)),
            ("general.confirmExit", json!(false)),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
        assert_eq!(writes, expected);
        assert!(plan.rows.contains(&row("Color Scheme", "Darcula")));
        assert!(plan.rows.contains(&row("Size", "17")));
        assert!(plan.rows.contains(&row("Remove trailing spaces on:", "All lines")));
        assert_eq!(plan.skipped, ["Keymap", "Code Style", "Inspections", "Plugins"]);
        assert!(plan.invalid.is_empty());
    }

    // planJetBrainsImport › writes only the keys present, and derives the line height from IDEA's spacing
    #[test]
    fn writes_only_the_keys_present_and_derives_the_line_height_from_ideas_spacing() {
        let archive = bundle(
            &[],
            vec![
                ("options/editor-font.xml", component("DefaultFont", &[("LINE_SPACING", "1.5")])),
                (
                    "options/editor.xml",
                    component(
                        "EditorSettings",
                        &[("USE_SOFT_WRAPS", "MAIN_EDITOR,CONSOLE"), ("STRIP_TRAILING_SPACES", "None")],
                    ),
                ),
            ],
        );
        let plan = plan(&archive, &editor());
        assert_eq!(
            plan.writes,
            [
                ("editor.lineHeight".to_owned(), json!(20)),
                ("files.trimTrailingWhitespace".to_owned(), json!(false)),
                ("editor.softWrap".to_owned(), json!(true)),
            ]
        );
        assert!(plan.skipped.is_empty());
    }

    // planJetBrainsImport › re-importing replaces the earlier copy of a scheme and drops its edits
    #[test]
    fn re_importing_replaces_the_earlier_copy_of_a_scheme_and_drops_its_edits() {
        let first = writes(&plan(&webstorm(), &editor()));
        let customs = first["editor.customSchemes"].clone();
        let id = customs.as_object().and_then(|schemes| schemes.keys().next().cloned()).expect("an id");
        let mut overrides = Map::new();
        overrides.insert(id.clone(), json!({ "keyword": { "bold": true } }));
        overrides.insert("other".to_owned(), json!({}));
        let again = EditorState { custom_schemes: customs, color_overrides: overrides, ..editor() };
        let writes = writes(&plan_jetbrains_import(&webstorm(), &again, || "other-id".to_owned(), &I18n::default()));
        let keys: Vec<&String> = writes["editor.customSchemes"].as_object().expect("schemes").keys().collect();
        assert_eq!(keys, [&id]);
        assert_eq!(writes["editor.colorOverrides"], json!({ "other": {} }));
        assert_eq!(writes["editor.colorScheme"], json!(id));
    }

    // planJetBrainsImport › skips colour schemes that cannot be read
    #[test]
    fn skips_colour_schemes_that_cannot_be_read() {
        let plan = plan(&bundle(&[], vec![("colors/broken.icls", "<nope".to_owned())]), &editor());
        assert_eq!(plan.invalid, ["broken.icls"]);
        assert!(plan.writes.is_empty());
    }
}
