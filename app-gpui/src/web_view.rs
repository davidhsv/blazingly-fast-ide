//! An HTML surface inside a GPUI window: the OS web view as a child view of the window.
//!
//! Most of the app is drawn natively. What is HTML by nature (a plugin's web panel, a Markdown
//! or notebook preview, an embedded browser) is shown in one of these, created when the surface
//! is first shown and dropped with it, so a window that shows none never starts a web process.
//!
//! The page talks to the app with `window.ipc.postMessage(text)`; the app talks to the page by
//! evaluating script in it.

use anyhow::Context as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::{Bounds, Pixels, Window, point, px, size};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

/// What a surface shows first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebContent {
    Html(String),
    Url(String),
}

/// A web view placed over part of a window.
pub struct WebSurface {
    view: WebView,
    scale: f64,
}

fn rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(f64::from(f32::from(bounds.origin.x)), f64::from(f32::from(bounds.origin.y)))
            .into(),
        size: LogicalSize::new(f64::from(f32::from(bounds.size.width)), f64::from(f32::from(bounds.size.height)))
            .into(),
    }
}

impl WebSurface {
    /// Puts a web view showing `content` over `bounds` of `window` (window coordinates, from the
    /// top left). The receiver yields what the page posts, in order; it ends when the surface is
    /// dropped.
    pub fn new(
        window: &Window,
        content: WebContent,
        bounds: Bounds<Pixels>,
    ) -> anyhow::Result<(Self, UnboundedReceiver<String>)> {
        let (sender, messages) = unbounded();
        let builder = WebViewBuilder::new().with_bounds(rect(bounds)).with_ipc_handler(move |request| {
            // The receiver is gone only when nobody listens to this surface any more.
            let _ = sender.unbounded_send(request.into_body());
        });
        let builder = match content {
            WebContent::Html(html) => builder.with_html(html),
            WebContent::Url(url) => builder.with_url(url),
        };
        let view = builder.build_as_child(window).context("cannot create the web view")?;
        Ok((Self { view, scale: f64::from(window.scale_factor()) }, messages))
    }

    /// Moves and resizes the surface within its window.
    pub fn set_bounds(&self, bounds: Bounds<Pixels>) -> anyhow::Result<()> {
        self.view.set_bounds(rect(bounds)).context("cannot place the web view")
    }

    /// Where the surface is within its window.
    pub fn bounds(&self) -> anyhow::Result<Bounds<Pixels>> {
        let rect = self.view.bounds().context("cannot read the web view's bounds")?;
        let (position, extent) = (rect.position.to_logical::<f64>(self.scale), rect.size.to_logical::<f64>(self.scale));
        Ok(Bounds {
            origin: point(px(position.x as f32), px(position.y as f32)),
            size: size(px(extent.width as f32), px(extent.height as f32)),
        })
    }

    /// Runs `script` in the page.
    pub fn eval(&self, script: &str) -> anyhow::Result<()> {
        self.view.evaluate_script(script).context("cannot run script in the web view")
    }

    /// Shows or hides the surface without unloading its page.
    pub fn set_visible(&self, visible: bool) -> anyhow::Result<()> {
        self.view.set_visible(visible).context("cannot show or hide the web view")
    }
}
