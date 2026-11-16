//! Editor › File and Code Templates: the Files, Includes and Code tabs of the scheme the project
//! uses (Default, or Project for this project only). Files lists the templates of New ▸ with their
//! name, extension, file name style, kinds and their texts, and Reformat according to style;
//! Includes the fragments `#parse` inserts; Code the members the test templates generate.
//!
//! The templates are IDE-level UI state kept in the UI storage, under
//! [`crate::templates::files_store::FILE_TEMPLATES_KEY`] and the per-project keys that extend it, in
//! the JSON the other app reads as well: every change is stored at once, so the page stages nothing
//! and Apply, Reset and Cancel leave it alone. What another window stores is shown as soon as it is
//! stored. A template's name is a label only: the file New creates is named by what the user types
//! there, which `files_model::plan_file` keeps inside the folder New is used in.

mod files_tab;
mod fragments;

use super::{Binding, ControlEvent, drop_down, drop_down_row, new_field, row, select_box};
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{button, search_label, tx};
use crate::templates::files_model::NameStyle;
use crate::templates::files_store::{FileTemplates, TemplateScheme, scheme_key};
use crate::templates::storage::StorageWrite;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseButton, Render,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
};

pub use files_tab::FileField;

pub const PAGE: &str = "editor.fileTemplates";

const SCHEME: &str = "Scheme";
const PROJECT_ONLY: &str = "Templates of this project only";
const EMPTY: &str = "Select a template to edit it";
const RESTORE: &str = "Restore Default";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 4] = [SCHEME, "Files", "Includes", "Code"];

/// The tabs of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateTab {
    Files,
    Includes,
    Code,
}

/// The tabs: (tab, selector name, title).
const TABS: [(TemplateTab, &str, &str); 3] = [
    (TemplateTab::Files, "files", "Files"),
    (TemplateTab::Includes, "includes", "Includes"),
    (TemplateTab::Code, "code", "Code"),
];

/// The schemes: (scheme, title).
const SCHEMES: [(TemplateScheme, &str); 2] =
    [(TemplateScheme::Default, "Default"), (TemplateScheme::Project, "Project")];

/// The title of `scheme`.
fn scheme_title(scheme: TemplateScheme) -> &'static str {
    SCHEMES.iter().find(|(each, _)| *each == scheme).map_or("Default", |(_, title)| *title)
}

/// The file name styles: (style, selector name, title).
pub const NAME_STYLES: [(NameStyle, &str, &str); 4] = [
    (NameStyle::Asis, "asis", "As typed"),
    (NameStyle::Snake, "snake", "snake_case"),
    (NameStyle::Pascal, "pascal", "PascalCase"),
    (NameStyle::Kebab, "kebab", "kebab-case"),
];

/// The text stored under `key`.
fn stored(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned)
}

/// `millis` in base 36, the digits of a new template's or kind's id.
fn base36(mut millis: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(millis % 36) as usize]);
        millis /= 36;
        if millis == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// The time now in milliseconds, in base 36.
fn now_base36() -> String {
    base36(crate::platform::local_time::now_ms().max(0.0) as u64)
}

pub struct FileTemplatesPageView {
    binding: Binding,
    store: FileTemplates,
    /// The stored texts the page shows: the project's scheme choice and the scheme's
    /// customisations. Another text stored under either is read again.
    shown: (Option<String>, Option<String>),
    tab: TemplateTab,
    scheme_open: bool,
    files_scroll: UniformListScrollHandle,
    files: files_tab::FilesState,
    includes: fragments::IncludesState,
    code: fragments::CodeState,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for FileTemplatesPageView {}

impl FileTemplatesPageView {
    /// The page for the project at `root` (as the backend names it), or for no project: the scheme
    /// is then Default and cannot be changed.
    pub fn new(binding: Binding, root: Option<String>, cx: &mut Context<Self>) -> Self {
        let store = {
            let storage = cx.try_global::<UiStorage>();
            FileTemplates::load(root.as_deref(), &|key| storage.and_then(|s| s.get(key)).map(str::to_owned))
        };
        let subscriptions = vec![
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
            cx.observe_global::<UiStorage>(Self::storage_changed),
        ];
        let mut view = Self {
            binding,
            store,
            shown: (None, None),
            tab: TemplateTab::Files,
            scheme_open: false,
            files_scroll: UniformListScrollHandle::new(),
            files: files_tab::FilesState::default(),
            includes: fragments::IncludesState::default(),
            code: fragments::CodeState::default(),
            _subscriptions: subscriptions,
        };
        view.shown = view.current_texts(cx);
        view.start_tab(cx);
        view
    }

    pub fn store(&self) -> &FileTemplates {
        &self.store
    }

    pub fn tab(&self) -> TemplateTab {
        self.tab
    }

    /// The stored texts the page depends on now.
    fn current_texts(&self, cx: &App) -> (Option<String>, Option<String>) {
        let scheme = self.store.root().and_then(|root| stored(cx, &scheme_key(root)));
        (scheme, stored(cx, &self.store.storage_key()))
    }

    /// Stores `writes` and remembers them as shown.
    fn persist(&mut self, writes: Vec<StorageWrite>, cx: &mut Context<Self>) {
        for write in writes {
            cx.default_global::<UiStorage>().set(&write.key, write.value);
        }
        self.shown = self.current_texts(cx);
    }

    /// Another text is stored under a key the page shows (another window saved): it is read again.
    /// Another scheme starts the tab over on its first entry; within the same scheme the entry
    /// selected stays, its fields showing what is stored now (nothing when it is gone).
    fn storage_changed(&mut self, cx: &mut Context<Self>) {
        if self.current_texts(cx) == self.shown {
            return;
        }
        let scheme = self.store.scheme();
        {
            let storage = cx.try_global::<UiStorage>();
            self.store.reload(&|key| storage.and_then(|s| s.get(key)).map(str::to_owned));
        }
        self.shown = self.current_texts(cx);
        if self.store.scheme() == scheme {
            match self.tab {
                TemplateTab::Files => self.make_file_fields(cx),
                TemplateTab::Includes => self.make_include_fields(cx),
                TemplateTab::Code => self.make_code_fields(cx),
            }
        } else {
            self.start_tab(cx);
        }
        cx.notify();
    }

    /// Drops the fields of every tab.
    fn clear_fields(&mut self) {
        self.files.clear();
        self.includes.clear();
        self.code.clear();
    }

    /// The tab shown starts on its first entry, its fields made for it.
    fn start_tab(&mut self, cx: &mut Context<Self>) {
        self.clear_fields();
        match self.tab {
            TemplateTab::Files => {
                self.files.selected = self.store.templates().first().map(|template| template.id.clone());
                self.files.kind = 0;
                self.make_file_fields(cx);
            }
            TemplateTab::Includes => {
                self.includes.selected = self.store.includes().first().map(|include| include.name.clone());
                self.make_include_fields(cx);
            }
            TemplateTab::Code => {
                self.code.selected = self.store.code_templates().first().map(|code| code.name.clone());
                self.make_code_fields(cx);
            }
        }
    }

    pub fn select_tab(&mut self, tab: TemplateTab, cx: &mut Context<Self>) {
        self.commit_include_name(cx);
        if self.tab != tab {
            self.tab = tab;
            self.start_tab(cx);
        }
        cx.notify();
    }

    /// Switches the project's scheme; the tab starts over on its first entry. Nothing without a
    /// project.
    pub fn set_scheme(&mut self, scheme: TemplateScheme, cx: &mut Context<Self>) {
        self.scheme_open = false;
        if self.store.root().is_none() {
            cx.notify();
            return;
        }
        self.commit_include_name(cx);
        let writes = {
            let storage = cx.try_global::<UiStorage>();
            self.store.set_scheme(scheme, &|key| storage.and_then(|s| s.get(key)).map(str::to_owned))
        };
        self.persist(writes, cx);
        self.start_tab(cx);
        cx.notify();
    }

    /// A single-line field showing `text`, read-only when `read_only`.
    fn text_field(text: &str, placeholder: Option<String>, read_only: bool, cx: &mut App) -> Entity<TextField> {
        let field = new_field(text, placeholder, cx);
        field.update(cx, |field, cx| field.set_read_only(read_only, cx));
        field
    }

    /// A template text area showing `text`, in the code font.
    fn text_area(text: &str, cx: &mut App) -> Entity<TextArea> {
        cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_code_font(true, cx);
            area.set_soft_wrap(false, cx);
            area.set_rows(8, 20, cx);
            area.set_text(text, cx);
            area
        })
    }

    /// Answers `field`: a change runs `changed`, Enter is the dialog's OK and Escape its Cancel.
    fn watch_field(
        field: &Entity<TextField>,
        changed: fn(&mut Self, &Entity<TextField>, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => changed(view, &field, cx),
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        })
    }

    /// Answers `area`: a change runs `changed` with its text, Escape is the dialog's Cancel.
    fn watch_area(
        area: &Entity<TextArea>,
        changed: fn(&mut Self, String, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(area, move |view: &mut Self, area, event: &TextAreaEvent, cx| match event {
            TextAreaEvent::Changed => {
                let text = area.read(cx).text().to_owned();
                changed(view, text, cx);
            }
            TextAreaEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            TextAreaEvent::Submitted | TextAreaEvent::Blurred => {}
        })
    }

    /// The page as text: the scheme, the tabs with the one shown in brackets, then the tab's lines.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut head = format!("{}: {} ▾", tx(cx, SCHEME), tx(cx, scheme_title(self.store.scheme())));
        if self.store.root().is_none() {
            head.push_str(" (disabled)");
        }
        if self.store.scheme() == TemplateScheme::Project {
            head = format!("{head} {}", tx(cx, PROJECT_ONLY));
        }
        let tabs: Vec<String> = TABS
            .iter()
            .map(|&(tab, _, title)| if tab == self.tab { format!("[{}]", tx(cx, title)) } else { tx(cx, title) })
            .collect();
        let mut out = vec![head, tabs.join(" | ")];
        match self.tab {
            TemplateTab::Files => out.extend(self.files_lines(cx)),
            TemplateTab::Includes => out.extend(self.includes_lines(cx)),
            TemplateTab::Code => out.extend(self.code_lines(cx)),
        }
        out
    }

    /// `rows` in a column that scrolls, drawn as `selector`.
    fn scrolled_rows(selector: &'static str, rows: Vec<AnyElement>) -> AnyElement {
        div().id(selector).flex_1().min_h_0().overflow_y_scroll().py(px(2.0)).children(rows).into_any_element()
    }

    /// The list column of a tab: its toolbar when it has one, then its list.
    fn side(cx: &App, toolbar: Option<AnyElement>, list: AnyElement) -> AnyElement {
        let toolbar = toolbar.map(|buttons| {
            div()
                .h(theme::length(cx, "h-header"))
                .px(px(4.0))
                .flex()
                .items_center()
                .gap(px(2.0))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
                .child(buttons)
        });
        div()
            .w(px(240.0))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme::color(cx, "border-strong"))
            .children(toolbar)
            .child(list)
            .into_any_element()
    }

    /// The list and the editor of a tab, side by side.
    fn split(cx: &App, side: AnyElement, editor: AnyElement) -> AnyElement {
        div()
            .flex_1()
            .min_w(px(520.0))
            .min_h(px(320.0))
            .flex()
            .overflow_hidden()
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .rounded(theme::length(cx, "radius-small"))
            .child(side)
            .child(
                div()
                    .id("settings-filetemplates-editor")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .py(px(10.0))
                    .px(px(12.0))
                    .child(editor),
            )
            .into_any_element()
    }

    /// A row of a tab's list drawn as `selector`, marked when `chosen`.
    fn list_row(cx: &App, selector: String, chosen: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .w_full()
            .h(theme::length(cx, "h-row"))
            .pl(px(18.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .whitespace_nowrap()
            .overflow_hidden()
            .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")).text_color(theme::color(cx, "fg-strong")))
            .when(!chosen, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
    }

    /// The text shown in place of an editor while nothing is selected.
    fn empty(cx: &App) -> AnyElement {
        div()
            .p(px(20.0))
            .flex()
            .justify_center()
            .text_color(theme::color(cx, "fg-faint"))
            .child(SharedString::from(tx(cx, EMPTY)))
            .into_any_element()
    }

    /// A row of an editor: its label, then `control` in a field frame drawn as `selector`.
    fn labelled(cx: &App, label: &str, selector: &'static str, control: AnyElement, mono: bool) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(92.0))
                    .flex_none()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, label))),
            )
            .child(Self::frame(cx, selector, control, mono).flex_1())
            .into_any_element()
    }

    /// The frame a field is drawn in, drawn as `selector`.
    fn frame(cx: &App, selector: &'static str, control: AnyElement, mono: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .min_w_0()
            .h(theme::length(cx, "h-control"))
            .px(px(6.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-input"))
            .when(mono, |el| el.font_family(super::code_family(cx)))
            .child(control)
    }

    /// A template text area's frame, drawn as `selector`.
    fn body_frame(cx: &App, selector: &'static str, area: &Entity<TextArea>) -> AnyElement {
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .py(px(6.0))
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-app"))
            .text_color(theme::color(cx, "fg-emphasis"))
            .child(area.clone())
            .into_any_element()
    }

    /// A help line of plain parts (in the UI language) and code.
    fn hint(cx: &App, parts: &[(bool, String)]) -> AnyElement {
        super::hint_flow(cx, parts)
    }

    /// `text` in the UI language with its `{code}` placeholder drawn as code.
    fn with_code(cx: &App, text: &str, code: &str) -> AnyElement {
        let translated = tx(cx, text);
        let (before, after) = translated.split_once("{code}").unwrap_or((translated.as_str(), ""));
        let mut parts = Vec::new();
        if !before.trim().is_empty() {
            parts.push((false, before.trim_end().to_owned()));
        }
        parts.push((true, code.to_owned()));
        if !after.trim().is_empty() {
            parts.push((false, after.trim_start().to_owned()));
        }
        Self::hint(cx, &parts)
    }

    /// `with_code`'s text as one line.
    fn with_code_line(cx: &App, text: &str, code: &str) -> String {
        tx(cx, text).replacen("{code}", code, 1)
    }

    /// A Restore Default button drawn as `selector`, running `restore`.
    fn restore_button(
        selector: &'static str,
        restore: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = tx(cx, RESTORE);
        div()
            .flex()
            .justify_end()
            .child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .child(button(cx, &label, false, true))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| restore(view, cx))),
            )
            .into_any_element()
    }

    fn scheme_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.binding.query(cx);
        let enabled = self.store.root().is_some();
        let current = self.store.scheme();
        let shown = tx(cx, scheme_title(current));
        let choices = (self.scheme_open && enabled).then(|| {
            let rows: Vec<AnyElement> = SCHEMES
                .iter()
                .map(|&(scheme, title)| {
                    let selector = format!("settings-filetemplates-scheme-{}", scheme.id());
                    let text = tx(cx, title);
                    drop_down_row(cx, selector, &text, scheme == current)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_scheme(scheme, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let label = tx(cx, SCHEME);
        row(cx, self.binding.matches(SCHEME, cx))
            .child(
                div()
                    .w(px(92.0))
                    .flex_none()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(search_label(cx, &label, &query)),
            )
            .child(
                div()
                    .relative()
                    .child(
                        select_box(cx, &shown)
                            .id("settings-filetemplates-scheme")
                            .debug_selector(|| "settings-filetemplates-scheme".to_owned())
                            .when(!enabled, |el| el.opacity(0.5))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .when(enabled, |el| {
                                el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.scheme_open = !view.scheme_open;
                                    cx.notify();
                                }))
                            }),
                    )
                    .children(choices),
            )
            .when(current == TemplateScheme::Project, |el| {
                el.child(
                    div()
                        .text_size(theme::length(cx, "fs-small"))
                        .text_color(theme::color(cx, "fg-subtle"))
                        .child(SharedString::from(tx(cx, PROJECT_ONLY))),
                )
            })
            .into_any_element()
    }

    fn tab_button(&self, tab: TemplateTab, name: &str, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("settings-filetemplates-tab-{name}");
        let active = tab == self.tab;
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
            .child(SharedString::from(tx(cx, title)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(tab, cx)))
            .into_any_element()
    }
}

impl Render for FileTemplatesPageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch_include_name_blur(window, cx);
        let scheme = self.scheme_row(cx);
        let tabs: Vec<AnyElement> =
            TABS.iter().map(|&(tab, name, title)| self.tab_button(tab, name, title, cx)).collect();
        let body = match self.tab {
            TemplateTab::Files => self.render_files(cx),
            TemplateTab::Includes => self.render_includes(cx),
            TemplateTab::Code => self.render_code(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_color(theme::color(cx, "fg"))
            // A press anywhere else on the page closes an open choice list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.scheme_open || view.files.style_open {
                        (view.scheme_open, view.files.style_open) = (false, false);
                        cx.notify();
                    }
                }),
            )
            .child(scheme)
            .child(div().flex().flex_wrap().gap(px(2.0)).children(tabs))
            .child(body)
    }
}

#[cfg(test)]
mod extension_guard;
#[cfg(test)]
mod tests;
