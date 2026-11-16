//! The file templates' actions and New › rows.
//!
//! - **New › <template>** (`templates.new.<id>`): one action per enabled template, registered again
//!   whenever the templates in effect change ([`sync_template_actions`]): a template turned off
//!   loses its action, a new one gets its own. Each opens the New File dialog for its template in
//!   the folder of its argument, else the active file's folder, else the project's root.
//! - **New from Template…** (`templates.newFromTemplate`): the dialog for the template its argument
//!   names (`templateId`), else for the template of its `extension` (one with the kind `kindId`
//!   preferred), else for a template of another feature's own (`template`), else the list of the
//!   enabled templates; `name` and `kindId` prefill the name and the kind.
//! - **The New menu's groups**: the enabled templates but File (the Project view's own row), each
//!   for the menu's folder, and From Template….

use super::Workbench;
use super::context_menus::{GroupFacts, GroupRow};
use super::plugin_contributions::static_action_id;
use super::services_window::on_workbench;
use super::template_files::{FileTemplateSet, NewFileRequest, templates_in_effect};
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::platform::i18n::{Localization, Param};
use crate::run_producers::context_run::{ContextMenu, MenuGroupContext};
use crate::templates::files_model::FileTemplate;
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use gpui_kit::{App, Context, Global, Window};
use serde_json::{Value, json};

pub const NEW_FROM_TEMPLATE: &str = "templates.newFromTemplate";
/// The start of the id of a template's own New action.
pub const NEW_TEMPLATE_PREFIX: &str = "templates.new.";
/// The template the Project view offers as its own File row.
const EMPTY_FILE: &str = "file";

/// The ids of the template actions registered now, to take back when the templates change.
#[derive(Default)]
struct TemplateActions {
    ids: Vec<String>,
}

impl Global for TemplateActions {}

/// Registers New from Template… and the action of each enabled template in effect.
pub(super) fn register_actions(cx: &mut App) {
    cx.default_global::<ActionRegistry>().handle(NEW_FROM_TEMPLATE, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.new_from_template(arg.as_ref(), window, cx))
    });
    let templates = cx.default_global::<FileTemplateSet>().templates.clone();
    sync_template_actions(&templates, cx);
}

/// Registers the actions of the enabled `templates` in place of the ones registered before.
pub fn sync_template_actions(templates: &[FileTemplate], cx: &mut App) {
    // The titles are built from the template's name: remembered here, the action list translates them.
    let i18n = &mut cx.default_global::<Localization>().0;
    for template in templates.iter().filter(|template| template.id != EMPTY_FILE) {
        i18n.tx_built(&["New ", "…"], &[Param::from(template.name.as_str())]);
    }
    let previous = std::mem::take(&mut cx.default_global::<TemplateActions>().ids);
    let ids = register_template_actions(cx.default_global::<ActionRegistry>(), &previous, templates);
    cx.default_global::<TemplateActions>().ids = ids;
}

/// Takes back the actions `previous` and registers one per enabled template; returns their ids.
pub fn register_template_actions(
    registry: &mut ActionRegistry,
    previous: &[String],
    templates: &[FileTemplate],
) -> Vec<String> {
    for id in previous {
        registry.unregister(id);
    }
    let enabled = templates.iter().filter(|template| template.enabled != Some(false));
    enabled
        .map(|template| {
            let action = new_file_action(template);
            let id = action.meta.id.clone();
            registry.register(action);
            id
        })
        .collect()
}

/// The words Find Action also matches a template's action by: its extensions and default name.
fn keywords(template: &FileTemplate) -> Vec<String> {
    let words = ["new", "create", "template"]
        .map(str::to_owned)
        .into_iter()
        .chain(std::iter::once(template.extension.clone()))
        .chain(template.kinds.iter().filter_map(|kind| kind.extension.clone()))
        .chain(template.default_name.clone());
    let mut kept: Vec<String> = Vec::new();
    for word in words {
        if !word.is_empty() && !kept.contains(&word) {
            kept.push(word);
        }
    }
    kept
}

/// New › <template>: the New File dialog for `template`.
fn new_file_action(template: &FileTemplate) -> Action {
    let id = format!("{NEW_TEMPLATE_PREFIX}{}", template.id);
    let title =
        if template.id == EMPTY_FILE { "New Empty File…".to_owned() } else { format!("New {}…", template.name) };
    let mut meta = ActionMeta::new(&id, &title).category("File").when("projectOpen");
    meta.icon = Some(template.icon.clone()).filter(|icon| !icon.is_empty());
    meta.keywords = keywords(template);
    let template_id = template.id.clone();
    Action::new(meta, move |call, window, cx| {
        let arg = call.arg.cloned();
        let template_id = template_id.clone();
        on_workbench(window, cx, move |workbench, window, cx| {
            let Some(directory) = workbench.template_directory(arg.as_ref(), cx) else { return };
            let request = NewFileRequest { directory, template_id: Some(template_id), ..NewFileRequest::default() };
            workbench.open_new_file_dialog(request, window, cx);
        })
    })
}

/// The id of the enabled template for files of `extension`: one with the kind `kind_id` if there is
/// one, else the first.
pub(super) fn template_for_extension(
    templates: &[FileTemplate],
    extension: Option<&str>,
    kind_id: Option<&str>,
) -> Option<String> {
    let extension = extension.filter(|extension| !extension.is_empty())?;
    let matching: Vec<&FileTemplate> = templates
        .iter()
        .filter(|template| template.enabled != Some(false) && template.extension == extension)
        .collect();
    let with_kind =
        matching.iter().find(|template| template.kinds.iter().any(|kind| Some(kind.id.as_str()) == kind_id));
    with_kind.or(matching.first()).map(|template| template.id.clone())
}

/// A template another feature hands over in the argument: one with a name and at least one kind.
pub(super) fn ad_hoc_template(value: &Value) -> Option<FileTemplate> {
    let named = value.get("name").is_some_and(Value::is_string);
    let has_kinds = value.get("kinds").and_then(Value::as_array).is_some_and(|kinds| !kinds.is_empty());
    if !(named && has_kinds) {
        return None;
    }
    let mut object = value.as_object()?.clone();
    // A template that is not kept in the store may leave out its id and icon.
    for key in ["id", "icon"] {
        object.entry(key).or_insert_with(|| json!(""));
    }
    match serde_json::from_value(Value::Object(object)) {
        Ok(template) => Some(template),
        Err(error) => {
            tracing::debug!("the template handed to New from Template… is not one: {error}");
            None
        }
    }
}

/// The New menu's row of each enabled template but File, for the menu's folder.
pub(super) fn templates_new_group(_: ContextMenu, _: &MenuGroupContext<'_>, facts: &GroupFacts) -> Vec<GroupRow> {
    facts.templates_new.clone()
}

impl Workbench {
    /// The folder an action's argument names, else the active file's, else the project's root;
    /// none for an empty folder.
    fn template_directory(&self, arg: Option<&Value>, cx: &App) -> Option<String> {
        let directory = match arg.and_then(|arg| arg.get("directory")) {
            Some(Value::String(directory)) => directory.clone(),
            None | Some(Value::Null) => {
                let edited = self.active_file_path(cx).and_then(|path| {
                    std::path::Path::new(&path).parent().map(|parent| parent.to_string_lossy().into_owned())
                });
                edited.or_else(|| self.project_root(cx).map(|root| root.to_string_lossy().into_owned()))?
            }
            Some(_) => return None,
        };
        (!directory.is_empty()).then_some(directory)
    }

    /// New from Template…, for its argument.
    fn new_from_template(&mut self, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.template_directory(arg, cx) else { return };
        let text = |key: &str| arg.and_then(|arg| arg.get(key)).and_then(Value::as_str).map(str::to_owned);
        let (name, kind_id) = (text("name"), text("kindId"));
        if let Some(template) = arg.and_then(|arg| arg.get("template")).and_then(ad_hoc_template) {
            let request = NewFileRequest { directory, template: Some(template), name, kind_id, template_id: None };
            return self.open_new_file_dialog(request, window, cx);
        }
        let template_id = text("templateId").or_else(|| {
            let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
            let set = templates_in_effect(root.as_deref(), cx);
            template_for_extension(&set.templates, text("extension").as_deref(), kind_id.as_deref())
        });
        let request = NewFileRequest { directory, template_id, name, kind_id, template: None };
        self.open_new_file_dialog(request, window, cx);
    }

    /// The rows of the `templates.new` group for `ctx` (the folder New creates in).
    pub(super) fn templates_new_rows(&self, ctx: &MenuGroupContext<'_>, cx: &mut App) -> Vec<GroupRow> {
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let arg = ctx.paths.first().map(|directory| json!({ "directory": directory }));
        let templates = templates_in_effect(root.as_deref(), cx).enabled();
        templates
            .into_iter()
            .filter(|template| template.id != EMPTY_FILE)
            .map(|template| {
                let action = static_action_id(&format!("{NEW_TEMPLATE_PREFIX}{}", template.id));
                MenuItem::Action(ActionItem { action, arg: arg.clone(), label: Some(template.name), disabled: false })
            })
            .collect()
    }
}
