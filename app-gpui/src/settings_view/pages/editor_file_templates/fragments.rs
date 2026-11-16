//! The Includes and Code tabs: the fragments file templates insert with `#parse("…")`. Includes can
//! be added, renamed (a built-in one keeps its name), removed and restored; code templates, the
//! members the test templates generate, are edited and restored.

use super::FileTemplatesPageView;
use crate::settings_view::controls::tx;
use crate::settings_view::pages::icon_button;
use crate::templates::files_model::TemplateFragment;
use crate::templates::files_store::is_builtin_include;
use crate::theme;
use crate::ui::text_area::TextArea;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};

/// The fields of the include being edited.
pub(super) struct IncludeFields {
    name: Entity<TextField>,
    body: Entity<TextArea>,
    /// Leaving the name field commits the name typed; made once the page is drawn.
    blur: Option<Subscription>,
    _events: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct IncludesState {
    pub(super) selected: Option<String>,
    fields: Option<IncludeFields>,
}

impl IncludesState {
    pub(super) fn clear(&mut self) {
        self.fields = None;
    }
}

/// The text of the code template being edited.
pub(super) struct CodeFields {
    body: Entity<TextArea>,
    _events: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct CodeState {
    pub(super) selected: Option<String>,
    fields: Option<CodeFields>,
}

impl CodeState {
    pub(super) fn clear(&mut self) {
        self.fields = None;
    }
}

const PARSE_INCLUDE: &str = "File templates insert this with {code}, indented like the directive; a directive line \
                             whose include is empty is dropped. The file template variables apply.";
const PARSE_CODE: &str = "Inserted where a file template says {code}.";

/// The directive that inserts fragment `name`.
fn parse_directive(name: &str) -> String {
    format!("#parse(\"{name}\")")
}

/// The name of an include added next to `includes`: "New Include", else the first free "New Include N".
fn new_include_name(includes: &[TemplateFragment]) -> String {
    let taken = |name: &str| includes.iter().any(|include| include.name == name);
    let mut name = "New Include".to_owned();
    let mut n = 2;
    while taken(&name) {
        name = format!("New Include {n}");
        n += 1;
    }
    name
}

impl FileTemplatesPageView {
    fn selected_include(&self) -> Option<&TemplateFragment> {
        let name = self.includes.selected.as_deref()?;
        self.store.includes().iter().find(|include| include.name == name)
    }

    fn selected_code(&self) -> Option<&TemplateFragment> {
        let name = self.code.selected.as_deref()?;
        self.store.code_templates().iter().find(|code| code.name == name)
    }

    /// The name of the include being edited.
    pub fn selected_include_name(&self) -> Option<&str> {
        self.selected_include().map(|include| include.name.as_str())
    }

    pub fn include_name_field(&self) -> Option<Entity<TextField>> {
        self.includes.fields.as_ref().map(|fields| fields.name.clone())
    }

    pub fn include_body(&self) -> Option<Entity<TextArea>> {
        self.includes.fields.as_ref().map(|fields| fields.body.clone())
    }

    pub fn code_body(&self) -> Option<Entity<TextArea>> {
        self.code.fields.as_ref().map(|fields| fields.body.clone())
    }

    pub(super) fn make_include_fields(&mut self, cx: &mut Context<Self>) {
        self.includes.fields = None;
        let Some(include) = self.selected_include().cloned() else { return };
        // A built-in include keeps its name: `#parse` finds it by that name.
        let name = Self::text_field(&include.name, None, is_builtin_include(&include.name), cx);
        let body = Self::text_area(&include.body, cx);
        let events = vec![
            cx.subscribe(&name, |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {}
                TextFieldEvent::Submitted => {
                    view.commit_include_name(cx);
                    cx.emit(super::ControlEvent::Submit);
                }
                TextFieldEvent::Cancelled => cx.emit(super::ControlEvent::Cancel),
            }),
            Self::watch_area(&body, Self::include_body_changed, cx),
        ];
        self.includes.fields = Some(IncludeFields { name, body, blur: None, _events: events });
    }

    /// Leaving the name field commits it: the page is drawn with a window, which the focus
    /// listener needs, so it is attached there.
    pub(super) fn watch_include_name_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fields) = self.includes.fields.as_mut() else { return };
        if fields.blur.is_none() {
            let focus = fields.name.focus_handle(cx);
            fields.blur = Some(cx.on_blur(&focus, window, |view, _, cx| view.commit_include_name(cx)));
        }
    }

    /// Commits the name typed for the include being edited: a new name that is neither empty nor
    /// taken renames it, anything else puts its name back in the field.
    pub(super) fn commit_include_name(&mut self, cx: &mut Context<Self>) {
        let (Some(include), Some(fields)) = (self.selected_include().cloned(), self.includes.fields.as_ref()) else {
            return;
        };
        let field = fields.name.clone();
        let next = field.read(cx).text().trim().to_owned();
        let taken = self.store.includes().iter().any(|other| other.name == next);
        if next.is_empty() || next == include.name || taken {
            if field.read(cx).text() != include.name {
                field.update(cx, |field, cx| field.set_text(&include.name, cx));
            }
            return;
        }
        let renamed = TemplateFragment { name: next.clone(), ..include.clone() };
        let write = self.store.save_include(&renamed, &include.name);
        self.persist(vec![write], cx);
        self.includes.selected = Some(next.clone());
        if field.read(cx).text() != next {
            field.update(cx, |field, cx| field.set_text(&next, cx));
        }
        cx.notify();
    }

    fn include_body_changed(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(include) = self.selected_include().cloned() else { return };
        let write = self.store.save_include(&TemplateFragment { body: text, ..include.clone() }, &include.name);
        self.persist(vec![write], cx);
        cx.notify();
    }

    pub fn select_include(&mut self, name: &str, cx: &mut Context<Self>) {
        self.commit_include_name(cx);
        if self.includes.selected.as_deref() != Some(name) {
            self.includes.selected = Some(name.to_owned());
            self.make_include_fields(cx);
        }
        cx.notify();
    }

    /// Add Include: an empty include under the first free "New Include" name, selected.
    pub fn add_include(&mut self, cx: &mut Context<Self>) {
        self.commit_include_name(cx);
        let name = new_include_name(self.store.includes());
        let include = TemplateFragment { name: name.clone(), body: String::new(), description: None };
        let write = self.store.save_include(&include, &name);
        self.persist(vec![write], cx);
        self.includes.selected = Some(name);
        self.make_include_fields(cx);
        cx.notify();
    }

    /// Remove Include: the user's include goes, a built-in one is hidden; nothing is selected.
    pub fn remove_include(&mut self, cx: &mut Context<Self>) {
        self.commit_include_name(cx);
        let Some(name) = self.selected_include().map(|include| include.name.clone()) else { return };
        let write = self.store.remove_include(&name);
        self.persist(vec![write], cx);
        self.includes.selected = None;
        self.make_include_fields(cx);
        cx.notify();
    }

    /// Restore Default: the selected built-in include as it ships, its fields showing it.
    pub fn restore_include(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected_include().map(|include| include.name.clone()) else { return };
        if !self.store.is_modified_include(&name) {
            return;
        }
        let write = self.store.reset_include(&name);
        self.persist(vec![write], cx);
        self.make_include_fields(cx);
        cx.notify();
    }

    pub(super) fn make_code_fields(&mut self, cx: &mut Context<Self>) {
        self.code.fields = None;
        let Some(code) = self.selected_code().cloned() else { return };
        let body = Self::text_area(&code.body, cx);
        let events = vec![Self::watch_area(&body, Self::code_body_changed, cx)];
        self.code.fields = Some(CodeFields { body, _events: events });
    }

    fn code_body_changed(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(name) = self.selected_code().map(|code| code.name.clone()) else { return };
        let write = self.store.save_code_template(&name, &text);
        self.persist(vec![write], cx);
        cx.notify();
    }

    pub fn select_code(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.code.selected.as_deref() != Some(name) {
            self.code.selected = Some(name.to_owned());
            self.make_code_fields(cx);
        }
        cx.notify();
    }

    /// Restore Default: the selected code template as it ships, its text showing it.
    pub fn restore_code(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected_code().map(|code| code.name.clone()) else { return };
        let write = self.store.reset_code_template(&name);
        self.persist(vec![write], cx);
        self.make_code_fields(cx);
        cx.notify();
    }

    /// A tab's list as text: `>` before the selected entry, ` *` after a changed built-in one.
    fn fragment_lines(
        list: &[TemplateFragment],
        selected: Option<&str>,
        modified: &dyn Fn(&str) -> bool,
    ) -> Vec<String> {
        list.iter()
            .map(|fragment| {
                let chosen = if selected == Some(fragment.name.as_str()) { ">" } else { " " };
                let changed = if modified(&fragment.name) { " *" } else { "" };
                format!("{chosen} {}{changed}", fragment.name)
            })
            .collect()
    }

    pub(super) fn includes_lines(&self, cx: &App) -> Vec<String> {
        let modified = |name: &str| self.store.is_modified_include(name);
        let mut out = Self::fragment_lines(self.store.includes(), self.includes.selected.as_deref(), &modified);
        let (Some(include), Some(fields)) = (self.selected_include(), self.includes.fields.as_ref()) else {
            out.push(tx(cx, super::EMPTY));
            return out;
        };
        let locked = if is_builtin_include(&include.name) { " (disabled)" } else { "" };
        out.push(format!("{}: {}{locked}", tx(cx, "Name"), fields.name.read(cx).text()));
        out.push(format!("{}: {}", tx(cx, "Include body"), fields.body.read(cx).text().replace('\n', "⏎")));
        out.push(Self::with_code_line(cx, PARSE_INCLUDE, &parse_directive(&include.name)));
        if is_builtin_include(&include.name) && self.store.is_modified_include(&include.name) {
            out.push(format!("[{}]", tx(cx, super::RESTORE)));
        }
        out
    }

    pub(super) fn code_lines(&self, cx: &App) -> Vec<String> {
        let modified = |name: &str| self.store.is_modified_code_template(name);
        let mut out = Self::fragment_lines(self.store.code_templates(), self.code.selected.as_deref(), &modified);
        let (Some(code), Some(fields)) = (self.selected_code(), self.code.fields.as_ref()) else {
            out.push(tx(cx, super::EMPTY));
            return out;
        };
        if let Some(description) = &code.description {
            out.push(format!("{} {}", tx(cx, "Used by"), tx(cx, description)));
        }
        let body = fields.body.read(cx).text().replace('\n', "⏎");
        out.push(format!("{}: {body}", tx(cx, "Code template body")));
        out.push(Self::with_code_line(cx, PARSE_CODE, &parse_directive(&code.name)));
        if self.store.is_modified_code_template(&code.name) {
            out.push(format!("[{}]", tx(cx, super::RESTORE)));
        }
        out
    }

    /// The rows of a fragment list, each drawn as `prefix` and its name, a click selecting it.
    fn fragment_rows(
        list: &[TemplateFragment],
        selected: Option<&str>,
        prefix: &str,
        modified: &dyn Fn(&str) -> bool,
        pick: fn(&mut Self, &str, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        list.iter()
            .map(|fragment| {
                let name = fragment.name.clone();
                let color = if modified(&name) { "link" } else { "fg" };
                Self::list_row(cx, format!("{prefix}{name}"), selected == Some(name.as_str()))
                    .child(theme::icon(cx, "file", 14.0, theme::color(cx, "fg-icon")))
                    .child(div().text_color(theme::color(cx, color)).child(SharedString::from(name.clone())))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| pick(view, &name, cx)))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_includes(&self, cx: &mut Context<Self>) -> AnyElement {
        let removable = self.selected_include().is_some();
        let add = icon_button(cx, "settings-includes-add".to_owned(), "plus", "Add Include")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_include(cx)));
        let remove = icon_button(cx, "settings-includes-remove".to_owned(), "minus", "Remove Include")
            .when(!removable, |el| el.opacity(0.5))
            .when(removable, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove_include(cx))));
        let toolbar = div().flex().items_center().gap(px(2.0)).child(add).child(remove).into_any_element();
        let includes = self.store.includes().to_vec();
        let modified = |name: &str| self.store.is_modified_include(name);
        let selected = self.includes.selected.clone();
        let prefix = "settings-includes-item-";
        let rows = Self::fragment_rows(&includes, selected.as_deref(), prefix, &modified, Self::select_include, cx);
        let side = Self::side(cx, Some(toolbar), Self::scrolled_rows("settings-includes-items", rows));
        let editor = match (self.selected_include().cloned(), self.includes.fields.as_ref()) {
            (Some(include), Some(fields)) => {
                let body = fields.body.clone();
                // A built-in include's name cannot change: it is drawn as a disabled field, its text
                // faint on no background.
                let name_row = if is_builtin_include(&include.name) {
                    let fixed = div()
                        .text_color(theme::color(cx, "fg-faint"))
                        .child(SharedString::from(include.name.clone()))
                        .into_any_element();
                    let frame = Self::frame(cx, "settings-includes-name", fixed, false)
                        .flex_1()
                        .bg(gpui_kit::transparent_black())
                        .border_color(theme::color(cx, "border"));
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(92.0))
                                .flex_none()
                                .text_color(theme::color(cx, "fg-muted"))
                                .child(SharedString::from(tx(cx, "Name"))),
                        )
                        .child(frame)
                        .into_any_element()
                } else {
                    Self::labelled(cx, "Name", "settings-includes-name", fields.name.clone().into_any_element(), false)
                };
                let mut rows = vec![
                    name_row,
                    Self::body_frame(cx, "settings-includes-body", &body),
                    Self::with_code(cx, PARSE_INCLUDE, &parse_directive(&include.name)),
                ];
                if is_builtin_include(&include.name) && self.store.is_modified_include(&include.name) {
                    rows.push(Self::restore_button("settings-includes-restore", Self::restore_include, cx));
                }
                div().flex().flex_col().gap(px(8.0)).children(rows).into_any_element()
            }
            _ => Self::empty(cx),
        };
        Self::split(cx, side, editor)
    }

    pub(super) fn render_code(&self, cx: &mut Context<Self>) -> AnyElement {
        let code = self.store.code_templates().to_vec();
        let modified = |name: &str| self.store.is_modified_code_template(name);
        let selected = self.code.selected.clone();
        let prefix = "settings-code-item-";
        let rows = Self::fragment_rows(&code, selected.as_deref(), prefix, &modified, Self::select_code, cx);
        let side = Self::side(cx, None, Self::scrolled_rows("settings-code-items", rows));
        let editor = match (self.selected_code().cloned(), self.code.fields.as_ref()) {
            (Some(code), Some(fields)) => {
                let body = fields.body.clone();
                let mut rows = Vec::new();
                if let Some(description) = &code.description {
                    let used_by = format!("{} {}", tx(cx, "Used by"), tx(cx, description));
                    rows.push(Self::hint(cx, &[(false, used_by)]));
                }
                rows.push(Self::body_frame(cx, "settings-code-body", &body));
                rows.push(Self::with_code(cx, PARSE_CODE, &parse_directive(&code.name)));
                if self.store.is_modified_code_template(&code.name) {
                    rows.push(Self::restore_button("settings-code-restore", Self::restore_code, cx));
                }
                div().flex().flex_col().gap(px(8.0)).children(rows).into_any_element()
            }
            _ => Self::empty(cx),
        };
        Self::split(cx, side, editor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: an include added next to "New Include" and "New Include 2" is "New Include 3".
    #[test]
    fn a_new_include_takes_the_first_free_name() {
        let named = |name: &str| TemplateFragment { name: name.to_owned(), ..TemplateFragment::default() };
        assert_eq!(new_include_name(&[named("File Header")]), "New Include");
        assert_eq!(new_include_name(&[named("New Include"), named("New Include 2")]), "New Include 3");
        assert_eq!(parse_directive("File Header"), "#parse(\"File Header\")");
    }
}
