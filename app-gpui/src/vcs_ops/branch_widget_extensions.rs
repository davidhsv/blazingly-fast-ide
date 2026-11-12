//! Extensions of the branch widget and its Branches popup contributed by other features:
//!
//! - a presentation of the current branch (icon, text, tooltip) replacing the widget's own, as a
//!   pull request integration shows "#12 on feature" for a pull request's branch;
//! - actions added to the popup's top, after its own (a separator before each extension's).
//!
//! An extension is provided under an id and replaced by providing it again (it follows its owner's
//! state); the handle it was provided with removes it.

use crate::vcs::branches::PopupAction;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchPresentation {
    /// Replaces the branch icon.
    pub icon: Option<String>,
    pub text: String,
    /// Shown above the widget's own tooltip lines.
    pub tooltip: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BranchWidgetExtension {
    pub id: String,
    /// Lower first (presentation: the first one wins). 100 when `None`.
    pub order: Option<i32>,
    pub presentation: Option<BranchPresentation>,
    pub actions: Vec<PopupAction>,
}

const DEFAULT_ORDER: i32 = 100;

/// What an extension was provided with: removes that very registration, not a later one of the
/// same id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionHandle {
    id: String,
    token: u64,
}

#[derive(Debug, Default)]
pub struct BranchWidgetExtensions {
    /// In the order the ids were first provided.
    entries: Vec<(u64, BranchWidgetExtension)>,
    tokens: u64,
}

impl BranchWidgetExtensions {
    /// Provides (or replaces) extension `extension.id`.
    pub fn provide(&mut self, extension: BranchWidgetExtension) -> ExtensionHandle {
        self.tokens += 1;
        let handle = ExtensionHandle { id: extension.id.clone(), token: self.tokens };
        match self.entries.iter_mut().find(|(_, known)| known.id == extension.id) {
            Some(entry) => *entry = (handle.token, extension),
            None => self.entries.push((handle.token, extension)),
        }
        handle
    }

    /// Removes the extension `handle` was provided with, unless it was replaced since. Returns
    /// whether it was removed.
    pub fn dispose(&mut self, handle: &ExtensionHandle) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(token, extension)| *token != handle.token || extension.id != handle.id);
        self.entries.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&BranchWidgetExtension> {
        self.entries.iter().map(|(_, extension)| extension).find(|extension| extension.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn ordered(&self) -> Vec<&BranchWidgetExtension> {
        let mut out: Vec<&BranchWidgetExtension> = self.entries.iter().map(|(_, extension)| extension).collect();
        out.sort_by_key(|extension| extension.order.unwrap_or(DEFAULT_ORDER));
        out
    }

    /// The presentation replacing the widget's, if any.
    pub fn branch_presentation(&self) -> Option<&BranchPresentation> {
        self.ordered().into_iter().find_map(|extension| extension.presentation.as_ref())
    }

    /// The extensions' popup actions, each extension in a group of its own.
    pub fn popup_actions(&self) -> Vec<PopupAction> {
        self.ordered()
            .into_iter()
            .flat_map(|extension| {
                let group = format!("ext:{}", extension.id);
                extension.actions.iter().map(move |action| PopupAction { group: Some(group.clone()), ..action.clone() })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, label: &str, checked: Option<bool>) -> PopupAction {
        PopupAction { id: id.to_owned(), label: label.to_owned(), checked, ..PopupAction::default() }
    }

    // src/features/vcs/__tests__/branchWidgetExtensions.test.ts › branch widget extensions › presents the branch with the first extension's presentation and groups the popup actions
    #[test]
    fn presents_the_branch_with_the_first_extensions_presentation_and_groups_the_popup_actions() {
        let mut extensions = BranchWidgetExtensions::default();
        let a = extensions.provide(BranchWidgetExtension {
            id: "a".to_owned(),
            order: Some(200),
            actions: vec![action("x", "X", None)],
            ..BranchWidgetExtension::default()
        });
        let presentation = BranchPresentation {
            text: "#1 on main".to_owned(),
            icon: Some("github-warning".to_owned()),
            tooltip: None,
        };
        let b = extensions.provide(BranchWidgetExtension {
            id: "b".to_owned(),
            presentation: Some(presentation.clone()),
            actions: vec![action("y", "Y", None), action("z", "Z", Some(true))],
            ..BranchWidgetExtension::default()
        });
        assert_eq!(extensions.branch_presentation(), Some(&presentation));
        let actions = extensions.popup_actions();
        let grouped: Vec<(&str, Option<&str>)> =
            actions.iter().map(|action| (action.id.as_str(), action.group.as_deref())).collect();
        assert_eq!(grouped, [("y", Some("ext:b")), ("z", Some("ext:b")), ("x", Some("ext:a"))]);
        // Providing again replaces; a stale handle leaves the replacement.
        let b2 = extensions.provide(BranchWidgetExtension { id: "b".to_owned(), ..BranchWidgetExtension::default() });
        assert!(!extensions.dispose(&b));
        assert!(extensions.get("b").is_some());
        assert_eq!(extensions.branch_presentation(), None);
        assert!(extensions.dispose(&b2));
        assert!(extensions.dispose(&a));
        assert_eq!(extensions.len(), 0);
        assert!(extensions.is_empty());
    }
}
