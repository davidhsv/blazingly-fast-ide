use super::*;
use ide_agent::AgentConfigVia;

fn value(id: &str, name: &str, description: Option<&str>) -> AgentConfigValue {
    AgentConfigValue { value: id.to_owned(), name: name.to_owned(), description: description.map(str::to_owned) }
}

fn setting(
    id: &str,
    name: &str,
    category: &str,
    current: Option<&str>,
    values: Vec<AgentConfigValue>,
) -> AgentConfigOption {
    AgentConfigOption {
        id: id.to_owned(),
        name: name.to_owned(),
        description: None,
        category: Some(category.to_owned()),
        current: current.map(str::to_owned),
        values,
        via: AgentConfigVia::Option,
    }
}

fn allow_all(current: &str) -> AgentConfigOption {
    let values = vec![value("on", "On", None), value("off", "Off", None)];
    setting("allow_all", "Allow All", "permissions", Some(current), values)
}

fn choice(id: &str, value: &str) -> Option<Choice> {
    Some((id.to_owned(), value.to_owned()))
}

fn copilot(effort: bool) -> Vec<AgentConfigOption> {
    let models =
        vec![value("claude-sonnet-5.5", "Claude Sonnet 5.5", Some("1x")), value("gpt-6-sol", "GPT-6 Sol", Some("1x"))];
    let mut config = vec![setting("model", "Model", "model", Some("claude-sonnet-5.5"), models)];
    if effort {
        let levels =
            ["low", "medium", "high", "xhigh"].map(|l| value(l, if l == "xhigh" { "XHigh" } else { l }, None)).to_vec();
        config.push(setting("reasoning_effort", "Reasoning Effort", "thought_level", Some("medium"), levels));
    }
    let agent = "https://agentclientprotocol.com/protocol/session-modes#agent";
    let plan = "https://agentclientprotocol.com/protocol/session-modes#plan";
    let modes = vec![value(agent, "Agent", None), value(plan, "Plan", None)];
    config.push(setting("mode", "Mode", "mode", Some(agent), modes));
    config.push(allow_all("on"));
    config
}

fn select_of<'a>(bar: &ConfigBar<'a>, id: &str) -> Option<&'a AgentConfigOption> {
    bar.inline.iter().find_map(|p| match p {
        Picker::Select(o) if o.id == id => Some(*o),
        _ => None,
    })
}

#[test]
fn shows_cursor_approvals_separately_from_its_execution_mode_and_sends_the_choice() {
    let modes = ["Agent", "Plan", "Ask"].map(|v| value(v, v, None)).to_vec();
    let mode = setting("mode", "Mode", "mode", Some("Agent"), modes);
    let approvals = vec![value("ask", "ask", None), value("bypass", "Bypass approvals", None)];
    let approval = setting("openidea.cursor.approval", "Approvals", "approval", Some("ask"), approvals);
    let config = [mode, approval];
    let bar = config_bar(&config);
    let values = Values::new();
    let mode = select_of(&bar, "mode").expect("the mode picker");
    assert_eq!(value_of(mode, &values), Some("Agent"));
    let approval = select_of(&bar, "openidea.cursor.approval").expect("a separate approvals picker");
    let list = OptionList::new(approval, &values);
    let filtered = list.filtered();
    let bypass = filtered.iter().find(|e| e.label == "Bypass approvals").expect("the option").value.clone();
    assert_eq!(list.click(&bypass), choice("openidea.cursor.approval", "bypass"));
}

#[test]
fn shows_allow_all_inside_the_mode_popup_when_both_options_exist() {
    let modes = [("agent", "Agent"), ("plan", "Plan"), ("autopilot", "Autopilot")].map(|(v, n)| value(v, n, None));
    let modes = modes.to_vec();
    let config = [setting("mode", "Mode", "mode", Some("agent"), modes), allow_all("off")];
    let bar = config_bar(&config);
    let [Picker::ModeAllowAll { mode, allow_all }] = bar.inline.as_slice() else { panic!("one mode menu") };
    let values = Values::new();
    assert_eq!(OptionList::new(mode, &values).click("plan"), choice("mode", "plan"));
    assert_eq!(switch_choice(allow_all, true), choice("allow_all", "on"));
}

#[test]
fn sends_the_complete_nested_model_id_through_config_pickers() {
    let beam = vec![value("9router/cmc/Beam", "9router/cmc/Beam", None)];
    let config = [setting("model", "Model", "model", None, beam)];
    let bar = config_bar(&config);
    let model = select_of(&bar, "model").expect("the model picker");
    let list = OptionList::new(model, &Values::new());
    let beam = list.filtered().into_iter().find(|e| e.label == "Beam").expect("the model").value.clone();
    assert_eq!(list.click(&beam), choice("model", "9router/cmc/Beam"));
    assert_eq!(list.click("9router"), None);
}

#[test]
fn edits_secondary_settings_from_the_compact_popup_and_restores_focus_when_dismissed() {
    let i18n = I18n::default();
    let approvals = vec![value("ask", "Ask first", None), value("bypass", "Bypass approvals", None)];
    let config = [setting("approvals", "Approvals", "approval", Some("ask"), approvals)];
    let values = Values::from([("approvals".to_owned(), "bypass".to_owned())]);
    let bar = config_bar(&config);
    let [Picker::Select(approval)] = bar.secondary.as_slice() else { panic!("the approvals in the popup") };
    let button = settings_button(&bar.secondary, &values, &i18n);
    assert_eq!(button.title, "Agent settings: Approvals: Bypass approvals");
    assert_eq!(button.icon, mode_icon("bypass"));
    assert_eq!(OptionList::new(approval, &values).click("ask"), choice("approvals", "ask"));
}

#[test]
fn shows_the_model_with_the_agents_effort_name_and_the_mode_with_allow_all() {
    let i18n = I18n::default();
    let config = copilot(true);
    let bar = config_bar(&config);
    let values = Values::new();
    let [Picker::ModelEffort { model, effort }, Picker::ModeAllowAll { mode, allow_all }] = bar.inline.as_slice()
    else {
        panic!("the model and mode buttons, not {:?}", bar.inline)
    };
    let chip = model_chip(model, effort, &values, &i18n);
    assert_eq!((chip.value.as_str(), chip.detail.as_deref()), ("Claude Sonnet 5.5", Some("medium")));
    let chip = mode_chip(mode, allow_all, &values, &i18n);
    assert_eq!((chip.value.as_str(), chip.detail.as_deref()), ("Agent", Some("Allow All")));
}

#[test]
fn steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup() {
    let config = copilot(true);
    let bar = config_bar(&config);
    let values = Values::from([("reasoning_effort".to_owned(), "high".to_owned())]);
    let Picker::ModelEffort { model, effort } = bar.inline[0] else { panic!("the model button") };
    let slider = effort_slider(effort, &values);
    assert_eq!(slider, EffortSlider { max: 3, index: 2, value_text: Some("high".to_owned()) });
    assert_eq!(effort_step(effort, &values, 3), choice("reasoning_effort", "xhigh"));
    assert_eq!(effort_step(effort, &values, 2), None);
    assert_eq!(OptionList::new(model, &values).click("gpt-6-sol"), choice("model", "gpt-6-sol"));
}

#[test]
fn filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard() {
    let config = copilot(true);
    let bar = config_bar(&config);
    let Picker::ModelEffort { model, .. } = bar.inline[0] else { panic!("the model button") };
    let mut list = OptionList::new(model, &Values::new());
    list.set_query("sol");
    assert_eq!(list.filtered().len(), 1);
    assert_eq!(list.key(ListKey::Down), None);
    assert_eq!(list.key(ListKey::Enter), choice("model", "gpt-6-sol"));
}

#[test]
fn keeps_a_plain_model_picker_when_the_agent_has_no_effort_option() {
    let config = copilot(false);
    let bar = config_bar(&config);
    assert!(matches!(bar.inline[0], Picker::Select(o) if o.id == "model"));
}
