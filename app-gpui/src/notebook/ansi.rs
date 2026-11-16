//! ANSI escape sequences of kernel text (streams, tracebacks, plain results) as styled runs.
//!
//! Select Graphic Rendition (`ESC [ … m`) sets the foreground and background colour (the eight
//! standard and eight bright colours, the 256-colour palette and 24-bit colours), bold, italic and
//! underline; every other escape sequence is dropped from the text. The colours are the palette
//! IPython's tracebacks are written for, so a traceback reads the same as in Jupyter.

/// A colour as red, green and blue.
pub type Rgb = (u8, u8, u8);

/// Text of one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnsiRun {
    pub text: String,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// The standard colours (30–37, 40–47) and the bright ones (90–97, 100–107).
const STANDARD: [Rgb; 8] =
    [(0, 0, 0), (187, 0, 0), (0, 187, 0), (187, 187, 0), (0, 0, 187), (187, 0, 187), (0, 187, 187), (255, 255, 255)];
const BRIGHT: [Rgb; 8] = [
    (85, 85, 85),
    (255, 85, 85),
    (0, 255, 0),
    (255, 255, 85),
    (85, 85, 255),
    (255, 85, 255),
    (85, 255, 255),
    (255, 255, 255),
];

/// Colour `index` of the 256-colour palette.
fn palette(index: u8) -> Rgb {
    match index {
        0..=7 => STANDARD[usize::from(index)],
        8..=15 => BRIGHT[usize::from(index - 8)],
        16..=231 => {
            let level = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            let n = index - 16;
            (level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            let grey = 8 + (index - 232) * 10;
            (grey, grey, grey)
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Style {
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// A 256-colour or 24-bit colour after `38` or `48`; how many parameters it took.
fn extended(params: &[u16]) -> (Option<Rgb>, usize) {
    let byte = |at: usize| params.get(at).map(|v| u8::try_from(*v).unwrap_or(u8::MAX));
    match params.first() {
        Some(5) => (byte(1).map(palette), 2),
        Some(2) => match (byte(1), byte(2), byte(3)) {
            (Some(r), Some(g), Some(b)) => (Some((r, g, b)), 4),
            _ => (None, params.len()),
        },
        _ => (None, params.len().min(1)),
    }
}

impl Style {
    fn apply(&mut self, params: &[u16]) {
        if params.is_empty() {
            *self = Style::default();
            return;
        }
        let mut at = 0;
        while at < params.len() {
            let code = params[at];
            at += 1;
            match code {
                0 => *self = Style::default(),
                1 => self.bold = true,
                3 => self.italic = true,
                4 => self.underline = true,
                22 => self.bold = false,
                23 => self.italic = false,
                24 => self.underline = false,
                30..=37 => self.fg = Some(STANDARD[usize::from(code - 30)]),
                39 => self.fg = None,
                40..=47 => self.bg = Some(STANDARD[usize::from(code - 40)]),
                49 => self.bg = None,
                90..=97 => self.fg = Some(BRIGHT[usize::from(code - 90)]),
                100..=107 => self.bg = Some(BRIGHT[usize::from(code - 100)]),
                38 | 48 => {
                    let (colour, used) = extended(&params[at..]);
                    at += used;
                    if colour.is_some() {
                        if code == 38 {
                            self.fg = colour;
                        } else {
                            self.bg = colour;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn push(runs: &mut Vec<AnsiRun>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && (last.fg, last.bg, last.bold, last.italic, last.underline)
            == (style.fg, style.bg, style.bold, style.italic, style.underline)
    {
        last.text.push_str(text);
        return;
    }
    runs.push(AnsiRun {
        text: text.to_owned(),
        fg: style.fg,
        bg: style.bg,
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
    });
}

/// `text` as styled runs, its escape sequences applied and removed.
pub fn ansi_runs(text: &str) -> Vec<AnsiRun> {
    let mut runs = Vec::new();
    let mut style = Style::default();
    let mut rest = text;
    while let Some(at) = rest.find('\u{1b}') {
        push(&mut runs, &rest[..at], style);
        let sequence = &rest[at + 1..];
        let Some(body) = sequence.strip_prefix('[') else {
            // A lone escape, or an escape that is not a control sequence: one character dropped.
            rest = sequence.char_indices().nth(1).map_or("", |(next, _)| &sequence[next..]);
            continue;
        };
        // Parameters and intermediates up to the final byte (`@` to `~`).
        let end = body.find(|c: char| ('@'..='~').contains(&c));
        let Some(end) = end else {
            rest = "";
            break;
        };
        if body[end..].starts_with('m') {
            let params: Vec<u16> = body[..end]
                .split([';', ':'])
                .filter(|p| !p.is_empty())
                .map(|p| p.parse().unwrap_or(u16::MAX))
                .collect();
            let reset = body[..end].is_empty();
            style.apply(if reset { &[] } else { &params });
        }
        rest = &body[end + 1..];
    }
    push(&mut runs, rest, style);
    runs
}

/// The text without its escape sequences.
pub fn plain_text(runs: &[AnsiRun]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_bold_and_resets_become_runs_and_other_sequences_are_dropped() {
        let runs = ansi_runs("\u{1b}[0;31mValueError\u{1b}[0m: bad \u{1b}[1;38;5;208mx\u{1b}[39m\u{1b}[22m\u{1b}[2Ky");
        let shown: Vec<(&str, Option<Rgb>, bool)> = runs.iter().map(|r| (r.text.as_str(), r.fg, r.bold)).collect();
        assert_eq!(
            shown,
            [
                ("ValueError", Some((187, 0, 0)), false),
                (": bad ", None, false),
                ("x", Some((255, 135, 0)), true),
                ("y", None, false)
            ]
        );
        assert_eq!(plain_text(&runs), "ValueError: bad xy");
    }

    #[test]
    fn true_colour_backgrounds_and_bright_colours_are_read() {
        let runs = ansi_runs("\u{1b}[48;2;1;2;3;92mok\u{1b}[49;4mu");
        assert_eq!(runs[0].bg, Some((1, 2, 3)));
        assert_eq!(runs[0].fg, Some((0, 255, 0)));
        assert_eq!((runs[1].bg, runs[1].underline), (None, true));
    }

    #[test]
    fn an_unterminated_sequence_ends_the_text() {
        assert_eq!(plain_text(&ansi_runs("done\u{1b}[31")), "done");
        assert_eq!(plain_text(&ansi_runs("a\u{1b}b")), "a");
    }
}
