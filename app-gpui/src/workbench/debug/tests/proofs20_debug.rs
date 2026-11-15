//! The adapter's events reach the window while a session lives: the program's output shows in the
//! debug console, a breakpoint the adapter reports changed shows its new state in the Breakpoints
//! dialog, and a hot code replace saying classes changed offers to reload them.

use super::*;
use crate::debugger::breakpoints::model::BreakpointStatus;

/// What the program prints once it resumes from its breakpoint.
const PRINTED: &str = "resumed from the breakpoint";
/// What the adapter says of the breakpoint it unbinds once the program resumes.
const UNBOUND: &str = "Line moved by the compiler";
/// The notice a hot code replace `BUILD_COMPLETE` raises.
const CLASSES_CHANGED: &str = "Classes changed";

/// A program that stops at its breakpoint on `middle`, then keeps running in its loop.
const RESUMING: &str = "start\nmiddle\nloop\n";

/// What the adapter sends when the program resumes, before it runs on: the program's output, the first
/// breakpoint now unbound, and the `hotcodereplace` event java-debug sends once a build replaced classes.
const ON_RESUME: &str = r#"        event('output', {'category': 'stdout', 'output': 'resumed from the breakpoint\n'})
        event('breakpoint', {'reason': 'changed',
                             'breakpoint': {'id': 1, 'verified': False, 'message': 'Line moved by the compiler'}})
        event('hotcodereplace', {'changeType': 'BUILD_COMPLETE', 'message': ''})
"#;

/// The fixture's adapter, sending [`ON_RESUME`] on `continue`: by then the session is known to the window.
fn eventful_adapter() -> String {
    let resumed = "        respond(request, {'allThreadsContinued': True})\n";
    let script = FAKE_ADAPTER.replacen(resumed, &format!("{resumed}{ON_RESUME}"), 1);
    assert!(script.contains("'hotcodereplace'") && script.contains(PRINTED) && script.contains(UNBOUND), "{script}");
    script
}

/// Whether the notice that classes changed is shown with its Reload button.
fn reload_offered(fixture: &Fixture, cx: &mut VisualTestContext) -> bool {
    fixture.workbench.read_with(cx, |w, _| {
        let items = w.notifications().items();
        items.iter().any(|n| n.title == CLASSES_CHANGED && n.actions.iter().any(|action| action.label == "Reload"))
    })
}

/// Whether the Breakpoints dialog lists the breakpoint on the second line unbound, with the adapter's message.
fn listed_unbound(dialog: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |dialog, _| {
        dialog.rows().iter().any(|row| match row {
            DialogRow::Line { entry, status: Some(BreakpointStatus { verified, message, .. }), .. } => {
                entry.line == 1 && !verified && message.as_deref() == Some(UNBOUND)
            }
            _ => false,
        })
    })
}

// Not from a spec: stopped at a breakpoint, nothing of what the adapter sends later shows yet; once the
// program resumes, its output is in the debug console, the breakpoint the adapter unbound is listed so in
// the Breakpoints dialog, and the hot code replace raises "Classes changed" with Reload.
#[gpui_kit::test]
async fn the_adapters_output_breakpoint_and_hot_code_replace_events_show_in_the_window(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, RESUMING);
    std::fs::write(fixture._folders.0.path().join("fake_dap.py"), eventful_adapter()).expect("the adapter is written");
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    fixture.select_main(cx);
    run(cx, "run.debug");
    wait_execution_at(&fixture, 1, "the stop at the breakpoint", cx);
    assert!(!fixture.console(cx).contains(PRINTED), "{}", fixture.console(cx));
    assert!(!reload_offered(&fixture, cx));

    run(cx, "debug.resume");
    long(cx, "the program's output", &|cx| fixture.console(cx).contains(PRINTED));
    long(cx, "the hot code replace notice", &|cx| reload_offered(&fixture, cx));
    assert_eq!(fixture.status(cx), Some(SessionStatus::Running), "the program runs on");

    run(cx, "debug.viewBreakpoints");
    let shown = |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().cloned());
    long(cx, "the Breakpoints dialog", &|cx| shown(cx).is_some());
    let dialog = shown(cx).expect("the dialog");
    long(cx, "the unbound breakpoint", &|cx| listed_unbound(&dialog, cx));
    dialog.update(cx, |_, cx| cx.emit(BreakpointsDialogEvent::Close));
    long(cx, "the dialog to close", &|cx| shown(cx).is_none());

    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}
