use super::*;

/// Set in the test process [`in_a_process_with_interrupts_ignored`] starts.
const IGNORED_RUN: &str = "OPENIDEA_TEST_SIGINT_IGNORED";

/// Whether this is the run of the test `name` in a test process started with SIGINT ignored, as a script's
/// `cmd &` starts one. Outside it, it runs that test again in such a process and fails unless that run passes:
/// an ignored signal cannot be set up from inside a process without `unsafe`, but `trap '' INT` before `exec` gives
/// the new process exactly that state.
fn in_a_process_with_interrupts_ignored(name: &str) -> bool {
    if std::env::var_os(IGNORED_RUN).is_some() {
        return true;
    }
    let test = format!("{}::{name}", module_path!().split_once("::").map_or(module_path!(), |(_, path)| path));
    let binary = std::env::current_exe().expect("the test binary");
    let run = std::process::Command::new("/bin/sh")
        .args(["-c", "trap '' INT; exec \"$0\" --exact \"$1\" --nocapture --test-threads=1"])
        .arg(&binary)
        .arg(&test)
        .env(IGNORED_RUN, "1")
        .output()
        .expect("the test binary starts");
    let output = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "{test} failed with SIGINT ignored:\n{output}");
    assert!(output.contains("1 passed"), "{test} did not run:\n{output}");
    false
}

/// Whether a program this process starts now is ended by SIGINT: a shell started with it ignored outlives
/// interrupting itself.
fn started_programs_stop_on_interrupt() -> bool {
    let survived = std::process::Command::new("/bin/sh")
        .args(["-c", "kill -INT $$; echo survived"])
        .output()
        .expect("the shell starts");
    !String::from_utf8_lossy(&survived.stdout).contains("survived")
}

/// In an IDE started with SIGINT ignored, a shell script run from a configuration is stopped by Run › Stop: the
/// IDE's own interrupt handler gives the programs it starts the default disposition back.
#[gpui_kit::test]
async fn stop_ends_a_shell_script_run_by_an_ide_started_with_interrupts_ignored(cx: &mut TestAppContext) {
    if !in_a_process_with_interrupts_ignored("stop_ends_a_shell_script_run_by_an_ide_started_with_interrupts_ignored") {
        return;
    }
    assert!(!started_programs_stop_on_interrupt(), "the test process starts with SIGINT ignored");
    let script = tempfile::tempdir().expect("a temporary folder");
    let path = script.path().join("serve.sh");
    std::fs::write(&path, "echo serving\nsleep 60\necho done\n").expect("the script is written");
    let (_workbench, cx, _folders, _root, store) =
        run_project(cx, vec![program("serve", "/bin/sh", path.to_str().expect("a UTF-8 path"))]);
    cx.update(|_, cx| quit_on_interrupt(cx)).expect("the handler is installed");
    assert!(started_programs_stop_on_interrupt(), "programs started now get the default disposition");

    let serve = |cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "serve").cloned())
    };
    run_action(cx, "run.run", None);
    wait_long(cx, "the script to print", |cx| {
        store.read_with(cx, |store, _| {
            store.sessions.iter().find(|s| s.config.name == "serve" && s.is_live()).is_some_and(|session| {
                store.console(&session.id).is_some_and(|console| console.text().contains("serving"))
            })
        })
    });
    run_action(cx, "run.stop", None);
    wait_long(cx, "the script to end", |cx| serve(cx).is_some_and(|s| !s.is_live()));
    let ended = serve(cx).expect("the run's tab");
    assert!(ended.stopped, "the run was stopped");
    assert_ne!(ended.exit_code, Some(0), "the script did not run to its end");
}

/// SIGINT to the IDE quits it as File › Exit does: the exit is confirmed, then the unsaved document is asked
/// about, and Cancel keeps the window and its text.
#[gpui_kit::test]
async fn an_interrupt_asks_to_exit_and_about_unsaved_changes_like_quit(cx: &mut TestAppContext) {
    if !in_a_process_with_interrupts_ignored("an_interrupt_asks_to_exit_and_about_unsaved_changes_like_quit") {
        return;
    }
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    cx.update(|_, cx| quit_on_interrupt(cx)).expect("the handler is installed");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.update(|window, cx| {
        let editor = workbench.read(cx).editor().cloned().expect("an editor");
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_input("my ");

    let sent = std::process::Command::new("/bin/kill")
        .args(["-INT", &std::process::id().to_string()])
        .status()
        .expect("kill starts");
    assert!(sent.success(), "the signal is sent");
    wait_long(cx, "the exit confirmation", |cx| app_dialog_title(&workbench, cx).as_deref() == Some("Confirm Exit"));
    click_on(cx, "prompt-button-exit");
    let (title, _) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(cx.windows().len(), 1, "the window stays");
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("my notes\n"));
    assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
