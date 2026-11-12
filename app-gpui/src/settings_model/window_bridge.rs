//! The Settings window and its project window.
//!
//! Settings live in a window of their own, labelled `settings-<parent>`, that only shows the
//! settings pages. The action registry stays with the project window: it sends the Settings
//! window a description of every action (the Keymap page, links that run actions) and runs the
//! actions the Settings window forwards.

use crate::platform::actions::ActionMeta;
use serde::{Deserialize, Serialize};

const SETTINGS_PREFIX: &str = "settings-";
/// The Settings window asks its parent for the actions.
pub const REQUEST_ACTIONS: &str = "openidea://settings/request-actions";
/// The parent sends the Settings window its actions ([`ActionInfo`]s).
pub const ACTIONS: &str = "openidea://settings/actions";
/// The Settings window asks its parent to run an action ([`ExecRequest`]).
pub const EXEC: &str = "openidea://settings/exec";

/// The label of the Settings window of the window `parent`.
pub fn settings_window_label(parent: &str) -> String {
    format!("{SETTINGS_PREFIX}{parent}")
}

/// The parent's label when `label` is a Settings window's.
pub fn settings_window_parent(label: Option<&str>) -> Option<&str> {
    label?.strip_prefix(SETTINGS_PREFIX)
}

/// An action as it is described to the Settings window: everything but what runs it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInfo {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// An action for the parent window to run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecRequest {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arg: Option<serde_json::Value>,
}

pub fn describe_actions<'a>(actions: impl IntoIterator<Item = &'a ActionMeta>) -> Vec<ActionInfo> {
    actions
        .into_iter()
        .map(|action| ActionInfo {
            id: action.id.clone(),
            title: action.title.clone(),
            category: action.category.clone(),
            description: action.description.clone(),
            icon: action.icon.clone(),
            when: action.when.clone(),
            keywords: action.keywords.clone(),
            hidden: action.hidden,
        })
        .collect()
}

/// The actions the Settings window mirrors as proxies that run in the parent: the parent's,
/// without those the Settings window has itself (`file.settings`), which win.
pub fn proxied_actions(infos: &[ActionInfo], is_local: impl Fn(&str) -> bool) -> Vec<&ActionInfo> {
    infos.iter().filter(|info| !is_local(&info.id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // settings window bridge › maps settings window labels to their parent
    #[test]
    fn maps_settings_window_labels_to_their_parent() {
        assert_eq!(settings_window_parent(Some("settings-main")), Some("main"));
        assert_eq!(settings_window_parent(Some("settings-project-2")), Some("project-2"));
        assert_eq!(settings_window_parent(Some("main")), None);
        assert_eq!(settings_window_parent(None), None);
    }

    // settings window bridge › describes actions without their handlers
    #[test]
    fn describes_actions_without_their_handlers() {
        let action = ActionMeta::new("a.b", "B").when("editorFocus");
        let infos = describe_actions([&action]);
        let expected = ActionInfo {
            id: "a.b".to_owned(),
            title: "B".to_owned(),
            when: Some("editorFocus".to_owned()),
            ..ActionInfo::default()
        };
        assert_eq!(infos, std::slice::from_ref(&expected));
        let sent = serde_json::to_value(&infos[0]).unwrap_or_default();
        assert_eq!(sent, json!({ "id": "a.b", "title": "B", "when": "editorFocus" }));
        assert_eq!(serde_json::from_value::<ActionInfo>(sent).ok(), Some(expected));
    }

    // Not from a spec: a Settings window is labelled after its parent, and only the parent's
    // actions it does not have itself are proxied.
    #[test]
    fn labels_the_window_and_proxies_what_is_not_local() {
        let label = settings_window_label("main");
        assert_eq!(settings_window_parent(Some(label.as_str())), Some("main"));
        let infos = describe_actions([&ActionMeta::new("file.settings", "Settings"), &ActionMeta::new("x.y", "Y")]);
        let proxied = proxied_actions(&infos, |id| id == "file.settings");
        assert_eq!(proxied.iter().map(|info| info.id.as_str()).collect::<Vec<_>>(), ["x.y"]);
        let request = ExecRequest { id: "x.y".to_owned(), arg: None };
        assert_eq!(serde_json::to_value(&request).unwrap_or_default(), json!({ "id": "x.y" }));
    }
}
