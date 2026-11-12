//! The OS web view inside a real GPUI window, checked by running the application itself.

use std::process::Command;

/// `--self-check=web-view`: a web view is created as a child of a GPUI window, its page posts to
/// the app, the app runs script in the page, and the page is laid out in the bounds it is given.
#[test]
fn a_web_view_lives_in_a_gpui_window_and_talks_both_ways() {
    let home = tempfile::tempdir().expect("a temporary home");
    let output = Command::new(env!("CARGO_BIN_EXE_openidea-gpui"))
        .arg("--self-check=web-view")
        .env("OPENIDEA_GPUI_HOME", home.path())
        .output()
        .expect("the application starts");
    let (stdout, stderr) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "the check failed: {stderr}\n{stdout}");
    assert!(stdout.contains("web-view: ok"), "no result line in: {stdout}\n{stderr}");
}
