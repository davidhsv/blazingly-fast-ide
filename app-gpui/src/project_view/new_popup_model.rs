//! The "New" popup: the New › rows under a "Search for file type" field.

use crate::platform::actions::ActionMeta;
use crate::platform::i18n::I18n;
use crate::project_view::context_menu::{Item, ProjectCommand};
use crate::ui::matching::fuzzy_match;
use crate::vcs_ops::menu::MenuItem;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What choosing a row of the popup does.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupRun {
    /// One of the project view's own commands.
    Command(ProjectCommand),
    /// A registered action, run with the argument.
    Action { id: &'static str, arg: Option<Value> },
}

/// A row of the popup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewPopupRow {
    pub key: String,
    pub separator: bool,
    /// In the UI language.
    pub label: String,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub run: Option<PopupRun>,
    /// Where the query matched the label, while filtering: the byte offset of each matched
    /// character.
    pub matches: Option<Vec<usize>>,
}

/// The popup to show: where, and the rows it filters.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPopupRequest {
    pub at: (f32, f32),
    pub items: Vec<Item>,
    /// Tells one opening of the popup from the next.
    pub seq: u64,
}

/// Whether the popup is open, and with what.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewPopup {
    pub request: Option<NewPopupRequest>,
    seq: u64,
}

impl NewPopup {
    pub fn open(&mut self, at: (f32, f32), items: Vec<Item>) {
        self.seq += 1;
        self.request = Some(NewPopupRequest { at, items, seq: self.seq });
    }

    pub fn close(&mut self) {
        self.request = None;
    }
}

/// What the popup knows of the registered actions.
#[derive(Debug, Clone, Copy)]
pub struct PopupActions<'a> {
    pub registered: &'a BTreeMap<String, ActionMeta>,
    /// The ids of the registered actions that are off in the current context.
    pub disabled: &'a BTreeSet<String>,
    /// The shortcut the keymap shows for an action, by action id.
    pub shortcuts: &'a BTreeMap<String, String>,
}

/// Keywords of actions that would match every template.
const GENERIC: [&str; 4] = ["new", "create", "template", "file"];

/// `text` in lower case, without the dot a file extension is typed with.
fn keyword(text: &str) -> String {
    let lower = text.to_lowercase();
    lower.strip_prefix('.').map_or_else(|| lower.clone(), str::to_owned)
}

/// Whether a keyword of the action `id` starts with the query (`css` finds Stylesheet).
fn keyword_match(actions: &PopupActions<'_>, id: &str, query: &str) -> bool {
    let wanted = keyword(query);
    let keywords = actions.registered.get(id).map(|action| action.keywords.as_slice()).unwrap_or_default();
    keywords.iter().any(|word| !GENERIC.contains(&word.as_str()) && keyword(word).starts_with(&wanted))
}

/// The row of `item`, the `index`th of the menu; none for a row that cannot be chosen.
fn row_of(item: &Item, index: usize, actions: &PopupActions<'_>, i18n: &I18n) -> Option<NewPopupRow> {
    match item {
        MenuItem::Separator => None,
        MenuItem::Action(bound) => {
            let action = actions.registered.get(bound.action)?;
            if bound.disabled || actions.disabled.contains(bound.action) {
                return None;
            }
            let key = match bound.arg.as_ref().and_then(Value::as_str) {
                Some(arg) => format!("{}|{arg}", bound.action),
                None => bound.action.to_owned(),
            };
            let label = match &bound.label {
                Some(label) => i18n.tx(label),
                None => action.localized(i18n).title,
            };
            Some(NewPopupRow {
                key,
                label,
                icon: action.icon.clone(),
                shortcut: actions.shortcuts.get(bound.action).cloned(),
                run: Some(PopupRun::Action { id: bound.action, arg: bound.arg.clone() }),
                ..NewPopupRow::default()
            })
        }
        MenuItem::Item(entry) => {
            let run = entry.run.clone().filter(|_| !entry.disabled)?;
            Some(NewPopupRow {
                key: format!("{index}:{}", entry.label),
                label: i18n.tx(&entry.label),
                icon: entry.icon.map(str::to_owned),
                shortcut: entry.shortcut.clone(),
                run: Some(PopupRun::Command(run)),
                ..NewPopupRow::default()
            })
        }
    }
}

/// The rows of the popup for `query`: the entries that can be chosen whose label matches (the
/// match is highlighted), then the ones whose action has a matching keyword. Separators are kept
/// only without a query.
pub fn new_popup_rows(items: &[Item], query: &str, actions: &PopupActions<'_>, i18n: &I18n) -> Vec<NewPopupRow> {
    let query = query.trim();
    let mut rows: Vec<NewPopupRow> = Vec::new();
    let mut by_keyword: Vec<NewPopupRow> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if matches!(item, MenuItem::Separator) {
            if query.is_empty() && rows.last().is_some_and(|last| !last.separator) {
                rows.push(NewPopupRow { key: format!("sep{index}"), separator: true, ..NewPopupRow::default() });
            }
            continue;
        }
        let Some(row) = row_of(item, index, actions, i18n) else { continue };
        if query.is_empty() {
            rows.push(row);
        } else if let Some(matches) = fuzzy_match(&row.label, query) {
            rows.push(NewPopupRow { matches: Some(matches), ..row });
        } else if item.action_id().is_some_and(|id| keyword_match(actions, id, query)) {
            by_keyword.push(row);
        }
    }
    while rows.last().is_some_and(|last| last.separator) {
        rows.pop();
    }
    rows.extend(by_keyword);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_view::context_menu::testing::Surroundings;
    use crate::project_view::context_menu::{NewKind, new_menu_items};
    use crate::project_view::store::{ProjectInit, ProjectViewState};
    use serde_json::json;

    /// The actions the templates and the HTTP client register.
    fn registered() -> BTreeMap<String, ActionMeta> {
        let mut stylesheet = ActionMeta::new("templates.new.stylesheet", "New Stylesheet…");
        stylesheet.keywords = ["new", "css", "scss", "less"].map(str::to_owned).to_vec();
        let actions = [
            ActionMeta::new("templates.new.file", "New Empty File…"),
            ActionMeta::new("templates.new.python", "New Python File…"),
            stylesheet,
            ActionMeta::new("http.newRequest", "New HTTP Request"),
        ];
        actions.into_iter().map(|action| (action.id.clone(), action)).collect()
    }

    /// The project `/p` with the file kinds contributed for `directory`: the templates' group,
    /// then the HTTP client's.
    fn surroundings(directory: &str) -> Surroundings {
        let mut state = ProjectViewState::default();
        state.init(Some(ProjectInit { root: "/p", name: "p", ..ProjectInit::default() }));
        let ids: Vec<String> = registered().into_keys().collect();
        let mut around = Surroundings::new(state, &ids.iter().map(String::as_str).collect::<Vec<_>>());
        around.is_git = false;
        let arg = json!({ "directory": directory });
        around.contributed.new = vec![
            MenuItem::action_with("templates.new.python", arg.clone()).labelled("Python File"),
            MenuItem::action_with("templates.new.stylesheet", arg.clone()).labelled("Stylesheet"),
            MenuItem::Separator,
            MenuItem::action_with("http.newRequest", arg).labelled("HTTP Request"),
            MenuItem::Separator,
        ];
        around
    }

    fn rows(items: &[Item], query: &str) -> Vec<NewPopupRow> {
        let (registered, disabled, shortcuts) = (registered(), BTreeSet::new(), BTreeMap::new());
        let actions = PopupActions { registered: &registered, disabled: &disabled, shortcuts: &shortcuts };
        new_popup_rows(items, query, &actions, &I18n::default())
    }

    fn labels(items: &[Item], query: &str) -> Vec<String> {
        rows(items, query).into_iter().map(|row| if row.separator { "-".to_owned() } else { row.label }).collect()
    }

    // src/features/project-tree/newPopupModel.test.ts › New popup ›
    // lists File, Directory and the contributed file kinds
    #[test]
    fn lists_file_directory_and_the_contributed_file_kinds() {
        let around = surroundings("/p/src");
        let items = new_menu_items(&around.ctx(), Some("/p/src"), false);
        let expected = ["File", "Directory", "-", "Python File", "Stylesheet", "-", "HTTP Request"];
        assert_eq!(labels(&items, ""), expected);
    }

    // src/features/project-tree/newPopupModel.test.ts › New popup › passes the target directory to contributed kinds
    #[test]
    fn passes_the_target_directory_to_contributed_kinds() {
        let around = surroundings("/p/api");
        let items = new_menu_items(&around.ctx(), Some("/p/api"), false);
        let http = items.iter().find(|item| item.action_id() == Some("http.newRequest")).expect("the HTTP row");
        assert_eq!(http.arg(), Some(&json!({ "directory": "/p/api" })));
        let chosen = rows(&items, "http").into_iter().next().and_then(|row| row.run);
        let run = PopupRun::Action { id: "http.newRequest", arg: Some(json!({ "directory": "/p/api" })) };
        assert_eq!(chosen, Some(run));
    }

    // src/features/project-tree/newPopupModel.test.ts › New popup ›
    // filters by label, then by keyword, without separators
    #[test]
    fn filters_by_label_then_by_keyword_without_separators() {
        let around = surroundings("/p");
        let items = new_menu_items(&around.ctx(), Some("/p"), false);
        assert_eq!(labels(&items, "py"), ["Python File"]);
        assert_eq!(labels(&items, "css"), ["Stylesheet"]);
        assert_eq!(labels(&items, "http"), ["HTTP Request"]);
        assert!(labels(&items, "zzz").is_empty());
        assert_eq!(rows(&items, "py")[0].matches, Some(vec![0, 1]));
    }

    // src/features/project-tree/newPopupModel.test.ts › New popup ›
    // asks for the file name in a dialog outside the project view
    #[test]
    fn asks_for_the_file_name_in_a_dialog_outside_the_project_view() {
        let around = surroundings("/p/src");
        let items = new_menu_items(&around.ctx(), Some("/p/src"), true);
        let file = rows(&items, "").into_iter().next().expect("the File row");
        assert_eq!(file.label, "File");
        let dialog = ProjectCommand::Run { action: "templates.new.file", arg: json!({ "directory": "/p/src" }) };
        assert_eq!(file.run, Some(PopupRun::Command(dialog)));
        // In the project view the name is typed into the tree.
        let inline = new_menu_items(&around.ctx(), Some("/p/src"), false);
        let new = ProjectCommand::New { kind: NewKind::File, parent: Some("/p/src".to_owned()), show_view: false };
        assert_eq!(rows(&inline, "")[0].run, Some(PopupRun::Command(new)));
    }

    // src/features/project-tree/newPopupModel.test.ts › New popup › opens with a fresh request each time
    #[test]
    fn opens_with_a_fresh_request_each_time() {
        let mut popup = NewPopup::default();
        popup.open((1.0, 2.0), Vec::new());
        let first = popup.request.as_ref().map(|request| request.seq).expect("the popup is open");
        popup.open((1.0, 2.0), Vec::new());
        assert_eq!(popup.request.as_ref().map(|request| request.seq), Some(first + 1));
        popup.close();
        assert_eq!(popup.request, None);
    }
}
