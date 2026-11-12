//! Colors plugins report for a document (`plugin_language_visuals`): a swatch on each color's
//! line, and the same system picker as the literal swatches, whose pick the plugin turns into
//! replacement strings (`plugin_color_presentations`) for precisely the reported range.

use super::colors::from_hex6;
use crate::database::model::js_number_string;
use ide_core::{DocId, Range};
use ide_plugin::language_features::ColorInfo;

/// Wait after an edit before the plugins are asked again.
pub const DEBOUNCE_MS: u64 = 300;

/// A request for the document's plugin colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorsTicket {
    generation: u64,
    /// The document version asked about.
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorsEffect {
    /// Call [`PluginColors::run`] after this many milliseconds (replacing a pending call).
    Schedule(u64),
    /// `plugin_language_visuals(doc)`; answer with [`PluginColors::answered`].
    Fetch(ColorsTicket),
}

/// The plugin colors of one editor, by the plugin that reported each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginColors {
    generation: u64,
    colors: Vec<(String, ColorInfo)>,
    /// The last answer came while the editor was hidden and was not shown.
    skipped: bool,
}

impl PluginColors {
    /// Colors of a new editor; the first request is scheduled.
    pub fn new() -> (Self, ColorsEffect) {
        (Self::default(), ColorsEffect::Schedule(DEBOUNCE_MS))
    }

    pub fn colors(&self) -> &[(String, ColorInfo)] {
        &self.colors
    }

    fn schedule(&mut self) -> ColorsEffect {
        self.generation += 1;
        ColorsEffect::Schedule(DEBOUNCE_MS)
    }

    /// The document changed: the colors shown are stale.
    pub fn doc_changed(&mut self) -> ColorsEffect {
        self.colors.clear();
        self.schedule()
    }

    /// The editor providers changed (a plugin came or went).
    pub fn providers_changed(&mut self) -> ColorsEffect {
        self.schedule()
    }

    /// The editor is visible again; an answer skipped while hidden is asked for again.
    pub fn shown_again(&mut self) -> Option<ColorsEffect> {
        if self.skipped { Some(self.schedule()) } else { None }
    }

    /// The debounce elapsed: asks about document version `version`.
    pub fn run(&self, version: u64) -> ColorsEffect {
        ColorsEffect::Fetch(ColorsTicket { generation: self.generation, version })
    }

    /// The answer to `ticket`, for a document now at `version`, shown (`visible`) or not. A
    /// failure clears the colors. Returns whether they changed.
    pub fn answered(
        &mut self,
        ticket: ColorsTicket,
        version: u64,
        visible: bool,
        result: Result<Vec<(String, ColorInfo)>, String>,
    ) -> bool {
        if ticket.generation != self.generation {
            return false;
        }
        match result {
            Err(_) => {
                if !visible || self.colors.is_empty() {
                    return false;
                }
                self.colors.clear();
                true
            }
            Ok(_) if ticket.version != version => false,
            Ok(colors) => {
                self.skipped = !visible;
                if self.skipped {
                    return false;
                }
                self.colors = colors;
                true
            }
        }
    }
}

/// `rgba(r, g, b, a)` with the plugin's 0–1 components, the swatch's tooltip.
pub fn swatch_title(color: &ColorInfo) -> String {
    let n = |v: f32| js_number_string(f64::from(v));
    format!("rgba({}, {}, {}, {})", n(color.red), n(color.green), n(color.blue), n(color.alpha))
}

/// The swatch's fill: red, green, blue 0–255 and alpha 0–1.
pub fn swatch_fill(color: &ColorInfo) -> [f64; 4] {
    let byte = |v: f32| f64::from(v) * 255.0;
    [byte(color.red), byte(color.green), byte(color.blue), f64::from(color.alpha)]
}

/// The presentations to ask a plugin for: `plugin_color_presentations(doc, plugin, color)`.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationsRequest {
    pub doc: DocId,
    pub plugin: String,
    pub color: ColorInfo,
}

/// What the plugin's presentations come to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presentations {
    /// The document changed or became read-only meanwhile, or the plugin offered nothing.
    Nothing,
    /// Replace `range` (UTF-16 line / column) with `text`.
    Apply { range: Range, text: String },
    /// Several presentations: a menu of them (shown as written); [`PluginColorPick::choose`]
    /// applies the one chosen.
    Choose(Vec<String>),
}

/// The system color picker open for a plugin's color.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginColorPick {
    doc: DocId,
    plugin: String,
    color: ColorInfo,
    /// The document version the color was reported for.
    version: u64,
}

impl PluginColorPick {
    /// Opens the picker on `plugin`'s `color` of the document `doc` at `version`.
    pub fn open(doc: DocId, plugin: &str, color: ColorInfo, version: u64) -> Self {
        Self { doc, plugin: plugin.to_owned(), color, version }
    }

    /// The picker's initial value (`#rrggbb`).
    pub fn initial_hex(&self) -> String {
        let hex = |v: f32| format!("{:02x}", (f64::from(v) * 255.0).round().clamp(0.0, 255.0) as u8);
        format!("#{}{}{}", hex(self.color.red), hex(self.color.green), hex(self.color.blue))
    }

    /// The picker chose `hex`: the presentations to ask the plugin for (alpha and range kept).
    pub fn picked(&self, hex: &str) -> Option<PresentationsRequest> {
        let [r, g, b, _] = from_hex6(hex, 1.0)?;
        let channel = |v: f64| (v / 255.0) as f32;
        let color = ColorInfo { red: channel(r), green: channel(g), blue: channel(b), ..self.color.clone() };
        Some(PresentationsRequest { doc: self.doc, plugin: self.plugin.clone(), color })
    }

    /// The plugin's presentations, for a document now at `version` and `read_only` or not.
    pub fn answered(&self, list: Vec<String>, version: u64, read_only: bool) -> Presentations {
        if version != self.version {
            return Presentations::Nothing;
        }
        match <[String; 1]>::try_from(list) {
            Ok([text]) => self.choose(text, version, read_only),
            Err(list) if list.is_empty() => Presentations::Nothing,
            Err(list) => Presentations::Choose(list),
        }
    }

    /// A presentation chosen from the menu, for a document now at `version`.
    pub fn choose(&self, text: String, version: u64, read_only: bool) -> Presentations {
        if version != self.version || read_only {
            return Presentations::Nothing;
        }
        Presentations::Apply { range: self.color.range, text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find::utf16;
    use ide_core::Pos;

    #[test]
    fn uses_plugin_presentations_to_replace_precisely_the_reported_range() {
        let red = ColorInfo {
            range: Range::new(Pos::new(0, 1), Pos::new(0, 2)),
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        };
        let (mut colors, first) = PluginColors::new();
        assert_eq!(first, ColorsEffect::Schedule(300));
        let ColorsEffect::Fetch(ticket) = colors.run(1) else { panic!("the debounce elapsed") };
        assert!(colors.answered(ticket, 1, true, Ok(vec![("p".to_owned(), red.clone())])));
        let (plugin, color) = colors.colors()[0].clone();

        let pick = PluginColorPick::open(DocId(1), &plugin, color, 1);
        assert_eq!(pick.initial_hex(), "#ff0000");
        let request = pick.picked("#00ff00").expect("a valid pick");
        assert_eq!(request.doc, DocId(1));
        assert_eq!(request.plugin, "p");
        let chosen = &request.color;
        assert_eq!((chosen.red, chosen.green, chosen.blue, chosen.alpha), (0.0, 1.0, 0.0, 1.0));
        assert_eq!(chosen.range, red.range);

        let Presentations::Apply { range, text } = pick.answered(vec!["red".to_owned()], 1, false) else {
            panic!("one presentation applies");
        };
        let doc = "xyz";
        let edited =
            format!("{}{text}{}", utf16::slice(doc, 0, range.start.col), utf16::slice_from(doc, range.end.col));
        assert_eq!(edited, "xredz");
        assert_eq!(pick.answered(vec!["red".to_owned()], 2, false), Presentations::Nothing);
    }
}
