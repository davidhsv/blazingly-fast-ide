//! Checks of what only a real window can show, run by `openidea-gpui --self-check=<name>`.
//!
//! GPUI's test windows have no native view behind them, so anything that embeds an OS view is
//! exercised here, in the real application, and driven from an integration test that starts the
//! binary. A check prints `<name>: ok` and quits, or prints what went wrong and exits with 1.

use crate::web_view::{WebContent, WebSurface};
use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt as _, StreamExt as _};
use gpui_kit::{
    App, AppContext as _, AsyncApp, Bounds, Context, IntoElement, Pixels, Render, Window, WindowOptions, div, point,
    prelude::*, px, size,
};
use std::time::Duration;

/// How long a check waits for one answer from the page.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs the check called `name`. False when there is no such check.
pub fn run(name: &str, cx: &mut App) -> bool {
    match name {
        "web-view" => {
            cx.spawn(async move |cx| {
                match web_view(cx).await {
                    Ok(()) => println!("web-view: ok"),
                    Err(error) => {
                        eprintln!("web-view: {error:#}");
                        std::process::exit(1);
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
            true
        }
        _ => false,
    }
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

async fn answer(messages: &mut UnboundedReceiver<String>, cx: &AsyncApp) -> anyhow::Result<String> {
    let mut timeout = cx.background_executor().timer(ANSWER_TIMEOUT).fuse();
    futures::select! {
        message = messages.next() => message.ok_or_else(|| anyhow::anyhow!("the page went away")),
        () = timeout => anyhow::bail!("the page did not answer within {ANSWER_TIMEOUT:?}"),
    }
}

fn area(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) }
}

/// A web view is a child of a GPUI window, its page reaches the app, the app reaches its page,
/// and it takes the bounds it is given.
async fn web_view(cx: &mut AsyncApp) -> anyhow::Result<()> {
    const PAGE: &str = r#"<!doctype html><html><body><script>
        window.report = () => window.ipc.postMessage(`size:${window.innerWidth}x${window.innerHeight}`);
        window.ipc.postMessage("ready");
    </script></body></html>"#;
    let window = cx.update(|cx| cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Blank)))?;
    let first = area(10.0, 40.0, 320.0, 200.0);
    let (surface, mut messages) =
        window.update(cx, |_, window, _| WebSurface::new(window, WebContent::Html(PAGE.to_owned()), first))??;

    // Page to app.
    let ready = answer(&mut messages, cx).await?;
    anyhow::ensure!(ready == "ready", "expected the page to say ready, it said {ready:?}");
    anyhow::ensure!(surface.bounds()? == first, "the surface is at {:?}, not {first:?}", surface.bounds()?);

    // App to page and back.
    surface.eval("window.ipc.postMessage('echo:' + (6 * 7))")?;
    let echo = answer(&mut messages, cx).await?;
    anyhow::ensure!(echo == "echo:42", "expected echo:42, the page said {echo:?}");

    // The page is laid out in the bounds the surface is given. Layout follows a moment after
    // the view is resized, so the page is asked until it reports the new size.
    let second = area(0.0, 0.0, 400.0, 300.0);
    surface.set_bounds(second)?;
    anyhow::ensure!(surface.bounds()? == second, "the surface is at {:?}, not {second:?}", surface.bounds()?);
    let mut reported = String::new();
    for _ in 0..50 {
        surface.eval("window.report()")?;
        reported = answer(&mut messages, cx).await?;
        if reported == "size:400x300" {
            return Ok(());
        }
        cx.background_executor().timer(Duration::from_millis(100)).await;
    }
    anyhow::bail!("the page never reported 400x300 after the resize; last it said {reported:?}")
}
