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
        "fonts" => {
            match fonts(cx) {
                Ok(report) => println!("fonts: ok\n{report}"),
                Err(error) => {
                    eprintln!("fonts: {error:#}");
                    std::process::exit(1);
                }
            }
            cx.quit();
            true
        }
        "text-width" => {
            match text_widths(cx) {
                Ok(report) => println!("text-width: ok\n{report}"),
                Err(error) => {
                    eprintln!("text-width: {error:#}");
                    std::process::exit(1);
                }
            }
            cx.quit();
            true
        }
        _ => false,
    }
}

/// The widths of the commit buttons' labels in the interface font (the bundled Inter) at 13px:
/// the sum of the glyphs' advances (kerning left out), what a layout of the Commit tool window has
/// to make room for.
fn text_widths(cx: &mut App) -> anyhow::Result<String> {
    use crate::theme::fonts::BUNDLED_UI_FAMILY;
    use gpui_kit::font;
    crate::theme::bundled_fonts::register(cx)?;
    let text_system = cx.text_system().clone();
    let id = text_system.resolve_font(&font(BUNDLED_UI_FAMILY));
    let found = text_system.get_font_for_id(id).map(|found| found.family.to_string());
    anyhow::ensure!(found.as_deref() == Some(BUNDLED_UI_FAMILY), "{BUNDLED_UI_FAMILY} resolves to {found:?}");
    let mut report = String::new();
    for label in ["Commit", "Commit and Push…", "Commit an…"] {
        let width: f32 = label
            .chars()
            .map(|ch| text_system.advance(id, px(13.0), ch).map(|size| f32::from(size.width)))
            .sum::<anyhow::Result<f32>>()?;
        report.push_str(&format!("{label:?} in {BUNDLED_UI_FAMILY} 13px: {width:.2}px\n"));
    }
    Ok(report)
}

/// The bundled faces load, resolve by the family names the theme draws with, and draw the weights
/// the interface and the editor use with faces of their own. The report says what each family
/// resolved to before the faces were registered, then for every weight and italic which family
/// and whether a face other than the regular one draws it, then the families the theme's
/// interface and code lists pick. Inter has no medium or bold face: what they fall back to is
/// reported, not failed.
fn fonts(cx: &mut App) -> anyhow::Result<String> {
    use crate::theme::fonts::{BUNDLED_MONO_FAMILY, BUNDLED_UI_FAMILY};
    use crate::theme::installed_fonts::{InstalledFontNames, mono_family, ui_family};
    use gpui_kit::{FontStyle, FontWeight, font};
    let text_system = cx.text_system().clone();
    let resolved = |family: &str, weight: FontWeight, style: FontStyle| {
        let id = text_system.resolve_font(&gpui_kit::Font { weight, style, ..font(family.to_owned()) });
        (id, text_system.get_font_for_id(id).map(|found| found.family.to_string()))
    };
    let mut report = String::new();
    for family in [BUNDLED_UI_FAMILY, BUNDLED_MONO_FAMILY] {
        let (_, before) = resolved(family, FontWeight::NORMAL, FontStyle::Normal);
        report.push_str(&format!("before registering, {family} resolves to {before:?}\n"));
    }
    crate::theme::bundled_fonts::register(cx)?;
    let weights = [
        ("400", FontWeight::NORMAL, FontStyle::Normal),
        ("500", FontWeight::MEDIUM, FontStyle::Normal),
        ("600", FontWeight::SEMIBOLD, FontStyle::Normal),
        ("700", FontWeight::BOLD, FontStyle::Normal),
        ("400 italic", FontWeight::NORMAL, FontStyle::Italic),
    ];
    let mut failures = Vec::new();
    for (family, required) in [(BUNDLED_UI_FAMILY, &["600"][..]), (BUNDLED_MONO_FAMILY, &["600", "700"][..])] {
        let (regular, found) = resolved(family, FontWeight::NORMAL, FontStyle::Normal);
        if found.as_deref() != Some(family) {
            failures.push(format!("{family} 400 resolves to {found:?}"));
        }
        let mut seen: Vec<(&str, gpui_kit::FontId)> = Vec::new();
        for (name, weight, style) in weights {
            let (id, found) = resolved(family, weight, style);
            let own = id != regular;
            // A weight without a face of its own falls back to the nearest one the family has.
            let face = match seen.iter().find(|(_, earlier)| *earlier == id) {
                Some((earlier, _)) => format!("the {earlier} face"),
                None => "a face of its own".to_owned(),
            };
            seen.push((name, id));
            let width: f32 = "Hamburgefonstiv"
                .chars()
                .map(|ch| text_system.advance(id, px(100.0), ch).map(|size| f32::from(size.width)))
                .sum::<anyhow::Result<f32>>()?;
            report.push_str(&format!("{family} {name}: {found:?}, {face} ({id:?}, width {width})\n"));
            if found.as_deref() != Some(family) {
                failures.push(format!("{family} {name} resolves to {found:?}"));
            } else if required.contains(&name) && !own {
                failures.push(format!("{family} {name} is drawn with the regular face"));
            }
        }
    }
    // What the views draw with once the installed families are known, as at start.
    cx.set_global(crate::theme::ActiveTheme::default());
    cx.set_global(InstalledFontNames(text_system.all_font_names()));
    report.push_str(&format!("--font-ui draws with {:?}, --font-mono with {:?}\n", ui_family(cx), mono_family(cx)));
    if ui_family(cx).as_ref() != BUNDLED_UI_FAMILY {
        failures.push(format!("--font-ui draws with {:?}", ui_family(cx)));
    }
    if mono_family(cx).as_ref() != BUNDLED_MONO_FAMILY {
        failures.push(format!("--font-mono draws with {:?}", mono_family(cx)));
    }
    anyhow::ensure!(failures.is_empty(), "{}\n{report}", failures.join("\n"));
    Ok(report)
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
