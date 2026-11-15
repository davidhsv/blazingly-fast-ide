use super::proofs13_agents_fixtures::*;
use super::*;
use crate::agents::composer_view::ComposerView;
use serde_json::{Value, json};

/// An ACP select setting: `values` as `(value, name)`.
fn select(id: &str, name: &str, category: &str, current: Option<&str>, values: &[(&str, &str)]) -> Value {
    let options: Vec<Value> = values.iter().map(|(value, name)| json!({ "value": value, "name": name })).collect();
    json!({
        "id": id,
        "name": name,
        "category": category,
        "type": "select",
        "currentValue": current,
        "options": options,
    })
}

fn allow_all(current: &str) -> Value {
    select("allow_all", "Allow All", "permissions", Some(current), &[("on", "On"), ("off", "Off")])
}

/// Copilot's settings: a model, its reasoning effort (with `effort` levels, none without), a mode and Allow All on.
fn copilot(effort: Option<&[(&str, &str)]>) -> Value {
    let model = select(
        "model",
        "Model",
        "model",
        Some("claude-sonnet-5.5"),
        &[("claude-sonnet-5.5", "Claude Sonnet 5.5"), ("gpt-6-sol", "GPT-6 Sol")],
    );
    let mode = select(
        "mode",
        "Mode",
        "mode",
        Some("https://agentclientprotocol.com/protocol/session-modes#agent"),
        &[
            ("https://agentclientprotocol.com/protocol/session-modes#agent", "Agent"),
            ("https://agentclientprotocol.com/protocol/session-modes#plan", "Plan"),
        ],
    );
    let mut options = vec![model];
    if let Some(levels) = effort {
        options.push(select("reasoning_effort", "Reasoning Effort", "thought_level", Some("medium"), levels));
    }
    options.extend([mode, allow_all("on")]);
    Value::Array(options)
}

const LEVELS: &[(&str, &str)] = &[("low", "low"), ("medium", "medium"), ("high", "high"), ("xhigh", "XHigh")];

/// Runs `run` in a project whose agent reports `settings`, on a task whose first turn ended, once its docked
/// composer shows the session's settings.
fn with_settings(cx: &mut TestAppContext, settings: Value, run: impl FnOnce(Fixture<'_>)) {
    let home = start(cx);
    let (folder, workbench, store, cx) = image_project(cx, home.path(), Some(settings));
    let task = start_with(&workbench, &store, "Check the project", Vec::new(), Vec::new(), cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    wait_long(cx, "the session's settings", |cx| !chips(&composer, cx).is_empty());
    run(Fixture { folder: &folder, composer: &composer, cx: &mut *cx });
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// The project's folder (where the agent logs what it is given), the docked composer and the window.
struct Fixture<'a> {
    folder: &'a tempfile::TempDir,
    composer: &'a Entity<ComposerView>,
    cx: &'a mut VisualTestContext,
}

fn chips(composer: &Entity<ComposerView>, cx: &mut VisualTestContext) -> Vec<String> {
    composer.read_with(cx, |composer, cx| composer.chips(cx))
}

fn entries(composer: &Entity<ComposerView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    composer.read_with(cx, |composer, _| composer.picker_entries())
}

fn open_chip(cx: &mut VisualTestContext, index: usize) {
    click_on(cx, leak(format!("composer-config-{index}")));
}

/// Clicks the entry labelled `label` of the open picker.
fn choose(composer: &Entity<ComposerView>, label: &str, cx: &mut VisualTestContext) {
    let shown = entries(composer, cx).expect("the picker is open");
    let index = shown.iter().position(|entry| entry.trim_end_matches(" *") == label);
    let index = index.unwrap_or_else(|| panic!("{label} in {shown:?}"));
    click_on(cx, leak(format!("composer-picker-{index}")));
}

/// Waits for the agent to have been given `setting` (`<id>=<value>`).
fn wait_for_setting(folder: &tempfile::TempDir, setting: &str, cx: &mut VisualTestContext) {
    wait_long(cx, setting, |_| agent_settings(folder).iter().any(|given| given == setting));
}

fn wait_for_chips(composer: &Entity<ComposerView>, expected: &[&str], cx: &mut VisualTestContext) {
    wait_long(cx, &format!("the chips {expected:?}"), |cx| chips(composer, cx) == expected);
}

/// The model of a router, named by its path, is listed under its provider; choosing it sends the whole id.
#[gpui_kit::test]
async fn sends_the_complete_nested_model_id_through_config_pickers(cx: &mut TestAppContext) {
    let settings = json!([select("model", "Model", "model", None, &[("9router/cmc/Beam", "Beam display")])]);
    with_settings(cx, settings, |Fixture { folder, composer, cx }| {
        assert_eq!(chips(composer, cx), ["Default"]);
        open_chip(cx, 0);
        assert_eq!(entries(composer, cx).as_deref(), Some(["9router", "Beam *"].map(str::to_owned).as_slice()));
        choose(composer, "Beam", cx);
        wait_for_setting(folder, "model=9router/cmc/Beam", cx);
        assert_eq!(agent_settings(folder), ["model=9router/cmc/Beam"]);
        wait_for_chips(composer, &["Beam display"], cx);
    });
}

/// An on/off permissions setting is a switch: a click turns it on, sending its on value.
#[gpui_kit::test]
async fn shows_allow_all_as_a_switch_and_sends_on_off_values(cx: &mut TestAppContext) {
    with_settings(cx, json!([allow_all("off")]), |Fixture { folder, composer, cx }| {
        assert_eq!(chips(composer, cx), ["Allow All [off]"]);
        open_chip(cx, 0);
        wait_for_setting(folder, "allow_all=on", cx);
        assert_eq!(agent_settings(folder), ["allow_all=on"]);
        assert!(entries(composer, cx).is_none(), "a switch opens no list");
        wait_for_chips(composer, &["Allow All [on]"], cx);
    });
}

/// With a mode and Allow All, the mode's picker lists the modes and carries the Allow All switch: a mode is sent
/// from the list, then the switch turns Allow All on, which the chip then names after the mode.
#[gpui_kit::test]
async fn shows_allow_all_inside_the_mode_popup_when_both_options_exist(cx: &mut TestAppContext) {
    let modes = [("agent", "Agent"), ("plan", "Plan"), ("autopilot", "Autopilot")];
    let mode = select("mode", "Mode", "mode", Some("agent"), &modes);
    with_settings(cx, json!([mode, allow_all("off")]), |Fixture { folder, composer, cx }| {
        assert_eq!(chips(composer, cx), ["Agent"], "one chip for the mode and its switch");
        open_chip(cx, 0);
        let listed = ["Agent *", "Plan", "Autopilot"].map(str::to_owned);
        assert_eq!(entries(composer, cx).as_deref(), Some(listed.as_slice()));
        choose(composer, "Plan", cx);
        wait_for_setting(folder, "mode=plan", cx);
        wait_for_chips(composer, &["Plan"], cx);
        open_chip(cx, 0);
        click_on(cx, "composer-allow-all");
        wait_for_setting(folder, "allow_all=on", cx);
        assert_eq!(agent_settings(folder), ["mode=plan", "allow_all=on"]);
        wait_for_chips(composer, &["Plan · Allow All"], cx);
    });
}

/// With a model and its reasoning effort, one chip shows the model with the agent's name for the effort level, and
/// the mode's chip shows Allow All while it is on; the effort has no chip of its own.
#[gpui_kit::test]
async fn shows_the_model_with_the_agents_effort_name_and_the_mode_with_allow_all(cx: &mut TestAppContext) {
    with_settings(cx, copilot(Some(LEVELS)), |Fixture { composer, cx, .. }| {
        assert_eq!(chips(composer, cx), ["Claude Sonnet 5.5 · medium", "Agent · Allow All"]);
    });
}

/// Without an effort option the model chip is a plain list of the models, with no effort steps.
#[gpui_kit::test]
async fn keeps_a_plain_model_picker_when_the_agent_has_no_effort_option(cx: &mut TestAppContext) {
    with_settings(cx, copilot(None), |Fixture { composer, cx, .. }| {
        assert_eq!(chips(composer, cx), ["Claude Sonnet 5.5", "Agent · Allow All"]);
        open_chip(cx, 0);
        let listed = ["Claude Sonnet 5.5 *", "GPT-6 Sol"].map(str::to_owned);
        assert_eq!(entries(composer, cx).as_deref(), Some(listed.as_slice()));
        assert!(cx.debug_bounds("composer-effort-0").is_none(), "no effort steps");
    });
}

/// Approvals are a setting of their own beside the execution mode: their chip opens their values, and the choice is
/// sent and shown.
#[gpui_kit::test]
async fn shows_approvals_separately_from_the_execution_mode_and_sends_the_choice(cx: &mut TestAppContext) {
    let mode = select("mode", "Mode", "mode", Some("Agent"), &[("Agent", "Agent"), ("Plan", "Plan"), ("Ask", "Ask")]);
    let approvals =
        select("approvals", "Approvals", "approval", Some("ask"), &[("ask", "ask"), ("bypass", "Bypass approvals")]);
    with_settings(cx, json!([mode, approvals]), |Fixture { folder, composer, cx }| {
        assert_eq!(chips(composer, cx), ["Agent", "ask"]);
        open_chip(cx, 1);
        choose(composer, "Bypass approvals", cx);
        wait_for_setting(folder, "approvals=bypass", cx);
        assert_eq!(agent_settings(folder), ["approvals=bypass"]);
        wait_for_chips(composer, &["Agent", "Bypass approvals"], cx);
    });
}

/// The model picker lists plain ids first, then each provider's models under the provider, providers in the order
/// they first appear and models in their order.
#[gpui_kit::test]
async fn collects_several_providers_in_encounter_order_and_preserves_order_inside_each_group(cx: &mut TestAppContext) {
    let values = ["a/One", "b/Two", "a/Three", "plain", "__section__:a"].map(|value| (value, value));
    let settings = json!([select("model", "Model", "model", Some("plain"), &values)]);
    with_settings(cx, settings, |Fixture { composer, cx, .. }| {
        open_chip(cx, 0);
        let listed = ["plain *", "__section__:a", "a", "One", "Three", "b", "Two"].map(str::to_owned);
        assert_eq!(entries(composer, cx).as_deref(), Some(listed.as_slice()));
    });
}

/// A model id without a path stays ungrouped under its own name, even when the name has a slash.
#[gpui_kit::test]
async fn keeps_plain_ids_ungrouped_even_when_the_display_name_contains_a_slash(cx: &mut TestAppContext) {
    let settings = json!([select("model", "Model", "model", Some("plain"), &[("plain", "Local / Beam")])]);
    with_settings(cx, settings, |Fixture { composer, cx, .. }| {
        assert_eq!(chips(composer, cx), ["Local / Beam"]);
        open_chip(cx, 0);
        assert_eq!(entries(composer, cx).as_deref(), Some(["Local / Beam *".to_owned()].as_slice()));
    });
}

/// A setting other than the model lists its values by their own names, a path-like value included.
#[gpui_kit::test]
async fn leaves_non_model_option_labels_and_descriptions_intact(cx: &mut TestAppContext) {
    let settings = json!([select("mode", "Mode", "mode", Some("a/One"), &[("a/One", "Custom")])]);
    with_settings(cx, settings, |Fixture { composer, cx, .. }| {
        assert_eq!(chips(composer, cx), ["Custom"]);
        open_chip(cx, 0);
        assert_eq!(entries(composer, cx).as_deref(), Some(["Custom *".to_owned()].as_slice()));
    });
}
