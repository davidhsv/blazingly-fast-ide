//! The file templates' actions and New › rows.
//!
//! - **New › <template>** (`templates.new.<id>`): one action per enabled template, registered again
//!   whenever the stored templates change or a window opens another project ([`sync_template_actions`]):
//!   a template turned off loses its action, a new one gets its own. The registry is the process's, so
//!   the actions are those of the templates any window has in effect; each opens the New File dialog
//!   for its template, as the window it runs in has it, in the folder of its argument, else the active
//!   file's folder, else the project's root.
//! - **New from Template…** (`templates.newFromTemplate`): the dialog for the template its argument
//!   names (`templateId`), else for the template of its `extension` (one with the kind `kindId`
//!   preferred), else for a template of another feature's own (`template`), else the list of the
//!   enabled templates; `name` and `kindId` prefill the name and the kind.
//! - **The New menu's groups**: the enabled templates of the window but File (the Project view's own
//!   row), each for the menu's folder, then From Template…. A template of a language belongs to the
//!   project types of that language and is left out of the New menu of a project of another type; the
//!   Tomcat context descriptor is offered only where a Tomcat run configuration runs from the project.

use super::context_menus::{GroupFacts, GroupRow};
use super::plugin_contributions::static_action_id;
use super::services_window::on_workbench;
use super::template_files::{NewFileRequest, templates_in_effect};
use super::{Workbench, Workbenches};
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::platform::i18n::{Localization, Param};
use crate::platform::storage::UiStorage;
use crate::project_view::project_type::{ProjectType, project_type_of};
use crate::run_producers::context_run::{ContextMenu, MenuGroupContext};
use crate::templates::files_model::FileTemplate;
use crate::templates::files_store::{FILE_TEMPLATES_KEY, project_key, scheme_key};
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use gpui_kit::{App, Context, Global, Window};
use serde_json::{Value, json};
use std::path::Path;

pub const NEW_FROM_TEMPLATE: &str = "templates.newFromTemplate";
/// The start of the id of a template's own New action.
pub const NEW_TEMPLATE_PREFIX: &str = "templates.new.";
/// The template the Project view offers as its own File row.
const EMPTY_FILE: &str = "file";

/// Per window root (`None`: the Default scheme), the stored texts its templates are read from.
type StoredTexts = Vec<(Option<String>, Vec<Option<String>>)>;

/// The template actions registered now: their ids, to take back when the templates change, and the
/// templates they were made from.
#[derive(Default)]
struct TemplateActions {
    ids: Vec<String>,
    /// The windows' roots and the stored texts the actions were made from.
    made_from: Option<StoredTexts>,
}

impl Global for TemplateActions {}

/// Registers New from Template… and the action of each enabled template in effect, and has the actions
/// follow the stored templates.
pub(super) fn register_actions(cx: &mut App) {
    cx.default_global::<ActionRegistry>().handle(NEW_FROM_TEMPLATE, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| workbench.new_from_template(arg.as_ref(), window, cx))
    });
    sync_template_actions(cx);
    cx.observe_global::<UiStorage>(sync_template_actions).detach();
}

/// The project types the templates of a language belong to; the others belong to every project.
/// JavaScript and TypeScript are offered in every project, as IntelliJ IDEA Ultimate does.
fn template_project_types(id: &str) -> Option<&'static [ProjectType]> {
    use ProjectType::{Cargo, Go, Gradle, Maven, Php, Python, Zig};
    Some(match id {
        "rust" => &[Cargo],
        "python" | "jupyter" => &[Python],
        "go" | "go-mod" => &[Go],
        "zig" => &[Zig],
        "php" | "php-class" => &[Php],
        "java" | "kotlin" => &[Maven, Gradle],
        _ => return None,
    })
}

/// The project roots of the windows, after the Default scheme's `None`, each once.
fn window_roots(cx: &App) -> Vec<Option<String>> {
    let mut roots: Vec<Option<String>> = vec![None];
    if let Some(workbenches) = cx.try_global::<Workbenches>() {
        for workbench in workbenches.0.values().filter_map(|workbench| workbench.upgrade()) {
            let root = workbench.read(cx).project_root(cx).map(|root| root.to_string_lossy().into_owned());
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// What the templates of the windows are read from: per root, the stored texts of the Default scheme,
/// of the root's scheme choice and of its Project scheme.
fn stored_texts(roots: &[Option<String>], cx: &App) -> StoredTexts {
    let storage = cx.try_global::<UiStorage>();
    let text = |key: &str| storage.and_then(|storage| storage.get(key)).map(str::to_owned);
    roots
        .iter()
        .map(|root| {
            let mut texts = vec![text(FILE_TEMPLATES_KEY)];
            if let Some(root) = root {
                texts.extend([text(&scheme_key(root)), text(&project_key(root))]);
            }
            (root.clone(), texts)
        })
        .collect()
}

/// The templates in effect in the windows of `roots` (`None`: the Default scheme's), each once, in the
/// order the first window that has it lists them.
fn templates_of_every_window(roots: &[Option<String>], cx: &App) -> Vec<FileTemplate> {
    let mut templates: Vec<FileTemplate> = Vec::new();
    for root in roots {
        for template in templates_in_effect(root.as_deref(), cx).enabled() {
            if !templates.iter().any(|known| known.id == template.id) {
                templates.push(template);
            }
        }
    }
    templates
}

/// Registers the actions of the templates every window has in effect in place of the ones registered
/// before.
pub fn sync_template_actions(cx: &mut App) {
    let roots = window_roots(cx);
    let texts = stored_texts(&roots, cx);
    // The storage changes for much else (layouts, sessions): the actions stay while what their templates
    // are read from does.
    if cx.default_global::<TemplateActions>().made_from.as_ref() == Some(&texts) {
        return;
    }
    let templates = templates_of_every_window(&roots, cx);
    register_templates(&templates, cx);
    cx.default_global::<TemplateActions>().made_from = Some(texts);
}

/// Registers the actions of the enabled `templates` in place of the ones registered before.
fn register_templates(templates: &[FileTemplate], cx: &mut App) {
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

    /// Whether a configuration of `kind` runs from the project that holds `directory`: the project is
    /// trusted and the configuration's folder is inside it.
    fn has_run_configuration(&self, kind: &str, directory: &str, cx: &App) -> bool {
        let Some(root) = self.project_root(cx) else { return false };
        if self.project_restricted(cx) || !Path::new(directory).starts_with(&root) {
            return false;
        }
        let Some(run) = self.run_driver() else { return false };
        run.read(cx).store().read(cx).configs().iter().any(|config| {
            let place = config.extras.module.as_deref().or_else(|| crate::run::configs::cwd_of(config));
            let folder = place.filter(|place| !place.is_empty()).map_or_else(|| root.clone(), |place| root.join(place));
            config.kind == kind && folder.starts_with(&root)
        })
    }

    /// The rows of the `templates.new` group for `ctx` (the folder New creates in).
    pub(super) fn templates_new_rows(&self, ctx: &MenuGroupContext<'_>, cx: &mut App) -> Vec<GroupRow> {
        let root = self.project_root(cx);
        let directory = ctx.paths.first();
        let arg = directory.map(|directory| json!({ "directory": directory }));
        // The type of the project the folder is in; a folder outside it has none, and every template is offered.
        let project_type = match (&root, directory) {
            (Some(root), Some(directory)) if Path::new(directory).starts_with(root) => project_type_of(root),
            _ => None,
        };
        let tomcat = directory.is_some_and(|directory| self.has_run_configuration("tomcat", directory, cx));
        let root = root.map(|root| root.to_string_lossy().into_owned());
        let templates = templates_in_effect(root.as_deref(), cx).enabled();
        templates
            .into_iter()
            .filter(|template| template.id != EMPTY_FILE)
            .filter(|template| {
                if template.id == "tomcat-context" {
                    return tomcat;
                }
                match (template_project_types(&template.id), project_type) {
                    (Some(types), Some(project_type)) => types.contains(&project_type),
                    _ => true,
                }
            })
            .map(|template| {
                let action = static_action_id(&format!("{NEW_TEMPLATE_PREFIX}{}", template.id));
                MenuItem::Action(ActionItem { action, arg: arg.clone(), label: Some(template.name), disabled: false })
            })
            .collect()
    }
}
