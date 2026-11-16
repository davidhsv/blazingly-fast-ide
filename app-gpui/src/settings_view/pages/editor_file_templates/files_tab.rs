//! The Files tab: the templates of New ▸, each enabled or not, Add and Remove; the selected one's
//! name, extension, file name style, kinds (each with its label and text), Reformat according to
//! style and Restore Default.

use super::{FileTemplatesPageView, NAME_STYLES, now_base36};
use crate::settings_view::controls::{check_box, tx};
use crate::settings_view::pages::{drop_down, drop_down_row, icon_button, select_box};
use crate::templates::files_model::{FileTemplate, FileTemplateKind, NameStyle, reformats_on_create};
use crate::theme;
use crate::ui::text_area::TextArea;
use crate::ui::text_field::TextField;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, MouseButton, SharedString, Subscription, div,
    prelude::*, px,
};

/// A single-line field of the Files tab's editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileField {
    Name,
    Extension,
    KindLabel,
}

/// The fields of the template being edited, for the kind shown.
pub(super) struct FileFields {
    name: Entity<TextField>,
    extension: Entity<TextField>,
    kind_label: Option<Entity<TextField>>,
    body: Option<Entity<TextArea>>,
    _events: Vec<Subscription>,
}

/// What the Files tab shows.
#[derive(Default)]
pub(super) struct FilesState {
    pub(super) selected: Option<String>,
    /// The kind shown, by index.
    pub(super) kind: usize,
    pub(super) style_open: bool,
    fields: Option<FileFields>,
}

impl FilesState {
    pub(super) fn clear(&mut self) {
        self.fields = None;
    }
}

/// The title of `style`.
fn style_title(style: NameStyle) -> &'static str {
    NAME_STYLES.iter().find(|(each, _, _)| *each == style).map_or("As typed", |(_, _, title)| *title)
}

impl FileTemplatesPageView {
    fn selected_file(&self) -> Option<&FileTemplate> {
        let id = self.files.selected.as_deref()?;
        self.store.templates().iter().find(|template| template.id == id)
    }

    /// The id of the template being edited.
    pub fn selected_file_template(&self) -> Option<&str> {
        self.selected_file().map(|template| template.id.as_str())
    }

    /// The kind shown of `template`, by index: the last one when fewer are left.
    fn kind_of(&self, template: &FileTemplate) -> usize {
        self.files.kind.min(template.kinds.len().saturating_sub(1))
    }

    pub fn file_field(&self, field: FileField) -> Option<Entity<TextField>> {
        let fields = self.files.fields.as_ref()?;
        match field {
            FileField::Name => Some(fields.name.clone()),
            FileField::Extension => Some(fields.extension.clone()),
            FileField::KindLabel => fields.kind_label.clone(),
        }
    }

    /// The text of the kind shown.
    pub fn file_body(&self) -> Option<Entity<TextArea>> {
        self.files.fields.as_ref().and_then(|fields| fields.body.clone())
    }

    /// Replaces the templates plugins contribute, listed like the built-in ones; the editor of one
    /// that is no longer contributed is emptied.
    pub fn set_contributed_templates(&mut self, templates: Vec<FileTemplate>, cx: &mut Context<Self>) {
        self.store.set_contributed(templates);
        if self.selected_file().is_none() && self.files.fields.is_some() {
            self.make_file_fields(cx);
        }
        cx.notify();
    }

    /// The fields for the selected template and the kind shown.
    pub(super) fn make_file_fields(&mut self, cx: &mut Context<Self>) {
        self.files.fields = None;
        let Some(template) = self.selected_file().cloned() else { return };
        let kind = template.kinds.get(self.kind_of(&template)).cloned();
        let name = Self::text_field(&template.name, None, false, cx);
        let extension = Self::text_field(&template.extension, Some(tx(cx, "none")), false, cx);
        let mut events = vec![
            Self::watch_field(&name, Self::name_changed, cx),
            Self::watch_field(&extension, Self::extension_changed, cx),
        ];
        let (kind_label, body) = match kind {
            Some(kind) => {
                let label = Self::text_field(&kind.label, None, false, cx);
                let body = Self::text_area(&kind.body, cx);
                events.push(Self::watch_field(&label, Self::kind_label_changed, cx));
                events.push(Self::watch_area(&body, Self::file_body_changed, cx));
                (Some(label), Some(body))
            }
            None => (None, None),
        };
        self.files.fields = Some(FileFields { name, extension, kind_label, body, _events: events });
    }

    /// Saves the selected template as `change` leaves it.
    fn update_file(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FileTemplate)) {
        let Some(mut template) = self.selected_file().cloned() else { return };
        change(&mut template);
        let write = self.store.save_template(&template);
        self.persist(vec![write], cx);
        cx.notify();
    }

    /// Saves the selected template with `change` made to the kind shown.
    fn update_kind(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FileTemplateKind)) {
        let Some(index) = self.selected_file().map(|template| self.kind_of(template)) else { return };
        self.update_file(cx, |template| {
            if let Some(kind) = template.kinds.get_mut(index) {
                change(kind);
            }
        });
    }

    fn name_changed(&mut self, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let name = field.read(cx).text().to_owned();
        self.update_file(cx, |template| template.name = name);
    }

    /// The extension is stored without a leading dot and the blanks around it, and shown so.
    fn extension_changed(&mut self, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let typed = field.read(cx).text().to_owned();
        let extension = typed.strip_prefix('.').unwrap_or(&typed).trim().to_owned();
        if extension != typed {
            field.update(cx, |field, cx| field.set_text(&extension, cx));
        }
        self.update_file(cx, |template| template.extension = extension);
    }

    fn kind_label_changed(&mut self, field: &Entity<TextField>, cx: &mut Context<Self>) {
        let label = field.read(cx).text().to_owned();
        self.update_kind(cx, |kind| kind.label = label);
    }

    fn file_body_changed(&mut self, text: String, cx: &mut Context<Self>) {
        self.update_kind(cx, |kind| kind.body = text);
    }

    pub fn select_template(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.files.selected.as_deref() != Some(id) {
            self.files.selected = Some(id.to_owned());
            self.files.kind = 0;
            self.make_file_fields(cx);
        }
        cx.notify();
    }

    /// Turns template `id` on or off in New ▸.
    pub fn toggle_template_enabled(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(mut template) = self.store.templates().iter().find(|template| template.id == id).cloned() else {
            return;
        };
        template.enabled = Some(template.enabled == Some(false));
        let write = self.store.save_template(&template);
        self.persist(vec![write], cx);
        cx.notify();
    }

    /// Add Template: "New Template", a `.txt` file named as typed with one Default kind, selected.
    pub fn add_template(&mut self, cx: &mut Context<Self>) {
        let template = FileTemplate {
            id: format!("user.{}", now_base36()),
            name: "New Template".to_owned(),
            icon: "file".to_owned(),
            extension: "txt".to_owned(),
            name_style: NameStyle::Asis,
            kinds: vec![FileTemplateKind {
                id: "default".to_owned(),
                label: "Default".to_owned(),
                body: "$END$\n".to_owned(),
                extension: None,
            }],
            ..FileTemplate::default()
        };
        let write = self.store.save_template(&template);
        self.persist(vec![write], cx);
        (self.files.selected, self.files.kind) = (Some(template.id), 0);
        self.make_file_fields(cx);
        cx.notify();
    }

    /// Remove Template: the user's template goes, a built-in one is hidden; nothing is selected.
    pub fn remove_template(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_file().map(|template| template.id.clone()) else { return };
        let write = self.store.remove_template(&id);
        self.persist(vec![write], cx);
        self.files.selected = None;
        self.make_file_fields(cx);
        cx.notify();
    }

    /// Restore Default: the selected built-in template as it ships, its fields showing it.
    pub fn restore_template(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_file().map(|template| template.id.clone()) else { return };
        if !self.store.is_modified_template(&id) {
            return;
        }
        let write = self.store.reset_template(&id);
        self.persist(vec![write], cx);
        self.make_file_fields(cx);
        cx.notify();
    }

    pub fn set_name_style(&mut self, style: NameStyle, cx: &mut Context<Self>) {
        self.files.style_open = false;
        self.update_file(cx, |template| template.name_style = style);
    }

    pub fn select_kind(&mut self, index: usize, cx: &mut Context<Self>) {
        self.files.kind = index;
        self.make_file_fields(cx);
        cx.notify();
    }

    /// Add Kind: "New kind" after the others, shown.
    pub fn add_kind(&mut self, cx: &mut Context<Self>) {
        let Some(count) = self.selected_file().map(|template| template.kinds.len()) else { return };
        let kind = FileTemplateKind {
            id: format!("kind{}", now_base36()),
            label: "New kind".to_owned(),
            body: "$END$\n".to_owned(),
            extension: None,
        };
        self.update_file(cx, |template| template.kinds.push(kind));
        self.select_kind(count, cx);
    }

    /// Remove Kind: the kind shown goes, the first one is shown; a template keeps at least one.
    pub fn remove_kind(&mut self, cx: &mut Context<Self>) {
        let Some(template) = self.selected_file() else { return };
        if template.kinds.len() < 2 {
            return;
        }
        let index = self.kind_of(template);
        self.update_file(cx, |template| {
            template.kinds.remove(index);
        });
        self.select_kind(0, cx);
    }

    /// Reformat according to style, for the selected template.
    pub fn toggle_reformat(&mut self, cx: &mut Context<Self>) {
        self.update_file(cx, |template| template.reformat = Some(!reformats_on_create(template)));
    }

    /// The tab as text: `[x] Name` per template (`>` before the selected one, ` *` after a changed
    /// built-in), then the editor's rows, or the empty text.
    pub(super) fn files_lines(&self, cx: &App) -> Vec<String> {
        let mark = |on: bool| if on { "[x]" } else { "[ ]" };
        let mut out: Vec<String> = self
            .store
            .templates()
            .iter()
            .map(|template| {
                let chosen = if self.files.selected.as_deref() == Some(template.id.as_str()) { ">" } else { " " };
                let changed = if self.store.is_modified_template(&template.id) { " *" } else { "" };
                format!("{chosen} {} {}{changed}", mark(template.enabled != Some(false)), template.name)
            })
            .collect();
        let (Some(template), Some(fields)) = (self.selected_file(), self.files.fields.as_ref()) else {
            out.push(tx(cx, super::EMPTY));
            return out;
        };
        let text = |field: &Entity<TextField>| field.read(cx).text().to_owned();
        let index = self.kind_of(template);
        out.push(format!("{}: {}", tx(cx, "Name"), text(&fields.name)));
        let style = tx(cx, style_title(template.name_style));
        let extension = text(&fields.extension);
        out.push(format!("{}: {extension} | {}: {style} ▾", tx(cx, "Extension"), tx(cx, "File name")));
        let kinds: Vec<String> = template
            .kinds
            .iter()
            .enumerate()
            .map(|(i, kind)| if i == index { format!("[{}]", kind.label) } else { kind.label.clone() })
            .collect();
        out.push(format!("{}: {}", tx(cx, "Kinds"), kinds.join(" | ")));
        if let (Some(label), Some(body)) = (&fields.kind_label, &fields.body) {
            out.push(format!("{}: {}", tx(cx, "Kind label"), text(label)));
            out.push(format!("{}: {}", tx(cx, "Template body"), body.read(cx).text().replace('\n', "⏎")));
        }
        out.push(format!("{} {}", mark(reformats_on_create(template)), tx(cx, "Reformat according to style")));
        if self.store.is_base_template(&template.id) && self.store.is_modified_template(&template.id) {
            out.push(format!("[{}]", tx(cx, super::RESTORE)));
        }
        out
    }

    fn file_item(&self, template: &FileTemplate, cx: &mut Context<Self>) -> AnyElement {
        let id = template.id.clone();
        let chosen = self.files.selected.as_deref() == Some(id.as_str());
        let on = template.enabled != Some(false);
        let color = if !on {
            "fg-faint"
        } else if self.store.is_modified_template(&id) {
            "link"
        } else {
            "fg"
        };
        let enable = format!("settings-filetemplates-enabled-{id}");
        let toggled = id.clone();
        Self::list_row(cx, format!("settings-filetemplates-item-{id}"), chosen)
            .child(
                div()
                    .id(SharedString::from(enable.clone()))
                    .debug_selector(move || enable)
                    .child(check_box(cx, on))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.toggle_template_enabled(&toggled, cx);
                    })),
            )
            .child(theme::icon(cx, &template.icon, 14.0, theme::color(cx, "fg-icon")))
            .child(div().text_color(theme::color(cx, color)).child(SharedString::from(template.name.clone())))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_template(&id, cx)))
            .into_any_element()
    }

    fn style_select(&self, template: &FileTemplate, cx: &mut Context<Self>) -> AnyElement {
        let current = template.name_style;
        let choices = self.files.style_open.then(|| {
            let rows: Vec<AnyElement> = NAME_STYLES
                .iter()
                .map(|&(style, name, title)| {
                    let selector = format!("settings-filetemplates-style-{name}");
                    let text = tx(cx, title);
                    drop_down_row(cx, selector, &text, style == current)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_name_style(style, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let shown = tx(cx, style_title(current));
        div()
            .relative()
            .child(
                select_box(cx, &shown)
                    .id("settings-filetemplates-style")
                    .debug_selector(|| "settings-filetemplates-style".to_owned())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.files.style_open = !view.files.style_open;
                        cx.notify();
                    })),
            )
            .children(choices)
            .into_any_element()
    }

    fn kind_tabs(&self, template: &FileTemplate, cx: &mut Context<Self>) -> AnyElement {
        let index = self.kind_of(template);
        let tabs: Vec<AnyElement> = template
            .kinds
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                let selector = format!("settings-filetemplates-kind-{i}");
                let active = i == index;
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(theme::length(cx, "h-control-small"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(theme::color(cx, if active { "fg-strong" } else { "fg-muted" }))
                    .when(active, |el| el.bg(theme::color(cx, "bg-selected")))
                    .when(!active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(SharedString::from(kind.label.clone()))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_kind(i, cx)))
                    .into_any_element()
            })
            .collect();
        let removable = template.kinds.len() >= 2;
        let add = icon_button(cx, "settings-filetemplates-add-kind".to_owned(), "plus", "Add Kind")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_kind(cx)));
        let remove = icon_button(cx, "settings-filetemplates-remove-kind".to_owned(), "minus", "Remove Kind")
            .when(!removable, |el| el.opacity(0.5))
            .when(removable, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove_kind(cx))));
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(92.0))
                    .flex_none()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, "Kinds"))),
            )
            .child(div().flex_1().flex().flex_wrap().gap(px(2.0)).children(tabs))
            .child(add)
            .child(remove)
            .into_any_element()
    }

    fn file_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let (Some(template), Some(fields)) = (self.selected_file().cloned(), self.files.fields.as_ref()) else {
            return Self::empty(cx);
        };
        let (name, extension) = (fields.name.clone(), fields.extension.clone());
        let (kind_label, body) = (fields.kind_label.clone(), fields.body.clone());
        let mut rows = vec![Self::labelled(cx, "Name", "settings-filetemplates-name", name.into_any_element(), false)];
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(92.0))
                        .flex_none()
                        .text_color(theme::color(cx, "fg-muted"))
                        .child(SharedString::from(tx(cx, "Extension"))),
                )
                .child(
                    Self::frame(cx, "settings-filetemplates-extension", extension.into_any_element(), true)
                        .w(px(120.0)),
                )
                .child(
                    div()
                        .w(px(92.0))
                        .flex_none()
                        .text_color(theme::color(cx, "fg-muted"))
                        .child(SharedString::from(tx(cx, "File name"))),
                )
                .child(self.style_select(&template, cx))
                .into_any_element(),
        );
        rows.push(self.kind_tabs(&template, cx));
        if let (Some(label), Some(body)) = (kind_label, body) {
            let label = label.into_any_element();
            rows.push(Self::labelled(cx, "Kind label", "settings-filetemplates-kind-label", label, false));
            rows.push(Self::body_frame(cx, "settings-filetemplates-body", &body));
        }
        rows.push(
            div()
                .id("settings-filetemplates-reformat")
                .debug_selector(|| "settings-filetemplates-reformat".to_owned())
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(check_box(cx, reformats_on_create(&template)))
                .child(SharedString::from(tx(cx, "Reformat according to style")))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_reformat(cx)))
                .into_any_element(),
        );
        let plain = |text: &str| (false, tx(cx, text));
        let code = |text: &str| (true, text.to_owned());
        let mut hint = vec![plain("Variables:")];
        hint.extend(
            ["$NAME$", "$PACKAGE$", "$FILE_NAME$", "$PROJECT_NAME$", "$USER$", "$DATE$", "$TIME$", "$YEAR$"].map(code),
        );
        hint.extend([
            plain("(or"),
            code("${NAME}"),
            (false, "…);".to_owned()),
            code("$END$"),
            plain("places the caret. A line with"),
            code("$PACKAGE$"),
            plain("is omitted (with one following blank line) when there is no package."),
            code("#parse(\"File Header\")"),
            plain("inserts a template from the Includes or Code tab."),
        ]);
        rows.push(Self::hint(cx, &hint));
        if self.store.is_base_template(&template.id) && self.store.is_modified_template(&template.id) {
            rows.push(Self::restore_button("settings-filetemplates-restore", Self::restore_template, cx));
        }
        div().flex().flex_col().gap(px(8.0)).children(rows).into_any_element()
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let removable = self.selected_file().is_some();
        let add = icon_button(cx, "settings-filetemplates-add".to_owned(), "plus", "Add Template")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_template(cx)));
        let remove = icon_button(cx, "settings-filetemplates-remove".to_owned(), "minus", "Remove Template")
            .when(!removable, |el| el.opacity(0.5))
            .when(removable, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove_template(cx))));
        let toolbar = div().flex().items_center().gap(px(2.0)).child(add).child(remove).into_any_element();
        let templates = self.store.templates().to_vec();
        let rows: Vec<AnyElement> = templates.iter().map(|template| self.file_item(template, cx)).collect();
        let side = Self::side(cx, Some(toolbar), "settings-filetemplates-items", rows);
        let editor = self.file_editor(cx);
        Self::split(cx, side, editor)
    }
}
