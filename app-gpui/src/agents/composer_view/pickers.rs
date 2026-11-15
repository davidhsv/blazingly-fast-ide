//! The composer's side of its settings chips: the session's settings are the task's, and a choice is sent to
//! the agent. The chips, pickers and Agent settings popup are [`crate::agents::config_pickers_view`]'s.

use super::ComposerView;
use crate::agents::config_pickers_view::{self as pickers, PickerHost, Pickers};
use crate::platform::i18n::{I18n, Localization};
use gpui_kit::{App, Context, FocusHandle, Focusable, Window};
use ide_agent::AgentConfigOption;

impl PickerHost for ComposerView {
    fn pickers(&self) -> &Pickers {
        &self.pickers
    }

    fn pickers_mut(&mut self) -> &mut Pickers {
        &mut self.pickers
    }

    fn picker_config(&self, cx: &App) -> Vec<AgentConfigOption> {
        self.store.read(cx).task(&self.task).map(|task| task.config.clone()).unwrap_or_default()
    }

    fn picker_choose(&mut self, option: String, value: String, cx: &mut Context<Self>) {
        self.set_config(option, value, cx);
    }

    fn picker_return_focus(&self, cx: &App) -> FocusHandle {
        self.field.read(cx).focus_handle(cx)
    }
}

impl ComposerView {
    /// The settings chips, as `<label>` with ` · <detail>`, ` [on]` / ` [off]` for a switch.
    pub fn chips(&self, cx: &App) -> Vec<String> {
        let english = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
        pickers::chip_texts(&self.picker_config(cx), i18n)
    }

    /// The entries of the open picker, as `<label>` with ` *` on the highlighted one.
    pub fn picker_entries(&self) -> Option<Vec<String>> {
        self.pickers.entries()
    }

    /// The description drawn beside each entry of the open picker, in the order of [`Self::picker_entries`].
    pub fn picker_details(&self) -> Option<Vec<Option<String>>> {
        self.pickers.details()
    }

    /// The text of the open picker's search field, when it has one.
    pub fn picker_query(&self, cx: &App) -> Option<String> {
        self.pickers.query(cx)
    }

    /// The tooltip of the Agent settings button, when the agent has settings besides the model and its effort.
    pub fn settings_title(&self, cx: &App) -> Option<String> {
        pickers::settings_title(self, cx)
    }

    /// The rows of the open Agent settings popup (see [`pickers::settings_rows`]).
    pub fn settings_rows(&self, cx: &App) -> Option<Vec<String>> {
        pickers::settings_rows(self, cx)
    }

    /// The Agent settings button has the keyboard.
    pub fn settings_button_focused(&self, window: &Window) -> bool {
        self.pickers.settings_button_focused(window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::composer_view::ComposerEvent;
    use crate::agents::fixtures::{calls, connected_store, local_task};
    use crate::agents::store::AgentCall;
    use crate::platform::input;
    use crate::theme;
    use gpui_kit::{Entity, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext, div, prelude::*};
    use ide_agent::{AgentConfigValue, AgentConfigVia};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ComposerEvent>>>;

    /// The composer at the bottom of its window, where its popups open upwards.
    struct AtBottom(Entity<ComposerView>);

    impl Render for AtBottom {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().justify_end().child(self.0.clone())
        }
    }

    /// A select setting: `values` as `(value, name, description)`.
    fn select(
        id: &str,
        name: &str,
        category: &str,
        current: Option<&str>,
        values: &[(&str, &str, Option<&str>)],
    ) -> AgentConfigOption {
        AgentConfigOption {
            id: id.to_owned(),
            name: name.to_owned(),
            description: None,
            category: Some(category.to_owned()),
            current: current.map(str::to_owned),
            values: values
                .iter()
                .map(|(value, name, description)| AgentConfigValue {
                    value: (*value).to_owned(),
                    name: (*name).to_owned(),
                    description: description.map(str::to_owned),
                })
                .collect(),
            via: AgentConfigVia::Option,
        }
    }

    fn allow_all(current: &str) -> AgentConfigOption {
        select("allow_all", "Allow All", "permissions", Some(current), &[("on", "On", None), ("off", "Off", None)])
    }

    /// Copilot's settings: a model with descriptions, its reasoning effort, a mode and Allow All on.
    fn copilot() -> Vec<AgentConfigOption> {
        let models = [("claude-sonnet-5.5", "Claude Sonnet 5.5", Some("1x")), ("gpt-6-sol", "GPT-6 Sol", Some("1x"))];
        let levels =
            [("low", "low", None), ("medium", "medium", None), ("high", "high", None), ("xhigh", "XHigh", None)];
        vec![
            select("model", "Model", "model", Some("claude-sonnet-5.5"), &models),
            select("reasoning_effort", "Reasoning Effort", "thought_level", Some("medium"), &levels),
            select("mode", "Mode", "mode", Some("agent"), &[("agent", "Agent", None), ("plan", "Plan", None)]),
            allow_all("on"),
        ]
    }

    fn open(
        cx: &mut TestAppContext,
        config: Vec<AgentConfigOption>,
    ) -> (Entity<ComposerView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            if !cx.has_global::<input::Keymap>() {
                input::init(cx);
            }
        });
        let mut task = local_task("A");
        task.config = config;
        let store = cx.new(|_| connected_store(vec![task]).0);
        let composer = cx.new(|cx| ComposerView::new(store, "A", cx));
        let shown = composer.clone();
        let (_, cx) = cx.add_window_view(move |_, _| AtBottom(shown));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&composer, move |_, event: &ComposerEvent, _| seen.borrow_mut().push(event.clone())).detach();
            let focus = composer.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (composer, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn entries(view: &Entity<ComposerView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
        view.read_with(cx, |view, _| view.picker_entries())
    }

    fn settings_rows(view: &Entity<ComposerView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
        view.read_with(cx, |view, cx| view.settings_rows(cx))
    }

    fn effect_calls(events: &Events) -> Vec<AgentCall> {
        events.borrow().iter().flat_map(|ComposerEvent::Effects(effects)| calls(effects)).collect()
    }

    fn set(option: &str, value: &str) -> AgentCall {
        AgentCall::SetConfig { task: "A".to_owned(), option: option.to_owned(), value: value.to_owned() }
    }

    fn owned(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[gpui_kit::test]
    async fn steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, copilot());
        click(cx, "composer-config-0");
        assert_eq!(entries(&view, cx), Some(owned(&["Claude Sonnet 5.5 *", "GPT-6 Sol"])));
        for step in ["composer-effort-0", "composer-effort-1", "composer-effort-2", "composer-effort-3"] {
            assert!(cx.debug_bounds(step).is_some(), "{step}");
        }
        assert!(cx.debug_bounds("composer-effort-4").is_none());
        click(cx, "composer-effort-3");
        assert!(entries(&view, cx).is_some(), "the popup stays open after an effort step");
        click(cx, "composer-picker-1");
        assert_eq!(effect_calls(&events), [set("reasoning_effort", "xhigh"), set("model", "gpt-6-sol")]);
        assert!(entries(&view, cx).is_some(), "the popup stays open after a model is picked");
    }

    #[gpui_kit::test]
    async fn filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, copilot());
        click(cx, "composer-config-0");
        assert_eq!(view.read_with(cx, |view, cx| view.picker_query(cx)), Some(String::new()), "a search field");
        cx.simulate_input("sol");
        cx.run_until_parked();
        assert_eq!(entries(&view, cx), Some(owned(&["GPT-6 Sol *"])));
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(effect_calls(&events), [set("model", "gpt-6-sol")]);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(entries(&view, cx), None, "Escape closes the popup");
        let message = view.read_with(cx, |view, cx| view.field().read(cx).focus_handle(cx));
        assert!(cx.update(|window, _| message.is_focused(window)), "the message gets the keyboard back");
    }

    #[gpui_kit::test]
    async fn groups_nested_paths_by_their_first_provider_and_preserves_the_full_value_and_description(
        cx: &mut TestAppContext,
    ) {
        let model = select("model", "Model", "model", None, &[("9router/cmc/Beam", "Beam display", Some("Fast"))]);
        let (view, events, cx) = open(cx, vec![model]);
        click(cx, "composer-config-0");
        assert_eq!(entries(&view, cx), Some(owned(&["9router", "Beam *"])));
        let details = view.read_with(cx, |view, _| view.picker_details());
        assert_eq!(details, Some(vec![None, Some("cmc/Beam · Fast".to_owned())]), "the description beside the name");
        click(cx, "composer-picker-1");
        assert_eq!(effect_calls(&events), [set("model", "9router/cmc/Beam")]);
        assert_eq!(entries(&view, cx), None, "a model list without effort closes on a pick");
    }

    #[gpui_kit::test]
    async fn keeps_plain_ids_ungrouped_even_when_the_display_name_contains_a_slash(cx: &mut TestAppContext) {
        let model = select("model", "Model", "model", Some("plain"), &[("plain", "Local / Beam", Some("Offline"))]);
        let (view, _, cx) = open(cx, vec![model]);
        click(cx, "composer-config-0");
        assert_eq!(entries(&view, cx), Some(owned(&["Local / Beam *"])));
        assert_eq!(view.read_with(cx, |view, _| view.picker_details()), Some(vec![Some("Offline".to_owned())]));
    }

    #[gpui_kit::test]
    async fn leaves_non_model_option_labels_and_descriptions_intact(cx: &mut TestAppContext) {
        let mode = select("mode", "Mode", "mode", Some("a/One"), &[("a/One", "Custom", Some("Description"))]);
        let (view, _, cx) = open(cx, vec![mode]);
        click(cx, "composer-config-0");
        assert_eq!(entries(&view, cx), Some(owned(&["Custom *"])));
        assert_eq!(view.read_with(cx, |view, _| view.picker_details()), Some(vec![Some("Description".to_owned())]));
        assert_eq!(view.read_with(cx, |view, cx| view.picker_query(cx)), None, "only the model list is searched");
    }

    #[gpui_kit::test]
    async fn shows_allow_all_inside_the_mode_popup_when_both_options_exist(cx: &mut TestAppContext) {
        let modes = [("agent", "Agent", None), ("plan", "Plan", None), ("autopilot", "Autopilot", None)];
        let mode = select("mode", "Mode", "mode", Some("agent"), &modes);
        let (view, events, cx) = open(cx, vec![mode, allow_all("off")]);
        click(cx, "composer-config-0");
        assert_eq!(entries(&view, cx), Some(owned(&["Agent *", "Plan", "Autopilot"])));
        click(cx, "composer-picker-1");
        assert_eq!(entries(&view, cx), None, "a mode pick closes the popup");
        click(cx, "composer-config-0");
        click(cx, "composer-allow-all");
        assert_eq!(effect_calls(&events), [set("mode", "plan"), set("allow_all", "on")]);
        assert!(entries(&view, cx).is_some(), "the switch keeps the popup open");
    }

    #[gpui_kit::test]
    async fn edits_secondary_settings_from_the_compact_popup_and_restores_focus_when_dismissed(
        cx: &mut TestAppContext,
    ) {
        let values = [("ask", "Ask first", None), ("bypass", "Bypass approvals", None)];
        let approvals = select("approvals", "Approvals", "approval", Some("bypass"), &values);
        let (view, events, cx) = open(cx, vec![approvals]);
        let title = view.read_with(cx, |view, cx| view.settings_title(cx));
        assert_eq!(title.as_deref(), Some("Agent settings: Approvals: Bypass approvals"));
        click(cx, "composer-settings");
        assert_eq!(settings_rows(&view, cx), Some(owned(&["Approvals: Bypass approvals"])));
        click(cx, "composer-setting-0");
        let listed = owned(&["Approvals: Bypass approvals", "  Ask first", "  Bypass approvals *"]);
        assert_eq!(settings_rows(&view, cx), Some(listed));
        click(cx, "composer-setting-0-0");
        assert_eq!(effect_calls(&events), [set("approvals", "ask")]);
        assert!(cx.debug_bounds("composer-settings-popup").is_some(), "the popup stays open after a choice");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.debug_bounds("composer-settings-popup").is_none());
        assert!(cx.update(|window, cx| view.read(cx).settings_button_focused(window)), "the button has the keyboard");
    }

    #[gpui_kit::test]
    async fn flips_a_switch_from_the_compact_popup_and_keeps_it_open(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, vec![allow_all("off")]);
        click(cx, "composer-settings");
        assert_eq!(settings_rows(&view, cx), Some(owned(&["Allow All [off]"])));
        click(cx, "composer-setting-0");
        assert_eq!(effect_calls(&events), [set("allow_all", "on")]);
        assert!(settings_rows(&view, cx).is_some());
    }
}
