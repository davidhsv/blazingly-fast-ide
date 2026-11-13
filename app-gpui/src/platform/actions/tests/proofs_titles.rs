use super::*;

/// Product names shown as they are in every language.
const PROPER_NAMES: [&str; 2] = ["GraphQL", "Vim"];

// src/platform/locales/actionTitles.test.ts › has a Chinese entry for every literal action title, including action
// factory arguments
/// Every action whose title is written as English text (not a catalog key, not a plugin's verbatim title) has a
/// Chinese title, so the Chinese UI never shows an English action name.
#[test]
fn has_a_chinese_entry_for_every_literal_action_title() {
    let mut zh = I18n::default();
    zh.set_locale(Some("zh"), "en");
    let titles: Vec<&str> = manifest().values().map(|meta| meta.title.as_str()).collect();
    // The titles the spec names to keep its scan honest are among those checked.
    assert!(titles.contains(&"Go to Declaration or Usages") && titles.contains(&"Next Result"), "{}", titles.len());
    let untranslated: Vec<String> = manifest()
        .values()
        .filter(|meta| meta.title_key.is_none() && !meta.verbatim && !PROPER_NAMES.contains(&meta.title.as_str()))
        .filter(|meta| meta.localized(&zh).title == meta.title)
        .map(|meta| format!("{}  {:?}", meta.id, meta.title))
        .collect();
    let count = untranslated.len();
    assert!(untranslated.is_empty(), "{count} titles have no Chinese entry:\n{}", untranslated.join("\n"));
}
