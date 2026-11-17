use super::proofs30_settings_run_targets::open_run_targets;
use super::*;
use crate::plugins::Plugins;
use crate::workbench::plugins_window::test_support::{
    link, plugin_copy, program, programs_in, start_with_plugins, trusted_project,
};
use ide_plugin::Invocation;

/// A run target a plugin registered for the project is listed, read-only and named after its plugin, on the
/// Run Targets page of Settings. The target's program is a stand-in in the test's home, the only folder the
/// plugin's programs are looked up in.
#[cfg(unix)]
#[gpui_kit::test]
async fn run_targets_lists_the_target_a_plugin_registered_for_the_project(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let bin = home.path().join("bin");
    program(&bin, "execution-tool", "#!/bin/sh\nexit 0\n");
    programs_in(&bin, cx);
    let folder = plugin_copy(home.path(), "execution", str::to_owned);
    let id = link(cx, &folder, &["process:execution-tool"], "run");
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    let invocation = Invocation {
        command_chain: Vec::new(),
        client: Some("w1".to_owned()),
        workspace_root: Some(root.clone()),
        editor: None,
        selected_paths: Vec::new(),
        global_settings: false,
    };
    let registering = std::thread::spawn(move || host.run_command(&id, "target", None, invocation));
    registering.join().expect("the command ran").expect("the target is registered");

    let (_dialog, view) = open_run_targets(&workbench, cx);
    let listed = "  Execution target | Plugin execution tests · Read-only";
    wait_long(cx, "the plugin's target is listed", |cx| {
        view.read_with(cx, |view, cx| view.lines(cx)).iter().any(|line| line == listed)
    });
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
