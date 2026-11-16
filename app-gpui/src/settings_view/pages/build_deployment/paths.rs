//! The Mappings and Excluded Paths tabs of a deployment server: the table of project folders and
//! the server folders they are deployed to (with their web paths), and the list of server paths
//! that are never uploaded to, downloaded from or deleted. The list is the shared
//! [`RecordTable`] over the selected server's paths, with its keyboard.

use super::{DeploymentServersView, MappingRow, commit, servers, tool, toolbar_strip};
use crate::settings_model::deployment_model::{
    MappingPatch, add_mapping, folder_within, remove_mapping, update_mapping,
};
use crate::settings_view::controls::tx;
use crate::settings_view::pages::tools_record_table::{ReadRows, RecordTable, TableSpec, WriteRows};
use crate::settings_view::pages::{Binding, ControlEvent, ROW_HEIGHT, Then, frame, new_field};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, IntoElement, MouseButton, SharedString, div, prelude::*,
    px,
};
use ide_core::settings::{DeploymentMapping, DeploymentServer};
use std::path::PathBuf;
use std::rc::Rc;

/// The mapping columns: (name, title, placeholder).
const COLUMNS: [(&str, &str, &str); 3] =
    [("local", "Local path", "Project root"), ("deployment", "Deployment path", "Root path"), ("web", "Web path", "/")];
const MAPPINGS_NOTE: &str = "Local paths are relative to the project root or absolute inside it (empty: the whole \
                             project); deployment paths are relative to the server's root path.";
const EXCLUDED_NOTE: &str = "Remote paths that are never uploaded to, downloaded from or deleted.";
const EXCLUDED_PLACEHOLDER: &str = "/var/www/app/cache";
const CELL_WIDTH: f32 = 170.0;

fn cells(mapping: &DeploymentMapping) -> [String; 3] {
    [mapping.local.clone(), mapping.deployment.clone(), mapping.web.clone()]
}

impl DeploymentServersView {
    /// The mapping rows show the selected server's mappings: rebuilt for another server or another
    /// number of rows, else each field takes a value that changed under it.
    pub(super) fn sync_mappings(&mut self, cx: &mut Context<Self>) {
        let current = self.current(cx);
        let id = current.as_ref().map(|server| server.id.clone());
        let mappings = current.map(|server| server.mappings).unwrap_or_default();
        if self.mapping_server != id || self.mapping_rows.len() != mappings.len() {
            let rows: Vec<MappingRow> =
                mappings.iter().enumerate().map(|(row, mapping)| Self::mapping_row(row, mapping, cx)).collect();
            self.mapping_server = id;
            self.mapping_rows = rows;
            if self.mapping_selected.is_some_and(|selected| selected >= mappings.len()) {
                self.mapping_selected = None;
            }
            return;
        }
        let mut updates: Vec<(Entity<TextField>, String)> = Vec::new();
        for (row, mapping) in self.mapping_rows.iter().zip(&mappings) {
            for (field, text) in row.fields.iter().zip(cells(mapping)) {
                if field.read(cx).text() != text {
                    updates.push((field.clone(), text));
                }
            }
        }
        for (field, text) in updates {
            field.update(cx, |field, cx| field.set_text(&text, cx));
        }
    }

    fn mapping_row(row: usize, mapping: &DeploymentMapping, cx: &mut Context<Self>) -> MappingRow {
        let texts = cells(mapping);
        let mut events = Vec::new();
        let fields: [Entity<TextField>; 3] = std::array::from_fn(|column| {
            let placeholder = tx(cx, COLUMNS[column].2);
            let field = new_field(&texts[column], Some(placeholder), cx);
            events.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    view.edit_mapping(row, column, text, cx);
                }
                TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            }));
            field
        });
        MappingRow { fields, _events: events }
    }

    /// Sets cell `column` of mapping `row` of the selected server.
    fn edit_mapping(&mut self, row: usize, column: usize, text: String, cx: &mut Context<Self>) {
        let patch = match column {
            0 => MappingPatch { local: Some(text), ..MappingPatch::default() },
            1 => MappingPatch { deployment: Some(text), ..MappingPatch::default() },
            _ => MappingPatch { web: Some(text), ..MappingPatch::default() },
        };
        self.mapping_selected = Some(row);
        self.update_current(cx, |server| server.mappings = update_mapping(&server.mappings, row, &patch));
    }

    pub fn add_mapping_row(&mut self, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx) else { return };
        self.update_current(cx, |server| server.mappings = add_mapping(&server.mappings));
        self.mapping_selected = Some(server.mappings.len());
        cx.notify();
    }

    pub fn remove_mapping_row(&mut self, cx: &mut Context<Self>) {
        let Some(selected) = self.mapping_selected.take() else { return };
        self.update_current(cx, |server| server.mappings = remove_mapping(&server.mappings, selected));
        cx.notify();
    }

    /// Browse… of a local path: the folder chosen, inside the project, becomes the row's local path
    /// relative to the project root; one outside it is not taken.
    fn browse_local(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(root) = self.project_root.clone() else { return };
        self.mapping_selected = Some(row);
        let view = cx.weak_entity();
        let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
            let picked = path.to_string_lossy().into_owned();
            let Some(relative) = folder_within(&root, &picked).map(str::to_owned) else { return };
            view.update(cx, |view, cx| view.edit_mapping(row, 0, relative, cx)).ok();
        });
        cx.emit(ControlEvent::Browse { directories: true, then });
    }

    /// The tab as text: the column titles, each row's cells (`>` the selected one) or the empty
    /// text, and the note.
    pub(super) fn mapping_lines(&self, server: &DeploymentServer, cx: &App) -> Vec<String> {
        let titles: Vec<String> = COLUMNS.iter().map(|(_, title, _)| tx(cx, title)).collect();
        let mut out = vec![titles.join(" | ")];
        if server.mappings.is_empty() {
            out.push(format!("  {}", tx(cx, "No mappings: nothing is deployed")));
        }
        for (index, row) in self.mapping_rows.iter().enumerate() {
            let mark = if self.mapping_selected == Some(index) { ">" } else { " " };
            let texts: Vec<String> = row.fields.iter().map(|field| field.read(cx).text().to_owned()).collect();
            out.push(format!("{mark} {}", texts.join(" | ")));
        }
        out.push(tx(cx, MAPPINGS_NOTE));
        out
    }

    pub(super) fn mappings_tab(&mut self, _server: &DeploymentServer, cx: &mut Context<Self>) -> AnyElement {
        let has = self.mapping_selected.is_some();
        let add = tool(cx, "settings-deployment-mappings-add", "plus", true, "Add")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_mapping_row(cx)));
        let remove = tool(cx, "settings-deployment-mappings-remove", "minus", has, "Remove")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove_mapping_row(cx))));
        let header =
            div().flex().gap(px(8.0)).px(px(6.0)).text_color(theme::color(cx, "fg-subtle")).children(
                COLUMNS.map(|(_, title, _)| div().w(px(CELL_WIDTH)).child(SharedString::from(tx(cx, title)))),
            );
        let can_browse = self.project_root.is_some();
        let selected_bg = theme::color(cx, "bg-selected");
        let rows: Vec<AnyElement> = self
            .mapping_rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let cells: Vec<AnyElement> = row
                    .fields
                    .iter()
                    .zip(COLUMNS)
                    .map(|(field, (name, _, _))| {
                        let selector = format!("settings-text-deployment.mapping-{index}-{name}");
                        let browse = (name == "local").then(|| {
                            let selector = format!("settings-deployment-mapping-{index}-browse");
                            tool(cx, &selector, "folder-open", can_browse, "Browse…").when(can_browse, |el| {
                                el.on_click(
                                    cx.listener(move |view, _: &ClickEvent, _, cx| view.browse_local(index, cx)),
                                )
                            })
                        });
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .child(
                                frame(cx, CELL_WIDTH, false)
                                    .id(SharedString::from(selector.clone()))
                                    .debug_selector(move || selector)
                                    .font_family("monospace")
                                    .child(field.clone()),
                            )
                            .children(browse)
                            .into_any_element()
                    })
                    .collect();
                let selector = format!("settings-deployment-mapping-{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .min_h(px(ROW_HEIGHT))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(self.mapping_selected == Some(index), |el| el.bg(selected_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            view.mapping_selected = Some(index);
                            cx.notify();
                        }),
                    )
                    .children(cells)
                    .into_any_element()
            })
            .collect();
        let empty = self.mapping_rows.is_empty().then(|| {
            let text = tx(cx, "No mappings: nothing is deployed");
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let note = tx(cx, MAPPINGS_NOTE);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .child(toolbar_strip(cx).child(add).child(remove))
                    .child(header)
                    .child(div().min_h(px(72.0)).flex().flex_col().children(rows).children(empty)),
            )
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
            .into_any_element()
    }

    // -----------------------------------------------------------------------
    // Excluded Paths

    /// The table of the excluded paths of server `id`: it reads them from the dialog's staged
    /// servers and stages the list it is left with.
    pub(super) fn excluded_table(binding: &Binding, id: Option<String>, cx: &mut Context<Self>) -> Entity<RecordTable> {
        let (read, write) = excluded_source(binding, id);
        let spec = TableSpec::list("deployment-excluded", EXCLUDED_PLACEHOLDER, "No excluded paths");
        cx.new(|cx| RecordTable::new(binding, spec, read, write, cx))
    }

    /// The table shows the excluded paths of server `id` from now on.
    pub(super) fn show_excluded_of(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let (read, write) = excluded_source(&self.binding, id);
        self.excluded.update(cx, |table, cx| table.set_source(read, write, cx));
    }

    pub fn excluded_paths(&self) -> &Entity<RecordTable> {
        &self.excluded
    }

    /// The tab as text: the table's rows (`>` the selected one) or its empty text, and the note.
    pub(super) fn excluded_lines(&self, _server: &DeploymentServer, cx: &App) -> Vec<String> {
        let mut out = self.excluded.read(cx).lines(cx);
        out.push(tx(cx, EXCLUDED_NOTE));
        out
    }

    pub(super) fn excluded_tab(&mut self, _server: &DeploymentServer, cx: &mut Context<Self>) -> AnyElement {
        let note = tx(cx, EXCLUDED_NOTE);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(div().max_w(px(560.0)).child(self.excluded.clone()))
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
            .into_any_element()
    }
}

/// Reads and writes the excluded paths of server `id` in the servers `binding` shows.
fn excluded_source(binding: &Binding, id: Option<String>) -> (ReadRows, WriteRows) {
    let (reader, wanted) = (binding.clone(), id.clone());
    let read: ReadRows = Rc::new(move |cx: &App| {
        let all = servers(&reader, cx);
        let found = all.into_iter().find(|server| Some(&server.id) == wanted.as_ref());
        found.map(|server| server.excluded_paths).unwrap_or_default().into_iter().map(|path| vec![path]).collect()
    });
    let writer = binding.clone();
    let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
        let mut all = servers(&writer, cx);
        let Some(server) = all.iter_mut().find(|server| Some(&server.id) == id.as_ref()) else { return };
        server.excluded_paths = rows.into_iter().filter_map(|row| row.into_iter().next()).collect();
        commit(&writer, &all, cx);
    });
    (read, write)
}
