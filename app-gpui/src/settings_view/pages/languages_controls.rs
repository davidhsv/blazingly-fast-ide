//! The page view of a language page, which holds its controls in their sections, and the
//! controls of those pages beyond the shared form controls: the languages an external formatter
//! formats, the Markdown preview font size, sharing JDT LS, the plugin formatter of each language, the PHP interpreter's version and Xdebug, and the forms that
//! only a project has.

use super::languages_sdk::{
    KnownLanguage, LanguageAsk, LanguageEvent, PathFieldView, PluginFormatter, SdkFieldView, Toolchain,
    ToolchainCatalog, ToolchainKind, Toolchains, enter_escape,
};
use super::list_editor::ListEditor;
use super::{
    Binding, ControlEvent, FormView, NUMBER_FIELD, Then, claim_field_keys, drop_down, drop_down_row, frame, hint_line,
    muted, new_field, row, row_label, same_value, select_box,
};
use crate::platform::input;
use crate::settings_model::staging::Scope;
use crate::settings_view::controls::{
    check_box, number_json, number_text, parse_number_input, search_label, section, tx, with_range_tooltip,
};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, IntoElement, Keystroke, MouseButton,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use serde_json::{Map, Value};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// The page view

/// One control of a language page.
pub enum Control {
    Form(Entity<FormView>),
    List(Entity<ListEditor>),
    Sdk(Entity<SdkFieldView>),
    Path(Entity<PathFieldView>),
    Languages(Entity<FormatterLanguagesView>),
    FontSize(Entity<PreviewFontSizeView>),
    Shared(Entity<JdtlsSharedView>),
    Formatters(Entity<PluginFormattersView>),
    Php(Entity<PhpInterpreterView>),
    ProjectOnly(Entity<ProjectOnlyView>),
}

impl Control {
    pub fn view(&self) -> AnyView {
        match self {
            Self::Form(view) => view.clone().into(),
            Self::List(view) => view.clone().into(),
            Self::Sdk(view) => view.clone().into(),
            Self::Path(view) => view.clone().into(),
            Self::Languages(view) => view.clone().into(),
            Self::FontSize(view) => view.clone().into(),
            Self::Shared(view) => view.clone().into(),
            Self::Formatters(view) => view.clone().into(),
            Self::Php(view) => view.clone().into(),
            Self::ProjectOnly(view) => view.clone().into(),
        }
    }

    /// The control as lines of text.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        match self {
            Self::Form(view) => view.read(cx).lines(cx),
            Self::List(view) => view.read(cx).lines(cx),
            Self::Sdk(view) => view.read(cx).lines(cx),
            Self::Path(view) => view.read(cx).lines(cx),
            Self::Languages(view) => view.read(cx).lines(cx),
            Self::FontSize(view) => view.read(cx).lines(cx),
            Self::Shared(view) => view.read(cx).lines(cx),
            Self::Formatters(view) => view.read(cx).lines(cx),
            Self::Php(view) => view.read(cx).lines(cx),
            Self::ProjectOnly(view) => view.read(cx).lines(cx),
        }
    }
}

/// A section of a page: its title and controls.
pub type Section = (Option<&'static str>, Vec<Control>);

/// A language page: its sections of controls. The installations the page offers are detected the
/// first time it is shown. What its controls ask goes out as the dialog's [`ControlEvent`]s and
/// the backend's [`LanguageAsk`]s.
pub struct LanguagePageView {
    page: &'static str,
    binding: Binding,
    sections: Vec<Section>,
    toolchains: Toolchains,
    detect: Vec<ToolchainKind>,
    asked: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for LanguagePageView {}
impl EventEmitter<LanguageAsk> for LanguagePageView {}

impl LanguagePageView {
    pub fn new(
        page: &'static str,
        binding: Binding,
        sections: Vec<Section>,
        toolchains: Toolchains,
        detect: Vec<ToolchainKind>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        for control in sections.iter().flat_map(|(_, controls)| controls) {
            subscriptions.push(Self::listen(control, cx));
        }
        Self { page, binding, sections, toolchains, detect, asked: false, _subscriptions: subscriptions }
    }

    fn listen(control: &Control, cx: &mut Context<Self>) -> Subscription {
        let control_event = |_: &mut Self, event: &ControlEvent, cx: &mut Context<Self>| cx.emit(event.clone());
        match control {
            Control::Form(view) => cx.subscribe(view, move |page, _, event, cx| control_event(page, event, cx)),
            Control::List(view) => cx.subscribe(view, move |page, _, event, cx| control_event(page, event, cx)),
            Control::Sdk(view) => cx.subscribe(view, move |page, _, event, cx| control_event(page, event, cx)),
            Control::Path(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::Languages(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::FontSize(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::Shared(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::Formatters(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::Php(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
            Control::ProjectOnly(view) => cx.subscribe(view, |_, _, event, cx| Self::forward(event, cx)),
        }
    }

    fn forward(event: &LanguageEvent, cx: &mut Context<Self>) {
        match event {
            LanguageEvent::Control(event) => cx.emit(event.clone()),
            LanguageEvent::Ask(ask) => cx.emit(ask.clone()),
        }
    }

    /// The page as lines of text: `— Title —` for a titled section, then its controls' lines.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = Vec::new();
        for (title, controls) in &self.sections {
            out.extend(title.map(|title| format!("— {} —", tx(cx, title))));
            for control in controls {
                out.extend(control.lines(cx));
            }
        }
        out
    }
}

impl Render for LanguagePageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The installations are detected when the page is first shown, not when Settings opens,
        // after the frame.
        if !self.asked {
            self.asked = true;
            let (toolchains, kinds) = (self.toolchains.clone(), self.detect.clone());
            cx.defer(move |cx| {
                for kind in kinds {
                    toolchains.detect(kind, false, cx);
                }
            });
        }
        let query = self.binding.query(cx);
        let drawn: Vec<Div> = self
            .sections
            .iter()
            .map(|(title, controls)| {
                let title = title.map(|title| tx(cx, title));
                let rows: Vec<AnyElement> = controls.iter().map(|control| control.view().into_any_element()).collect();
                section(cx, title.as_deref(), &query, rows)
            })
            .collect();
        // The page scrolls inside the dialog: Prettier's languages and PHP's sections run past it.
        div()
            .id(SharedString::from(format!("settings-language-page-{}", self.page)))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .children(drawn)
    }
}

/// `[x] Label` with its help line, as the forms draw a checkbox.
fn toggle_row(
    cx: &App,
    selector: String,
    label: &str,
    on: bool,
    enabled: bool,
    matched: bool,
    query: &str,
) -> gpui_kit::Stateful<Div> {
    let id = selector.clone();
    row(cx, matched)
        .id(SharedString::from(id))
        .debug_selector(move || selector)
        .when(!enabled, |el| el.opacity(0.5))
        .child(check_box(cx, on))
        .child(search_label(cx, label, query))
}

// ---------------------------------------------------------------------------
// Enabled languages of an external formatter

/// A checkbox per language an external formatter can format; the setting lists those it does.
pub struct FormatterLanguagesView {
    binding: Binding,
    key: &'static str,
    languages: &'static [(&'static str, &'static str)],
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for FormatterLanguagesView {}

impl FormatterLanguagesView {
    /// `languages`: (id, label) in the order they are listed.
    pub fn new(
        binding: Binding,
        key: &'static str,
        languages: &'static [(&'static str, &'static str)],
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self { binding, key, languages, _subscriptions: vec![observe] }
    }

    /// Turns `language` on (added at the end) or off.
    pub fn flip(&mut self, language: &str, cx: &mut Context<Self>) {
        let mut list = self.binding.strings(self.key, cx);
        if list.iter().any(|own| own == language) {
            list.retain(|own| own != language);
        } else {
            list.push(language.to_owned());
        }
        self.binding.stage(self.key, Value::from(list), cx);
        cx.notify();
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let enabled = self.binding.strings(self.key, cx);
        let mark = |id: &str| if enabled.iter().any(|own| own == id) { "x" } else { " " };
        self.languages.iter().map(|(id, label)| format!("[{}] {}", mark(id), tx(cx, label))).collect()
    }
}

impl Render for FormatterLanguagesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let enabled = self.binding.strings(self.key, cx);
        let key = self.key;
        let mut rows: Vec<AnyElement> = Vec::new();
        for &(id, label) in self.languages {
            let text = tx(cx, label);
            let on = enabled.iter().any(|own| own == id);
            let matched = self.binding.matches(label, cx);
            let drawn = toggle_row(cx, format!("settings-toggle-{key}-{id}"), &text, on, true, matched, &query)
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(id, cx)));
            rows.push(drawn.into_any_element());
        }
        div().flex().flex_col().children(rows)
    }
}

// ---------------------------------------------------------------------------
// Markdown › Preview font size

/// The setting of the preview font size; unset follows the editor font size.
pub const PREVIEW_FONT_SIZE: &str = "languages.markdown.previewFontSize";
const EDITOR_FONT_SIZE: &str = "editor.fontSize";
const PREVIEW_RANGE: (f64, f64) = (8.0, 72.0);

/// Preview font size: the size set, else the editor's; a whole number of px within 8..72.
pub struct PreviewFontSizeView {
    binding: Binding,
    field: Entity<TextField>,
    invalid: bool,
    seen: Option<Value>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for PreviewFontSizeView {}

impl PreviewFontSizeView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let text = number_text(Self::size(&binding, cx));
        let field = new_field(&text, None, cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                view.edited(&text, cx);
            }
            TextFieldEvent::Submitted => cx.emit(LanguageEvent::Control(ControlEvent::Submit)),
            TextFieldEvent::Cancelled => cx.emit(LanguageEvent::Control(ControlEvent::Cancel)),
        });
        let claim = claim_field_keys(&field, |view: &mut Self, keystroke, cx| view.key(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            view.sync(cx);
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        let seen = binding.value(PREVIEW_FONT_SIZE, cx);
        Self { binding, field, invalid: false, seen, claim, _subscriptions: vec![events, observe] }
    }

    /// The size the preview uses: the one set, else the editor font size.
    fn size(binding: &Binding, cx: &App) -> f64 {
        let set = binding.value(PREVIEW_FONT_SIZE, cx).and_then(|value| value.as_f64());
        set.unwrap_or_else(|| binding.number(EDITOR_FONT_SIZE, 13.0, cx))
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn is_invalid(&self) -> bool {
        self.invalid
    }

    fn edited(&mut self, text: &str, cx: &mut Context<Self>) {
        let (min, max) = PREVIEW_RANGE;
        match parse_number_input(text, min, max, true) {
            Some(size) => {
                self.invalid = false;
                let value = number_json(size);
                self.seen = Some(value.clone());
                self.binding.stage(PREVIEW_FONT_SIZE, value, cx);
            }
            None => self.invalid = true,
        }
        cx.notify();
    }

    /// Up / Down step the size by one within its range; Enter is OK and Escape Cancel.
    fn key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let step = match keystroke.key.as_str() {
            "up" => 1.0,
            "down" => -1.0,
            _ => return enter_escape(keystroke, cx),
        };
        let (min, max) = PREVIEW_RANGE;
        let next = (Self::size(&self.binding, cx) + step).clamp(min, max);
        self.invalid = false;
        self.binding.stage(PREVIEW_FONT_SIZE, number_json(next), cx);
        true
    }

    /// Shows the size again when it changed under the field (Reset, a step, another page).
    fn sync(&mut self, cx: &mut Context<Self>) {
        let wanted = self.binding.value(PREVIEW_FONT_SIZE, cx);
        let shown = self.field.read(cx).text().to_owned();
        let size = Self::size(&self.binding, cx);
        let (min, max) = PREVIEW_RANGE;
        let spells = parse_number_input(&shown, min, max, true) == Some(size);
        if !same_value(wanted.as_ref(), self.seen.as_ref()) || (!self.invalid && !spells) {
            self.invalid = false;
            let text = number_text(size);
            if text != shown {
                self.field.update(cx, |field, cx| field.set_text(&text, cx));
            }
        }
        self.seen = wanted;
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let invalid = if self.invalid { " (invalid)" } else { "" };
        let text = self.field.read(cx).text().to_owned();
        vec![format!("{}: {text} px{invalid}", tx(cx, "Preview font size")), format!("    {}", tx(cx, PREVIEW_HINT))]
    }
}

const PREVIEW_HINT: &str = "Also used by Markdown cells in Jupyter notebooks. Defaults to the editor font size, \
                            independent of the UI font size.";

impl Render for PreviewFontSizeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL: &str = "Preview font size";
        let query = self.binding.query(cx);
        let matched = self.binding.matches(LABEL, cx);
        let label = tx(cx, LABEL);
        let (min, max) = PREVIEW_RANGE;
        let drawn = frame(cx, NUMBER_FIELD, self.invalid)
            .id(SharedString::from(format!("settings-number-{PREVIEW_FONT_SIZE}")))
            .debug_selector(|| format!("settings-number-{PREVIEW_FONT_SIZE}"))
            .child(self.field.clone());
        let unit = tx(cx, "px");
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(with_range_tooltip(drawn, self.invalid, min, max))
                    .child(muted(cx, &unit)),
            )
            .child(hint_line(cx, &tx(cx, PREVIEW_HINT), 0.0))
    }
}

// ---------------------------------------------------------------------------
// Java › Share JDT LS across builds

pub const JDTLS_SHARED: &str = "languages.java.jdtlsShared";
const SHARED_LABEL: &str = "Share JDT LS across builds";
const SHARED_IN_WORKSPACE: &str = "Shared JDT LS is available only in local ordinary project windows. Open a \
                                   workspace member as a project to change this setting for it.";
const SHARED_HINT: &str = "Uses one JDT LS process for linked builds in each local ordinary project window. Other \
                           project windows use separate processes.";

/// Share JDT LS across builds: not editable for the project of a workspace, which never shares one.
pub struct JdtlsSharedView {
    binding: Binding,
    /// The window's project is a workspace; asked of the backend when the checkbox is first shown.
    workspace: bool,
    asked: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for JdtlsSharedView {}

impl JdtlsSharedView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self { binding, workspace: false, asked: false, _subscriptions: vec![observe] }
    }

    pub fn set_workspace(&mut self, workspace: bool, cx: &mut Context<Self>) {
        self.workspace = workspace;
        cx.notify();
    }

    pub fn is_disabled(&self, cx: &App) -> bool {
        self.binding.scope(cx) == Scope::Project && self.workspace
    }

    fn hint(&self, cx: &App) -> &'static str {
        if self.is_disabled(cx) { SHARED_IN_WORKSPACE } else { SHARED_HINT }
    }

    fn flip(&mut self, cx: &mut Context<Self>) {
        if self.is_disabled(cx) {
            return;
        }
        let now = self.binding.bool(JDTLS_SHARED, cx);
        self.binding.stage(JDTLS_SHARED, Value::Bool(!now), cx);
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let on = if self.binding.bool(JDTLS_SHARED, cx) { "x" } else { " " };
        let disabled = if self.is_disabled(cx) { " (disabled)" } else { "" };
        vec![format!("[{on}] {}{disabled}", tx(cx, SHARED_LABEL)), format!("    {}", tx(cx, self.hint(cx)))]
    }
}

impl Render for JdtlsSharedView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.asked {
            self.asked = true;
            let view = cx.weak_entity();
            let then: Then<bool> = Rc::new(move |workspace, cx: &mut App| {
                view.update(cx, |view, cx| view.set_workspace(workspace, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::Workspace { then }));
        }
        let query = self.binding.query(cx);
        let matched = self.binding.matches(SHARED_LABEL, cx);
        let label = tx(cx, SHARED_LABEL);
        let on = self.binding.bool(JDTLS_SHARED, cx);
        let enabled = !self.is_disabled(cx);
        let drawn = toggle_row(cx, format!("settings-toggle-{JDTLS_SHARED}"), &label, on, enabled, matched, &query)
            .when(enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.flip(cx))));
        let hint = tx(cx, self.hint(cx));
        div().flex().flex_col().child(drawn).child(hint_line(cx, &hint, 28.0))
    }
}

// ---------------------------------------------------------------------------
// Plugin Formatters

pub const PLUGIN_FORMATTERS: &str = "languages.pluginFormatters";

/// A formatter choice per language a plugin can format (or one was chosen for).
pub struct PluginFormattersView {
    binding: Binding,
    known: Vec<String>,
    formatters: Vec<PluginFormatter>,
    open: Option<usize>,
    asked: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for PluginFormattersView {}

impl PluginFormattersView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self {
            binding,
            known: Vec::new(),
            formatters: Vec::new(),
            open: None,
            asked: false,
            _subscriptions: vec![observe],
        }
    }

    pub fn set_known(&mut self, known: Vec<KnownLanguage>, cx: &mut Context<Self>) {
        self.known = known.into_iter().map(|language| language.id).collect();
        cx.notify();
    }

    pub fn set_formatters(&mut self, formatters: Vec<PluginFormatter>, cx: &mut Context<Self>) {
        self.formatters = formatters;
        cx.notify();
    }

    fn selected(&self, cx: &App) -> Map<String, Value> {
        let value = self.binding.value(PLUGIN_FORMATTERS, cx);
        value.and_then(|value| value.as_object().cloned()).unwrap_or_default()
    }

    fn covers(formatter: &PluginFormatter, language: &str) -> bool {
        formatter.languages.iter().any(|own| own == language || own == "*")
    }

    /// The languages listed: the registry's, those with a choice stored, those a plugin names;
    /// each once, and only those a plugin can format or one was chosen for.
    pub fn languages(&self, cx: &App) -> Vec<String> {
        let selected = self.selected(cx);
        let chosen = |language: &str| selected.get(language).and_then(Value::as_str).is_some_and(|id| !id.is_empty());
        let named = self.formatters.iter().flat_map(|formatter| formatter.languages.iter().cloned());
        let mut out: Vec<String> = Vec::new();
        for language in self.known.iter().cloned().chain(selected.keys().cloned()).chain(named) {
            let offered = self.formatters.iter().any(|formatter| Self::covers(formatter, &language));
            if language != "*" && !out.contains(&language) && (chosen(&language) || offered) {
                out.push(language);
            }
        }
        out
    }

    /// The choices of `language`: Automatic, the formatters that cover it, a stored one no plugin
    /// offers now.
    fn choices(&self, language: &str, cx: &App) -> Vec<(String, String)> {
        let mut out = vec![(String::new(), tx(cx, "Automatic (language server first)"))];
        let covering = self.formatters.iter().filter(|formatter| Self::covers(formatter, language));
        out.extend(covering.map(|formatter| (formatter.id.clone(), formatter.name.clone())));
        let current = self.current(language, cx);
        if !current.is_empty() && !self.formatters.iter().any(|formatter| formatter.id == current) {
            out.push((current.clone(), current));
        }
        out
    }

    fn current(&self, language: &str, cx: &App) -> String {
        let selected = self.selected(cx);
        selected.get(language).and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    fn choose(&mut self, language: &str, value: String, cx: &mut Context<Self>) {
        self.open = None;
        let mut selected = self.selected(cx);
        selected.insert(language.to_owned(), Value::String(value));
        self.binding.stage(PLUGIN_FORMATTERS, Value::Object(selected), cx);
        cx.notify();
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let rows = self.languages(cx).into_iter().map(|language| {
            let current = self.current(&language, cx);
            let choices = self.choices(&language, cx);
            let shown = choices.into_iter().find(|(value, _)| *value == current).map(|(_, label)| label);
            format!("{language}: {} ▾", shown.unwrap_or_default())
        });
        rows.collect()
    }
}

impl Render for PluginFormattersView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.asked {
            self.asked = true;
            let view = cx.weak_entity();
            let languages: Then<Vec<KnownLanguage>> = Rc::new(move |known, cx: &mut App| {
                view.update(cx, |view, cx| view.set_known(known, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::Languages { then: languages }));
            let view = cx.weak_entity();
            let formatters: Then<Vec<PluginFormatter>> = Rc::new(move |formatters, cx: &mut App| {
                view.update(cx, |view, cx| view.set_formatters(formatters, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::PluginFormatters { then: formatters }));
        }
        let query = self.binding.query(cx);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (index, language) in self.languages(cx).into_iter().enumerate() {
            let current = self.current(&language, cx);
            let choices = self.choices(&language, cx);
            let shown = choices.iter().find(|(value, _)| *value == current).map(|(_, label)| label.clone());
            let matched = self.binding.matches(&language, cx);
            let selector = format!("settings-formatter-{index}");
            let boxed = select_box(cx, &shown.unwrap_or_default())
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.open = if view.open == Some(index) { None } else { Some(index) };
                    cx.notify();
                }));
            let open = (self.open == Some(index)).then(|| {
                let mut list: Vec<AnyElement> = Vec::new();
                for (at, (value, label)) in choices.into_iter().enumerate() {
                    let language = language.clone();
                    let chosen = value == current;
                    let drawn = drop_down_row(cx, format!("settings-formatter-{index}-{at}"), &label, chosen).on_click(
                        cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.choose(&language, value.clone(), cx);
                        }),
                    );
                    list.push(drawn.into_any_element());
                }
                drop_down(cx, list)
            });
            let drawn = row(cx, matched)
                .child(row_label(cx, &language, &query))
                .child(div().relative().child(boxed).children(open));
            rows.push(drawn.into_any_element());
        }
        div().flex().flex_col().children(rows)
    }
}

// ---------------------------------------------------------------------------
// PHP › CLI Interpreter: version and Xdebug

pub const PHP_INTERPRETER: &str = "languages.php.interpreter";

/// The version of the PHP the page selects and the Xdebug it loads. The Default interpreter is the
/// `php` on the PATH, looked up while no interpreter is set.
pub struct PhpInterpreterView {
    binding: Binding,
    toolchains: Toolchains,
    path_php: Option<String>,
    asked_path: bool,
    /// The interpreter Xdebug was asked for, and the answer once it came (`None` inside: failed).
    xdebug: Option<(String, Option<Option<Option<String>>>)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for PhpInterpreterView {}

impl PhpInterpreterView {
    pub fn new(binding: Binding, toolchains: Toolchains, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        let catalog = cx.observe_global::<ToolchainCatalog>(|_: &mut Self, cx| cx.notify());
        let subscriptions = vec![observe, catalog];
        Self { binding, toolchains, path_php: None, asked_path: false, xdebug: None, _subscriptions: subscriptions }
    }

    fn set_path_php(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.path_php = path;
        cx.notify();
    }

    fn set_xdebug(&mut self, php: String, xdebug: Option<Option<String>>, cx: &mut Context<Self>) {
        if self.xdebug.as_ref().is_some_and(|(asked, _)| *asked == php) {
            self.xdebug = Some((php, Some(xdebug)));
            cx.notify();
        }
    }

    /// The detected PHP the page selects: the one set, else the one on the PATH.
    fn selected(&self, cx: &App) -> Option<Toolchain> {
        let interpreter = self.binding.text(PHP_INTERPRETER, cx);
        let wanted = if interpreter.is_empty() { self.path_php.clone()? } else { interpreter };
        self.toolchains.found(ToolchainKind::Php, cx).into_iter().find(|sdk| sdk.path == wanted)
    }

    /// `Some(xdebug)` once the selected PHP was checked; `None` while it is or when the check failed.
    fn xdebug_of(&self, php: &str) -> Option<Option<String>> {
        let (asked, answer) = self.xdebug.as_ref()?;
        if asked != php {
            return None;
        }
        answer.clone().flatten()
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let Some(selected) = self.selected(cx) else { return Vec::new() };
        let mut out: Vec<String> =
            selected.version.iter().map(|version| format!("{}: {version}", tx(cx, "Version"))).collect();
        if let Some(xdebug) = self.xdebug_of(&selected.path) {
            let shown = xdebug.unwrap_or_else(|| tx(cx, "Not installed"));
            out.push(format!("{}: {shown}", tx(cx, "Xdebug")));
        }
        out
    }
}

impl Render for PhpInterpreterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.asked_path && self.binding.text(PHP_INTERPRETER, cx).is_empty() {
            self.asked_path = true;
            let view = cx.weak_entity();
            let then: Then<Option<String>> = Rc::new(move |path, cx: &mut App| {
                view.update(cx, |view, cx| view.set_path_php(path, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::LocateTool { name: "php", then }));
        }
        let selected = self.selected(cx);
        if let Some(selected) = &selected
            && self.xdebug.as_ref().is_none_or(|(asked, _)| *asked != selected.path)
        {
            let php = selected.path.clone();
            self.xdebug = Some((php.clone(), None));
            let view = cx.weak_entity();
            let asked = php.clone();
            let then: Then<Option<Option<String>>> = Rc::new(move |xdebug, cx: &mut App| {
                let asked = asked.clone();
                view.update(cx, |view, cx| view.set_xdebug(asked, xdebug, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::PhpXdebug { php, then }));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(selected) = selected {
            let query = self.binding.query(cx);
            if let Some(version) = &selected.version {
                let label = tx(cx, "Version");
                let drawn = row(cx, false)
                    .debug_selector(|| "settings-php-version".to_owned())
                    .child(row_label(cx, &label, &query))
                    .child(SharedString::from(version.clone()));
                rows.push(drawn.into_any_element());
            }
            if let Some(xdebug) = self.xdebug_of(&selected.path) {
                let label = tx(cx, "Xdebug");
                let shown = xdebug.unwrap_or_else(|| tx(cx, "Not installed"));
                let drawn = row(cx, false)
                    .debug_selector(|| "settings-php-xdebug".to_owned())
                    .child(row_label(cx, &label, &query))
                    .child(SharedString::from(shown));
                rows.push(drawn.into_any_element());
            }
        }
        div().flex().flex_col().children(rows)
    }
}

// ---------------------------------------------------------------------------
// Forms only a project has

/// A form of the project's settings: with no project open, or while the dialog edits the settings
/// for new projects, it says a project is needed instead.
pub struct ProjectOnlyView {
    form: Entity<FormView>,
    available: bool,
    message: &'static str,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for ProjectOnlyView {}

impl ProjectOnlyView {
    pub fn new(form: Entity<FormView>, available: bool, message: &'static str, cx: &mut Context<Self>) -> Self {
        let events = cx.subscribe(&form, |_: &mut Self, _, event: &ControlEvent, cx| {
            cx.emit(LanguageEvent::Control(event.clone()));
        });
        Self { form, available, message, _subscriptions: vec![events] }
    }

    pub fn form(&self) -> &Entity<FormView> {
        &self.form
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        if self.available { self.form.read(cx).lines(cx) } else { vec![tx(cx, self.message)] }
    }
}

impl Render for ProjectOnlyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.available {
            return div().child(self.form.clone()).into_any_element();
        }
        let text = tx(cx, self.message);
        muted(cx, &text).debug_selector(|| "settings-project-only".to_owned()).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::languages_sdk::SdkDetection;
    use super::super::test_support::{click, dialog_model, init, staged, type_over};
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;

    mod proofs22_language_controls;

    fn open<V, F>(cx: &mut TestAppContext, build: F) -> (Entity<V>, &mut VisualTestContext)
    where
        V: Render + EventEmitter<LanguageEvent>,
        F: FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
    {
        init(cx);
        let (view, cx) = cx.add_window_view(build);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        (view, cx)
    }

    const PRETTIER: &[(&str, &str)] = &[("markdown", "Markdown"), ("yaml", "YAML")];

    // Not from a spec: a language checkbox removes the language from the list it is in and adds
    // it back at the end.
    #[gpui_kit::test]
    async fn a_language_checkbox_removes_and_adds_the_language(cx: &mut TestAppContext) {
        const PAGE: &str = "languages.prettier";
        let stored = [("languages.prettier.languages", json!(["markdown", "yaml"]))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let key = "languages.prettier.languages";
        let (view, cx) = open(cx, move |_, cx| FormatterLanguagesView::new(binding, key, PRETTIER, cx));
        click(cx, "settings-toggle-languages.prettier.languages-markdown");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["[ ] Markdown", "[x] YAML"]);
        click(cx, "settings-toggle-languages.prettier.languages-markdown");
        assert_eq!(
            staged(&model, cx),
            [("global:languages.prettier.languages".to_owned(), "[\"yaml\",\"markdown\"]".to_owned())]
        );
    }

    // Not from a spec: the preview size shows the editor's until one is set; one within 8..72 is
    // staged, one outside is flagged, and Up steps it.
    #[gpui_kit::test]
    async fn the_preview_size_follows_the_editor_until_set_and_keeps_its_range(cx: &mut TestAppContext) {
        const PAGE: &str = "languages.markdown";
        let model = cx.update(|cx| dialog_model(PAGE, &[("editor.fontSize", json!(15.0))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, cx) = open(cx, move |_, cx| PreviewFontSizeView::new(binding, cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[0], "Preview font size: 15 px");
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "5", cx);
        assert!(view.read_with(cx, |view, _| view.is_invalid()));
        assert!(staged(&model, cx).is_empty(), "5 is below the range");
        type_over(&field, "20", cx);
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [("global:languages.markdown.previewFontSize".to_owned(), "21".to_owned())]);
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "21");
    }

    // languages settings › defaults shared JDT LS to off: in a workspace's project scope the
    // checkbox is not editable and says why; in the global scope it is.
    #[gpui_kit::test]
    async fn shared_jdt_ls_is_not_editable_for_a_workspace_project(cx: &mut TestAppContext) {
        const PAGE: &str = "languages.java";
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, cx) = open(cx, move |_, cx| JdtlsSharedView::new(binding, cx));
        view.update(cx, |view, cx| view.set_workspace(true, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(
            lines,
            ["[ ] Share JDT LS across builds (disabled)".to_owned(), format!("    {SHARED_IN_WORKSPACE}")]
        );
        click(cx, "settings-toggle-languages.java.jdtlsShared");
        assert!(staged(&model, cx).is_empty());
    }

    // Not from a spec: a language a plugin formatter covers gets a choice of Automatic and the
    // formatters covering it; the one chosen is staged, and a language none covers is not listed.
    #[gpui_kit::test]
    async fn a_plugin_formatter_is_offered_for_the_languages_it_covers(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PLUGIN_FORMATTERS, &[], cx));
        let binding = Binding::fixed(model.clone(), PLUGIN_FORMATTERS, Scope::Global);
        let (view, cx) = open(cx, move |_, cx| PluginFormattersView::new(binding, cx));
        view.update(cx, |view, cx| {
            let known = |id: &str| KnownLanguage { id: id.to_owned(), name: id.to_owned(), servers: Vec::new() };
            view.set_known(vec![known("lua"), known("python")], cx);
            let stylua =
                PluginFormatter { id: "p.stylua".into(), name: "StyLua".into(), languages: vec!["lua".into()] };
            view.set_formatters(vec![stylua], cx);
        });
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["lua: Automatic (language server first) ▾"]);
        click(cx, "settings-formatter-0");
        click(cx, "settings-formatter-0-1");
        assert_eq!(
            staged(&model, cx),
            [("global:languages.pluginFormatters".to_owned(), "{\"lua\":\"p.stylua\"}".to_owned())]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["lua: StyLua ▾"]);
    }

    // Not from a spec: the Xdebug of the selected PHP is shown once the check answers.
    #[gpui_kit::test]
    async fn the_selected_php_shows_its_version_and_xdebug(cx: &mut TestAppContext) {
        const PAGE: &str = "languages.php";
        let stored = [(PHP_INTERPRETER, json!("/opt/php/bin/php"))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        let toolchains = cx.update(|cx| {
            let php = Toolchain {
                path: "/opt/php/bin/php".into(),
                name: "PHP 8.3".into(),
                version: Some("8.3.1".into()),
                ..Toolchain::default()
            };
            let test = SdkDetection {
                found: vec![(ToolchainKind::Php, vec![php])],
                xdebug: vec![("/opt/php/bin/php".into(), Some("3.3.0".into()))],
                ..Default::default()
            };
            cx.set_global(test);
            let toolchains = Toolchains::new(&model, "w1", cx);
            toolchains.detect(ToolchainKind::Php, false, cx);
            toolchains
        });
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, cx) = open(cx, move |_, cx| PhpInterpreterView::new(binding, toolchains, cx));
        // The view asks for Xdebug when drawn; the test answers it as the dialog's caller does.
        let asked: Rc<std::cell::RefCell<Vec<LanguageAsk>>> = Rc::default();
        let keep = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &LanguageEvent, cx| {
                if let LanguageEvent::Ask(ask) = event {
                    keep.borrow_mut().push(ask.clone());
                    super::super::languages_sdk::answer(ask.clone(), "w1", cx);
                }
            })
            .detach();
        });
        view.update(cx, |view, cx| {
            view.xdebug = None;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(asked.borrow().iter().any(|ask| matches!(ask, LanguageAsk::PhpXdebug { .. })));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Version: 8.3.1", "Xdebug: 3.3.0"]);
    }
}
