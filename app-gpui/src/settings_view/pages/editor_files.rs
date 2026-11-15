//! Editor › File Types: the recognized file types (the languages of the definitions, each with its
//! file name patterns and hashbang patterns, stored in `files.fileTypes` as additions and
//! removals), and the names and globs left out of the project tree, search and indexing.

use super::list_editor::{ListShape, ListSpec};
use super::tools_record_table::{ReadRows, RecordTable, TableSpec, WriteRows};
use super::{Binding, ControlEvent, MEDIUM_FIELD, frame, hint_line, new_field, row, row_label};
use crate::backend::Backend;
use crate::platform::collate;
use crate::settings_model::file_types::{
    FILE_TYPES_KEY, FileTypeAssociations, LanguagePatterns, apply_pattern_edit, effective_patterns, sample_file_name,
    set_hashbangs,
};
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::rc::Rc;

pub const PAGE: &str = "editor.files";

/// A language as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileType {
    pub id: String,
    pub name: String,
    /// Without the dot.
    pub extensions: Vec<String>,
    pub filenames: Vec<String>,
    /// The first-line regex that also selects the language (`<?php`, a shebang).
    pub first_line: Option<String>,
}

impl FileType {
    fn patterns(&self) -> LanguagePatterns<'_> {
        LanguagePatterns { id: &self.id, extensions: &self.extensions, filenames: &self.filenames }
    }
}

/// The languages of the backend's definitions, by name.
pub fn backend_file_types(cx: &App) -> Vec<FileType> {
    let Some(backend) = cx.try_global::<Backend>() else { return Vec::new() };
    let mut types: Vec<FileType> = backend
        .host
        .languages
        .all()
        .into_iter()
        .map(|language| FileType {
            id: language.id.0,
            name: language.name,
            extensions: language.extensions,
            filenames: language.filenames,
            first_line: language.first_line,
        })
        .collect();
    types.sort_by(|a, b| collate::compare(&a.name, &b.name));
    types
}

/// The list editors of the page.
pub fn lists() -> Vec<ListSpec> {
    vec![
        ListSpec {
            label: "Ignore files and folders",
            key: "files.ignoredNames",
            shape: ListShape::Strings { placeholder: "*.ext" },
            empty: "No patterns",
            hint: Some(
                "Names (e.g. .DS_Store, *.pyc) hidden from the project tree, navigation bar, search, indexing and Git \
                 changes.",
            ),
        },
        ListSpec {
            label: "Exclude patterns",
            key: "files.exclude",
            shape: ListShape::Strings { placeholder: "**/pattern" },
            empty: "No patterns",
            hint: Some(
                "Glob patterns (e.g. **/dist, *.log) hidden from the project tree and skipped by search and indexing.",
            ),
        },
    ]
}

/// The user's patterns as the page shows them; a value that is not one reads as none.
fn associations(binding: &Binding, cx: &App) -> FileTypeAssociations {
    binding.value(FILE_TYPES_KEY, cx).and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default()
}

fn stage_associations(binding: &Binding, assoc: &FileTypeAssociations, cx: &mut App) {
    match serde_json::to_value(assoc) {
        Ok(value) => binding.stage(FILE_TYPES_KEY, value, cx),
        Err(error) => tracing::warn!("Settings: the file types could not be staged: {error}"),
    }
}

/// Recognized File Types: a filter, the languages, and the patterns of the one selected.
pub struct RecognizedFileTypesView {
    binding: Binding,
    types: Rc<Vec<FileType>>,
    filter: Entity<TextField>,
    /// The language picked in the list; none: the first one the filter keeps.
    selected: Option<String>,
    /// The language whose patterns the tables show.
    sourced: Option<String>,
    patterns: Entity<RecordTable>,
    hashbangs: Entity<RecordTable>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for RecognizedFileTypesView {}

impl RecognizedFileTypesView {
    pub fn new(binding: Binding, types: Vec<FileType>, cx: &mut Context<Self>) -> Self {
        let types = Rc::new(types);
        let filter = new_field("", Some(tx(cx, "Filter file types")), cx);
        let filter_events = cx.subscribe(&filter, |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                view.follow_selection(cx);
                cx.notify();
            }
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        });
        let first = types.first().map(|language| language.id.clone());
        let (read, write) = pattern_source(&binding, &types, first.clone());
        let patterns = cx.new(|cx| {
            RecordTable::new(&binding, TableSpec::list("file-type-patterns", "*.ext", "No patterns"), read, write, cx)
        });
        let (read, write) = hashbang_source(&binding, first.clone());
        let hashbangs = cx.new(|cx| {
            RecordTable::new(
                &binding,
                TableSpec::list("file-type-hashbangs", "interpreter", "No patterns"),
                read,
                write,
                cx,
            )
        });
        let forward = |view: &Entity<RecordTable>, cx: &mut Context<Self>| {
            cx.subscribe(view, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone()))
        };
        let subscriptions = vec![
            filter_events,
            forward(&patterns, cx),
            forward(&hashbangs, cx),
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
        ];
        Self {
            binding,
            types,
            filter,
            selected: None,
            sourced: first,
            patterns,
            hashbangs,
            _subscriptions: subscriptions,
        }
    }

    pub fn filter(&self) -> &Entity<TextField> {
        &self.filter
    }

    pub fn patterns(&self) -> &Entity<RecordTable> {
        &self.patterns
    }

    pub fn hashbangs(&self) -> &Entity<RecordTable> {
        &self.hashbangs
    }

    /// The languages the filter keeps: by name, or by one of their patterns.
    pub fn shown(&self, cx: &App) -> Vec<&FileType> {
        let query = self.filter.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            return self.types.iter().collect();
        }
        let assoc = associations(&self.binding, cx);
        let keeps = |language: &&FileType| {
            language.name.to_lowercase().contains(&query)
                || effective_patterns(language.patterns(), &assoc)
                    .iter()
                    .any(|listed| listed.pattern.to_lowercase().contains(&query))
        };
        self.types.iter().filter(keeps).collect()
    }

    /// The language whose patterns are shown: the one picked, else the first one listed.
    pub fn selected<'a>(&'a self, cx: &App) -> Option<&'a FileType> {
        let picked = self.selected.as_deref().and_then(|id| self.types.iter().find(|language| language.id == id));
        picked.or_else(|| self.shown(cx).first().copied())
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_owned());
        self.follow_selection(cx);
        cx.notify();
    }

    /// Has the tables show the patterns of the language shown as selected, when it changed.
    fn follow_selection(&mut self, cx: &mut Context<Self>) {
        let shown = self.selected(cx).map(|language| language.id.clone());
        if shown == self.sourced {
            return;
        }
        self.sourced = shown;
        let (read, write) = pattern_source(&self.binding, &self.types, self.sourced.clone());
        self.patterns.update(cx, |table, cx| table.set_source(read, write, cx));
        let (read, write) = hashbang_source(&self.binding, self.sourced.clone());
        self.hashbangs.update(cx, |table, cx| table.set_source(read, write, cx));
    }

    /// The control as text: each language listed (`>` the selected one, `(modified)` one with
    /// patterns of the user), then the patterns of the selected one.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let assoc = associations(&self.binding, cx);
        let selected = self.selected(cx).map(|language| language.id.clone());
        let mut out: Vec<String> = self
            .shown(cx)
            .into_iter()
            .map(|language| {
                let mark = if selected.as_deref() == Some(language.id.as_str()) { ">" } else { " " };
                let modified = if assoc.contains_key(&language.id) { " (modified)" } else { "" };
                format!("{mark} {}{modified}", language.name)
            })
            .collect();
        if let Some(language) = self.selected(cx) {
            out.push(format!("{}:", tx(cx, "File name patterns")));
            out.extend(self.patterns.read(cx).lines(cx));
            out.push(format!("{}:", tx(cx, "Hashbang patterns")));
            out.extend(self.hashbangs.read(cx).lines(cx));
            if let Some(first_line) = &language.first_line {
                out.push(format!("{} {first_line}", tx(cx, "Also recognized by its first line:")));
            }
        }
        out
    }
}

/// The file name patterns of language `id` and how an edit of them is stored.
fn pattern_source(binding: &Binding, types: &Rc<Vec<FileType>>, id: Option<String>) -> (ReadRows, WriteRows) {
    let (reader, read_types, read_id) = (binding.clone(), types.clone(), id.clone());
    let read: ReadRows = Rc::new(move |cx: &App| {
        let Some(language) = read_id.as_deref().and_then(|id| read_types.iter().find(|language| language.id == id))
        else {
            return Vec::new();
        };
        let assoc = associations(&reader, cx);
        effective_patterns(language.patterns(), &assoc).into_iter().map(|listed| vec![listed.pattern]).collect()
    });
    let (writer, write_types) = (binding.clone(), types.clone());
    let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
        let Some(language) = id.as_deref().and_then(|id| write_types.iter().find(|language| language.id == id)) else {
            return;
        };
        let next: Vec<String> = rows.into_iter().filter_map(|row| row.into_iter().next()).collect();
        let all: Vec<LanguagePatterns<'_>> = write_types.iter().map(FileType::patterns).collect();
        let assoc = apply_pattern_edit(&associations(&writer, cx), &all, language.patterns(), &next);
        stage_associations(&writer, &assoc, cx);
    });
    (read, write)
}

/// The hashbang patterns of language `id` and how an edit of them is stored.
fn hashbang_source(binding: &Binding, id: Option<String>) -> (ReadRows, WriteRows) {
    let (reader, read_id) = (binding.clone(), id.clone());
    let read: ReadRows = Rc::new(move |cx: &App| {
        let Some(id) = read_id.as_deref() else { return Vec::new() };
        let assoc = associations(&reader, cx);
        let hashbangs = assoc.get(id).map(|own| own.hashbangs.clone()).unwrap_or_default();
        hashbangs.into_iter().map(|hashbang| vec![hashbang]).collect()
    });
    let writer = binding.clone();
    let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
        let Some(id) = id.as_deref() else { return };
        let next: Vec<String> = rows.into_iter().filter_map(|row| row.into_iter().next()).collect();
        let assoc = set_hashbangs(&associations(&writer, cx), id, &next);
        stage_associations(&writer, &assoc, cx);
    });
    (read, write)
}

impl Render for RecognizedFileTypesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let assoc = associations(&self.binding, cx);
        let selected = self.selected(cx).cloned();
        let selected_bg = theme::color(cx, "bg-selected");
        let fg = theme::color(cx, "fg-icon");
        let modified = tx(cx, "modified");
        let listed: Vec<FileType> = self.shown(cx).into_iter().cloned().collect();
        let rows: Vec<AnyElement> = listed
            .into_iter()
            .map(|language| {
                let id = language.id.clone();
                let selector = format!("settings-file-type-{id}");
                let icon = sample_file_name(language.patterns())
                    .map_or("file-text", |sample| theme::icons::file_icon_name(&sample));
                let chosen = selected.as_ref().is_some_and(|selected| selected.id == language.id);
                let tag = assoc.contains_key(&language.id).then(|| super::muted(cx, &modified));
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(super::ROW_HEIGHT - 4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(chosen, |el| el.bg(selected_bg))
                    .child(theme::icon(cx, icon, 16.0, fg))
                    .child(div().flex_1().min_w_0().whitespace_nowrap().child(SharedString::from(language.name)))
                    .children(tag)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(&id, cx)))
                    .into_any_element()
            })
            .collect();
        let types = div()
            .flex_none()
            .w(px(MEDIUM_FIELD))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                frame(cx, MEDIUM_FIELD, false)
                    .id("settings-file-types-filter")
                    .debug_selector(|| "settings-file-types-filter".to_owned())
                    .child(self.filter.clone()),
            )
            .child(
                div()
                    .id("settings-file-types-list")
                    .h(px(280.0))
                    .overflow_y_scroll()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .children(rows),
            );
        let patterns = selected.map(|language| {
            let matched = |label: &str| self.binding.matches(label, cx);
            let patterns_label = tx(cx, "File name patterns");
            let hashbangs_label = tx(cx, "Hashbang patterns");
            let first_line = language.first_line.map(|first_line| {
                let note = tx(cx, "Also recognized by its first line:");
                div()
                    .flex()
                    .gap(px(4.0))
                    .px(px(4.0))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(note))
                    .child(div().font_family("monospace").child(SharedString::from(first_line)))
            });
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(row(cx, matched("File name patterns")).child(row_label(cx, &patterns_label, &query)))
                .child(self.patterns.clone())
                .child(hint_line(cx, &tx(cx, "* matches any characters, ? one (e.g. *.foo, Jenkinsfile)."), 4.0))
                .child(row(cx, matched("Hashbang patterns")).child(row_label(cx, &hashbangs_label, &query)))
                .child(self.hashbangs.clone())
                .child(hint_line(
                    cx,
                    &tx(cx, "Interpreters run by a #! first line, e.g. node for #!/usr/bin/env node."),
                    4.0,
                ))
                .children(first_line)
        });
        div().flex().gap(px(16.0)).child(types).children(patterns)
    }
}

/// Recognized File Types over the backend's languages.
pub fn recognized_file_types(binding: Binding, cx: &mut App) -> Entity<RecognizedFileTypesView> {
    let types = backend_file_types(cx);
    cx.new(|cx| RecognizedFileTypesView::new(binding, types, cx))
}

/// The value the setting holds after an edit, as stored (for the tests).
pub fn stored_associations(value: Option<&Value>) -> FileTypeAssociations {
    value.and_then(|value| serde_json::from_value(value.clone()).ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::json;

    fn types() -> Vec<FileType> {
        let language = |id: &str, name: &str, extensions: &[&str], first_line: Option<&str>| FileType {
            id: id.to_owned(),
            name: name.to_owned(),
            extensions: extensions.iter().map(|extension| (*extension).to_owned()).collect(),
            filenames: Vec::new(),
            first_line: first_line.map(str::to_owned),
        };
        vec![
            language("javascript", "JavaScript", &["js", "mjs"], Some("^#!.*\\bnode")),
            language("python", "Python", &["py"], None),
        ]
    }

    // Not from a spec: the first language is selected; a pattern added to Python is stored as an
    // addition and a built-in one removed is stored as removed; the language is marked modified.
    #[gpui_kit::test]
    async fn patterns_added_and_removed_are_stored_as_additions_and_removals(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, cx| RecognizedFileTypesView::new(binding, types(), cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[..2], ["> JavaScript".to_owned(), "  Python".to_owned()]);
        assert!(lines.contains(&"Also recognized by its first line: ^#!.*\\bnode".to_owned()), "{lines:?}");
        click(cx, "settings-file-type-python");
        click(cx, "settings-table-file-type-patterns-add");
        cx.simulate_input("*.pyw");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        click(cx, "settings-table-file-type-patterns-row-0");
        click(cx, "settings-table-file-type-patterns-remove");
        let staged = staged(&model, cx);
        let (key, value) = staged.last().cloned().expect("an edit is staged");
        assert_eq!(key, "project:files.fileTypes");
        let assoc = stored_associations(serde_json::from_str::<Value>(&value).ok().as_ref());
        let python = assoc.get("python").cloned().unwrap_or_default();
        assert_eq!((python.patterns, python.removed_patterns), (vec!["*.pyw".to_owned()], vec!["*.py".to_owned()]));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.contains(&"> Python (modified)".to_owned()), "{lines:?}");
    }

    // Not from a spec: the filter keeps the languages whose name or a pattern contains its text;
    // hashbangs are stored for the language shown.
    #[gpui_kit::test]
    async fn the_filter_matches_names_and_patterns_and_hashbangs_are_stored(cx: &mut TestAppContext) {
        let stored = json!({ "python": { "patterns": ["SConstruct"] } });
        let model = cx.update(|cx| dialog_model(PAGE, &[(FILE_TYPES_KEY, stored)], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| RecognizedFileTypesView::new(binding, types(), cx));
        let filter = view.read_with(cx, |view, _| view.filter().clone());
        type_over(&filter, "scons", cx);
        let shown = view.read_with(cx, |view, cx| view.shown(cx).iter().map(|l| l.id.clone()).collect::<Vec<_>>());
        assert_eq!(shown, ["python"]);
        click(cx, "settings-table-file-type-hashbangs-add");
        cx.simulate_input("python3");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let staged = staged(&model, cx);
        let assoc = stored_associations(serde_json::from_str::<Value>(&staged[0].1).ok().as_ref());
        let python = assoc.get("python").cloned().unwrap_or_default();
        assert_eq!((python.patterns, python.hashbangs), (vec!["SConstruct".to_owned()], vec!["python3".to_owned()]));
    }
}
