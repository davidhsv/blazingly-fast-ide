//! What pasting the clipboard into a terminal types at the prompt: its text as it is, copied
//! files as their paths, and media (a screenshot, a recording) as the path of a file it is
//! stored in first, so that a command-line program can read it.
//!
//! The functions decide; the view reads the clipboard, carries a [`PastePlan`] out and shows a
//! [`PasteError`]. Storing media takes time: before it pastes the stored path
//! ([`stored_media_text`]) or reports a failure, the view checks that the terminal the paste was
//! meant for is still the one shown.

use crate::platform::i18n::{I18n, Param};
use crate::platform::paths::ipc_path;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::TerminalId;

/// The largest media a paste stores, in megabytes.
const MAX_MEDIA_MB: u32 = 20;
pub const MAX_MEDIA_BYTES: u64 = MAX_MEDIA_MB as u64 * 1024 * 1024;

/// The terminal a paste goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPasteTarget {
    /// The session that stores pasted media.
    pub id: TerminalId,
    /// Its host can store pasted media.
    pub supported: bool,
    /// Its shell runs on a remote host, where the paths of local files mean nothing.
    pub remote: bool,
    /// The shell's name (`zsh`, `pwsh`, `cmd.exe`): paths are quoted for it.
    pub shell: String,
}

/// Media on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardMedia {
    /// Its MIME type (`image/png`).
    pub mime: String,
    /// Its size in bytes.
    pub size: u64,
}

/// Lists the files on the clipboard; `None` when they cannot be listed. Asked only when a paste
/// needs them.
pub type FileList<'a> = &'a dyn Fn() -> Option<Vec<String>>;

/// What a paste does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PastePlan {
    /// The clipboard has nothing a terminal can take.
    Nothing,
    /// Paste this text.
    Text(String),
    /// Store the clipboard's media of this MIME type in the target's session, then paste
    /// [`stored_media_text`] of the path it was stored at.
    StoreMedia(String),
}

impl PastePlan {
    fn text(text: String) -> Self {
        if text.is_empty() { Self::Nothing } else { Self::Text(text) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteError {
    /// A path has characters `cmd.exe` expands and no quoting protects.
    UnsafeCmdPath,
    MediaTooLarge,
    /// The remote host cannot store pasted media.
    RemoteUnsupported,
    /// The clipboard has no text or files, and its media cannot be read on this platform.
    MediaUnavailable,
    /// Reading the clipboard or storing its media failed, with the reason.
    Failed(String),
}

impl PasteError {
    /// The error in the UI language.
    pub fn message(&self, i18n: &mut I18n) -> String {
        match self {
            Self::UnsafeCmdPath => {
                i18n.tx("Cannot paste this path safely in cmd.exe. Use PowerShell or a POSIX shell.")
            }
            Self::MediaTooLarge => i18n.tx_built(
                &["Clipboard media is too large to paste (limit ", " MB)."],
                &[Param::Number(f64::from(MAX_MEDIA_MB))],
            ),
            Self::RemoteUnsupported => i18n.tx("Remote server does not support image paste."),
            Self::MediaUnavailable => i18n.tx("Clipboard image reading is unavailable."),
            Self::Failed(reason) => reason.clone(),
        }
    }

    /// The notification that reports the error.
    pub fn notification(&self, i18n: &mut I18n) -> NotificationRequest {
        let title = i18n.tx("Cannot paste clipboard media");
        let body = Some(self.message(i18n));
        NotificationRequest { body, ..NotificationRequest::new(NotificationLevel::Error, title) }
    }
}

/// `paths` as words of a command line of `shell`, each quoted so the shell reads it literally.
pub fn paths_text<S: AsRef<str>>(paths: &[S], shell: &str) -> Result<String, PasteError> {
    let shell = shell.to_lowercase();
    let shell = shell.strip_suffix(".exe").unwrap_or(&shell);
    let quoted = |path: &str| match shell {
        "cmd" if path.contains(['%', '!', '"', '\r', '\n']) => Err(PasteError::UnsafeCmdPath),
        "cmd" => Ok(format!("\"{path}\"")),
        "powershell" | "pwsh" => Ok(format!("'{}'", path.replace('\'', "''"))),
        "fish" => Ok(format!("'{}'", path.replace('\\', r"\\").replace('\'', r"\'"))),
        _ => Ok(format!("'{}'", path.replace('\'', r"'\''"))),
    };
    let words = paths.iter().map(|path| quoted(path.as_ref())).collect::<Result<Vec<_>, _>>()?;
    Ok(words.join(" "))
}

/// The text that pastes the file pasted media was stored at.
pub fn stored_media_text(path: &str, target: &TerminalPasteTarget) -> Result<String, PasteError> {
    paths_text(&[path], &target.shell)
}

/// Images, sound and video: what is stored in a file rather than pasted as text.
pub fn is_media(mime: &str) -> bool {
    ["image/", "audio/", "video/"].iter().any(|kind| mime.starts_with(kind))
}

/// `text` with its `%XX` escapes decoded; `None` when an escape is malformed or the result is
/// not UTF-8.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = text.get(at + 1..at + 3).filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn is_drive(text: &str) -> bool {
    matches!(text.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic())
}

/// The path a `file:` URL names: `//host/share/…` for another host, `C:/…` for a Windows drive.
fn file_uri_path(uri: &str) -> Option<String> {
    let uri = uri.trim();
    let rest = uri.get(..5).filter(|scheme| scheme.eq_ignore_ascii_case("file:")).map(|_| &uri[5..])?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest).replace('\\', "/");
    let (host, path) = match rest.strip_prefix("//") {
        Some(authority) => authority.split_at(authority.find('/').unwrap_or(authority.len())),
        None => ("", rest.as_str()),
    };
    // A drive where the host goes (`file://C:/dir`) is the start of the path.
    let (host, path) = if is_drive(host) { ("", format!("/{host}{path}")) } else { (host, path.to_owned()) };
    let path = percent_decoded(&path)?;
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return Some(format!("//{}{path}", host.to_lowercase()));
    }
    match path.as_bytes() {
        [b'/', letter, b':', b'/', ..] if letter.is_ascii_alphabetic() => Some(path[1..].to_owned()),
        _ => Some(path),
    }
}

/// The local files a `text/uri-list` names. Comments, other schemes and malformed lines are
/// skipped.
pub fn file_paths(uri_list: &str) -> Vec<String> {
    uri_list.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).filter_map(file_uri_path).collect()
}

/// How copied files are written at the prompt.
#[derive(Debug, Clone, Copy)]
enum FileFormat<'a> {
    /// One path per line, as they are.
    Raw,
    /// Words quoted for this shell.
    Quoted(&'a str),
}

impl FileFormat<'_> {
    fn text(self, paths: &[String]) -> Result<String, PasteError> {
        match self {
            Self::Raw => Ok(paths.join("\n")),
            Self::Quoted(shell) => paths_text(paths, shell),
        }
    }
}

/// The text of a clipboard whose plain text is `text`: that text, unless the clipboard holds
/// files, which are then written in `format`. Some platforms hide the text flavour of copied
/// files, others supply the file list as text too; unrelated text still wins over the files.
fn clipboard_text(text: &str, may_have_files: bool, format: FileFormat, files: FileList) -> Result<String, PasteError> {
    if (!text.is_empty() && matches!(format, FileFormat::Raw)) || !may_have_files {
        return Ok(text.to_owned());
    }
    let Some(paths) = files() else { return Ok(text.to_owned()) };
    let as_listed = || text.replace("\r\n", "\n").split('\n').map(ipc_path).collect::<Vec<_>>().join("\n");
    if !text.is_empty() && as_listed() != paths.join("\n") {
        return Ok(text.to_owned());
    }
    format.text(&paths)
}

/// Media goes to a file in the target's session, when it is small enough and the host can store it.
fn media_plan(media: &ClipboardMedia, target: &TerminalPasteTarget) -> Result<PastePlan, PasteError> {
    if media.size > MAX_MEDIA_BYTES {
        return Err(PasteError::MediaTooLarge);
    }
    if !target.supported {
        return Err(PasteError::RemoteUnsupported);
    }
    Ok(PastePlan::StoreMedia(media.mime.clone()))
}

/// The clipboard as a paste gesture (the paste shortcut, a middle click) delivers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PastedClipboard<'a> {
    /// Its plain text; empty when it has none.
    pub text: &'a str,
    /// It holds files: copied in a file manager, or media that travels as a file.
    pub has_files: bool,
    /// Its `text/uri-list` flavour, when it has one.
    pub uri_list: Option<&'a str>,
    /// The first of its files that is media.
    pub media: Option<&'a ClipboardMedia>,
}

/// What a paste gesture pastes into `target`; without a target (a terminal with no session
/// behind it) only text and the raw paths of files.
///
/// A local shell gets copied files as quoted paths, and media that is no file yet stored in one.
/// A remote shell cannot read local files: it gets text when there is any, else media uploaded
/// to its host, else the raw paths.
pub fn paste_event_plan(
    target: Option<&TerminalPasteTarget>,
    clipboard: &PastedClipboard,
    files: FileList,
) -> Result<PastePlan, PasteError> {
    let media = clipboard.media.filter(|media| is_media(&media.mime));
    if let Some(target) = target {
        let has_files = clipboard.has_files || clipboard.uri_list.is_some();
        if (!target.remote && has_files) || (media.is_some() && clipboard.text.is_empty()) {
            if !target.remote {
                let listed = file_paths(clipboard.uri_list.unwrap_or_default());
                let text = if listed.is_empty() {
                    clipboard_text(clipboard.text, clipboard.has_files, FileFormat::Quoted(&target.shell), files)?
                } else {
                    paths_text(&listed, &target.shell)?
                };
                if !text.is_empty() {
                    return Ok(PastePlan::Text(text));
                }
            }
            return media.map_or(Ok(PastePlan::Nothing), |media| media_plan(media, target));
        }
    }
    clipboard_text(clipboard.text, clipboard.has_files, FileFormat::Raw, files).map(PastePlan::text)
}

/// What reading the clipboard's media gave an explicit Paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRead<'a> {
    /// The platform cannot read clipboard media.
    Unavailable,
    /// Reading failed, with the reason.
    Failed(&'a str),
    /// The first media on the clipboard, if it has any.
    Read(Option<&'a ClipboardMedia>),
}

/// What an explicit Paste (the context menu, the Paste action) pastes into `target`: the
/// clipboard's text or copied files first, then its media, then whatever files it lists. `text`:
/// the clipboard's plain text, `None` when reading it was refused, which pastes as no text.
pub fn paste_menu_plan(
    target: Option<&TerminalPasteTarget>,
    text: Option<&str>,
    media: MediaRead,
    files: FileList,
) -> Result<PastePlan, PasteError> {
    let text = text.unwrap_or_default();
    let Some(target) = target else {
        return clipboard_text(text, true, FileFormat::Raw, files).map(PastePlan::text);
    };
    let format = if target.remote { FileFormat::Raw } else { FileFormat::Quoted(&target.shell) };
    let pasted = if target.remote { text.to_owned() } else { clipboard_text(text, true, format, files)? };
    if !pasted.is_empty() {
        return Ok(PastePlan::Text(pasted));
    }
    match media {
        MediaRead::Failed(reason) => return Err(PasteError::Failed(reason.to_owned())),
        MediaRead::Read(Some(media)) if is_media(&media.mime) => return media_plan(media, target),
        MediaRead::Read(_) | MediaRead::Unavailable => {}
    }
    let paths = clipboard_text("", true, format, files)?;
    if paths.is_empty() && media == MediaRead::Unavailable {
        return Err(PasteError::MediaUnavailable);
    }
    Ok(PastePlan::text(paths))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn remote_target() -> TerminalPasteTarget {
        TerminalPasteTarget { id: TerminalId(42), supported: true, remote: true, shell: "sh".to_owned() }
    }

    fn local_target(shell: &str) -> TerminalPasteTarget {
        TerminalPasteTarget { remote: false, shell: shell.to_owned(), ..remote_target() }
    }

    fn image(size: u64) -> ClipboardMedia {
        ClipboardMedia { mime: "image/png".to_owned(), size }
    }

    /// A clipboard holding files, with this plain text.
    fn files_event(text: &str) -> PastedClipboard<'_> {
        PastedClipboard { text, has_files: true, uri_list: None, media: None }
    }

    /// A clipboard holding `media` as a file, with this plain text.
    fn media_event<'a>(media: &'a ClipboardMedia, text: &'a str) -> PastedClipboard<'a> {
        PastedClipboard { media: Some(media), ..files_event(text) }
    }

    fn paths(list: &[&str]) -> Option<Vec<String>> {
        Some(list.iter().map(|path| (*path).to_owned()).collect())
    }

    fn no_files() -> Option<Vec<String>> {
        Some(Vec::new())
    }

    fn text(plan: &str) -> Result<PastePlan, PasteError> {
        Ok(PastePlan::Text(plan.to_owned()))
    }

    fn store(mime: &str) -> Result<PastePlan, PasteError> {
        Ok(PastePlan::StoreMedia(mime.to_owned()))
    }

    fn notified(error: &PasteError) -> (String, Option<String>) {
        let notification = error.notification(&mut I18n::default());
        assert_eq!(notification.level, NotificationLevel::Error);
        (notification.title, notification.body)
    }

    // src/features/terminal/paste.test.ts › event: $name
    // src/features/terminal/paste.test.ts › menu: $name
    #[test]
    fn pastes_text_or_else_the_raw_paths_of_copied_files() {
        let cases = [
            ("plain", paths(&["/stale"]), "plain", 0),
            ("", paths(&["/a b", "/中文"]), "/a b\n/中文", 1),
            ("", no_files(), "", 1),
            ("", None, "", 1),
        ];
        for (clipboard, listed, expected, calls) in cases {
            for menu in [false, true] {
                let asked = Cell::new(0);
                let files = || {
                    asked.set(asked.get() + 1);
                    listed.clone()
                };
                let plan = if menu {
                    paste_menu_plan(None, Some(clipboard), MediaRead::Unavailable, &files)
                } else {
                    paste_event_plan(None, &files_event(clipboard), &files)
                };
                assert_eq!(plan, Ok(PastePlan::text(expected.to_owned())));
                assert_eq!(asked.get(), calls);
            }
        }
    }

    // src/features/terminal/paste.test.ts › rejects oversized %s images before reading or uploading
    #[test]
    fn rejects_oversized_images_before_reading_or_uploading() {
        let oversized = image(MAX_MEDIA_BYTES + 1);
        let target = remote_target();
        let event = paste_event_plan(Some(&target), &media_event(&oversized, ""), &no_files);
        let menu = paste_menu_plan(Some(&target), Some(""), MediaRead::Read(Some(&oversized)), &no_files);
        for plan in [event, menu] {
            assert_eq!(plan, Err(PasteError::MediaTooLarge));
        }
        let (title, body) = notified(&PasteError::MediaTooLarge);
        assert_eq!(title, "Cannot paste clipboard media");
        assert_eq!(body.as_deref(), Some("Clipboard media is too large to paste (limit 20 MB)."));
    }

    // src/features/terminal/paste.test.ts › allows images at the size limit
    #[test]
    fn allows_images_at_the_size_limit() {
        let target = remote_target();
        let largest = image(20 * 1024 * 1024);
        assert_eq!(paste_event_plan(Some(&target), &media_event(&largest, ""), &no_files), store("image/png"));
        assert_eq!(stored_media_text("/remote/image.png", &target).as_deref(), Ok("'/remote/image.png'"));
    }

    // src/features/terminal/paste.test.ts › claims remote image events before xterm and pastes a quoted path only
    // after upload
    #[test]
    fn stores_a_remote_image_and_pastes_its_quoted_path_only_after_the_upload() {
        let target = remote_target();
        assert_eq!(paste_event_plan(Some(&target), &media_event(&image(2), ""), &no_files), store("image/png"));
        assert_eq!(stored_media_text("/tmp/a'b c.png", &target).as_deref(), Ok(r"'/tmp/a'\''b c.png'"));
    }

    // src/features/terminal/paste.test.ts › reads an image from the explicit paste menu after text and file recovery
    #[test]
    fn reads_an_image_from_the_explicit_paste_after_text_and_file_recovery() {
        let stale = || paths(&["/local/stale.png"]);
        let plan = paste_menu_plan(Some(&remote_target()), Some(""), MediaRead::Read(Some(&image(2))), &stale);
        assert_eq!(plan, store("image/png"));
    }

    // src/features/terminal/paste.test.ts › recovers remote menu file paths when image clipboard reading is
    // unavailable
    #[test]
    fn recovers_remote_file_paths_when_clipboard_media_cannot_be_read() {
        let listed = || paths(&["/local/file.txt"]);
        let plan = paste_menu_plan(Some(&remote_target()), Some(""), MediaRead::Unavailable, &listed);
        assert_eq!(plan, text("/local/file.txt"));
        let nothing = paste_menu_plan(Some(&remote_target()), Some(""), MediaRead::Unavailable, &no_files);
        assert_eq!(nothing, Err(PasteError::MediaUnavailable));
    }

    // src/features/terminal/paste.test.ts › uploads a menu image even if reading clipboard text is rejected
    #[test]
    fn uploads_an_image_even_if_reading_the_clipboard_text_is_rejected() {
        let plan = paste_menu_plan(Some(&remote_target()), None, MediaRead::Read(Some(&image(2))), &no_files);
        assert_eq!(plan, store("image/png"));
    }

    // src/features/terminal/paste.test.ts › uploads image events instead of recovering a local image path
    #[test]
    fn uploads_image_events_instead_of_recovering_a_local_image_path() {
        let asked = Cell::new(false);
        let files = || {
            asked.set(true);
            paths(&["/local/photo.png"])
        };
        let plan = paste_event_plan(Some(&remote_target()), &media_event(&image(2), ""), &files);
        assert_eq!(plan, store("image/png"));
        assert!(!asked.get());
        assert_eq!(stored_media_text("/remote/photo.png", &remote_target()).as_deref(), Ok("'/remote/photo.png'"));
    }

    // src/features/terminal/paste.test.ts › reports image %s failure without pasting
    #[test]
    fn reports_an_image_failure_without_pasting() {
        let unsupported = TerminalPasteTarget { supported: false, ..remote_target() };
        let plan = paste_event_plan(Some(&unsupported), &media_event(&image(2), ""), &no_files);
        assert_eq!(plan, Err(PasteError::RemoteUnsupported));
        let (_, body) = notified(&PasteError::RemoteUnsupported);
        assert_eq!(body.as_deref(), Some("Remote server does not support image paste."));
        // Reading the media and storing it fail in the view, which reports the reason.
        for reason in ["read failed", "upload failed"] {
            let (title, body) = notified(&PasteError::Failed(reason.to_owned()));
            assert_eq!((title.as_str(), body.as_deref()), ("Cannot paste clipboard media", Some(reason)));
        }
        let failed = paste_menu_plan(Some(&remote_target()), Some(""), MediaRead::Failed("read failed"), &no_files);
        assert_eq!(failed, Err(PasteError::Failed("read failed".to_owned())));
    }

    // src/features/terminal/paste.test.ts › keeps local image-only file recovery unchanged
    #[test]
    fn keeps_local_image_only_file_recovery_unchanged() {
        let listed = || paths(&["/local/photo.png"]);
        assert_eq!(paste_event_plan(None, &media_event(&image(2), ""), &listed), text("/local/photo.png"));
    }

    // src/features/terminal/paste.test.ts › leaves local image and remote text events to existing paste handlers
    #[test]
    fn pastes_the_text_of_local_image_and_remote_text_events() {
        let picture = image(2);
        let event = media_event(&picture, "plain");
        assert_eq!(paste_event_plan(Some(&remote_target()), &event, &no_files), text("plain"));
        assert_eq!(paste_event_plan(None, &event, &no_files), text("plain"));
    }

    // src/features/terminal/paste.test.ts › stores raw local clipboard media through %s paste
    #[test]
    fn stores_raw_local_clipboard_media() {
        let target = local_target("sh");
        let picture = image(2);
        let event = paste_event_plan(Some(&target), &media_event(&picture, ""), &no_files);
        let menu = paste_menu_plan(Some(&target), Some(""), MediaRead::Read(Some(&picture)), &no_files);
        for plan in [event, menu] {
            assert_eq!(plan, store("image/png"));
        }
        assert_eq!(stored_media_text("/tmp/a'b c.png", &target).as_deref(), Ok(r"'/tmp/a'\''b c.png'"));
    }

    // src/features/terminal/paste.test.ts › keeps existing copied files on their original quoted paths during %s
    // paste
    #[test]
    fn keeps_existing_copied_files_on_their_original_quoted_paths() {
        let copied = ["/a b/photo.png", "/it's here/clip.mp4"];
        let listed = || paths(&copied);
        let target = local_target("sh");
        let picture = image(2);
        let event = paste_event_plan(Some(&target), &media_event(&picture, &copied.join("\n")), &listed);
        let as_text = copied.join("\r\n");
        let menu = paste_menu_plan(Some(&target), Some(&as_text), MediaRead::Read(Some(&picture)), &listed);
        for plan in [event, menu] {
            assert_eq!(plan, text(r"'/a b/photo.png' '/it'\''s here/clip.mp4'"));
        }
    }

    // src/features/terminal/paste.test.ts › reads file URI lists without relying on a native file-list backend
    #[test]
    fn reads_file_uri_lists_without_asking_for_the_file_list() {
        let asked = Cell::new(false);
        let files = || {
            asked.set(true);
            no_files()
        };
        let uri_list = "# files\r\nfile:///tmp/a%20b.png\r\nhttps://example.com\r\nfile:///tmp/%E4%B8%AD%E6%96%87.mp4";
        let clipboard = PastedClipboard { text: "", has_files: false, uri_list: Some(uri_list), media: None };
        let plan = paste_event_plan(Some(&local_target("sh")), &clipboard, &files);
        assert_eq!(plan, text("'/tmp/a b.png' '/tmp/中文.mp4'"));
        assert!(!asked.get());
    }

    // src/features/terminal/paste.test.ts › quotes a stored path for $shell
    #[test]
    fn quotes_a_stored_path_for_the_shell() {
        let cases = [
            ("pwsh", r"C:\Users\a'b $c\image.png", r"'C:\Users\a''b $c\image.png'"),
            ("powershell", r"C:\a b\image.png", r"'C:\a b\image.png'"),
            ("zsh", "/tmp/a'b $中文.png", r"'/tmp/a'\''b $中文.png'"),
            ("bash", "/tmp/a'b $中文.png", r"'/tmp/a'\''b $中文.png'"),
            ("fish", "/tmp/a'b $中文.png", r"'/tmp/a\'b $中文.png'"),
            ("fish", r"/tmp/a\\b\'中文.png", r"'/tmp/a\\\\b\\\'中文.png'"),
            ("cmd", r"C:\a&b\image.png", r#""C:\a&b\image.png""#),
            ("cmd.exe", r"C:\a b $中文\image.png", r#""C:\a b $中文\image.png""#),
        ];
        for (shell, path, expected) in cases {
            assert_eq!(stored_media_text(path, &local_target(shell)).as_deref(), Ok(expected), "{shell}");
        }
    }

    // src/features/terminal/paste.test.ts › stores raw %s clipboard data
    #[test]
    fn stores_raw_audio_and_video_clipboard_data() {
        let target = local_target("sh");
        for mime in ["audio/wav", "video/mp4"] {
            let media = ClipboardMedia { mime: mime.to_owned(), size: 2 };
            assert_eq!(paste_event_plan(Some(&target), &media_event(&media, ""), &no_files), store(mime));
        }
        assert_eq!(stored_media_text("/tmp/media", &target).as_deref(), Ok("'/tmp/media'"));
    }

    // src/features/terminal/paste.test.ts › rejects cmd expansion in copied file paths through %s paste
    #[test]
    fn rejects_cmd_expansion_in_copied_file_paths() {
        let listed = || paths(&[r"C:\clips\%OS%!clip!.png"]);
        let target = local_target("cmd");
        let event = paste_event_plan(Some(&target), &media_event(&image(2), ""), &listed);
        let menu = paste_menu_plan(Some(&target), Some(""), MediaRead::Unavailable, &listed);
        for plan in [event, menu] {
            assert_eq!(plan, Err(PasteError::UnsafeCmdPath));
        }
        let (title, body) = notified(&PasteError::UnsafeCmdPath);
        assert_eq!(title, "Cannot paste clipboard media");
        let expected = "Cannot paste this path safely in cmd.exe. Use PowerShell or a POSIX shell.";
        assert_eq!(body.as_deref(), Some(expected));
    }

    // src/features/terminal/paste.test.ts › recovers quoted local file paths when clipboard text permission is
    // denied
    #[test]
    fn recovers_quoted_local_file_paths_when_reading_the_clipboard_text_is_denied() {
        let listed = || paths(&[r"C:\a b\image.png"]);
        let plan = paste_menu_plan(Some(&local_target("pwsh")), None, MediaRead::Unavailable, &listed);
        assert_eq!(plan, text(r"'C:\a b\image.png'"));
    }

    // Not from a spec: other hosts, Windows drives and malformed escapes in a file URI list.
    #[test]
    fn reads_the_paths_file_uris_name() {
        let lines = [
            "file://server/share/a.txt",
            "file://localhost/tmp/b.txt",
            "file:///C:/dir/c.txt",
            "file:///tmp/%zz",
            "mailto:x",
        ];
        assert_eq!(file_paths(&lines.join("\n")), ["//server/share/a.txt", "/tmp/b.txt", "C:/dir/c.txt"]);
        let odd = "FILE:///tmp/d.txt?x#y\r\nfile://C:/dir/e.txt\nfile:///tmp/%ff";
        assert_eq!(file_paths(odd), ["/tmp/d.txt", "C:/dir/e.txt"]);
    }
}
