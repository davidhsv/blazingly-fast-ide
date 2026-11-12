//! The questions version control operations ask, as data: what a dialog shows and what it answers.
//!
//! An operation that needs an answer returns one of these requests and continues in a step that
//! takes the answer. Texts built from values are already in the UI language; fixed texts are the
//! English source text the dialog translates when it shows them.

/// A yes / no question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub title: String,
    pub message: String,
    pub ok_label: String,
    /// The action destroys something: shown with a warning icon.
    pub danger: bool,
}

impl ConfirmRequest {
    /// A question about a destructive action.
    pub fn danger(title: &str, message: String, ok_label: &str) -> Self {
        Self { title: title.to_owned(), message, ok_label: ok_label.to_owned(), danger: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptCheckbox {
    pub label: String,
    pub initial: bool,
}

/// A one-line text prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptRequest {
    pub title: String,
    pub label: Option<String>,
    pub initial: String,
    pub ok_label: String,
    /// The field holds a name or a revision: shown in the monospaced font.
    pub mono: bool,
    /// Show the prefilled value's problem (e.g. the branch exists) before it is edited.
    pub validate_initial: bool,
    /// Optional checkbox below the field.
    pub checkbox: Option<PromptCheckbox>,
}

/// What a text prompt was closed with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptResult {
    pub value: String,
    pub checked: bool,
    /// The user chose to overwrite the existing branch of that name.
    pub overwrite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskButton {
    pub id: &'static str,
    pub label: &'static str,
    pub primary: bool,
}

/// A message box with its own buttons; closing it otherwise answers `cancel`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskRequest {
    pub title: String,
    pub body: String,
    /// Shown with a warning icon.
    pub warning: bool,
    pub width: Option<u32>,
    /// Label of a "Don't ask again" style checkbox.
    pub checkbox: Option<String>,
    pub buttons: Vec<AskButton>,
}

/// The button a message box was closed with and the state of its checkbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskAnswer {
    pub button: String,
    pub checked: bool,
}

impl AskAnswer {
    pub fn new(button: &str, checked: bool) -> Self {
        Self { button: button.to_owned(), checked }
    }
}
