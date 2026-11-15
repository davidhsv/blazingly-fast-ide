//! Build, Execution, Deployment › CMake: the CMake profiles of the project (`cmake.profiles`), a
//! list with + / −, and the settings of the profile selected: name, build type, generator, the
//! arguments of the configure and build steps, the build directory and the environment.
//!
//! Without stored profiles the page shows the one CMake starts with (Debug); the first edit stores
//! the list. Arguments are edited as a command line and stored as the words it splits into.

use super::tools_record_table::{Column, ReadRows, RecordTable, TableSpec, WriteRows};
use super::{Binding, ControlEvent, WIDE_FIELD, frame, new_field, row, row_label};
use crate::platform::command_line::{format_args, parse_args};
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_core::settings::CMakeProfile;
use std::cell::Cell;
use std::rc::Rc;

pub const PAGE: &str = "build.cmake";
const KEY: &str = "cmake.profiles";

/// The text fields of a profile: (field, label).
const FIELDS: [(&str, &str); 6] = [
    ("name", "Name"),
    ("buildType", "Build type"),
    ("generator", "Generator"),
    ("cmakeArgs", "CMake arguments"),
    ("buildDir", "Build directory"),
    ("buildArgs", "Build arguments"),
];

/// The profiles as the page shows them: those stored, else the default one.
pub fn profiles(binding: &Binding, cx: &App) -> Vec<CMakeProfile> {
    let stored = binding.value(KEY, cx).filter(|value| !value.is_null());
    match stored.map(serde_json::from_value::<Vec<CMakeProfile>>) {
        Some(Ok(profiles)) => profiles,
        Some(Err(error)) => {
            tracing::warn!("Settings: the stored CMake profiles are not readable: {error}");
            vec![CMakeProfile::default()]
        }
        None => vec![CMakeProfile::default()],
    }
}

fn stage_profiles(binding: &Binding, profiles: &[CMakeProfile], cx: &mut App) {
    match serde_json::to_value(profiles) {
        Ok(value) => binding.stage(KEY, value, cx),
        Err(error) => tracing::warn!("Settings: the CMake profiles could not be staged: {error}"),
    }
}

/// The text of field `name` of `profile`.
fn field_text(profile: &CMakeProfile, name: &str) -> String {
    match name {
        "name" => profile.name.clone(),
        "buildType" => profile.build_type.clone(),
        "generator" => profile.generator.clone(),
        "cmakeArgs" => format_args(&profile.cmake_args),
        "buildDir" => profile.build_dir.clone(),
        "buildArgs" => format_args(&profile.build_args),
        _ => String::new(),
    }
}

/// `profile` with field `name` set from `text`.
fn with_field(mut profile: CMakeProfile, name: &str, text: &str) -> CMakeProfile {
    match name {
        "name" => profile.name = text.to_owned(),
        "buildType" => profile.build_type = text.to_owned(),
        "generator" => profile.generator = text.to_owned(),
        "cmakeArgs" => profile.cmake_args = parse_args(text, false),
        "buildDir" => profile.build_dir = text.to_owned(),
        "buildArgs" => profile.build_args = parse_args(text, false),
        _ => {}
    }
    profile
}

pub struct CMakeProfilesView {
    binding: Binding,
    /// The profile selected, shared with the environment table.
    selected: Rc<Cell<usize>>,
    fields: Vec<(&'static str, Entity<TextField>)>,
    env: Entity<RecordTable>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for CMakeProfilesView {}

impl CMakeProfilesView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let selected = Rc::new(Cell::new(0));
        let current = profiles(&binding, cx).into_iter().next().unwrap_or_default();
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        for (name, _) in FIELDS {
            let field = new_field(&field_text(&current, name), None, cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.edit(name, &text, cx);
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
                }
            }));
            fields.push((name, field));
        }
        let (reader, at) = (binding.clone(), selected.clone());
        let read: ReadRows = Rc::new(move |cx: &App| {
            let all = profiles(&reader, cx);
            let env = all.get(at.get()).map(|profile| profile.env.clone()).unwrap_or_default();
            env.into_iter().map(|(name, value)| vec![name, value]).collect()
        });
        let (writer, at) = (binding.clone(), selected.clone());
        let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
            let mut all = profiles(&writer, cx);
            let Some(profile) = all.get_mut(at.get()) else { return };
            profile.env = rows
                .into_iter()
                .map(|row| {
                    let mut cells = row.into_iter();
                    (cells.next().unwrap_or_default(), cells.next().unwrap_or_default())
                })
                .collect();
            stage_profiles(&writer, &all, cx);
        });
        let spec = TableSpec {
            id: "cmake-env",
            columns: vec![
                Column { title: "Name", placeholder: "NAME" },
                Column { title: "Value", placeholder: "value" },
            ],
            empty: "No variables",
        };
        let env = cx.new(|cx| RecordTable::new(&binding, spec, read, write, cx));
        subscriptions.push(cx.subscribe(&env, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone())));
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let mut view = Self { binding, selected, fields, env, _subscriptions: subscriptions };
        view.update_placeholders(&current, cx);
        view
    }

    pub fn profiles(&self, cx: &App) -> Vec<CMakeProfile> {
        profiles(&self.binding, cx)
    }

    /// The profile selected, kept within the list.
    pub fn selected(&self, cx: &App) -> usize {
        self.selected.get().min(self.profiles(cx).len().saturating_sub(1))
    }

    /// The text field of `name` (`name`, `buildType`, `generator`, `cmakeArgs`, `buildDir`,
    /// `buildArgs`).
    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|(own, _)| *own == name).map(|(_, field)| field)
    }

    pub fn env(&self) -> &Entity<RecordTable> {
        &self.env
    }

    fn edit(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        let mut all = self.profiles(cx);
        let at = self.selected(cx);
        let Some(profile) = all.get(at).cloned() else { return };
        all[at] = with_field(profile, name, text);
        stage_profiles(&self.binding, &all, cx);
    }

    /// The fields show the selected profile: one whose text spells what is stored keeps it (an
    /// argument being typed), the others take the stored value.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.profiles(cx).get(self.selected(cx)).cloned() else {
            cx.notify();
            return;
        };
        let mut updates = Vec::new();
        for (name, field) in &self.fields {
            let shown = field.read(cx).text().to_owned();
            let spells = with_field(current.clone(), name, &shown) == current;
            if !spells {
                updates.push((field.clone(), field_text(&current, name)));
            }
        }
        for (field, text) in updates {
            field.update(cx, |field, cx| field.set_text(&text, cx));
        }
        self.update_placeholders(&current, cx);
        cx.notify();
    }

    /// The build directory says the one CMake uses for the build type; the generator says CMake
    /// picks it.
    fn update_placeholders(&mut self, current: &CMakeProfile, cx: &mut Context<Self>) {
        for (name, field) in self.fields.clone() {
            let placeholder = match name {
                "buildDir" => format!("cmake-build-{}", current.build_type.to_lowercase()),
                "generator" => tx(cx, "CMake default"),
                _ => continue,
            };
            if field.read(cx).placeholder() != placeholder {
                field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
            }
        }
    }

    /// Shows profile `index`.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected.set(index);
        let Some(current) = self.profiles(cx).get(self.selected(cx)).cloned() else { return };
        for (name, field) in self.fields.clone() {
            let text = field_text(&current, name);
            field.update(cx, |field, cx| field.set_text(&text, cx));
        }
        self.update_placeholders(&current, cx);
        self.env.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// + : a new profile named after its place, selected.
    pub fn add(&mut self, cx: &mut Context<Self>) {
        let mut all = self.profiles(cx);
        let localization = cx.default_global::<crate::platform::i18n::Localization>();
        let name = localization
            .0
            .tx_built(&["CMake profile ", ""], &[crate::platform::i18n::Param::Number((all.len() + 1) as f64)]);
        all.push(CMakeProfile { name, ..CMakeProfile::default() });
        let index = all.len() - 1;
        stage_profiles(&self.binding, &all, cx);
        self.select(index, cx);
    }

    /// − : the selected profile goes; the one before it is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let mut all = self.profiles(cx);
        if all.is_empty() {
            return;
        }
        let at = self.selected(cx);
        all.remove(at);
        stage_profiles(&self.binding, &all, cx);
        self.select(at.saturating_sub(1), cx);
    }

    /// The page as text: the profiles (`>` the selected one), then `Label: text` per field of the
    /// selected profile and its environment.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let all = self.profiles(cx);
        let at = self.selected(cx);
        let mut out = vec![format!("— {} —", tx(cx, "CMake profiles"))];
        if all.is_empty() {
            out.push(format!("  {}", tx(cx, "No CMake profiles. Click + to add one.")));
            return out;
        }
        out.extend(
            all.iter()
                .enumerate()
                .map(|(index, profile)| format!("{} {}", if index == at { ">" } else { " " }, profile.name)),
        );
        out.push(format!("— {} —", tx(cx, "Profile settings")));
        for (name, label) in FIELDS {
            let text = self.field(name).map(|field| field.read(cx).text().to_owned()).unwrap_or_default();
            out.push(format!("{}: {text}", tx(cx, label)));
        }
        out.push(format!("{}:", tx(cx, "Environment variables")));
        out.extend(self.env.read(cx).lines(cx));
        out
    }
}

/// A section title of the page.
fn section_title(cx: &App, title: &str) -> gpui_kit::Div {
    div().pt(px(8.0)).pb(px(4.0)).text_color(theme::color(cx, "fg-strong")).child(SharedString::from(tx(cx, title)))
}

impl Render for CMakeProfilesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let all = self.profiles(cx);
        let at = self.selected(cx);
        let tool = |cx: &mut Context<Self>, icon: &str, name: &str, enabled: bool| {
            let selector = format!("settings-cmake-{name}");
            let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
                .child(theme::icon(cx, icon, 16.0, color))
        };
        let add = tool(cx, "plus", "add", true).on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(cx)));
        let has_current = !all.is_empty();
        let remove = tool(cx, "minus", "remove", has_current)
            .when(has_current, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        let selected_bg = theme::color(cx, "bg-selected");
        let rows: Vec<AnyElement> = all
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let selector = format!("settings-cmake-profile-{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(super::ROW_HEIGHT - 4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .when(index == at, |el| el.bg(selected_bg))
                    .child(SharedString::from(profile.name.clone()))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(index, cx)))
                    .into_any_element()
            })
            .collect();
        let empty = all.is_empty().then(|| {
            let text = tx(cx, "No CMake profiles. Click + to add one.");
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        let list = div()
            .max_w(px(560.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .child(
                div()
                    .flex()
                    .gap(px(2.0))
                    .p(px(2.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(add)
                    .child(remove),
            )
            .child(div().min_h(px(72.0)).flex().flex_col().children(rows).children(empty));
        let settings = all.get(at).map(|_| {
            let mut column = div().flex().flex_col().child(section_title(cx, "Profile settings"));
            for (name, label) in FIELDS {
                let Some(field) = self.field(name).cloned() else { continue };
                let selector = format!("settings-text-cmake.{name}");
                let text = tx(cx, label);
                column = column.child(
                    row(cx, self.binding.matches(label, cx)).child(row_label(cx, &text, &query)).child(
                        frame(cx, WIDE_FIELD, false)
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector)
                            .child(field),
                    ),
                );
            }
            let env_label = tx(cx, "Environment variables");
            let note = tx(
                cx,
                "Quote arguments containing spaces. Build arguments go to cmake --build; use -- before native build \
                 tool options.",
            );
            column
                .child(
                    row(cx, self.binding.matches("Environment variables", cx)).child(row_label(cx, &env_label, &query)),
                )
                .child(self.env.clone())
                .child(div().pt(px(6.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
        });
        div()
            .id("settings-page-build.cmake-body")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(section_title(cx, "CMake profiles"))
            .child(list)
            .children(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::{Value, json};

    fn stored(
        model: &Entity<crate::settings_model::staging::SettingsDialog>,
        cx: &mut gpui_kit::VisualTestContext,
    ) -> Value {
        let staged = staged(model, cx);
        let (_, value) = staged.last().cloned().expect("an edit is staged");
        serde_json::from_str(&value).expect("JSON")
    }

    // Not from a spec: without stored profiles the Debug profile is shown; arguments typed are
    // stored as words, and the first edit stores the list.
    #[gpui_kit::test]
    async fn arguments_typed_are_stored_as_words(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, cx| CMakeProfilesView::new(binding, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(
            lines[..4],
            ["— CMake profiles —", "> Debug", "— Profile settings —", "Name: Debug"].map(str::to_owned)
        );
        let args = view.read_with(cx, |view, _| view.field("cmakeArgs").cloned()).expect("the field");
        type_over(&args, "-DFOO=\"a b\" -GNinja", cx);
        let profiles = stored(&model, cx);
        assert_eq!(profiles[0]["cmakeArgs"], json!(["-DFOO=a b", "-GNinja"]));
        assert_eq!(profiles[0]["name"], json!("Debug"));
        assert_eq!(args.read_with(cx, |field, _| field.text().to_owned()), "-DFOO=\"a b\" -GNinja", "the draft stays");
    }

    // Not from a spec: + adds a profile named after its place and selects it; its environment is
    // its own; − removes it and selects the one before.
    #[gpui_kit::test]
    async fn add_and_remove_profiles_with_their_own_environment(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, cx| CMakeProfilesView::new(binding, cx));
        click(cx, "settings-cmake-add");
        assert_eq!(view.read_with(cx, |view, cx| view.selected(cx)), 1);
        let name = view.read_with(cx, |view, _| view.field("name").cloned()).expect("the field");
        assert_eq!(name.read_with(cx, |field, _| field.text().to_owned()), "CMake profile 2");
        click(cx, "settings-table-cmake-env-add");
        cx.simulate_input("CC");
        cx.simulate_keystrokes("tab");
        cx.simulate_input("clang");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let profiles = stored(&model, cx);
        assert_eq!(profiles[1]["env"], json!({ "CC": "clang" }));
        assert_eq!(profiles[0]["env"], json!({}));
        click(cx, "settings-cmake-remove");
        let profiles = stored(&model, cx);
        assert_eq!(profiles.as_array().map(Vec::len), Some(1));
        assert_eq!(view.read_with(cx, |view, cx| view.selected(cx)), 0);
    }
}
