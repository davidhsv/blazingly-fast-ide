//! The image viewer: the image drawn from its file at the zoom of [`super::image_zoom`], centred
//! when it opens (at its actual size, unless it does not fit), with the toolbar of IDEA's viewer:
//! Zoom In / Out, Actual Size, Fit Zoom to Window, the transparency chessboard, Reload from Disk,
//! the size label (`400×200 · 1.2 KB · 100%`) and, for SVG files, Open as Text.
//!
//! ⌘/Ctrl+wheel zooms about the pointer, the wheel alone and a drag with the left or middle button
//! pan. The chessboard choice is one for all viewers, kept in [`UiStorage`].
//!
//! [`ImageView`] reads the file through GPUI's image loader; the file's size on disk comes from the
//! owner ([`ImageView::set_file_size`], answering [`ImageViewEvent::Probe`]).

use super::file_kinds::{IMAGE_ACTIONS, IMAGE_KIND, format_size, is_svg_path};
use super::image_zoom::{View, clamp_zoom, fit_zoom, initial_zoom, step_zoom, zoom_at, zoom_label};
use crate::editor::tabs::EditorInput;
use crate::platform::i18n::{Localization, Param};
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::tooltip::tooltip;
use gpui_kit::{
    App, Bounds, Context, Div, EventEmitter, FocusHandle, Focusable, ImgResourceLoader, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage, Resource,
    SMOOTH_SVG_SCALE_FACTOR, ScrollDelta, ScrollEvent, SharedString, Stateful, Subscription, Window, canvas, div, fill,
    img, point, prelude::*, px, size,
};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// The storage key of the chessboard choice (`"false"` hides it).
pub const CHESSBOARD_KEY: &str = "openidea.imageViewer.chessboard";
/// Room kept around the image when it opens and when it is fitted.
const PADDING: f64 = 32.0;
/// The size an SVG without an intrinsic size is shown at.
const DEFAULT_SVG_SIZE: (f64, f64) = (300.0, 150.0);
/// The chessboard's cells, in image pixels.
const CELL: f64 = 8.0;
/// The smallest chessboard cell drawn on screen, so a zoomed-out image stays cheap to paint.
const MIN_CELL_PX: f64 = 4.0;

/// What the image viewer asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageViewEvent {
    /// Probe the file (`ide_host::ops::bytes::fs_probe`) and pass its size to
    /// [`ImageView::set_file_size`]: when the viewer opens and on Reload from Disk.
    Probe(String),
    /// Open the file in a text editor, past the viewers (`openFile(path, { asText: true })`).
    OpenAsText(String),
}

/// A toolbar button: its action id (or `image.reload`), whether it is pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub pressed: bool,
}

/// Whether the chessboard shows behind transparent images.
pub fn chessboard(cx: &App) -> bool {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(CHESSBOARD_KEY)) != Some("false")
}

/// Show Chessboard: turns the chessboard on or off in every viewer.
pub fn toggle_chessboard(cx: &mut App) {
    let on = !chessboard(cx);
    cx.default_global::<UiStorage>();
    cx.update_global::<UiStorage, _>(|storage, _| storage.set(CHESSBOARD_KEY, on.to_string()));
}

pub struct ImageView {
    path: String,
    resource: Resource,
    /// The file's size on disk, once probed.
    size: Option<u64>,
    /// The image's size in CSS pixels, once decoded.
    natural: Option<(f64, f64)>,
    view: Option<View>,
    error: Option<String>,
    /// The pointer position of a pan drag in progress.
    drag: Option<Point<Pixels>>,
    /// The canvas' bounds in the last frame, shared with its prepaint.
    canvas: Rc<Cell<Option<Bounds<Pixels>>>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ImageViewEvent> for ImageView {}

impl ImageView {
    pub fn new(path: String, cx: &mut Context<Self>) -> Self {
        let resource = Resource::Path(Arc::from(Path::new(&path)));
        let subscriptions = vec![cx.observe_global::<UiStorage>(|_, cx| cx.notify())];
        Self {
            path,
            resource,
            size: None,
            natural: None,
            view: None,
            error: None,
            drag: None,
            canvas: Rc::default(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// The viewer of a tab's `image` input ([`super::file_kinds::image_input`]); `None` for
    /// another kind of tab.
    pub fn from_input(input: &EditorInput, cx: &mut Context<Self>) -> Option<Self> {
        if input.kind != IMAGE_KIND {
            return None;
        }
        Some(Self::new(input.path.clone()?, cx))
    }

    /// Asks for the file's size: call once the owner listens.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.emit(ImageViewEvent::Probe(self.path.clone()));
    }

    /// The answer to [`ImageViewEvent::Probe`]; an error leaves the size out of the label.
    pub fn set_file_size(&mut self, answer: Result<u64, String>, cx: &mut Context<Self>) {
        match answer {
            Ok(size) => self.size = Some(size),
            Err(error) => tracing::warn!("probing {} failed: {error}", self.path),
        }
        cx.notify();
    }

    /// Runs image action `id` ([`IMAGE_ACTIONS`]) on this viewer; `false` for another id.
    pub fn run_action(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        match id {
            "image.zoomIn" => self.zoom_step(1),
            "image.zoomOut" => self.zoom_step(-1),
            "image.actualSize" => self.actual_size(),
            "image.fitToWindow" => self.fit(),
            "image.toggleChessboard" => toggle_chessboard(cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Reload from Disk: the image is decoded again and the file probed again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        cx.remove_asset::<ImgResourceLoader>(&self.resource);
        self.error = None;
        self.natural = None;
        cx.emit(ImageViewEvent::Probe(self.path.clone()));
        cx.notify();
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The pan and zoom, once the image is shown.
    pub fn view(&self) -> Option<View> {
        self.view
    }

    /// The image's size in pixels, once decoded.
    pub fn natural_size(&self) -> Option<(f64, f64)> {
        self.natural
    }

    /// The label: dimensions, file size and zoom (`400×200 · 1.2 KB · 100%`).
    pub fn info(&self) -> String {
        let mut parts = Vec::new();
        if let Some((w, h)) = self.natural {
            parts.push(format!("{w}×{h}"));
        }
        if let Some(size) = self.size {
            parts.push(format_size(size));
        }
        if let Some(view) = self.view {
            parts.push(zoom_label(view.z));
        }
        parts.join(" · ")
    }

    /// The message shown instead of the image when it cannot be read or decoded.
    pub fn error_message(&self, cx: &mut App) -> Option<String> {
        let error = self.error.as_deref()?;
        Some(cx.default_global::<Localization>().0.tx_built(&["Cannot show the image: ", ""], &[Param::from(error)]))
    }

    /// The toolbar's buttons, left to right.
    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarButton> {
        let chessboard = chessboard(cx);
        IMAGE_ACTIONS
            .into_iter()
            .map(|id| ToolbarButton { id, pressed: id == "image.toggleChessboard" && chessboard })
            .chain(std::iter::once(ToolbarButton { id: "image.reload", pressed: false }))
            .collect()
    }

    /// Whether Open as Text shows: for SVG files.
    pub fn offers_text(&self) -> bool {
        is_svg_path(&self.path)
    }

    // ---------------------------------------------------------------------------------------------
    // Zoom and pan

    fn canvas_size(&self) -> Option<(f64, f64)> {
        let bounds = self.canvas.get()?;
        Some((f64::from(f32::from(bounds.size.width)), f64::from(f32::from(bounds.size.height))))
    }

    /// The view showing the image at zoom `z`, centred in the canvas.
    fn centered(&self, z: f64) -> Option<View> {
        let (cw, ch) = self.canvas_size()?;
        let (w, h) = self.natural?;
        Some(View { z, x: (cw - w * z) / 2.0, y: (ch - h * z) / 2.0 })
    }

    /// Once both the image and the canvas are measured, the image opens centred at its actual
    /// size, unless it does not fit.
    fn place(&mut self) {
        if self.view.is_some() {
            return;
        }
        let (Some((cw, ch)), Some((w, h))) = (self.canvas_size(), self.natural) else { return };
        self.view = self.centered(initial_zoom(w, h, cw - PADDING, ch - PADDING));
    }

    fn zoom_step(&mut self, dir: i8) {
        let (Some(view), Some((cw, ch))) = (self.view, self.canvas_size()) else { return };
        self.view = Some(zoom_at(view, step_zoom(view.z, dir), cw / 2.0, ch / 2.0));
    }

    fn actual_size(&mut self) {
        if let Some(view) = self.centered(1.0) {
            self.view = Some(view);
        }
    }

    fn fit(&mut self) {
        let (Some((cw, ch)), Some((w, h))) = (self.canvas_size(), self.natural) else { return };
        if let Some(view) = self.centered(fit_zoom(w, h, cw - PADDING, ch - PADDING)) {
            self.view = Some(view);
        }
    }

    /// The canvas' size changed (measured after a frame).
    fn measured(&mut self, cx: &mut Context<Self>) {
        self.place();
        cx.notify();
    }

    fn wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (Some(view), Some(bounds)) = (self.view, self.canvas.get()) else { return };
        let modifiers = &event.modifiers;
        if modifiers.platform || modifiers.control {
            let k = match event.delta {
                ScrollDelta::Pixels(delta) => f64::from(f32::from(delta.y)) * 0.01,
                ScrollDelta::Lines(delta) => f64::from(delta.y) * 0.33,
            };
            let at = event.position - bounds.origin;
            let (ax, ay) = (f64::from(f32::from(at.x)), f64::from(f32::from(at.y)));
            self.view = Some(zoom_at(view, clamp_zoom(view.z * k.exp()), ax, ay));
        } else {
            let delta = event.delta.pixel_delta(px(16.0));
            let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
            self.view = Some(View { x: view.x + dx, y: view.y + dy, ..view });
        }
        cx.notify();
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(from), Some(view)) = (self.drag, self.view) else { return };
        let delta = position - from;
        self.drag = Some(position);
        let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        self.view = Some(View { x: view.x + dx, y: view.y + dy, ..view });
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    /// The decoded image, once loaded; records its size, or the error.
    fn image(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        match window.use_asset::<ImgResourceLoader>(&self.resource, cx)? {
            Ok(image) => {
                if self.natural.is_none() {
                    let size = image.size(0);
                    let scale = if is_svg_path(&self.path) { f64::from(SMOOTH_SVG_SCALE_FACTOR) } else { 1.0 };
                    let (w, h) = (f64::from(size.width.0) / scale, f64::from(size.height.0) / scale);
                    self.natural = Some(if w > 0.0 && h > 0.0 { (w.round(), h.round()) } else { DEFAULT_SVG_SIZE });
                    self.place();
                }
                Some(image)
            }
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    fn tool_button(&self, button: &ToolbarButton, icon: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = button.id;
        let selector = match id {
            "image.zoomIn" => "image-zoom-in",
            "image.zoomOut" => "image-zoom-out",
            "image.actualSize" => "image-actual-size",
            "image.fitToWindow" => "image-fit",
            "image.toggleChessboard" => "image-chessboard",
            _ => "image-reload",
        };
        let color = theme::color(cx, if button.pressed { "fg-strong" } else { "fg-muted" });
        let title = match id {
            "image.zoomIn" => "Zoom In",
            "image.zoomOut" => "Zoom Out",
            "image.actualSize" => "Actual Size",
            "image.fitToWindow" => "Fit Zoom to Window",
            "image.toggleChessboard" => "Show Chessboard",
            _ => "Reload from Disk",
        };
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-toolbar-button"))
            .when(button.pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .tooltip(tooltip(title))
            .on_click(cx.listener(move |view, _, _, cx| {
                if id == "image.reload" {
                    view.reload(cx);
                } else {
                    view.run_action(id, cx);
                }
            }))
            .child(theme::icon(cx, icon, 14.0, color))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = self.toolbar(cx);
        let icons = ["zoom-in", "zoom-out", "actual-size", "maximize", "chessboard", "refresh"];
        let mut row = div()
            .flex_none()
            .h(px(36.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-editor-tabs"))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-secondary"));
        for (index, (button, icon)) in buttons.iter().zip(icons).enumerate() {
            if index == 4 {
                row = row.child(div().w(px(1.0)).h(px(16.0)).mx(px(6.0)).bg(theme::color(cx, "border-strong")));
            }
            row = row.child(self.tool_button(button, icon, cx));
        }
        let open_as_text = cx.default_global::<Localization>().0.tx("Open as Text");
        let path = self.path.clone();
        row.child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "image-info".to_owned())
                    .px(px(6.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(SharedString::from(self.info())),
            )
            .when(self.offers_text(), |el| {
                el.child(
                    div()
                        .id("image-open-as-text")
                        .debug_selector(|| "image-open-as-text".to_owned())
                        .h(px(24.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(theme::length(cx, "radius-toolbar-button"))
                        .text_color(theme::color(cx, "link"))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(ImageViewEvent::OpenAsText(path.clone()))))
                        .child(SharedString::from(open_as_text)),
                )
            })
    }

    /// The transparency chessboard under the image at `bounds`, as cells of [`CELL`] image pixels
    /// scaled with the zoom; only the visible cells are painted.
    fn chessboard_layer(z: f64, cx: &App) -> impl IntoElement {
        let (light, dark) = (theme::color(cx, "bg-elev"), theme::color(cx, "bg-hover"));
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                window.paint_quad(fill(bounds, light));
                let visible = bounds.intersect(&window.content_mask().bounds);
                let cell = (CELL * z).max(MIN_CELL_PX);
                let origin = bounds.origin;
                let first_col = (f64::from(f32::from(visible.left() - origin.x)) / cell).floor().max(0.0) as i64;
                let first_row = (f64::from(f32::from(visible.top() - origin.y)) / cell).floor().max(0.0) as i64;
                let last_col = (f64::from(f32::from(visible.right() - origin.x)) / cell).ceil() as i64;
                let last_row = (f64::from(f32::from(visible.bottom() - origin.y)) / cell).ceil() as i64;
                for row in first_row..last_row {
                    for col in first_col..last_col {
                        if (row + col) % 2 == 1 {
                            let (left, top) = ((col as f64 * cell) as f32, (row as f64 * cell) as f32);
                            let at = point(origin.x + px(left), origin.y + px(top));
                            let square = Bounds::new(at, size(px(cell as f32), px(cell as f32)));
                            window.paint_quad(fill(square.intersect(&bounds), dark));
                        }
                    }
                }
            },
        )
        .absolute()
        .size_full()
    }
}

impl Focusable for ImageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ImageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = if self.error.is_none() { self.image(window, cx) } else { None };
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg"))
            .child(self.render_toolbar(cx));
        if let Some(message) = self.error_message(cx) {
            return root.child(
                div()
                    .debug_selector(|| "image-error".to_owned())
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(message)),
            );
        }
        let measured = Rc::clone(&self.canvas);
        let weak = cx.weak_entity();
        let measure = canvas(
            move |bounds, _, cx| {
                if measured.get() != Some(bounds) {
                    measured.set(Some(bounds));
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |view, cx| view.measured(cx));
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let shown = image.zip(self.view).zip(self.natural).map(|((image, view), (w, h))| {
            let (width, height) = ((w * view.z) as f32, (h * view.z) as f32);
            div()
                .debug_selector(|| "image-shown".to_owned())
                .absolute()
                .left(px(view.x as f32))
                .top(px(view.y as f32))
                .w(px(width))
                .h(px(height))
                .child(
                    div()
                        .absolute()
                        .left(px(-1.0))
                        .top(px(-1.0))
                        .w(px(width + 2.0))
                        .h(px(height + 2.0))
                        .border_1()
                        .border_color(theme::color(cx, "border-strong")),
                )
                .when(chessboard(cx), |el| el.child(Self::chessboard_layer(view.z, cx)))
                .child(img(image).absolute().size_full())
        });
        root.child(
            div()
                .id("image-canvas")
                .debug_selector(|| "image-canvas".to_owned())
                .flex_1()
                .min_h_0()
                .relative()
                .overflow_hidden()
                .map(|el| if self.drag.is_some() { el.cursor_grabbing() } else { el.cursor_grab() })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, event: &MouseDownEvent, window, cx| {
                        window.focus(&view.focus, cx);
                        view.drag = Some(event.position);
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(|view, event: &MouseDownEvent, _, cx| {
                        view.drag = Some(event.position);
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                    if view.drag.is_none() {
                        return;
                    }
                    if event.pressed_button.is_none() {
                        view.drag = None;
                        cx.notify();
                        return;
                    }
                    view.drag_to(event.position, cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseUpEvent, _, cx| {
                        view.drag = None;
                        cx.notify();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(|view, _: &MouseUpEvent, _, cx| {
                        view.drag = None;
                        cx.notify();
                    }),
                )
                .on_scroll(cx.listener(|view, event: &ScrollEvent, _, cx| view.wheel(event, cx)))
                .child(measure)
                .children(shown),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<ImageViewEvent>>>;

    /// A 400×200 SVG on disk, kept as long as the returned directory.
    fn svg_file() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("picture.svg");
        std::fs::write(
            &path,
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">"#,
                r#"<rect width="400" height="200" fill="red"/></svg>"#,
            ),
        )
        .expect("the SVG is written");
        (dir, path.to_string_lossy().into_owned())
    }

    fn open(cx: &mut TestAppContext, path: String) -> (Entity<ImageView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
        });
        let (view, cx) = cx.add_window_view(|_, cx| ImageView::new(path, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ImageViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.load(cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn view_of(view: &Entity<ImageView>, cx: &mut VisualTestContext) -> View {
        view.read_with(cx, |view, _| view.view()).expect("the image is shown")
    }

    fn close(a: View, b: View) -> bool {
        (a.z - b.z).abs() < 1e-6 && (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[gpui_kit::test]
    async fn opens_an_image_at_actual_size_centred_with_its_size_label(cx: &mut TestAppContext) {
        let (_dir, path) = svg_file();
        let (view, events, cx) = open(cx, path.clone());
        assert_eq!(*events.borrow(), [ImageViewEvent::Probe(path)]);
        view.update(cx, |view, cx| view.set_file_size(Ok(1234), cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.natural_size()), Some((400.0, 200.0)));
        assert_eq!(view.read_with(cx, |view, _| view.info()), "400×200 · 1.2 KB · 100%");
        let canvas = cx.debug_bounds("image-canvas").expect("the canvas is drawn");
        let (cw, ch) = (f64::from(f32::from(canvas.size.width)), f64::from(f32::from(canvas.size.height)));
        assert!(close(view_of(&view, cx), View { z: 1.0, x: (cw - 400.0) / 2.0, y: (ch - 200.0) / 2.0 }));
        assert!(cx.debug_bounds("image-shown").is_some());
    }

    #[gpui_kit::test]
    async fn steps_the_zoom_from_the_toolbar_and_fits_the_window(cx: &mut TestAppContext) {
        let (_dir, path) = svg_file();
        let (view, _, cx) = open(cx, path);
        click(cx, "image-zoom-in");
        assert_eq!(view_of(&view, cx).z, 1.25);
        assert!(view.read_with(cx, |view, _| view.info()).ends_with("125%"));
        click(cx, "image-zoom-out");
        click(cx, "image-zoom-out");
        assert_eq!(view_of(&view, cx).z, 0.8);
        click(cx, "image-actual-size");
        assert_eq!(view_of(&view, cx).z, 1.0);
        click(cx, "image-fit");
        let canvas = cx.debug_bounds("image-canvas").expect("the canvas is drawn");
        let (cw, ch) = (f64::from(f32::from(canvas.size.width)), f64::from(f32::from(canvas.size.height)));
        assert_eq!(view_of(&view, cx).z, fit_zoom(400.0, 200.0, cw - PADDING, ch - PADDING));
    }

    #[gpui_kit::test]
    async fn zooms_about_the_pointer_with_the_primary_modifier_and_pans_otherwise(cx: &mut TestAppContext) {
        let (_dir, path) = svg_file();
        let (view, _, cx) = open(cx, path);
        let canvas = cx.debug_bounds("image-canvas").expect("the canvas is drawn");
        let at = canvas.origin + point(px(100.0), px(80.0));
        let before = view_of(&view, cx);
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.0), px(50.0))),
            modifiers: Modifiers { control: true, ..Modifiers::default() },
            ..Default::default()
        });
        cx.run_until_parked();
        let zoomed = zoom_at(before, clamp_zoom(before.z * 0.5f64.exp()), 100.0, 80.0);
        assert!(close(view_of(&view, cx), zoomed));

        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(10.0), px(20.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        assert!(close(view_of(&view, cx), View { x: zoomed.x + 10.0, y: zoomed.y + 20.0, ..zoomed }));

        let start = view_of(&view, cx);
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(at + point(px(30.0), px(40.0)), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(at + point(px(30.0), px(40.0)), MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert!(close(view_of(&view, cx), View { x: start.x + 30.0, y: start.y + 40.0, ..start }));
    }

    #[gpui_kit::test]
    async fn turns_the_chessboard_off_for_every_viewer(cx: &mut TestAppContext) {
        let (_dir, path) = svg_file();
        let (view, _, cx) = open(cx, path);
        let pressed = |view: &Entity<ImageView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| view.toolbar(cx)).iter().any(|button| button.pressed)
        };
        assert!(pressed(&view, cx));
        click(cx, "image-chessboard");
        assert!(!pressed(&view, cx));
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(CHESSBOARD_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some("false"));
        view.update(cx, |view, cx| view.run_action("image.toggleChessboard", cx));
        assert!(pressed(&view, cx));
    }

    #[gpui_kit::test]
    async fn offers_an_svg_as_text_and_reloads_it_from_disk(cx: &mut TestAppContext) {
        let (_dir, path) = svg_file();
        let (view, events, cx) = open(cx, path.clone());
        assert!(view.read_with(cx, |view, _| view.offers_text()));
        click(cx, "image-open-as-text");
        assert_eq!(events.borrow().last(), Some(&ImageViewEvent::OpenAsText(path.clone())));
        click(cx, "image-reload");
        assert_eq!(events.borrow().last(), Some(&ImageViewEvent::Probe(path)));
        assert_eq!(view.read_with(cx, |view, _| view.natural_size()), Some((400.0, 200.0)));
    }

    #[gpui_kit::test]
    async fn says_why_an_image_cannot_be_shown(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("missing.png").to_string_lossy().into_owned();
        let (view, _, cx) = open(cx, path);
        let message = view.update(cx, |view, cx| view.error_message(cx)).expect("an error is shown");
        assert!(message.starts_with("Cannot show the image: "), "{message}");
        assert!(cx.debug_bounds("image-error").is_some());
        assert!(!view.read_with(cx, |view, _| view.offers_text()));
    }
}
