//! What ⌘V pastes into a terminal running the plain test shell: copied files as quoted paths, a
//! copied picture as the path of the file it is stored in, nothing for a session that has changed
//! meanwhile, and a notification when the paste cannot be done.

use super::*;
use crate::terminal::paste::{MAX_MEDIA_BYTES, TerminalPasteTarget};
use crate::workbench::notifications::NotificationRequest;
use gpui_kit::{ClipboardEntry, ExternalPaths, Image, ImageFormat};
use std::cell::RefCell;
use std::rc::Rc;

/// The failed pastes a terminal reported, in order.
type Failures = Rc<RefCell<Vec<NotificationRequest>>>;

/// A terminal running the plain test shell, focused, with its shell started and its failed pastes
/// logged; the home and the folder live as long as the returned guards.
fn pasting_shell(
    cx: &mut TestAppContext,
) -> (Entity<TerminalView>, &mut VisualTestContext, Failures, [tempfile::TempDir; 2]) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        crate::terminal::test_support::isolate_shells(home.path(), cx);
        crate::terminal::test_support::trusted_folder("w1", folder.path(), cx);
    });
    let shell = crate::terminal::test_support::plain_shell(home.path());
    let options = TerminalOptions {
        cwd: ide_core::IpcPath(folder.path().to_path_buf()),
        shell: Some(shell.to_string_lossy().into_owned()),
        args: Vec::new(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
        shell_integration: false,
    };
    let (view, cx) = cx.add_window_view(|_, cx| TerminalView::new("w1", options, cx));
    let failures: Failures = Rc::default();
    let log = failures.clone();
    cx.update(|window, cx| {
        window.activate_window();
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        cx.subscribe(&view, move |_, event: &TerminalEvent, _| {
            if let TerminalEvent::PasteFailed(notification) = event {
                log.borrow_mut().push(notification.clone());
            }
        })
        .detach();
    });
    settle(cx, "the shell runs and the grid is fitted", |cx| {
        view.read_with(cx, |view, _| view.id.is_some() && view.grid.is_some())
    });
    (view, cx, failures, [home, folder])
}

/// The screen with its rows run together, so that a line the grid wrapped reads as one.
fn joined(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.emulator.lines().concat())
}

/// Types `printf '<%s>\n' ` so that the words pasted after it come back one per line between angle
/// brackets once Enter runs it, which tells what reached the shell from what the terminal echoes.
fn print_words(cx: &mut VisualTestContext) {
    cx.simulate_input("printf '<%s>\\n' ");
}

fn clipboard(entries: Vec<ClipboardEntry>, cx: &mut VisualTestContext) {
    cx.write_to_clipboard(ClipboardItem { entries });
}

// src/features/terminal/paste.test.ts › keeps existing copied files on their original quoted paths during %s paste
/// Copied files paste as their paths, quoted for the shell and separated by spaces, at the prompt:
/// they run only once Enter is pressed.
#[gpui_kit::test]
async fn keeps_existing_copied_files_on_their_original_quoted_paths_during_paste(cx: &mut TestAppContext) {
    let (view, cx, failures, _dirs) = pasting_shell(cx);
    let copied = ["/a b/photo.png", "/it's here/clip.mp4"];
    let paths = copied.iter().map(std::path::PathBuf::from).collect::<Vec<_>>();
    clipboard(vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths.into()))], cx);
    print_words(cx);
    cx.simulate_keystrokes("cmd-v");
    settle(cx, "the paths are typed", |cx| joined(&view, cx).contains(r"'/a b/photo.png' '/it'\''s here/clip.mp4'"));
    assert!(!joined(&view, cx).contains("</a b/photo.png>"), "nothing runs before Enter");
    cx.simulate_keystrokes("enter");
    settle(cx, "the command runs", |cx| {
        let shown = joined(&view, cx);
        shown.contains("</a b/photo.png>") && shown.contains("</it's here/clip.mp4>")
    });
    assert!(failures.borrow().is_empty());
}

// src/features/terminal/paste.test.ts › stores raw local clipboard media through %s paste
/// A copied picture with no text is stored in a file of the backend's data folder, and the file's
/// path is pasted, quoted, once it is there.
#[gpui_kit::test]
async fn stores_raw_local_clipboard_media_through_paste(cx: &mut TestAppContext) {
    let (view, cx, failures, dirs) = pasting_shell(cx);
    let picture = Image::from_bytes(ImageFormat::Png, vec![0x89, b'P', b'N', b'G', 1, 2, 3]);
    clipboard(vec![ClipboardEntry::Image(picture)], cx);
    print_words(cx);
    cx.simulate_keystrokes("cmd-v");
    settle(cx, "the picture is stored and its path typed", |cx| {
        view.read_with(cx, |view, _| view.storing == 0) && joined(&view, cx).contains(".png'")
    });
    cx.simulate_keystrokes("enter");
    settle(cx, "the command runs", |cx| joined(&view, cx).contains(".png>"));
    let shown = joined(&view, cx);
    let end = shown.rfind(".png>").expect("the path is printed") + ".png".len();
    let start = shown[..end].rfind('<').expect("the path's start") + 1;
    let stored = std::path::PathBuf::from(&shown[start..end]);
    assert!(stored.to_string_lossy().contains("terminal-images"), "{stored:?}");
    assert_eq!(std::fs::read(&stored).expect("the file is there"), [0x89, b'P', b'N', b'G', 1, 2, 3]);
    let home = dirs[0].path().canonicalize().expect("the home exists");
    let stored = stored.canonicalize().expect("the file exists");
    assert!(stored.starts_with(&home), "{stored:?} is in the test's home");
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

// src/features/terminal/paste.test.ts › ignores an image after its target is replaced during %s
// src/features/terminal/paste.test.ts › does not paste after the event handler is disposed during upload
/// Media stored for a session the terminal no longer runs pastes nothing and reports nothing; for the
/// session it runs, the path is pasted, and a failure to store it is reported.
#[gpui_kit::test]
async fn ignores_an_image_after_its_target_is_replaced_during_upload(cx: &mut TestAppContext) {
    let (view, cx, failures, _dirs) = pasting_shell(cx);
    let id = view.read_with(cx, |view, _| view.id).expect("the shell runs");
    let target = TerminalPasteTarget { id, supported: true, remote: false, shell: "sh".to_owned() };
    let replaced = TerminalPasteTarget { id: ide_core::TerminalId::next(), ..target.clone() };
    print_words(cx);
    view.update(cx, |view, cx| {
        view.media_stored(&replaced, Ok("/stored/stale.png".to_owned()), cx);
        view.media_stored(&replaced, Err(ide_core::IdeError::internal("upload failed")), cx);
    });
    cx.simulate_keystrokes("enter");
    settle(cx, "the command runs with no word", |cx| joined(&view, cx).contains("<>"));
    assert!(!joined(&view, cx).contains("stale.png"));
    assert!(failures.borrow().is_empty(), "a replaced session is told nothing");

    print_words(cx);
    view.update(cx, |view, cx| view.media_stored(&target, Ok("/stored/a b.png".to_owned()), cx));
    cx.simulate_keystrokes("enter");
    settle(cx, "the stored path runs", |cx| joined(&view, cx).contains("</stored/a b.png>"));
    view.update(cx, |view, cx| view.media_stored(&target, Err(ide_core::IdeError::internal("upload failed")), cx));
    let reported: Vec<(String, Option<String>)> =
        failures.borrow().iter().map(|failure| (failure.title.clone(), failure.body.clone())).collect();
    assert_eq!(reported, [("Cannot paste clipboard media".to_owned(), Some("upload failed".to_owned()))]);
}

// src/features/terminal/paste.test.ts › rejects oversized %s images before reading or uploading
/// A copied picture over 20 MB is not stored: the paste says so and types nothing.
#[gpui_kit::test]
async fn rejects_oversized_images_before_reading_or_uploading(cx: &mut TestAppContext) {
    let (view, cx, failures, _dirs) = pasting_shell(cx);
    let oversized = usize::try_from(MAX_MEDIA_BYTES).expect("it fits") + 1;
    clipboard(vec![ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, vec![0; oversized]))], cx);
    print_words(cx);
    cx.simulate_keystrokes("cmd-v");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.storing), 0, "nothing is stored");
    let reported: Vec<Option<String>> = failures.borrow().iter().map(|failure| failure.body.clone()).collect();
    assert_eq!(reported, [Some("Clipboard media is too large to paste (limit 20 MB).".to_owned())]);
    cx.simulate_keystrokes("enter");
    settle(cx, "the command runs with no word", |cx| joined(&view, cx).contains("<>"));
}
