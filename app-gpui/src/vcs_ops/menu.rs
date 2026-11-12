//! Context menus of the version control views, as data.
//!
//! A row is a registered action (shown with the action's own title, icon and shortcut unless the
//! row overrides them) or a row of its own, whose `run` says what choosing it does: a command of
//! the menu's own vocabulary `C`, which the view carries out.

use serde_json::Value;

/// A row bound to a registered action.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionItem {
    pub action: &'static str,
    /// Argument the action is executed with.
    pub arg: Option<Value>,
    /// Replaces the action's title.
    pub label: Option<String>,
    pub disabled: bool,
}

/// A row with its own label.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuEntry<C> {
    pub label: String,
    pub icon: Option<&'static str>,
    /// Shortcut shown after the label.
    pub shortcut: Option<String>,
    pub disabled: bool,
    /// A destructive row.
    pub danger: bool,
    pub run: Option<C>,
    pub submenu: Vec<MenuItem<C>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem<C> {
    Separator,
    Action(ActionItem),
    Item(MenuEntry<C>),
}

impl<C> MenuItem<C> {
    pub fn action(action: &'static str) -> Self {
        Self::Action(ActionItem { action, arg: None, label: None, disabled: false })
    }

    pub fn action_with(action: &'static str, arg: Value) -> Self {
        Self::Action(ActionItem { action, arg: Some(arg), label: None, disabled: false })
    }

    /// A row that does `run` when chosen; without one it is shown disabled.
    pub fn entry(label: impl Into<String>, run: Option<C>) -> Self {
        Self::Item(MenuEntry {
            label: label.into(),
            icon: None,
            shortcut: None,
            disabled: run.is_none(),
            danger: false,
            run,
            submenu: Vec::new(),
        })
    }

    pub fn item(label: impl Into<String>, run: C) -> Self {
        Self::entry(label, Some(run))
    }

    pub fn submenu(label: impl Into<String>, submenu: Vec<MenuItem<C>>) -> Self {
        Self::Item(MenuEntry {
            label: label.into(),
            icon: None,
            shortcut: None,
            disabled: false,
            danger: false,
            run: None,
            submenu,
        })
    }

    /// The command of a row of its own.
    pub fn command(&self) -> Option<&C> {
        match self {
            Self::Item(entry) => entry.run.as_ref(),
            Self::Separator | Self::Action(_) => None,
        }
    }

    /// The argument of an action row.
    pub fn arg(&self) -> Option<&Value> {
        match self {
            Self::Action(item) => item.arg.as_ref(),
            Self::Separator | Self::Item(_) => None,
        }
    }

    /// Sets the row's label (for an action: instead of its title).
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        match &mut self {
            Self::Separator => {}
            Self::Action(item) => item.label = Some(label.into()),
            Self::Item(entry) => entry.label = label.into(),
        }
        self
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        if let Self::Item(entry) = &mut self {
            entry.icon = Some(icon);
        }
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        if let Self::Item(entry) = &mut self {
            entry.shortcut = Some(shortcut.into());
        }
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        match &mut self {
            Self::Separator => {}
            Self::Action(item) => item.disabled = disabled,
            Self::Item(entry) => entry.disabled = disabled,
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let Self::Item(entry) = &mut self {
            entry.danger = true;
        }
        self
    }

    /// Id of the action the row is bound to.
    pub fn action_id(&self) -> Option<&'static str> {
        match self {
            Self::Action(item) => Some(item.action),
            Self::Separator | Self::Item(_) => None,
        }
    }

    /// The row's own label (an action row's override).
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Separator => None,
            Self::Action(item) => item.label.as_deref(),
            Self::Item(entry) => Some(&entry.label),
        }
    }

    pub fn is_disabled(&self) -> bool {
        match self {
            Self::Separator => false,
            Self::Action(item) => item.disabled,
            Self::Item(entry) => entry.disabled,
        }
    }

    /// The same menu with every command passed through `f` (a menu embedded in another one's
    /// vocabulary).
    pub fn map<D, F: Fn(C) -> D>(self, f: &F) -> MenuItem<D> {
        match self {
            Self::Separator => MenuItem::Separator,
            Self::Action(item) => MenuItem::Action(item),
            Self::Item(entry) => MenuItem::Item(MenuEntry {
                label: entry.label,
                icon: entry.icon,
                shortcut: entry.shortcut,
                disabled: entry.disabled,
                danger: entry.danger,
                run: entry.run.map(f),
                submenu: entry.submenu.into_iter().map(|item| item.map(f)).collect(),
            }),
        }
    }
}
