//! The Built-in tab of Tools › Agents in a window of its own, over a dialog model and agents a
//! stand-in lister gives: what it lists and shows, and what the mouse and the keyboard stage.
//! Nothing here reaches the backend: the lister is the test's, and Browse… is answered by the test.

use super::super::test_support::{click, dialog_model, init, type_over};
use super::*;
use crate::settings_model::staging::{Scope, SettingsDialog};
use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
use ide_agent::{AgentCommandReadOnlyReason, AgentSource};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

fn claude(command: &str) -> AgentProvider {
    AgentProvider {
        id: "claude".into(),
        name: "Claude Code".into(),
        command: command.into(),
        program: command.into(),
        args: Vec::new(),
        default_program: Some("claude".into()),
        default_args: Vec::new(),
        login_hint: Some("Run `claude` in a terminal and sign in.".into()),
        command_overridden: command != "claude",
        command_editable: true,
        command_read_only_reason: None,
        installed: true,
        env: BTreeMap::new(),
        config: Vec::new(),
        thought_levels: BTreeMap::new(),
        commands: Vec::new(),
        source: AgentSource::Builtin,
    }
}

fn codex() -> AgentProvider {
    AgentProvider {
        id: "codex".into(),
        name: "Codex".into(),
        default_program: Some("codex".into()),
        command_overridden: false,
        installed: false,
        login_hint: Some("Run `codex login` in a terminal.".into()),
        args: vec!["app-server".into()],
        default_args: vec!["app-server".into()],
        ..claude("codex")
    }
}

/// What the stand-in lister answers, and how many times it was asked.
#[derive(Clone)]
struct Listing {
    answer: Rc<RefCell<Result<Vec<AgentProvider>, String>>>,
    asked: Rc<Cell<usize>>,
}

impl Listing {
    fn of(answer: Result<Vec<AgentProvider>, String>) -> Self {
        Self { answer: Rc::new(RefCell::new(answer)), asked: Rc::default() }
    }

    fn lister(&self) -> AgentLister {
        let listing = self.clone();
        Rc::new(move |then: Then<Result<Vec<AgentProvider>, String>>, cx: &mut App| {
            listing.asked.set(listing.asked.get() + 1);
            let answer = listing.answer.borrow().clone();
            then(answer, cx);
        })
    }
}

/// The Browse… requests the page made, each with the callback that takes the file chosen.
type Browsed = Rc<RefCell<Vec<Then<PathBuf>>>>;

/// The page in a window of its own, the dialog showing it, over what `listing` answers.
fn open(
    cx: &mut TestAppContext,
    listing: &Listing,
) -> (Entity<AgentsPageView>, Entity<SettingsDialog>, Browsed, &mut VisualTestContext) {
    init(cx);
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
    let lister = listing.lister();
    let (view, cx) = cx.add_window_view(move |_, cx| AgentsPageView::new(binding, lister, cx));
    cx.update(|window, _| window.activate_window());
    let browsed: Browsed = Rc::default();
    let log = browsed.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ControlEvent, _| {
            if let ControlEvent::Browse { directories: false, then } = event {
                log.borrow_mut().push(then.clone());
            }
        })
        .detach();
    });
    cx.run_until_parked();
    (view, model, browsed, cx)
}

fn lines(view: &Entity<AgentsPageView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.update(cx, |view, cx| view.lines(cx))
}

fn staged(model: &Entity<SettingsDialog>, key: &str, cx: &mut VisualTestContext) -> Option<Value> {
    model.read_with(cx, |dialog, _| dialog.staged_external(PAGE, key).cloned())
}

fn field_text(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
    field.read_with(cx, |field, _| field.text().to_owned())
}

/// Gives `field` the keyboard and deletes its whole text.
fn clear(field: &Entity<TextField>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn follows_locale_changes_in_tabs_and_renders_login_commands_as_inline_code(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude"), codex()]));
    let (view, _, _, cx) = open(cx, &listing);
    let shown = lines(&view, cx);
    assert_eq!(shown[..3], ["[Built-in]".to_owned(), "> Claude Code".to_owned(), "  Codex (Not found)".to_owned()]);
    assert!(shown.contains(&"Sign in: Run `claude` in a terminal and sign in.".to_owned()), "{shown:?}");
    assert!(cx.debug_bounds("settings-agents-login").is_some());
    let switch = |locale: &str, cx: &mut VisualTestContext| {
        let locale = locale.to_owned();
        assert!(cx.update(|_, cx| cx.default_global::<Localization>().0.set_locale(Some(&locale), "en")));
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
    };
    switch("zh", cx);
    let translated = lines(&view, cx);
    assert_eq!(translated[0], "[内置]");
    assert_eq!(translated[2], "  Codex (未找到)");
    switch("en", cx);
    assert_eq!(lines(&view, cx)[..3], shown[..3]);
}

#[gpui_kit::test]
async fn reports_when_providers_cannot_be_read_no_trusted_project(cx: &mut TestAppContext) {
    let listing = Listing::of(Err("restricted".to_owned()));
    let (view, _, _, cx) = open(cx, &listing);
    assert_eq!(lines(&view, cx), ["[Built-in]", "! restricted"]);
    assert!(cx.debug_bounds("settings-agents-error").is_some());
    assert!(cx.debug_bounds("settings-agents-executable").is_none());
}

// Not from a spec: the executable typed is staged for the agent selected, under its own key, and
// nothing else is; the page has no Save button of its own.
#[gpui_kit::test]
async fn an_executable_typed_is_staged_for_the_agent_selected(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude"), codex()]));
    let (view, model, _, cx) = open(cx, &listing);
    let field = view.read_with(cx, |view, _| view.executable_field().clone());
    assert_eq!(field_text(&field, cx), "");
    assert_eq!(field.read_with(cx, |field, _| field.placeholder().to_owned()), "Auto-detect (claude)");
    type_over(&field, "D:\\tools\\claude.exe", cx);
    assert_eq!(staged(&model, "agents.claude.command", cx), Some(Value::from("D:\\tools\\claude.exe")));
    assert_eq!(staged(&model, "agents.codex.command", cx), None);
    assert!(model.read_with(cx, |dialog, _| BuiltinAgents::modified(dialog, "claude")));
    // The tag follows what is stored, not what is typed.
    assert_eq!(lines(&view, cx)[1], "> Claude Code");
    // Emptying the field again, the stored value, leaves nothing staged.
    clear(&field, cx);
    assert_eq!(field_text(&field, cx), "");
    assert_eq!(staged(&model, "agents.claude.command", cx), None);
}

// Not from a spec: the environment typed is staged, and its values never show in the page's text.
#[gpui_kit::test]
async fn the_environment_typed_is_staged_and_never_listed(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude")]));
    let (view, model, _, cx) = open(cx, &listing);
    let field = view.read_with(cx, |view, _| view.env_field().clone());
    let typed = "ANTHROPIC_API_KEY=sk-ant-secret-value";
    type_over(&field, typed, cx);
    assert_eq!(staged(&model, "agents.claude.env", cx), Some(Value::from(typed)));
    let shown = lines(&view, cx);
    assert!(shown.contains(&"Environment variables".to_owned()), "{shown:?}");
    assert!(!shown.iter().any(|line| line.contains("sk-ant-secret-value")), "{shown:?}");
}

/// An agent the page cannot change the executable of, for `reason`, is listed with its launch
/// command and why, without the executable field or Browse…; a registry agent has no environment
/// field either.
fn read_only_agent_is_shown_with_why(reason: AgentCommandReadOnlyReason, cx: &mut TestAppContext) {
    let registry = reason == AgentCommandReadOnlyReason::Registry;
    let agent = AgentProvider {
        id: "claude-acp".into(),
        name: "Claude Code (ACP)".into(),
        default_program: Some("npx".into()),
        default_args: vec!["-y".into(), "adapter".into()],
        command_editable: false,
        command_read_only_reason: Some(reason),
        source: if registry {
            AgentSource::Registry { version: "1".into(), update: None }
        } else {
            AgentSource::Builtin
        },
        ..claude("npx")
    };
    let listing = Listing::of(Ok(vec![agent]));
    let (view, model, browsed, cx) = open(cx, &listing);
    let shown = lines(&view, cx);
    assert!(shown.iter().any(|line| line.ends_with(" · npx -y adapter")), "{shown:?}");
    let expected = if registry { "installed from the ACP Registry" } else { "runs through a package runner" };
    assert!(shown.iter().any(|line| line.contains(expected)), "{shown:?}");
    assert!(cx.debug_bounds("settings-agents-executable").is_none());
    assert!(cx.debug_bounds("settings-agents-browse").is_none());
    assert_eq!(cx.debug_bounds("settings-agents-env").is_some(), !registry);
    assert!(browsed.borrow().is_empty());
    assert!(model.read_with(cx, |dialog, _| dialog.staged_external(PAGE, "agents.claude-acp.command").is_none()));
}

#[gpui_kit::test]
async fn keeps_package_runner_built_ins_visible_and_read_only(cx: &mut TestAppContext) {
    read_only_agent_is_shown_with_why(AgentCommandReadOnlyReason::PackageRunner, cx);
}

#[gpui_kit::test]
async fn keeps_registry_built_ins_visible_and_read_only(cx: &mut TestAppContext) {
    read_only_agent_is_shown_with_why(AgentCommandReadOnlyReason::Registry, cx);
}

// Not from a spec: a click on an agent of the list, or Up and Down while the list has the keyboard,
// selects it; the fields then show that agent's values.
#[gpui_kit::test]
async fn an_agent_is_selected_with_the_mouse_or_the_keyboard(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("/saved/claude"), codex()]));
    let (view, _, _, cx) = open(cx, &listing);
    let field = view.read_with(cx, |view, _| view.executable_field().clone());
    assert_eq!(field_text(&field, cx), "/saved/claude");
    click(cx, "settings-agents-row-1");
    let shown = lines(&view, cx);
    assert_eq!(shown[2], "> Codex (Not found)");
    assert!(shown.contains(&"Not found on PATH · codex app-server".to_owned()), "{shown:?}");
    assert_eq!(field_text(&field, cx), "");
    assert_eq!(field.read_with(cx, |field, _| field.placeholder().to_owned()), "Auto-detect (codex)");
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    assert_eq!(lines(&view, cx)[1], "> Claude Code (Custom)");
    assert_eq!(field_text(&field, cx), "/saved/claude");
    cx.simulate_keystrokes("end");
    cx.run_until_parked();
    assert_eq!(lines(&view, cx)[2], "> Codex (Not found)");
}

#[gpui_kit::test]
async fn reset_stages_automatic_detection_until_apply(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("/saved/claude")]));
    let (view, model, _, cx) = open(cx, &listing);
    let field = view.read_with(cx, |view, _| view.executable_field().clone());
    click(cx, "settings-agents-reset");
    assert_eq!(field_text(&field, cx), "");
    assert_eq!(staged(&model, "agents.claude.command", cx), Some(Value::from("")));
    assert!(cx.debug_bounds("settings-agents-reset").is_none(), "Reset shows only while the field has a value");
    // What is stored still decides the tag until Apply.
    assert_eq!(lines(&view, cx)[1], "> Claude Code (Custom)");
}

#[gpui_kit::test]
async fn stages_a_browsed_executable(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude")]));
    let (view, model, browsed, cx) = open(cx, &listing);
    click(cx, "settings-agents-browse");
    let then = browsed.borrow().first().cloned().expect("Browse… asks for a file");
    assert_eq!(staged(&model, "agents.claude.command", cx), None);
    cx.update(|_, cx| then(PathBuf::from("/chosen/agent"), cx));
    cx.run_until_parked();
    let field = view.read_with(cx, |view, _| view.executable_field().clone());
    assert_eq!(field_text(&field, cx), "/chosen/agent");
    assert_eq!(staged(&model, "agents.claude.command", cx), Some(Value::from("/chosen/agent")));
}

// Not from a spec: a file chosen after another agent was selected is not staged for either.
#[gpui_kit::test]
async fn a_file_chosen_after_another_agent_was_selected_is_dropped(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude"), codex()]));
    let (_view, model, browsed, cx) = open(cx, &listing);
    click(cx, "settings-agents-browse");
    let then = browsed.borrow().first().cloned().expect("Browse… asks for a file");
    click(cx, "settings-agents-row-1");
    cx.update(|_, cx| then(PathBuf::from("/chosen/agent"), cx));
    cx.run_until_parked();
    assert_eq!(staged(&model, "agents.claude.command", cx), None);
    assert_eq!(staged(&model, "agents.codex.command", cx), None);
}

// Not from a spec: the page reads the agents each time the dialog shows it, and only then; it opens
// on the Built-in tab with the first agent selected, and an edit staged earlier still shows.
#[gpui_kit::test]
async fn the_agents_are_read_each_time_the_dialog_shows_the_page(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude"), codex()]));
    let (view, model, _, cx) = open(cx, &listing);
    assert_eq!(listing.asked.get(), 1);
    let field = view.read_with(cx, |view, _| view.executable_field().clone());
    type_over(&field, "/custom/claude", cx);
    click(cx, "settings-agents-row-1");
    model.update(cx, |dialog, cx| {
        dialog.select_page("other");
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(listing.asked.get(), 1, "a page not shown reads nothing");
    model.update(cx, |dialog, cx| {
        dialog.select_page(PAGE);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(listing.asked.get(), 2);
    assert_eq!(lines(&view, cx)[1], "> Claude Code");
    assert_eq!(field_text(&field, cx), "/custom/claude");
}

// Not from a spec: Enter in a field is the dialog's OK, Escape its Cancel.
#[gpui_kit::test]
async fn enter_and_escape_in_a_field_ask_for_ok_and_cancel(cx: &mut TestAppContext) {
    let listing = Listing::of(Ok(vec![claude("claude")]));
    let (view, _, _, cx) = open(cx, &listing);
    let seen: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let log = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &ControlEvent, _| match event {
            ControlEvent::Submit => log.borrow_mut().push("submit"),
            ControlEvent::Cancel => log.borrow_mut().push("cancel"),
            _ => {}
        })
        .detach();
    });
    let bounds = cx.debug_bounds("settings-agents-env").expect("the environment field is drawn");
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), ["submit", "cancel"]);
}
