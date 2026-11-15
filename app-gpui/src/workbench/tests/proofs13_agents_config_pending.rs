//! Tests waiting for the Model & effort popup to stay open after an effort step or a model pick, and for its
//! model search field; neither is built. Not declared until they exist.

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

/// The model's picker has a step per effort level the agent advertises; the last step sends the highest level, a
/// model picked from the same picker is sent too, and the picker stays open through both.
#[gpui_kit::test]
async fn steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup(cx: &mut TestAppContext) {
    with_settings(cx, copilot(Some(LEVELS)), |Fixture { folder, composer, cx }| {
        open_chip(cx, 0);
        let listed = ["Claude Sonnet 5.5 *", "GPT-6 Sol"].map(str::to_owned);
        assert_eq!(entries(composer, cx).as_deref(), Some(listed.as_slice()), "the models are in the picker itself");
        for step in 0..LEVELS.len() {
            assert!(cx.debug_bounds(leak(format!("composer-effort-{step}"))).is_some(), "step {step}");
        }
        assert!(cx.debug_bounds(leak(format!("composer-effort-{}", LEVELS.len()))).is_none());
        click_on(cx, leak(format!("composer-effort-{}", LEVELS.len() - 1)));
        wait_for_setting(folder, "reasoning_effort=xhigh", cx);
        assert!(entries(composer, cx).is_some(), "the picker stays open after an effort step");
        choose(composer, "GPT-6 Sol", cx);
        wait_for_setting(folder, "model=gpt-6-sol", cx);
        assert!(entries(composer, cx).is_some(), "the picker stays open after a model is picked");
    });
}

/// Typing in the model picker filters its list; ↓ and Enter pick the highlighted model.
#[gpui_kit::test]
async fn filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard(cx: &mut TestAppContext) {
    with_settings(cx, copilot(Some(LEVELS)), |Fixture { folder, composer, cx }| {
        open_chip(cx, 0);
        cx.simulate_input("sol");
        cx.run_until_parked();
        assert_eq!(entries(composer, cx).map(|shown| shown.len()), Some(1), "{:?}", entries(composer, cx));
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        wait_for_setting(folder, "model=gpt-6-sol", cx);
    });
}
