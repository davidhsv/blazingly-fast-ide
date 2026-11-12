use gpui_kit::App;
use openidea_gpui::theme::icons::IconAssets;
use openidea_gpui::workbench::windows::{self, Target};
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let argument = std::env::args_os().nth(1);
    let self_check = argument.as_ref().and_then(|arg| arg.to_str()?.strip_prefix("--self-check=").map(str::to_owned));
    // A folder given on the command line opens as a project, a file opens for editing.
    let file = argument.filter(|_| self_check.is_none()).map(PathBuf::from);
    gpui_kit::application().with_assets(IconAssets { fallback: gpui_kit::assets::Assets }).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        if let Some(check) = &self_check {
            if !openidea_gpui::self_check::run(check, cx) {
                eprintln!("there is no self-check called {check}");
                std::process::exit(2);
            }
            return;
        }
        if let Err(error) = openidea_gpui::init(cx) {
            tracing::error!("cannot start: {error:#}");
            cx.quit();
            return;
        }
        let target = match file.clone() {
            Some(folder) if folder.is_dir() => Target::Project(folder),
            Some(file) => Target::File(file),
            None => Target::Empty,
        };
        let opened = windows::open_window(target, cx);
        if let Err(error) = opened {
            tracing::error!("cannot open a window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
