use super::*;
use crate::agents::fixtures::connected_store;
use gpui_kit::{ClipboardItem, Image, ImageFormat, Modifiers, TestAppContext, VisualTestContext};
use ide_agent::{AgentConfigValue, AgentConfigVia, AgentProvider, AgentSource, Isolation, NewAgentTask};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<NewTaskEvent>>>;

fn option(id: &str, category: &str, values: &[&str], current: &str) -> AgentConfigOption {
    AgentConfigOption {
        id: id.to_owned(),
        name: if id == "model" { "Model" } else { "Reasoning" }.to_owned(),
        description: None,
        category: Some(category.to_owned()),
        current: Some(current.to_owned()),
        values: values
            .iter()
            .map(|v| AgentConfigValue { value: (*v).to_owned(), name: (*v).to_owned(), description: None })
            .collect(),
        via: AgentConfigVia::Option,
    }
}

/// The agent `fake`: a model list, with effort levels for `big` and `small`, none for `plain`, and no entry for
/// `unknown`.
fn fake() -> AgentProvider {
    AgentProvider {
        id: "fake".into(),
        name: "Fake".into(),
        command: "fake".into(),
        program: "fake".into(),
        args: Vec::new(),
        default_program: None,
        default_args: Vec::new(),
        login_hint: None,
        command_overridden: false,
        command_editable: true,
        command_read_only_reason: None,
        installed: true,
        env: BTreeMap::new(),
        config: vec![option("model", "model", &["big", "small", "plain", "unknown"], "big")],
        thought_levels: BTreeMap::from([
            ("big".to_owned(), vec![option("effort", "thought_level", &["low", "high"], "high")]),
            ("small".to_owned(), vec![option("effort", "thought_level", &["minimal", "medium", "max"], "minimal")]),
            ("plain".to_owned(), Vec::new()),
        ]),
        commands: Vec::new(),
        source: AgentSource::Custom,
    }
}

fn command(name: &str, description: &str, source: AgentCommandSource) -> AgentCommand {
    AgentCommand { name: name.to_owned(), description: description.to_owned(), hint: None, source }
}

fn commands(list: &[(&str, &str)]) -> Vec<AgentCommand> {
    list.iter().map(|(name, description)| command(name, description, AgentCommandSource::Command)).collect()
}

fn open(
    cx: &mut TestAppContext,
    text: &str,
) -> (Entity<NewTaskView>, Entity<AgentsStore>, Events, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let mut store = connected_store(Vec::new()).0;
    store.providers_loaded(Some(vec![fake()]));
    store.update_draft(|draft| draft.provider = "fake".to_owned());
    store.update_composer(HOME, |draft| draft.text = text.to_owned());
    let store = cx.new(|_| store);
    let shared = store.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| NewTaskView::new(shared, cx));
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&view, move |_, event: &NewTaskEvent, _| seen.borrow_mut().push(event.clone())).detach();
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (view, store, events, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

fn chips(view: &Entity<NewTaskView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.chips(cx))
}

fn rows(view: &Entity<NewTaskView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    view.read_with(cx, |view, cx| view.command_rows(cx))
}

fn text(store: &Entity<AgentsStore>, cx: &mut VisualTestContext) -> String {
    store.read_with(cx, |store, _| store.composer(HOME).text)
}

/// Puts `text` in the description, as typed over it: the caret at its end.
fn replace_text(store: &Entity<AgentsStore>, text: &str, cx: &mut VisualTestContext) {
    store.update(cx, |store, cx| {
        store.update_composer(HOME, |draft| draft.text = text.to_owned());
        cx.notify();
    });
    cx.run_until_parked();
}

fn set_commands(view: &Entity<NewTaskView>, list: Vec<AgentCommand>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_commands("fake", list, cx));
    cx.run_until_parked();
}

fn starts(events: &Events) -> Vec<Option<String>> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            NewTaskEvent::Start { project_root } => Some(project_root.clone()),
            _ => None,
        })
        .collect()
}

/// What the window sends to the backend for the page's last Start.
fn started(store: &Entity<AgentsStore>, project_root: Option<&str>, cx: &mut VisualTestContext) -> NewAgentTask {
    let effect = store.update(cx, |store, _| store.start_task(None, project_root));
    match effect {
        Some(AgentEffect::Create { request, .. }) => *request,
        other => panic!("a task to create, not {other:?}"),
    }
}

fn config_of(request: &NewAgentTask) -> Vec<(String, String)> {
    request.config.iter().map(|choice| (choice.id.clone(), choice.value.clone())).collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

/// Opens the model chip and picks `model` from its list.
fn choose_model(view: &Entity<NewTaskView>, model: &str, cx: &mut VisualTestContext) {
    if view.read_with(cx, |view, _| view.picker_entries()).is_none() {
        click(cx, "composer-config-0");
    }
    let entries = view.read_with(cx, |view, _| view.picker_entries()).expect("the model list is open");
    let index = entries.iter().position(|entry| entry.trim_end_matches(" *") == model).expect("the model is listed");
    click(cx, Box::leak(format!("composer-picker-{index}").into_boxed_str()));
}

// ----- the agent's settings -----

#[gpui_kit::test]
async fn switches_model_specific_levels_and_restores_each_draft_effort_before_starting(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "Check the project");
    assert_eq!(chips(&view, cx), ["big · high"]);
    choose_model(&view, "small", cx);
    assert_eq!(chips(&view, cx), ["small · minimal"]);
    assert!(view.read_with(cx, |view, _| view.picker_entries()).is_some(), "the popup stays open");
    for step in ["composer-effort-0", "composer-effort-1", "composer-effort-2"] {
        assert!(cx.debug_bounds(step).is_some(), "{step}");
    }
    assert!(cx.debug_bounds("composer-effort-3").is_none(), "the small model's three levels");
    click(cx, "composer-effort-1");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.picker_entries()), None);
    choose_model(&view, "big", cx);
    assert_eq!(chips(&view, cx), ["big · high"]);
    choose_model(&view, "small", cx);
    assert_eq!(chips(&view, cx), ["small · medium"]);
    let request = started(&store, None, cx);
    assert_eq!(config_of(&request), pairs(&[("model", "small"), ("effort", "medium")]));
}

#[gpui_kit::test]
async fn hides_effort_for_plain_and_unknown_models_and_sends_no_stale_effort(cx: &mut TestAppContext) {
    for model in ["plain", "unknown"] {
        let (view, store, _, cx) = open(cx, "Check the project");
        choose_model(&view, model, cx);
        assert_eq!(chips(&view, cx), [model], "a plain model chip");
        assert_eq!(view.read_with(cx, |view, _| view.picker_entries()), None, "a plain list closes on a pick");
        let request = started(&store, None, cx);
        assert_eq!(config_of(&request), pairs(&[("model", model)]));
    }
}

#[gpui_kit::test]
async fn sends_the_displayed_remembered_model_and_effort_even_when_neither_is_changed(cx: &mut TestAppContext) {
    let (view, store, events, cx) = open(cx, "Check the project");
    assert_eq!(chips(&view, cx), ["big · high"]);
    click(cx, "agents-home-start");
    assert_eq!(starts(&events), [None]);
    let request = started(&store, None, cx);
    assert_eq!(config_of(&request), pairs(&[("model", "big"), ("effort", "high")]));
}

#[gpui_kit::test]
async fn ignores_old_provider_wide_effort_choices_and_refreshes_unsupported_per_model_choices(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "Check the project");
    store.update(cx, |store, cx| {
        store.update_draft(|draft| {
            draft.config = BTreeMap::from([("effort".to_owned(), "low".to_owned())]);
            draft.thought_levels =
                BTreeMap::from([("big".to_owned(), BTreeMap::from([("effort".to_owned(), "removed".to_owned())]))]);
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(chips(&view, cx), ["big · high"]);
    let request = started(&store, None, cx);
    assert!(config_of(&request).contains(&("effort".to_owned(), "high".to_owned())), "{request:?}");
}

// ----- the `/` commands -----

#[gpui_kit::test]
async fn reaches_commands_after_the_first_twelve_with_the_keyboard(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "");
    let names: Vec<String> = (0..30).map(|i| format!("command-{i}")).collect();
    let list: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "")).collect();
    set_commands(&view, commands(&list), cx);
    cx.simulate_input("/");
    cx.run_until_parked();
    assert_eq!(rows(&view, cx).map(|rows| rows.len()), Some(31), "a heading and the 30 commands");
    cx.simulate_keystrokes(&["down"; 29].join(" "));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "/command-29 ");
}

#[gpui_kit::test]
async fn groups_commands_before_skills_skips_headers_wraps_and_hides_empty_groups(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "");
    let list = vec![
        command("triage", "", AgentCommandSource::Skill),
        command("compact", "", AgentCommandSource::Command),
        command("review", "", AgentCommandSource::Skill),
    ];
    set_commands(&view, list, cx);
    cx.simulate_input("/");
    cx.run_until_parked();
    let listed = |active: usize| {
        let mut shown = ["Commands", "/compact", "Skills", "/triage", "/review"].map(str::to_owned).to_vec();
        let row = [1, 3, 4][active];
        shown[row].push_str(" *");
        shown
    };
    assert_eq!(rows(&view, cx), Some(listed(0)));
    cx.simulate_keystrokes("down");
    assert_eq!(rows(&view, cx), Some(listed(1)));
    cx.simulate_keystrokes("up up");
    assert_eq!(rows(&view, cx), Some(listed(2)), "↑ from the first wraps to the last");
    cx.simulate_keystrokes("down");
    assert_eq!(rows(&view, cx), Some(listed(0)), "↓ from the last wraps to the first");
    replace_text(&store, "/tri", cx);
    assert_eq!(rows(&view, cx), Some(vec!["Skills".to_owned(), "/triage *".to_owned()]));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "/triage ");
    replace_text(&store, "/com", cx);
    assert_eq!(rows(&view, cx), Some(vec!["Commands".to_owned(), "/compact *".to_owned()]));
}

#[gpui_kit::test]
async fn names_the_icon_only_start_action_and_disables_it_while_starting(cx: &mut TestAppContext) {
    let (view, _, events, cx) = open(cx, "");
    assert_eq!(view.read_with(cx, |view, cx| view.start_label(cx)), "Start");
    assert!(!view.read_with(cx, |view, cx| view.can_start(cx)));
    cx.simulate_input("Do the task");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, cx| view.can_start(cx)));
    click(cx, "agents-home-start");
    assert_eq!(view.read_with(cx, |view, cx| view.start_label(cx)), "Starting…");
    assert!(!view.read_with(cx, |view, cx| view.can_start(cx)));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(starts(&events).len(), 1, "Enter while starting starts nothing");
    view.update(cx, |view, cx| view.finished(Some("stop here".to_owned()), cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, cx| view.can_start(cx)));
    assert_eq!(view.read_with(cx, |view, cx| view.start_label(cx)), "Start");
    assert!(cx.debug_bounds("agents-home-error").is_some());
}

#[gpui_kit::test]
async fn completes_the_projects_skills_merged_with_the_agents_commands(cx: &mut TestAppContext) {
    let (view, store, events, cx) = open(cx, "");
    let list = commands(&[
        ("compact", "Free up context"),
        ("triage", "One pass over open issues and PRs"),
        ("issue-fix", "Fix an issue"),
    ]);
    set_commands(&view, list, cx);
    cx.simulate_input("/trig");
    cx.run_until_parked();
    let listed = ["Commands", "/triage · One pass over open issues and PRs *"].map(str::to_owned).to_vec();
    assert_eq!(rows(&view, cx), Some(listed));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "/triage ");
    assert!(starts(&events).is_empty(), "Enter completed instead of starting");
    cx.simulate_input("dry-run");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(starts(&events), [None]);
    let request = started(&store, None, cx);
    assert_eq!(request.prompt.text, "/triage dry-run", "sent as typed, so the agent runs the skill");
    assert_eq!(request.provider, "fake");
}

#[gpui_kit::test]
async fn tags_file_references_and_skills_where_they_stand(cx: &mut TestAppContext) {
    let (view, _, _, cx) = open(cx, "");
    set_commands(&view, commands(&[("triage", "One pass over issues")]), cx);
    cx.simulate_input("12 @Cargo.toml and /triage");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, cx| view.tags(cx)), ["@Cargo.toml", "/triage"]);
}

#[gpui_kit::test]
async fn offers_nothing_the_project_does_not_have(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "");
    set_commands(&view, commands(&[("compact", "Free up context")]), cx);
    replace_text(&store, "/trig", cx);
    assert_eq!(rows(&view, cx), None);
    assert!(cx.debug_bounds("agents-home-commands").is_none());
    replace_text(&store, "/com", cx);
    assert_eq!(rows(&view, cx), Some(vec!["Commands".to_owned(), "/compact · Free up context *".to_owned()]));
    assert!(cx.debug_bounds("agents-home-commands").is_some());
}

#[gpui_kit::test]
async fn completes_at_the_caret_without_replacing_the_rest_of_the_message(cx: &mut TestAppContext) {
    let (view, store, events, cx) = open(cx, "");
    set_commands(&view, commands(&[("compact", "Free up context")]), cx);
    cx.simulate_input("/com keep this context");
    cx.simulate_keystrokes(&["left"; 18].join(" "));
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), Some(vec!["Commands".to_owned(), "/compact · Free up context *".to_owned()]));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "/compact keep this context");
    assert_eq!(rows(&view, cx), None, "the list closes");
    assert!(starts(&events).is_empty());
    let focused = view.read_with(cx, |view, cx| view.field().read(cx).focus_handle(cx));
    assert!(cx.update(|window, _| focused.is_focused(window)), "the description keeps the keyboard");
}

#[gpui_kit::test]
async fn completes_a_command_typed_after_other_text_in_place(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "");
    set_commands(&view, commands(&[("triage", "One pass over open issues and PRs")]), cx);
    cx.simulate_input("fix the failing build /tri");
    cx.run_until_parked();
    assert!(rows(&view, cx).is_some());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "fix the failing build /triage ");
}

#[gpui_kit::test]
async fn closes_the_command_list_with_escape_until_the_text_changes(cx: &mut TestAppContext) {
    let (view, store, events, cx) = open(cx, "");
    set_commands(&view, commands(&[("compact", "")]), cx);
    cx.simulate_input("/com");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(rows(&view, cx), None);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(starts(&events).len(), 1, "Enter starts once the list is closed");
    assert_eq!(text(&store, cx), "/com");
}

// ----- paste -----

#[gpui_kit::test]
async fn attaches_the_clipboards_image_when_pasting_without_text(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, "");
    let png = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Png, png))));
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    let images = store.read_with(cx, |store, _| store.composer(HOME).images);
    let (mime_type, data) = ("image/png".to_owned(), "iVBORw0KGgo=".to_owned());
    let expected = AgentImage { mime_type, data, name: Some("image".to_owned()) };
    assert_eq!(images, [expected]);
    assert_eq!(view.read_with(cx, |view, cx| view.images(cx)), ["image"]);
    assert!(cx.debug_bounds("agents-home-image-0").is_some());
    click(cx, "agents-home-image-0-remove");
    assert!(store.read_with(cx, |store, _| store.composer(HOME).images.is_empty()));
}

#[gpui_kit::test]
async fn leaves_a_text_paste_to_the_text_box(cx: &mut TestAppContext) {
    let (_, store, _, cx) = open(cx, "");
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("pasted".to_owned())));
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(text(&store, cx), "pasted");
    assert!(store.read_with(cx, |store, _| store.composer(HOME).images.is_empty()));
}

#[test]
fn encodes_images_in_padded_base64() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

// ----- the workspace member the task runs in -----

#[gpui_kit::test]
async fn sends_the_selected_project_with_local_and_worktree_isolation(cx: &mut TestAppContext) {
    for isolation in [Isolation::Local, Isolation::Worktree] {
        let (view, store, events, cx) = open(cx, "");
        store.update(cx, |store, _| {
            store.update_draft(|draft| {
                draft.isolation = isolation;
                draft.isolation_chosen = true;
            });
        });
        let members = vec!["/project-a".to_owned(), "/project-b".to_owned()];
        view.update(cx, |view, cx| view.set_projects(members, Some("/project-b".to_owned()), cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.project_label()).as_deref(), Some("project-b"));
        click(cx, "agents-home-project");
        let listed = ["project-a · /project-a", "project-b · /project-b *"].map(str::to_owned).to_vec();
        assert_eq!(view.read_with(cx, |view, _| view.project_rows()), Some(listed));
        click(cx, "agents-home-project-0");
        assert_eq!(view.read_with(cx, |view, _| view.project_label()).as_deref(), Some("project-a"));
        let focus = view.read_with(cx, |view, cx| view.focus_handle(cx));
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.simulate_input("Update member");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(starts(&events), [Some("/project-a".to_owned())]);
        let request = started(&store, Some("/project-a"), cx);
        assert_eq!(request.project_root.map(|root| root.0), Some(std::path::PathBuf::from("/project-a")));
        assert_eq!(request.isolation, isolation);
    }
}
