use super::*;
use crate::debugger::state::SessionStatus;
use crate::workbench::debug_extras::tests::PROGRAM;

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// Debug on the Services toolbar, with a run configuration selected in the tree, debugs that configuration: the
/// session stops at the breakpoint of its program.
#[gpui_kit::test]
async fn debug_on_the_services_toolbar_debugs_the_selected_configuration(cx: &mut TestAppContext) {
    let (fixture, cx) = open_project(cx, FAKE_ADAPTER, &[("main.fake", PROGRAM)], Some("main.fake"));
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint", None);
    long(cx, "listing the configurations", &|cx| fixture.run_store(cx).read_with(cx, |s, _| s.loaded));
    run(cx, "services.show", None);
    run(cx, "run.services.addType", Some(Value::from("program")));
    click(cx, "services-row:run:config:main");
    assert!(!context(cx, "debugSessionActive"), "nothing is debugged yet");
    click(cx, "services-action:debug");
    long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
    long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:2"]);
    run(cx, "debug.stop", None);
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    fixture.close_project(cx);
}
