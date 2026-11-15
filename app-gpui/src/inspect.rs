//! A script of user steps the debug build carries out after its first window is shown, so the real
//! window can be driven and looked at without any operating-system input.
//!
//! `main` reads the script named by `OPENIDEA_GPUI_INSPECT` and hands it to [`start`] with the log
//! file named by `OPENIDEA_GPUI_INSPECT_LOG`. One step per line, the step's name and its argument
//! separated by a tab; blank lines and lines starting with `#` are skipped:
//!
//! - `action <id> [json argument]`: runs the action through the registry in the active window;
//! - `open <absolute path>`: opens the file in the active window (a tab of its project);
//! - `keys <keystrokes>`: GPUI keystrokes separated by spaces (`cmd-shift-a escape`), dispatched
//!   to the active window as typed keys are;
//! - `type <text>`: the text, typed into the focused element character by character;
//! - `click <x> <y>`, `rclick <x> <y>`: the pointer moves to the point (in pixels from the
//!   top-left of the window's content) and the left or right button is pressed and released
//!   there, through the window's own input dispatch;
//! - `hover <x> <y>`: the pointer moves to the point;
//! - `mods <modifiers>`: the modifier keys held from now on, `-`-separated (`shift`, `cmd-shift`; `ctrl`,
//!   `alt`, `fn` too), none when empty: a modifiers-changed event through the window's own input
//!   dispatch, as the keyboard reports a modifier pressed or released (`mods shift` then `mods` is a
//!   tap of Shift);
//! - `wait <milliseconds>`;
//! - `mark <name>`: appends `mark <name> <unix milliseconds>` to the log;
//! - `quit`.
//!
//! Each step runs from this module's own task, never from inside an update of the window it
//! drives, and the next step waits for the window to draw a frame. A step that fails, or a line
//! that does not parse, is written to the log as `fail <line> <step> <reason>` (the step's tabs
//! written as spaces) and the script goes on; a window that draws no frame within a second (covered,
//! minimized) gets a `note` line and the script goes on. The log ends with `done <unix milliseconds>`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::FutureExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AsyncApp, Capslock, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, Task, Window, point, px,
};

use crate::platform::actions::execute_action_or_throw;
use crate::workbench::windows;

/// How long a step waits for the window to draw what it changed.
const FRAME_TIMEOUT: Duration = Duration::from_secs(1);

/// One thing the script does.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Action {
        id: String,
        arg: Option<serde_json::Value>,
    },
    Open(PathBuf),
    Keys(Vec<String>),
    Type(String),
    /// A press and release of `button` at a point of the window's content.
    Click {
        at: Point<Pixels>,
        button: MouseButton,
    },
    /// The pointer moves to a point of the window's content.
    Hover(Point<Pixels>),
    /// The modifier keys held from now on.
    Modifiers(Modifiers),
    Wait(Duration),
    Mark(String),
    Quit,
}

/// A line of the script: its 1-based number, its text, and the step it says or why it says none.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub step: Result<Step, String>,
}

/// The steps of `script`, in order, skipping blank lines and `#` comments.
pub fn parse(script: &str) -> Vec<Line> {
    script
        .lines()
        .enumerate()
        .filter(|(_, text)| !text.trim().is_empty() && !text.trim_start().starts_with('#'))
        .map(|(index, text)| Line { number: index + 1, text: text.to_owned(), step: parse_step(text) })
        .collect()
}

fn parse_step(text: &str) -> Result<Step, String> {
    let (name, rest) = text.split_once('\t').unwrap_or((text, ""));
    let name = name.trim();
    let required = |what: &str| {
        let rest = rest.trim();
        if rest.is_empty() { Err(format!("{name} needs {what}")) } else { Ok(rest) }
    };
    match name {
        "action" => {
            let (id, arg) = rest.split_once('\t').unwrap_or((rest, ""));
            let id = id.trim();
            if id.is_empty() {
                return Err("action needs an id".to_owned());
            }
            let arg = arg.trim();
            let arg = if arg.is_empty() {
                None
            } else {
                Some(serde_json::from_str(arg).map_err(|error| format!("the argument is not JSON: {error}"))?)
            };
            Ok(Step::Action { id: id.to_owned(), arg })
        }
        "open" => {
            let path = PathBuf::from(required("a path")?);
            if !path.is_absolute() {
                return Err(format!("{} is not an absolute path", path.display()));
            }
            Ok(Step::Open(path))
        }
        "keys" => Ok(Step::Keys(required("keystrokes")?.split_whitespace().map(str::to_owned).collect())),
        // Typed text keeps its spaces: only the separating tab is dropped.
        "type" if !rest.is_empty() => Ok(Step::Type(rest.to_owned())),
        "type" => Err("type needs text".to_owned()),
        "wait" => {
            let millis = required("milliseconds")?;
            let millis = millis.parse::<u64>().map_err(|_| format!("{millis} is not a number of milliseconds"))?;
            Ok(Step::Wait(Duration::from_millis(millis)))
        }
        "click" => Ok(Step::Click { at: parse_point(name, rest)?, button: MouseButton::Left }),
        "rclick" => Ok(Step::Click { at: parse_point(name, rest)?, button: MouseButton::Right }),
        "hover" => Ok(Step::Hover(parse_point(name, rest)?)),
        "mods" => parse_modifiers(rest).map(Step::Modifiers),
        "mark" => Ok(Step::Mark(required("a name")?.to_owned())),
        "quit" => Ok(Step::Quit),
        other => Err(format!("there is no step called {other:?}")),
    }
}

/// The modifier keys `shift`, `ctrl`, `alt`, `cmd`, `fn` joined by `-` (none when empty).
fn parse_modifiers(rest: &str) -> Result<Modifiers, String> {
    let mut modifiers = Modifiers::default();
    for name in rest.trim().split('-').filter(|name| !name.is_empty()) {
        let held = match name.to_ascii_lowercase().as_str() {
            "shift" => &mut modifiers.shift,
            "ctrl" | "control" => &mut modifiers.control,
            "alt" | "option" => &mut modifiers.alt,
            "cmd" | "command" | "meta" | "platform" | "super" | "win" => &mut modifiers.platform,
            "fn" | "function" => &mut modifiers.function,
            _ => return Err(format!("{name} is not a modifier key")),
        };
        *held = true;
    }
    Ok(modifiers)
}

/// The point `<x> <y>` (tab-separated, in pixels) a pointer step names.
fn parse_point(name: &str, rest: &str) -> Result<Point<Pixels>, String> {
    let coordinates: Vec<&str> = rest.split('\t').map(str::trim).collect();
    let [x, y] = coordinates[..] else { return Err(format!("{name} needs an x and a y")) };
    let coordinate = |text: &str| match text.parse::<f32>() {
        Ok(value) if value.is_finite() && value >= 0.0 => Ok(px(value)),
        _ => Err(format!("{text} is not a coordinate")),
    };
    Ok(point(coordinate(x)?, coordinate(y)?))
}

/// Where the script's marks and failures go: the log file, or the application's log without one.
#[derive(Debug, Clone)]
struct Log(Option<PathBuf>);

impl Log {
    fn write(&self, fields: &[&str]) {
        let line = fields.join("\t");
        let Some(path) = &self.0 else {
            tracing::info!("inspection: {line}");
            return;
        };
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = written {
            tracing::error!("inspection: cannot write {line:?} to {}: {error}", path.display());
        }
    }
}

fn unix_millis() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis()).to_string()
}

/// Reads the script at `script` and carries it out; `log` receives marks and failures.
pub fn start_from_file(script: &Path, log: Option<PathBuf>, cx: &mut App) {
    match std::fs::read_to_string(script) {
        Ok(text) => start(&text, log, cx).detach(),
        Err(error) => Log(log).write(&["fail", "0", &script.display().to_string(), &error.to_string()]),
    }
}

/// Carries out `script` on the application's thread, one step after another.
pub fn start(script: &str, log: Option<PathBuf>, cx: &mut App) -> Task<()> {
    let lines = parse(script);
    let log = Log(log);
    cx.spawn(async move |cx| {
        for line in lines {
            let number = line.number.to_string();
            let outcome = match line.step {
                Err(reason) => Err(reason),
                Ok(step) => run(step, &log, cx).await,
            };
            if let Err(reason) = outcome {
                // The step's own tabs would split the log's fields.
                log.write(&["fail", &number, &line.text.replace('\t', " "), &reason]);
            }
        }
        log.write(&["done", &unix_millis()]);
    })
}

async fn run(step: Step, log: &Log, cx: &mut AsyncApp) -> Result<(), String> {
    match step {
        Step::Wait(duration) => {
            cx.background_executor().timer(duration).await;
            Ok(())
        }
        Step::Mark(name) => {
            log.write(&["mark", &name, &unix_millis()]);
            Ok(())
        }
        Step::Quit => {
            cx.update(|cx| cx.quit());
            Ok(())
        }
        step => {
            let handle = cx.update(target_window).ok_or_else(|| "there is no window".to_owned())?;
            let drawn = handle
                .update(cx, |_, window, cx| {
                    let outcome = perform(&step, window, cx);
                    let (drawn, done) = futures::channel::oneshot::channel();
                    window.on_next_frame(move |_, _| {
                        let _ = drawn.send(());
                    });
                    window.refresh();
                    (outcome, done)
                })
                .map_err(|error| format!("the window is gone: {error}"))?;
            let (outcome, frame) = drawn;
            // The next step sees what this one changed on screen; a window closed meanwhile ends the wait.
            // A covered or minimized window draws no frames, so the wait is bounded and says so.
            let mut frame = frame.fuse();
            let mut timeout = cx.background_executor().timer(FRAME_TIMEOUT).fuse();
            futures::select! {
                _ = frame => {}
                () = timeout => log.write(&["note", "no frame was drawn within a second: is the window covered?"]),
            }
            outcome
        }
    }
}

/// The window steps act on: the active window, else the frontmost, else the last one opened.
fn target_window(cx: &mut App) -> Option<AnyWindowHandle> {
    cx.active_window()
        .or_else(|| cx.window_stack().and_then(|stack| stack.into_iter().next()))
        .or_else(|| windows::windows(cx).pop().map(|(_, handle)| handle))
}

fn perform(step: &Step, window: &mut Window, cx: &mut App) -> Result<(), String> {
    match step {
        Step::Action { id, arg } => execute_action_or_throw(id, arg.as_ref(), window, cx).map_err(|e| e.to_string()),
        Step::Open(path) => windows::open_files(vec![path.clone()], window, cx).map_err(|e| format!("{e:#}")),
        Step::Keys(strokes) => {
            let os = cx.platform_info().os;
            for stroke in strokes {
                let keystroke = Keystroke::parse_with_os(stroke, os).map_err(|error| format!("{stroke}: {error}"))?;
                window.dispatch_keystroke(keystroke, cx);
            }
            Ok(())
        }
        Step::Type(text) => {
            for typed in text.chars() {
                if !window.dispatch_keystroke(typed_keystroke(typed), cx) {
                    return Err(format!("nothing took the typed {typed:?}: no element with text input has focus"));
                }
            }
            Ok(())
        }
        Step::Click { at, button } => {
            let (position, button) = (*at, *button);
            move_pointer(position, window, cx);
            // A press carries the modifier keys held at that moment, as the platform's does.
            let modifiers = window.modifiers();
            let down = MouseDownEvent { button, position, modifiers, click_count: 1, first_mouse: false };
            window.dispatch_event(PlatformInput::MouseDown(down), cx);
            let up = MouseUpEvent { button, position, modifiers, click_count: 1 };
            window.dispatch_event(PlatformInput::MouseUp(up), cx);
            Ok(())
        }
        Step::Hover(position) => {
            move_pointer(*position, window, cx);
            Ok(())
        }
        Step::Modifiers(modifiers) => {
            let changed = ModifiersChangedEvent { modifiers: *modifiers, capslock: Capslock::default() };
            window.dispatch_event(PlatformInput::ModifiersChanged(changed), cx);
            Ok(())
        }
        Step::Wait(_) | Step::Mark(_) | Step::Quit => Ok(()),
    }
}

/// Moves the pointer to `position`, as the platform reports a move: hover states and tooltips follow.
fn move_pointer(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    let moved = MouseMoveEvent { position, pressed_button: None, modifiers: window.modifiers() };
    window.dispatch_event(PlatformInput::MouseMove(moved), cx);
}

/// The keystroke that types `typed` on a US layout.
fn typed_keystroke(typed: char) -> Keystroke {
    let key = match typed {
        ' ' => "space".to_owned(),
        '\t' => "tab".to_owned(),
        '\n' => "enter".to_owned(),
        other => other.to_lowercase().collect(),
    };
    let modifiers = Modifiers { shift: typed.is_uppercase(), ..Modifiers::default() };
    Keystroke { modifiers, key, key_char: Some(typed.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(script: &str) -> Vec<Result<Step, String>> {
        parse(script).into_iter().map(|line| line.step).collect()
    }

    #[test]
    fn parses_every_step_with_its_argument() {
        let script = "action\tview.toggleTheme\n\
                      action\tfile.settings\t\"editor.general\"\n\
                      open\t/tmp/a b.rs\n\
                      keys\tcmd-shift-a  escape\n\
                      type\t  two words\n\
                      wait\t250\n\
                      mark\tafter settings\n\
                      click\t10\t20.5\n\
                      rclick\t0\t7\n\
                      hover\t3\t4\n\
                      mods\tcmd-shift\n\
                      mods\t\n\
                      quit";
        assert_eq!(
            steps(script),
            vec![
                Ok(Step::Action { id: "view.toggleTheme".into(), arg: None }),
                Ok(Step::Action { id: "file.settings".into(), arg: Some(serde_json::json!("editor.general")) }),
                Ok(Step::Open(PathBuf::from("/tmp/a b.rs"))),
                Ok(Step::Keys(vec!["cmd-shift-a".into(), "escape".into()])),
                Ok(Step::Type("  two words".into())),
                Ok(Step::Wait(Duration::from_millis(250))),
                Ok(Step::Mark("after settings".into())),
                Ok(Step::Click { at: point(px(10.0), px(20.5)), button: MouseButton::Left }),
                Ok(Step::Click { at: point(px(0.0), px(7.0)), button: MouseButton::Right }),
                Ok(Step::Hover(point(px(3.0), px(4.0)))),
                Ok(Step::Modifiers(Modifiers { platform: true, shift: true, ..Modifiers::default() })),
                Ok(Step::Modifiers(Modifiers::default())),
                Ok(Step::Quit),
            ]
        );
    }

    #[test]
    fn skips_blank_lines_and_comments_and_keeps_line_numbers() {
        let lines = parse("# a comment\n\n   \nmark\tone\n  # indented comment\nquit\n");
        let numbers: Vec<usize> = lines.iter().map(|line| line.number).collect();
        assert_eq!(numbers, [4, 6]);
        assert_eq!(lines[0].text, "mark\tone");
    }

    #[test]
    fn says_why_a_line_is_not_a_step() {
        let reasons: Vec<String> =
            steps("tap\tbutton\naction\naction\tx\t{not json\nopen\trelative/path\nwait\tsoon\nmark\ntype\nkeys\t \nclick\t1\nhover\tleft\t2\nmods\tshift-hyper")
                .into_iter()
                .map(|step| step.expect_err("each line is wrong"))
                .collect();
        assert_eq!(reasons[0], "there is no step called \"tap\"");
        assert_eq!(reasons[1], "action needs an id");
        assert!(reasons[2].starts_with("the argument is not JSON"), "{}", reasons[2]);
        assert_eq!(reasons[3], "relative/path is not an absolute path");
        assert_eq!(reasons[4], "soon is not a number of milliseconds");
        assert_eq!(reasons[5], "mark needs a name");
        assert_eq!(reasons[6], "type needs text");
        assert_eq!(reasons[7], "keys needs keystrokes");
        assert_eq!(reasons[8], "click needs an x and a y");
        assert_eq!(reasons[9], "left is not a coordinate");
        assert_eq!(reasons[10], "hyper is not a modifier key");
    }

    #[test]
    fn typed_characters_carry_their_text_and_shift() {
        let upper = typed_keystroke('A');
        assert_eq!((upper.key.as_str(), upper.key_char.as_deref(), upper.modifiers.shift), ("a", Some("A"), true));
        let space = typed_keystroke(' ');
        assert_eq!((space.key.as_str(), space.key_char.as_deref()), ("space", Some(" ")));
        let dash = typed_keystroke('-');
        assert_eq!((dash.key.as_str(), dash.key_char.as_deref(), dash.modifiers.shift), ("-", Some("-"), false));
    }
}
