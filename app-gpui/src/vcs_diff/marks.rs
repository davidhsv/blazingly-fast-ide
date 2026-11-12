//! Marks on an editor's overview lane (the stripe over the scrollbar), as the change gutter, the diff
//! viewer and the merge editor produce them.

/// How a mark is drawn on the lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkKind {
    /// A bar four pixels wide.
    #[default]
    Line,
    /// A bar two pixels wide (search hits, occurrences).
    Thin,
    /// The lane's whole width, from the top of the first line to the top of the last, so a one-line
    /// change is a dash of the minimum height.
    Full,
}

/// A mark on the overview lane. Offsets are UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollbarMark {
    pub from: usize,
    /// End offset; a mark spanning several lines is taller. `None` ends at `from`.
    pub to: Option<usize>,
    /// Name of the theme token holding the colour, such as `diff-modified-stripe`.
    pub color: &'static str,
    pub kind: MarkKind,
    /// English text shown on hover.
    pub tooltip: Option<&'static str>,
    /// The higher one wins where marks overlap.
    pub priority: u32,
}
