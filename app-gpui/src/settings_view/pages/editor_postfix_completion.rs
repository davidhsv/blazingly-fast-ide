//! Editor › General › Postfix Completion: whether `expr.key` expands at all, with Tab and in code
//! completion, and which templates of each language set are on.
//!
//! The settings are IDE-level UI state kept in the UI storage under [`POSTFIX_KEY`], in the JSON
//! the other app reads as well, not in the settings files: every change is stored at once, so the
//! page stages nothing and Apply, Reset and Cancel leave it alone. A change stored by another
//! window is shown as soon as it is stored.

use super::{Binding, ControlEvent, description_cell, row};
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{check_box, search_label, tx};
use crate::templates::postfix_model::{POSTFIX_GROUPS, PostfixTemplate, postfix_templates};
use crate::templates::postfix_store::{POSTFIX_KEY, PostfixSettings};
use crate::templates::storage::StorageWrite;
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};

pub const PAGE: &str = "editor.general.postfixCompletion";

const ENABLE: &str = "Enable postfix completion";
const WITH_TAB: &str = "Expand templates with Tab";
const IN_COMPLETION: &str = "Show postfix templates in code completion";
const HINT_START: &str = "Type an expression followed by";
const HINT_END: &str = ", then press Tab.";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 3] = [ENABLE, WITH_TAB, IN_COMPLETION];

/// The settings as stored, the defaults while nothing is.
pub fn stored_postfix_settings(cx: &App) -> PostfixSettings {
    PostfixSettings::parse(cx.try_global::<UiStorage>().and_then(|storage| storage.get(POSTFIX_KEY)))
}

/// Stores `write` in the UI storage.
fn store(write: StorageWrite, cx: &mut App) {
    cx.default_global::<UiStorage>().set(&write.key, write.value);
}

/// The templates of the language set whose ids start with `group` and a dot, in their order.
fn templates_of(group: &str) -> Vec<&'static PostfixTemplate> {
    let prefix = format!("{group}.");
    postfix_templates().iter().filter(|template| template.id.starts_with(&prefix)).collect()
}

pub struct PostfixPageView {
    binding: Binding,
    settings: PostfixSettings,
    /// The id prefix of the language set shown.
    group: &'static str,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for PostfixPageView {}

impl PostfixPageView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
            // Another window stored other settings: show them.
            cx.observe_global::<UiStorage>(|view: &mut Self, cx| {
                let stored = stored_postfix_settings(cx);
                if stored != view.settings {
                    view.settings = stored;
                    cx.notify();
                }
            }),
        ];
        let settings = stored_postfix_settings(cx);
        Self { binding, settings, group: POSTFIX_GROUPS[0].0, _subscriptions: subscriptions }
    }

    pub fn settings(&self) -> &PostfixSettings {
        &self.settings
    }

    /// The id prefix of the language set shown (`ts`, `rust`…).
    pub fn group(&self) -> &str {
        self.group
    }

    fn commit(&mut self, write: StorageWrite, cx: &mut Context<Self>) {
        store(write, cx);
        cx.notify();
    }

    pub fn toggle_enabled(&mut self, cx: &mut Context<Self>) {
        self.settings.enabled = !self.settings.enabled;
        let write = self.settings.write();
        self.commit(write, cx);
    }

    /// Expand templates with Tab; nothing while postfix completion is off, as its box is disabled.
    pub fn toggle_expand_with_tab(&mut self, cx: &mut Context<Self>) {
        if !self.settings.enabled {
            return;
        }
        self.settings.expand_with_tab = !self.settings.expand_with_tab;
        let write = self.settings.write();
        self.commit(write, cx);
    }

    /// Show postfix templates in code completion; nothing while postfix completion is off.
    pub fn toggle_show_in_completion(&mut self, cx: &mut Context<Self>) {
        if !self.settings.enabled {
            return;
        }
        self.settings.show_in_completion = !self.settings.show_in_completion;
        let write = self.settings.write();
        self.commit(write, cx);
    }

    /// Turns template `id` on or off; nothing while postfix completion is off.
    pub fn toggle_template(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.settings.enabled {
            return;
        }
        let on = self.settings.disabled.iter().any(|disabled| disabled == id);
        let write = self.settings.set_template_enabled(id, on);
        self.commit(write, cx);
    }

    /// Shows the templates of the language set `group` (an id prefix of [`POSTFIX_GROUPS`]).
    pub fn select_group(&mut self, group: &str, cx: &mut Context<Self>) {
        if let Some(&(id, _)) = POSTFIX_GROUPS.iter().find(|(id, _)| *id == group) {
            self.group = id;
            cx.notify();
        }
    }

    /// The page as text: the three options, the hint, the language sets with the one shown in
    /// brackets, then `[x] .key | description` per template of that set. A box that cannot be used
    /// ends in ` (disabled)`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mark = |on: bool| if on { "[x]" } else { "[ ]" };
        let off = if self.settings.enabled { "" } else { " (disabled)" };
        let mut out = vec![
            format!("{} {}", mark(self.settings.enabled), tx(cx, ENABLE)),
            format!("{} {}{off}", mark(self.settings.expand_with_tab), tx(cx, WITH_TAB)),
            format!("{} {}{off}", mark(self.settings.show_in_completion), tx(cx, IN_COMPLETION)),
            format!("{} .key, e.g. user.name.nn or items.for{}", tx(cx, HINT_START), tx(cx, HINT_END)),
        ];
        let tabs: Vec<String> = POSTFIX_GROUPS
            .iter()
            .map(|&(id, title)| if id == self.group { format!("[{title}]") } else { title.to_owned() })
            .collect();
        out.push(tabs.join(" | "));
        for template in templates_of(self.group) {
            let on = !self.settings.disabled.contains(&template.id);
            out.push(format!("  {} .{} | {}{off}", mark(on), template.key, template.description));
        }
        out
    }

    /// An option's checkbox drawn as `selector`; a click runs `flip` unless the box is disabled.
    fn option_row(
        &self,
        selector: &'static str,
        label: &str,
        on: bool,
        enabled: bool,
        flip: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.binding.query(cx);
        let text = tx(cx, label);
        row(cx, self.binding.matches(label, cx))
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .when(!enabled, |el| el.opacity(0.5))
            .child(check_box(cx, on))
            .child(search_label(cx, &text, &query))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| flip(view, cx))))
            .into_any_element()
    }

    fn hint(cx: &App) -> AnyElement {
        let code = |text: &str| (true, text.to_owned());
        let parts = [
            (false, tx(cx, HINT_START)),
            code(".key"),
            (false, ", e.g.".to_owned()),
            code("user.name.nn"),
            (false, "or".to_owned()),
            code("items.for"),
            (false, tx(cx, HINT_END)),
        ];
        super::hint_flow(cx, &parts)
    }

    fn group_tab(&self, id: &'static str, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("settings-postfix-group-{id}");
        let active = id == self.group;
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
            .child(SharedString::from(title))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_group(id, cx)))
            .into_any_element()
    }

    fn template_row(&self, template: &'static PostfixTemplate, cx: &mut Context<Self>) -> AnyElement {
        let selector = format!("settings-postfix-template-{}", template.id);
        let enabled = self.settings.enabled;
        let on = !self.settings.disabled.contains(&template.id);
        let id = template.id.as_str();
        div()
            .min_h(theme::length(cx, "h-row"))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .when(!enabled, |el| el.opacity(0.5))
                    .child(check_box(cx, on))
                    .when(enabled, |el| {
                        el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_template(id, cx)))
                    }),
            )
            .child(
                div()
                    .min_w(px(70.0))
                    .font_family(super::code_family(cx))
                    .text_color(theme::color(cx, "fg-emphasis"))
                    .child(SharedString::from(format!(".{}", template.key))),
            )
            .child(description_cell(cx, format!("settings-postfix-detail-{}", template.id), &template.description))
            .into_any_element()
    }
}

impl Render for PostfixPageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        let enable =
            self.option_row("settings-postfix-enabled", ENABLE, settings.enabled, true, Self::toggle_enabled, cx);
        let with_tab = self.option_row(
            "settings-postfix-expandWithTab",
            WITH_TAB,
            settings.expand_with_tab,
            settings.enabled,
            Self::toggle_expand_with_tab,
            cx,
        );
        let in_completion = self.option_row(
            "settings-postfix-showInCompletion",
            IN_COMPLETION,
            settings.show_in_completion,
            settings.enabled,
            Self::toggle_show_in_completion,
            cx,
        );
        let tabs: Vec<AnyElement> = POSTFIX_GROUPS.iter().map(|&(id, title)| self.group_tab(id, title, cx)).collect();
        let rows: Vec<AnyElement> =
            templates_of(self.group).into_iter().map(|template| self.template_row(template, cx)).collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_color(theme::color(cx, "fg"))
            .child(div().flex().flex_wrap().gap_x(px(18.0)).children([enable, with_tab, in_completion]))
            .child(Self::hint(cx))
            .child(div().flex().flex_wrap().gap(px(2.0)).children(tabs))
            .child(
                div()
                    .id("settings-postfix-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .rounded(theme::length(cx, "radius-small"))
                    .children(rows),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{Seen, click, dialog_model, open};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    mod proofs39_java_var_row;
    mod proofs39_zig_tab;

    fn page(cx: &mut TestAppContext) -> (Entity<PostfixPageView>, Seen, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(UiStorage::default()));
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        open(cx, move |_, cx| PostfixPageView::new(Binding::fixed(model, PAGE, Scope::Global), cx))
    }

    fn stored(cx: &mut VisualTestContext) -> PostfixSettings {
        cx.update(|_, cx| stored_postfix_settings(cx))
    }

    // src/features/templates/__tests__/views.test.tsx › settings pages › toggles postfix templates
    #[gpui_kit::test]
    async fn toggles_postfix_templates(cx: &mut TestAppContext) {
        let (view, _, cx) = page(cx);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[4], "[JavaScript / TypeScript] | Rust | Python | Go | Zig | Java | Kotlin");
        assert!(lines.contains(&"  [x] .if | if (expr)".to_owned()), "{lines:?}");
        click(cx, "settings-postfix-template-ts.if");
        assert!(stored(cx).disabled.contains(&"ts.if".to_owned()));
        assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"  [ ] .if | if (expr)".to_owned()));
        click(cx, "settings-postfix-template-ts.if");
        assert!(!stored(cx).disabled.contains(&"ts.if".to_owned()));
    }

    // Not from a spec: each option is stored at once in the JSON the other app reads; with postfix
    // completion off its two options and every template are disabled and clicks change nothing; a
    // language set's tab shows its templates.
    #[gpui_kit::test]
    async fn options_are_stored_at_once_and_off_disables_the_rest(cx: &mut TestAppContext) {
        let (view, seen, cx) = page(cx);
        click(cx, "settings-postfix-expandWithTab");
        click(cx, "settings-postfix-showInCompletion");
        let raw = cx.update(|_, cx| cx.global::<UiStorage>().get(POSTFIX_KEY).map(str::to_owned));
        let json: serde_json::Value = serde_json::from_str(&raw.expect("the settings are stored")).expect("JSON");
        assert_eq!(
            json,
            serde_json::json!({ "enabled": true, "expandWithTab": false, "showInCompletion": false, "disabled": [] })
        );
        click(cx, "settings-postfix-enabled");
        assert!(!stored(cx).enabled);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[1], "[ ] Expand templates with Tab (disabled)");
        assert!(lines.contains(&"  [x] .if | if (expr) (disabled)".to_owned()), "{lines:?}");
        click(cx, "settings-postfix-expandWithTab");
        click(cx, "settings-postfix-template-ts.if");
        let off =
            PostfixSettings { enabled: false, expand_with_tab: false, show_in_completion: false, disabled: vec![] };
        assert_eq!(stored(cx), off);
        click(cx, "settings-postfix-group-rust");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[4], "JavaScript / TypeScript | [Rust] | Python | Go | Zig | Java | Kotlin");
        assert!(lines.contains(&"  [x] .if | if expr (disabled)".to_owned()), "{lines:?}");
        assert!(seen.borrow().is_empty(), "the page asks nothing of the dialog");
    }

    // Not from a spec: settings another window stores are shown at once, and unreadable ones read as
    // the defaults.
    #[gpui_kit::test]
    async fn settings_stored_elsewhere_are_shown_at_once(cx: &mut TestAppContext) {
        let (view, _, cx) = page(cx);
        let saved = r#"{"enabled":true,"disabled":["ts.if"]}"#.to_owned();
        cx.update(|_, cx| cx.global_mut::<UiStorage>().set(POSTFIX_KEY, saved));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"  [ ] .if | if (expr)".to_owned()));
        cx.update(|_, cx| cx.global_mut::<UiStorage>().set(POSTFIX_KEY, "not JSON".to_owned()));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.settings().clone()), PostfixSettings::default());
    }
}
