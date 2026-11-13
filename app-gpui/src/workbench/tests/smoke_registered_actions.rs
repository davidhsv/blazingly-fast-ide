use super::*;
use crate::platform::actions::ActionRegistry;

/// The ids the manifest's dynamic entries stand for, by the source line the manifest names: an id
/// matches an entry when it starts with the prefix and the rest is a non-empty `rest` of the entry's
/// kind. The table must name exactly the manifest's dynamic entries, so an entry added to the manifest
/// fails here until its rule is written.
const DYNAMIC: &[(&str, &str, Rest)] = &[
    ("src/editor/core/actions.ts:796", "code.expandToLevel", Rest::Digits),
    ("src/editor/core/actions.ts:800", "code.expandAllToLevel", Rest::Digits),
    ("src/features/macros/index.ts:101", "macros.play.", Rest::Any),
    ("src/features/plugins/points.tsx:199", "plugin.", Rest::Dotted),
    ("src/features/plugins/views/serviceProvider.tsx:88", "", Rest::NulSeparated),
    ("src/features/run/externalTools.ts:164", "externalTools.", Rest::Any),
    ("src/features/search/index.ts:476", "navigate.toggleBookmark", Rest::Digit),
    ("src/features/search/index.ts:486", "navigate.gotoBookmark", Rest::Digit),
    ("src/features/templates/index.ts:130", "templates.new.", Rest::Any),
    ("src/workbench/toolwindows/actions.ts:160", "toolwindow.", Rest::Any),
    ("src/workbench/toolwindows/actions.ts:247", "toolwindow.resize", Rest::Direction),
];

#[derive(Clone, Copy, Debug)]
enum Rest {
    /// Any non-empty text.
    Any,
    /// One digit.
    Digit,
    /// One or more digits.
    Digits,
    /// `<plugin>.<command>`, both non-empty.
    Dotted,
    /// `<item>\0<button>`, both non-empty.
    NulSeparated,
    /// `Left`, `Right`, `Up` or `Down`.
    Direction,
}

impl Rest {
    fn matches(self, rest: &str) -> bool {
        match self {
            Rest::Any => !rest.is_empty(),
            Rest::Digit => rest.len() == 1 && rest.chars().all(|c| c.is_ascii_digit()),
            Rest::Digits => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
            Rest::Dotted => {
                rest.split_once('.').is_some_and(|(plugin, command)| !plugin.is_empty() && !command.is_empty())
            }
            Rest::NulSeparated => {
                rest.split_once('\0').is_some_and(|(item, button)| !item.is_empty() && !button.is_empty())
            }
            Rest::Direction => ["Left", "Right", "Up", "Down"].contains(&rest),
        }
    }
}

/// Every action registered once a project window has opened a Git repository is an id of
/// `assets/gen/actions.json` or one its dynamic entries stand for; the sorted ids are printed.
#[gpui_kit::test]
async fn every_action_registered_in_a_project_window_is_known_to_the_manifest(cx: &mut TestAppContext) {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/gen/actions.json")).expect("the manifest is valid JSON");
    let known: std::collections::HashSet<&str> = manifest["actions"]
        .as_array()
        .expect("the manifest lists actions")
        .iter()
        .filter_map(|action| action["id"].as_str())
        .collect();
    let mut sources: Vec<&str> = manifest["dynamic"]
        .as_array()
        .expect("the manifest lists dynamic entries")
        .iter()
        .filter_map(|entry| entry["source"].as_str())
        .collect();
    sources.sort_unstable();
    let mut ruled: Vec<&str> = DYNAMIC.iter().map(|(source, _, _)| *source).collect();
    ruled.sort_unstable();
    assert_eq!(ruled, sources, "every dynamic entry of the manifest has its rule here, and no other");

    let (_workbench, cx, _folders, _root) = git_project(cx, init_repo);
    let mut ids: Vec<String> = cx.update(|_, cx| cx.global::<ActionRegistry>().ids().map(str::to_owned).collect());
    ids.sort_unstable();
    for id in &ids {
        println!("{}", id.replace('\0', "\\0"));
    }
    let unknown: Vec<&String> = ids
        .iter()
        .filter(|id| !known.contains(id.as_str()))
        .filter(|id| {
            !DYNAMIC.iter().any(|(_, prefix, rest)| id.strip_prefix(prefix).is_some_and(|tail| rest.matches(tail)))
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "{} of {} registered ids are unknown to the manifest: {unknown:?}",
        unknown.len(),
        ids.len()
    );
}
