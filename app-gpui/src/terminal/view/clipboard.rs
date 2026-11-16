//! What a terminal takes from the clipboard and from files dropped on it.
//!
//! A paste types the clipboard's text, the quoted paths of copied files, or the path of the file a
//! copied picture or recording is stored in first; a drop types the quoted paths of the files. Neither
//! presses Enter. Storing media takes a moment: what it pastes goes to the terminal only while that
//! terminal still runs the session the paste was meant for.

use super::{TerminalEvent, TerminalView};
use crate::backend::Backend;
use crate::platform::i18n::Localization;
use crate::platform::misc::platform_info::Os;
use crate::terminal::drop::terminal_drop_text;
use crate::terminal::encode::encode_paste;
use crate::terminal::paste::{
    ClipboardMedia, PasteError, PastePlan, PastedClipboard, TerminalPasteTarget, paste_event_plan, stored_media_text,
};
use gpui_kit::{
    ClipboardEntry, Context, DispatchPhase, DragMoveEvent, ExternalPaths, HitboxBehavior, IntoElement, MouseButton,
    MouseDownEvent, MouseUpEvent, Styled, Window, canvas,
};
use ide_core::IdeResult;

/// What a clipboard holds, as a paste reads it.
#[derive(Debug, Default)]
struct ClipboardContents {
    text: String,
    files: Vec<String>,
    /// The first picture, with its MIME type and bytes.
    media: Option<(String, Vec<u8>)>,
}

impl ClipboardContents {
    fn read(entries: &[ClipboardEntry]) -> Self {
        let mut contents = Self::default();
        for entry in entries {
            match entry {
                ClipboardEntry::String(text) => contents.text.push_str(text.text()),
                ClipboardEntry::ExternalPaths(paths) => {
                    contents.files.extend(paths.paths().iter().map(|path| path.to_string_lossy().into_owned()));
                }
                ClipboardEntry::Image(image) if contents.media.is_none() => {
                    contents.media = Some((media_type(image.format.mime_type()).to_owned(), image.bytes.clone()));
                }
                ClipboardEntry::Image(_) => {}
            }
        }
        contents
    }
}

/// The MIME type pasted media is stored under: icons travel as `image/ico`, which the backend
/// knows as `image/x-icon`.
fn media_type(mime: &str) -> &str {
    if mime == "image/ico" { "image/x-icon" } else { mime }
}

/// The OS the app runs on.
fn this_os() -> Os {
    Os::from_platform(std::env::consts::OS)
}

impl TerminalView {
    /// The session a paste is meant for: this terminal's, while its shell runs. Pasted media is
    /// stored on this machine's backend, where the shell runs too.
    fn paste_target(&self) -> Option<TerminalPasteTarget> {
        let id = self.id?;
        Some(TerminalPasteTarget { id, supported: true, remote: false, shell: self.spawned_title.clone() })
    }

    /// Types `text` as a paste: line breaks as Enter would send them, and inside bracketed paste
    /// marks when the program asked for them, so that it does not run line by line.
    pub(super) fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        let pasted = encode_paste(text, self.emulator.bracketed_paste());
        self.write(&pasted, cx);
    }

    /// Pastes the clipboard as the paste shortcut does. False when the clipboard holds nothing the
    /// terminal can take.
    pub(super) fn paste_clipboard(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        let contents = ClipboardContents::read(item.entries());
        let target = self.paste_target();
        let media = contents.media.as_ref().map(|(mime, bytes)| ClipboardMedia {
            mime: mime.clone(),
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        });
        let clipboard = PastedClipboard {
            text: &contents.text,
            has_files: !contents.files.is_empty(),
            uri_list: None,
            media: media.as_ref(),
        };
        let files = || Some(contents.files.clone());
        match paste_event_plan(target.as_ref(), &clipboard, &files) {
            Ok(PastePlan::Nothing) => false,
            Ok(PastePlan::Text(text)) => {
                self.paste_text(&text, cx);
                true
            }
            Ok(PastePlan::StoreMedia(mime)) => {
                let (Some(target), Some((_, bytes))) = (target, contents.media) else { return false };
                self.store_media(target, bytes, mime, cx);
                true
            }
            Err(error) => {
                self.paste_failed(&error, cx);
                true
            }
        }
    }

    /// Stores pasted media in the target's session on the backend, off the main thread, then pastes
    /// the path it was stored at.
    fn store_media(&mut self, target: TerminalPasteTarget, bytes: Vec<u8>, mime: String, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let id = target.id;
        let call = backend
            .with_host(move |host| async move { ide_host::ops::term::store_image(&host, id, bytes, mime).await });
        self.storing += 1;
        cx.spawn(async move |view, cx| {
            let stored = match call.await {
                Ok(stored) => stored,
                Err(error) => Err(ide_core::IdeError::internal(error.to_string())),
            };
            // A terminal closed meanwhile takes nothing.
            let _ = view.update(cx, |view: &mut TerminalView, cx| {
                view.storing = view.storing.saturating_sub(1);
                view.media_stored(&target, stored, cx);
            });
        })
        .detach();
    }

    /// Pastes the path pasted media was stored at, or says why it could not be stored, as long as
    /// this terminal still runs the session of `target`; otherwise nothing.
    pub(super) fn media_stored(
        &mut self,
        target: &TerminalPasteTarget,
        stored: IdeResult<String>,
        cx: &mut Context<Self>,
    ) {
        if self.id != Some(target.id) {
            return;
        }
        match stored
            .map_err(|error| PasteError::Failed(error.message))
            .and_then(|path| stored_media_text(&path, target))
        {
            Ok(text) => self.paste_text(&text, cx),
            Err(error) => self.paste_failed(&error, cx),
        }
    }

    /// Tells the terminal's owner why a paste failed, in the UI language.
    fn paste_failed(&mut self, error: &PasteError, cx: &mut Context<Self>) {
        let notification = error.notification(&mut cx.default_global::<Localization>().0);
        cx.emit(TerminalEvent::PasteFailed(notification));
    }

    /// Files from the operating system dragged over the window: the ones a release over this terminal
    /// drops on it, while the pointer is over it.
    pub(super) fn files_dragged(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dragged = event.bounds.contains(&event.event.position).then(|| event.drag(cx).clone());
    }

    /// Where files are dropped: the terminal as drawn, when nothing is drawn over it at the pointer.
    ///
    /// The drop is taken from the release itself rather than from `on_drop`: files dragged from the
    /// operating system arrive as file-drop events, which leave the window's last input as it was, so
    /// after typing at the prompt the window reports no element under the pointer and `on_drop` never
    /// fires. Any press or release forgets the files, so that they are never dropped by a later drag
    /// of something else.
    pub(super) fn drop_target(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                let pressed = view.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        pressed.update(cx, |view, _| view.dragged = None);
                    }
                });
                let released = view.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    let over = cx.has_active_drag() && hitbox.is_hovered_at(event.position, window);
                    if released.update(cx, |view, cx| view.files_dropped(over, cx)) {
                        cx.stop_active_drag(window);
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// The release of a drag: when `over` this terminal with files from the operating system, their
    /// quoted paths are typed at the prompt, without Enter. Answers whether files were dropped.
    fn files_dropped(&mut self, over: bool, cx: &mut Context<Self>) -> bool {
        let Some(paths) = self.dragged.take().filter(|_| over) else { return false };
        let paths: Vec<String> = paths.paths().iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if let Some(text) = terminal_drop_text(&paths, this_os(), false, true) {
            self.paste_text(&text, cx);
        }
        true
    }
}
