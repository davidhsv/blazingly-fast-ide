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
use gpui_kit::{ClipboardEntry, Context, ExternalPaths, Window};
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

    /// Files dropped on the terminal: their quoted paths are typed at the prompt, without Enter.
    pub(super) fn files_dropped(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = paths.paths().iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if let Some(text) = terminal_drop_text(&paths, this_os(), false, true) {
            self.paste_text(&text, cx);
        }
    }
}
