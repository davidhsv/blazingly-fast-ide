//! Import Data from File: a CSV / TSV file read into insert edits of a table ([`import_edits`]),
//! matched to its columns by the header's names or by position, with a preview of the first rows.
//! The edits are applied in one transaction by the owner (`db_apply_edits`), which binds every
//! value: no SQL is written from the file's text.

use super::dialog_parts::{button, checkbox, dialog_body, dialog_footer, dialog_frame, field_frame, label, tx};
use super::model::{guess_separator, import_edits, parse_delimited};
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};
use ide_db::DbRowEdit;

/// The separators offered, with their labels.
pub const SEPARATORS: [(char, &str); 4] = [(',', "Comma"), ('\t', "Tab"), (';', "Semicolon"), ('|', "Pipe")];
/// Rows of the file the preview shows.
const PREVIEW_ROWS: usize = 8;

/// What the dialog asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportEvent {
    /// Choose File…: an open dialog for a CSV or TSV file; answer with [`ImportDialog::file_read`].
    ChooseFile,
    /// Import: insert these rows; answer with [`ImportDialog::finished`].
    Apply(Vec<DbRowEdit>),
    Cancelled,
}

pub struct ImportDialog {
    schema: String,
    table: String,
    /// The table's columns; `None` while they load.
    columns: Option<Result<Vec<String>, String>>,
    file: Option<String>,
    text: String,
    separator: char,
    header: bool,
    empty_as_null: bool,
    busy: bool,
    focus: FocusHandle,
}

impl EventEmitter<ImportEvent> for ImportDialog {}

impl ImportDialog {
    pub fn new(schema: &str, table: &str, cx: &mut Context<Self>) -> Self {
        Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
            columns: None,
            file: None,
            text: String::new(),
            separator: ',',
            header: true,
            empty_as_null: true,
            busy: false,
            focus: cx.focus_handle(),
        }
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    /// `Import Data into main.users`.
    pub fn title(&self) -> String {
        format!("Import Data into {}.{}", self.schema, self.table)
    }

    /// The table's column names, or why they cannot be read.
    pub fn set_columns(&mut self, columns: Result<Vec<String>, String>, cx: &mut Context<Self>) {
        self.columns = Some(columns);
        cx.notify();
    }

    /// The chosen file and its text; the separator is guessed from it.
    pub fn file_read(&mut self, path: &str, text: String, cx: &mut Context<Self>) {
        self.separator = guess_separator(&text);
        self.file = Some(path.to_owned());
        self.text = text;
        cx.notify();
    }

    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    pub fn separator(&self) -> char {
        self.separator
    }

    pub fn set_separator(&mut self, separator: char, cx: &mut Context<Self>) {
        self.separator = separator;
        cx.notify();
    }

    pub fn toggle_header(&mut self, cx: &mut Context<Self>) {
        self.header = !self.header;
        cx.notify();
    }

    pub fn toggle_empty_as_null(&mut self, cx: &mut Context<Self>) {
        self.empty_as_null = !self.empty_as_null;
        cx.notify();
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.text.is_empty() { Vec::new() } else { parse_delimited(&self.text, self.separator) }
    }

    fn column_names(&self) -> &[String] {
        match &self.columns {
            Some(Ok(columns)) => columns,
            _ => &[],
        }
    }

    /// The insert edits Import applies, and the table columns the fields fill.
    pub fn plan(&self) -> (Vec<DbRowEdit>, Vec<String>) {
        let import = import_edits(&self.rows(), self.column_names(), self.header, self.empty_as_null);
        (import.edits, import.mapped)
    }

    /// The first rows of the file as the separator splits them.
    pub fn preview(&self) -> Vec<Vec<String>> {
        self.rows().into_iter().take(PREVIEW_ROWS).collect()
    }

    pub fn can_import(&self) -> bool {
        let (edits, mapped) = self.plan();
        !self.busy && !edits.is_empty() && !mapped.is_empty()
    }

    pub fn press_import(&mut self, cx: &mut Context<Self>) {
        if !self.can_import() {
            return;
        }
        self.busy = true;
        let (edits, _) = self.plan();
        cx.emit(ImportEvent::Apply(edits));
        cx.notify();
    }

    /// The rows went in (`ok`: the dialog closes) or not (it stays for another try).
    pub fn finished(&mut self, ok: bool, cx: &mut Context<Self>) {
        self.busy = false;
        if ok {
            cx.emit(ImportEvent::Cancelled);
        }
        cx.notify();
    }
}

impl Focusable for ImportDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let title = cx.default_global::<crate::platform::i18n::Localization>().0.tx_template(&self.title());
        let placeholder = tx(cx, "Choose a CSV or TSV file");
        let (file_text, file_color) = match &self.file {
            Some(file) => (file.clone(), theme::color(cx, "fg")),
            None => (placeholder, muted),
        };
        let choose_title = tx(cx, "Choose File…");
        let choose = div()
            .id("db-import-choose")
            .debug_selector(|| "db-import-choose".to_owned())
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .tooltip(crate::ui::tooltip::tooltip(choose_title))
            .child(theme::icon(cx, "folder", 14.0, theme::color(cx, "fg")))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ImportEvent::ChooseFile)));
        let file_label = label(cx, "File:");
        let file_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(file_label)
            .child(
                field_frame(cx)
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(file_color)
                    .child(SharedString::from(file_text)),
            )
            .child(choose);
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let border = theme::color(cx, "border");
        let separators: Vec<_> = SEPARATORS
            .iter()
            .enumerate()
            .map(|(index, (separator, name))| {
                let separator = *separator;
                let text = tx(cx, name);
                div()
                    .id(SharedString::from(format!("db-import-separator-{index}")))
                    .debug_selector(move || format!("db-import-separator-{index}"))
                    .h(px(24.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(border)
                    .when(self.separator == separator, |el| el.bg(selected_bg))
                    .child(SharedString::from(text))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.set_separator(separator, cx)))
            })
            .collect();
        let separator_label = label(cx, "Separator:");
        let separator_row = div().flex().items_center().gap(px(8.0)).child(separator_label).children(separators);
        let header_text = tx(cx, "First row is header (columns matched by name)");
        let header = checkbox(cx, header_text, self.header, true)
            .id("db-import-header")
            .debug_selector(|| "db-import-header".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_header(cx)));
        let null_text = tx(cx, "Treat empty values as NULL");
        let empty_as_null = checkbox(cx, null_text, self.empty_as_null, true)
            .id("db-import-null")
            .debug_selector(|| "db-import-null".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_empty_as_null(cx)));
        let (edits, mapped) = self.plan();
        let columns_text = match &self.columns {
            None => tx(cx, "Loading…"),
            Some(Err(message)) => message.clone(),
            Some(Ok(_)) if mapped.is_empty() => tx(cx, "No column of the file matches the table"),
            Some(Ok(_)) => mapped.join(", "),
        };
        let columns_label = label(cx, "Columns:");
        let columns_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(columns_label)
            .child(div().text_color(muted).child(SharedString::from(columns_text)));
        let header_row = self.header;
        let preview: Vec<_> = self
            .preview()
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                div().flex().when(header_row && index == 0, |el| el.text_color(theme::color(cx, "fg-strong"))).children(
                    row.into_iter().map(|value| {
                        div()
                            .w(px(120.0))
                            .flex_none()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(value))
                    }),
                )
            })
            .collect();
        let preview = (!preview.is_empty()).then(|| {
            div()
                .debug_selector(|| "db-import-preview".to_owned())
                .max_h(px(200.0))
                .overflow_hidden()
                .p(px(6.0))
                .border_1()
                .border_color(border)
                .rounded(theme::length(cx, "radius-small"))
                .text_size(theme::length(cx, "fs-small"))
                .children(preview)
        });
        let import_label = cx
            .default_global::<crate::platform::i18n::Localization>()
            .0
            .tx_template(&format!("Import {} Rows", edits.len()));
        let cancel_label = tx(cx, "Cancel");
        let can_import = self.can_import();
        let import = button(cx, import_label, true, can_import)
            .id("db-import-ok")
            .debug_selector(|| "db-import-ok".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press_import(cx)));
        let cancel = button(cx, cancel_label, false, true)
            .id("db-import-cancel")
            .debug_selector(|| "db-import-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ImportEvent::Cancelled)));
        dialog_frame(cx, &title, 720.0)
            .track_focus(&self.focus)
            .child(
                dialog_body()
                    .child(file_row)
                    .child(separator_row)
                    .child(header)
                    .child(empty_as_null)
                    .child(columns_row)
                    .children(preview),
            )
            .child(dialog_footer().child(cancel).child(import))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_db::CellValue;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ImportEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<ImportDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(|_, cx| ImportDialog::new("main", "users", cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ImportEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        let columns = vec!["id".to_owned(), "name".to_owned()];
        view.update(cx, |v, cx| v.set_columns(Ok(columns), cx));
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn insert(values: &[(&str, Option<&str>)]) -> DbRowEdit {
        DbRowEdit::Insert {
            values: values
                .iter()
                .map(|(column, value)| CellValue { column: (*column).into(), value: value.map(str::to_owned) })
                .collect(),
        }
    }

    #[gpui_kit::test]
    async fn a_csv_file_with_a_header_fills_the_columns_it_names(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert!(!view.read_with(cx, |v, _| v.can_import()), "nothing to import before a file is chosen");
        click(cx, "db-import-choose");
        assert_eq!(events.borrow().last(), Some(&ImportEvent::ChooseFile));
        view.update(cx, |v, cx| v.file_read("/data/users.csv", "name;extra\ncyd;1\n;2\n".into(), cx));
        assert_eq!(view.read_with(cx, |v, _| v.separator()), ';', "the separator is guessed");
        let (edits, mapped) = view.read_with(cx, |v, _| v.plan());
        assert_eq!(mapped, ["name"]);
        assert_eq!(edits, [insert(&[("name", Some("cyd"))]), insert(&[("name", None)])]);
        click(cx, "db-import-ok");
        assert_eq!(events.borrow().last(), Some(&ImportEvent::Apply(edits)));
    }

    #[gpui_kit::test]
    async fn without_a_header_the_fields_fill_the_columns_in_order(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        view.update(cx, |v, cx| v.file_read("/data/users.tsv", "7\tzoe\n".into(), cx));
        click(cx, "db-import-header");
        click(cx, "db-import-null");
        let (edits, mapped) = view.read_with(cx, |v, _| v.plan());
        assert_eq!(mapped, ["id", "name"]);
        assert_eq!(edits, [insert(&[("id", Some("7")), ("name", Some("zoe"))])]);
        click(cx, "db-import-separator-0");
        assert_eq!(view.read_with(cx, |v, _| v.separator()), ',');
    }

    #[gpui_kit::test]
    async fn a_file_matching_no_column_cannot_be_imported(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        view.update(cx, |v, cx| v.file_read("/data/x.csv", "other\n1\n".into(), cx));
        assert!(!view.read_with(cx, |v, _| v.can_import()));
        click(cx, "db-import-ok");
        assert!(!events.borrow().iter().any(|event| matches!(event, ImportEvent::Apply(_))));
        click(cx, "db-import-cancel");
        assert_eq!(events.borrow().last(), Some(&ImportEvent::Cancelled));
    }
}
