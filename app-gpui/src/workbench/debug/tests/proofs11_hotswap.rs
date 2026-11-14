//! Reload Changed Classes and Cancel Hot Swap Build in a window whose Java debug session is the fake
//! adapter behind the test's Java debug server, with the compile answered by the test instead of jdtls.

use super::*;
use crate::debugger::driver::{JavaBuildAnswer, JavaDebugServerPort};
use ide_lsp::jdtls::JavaBuildStatus;
use std::io::Write as _;
use std::net::{Shutdown, TcpListener};
use std::process::{Command, Stdio};

/// A program that keeps running until it is stopped: the session stays live.
const RUNNING: &str = "start\nloop\nend\n";

/// The fake adapter as java-debug: an attach runs `main.fake` of its folder as a launch would, and
/// `redefineClasses` reloads `com.example.App`, leaving a file `redefined` in its folder.
fn java_adapter() -> String {
    let launch = "    elif command == 'launch':\n        launch = request\n        \
                  source = os.path.join(arguments.get('cwd') or '.', arguments['program'])\n";
    let attach = "    elif command in ('launch', 'attach'):\n        launch = request\n        \
                  source = os.path.join(arguments.get('cwd') or '.', arguments.get('program') or 'main.fake')\n";
    let redefine = "    elif command == 'redefineClasses':\n        open('redefined', 'w').close()\n        \
                    respond(request, {'changedClasses': ['com.example.App']})\n    elif command == 'pause':";
    let script = FAKE_ADAPTER.replacen(launch, attach, 1).replacen("    elif command == 'pause':", redefine, 1);
    assert!(script.contains("'attach'") && script.contains("'redefineClasses'"), "{script}");
    script
}

/// The Java debug server of the test: every connection to its loopback port runs `adapter` over the
/// socket, in `folder`. Its port is what the debugger gets for jdtls' debug server.
fn java_debug_server(adapter: PathBuf, folder: PathBuf) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback port");
    let port = listener.local_addr().expect("the listener is bound").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut child = Command::new("python3")
                .arg(&adapter)
                .current_dir(&folder)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("the adapter starts");
            let (mut input, mut output) =
                (child.stdin.take().expect("the adapter's input"), child.stdout.take().expect("its output"));
            let mut from_debugger = stream.try_clone().expect("the socket is shared");
            let mut to_debugger = stream;
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut from_debugger, &mut input);
                let _ = input.flush();
            });
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut output, &mut to_debugger);
                let _ = to_debugger.shutdown(Shutdown::Both);
                let _ = child.wait();
            });
        }
    });
    port
}

/// A Java debug session of a Remote JVM Debug configuration attached through the test's Java debug
/// server, running; every hot swap compile answers `status` after `after`.
fn java_session(fixture: &Fixture, status: JavaBuildStatus, after: Duration, cx: &mut VisualTestContext) {
    let adapter = fixture._folders.0.path().join("java_dap.py");
    std::fs::write(&adapter, java_adapter()).expect("the adapter script is written");
    let port = java_debug_server(adapter, fixture.root.clone());
    cx.update(|_, cx| {
        cx.set_global(JavaDebugServerPort(port));
        cx.set_global(JavaBuildAnswer { status, after });
    });
    let mut config = blank_config("remote", "Remote", "remote-jvm");
    for (key, value) in [("mode", "attach"), ("host", "127.0.0.1"), ("port", "5005")] {
        config.extras.params.insert(key.to_owned(), value.to_owned());
    }
    let arg = serde_json::json!({ "config": serde_json::to_value(&config).expect("the configuration as JSON") });
    let ran = cx.update(|window, cx| execute_action_or_throw("debug.startConfig", Some(&arg), window, cx));
    ran.expect("debug.startConfig runs");
    long(cx, "the running Java session", &|cx| context(cx, "debugSessionRunning"));
}

fn titled(fixture: &Fixture, title: &str, cx: &mut VisualTestContext) -> bool {
    notification_titles(fixture, cx).iter().any(|shown| shown == title)
}

// Not from a spec: Reload Changed Classes, offered while a Java session can reload classes, compiles and then
// has the session redefine its classes; the adapter is asked and the notice says what was reloaded.
#[gpui_kit::test]
async fn reload_changed_classes_compiles_then_reloads_the_classes_into_the_java_session(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, RUNNING);
    assert!(!context(cx, "debugCanHotSwap"), "no session can reload classes yet");
    java_session(&fixture, JavaBuildStatus::Succeeded, Duration::ZERO, cx);
    long(cx, "the session can reload classes", &|cx| context(cx, "debugCanHotSwap"));
    assert!(!context(cx, "debugHotSwapBuilding"));

    run(cx, "debug.reloadChangedClasses");
    long(cx, "the reload", &|cx| titled(&fixture, "1 class reloaded", cx));
    assert!(fixture.root.join("redefined").exists(), "the adapter was asked to redefine the classes");
    assert!(!context(cx, "debugHotSwapBuilding"), "the compile is over");
    assert_eq!(fixture.status(cx), Some(SessionStatus::Running), "the session keeps running");
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}

// Not from a spec: Cancel Hot Swap Build, offered while the compile of a reload runs, aborts the reload: the
// classes are not redefined and the notice says the hot swap was cancelled.
#[gpui_kit::test]
async fn cancel_hot_swap_build_aborts_the_reload_before_any_class_is_redefined(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, RUNNING);
    java_session(&fixture, JavaBuildStatus::Succeeded, Duration::from_secs(5), cx);
    long(cx, "the session can reload classes", &|cx| context(cx, "debugCanHotSwap"));

    run(cx, "debug.reloadChangedClasses");
    long(cx, "the compile", &|cx| {
        context(cx, "debugHotSwapBuilding") && titled(&fixture, "Compiling changed classes…", cx)
    });
    run(cx, "debug.cancelHotSwap");
    long(cx, "the cancelled reload", &|cx| titled(&fixture, "Hot Swap cancelled", cx));
    assert!(!context(cx, "debugHotSwapBuilding"), "the compile is over");
    assert!(!fixture.root.join("redefined").exists(), "no class was redefined");
    assert!(!titled(&fixture, "1 class reloaded", cx));
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}
