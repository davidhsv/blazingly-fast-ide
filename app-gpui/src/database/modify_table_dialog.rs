//! Modify Table / Create Table: the table's name, its columns (added, renamed, retyped, dropped),
//! the selected column's properties, and a preview of the statements OK runs. The statements come
//! from [`alter_table_sql`] / [`create_table_sql`], which quote every name the user types.

use super::dialog_parts::{
    button, checkbox, dialog_body, dialog_footer, dialog_frame, field_frame, label, sql_preview, tx, tx_template,
};
use super::objects::{ColumnDraft, alter_table_sql, can_alter_column, create_table_sql, draft_of};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_db::{ColumnInfo, DbDriver};

/// Why an existing SQLite column's type, nullability and default cannot be changed.
const SQLITE_LOCKED: &str = "SQLite can rename or drop an existing column, not change its type, NOT NULL or default.";

/// What the dialog asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModifyTableEvent {
    /// OK: run these statements on the data source (`DbStore::run_sql`); answer with
    /// [`ModifyTableDialog::finished`].
    Apply(Vec<String>),
    Cancelled,
}

/// What the dialog is opened for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModifyTableRequest {
    pub target: String,
    pub schema: String,
    /// `None`: Create Table.
    pub table: Option<String>,
    pub driver: DbDriver,
    /// The column whose properties show first (Modify Column…).
    pub focus_column: Option<String>,
    /// New › Column: a new column is added and selected.
    pub add_column: bool,
}

/// A column's property field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnField {
    Name,
    DataType,
    Default,
}

/// A row of the column list as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRow {
    pub name: String,
    pub data_type: String,
    pub primary_key: bool,
    pub dropped: bool,
    pub selected: bool,
}

/// The data types offered first for a new column.
fn first_type(driver: DbDriver) -> &'static str {
    match driver {
        DbDriver::Sqlite => "INTEGER",
        DbDriver::Postgres => "integer",
        DbDriver::Mysql | DbDriver::SqlServer => "int",
        DbDriver::Clickhouse => "UInt64",
        DbDriver::Oracle => "NUMBER",
    }
}

pub struct ModifyTableDialog {
    request: ModifyTableRequest,
    /// `None` while the table's columns load; `Some(Err)` when they cannot be read.
    columns: Option<Result<Vec<ColumnDraft>, String>>,
    selected: Option<u64>,
    next_key: u64,
    busy: bool,
    name: Entity<TextField>,
    column_name: Entity<TextField>,
    data_type: Entity<TextField>,
    default: Entity<TextField>,
    _events: Vec<Subscription>,
}

impl EventEmitter<ModifyTableEvent> for ModifyTableDialog {}

impl ModifyTableDialog {
    /// The dialog for `request`; a table to modify waits for [`Self::set_columns`].
    pub fn new(request: ModifyTableRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = request.table.clone().unwrap_or_else(|| "table_name".to_owned());
        let name = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&initial, cx);
            field
        });
        let column_name = cx.new(TextField::new);
        let data_type = cx.new(TextField::new);
        let default = cx.new(|cx| TextField::new(cx).with_placeholder("expression"));
        let mut events = vec![cx.subscribe_in(&name, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| {
            dialog.field_event(None, event, cx)
        })];
        for (field, which) in
            [(&column_name, ColumnField::Name), (&data_type, ColumnField::DataType), (&default, ColumnField::Default)]
        {
            events.push(cx.subscribe_in(field, window, move |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| {
                dialog.field_event(Some(which), event, cx)
            }));
        }
        let mut dialog = Self {
            columns: None,
            selected: None,
            next_key: 1,
            busy: false,
            name,
            column_name,
            data_type,
            default,
            _events: events,
            request,
        };
        if dialog.request.table.is_none() {
            let mut first = dialog.blank_column(1);
            first.name = "id".to_owned();
            first.primary_key = dialog.request.driver != DbDriver::Clickhouse;
            first.not_null = true;
            dialog.selected = Some(first.key);
            dialog.columns = Some(Ok(vec![first]));
            dialog.show_selected(cx);
        }
        dialog
    }

    fn blank_column(&mut self, n: usize) -> ColumnDraft {
        let key = self.next_key;
        self.next_key += 1;
        ColumnDraft {
            key,
            original: None,
            name: format!("column_{n}"),
            data_type: first_type(self.request.driver).to_owned(),
            not_null: false,
            default: String::new(),
            primary_key: false,
            dropped: false,
        }
    }

    /// The table's columns as the backend read them, or why they cannot be read.
    pub fn set_columns(&mut self, columns: Result<Vec<ColumnInfo>, String>, cx: &mut Context<Self>) {
        let columns = match columns {
            Ok(columns) => columns,
            Err(message) => {
                self.columns = Some(Err(message));
                cx.notify();
                return;
            }
        };
        let mut drafts: Vec<ColumnDraft> = columns
            .iter()
            .map(|column| {
                let key = self.next_key;
                self.next_key += 1;
                draft_of(column, key)
            })
            .collect();
        if self.request.add_column {
            let blank = self.blank_column(drafts.len() + 1);
            drafts.push(blank);
        }
        let focus = match (&self.request.focus_column, self.request.add_column) {
            (_, true) => drafts.last(),
            (Some(name), false) => drafts.iter().find(|draft| &draft.name == name).or(drafts.first()),
            (None, false) => drafts.first(),
        };
        self.selected = focus.map(|draft| draft.key);
        self.columns = Some(Ok(drafts));
        self.show_selected(cx);
        cx.notify();
    }

    pub fn request(&self) -> &ModifyTableRequest {
        &self.request
    }

    /// `Modify Table users`, or `Create Table`.
    pub fn title(&self) -> String {
        match &self.request.table {
            Some(table) => format!("Modify Table {table}"),
            None => "Create Table".to_owned(),
        }
    }

    pub fn is_loading(&self) -> bool {
        self.columns.is_none()
    }

    pub fn error(&self) -> Option<&str> {
        match &self.columns {
            Some(Err(message)) => Some(message),
            _ => None,
        }
    }

    fn drafts(&self) -> &[ColumnDraft] {
        match &self.columns {
            Some(Ok(drafts)) => drafts,
            _ => &[],
        }
    }

    fn drafts_mut(&mut self) -> Option<&mut Vec<ColumnDraft>> {
        match &mut self.columns {
            Some(Ok(drafts)) => Some(drafts),
            _ => None,
        }
    }

    fn current(&self) -> Option<&ColumnDraft> {
        self.drafts().iter().find(|draft| Some(draft.key) == self.selected)
    }

    pub fn rows(&self) -> Vec<ColumnRow> {
        self.drafts()
            .iter()
            .map(|draft| ColumnRow {
                name: draft.name.clone(),
                data_type: draft.data_type.clone(),
                primary_key: draft.primary_key,
                dropped: draft.dropped,
                selected: Some(draft.key) == self.selected,
            })
            .collect()
    }

    /// The statements OK runs.
    pub fn statements(&self, cx: &App) -> Vec<String> {
        if self.columns.as_ref().is_none_or(Result::is_err) {
            return Vec::new();
        }
        let request = &self.request;
        let name = self.name.read(cx).text().trim().to_owned();
        match &request.table {
            None => {
                let name = if name.is_empty() { "table_name" } else { name.as_str() };
                vec![create_table_sql(request.driver, &request.schema, name, self.drafts())]
            }
            Some(table) => alter_table_sql(request.driver, &request.schema, table, &name, self.drafts()),
        }
    }

    pub fn can_apply(&self, cx: &App) -> bool {
        !self.busy && !self.name.read(cx).text().trim().is_empty() && !self.statements(cx).is_empty()
    }

    /// The selected column cannot change its type, nullability or default (SQLite).
    pub fn locked(&self) -> bool {
        self.current().is_some_and(|draft| !can_alter_column(self.request.driver, draft))
    }

    pub fn table_field(&self) -> &Entity<TextField> {
        &self.name
    }

    pub fn column_field(&self, which: ColumnField) -> &Entity<TextField> {
        match which {
            ColumnField::Name => &self.column_name,
            ColumnField::DataType => &self.data_type,
            ColumnField::Default => &self.default,
        }
    }

    /// The selected column's values in its fields.
    fn show_selected(&mut self, cx: &mut Context<Self>) {
        let (name, data_type, default) = match self.current() {
            Some(draft) => (draft.name.clone(), draft.data_type.clone(), draft.default.clone()),
            None => Default::default(),
        };
        self.column_name.update(cx, |field, cx| field.set_text(&name, cx));
        self.data_type.update(cx, |field, cx| field.set_text(&data_type, cx));
        self.default.update(cx, |field, cx| field.set_text(&default, cx));
    }

    fn patch(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ColumnDraft)) {
        let selected = self.selected;
        if let Some(draft) = self.drafts_mut().and_then(|drafts| drafts.iter_mut().find(|d| Some(d.key) == selected)) {
            edit(draft);
        }
        cx.notify();
    }

    fn field_event(&mut self, which: Option<ColumnField>, event: &TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Submitted => self.press_ok(cx),
            TextFieldEvent::Cancelled => cx.emit(ModifyTableEvent::Cancelled),
            TextFieldEvent::Changed => {
                let Some(which) = which else {
                    cx.notify();
                    return;
                };
                let text = self.column_field(which).read(cx).text().to_owned();
                let locked = self.locked();
                self.patch(cx, |draft| match which {
                    ColumnField::Name if !draft.dropped => draft.name = text,
                    ColumnField::DataType if !draft.dropped && !locked => draft.data_type = text,
                    ColumnField::Default if !draft.dropped && !locked => draft.default = text,
                    _ => {}
                });
            }
        }
    }

    pub fn select_column(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = self.drafts().get(index).map(|draft| draft.key);
        self.show_selected(cx);
        cx.notify();
    }

    /// Add Column: a blank column, selected.
    pub fn add_column(&mut self, cx: &mut Context<Self>) {
        let n = self.drafts().len() + 1;
        let blank = self.blank_column(n);
        let key = blank.key;
        let Some(drafts) = self.drafts_mut() else { return };
        drafts.push(blank);
        self.selected = Some(key);
        self.show_selected(cx);
        cx.notify();
    }

    /// Drop Column: an existing column is marked dropped (pressed again: kept); a new one goes.
    pub fn remove_column(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current().cloned() else { return };
        if current.original.is_some() {
            self.patch(cx, |draft| draft.dropped = !draft.dropped);
            return;
        }
        let Some(drafts) = self.drafts_mut() else { return };
        drafts.retain(|draft| draft.key != current.key);
        self.selected = drafts.last().map(|draft| draft.key);
        self.show_selected(cx);
        cx.notify();
    }

    pub fn toggle_not_null(&mut self, cx: &mut Context<Self>) {
        if self.locked() || self.request.driver == DbDriver::Clickhouse {
            return;
        }
        self.patch(cx, |draft| {
            if !draft.dropped {
                draft.not_null = !draft.not_null;
            }
        });
    }

    /// Primary key is offered when creating a table only.
    pub fn toggle_primary_key(&mut self, cx: &mut Context<Self>) {
        if self.request.table.is_some() || self.request.driver == DbDriver::Clickhouse {
            return;
        }
        self.patch(cx, |draft| draft.primary_key = !draft.primary_key);
    }

    pub fn press_ok(&mut self, cx: &mut Context<Self>) {
        if !self.can_apply(cx) {
            return;
        }
        self.busy = true;
        let statements = self.statements(cx);
        cx.emit(ModifyTableEvent::Apply(statements));
        cx.notify();
    }

    /// The statements ran (`ok`: all of them); a failure leaves the dialog open for another try,
    /// the error told by a notification.
    pub fn finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        self.busy = false;
        if ok {
            cx.emit(ModifyTableEvent::Cancelled);
        }
        cx.notify();
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn icon_button(
        id: &'static str,
        icon: &str,
        title: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let title = tx(cx, title);
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(title))
            .child(theme::icon(cx, icon, 14.0, theme::color(cx, if enabled { "fg" } else { "fg-faint" })))
    }

    fn field_row(cx: &mut Context<Self>, caption: &str, field: &Entity<TextField>, enabled: bool) -> gpui_kit::Div {
        let caption = label(cx, caption);
        let frame = field_frame(cx).flex_1().when(!enabled, |el| el.opacity(0.5)).child(field.clone());
        div().flex().items_center().gap(px(8.0)).child(caption).child(frame)
    }
}

impl Focusable for ModifyTableDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.request.table.is_none() { self.name.focus_handle(cx) } else { self.column_name.focus_handle(cx) }
    }
}

impl Render for ModifyTableDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx_template(cx, &self.title());
        let (ok_label, cancel_label) = (tx(cx, "OK"), tx(cx, "Cancel"));
        let can_apply = self.can_apply(cx);
        let ok = button(cx, ok_label, true, can_apply)
            .id("db-modify-ok")
            .debug_selector(|| "db-modify-ok".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press_ok(cx)));
        let cancel = button(cx, cancel_label, false, true)
            .id("db-modify-cancel")
            .debug_selector(|| "db-modify-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ModifyTableEvent::Cancelled)));
        let footer = dialog_footer().child(cancel).child(ok);
        if self.columns.as_ref().is_none_or(Result::is_err) {
            let text = match self.error() {
                Some(message) => message.to_owned(),
                None => tx(cx, "Loading…"),
            };
            return dialog_frame(cx, &title, 440.0).child(dialog_body().child(SharedString::from(text))).child(footer);
        }
        let muted = theme::color(cx, "fg-muted");
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let name_row = {
            let caption = label(cx, "Table:");
            let frame = field_frame(cx).flex_1().child(self.name.clone());
            let schema = div().text_color(muted).child(SharedString::from(self.request.schema.clone()));
            div().flex().items_center().gap(px(8.0)).child(caption).child(frame).child(schema)
        };
        let rows: Vec<_> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let icon = if row.primary_key { "db-column-key" } else { "db-column" };
                let name = if row.name.is_empty() { "<unnamed>".to_owned() } else { row.name };
                div()
                    .id(SharedString::from(format!("db-column-row-{index}")))
                    .debug_selector(move || format!("db-column-row-{index}"))
                    .h(theme::length(cx, "h-list-row"))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .whitespace_nowrap()
                    .when(row.selected, |el| el.bg(selected_bg))
                    .when(row.dropped, |el| el.line_through().opacity(0.6))
                    .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg")))
                    .child(SharedString::from(name))
                    .child(div().text_color(muted).child(SharedString::from(row.data_type)))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.select_column(index, cx)))
            })
            .collect();
        let current = self.current().cloned();
        let dropped = current.as_ref().is_some_and(|draft| draft.dropped);
        let add = Self::icon_button("db-column-add", "plus", "Add Column", true, cx)
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.add_column(cx)));
        let remove_title = if dropped { "Keep Column" } else { "Drop Column" };
        let remove = Self::icon_button("db-column-remove", "minus", remove_title, current.is_some(), cx)
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.remove_column(cx)));
        let locked = self.locked();
        let properties = match &current {
            None => {
                let text = tx(cx, "Select a column");
                div().text_color(muted).child(SharedString::from(text))
            }
            Some(draft) => {
                let driver = self.request.driver;
                let mut props = div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(Self::field_row(cx, "Name", &self.column_name, !dropped))
                    .child(Self::field_row(cx, "Data type", &self.data_type, !dropped && !locked));
                if driver != DbDriver::Clickhouse {
                    let text = tx(cx, "Not null");
                    props = props.child(
                        checkbox(cx, text, draft.not_null, !dropped && !locked)
                            .id("db-column-not-null")
                            .debug_selector(|| "db-column-not-null".to_owned())
                            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_not_null(cx))),
                    );
                }
                if self.request.table.is_none() && driver != DbDriver::Clickhouse {
                    let text = tx(cx, "Primary key");
                    props = props.child(
                        checkbox(cx, text, draft.primary_key, true)
                            .id("db-column-primary-key")
                            .debug_selector(|| "db-column-primary-key".to_owned())
                            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_primary_key(cx))),
                    );
                }
                props = props.child(Self::field_row(cx, "Default", &self.default, !dropped && !locked));
                if locked {
                    let hint = tx(cx, SQLITE_LOCKED);
                    props = props.child(div().text_color(muted).child(SharedString::from(hint)));
                }
                props
            }
        };
        let statements = self.statements(cx);
        let preview = if statements.is_empty() { "-- no changes".to_owned() } else { statements.join("\n") };
        let border = theme::color(cx, "border");
        let preview = sql_preview(cx, "Preview", &preview);
        dialog_frame(cx, &title, 760.0)
            .child(
                dialog_body().child(name_row).child(
                    div()
                        .h(px(240.0))
                        .flex()
                        .gap(px(12.0))
                        .child(
                            div()
                                .w(px(280.0))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .border_1()
                                .border_color(border)
                                .rounded(theme::length(cx, "radius-small"))
                                .child(div().flex().gap(px(2.0)).p(px(2.0)).child(add).child(remove))
                                .child(
                                    div().id("db-column-list").flex_1().min_h_0().overflow_y_scroll().children(rows),
                                ),
                        )
                        .child(div().flex_1().min_w_0().child(properties)),
                ),
            )
            .child(div().px(px(16.0)).child(preview))
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ModifyTableEvent>>>;

    fn column(name: &str, data_type: &str, primary_key: bool) -> ColumnInfo {
        let nullable = !primary_key;
        ColumnInfo { name: name.into(), data_type: data_type.into(), nullable, default: None, primary_key }
    }

    fn request(table: Option<&str>, driver: DbDriver) -> ModifyTableRequest {
        ModifyTableRequest {
            target: "s1".into(),
            schema: "main".into(),
            table: table.map(str::to_owned),
            driver,
            focus_column: None,
            add_column: false,
        }
    }

    fn open(
        cx: &mut TestAppContext,
        request: ModifyTableRequest,
    ) -> (Entity<ModifyTableDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| ModifyTableDialog::new(request, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &ModifyTableEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn type_into(view: &Entity<ModifyTableDialog>, which: ColumnField, text: &str, cx: &mut VisualTestContext) {
        let field = view.read_with(cx, |v, _| v.column_field(which).clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(text);
    }

    #[gpui_kit::test]
    async fn create_table_starts_with_an_id_primary_key_and_previews_the_create_statement(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, request(None, DbDriver::Sqlite));
        assert_eq!(view.read_with(cx, |v, _| v.title()), "Create Table");
        let rows = view.read_with(cx, |v, _| v.rows());
        assert_eq!((rows[0].name.as_str(), rows[0].primary_key), ("id", true));
        click(cx, "db-column-add");
        type_into(&view, ColumnField::Name, "title", cx);
        type_into(&view, ColumnField::DataType, "TEXT", cx);
        let field = view.read_with(cx, |v, _| v.table_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("books");
        let sql = view.read_with(cx, |v, cx| v.statements(cx));
        assert_eq!(sql.len(), 1);
        assert!(sql[0].starts_with("create table books"), "{sql:?}");
        assert!(sql[0].contains("title TEXT") && sql[0].contains("primary key (id)"), "{sql:?}");
        click(cx, "db-modify-ok");
        assert_eq!(events.borrow().last(), Some(&ModifyTableEvent::Apply(sql)));
        assert!(view.read_with(cx, |v, _| v.is_busy()), "OK waits for the statements");
    }

    #[gpui_kit::test]
    async fn modify_table_adds_renames_and_drops_columns(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, request(Some("users"), DbDriver::Sqlite));
        assert!(view.read_with(cx, |v, _| v.is_loading()));
        let columns = vec![column("id", "INTEGER", true), column("name", "TEXT", false)];
        view.update(cx, |v, cx| v.set_columns(Ok(columns), cx));
        assert_eq!(view.read_with(cx, |v, cx| v.statements(cx)), Vec::<String>::new(), "nothing changed yet");
        click(cx, "db-column-row-1");
        type_into(&view, ColumnField::Name, "full_name", cx);
        click(cx, "db-column-add");
        type_into(&view, ColumnField::Name, "email", cx);
        type_into(&view, ColumnField::DataType, "TEXT", cx);
        let sql = view.read_with(cx, |v, cx| v.statements(cx));
        assert_eq!(
            sql,
            [
                "alter table users rename column name to full_name;".to_owned(),
                "alter table users add column email TEXT;".to_owned(),
            ]
        );
        click(cx, "db-column-row-0");
        click(cx, "db-column-remove");
        assert!(view.read_with(cx, |v, _| v.rows()[0].dropped));
        let sql = view.read_with(cx, |v, cx| v.statements(cx));
        assert_eq!(sql.first().map(String::as_str), Some("alter table users drop column id;"));
    }

    #[gpui_kit::test]
    async fn an_existing_sqlite_column_keeps_its_type(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, request(Some("users"), DbDriver::Sqlite));
        view.update(cx, |v, cx| v.set_columns(Ok(vec![column("name", "TEXT", false)]), cx));
        assert!(view.read_with(cx, |v, _| v.locked()));
        type_into(&view, ColumnField::DataType, "INTEGER", cx);
        click(cx, "db-column-not-null");
        assert_eq!(view.read_with(cx, |v, _| v.rows()[0].data_type.clone()), "TEXT");
        assert!(view.read_with(cx, |v, cx| v.statements(cx)).is_empty());
    }

    #[gpui_kit::test]
    async fn new_column_adds_one_and_selects_it(cx: &mut TestAppContext) {
        let request = ModifyTableRequest { add_column: true, ..request(Some("users"), DbDriver::Postgres) };
        let (view, _, cx) = open(cx, request);
        view.update(cx, |v, cx| v.set_columns(Ok(vec![column("id", "integer", true)]), cx));
        let rows = view.read_with(cx, |v, _| v.rows());
        assert_eq!(rows.len(), 2);
        assert!(rows[1].selected && rows[1].name == "column_2");
        let sql = view.read_with(cx, |v, cx| v.statements(cx));
        assert_eq!(sql, ["alter table users add column column_2 integer;".to_owned()]);
    }

    #[gpui_kit::test]
    async fn a_table_that_cannot_be_read_shows_why_and_offers_no_ok(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, request(Some("users"), DbDriver::Sqlite));
        view.update(cx, |v, cx| v.set_columns(Err("no such table: users".into()), cx));
        assert_eq!(view.read_with(cx, |v, _| v.error().map(str::to_owned)), Some("no such table: users".into()));
        click(cx, "db-modify-ok");
        click(cx, "db-modify-cancel");
        assert_eq!(*events.borrow(), [ModifyTableEvent::Cancelled]);
    }

    #[gpui_kit::test]
    async fn a_failed_run_keeps_the_dialog_and_a_good_one_closes_it(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, request(None, DbDriver::Sqlite));
        click(cx, "db-modify-ok");
        view.update(cx, |v, cx| v.finished(false, cx));
        assert!(!view.read_with(cx, |v, _| v.is_busy()));
        assert_eq!(events.borrow().len(), 1);
        click(cx, "db-modify-ok");
        view.update(cx, |v, cx| v.finished(true, cx));
        assert_eq!(events.borrow().last(), Some(&ModifyTableEvent::Cancelled));
    }
}
