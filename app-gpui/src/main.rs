use gpui_kit::{App, AppContext as _, WindowOptions};
use openidea_gpui::theme::icons::IconAssets;
use openidea_gpui::workbench::Workbench;
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    // A folder given on the command line opens as a project, a file opens for editing.
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    gpui_kit::application().with_assets(IconAssets { fallback: gpui_kit::assets::Assets }).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        if let Err(error) = openidea_gpui::init(cx) {
            tracing::error!("cannot start: {error:#}");
            cx.quit();
            return;
        }
        let opened = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut workbench = Workbench::new("main", window, cx);
                match file.clone() {
                    Some(folder) if folder.is_dir() => workbench.open_project(folder, window, cx),
                    Some(file) => workbench.open_file(file, window, cx),
                    None => {}
                }
                workbench
            })
        });
        if let Err(error) = opened {
            tracing::error!("cannot open a window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
