//! Every built-in Settings page, visited through File › Settings…: one control a user can change is
//! changed with the mouse, applied, and read back from the settings in effect.

use super::*;
use crate::settings_model::builtin_pages::settings_registry;
use crate::settings_model::page_spec::OptionSpec;
use crate::settings_view::controls::UNAVAILABLE;

/// A selector built at run time, for the test helpers that take a `'static` one.
fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The value of setting `key` in effect, as text.
fn effective_text(key: &str, cx: &mut VisualTestContext) -> Option<String> {
    let value = cx.update(|_, cx| setting_of(&crate::platform::settings::effective("w1", cx), key))?;
    Some(value.as_str().map_or_else(|| value.to_string(), str::to_owned))
}

/// What a page's first changeable control is changed to: the selectors to click (a second one for a
/// drop-down's choice), the text then typed over the field's text, the setting and the value it then holds.
struct Change {
    clicks: Vec<&'static str>,
    typed: Option<&'static str>,
    key: &'static str,
    expected: String,
}

/// The first control of the page that is drawn and can be changed by a click: a checkbox, a radio button
/// or a drop-down choice other than the current one; else the first text field, typed into.
fn first_change(options: &[OptionSpec], cx: &mut VisualTestContext) -> Option<Change> {
    let clicked = options.iter().find_map(|option| clicked_change(option, cx));
    clicked.or_else(|| {
        options.iter().find_map(|option| {
            let OptionSpec::Text { key, .. } = option else { return None };
            let click = selector(format!("settings-text-{key}"));
            cx.debug_bounds(click)?;
            Some(Change { clicks: vec![click], typed: Some("Smoke"), key, expected: "Smoke".to_owned() })
        })
    })
}

/// The change a click makes to `option`, when it is drawn and a click changes it.
fn clicked_change(option: &OptionSpec, cx: &mut VisualTestContext) -> Option<Change> {
    match option {
        OptionSpec::Toggle { key, .. } => {
            let click = selector(format!("settings-toggle-{key}"));
            cx.debug_bounds(click)?;
            let now = effective_text(key, cx).is_some_and(|value| value == "true");
            Some(Change { clicks: vec![click], typed: None, key, expected: (!now).to_string() })
        }
        OptionSpec::Select { key, options, .. } => {
            let now = effective_text(key, cx);
            let (value, _) = options.iter().find(|(value, _)| Some(*value) != now.as_deref())?;
            let radio = selector(format!("settings-radio-{key}-{value}"));
            if cx.debug_bounds(radio).is_some() {
                return Some(Change { clicks: vec![radio], typed: None, key, expected: (*value).to_owned() });
            }
            let open = selector(format!("settings-select-{key}"));
            cx.debug_bounds(open)?;
            let choice = selector(format!("settings-choice-{key}-{value}"));
            Some(Change { clicks: vec![open, choice], typed: None, key, expected: (*value).to_owned() })
        }
        _ => None,
    }
}

/// Each built-in page opens through File › Settings… on its id. A page made of settings controls has one
/// of them changed with the mouse (a checkbox, a radio button, a drop-down; a text field typed into when
/// it has none of those), and Apply puts the new value in effect. Every other page is drawn by a view of
/// its own (Keymap) or says, control by control, that it is not available.
#[gpui_kit::test]
async fn every_settings_page_opens_and_applies_a_change_made_with_the_mouse(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let mut changed = Vec::new();
    let mut failed = Vec::new();
    for spec in settings_registry(false) {
        run_action(cx, "file.settings", Some(serde_json::Value::from(spec.id)));
        let open = settings_dialog_of(&workbench, cx).expect("Settings opens");
        let shown = open.read_with(cx, |view, cx| view.selected_page(cx));
        if shown.as_deref() != Some(spec.id) {
            failed.push(format!("{}: opens on {shown:?}", spec.id));
            click_on(cx, "settings-cancel");
            continue;
        }
        let lines = open.read_with(cx, |view, cx| view.control_lines(cx));
        let options: Vec<OptionSpec> = spec.sections.iter().flat_map(|section| section.options.clone()).collect();
        match first_change(&options, cx) {
            Some(Change { clicks, typed, key, expected }) => {
                for click in clicks {
                    click_on(cx, click);
                }
                if let Some(text) = typed {
                    cx.simulate_keystrokes("cmd-a");
                    cx.simulate_input(text);
                }
                click_on(cx, "settings-apply");
                let now = effective_text(key, cx);
                if now.as_deref() == Some(expected.as_str()) {
                    changed.push(spec.id);
                } else {
                    failed.push(format!("{}: {key} is {now:?} after Apply, not {expected}", spec.id));
                }
            }
            None => {
                let drawn_elsewhere = lines == ["<page view>"];
                let unavailable = lines.iter().all(|line| line.starts_with('—') || line.ends_with(UNAVAILABLE));
                if !drawn_elsewhere && !unavailable {
                    failed.push(format!("{}: nothing to change in {lines:?}", spec.id));
                }
            }
        }
        click_on(cx, "settings-ok");
        assert!(settings_dialog_of(&workbench, cx).is_none(), "OK closes the dialog on {}", spec.id);
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(changed.contains(&"tools.terminal") && changed.len() >= 38, "{changed:?}");
}
