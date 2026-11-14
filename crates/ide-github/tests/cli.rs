//! `GhCli` runs the program it is given with gh's arguments, and without gh every call answers as
//! on a machine that has none. No real gh runs.
#![cfg(unix)]

use ide_github::{GhApiRequest, GhAvailability, GhCli};
use std::path::PathBuf;

/// A folder of this test in the temporary directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ide-github-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the folder is made");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn get_user() -> GhApiRequest {
    GhApiRequest { method: "GET".into(), endpoint: "user".into(), body: None, paginate: false }
}

#[test]
fn a_given_program_answers_with_the_arguments_gh_would_get() {
    let scratch = Scratch::new("cli");
    let script = scratch.0.join("gh");
    std::fs::write(&script, "printf '%s|' \"$@\"\n").expect("the stand-in is written");
    let cli = GhCli::at("/bin/sh").arg(script.to_string_lossy());
    assert_eq!(cli.api(&scratch.0, &get_user()).expect("it answers"), "api|--method|GET|user|");
}

#[test]
fn a_failing_program_is_classified_like_gh() {
    let scratch = Scratch::new("login");
    let script = scratch.0.join("gh");
    std::fs::write(&script, "echo 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 1\n")
        .expect("the stand-in is written");
    let cli = GhCli::at("/bin/sh").arg(script.to_string_lossy());
    assert!(matches!(cli.availability(&scratch.0), GhAvailability::NotLoggedIn { .. }));
}

#[test]
fn without_gh_every_call_says_it_is_not_installed() {
    let scratch = Scratch::new("none");
    let cli = GhCli::not_installed();
    assert_eq!(cli.availability(&scratch.0), GhAvailability::NotInstalled);
    let error = cli.api(&scratch.0, &get_user()).expect_err("nothing runs");
    assert!(error.message.contains("gh executable not found"), "{}", error.message);
}
