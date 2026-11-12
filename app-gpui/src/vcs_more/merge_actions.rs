//! The merge editor's commands as actions: they stay registered while no merge editor is open, are
//! enabled through context keys the focused editor sets, and run in that editor.

use crate::platform::actions::{ActionMeta, is_action_enabled};
use crate::platform::context::ContextKeys;

/// Context key set while a merge editor has focus.
pub const MERGE_EDITOR_FOCUS: &str = "mergeEditorFocus";

/// A command of the merge editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergeCommand {
    ApplyLeft,
    ApplyAll,
    ApplyRight,
    ResolveSimple,
    Revert,
    AcceptYours,
    AcceptTheirs,
    Apply,
    Cancel,
    Reload,
    Compare,
    Settings,
    AutoApply,
    CompareYoursResult,
    CompareResultTheirs,
    CompareYoursTheirs,
    CompareBaseYours,
    CompareBaseResult,
    CompareBaseTheirs,
    Accept,
    ResolveUsingYours,
    ResolveUsingTheirs,
    Ignore,
    ResolveAutomatically,
    Annotate,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Change,
}

impl MergeCommand {
    pub const ALL: [Self; 30] = [
        Self::ApplyLeft,
        Self::ApplyAll,
        Self::ApplyRight,
        Self::ResolveSimple,
        Self::Revert,
        Self::AcceptYours,
        Self::AcceptTheirs,
        Self::Apply,
        Self::Cancel,
        Self::Reload,
        Self::Compare,
        Self::Settings,
        Self::AutoApply,
        Self::CompareYoursResult,
        Self::CompareResultTheirs,
        Self::CompareYoursTheirs,
        Self::CompareBaseYours,
        Self::CompareBaseResult,
        Self::CompareBaseTheirs,
        Self::Accept,
        Self::ResolveUsingYours,
        Self::ResolveUsingTheirs,
        Self::Ignore,
        Self::ResolveAutomatically,
        Self::Annotate,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::SelectAll,
        Self::Change,
    ];

    /// The command's name, English title and icon.
    const fn spec(self) -> (&'static str, &'static str, Option<&'static str>) {
        match self {
            Self::ApplyLeft => {
                ("applyLeft", "Apply Non-Conflicting Changes from the Left Side", Some("diff-apply-left"))
            }
            Self::ApplyAll => ("applyAll", "Apply All Non-Conflicting Changes", Some("diff-apply-all")),
            Self::ApplyRight => {
                ("applyRight", "Apply Non-Conflicting Changes from the Right Side", Some("diff-apply-right"))
            }
            Self::ResolveSimple => ("resolveSimple", "Resolve Simple Conflicts", Some("diff-magic-toolbar")),
            Self::Revert => ("revert", "Revert Conflict Resolution", Some("diff-revert")),
            Self::AcceptYours => ("acceptYours", "Accept Yours", None),
            Self::AcceptTheirs => ("acceptTheirs", "Accept Theirs", None),
            Self::Apply => ("apply", "Apply Merge Result", None),
            Self::Cancel => ("cancel", "Cancel Merge", None),
            Self::Reload => ("reload", "Reload from Disk", Some("refresh")),
            Self::Compare => ("compare", "Compare Contents", Some("diff")),
            Self::Settings => ("settings", "Merge Settings", Some("settings")),
            Self::AutoApply => ("autoApply", "Automatically Apply Non-Conflicting Changes", None),
            Self::CompareYoursResult => ("compareYoursResult", "Yours and Result", None),
            Self::CompareResultTheirs => ("compareResultTheirs", "Result and Theirs", None),
            Self::CompareYoursTheirs => ("compareYoursTheirs", "Yours and Theirs", None),
            Self::CompareBaseYours => ("compareBaseYours", "Base and Yours", None),
            Self::CompareBaseResult => ("compareBaseResult", "Base and Result", None),
            Self::CompareBaseTheirs => ("compareBaseTheirs", "Base and Theirs", None),
            Self::Accept => ("accept", "Accept", None),
            Self::ResolveUsingYours => ("resolveUsingYours", "Resolve Using Yours", None),
            Self::ResolveUsingTheirs => ("resolveUsingTheirs", "Resolve Using Theirs", None),
            Self::Ignore => ("ignore", "Ignore", Some("diff-remove")),
            Self::ResolveAutomatically => ("resolveAutomatically", "Resolve Automatically", Some("diff-magic")),
            Self::Annotate => ("annotate", "Annotate with Git Blame", Some("changelist")),
            Self::Cut => ("cut", "Cut", None),
            Self::Copy => ("copy", "Copy", Some("copy")),
            Self::Paste => ("paste", "Paste", None),
            Self::SelectAll => ("selectAll", "Select All", None),
            Self::Change => ("change", "Resolve Merge Change", None),
        }
    }

    /// The command's name, the part of its action id after `merge.`.
    pub const fn name(self) -> &'static str {
        self.spec().0
    }

    /// Id of the command's action, which is also the context key that enables it.
    pub fn id(self) -> String {
        format!("merge.{}", self.name())
    }

    /// The command an action id names.
    pub fn from_id(id: &str) -> Option<Self> {
        let name = id.strip_prefix("merge.")?;
        Self::ALL.into_iter().find(|command| command.name() == name)
    }

    /// The command's action: enabled while a merge editor that has something to do with it is
    /// focused. Resolving one change takes an argument, so it is hidden from the action search.
    pub fn meta(self) -> ActionMeta {
        let (_, title, icon) = self.spec();
        let id = self.id();
        ActionMeta {
            when: Some(format!("{MERGE_EDITOR_FOCUS} && {id}")),
            category: Some("Merge".to_owned()),
            icon: icon.map(str::to_owned),
            keywords: vec!["merge".to_owned(), "conflict".to_owned()],
            hidden: self == Self::Change,
            toggle: self == Self::AutoApply,
            ..ActionMeta::new(&id, title)
        }
    }
}

/// The actions of every merge command.
pub fn merge_actions() -> Vec<ActionMeta> {
    MergeCommand::ALL.into_iter().map(MergeCommand::meta).collect()
}

/// The merge editor the commands target: the one that has focus, which also holds while it opens a
/// toolbar menu or the action search. `H` identifies an editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedMergeEditor<H> {
    focused: Option<H>,
}

impl<H> Default for FocusedMergeEditor<H> {
    fn default() -> Self {
        Self { focused: None }
    }
}

impl<H: PartialEq> FocusedMergeEditor<H> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn focused(&self) -> Option<&H> {
        self.focused.as_ref()
    }

    /// Writes the focus and each command's enablement to the context keys.
    fn publish(&self, enabled: impl Fn(MergeCommand) -> bool, keys: &mut ContextKeys) {
        keys.set(MERGE_EDITOR_FOCUS, self.focused.is_some());
        for command in MergeCommand::ALL {
            keys.set(&command.id(), self.focused.is_some() && enabled(command));
        }
    }

    /// `host` took focus; `enabled` says which commands it has something to do with.
    pub fn focus(&mut self, host: H, enabled: impl Fn(MergeCommand) -> bool, keys: &mut ContextKeys) {
        self.focused = Some(host);
        self.publish(enabled, keys);
    }

    /// No merge editor has focus: every command is disabled.
    pub fn blur(&mut self, keys: &mut ContextKeys) {
        self.focused = None;
        self.publish(|_| false, keys);
    }

    /// The enablement of `host`'s commands changed; nothing happens unless it is the focused one.
    pub fn refresh(&mut self, host: &H, enabled: impl Fn(MergeCommand) -> bool, keys: &mut ContextKeys) -> bool {
        let focused = self.focused.as_ref() == Some(host);
        if focused {
            self.publish(enabled, keys);
        }
        focused
    }

    /// `host` is gone; the focus stays where it is when another editor has it.
    pub fn clear(&mut self, host: &H, keys: &mut ContextKeys) -> bool {
        let focused = self.focused.as_ref() == Some(host);
        if focused {
            self.blur(keys);
        }
        focused
    }

    /// The editor that runs the action `id` and the command it runs; `None` when the action is not
    /// a merge command or is disabled in `keys`.
    pub fn target(&self, id: &str, keys: &ContextKeys) -> Option<(&H, MergeCommand)> {
        let command = MergeCommand::from_id(id)?;
        let host = self.focused.as_ref()?;
        is_action_enabled(&command.meta(), keys).then_some((host, command))
    }

    /// Whether Automatically Apply Non-Conflicting Changes shows as on: the focused editor's
    /// setting, off without one.
    pub fn auto_apply(&self, of: impl Fn(&H) -> bool) -> bool {
        self.focused.as_ref().is_some_and(of)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn enabled_ids(keys: &ContextKeys) -> Vec<String> {
        merge_actions().into_iter().filter(|meta| is_action_enabled(meta, keys)).map(|meta| meta.id).collect()
    }

    // src/features/vcs/__tests__/mergeActions.test.ts › keeps merge commands registered and disabled outside a merge editor
    #[test]
    fn keeps_merge_commands_registered_and_disabled_outside_a_merge_editor() {
        let mut keys = ContextKeys::new();
        let mut editors = FocusedMergeEditor::<u32>::new();
        editors.blur(&mut keys);
        let commands = merge_actions();
        assert_eq!(commands.len(), 30);
        assert!(commands.iter().all(|meta| meta.id.starts_with("merge.")));
        assert!(commands.iter().filter(|meta| !meta.hidden).all(|meta| meta.keywords.contains(&"merge".to_owned())));
        assert!(enabled_ids(&keys).is_empty());
        assert_eq!(editors.target("merge.acceptYours", &keys), None);
    }

    // src/features/vcs/__tests__/mergeActions.test.ts › routes only to the focused editor and preserves it when another editor unmounts
    #[test]
    fn routes_only_to_the_focused_editor_and_preserves_it_when_another_editor_unmounts() {
        let mut keys = ContextKeys::new();
        let mut editors = FocusedMergeEditor::new();
        let (first, second) = (1_u32, 2_u32);
        editors.focus(first, |_| true, &mut keys);
        assert_eq!(editors.target("merge.applyAll", &keys), Some((&first, MergeCommand::ApplyAll)));
        editors.focus(second, |_| true, &mut keys);
        assert!(!editors.clear(&first, &mut keys));
        assert_eq!(editors.target("merge.revert", &keys), Some((&second, MergeCommand::Revert)));
        assert!(editors.clear(&second, &mut keys));
        assert_eq!(editors.target("merge.revert", &keys), None);
    }

    // src/features/vcs/__tests__/mergeActions.test.ts › shares changing enablement and toggle state with action menus
    #[test]
    fn shares_changing_enablement_and_toggle_state_with_action_menus() {
        let enabled = Cell::new(true);
        let checked = Cell::new(false);
        let editor = 7_u32;
        let mut keys = ContextKeys::new();
        let mut editors = FocusedMergeEditor::new();
        editors.focus(editor, |_| enabled.get(), &mut keys);
        // A menu row of the action is disabled when the action is, and checked when it is toggled on.
        let auto_apply = MergeCommand::AutoApply.meta();
        assert!(auto_apply.toggle);
        assert!(is_action_enabled(&auto_apply, &keys));
        assert!(!editors.auto_apply(|_| checked.get()));
        checked.set(true);
        assert!(editors.auto_apply(|_| checked.get()));
        enabled.set(false);
        assert!(editors.refresh(&editor, |_| enabled.get(), &mut keys));
        assert!(!is_action_enabled(&auto_apply, &keys));
        assert!(!is_action_enabled(&MergeCommand::ResolveSimple.meta(), &keys));
        assert_eq!(editors.target("merge.resolveSimple", &keys), None);
    }
}
