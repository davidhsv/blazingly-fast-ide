//! Run configuration types: what a kind of configuration (Shell Script, npm, Application…) is called,
//! which form fields edit it and how those fields become the launched command line.
//!
//! A type is declarative: its fields write [`RunConfig`]'s `params`, and [`RunConfigType::command_line`]
//! turns them into what is launched. [`RunConfigTypes`] is the registry features contribute types to.

use crate::platform::collate;
use ide_run::RunConfig;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Values of a type's form fields, by field key (`RunConfig.params`).
pub type Params = BTreeMap<String, String>;

/// What a type reads from its surroundings when it builds a command line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeEnv {
    /// The host that runs the process is Windows (wrapper scripts, path quoting).
    pub windows: bool,
    /// Settings › Build Tools › Gradle uses a local installation: new Gradle configurations do not
    /// use the wrapper.
    pub gradle_local: bool,
}

pub type CommandLineFn = Arc<dyn Fn(&Params, &TypeEnv) -> Option<CommandLine> + Send + Sync>;
pub type ParamsFromFn = Arc<dyn Fn(&RunConfig) -> Option<Params> + Send + Sync>;
pub type SuggestionsFn = Arc<dyn Fn(&[RunConfig], &Params) -> Vec<String> + Send + Sync>;
pub type VisibleFn = Arc<dyn Fn(&Params) -> bool + Send + Sync>;
pub type ValueFn = Arc<dyn Fn(&Params) -> String + Send + Sync>;
pub type ValidateFn = Arc<dyn Fn(&str, &Params) -> Option<String> + Send + Sync>;
pub type BrowseFn = Arc<dyn Fn(&Params) -> BrowseKind + Send + Sync>;

/// The control a field is edited with. `Checkbox` values are `"true"` or absent; `Output` is read-only
/// text computed by the field's `value` (not stored).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FieldControl {
    #[default]
    Text,
    Textarea,
    Select,
    Checkbox,
    Output,
}

/// One choice of a `Select` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldOption {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

impl FieldOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into(), detail: None }
    }
}

/// What the button next to a path field picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseKind {
    File,
    Directory,
}

/// A path field's picker: always the same kind, or one that follows the other fields.
#[derive(Clone)]
pub enum Browse {
    Fixed(BrowseKind),
    ByParams(BrowseFn),
}

impl Browse {
    pub fn kind(&self, params: &Params) -> BrowseKind {
        match self {
            Self::Fixed(kind) => *kind,
            Self::ByParams(pick) => pick(params),
        }
    }
}

/// Further options of a `Select`, read from the backend when the form shows the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsSource {
    /// The installed JDKs (value: the JDK home).
    InstalledJdks,
    /// The build model's modules (value: the module directory).
    BuildModules,
}

/// A form field of a run configuration type.
#[derive(Clone)]
pub struct RunConfigTypeField {
    /// Key in `RunConfig.params`.
    pub key: String,
    pub label: String,
    pub control: FieldControl,
    pub mono: bool,
    /// Visible lines of a `Textarea` (default 4).
    pub rows: Option<u32>,
    pub placeholder: Option<String>,
    pub options: Vec<FieldOption>,
    pub load_options: Option<OptionsSource>,
    /// Values offered while typing (e.g. the scripts of detected configurations).
    pub suggestions: Option<SuggestionsFn>,
    pub required: bool,
    pub visible: Option<VisibleFn>,
    /// An `Output` field's text.
    pub value: Option<ValueFn>,
    /// Why a non-empty value is invalid, or `None`.
    pub validate: Option<ValidateFn>,
    /// Shown under the control.
    pub hint: Option<String>,
    pub browse: Option<Browse>,
}

impl RunConfigTypeField {
    /// A plain text field.
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            control: FieldControl::Text,
            mono: false,
            rows: None,
            placeholder: None,
            options: Vec::new(),
            load_options: None,
            suggestions: None,
            required: false,
            visible: None,
            value: None,
            validate: None,
            hint: None,
            browse: None,
        }
    }

    pub fn control(mut self, control: FieldControl) -> Self {
        self.control = control;
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn rows(mut self, rows: u32) -> Self {
        self.rows = Some(rows);
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Makes the field a `Select` over `options`.
    pub fn select(mut self, options: Vec<FieldOption>) -> Self {
        self.control = FieldControl::Select;
        self.options = options;
        self
    }

    pub fn load_options(mut self, source: OptionsSource) -> Self {
        self.load_options = Some(source);
        self
    }

    pub fn suggestions(
        mut self,
        suggest: impl Fn(&[RunConfig], &Params) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.suggestions = Some(Arc::new(suggest));
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn visible(mut self, visible: impl Fn(&Params) -> bool + Send + Sync + 'static) -> Self {
        self.visible = Some(Arc::new(visible));
        self
    }

    /// Makes the field an `Output` showing `value`.
    pub fn output(mut self, value: impl Fn(&Params) -> String + Send + Sync + 'static) -> Self {
        self.control = FieldControl::Output;
        self.value = Some(Arc::new(value));
        self
    }

    pub fn validate(mut self, validate: impl Fn(&str, &Params) -> Option<String> + Send + Sync + 'static) -> Self {
        self.validate = Some(Arc::new(validate));
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn browse(mut self, kind: BrowseKind) -> Self {
        self.browse = Some(Browse::Fixed(kind));
        self
    }

    pub fn browse_by(mut self, pick: impl Fn(&Params) -> BrowseKind + Send + Sync + 'static) -> Self {
        self.browse = Some(Browse::ByParams(Arc::new(pick)));
        self
    }

    /// Whether the form shows the field for `params` (a field without a condition always shows).
    pub fn is_visible(&self, params: &Params) -> bool {
        self.visible.as_ref().is_none_or(|visible| visible(params))
    }
}

impl fmt::Debug for RunConfigTypeField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunConfigTypeField").field("key", &self.key).field("control", &self.control).finish()
    }
}

/// What a type launches for a set of params.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    pub command: String,
    pub args: Vec<String>,
    /// The working directory, for types that own it.
    pub cwd: Option<String>,
    /// A shell command line run on Stop before the process is signalled (`docker stop …`).
    pub stop: Option<String>,
}

impl CommandLine {
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self { command: command.into(), args, cwd: None, stop: None }
    }
}

/// A kind of run configuration.
#[derive(Clone)]
pub struct RunConfigType {
    /// Stored as `RunConfig.kind`.
    pub id: String,
    pub label: String,
    pub icon: String,
    pub description: Option<String>,
    /// Parent node in "Add New Configuration" (e.g. `Docker`).
    pub group: Option<String>,
    /// Params of a new configuration (before templates apply).
    pub defaults: Params,
    pub fields: Vec<RunConfigTypeField>,
    /// What is launched for the params; `None` when the type runs no process itself (Compound).
    pub command_line: CommandLineFn,
    /// Params that reproduce a configuration made elsewhere (detection, older files); `None` when it
    /// does not fit.
    pub params_from: Option<ParamsFromFn>,
    /// The type's fields set the working directory (no separate field).
    pub owns_working_directory: bool,
    /// "Emulate terminal in output console" applies.
    pub terminal: bool,
    /// The type adjusts what is launched right before it starts (through the backend).
    pub prepares_launch: bool,
    /// Only Debug applies: the configuration attaches a debugger and launches no process.
    pub debug_only: bool,
    /// `RunConfig.debug_adapter` of a new configuration.
    pub debug_adapter: Option<String>,
}

impl RunConfigType {
    /// A type without fields whose command line is `command_line`.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        icon: impl Into<String>,
        command_line: impl Fn(&Params, &TypeEnv) -> Option<CommandLine> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            description: None,
            group: None,
            defaults: Params::new(),
            fields: Vec::new(),
            command_line: Arc::new(command_line),
            params_from: None,
            owns_working_directory: false,
            terminal: true,
            prepares_launch: false,
            debug_only: false,
            debug_adapter: None,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn defaults(mut self, defaults: &[(&str, &str)]) -> Self {
        self.defaults = defaults.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        self
    }

    pub fn fields(mut self, fields: Vec<RunConfigTypeField>) -> Self {
        self.fields = fields;
        self
    }

    pub fn params_from(mut self, derive: impl Fn(&RunConfig) -> Option<Params> + Send + Sync + 'static) -> Self {
        self.params_from = Some(Arc::new(derive));
        self
    }

    pub fn owns_working_directory(mut self) -> Self {
        self.owns_working_directory = true;
        self
    }

    pub fn no_terminal(mut self) -> Self {
        self.terminal = false;
        self
    }

    pub fn prepares_launch(mut self) -> Self {
        self.prepares_launch = true;
        self
    }

    pub fn debug_only(mut self) -> Self {
        self.debug_only = true;
        self
    }

    pub fn debug_adapter(mut self, adapter: impl Into<String>) -> Self {
        self.debug_adapter = Some(adapter.into());
        self
    }

    /// The command line of `params`.
    pub fn line(&self, params: &Params, env: &TypeEnv) -> Option<CommandLine> {
        (self.command_line)(params, env)
    }

    /// The params reproducing `config`, when the type can derive them.
    pub fn derive_params(&self, config: &RunConfig) -> Option<Params> {
        self.params_from.as_ref().and_then(|derive| derive(config))
    }
}

impl fmt::Debug for RunConfigType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunConfigType").field("id", &self.id).field("label", &self.label).finish()
    }
}

/// The registered run configuration types, in registration order.
#[derive(Clone, Debug, Default)]
pub struct RunConfigTypes {
    types: Vec<Arc<RunConfigType>>,
}

impl RunConfigTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `ty`, replacing a type with the same id in place. The returned handle identifies
    /// this registration to [`Self::unregister`].
    pub fn register(&mut self, ty: RunConfigType) -> Arc<RunConfigType> {
        let ty = Arc::new(ty);
        match self.types.iter_mut().find(|existing| existing.id == ty.id) {
            Some(existing) => *existing = Arc::clone(&ty),
            None => self.types.push(Arc::clone(&ty)),
        }
        ty
    }

    /// Removes a registration, unless a later one replaced it.
    pub fn unregister(&mut self, registration: &Arc<RunConfigType>) {
        self.types.retain(|ty| !Arc::ptr_eq(ty, registration));
    }

    pub fn get(&self, kind: &str) -> Option<&RunConfigType> {
        self.types.iter().find(|ty| ty.id == kind).map(Arc::as_ref)
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.get(kind).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &RunConfigType> {
        self.types.iter().map(Arc::as_ref)
    }

    /// The registered types, by label.
    pub fn by_label(&self) -> Vec<&RunConfigType> {
        let mut types: Vec<&RunConfigType> = self.iter().collect();
        types.sort_by(|a, b| collate::compare(&a.label, &b.label));
        types
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(id: &str, label: &str) -> RunConfigType {
        RunConfigType::new(id, label, "server", |_, _| None)
    }

    // Not from a spec: a registration replaces the same id in place and only its own handle removes it.
    #[test]
    fn registering_replaces_in_place_and_unregisters_by_handle() {
        let mut types = RunConfigTypes::new();
        let first = types.register(fake("a", "Zeta"));
        types.register(fake("b", "Alpha"));
        let second = types.register(fake("a", "Beta"));
        assert_eq!(types.iter().map(|ty| ty.label.as_str()).collect::<Vec<_>>(), ["Beta", "Alpha"]);
        types.unregister(&first);
        assert!(types.contains("a"));
        types.unregister(&second);
        assert!(!types.contains("a"));
        assert_eq!(types.by_label().iter().map(|ty| ty.id.as_str()).collect::<Vec<_>>(), ["b"]);
    }

    // Not from a spec: a field without a visibility condition always shows; a browse kind may follow params.
    #[test]
    fn field_visibility_and_browse_kind_follow_params() {
        let mut params = Params::new();
        let always = RunConfigTypeField::new("a", "A");
        let file_mode = RunConfigTypeField::new("b", "B")
            .visible(|p| p.get("mode").is_some_and(|mode| mode == "file"))
            .browse_by(|p| if p.contains_key("war") { BrowseKind::File } else { BrowseKind::Directory });
        assert!(always.is_visible(&params));
        assert!(!file_mode.is_visible(&params));
        params.insert("mode".into(), "file".into());
        assert!(file_mode.is_visible(&params));
        assert_eq!(file_mode.browse.as_ref().map(|browse| browse.kind(&params)), Some(BrowseKind::Directory));
    }
}
