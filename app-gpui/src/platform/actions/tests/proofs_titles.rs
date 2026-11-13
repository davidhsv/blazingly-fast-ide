use super::*;

fn manifest_actions() -> Vec<serde_json::Value> {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../assets/gen/actions.json")).expect("valid manifest");
    manifest["actions"].as_array().expect("an action list").clone()
}

/// Product names shown as they are in every language.
const PROPER_NAMES: [&str; 2] = ["GraphQL", "Vim"];

/// Every literal title of the manifest, with the id of the action it names: the title, plus each title the same
/// action factory gives the other actions it makes (`titleVariants`). Catalog-key, plugin and template-built titles
/// are left out.
fn literal_titles() -> Vec<(String, String)> {
    let mut titles = Vec::new();
    for action in manifest_actions() {
        let keyed = action.get("titleKey").is_some_and(|key| !key.is_null());
        if keyed || action["verbatim"] == true || action.get("titleTemplate").is_some() {
            continue;
        }
        let id = action["id"].as_str().unwrap_or_default().to_owned();
        let variants = action.get("titleVariants").and_then(|v| v.as_array()).into_iter().flatten();
        for title in std::iter::once(&action["title"]).chain(variants).filter_map(|t| t.as_str()) {
            if !title.is_empty() {
                titles.push((id.clone(), title.to_owned()));
            }
        }
    }
    titles
}

// src/platform/locales/actionTitles.test.ts › has a Chinese entry for every literal action title, including action
// factory arguments
/// Every action whose title is written as English text (not a catalog key, not a plugin's verbatim title) has a
/// Chinese title, so the Chinese UI never shows an English action name.
#[test]
fn has_a_chinese_entry_for_every_literal_action_title() {
    let mut zh = I18n::default();
    zh.set_locale(Some("zh"), "en");
    let titles = literal_titles();
    // The titles the spec names to keep its scan honest are among those checked.
    for named in ["Go to Declaration or Usages", "Next Result"] {
        assert!(titles.iter().any(|(_, title)| title == named), "{named} is scanned");
    }
    let mut untranslated: Vec<String> = titles
        .iter()
        .filter(|(_, title)| !PROPER_NAMES.contains(&title.as_str()) && zh.tx(title) == *title)
        .map(|(id, title)| format!("{id}  {title:?}"))
        .collect();
    untranslated.dedup();
    let count = untranslated.len();
    assert!(untranslated.is_empty(), "{count} titles have no Chinese entry:\n{}", untranslated.join("\n"));
}

/// A title built from a template (`Project View: {label}`) translates through the catalog's template, so the
/// Chinese UI does not show it as the English text.
#[test]
fn translates_every_action_title_built_from_a_template() {
    let mut zh = I18n::default();
    zh.set_locale(Some("zh"), "en");
    let built: Vec<(String, String)> = manifest_actions()
        .iter()
        .filter(|action| action.get("titleTemplate").is_some())
        .map(|action| {
            (
                action["id"].as_str().unwrap_or_default().to_owned(),
                action["title"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert!(built.iter().any(|(id, _)| id == "project.option.compact"), "template-built titles are scanned");
    let untranslated: Vec<String> = built
        .iter()
        .filter(|(_, title)| zh.tx_template(title) == *title)
        .map(|(id, title)| format!("{id}  {title:?}"))
        .collect();
    assert!(
        untranslated.is_empty(),
        "{} titles have no Chinese entry:\n{}",
        untranslated.len(),
        untranslated.join("\n")
    );
}
