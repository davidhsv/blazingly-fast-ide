//! Comment with Line Comment, Comment with Block Comment and Enter inside a line comment.
//!
//! The comment tokens come from the syntax context at each position ([`CommentTokensAt`]): a JSX
//! child is commented `{/* … */}`, the text inside that comment is script again, where the tokens
//! are `//` and `/* */`.

use super::tree::SyntaxTree;
use super::visible_column::{ColumnBias, offset_at_visible_column, visible_column};
use super::{
    CommentTokensAt, Edit, EditState, RangeEdit, Selection, SelectionExt, apply_changes, caret, change,
    change_by_range, insertion, map_selection, utf16_len,
};
use crate::editor::buffer::Assoc;
use ide_core::TextChange;

/// Code Style › Code Generation › Comment Code: where a line comment goes and whether a space
/// follows its token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCommentStyle {
    pub at_first_column: bool,
    pub add_space: bool,
}

impl Default for LineCommentStyle {
    fn default() -> Self {
        Self { at_first_column: true, add_space: false }
    }
}

/// UTF-16 length (and byte length: they are ASCII) of the spaces and tabs `text` starts with.
fn leading_ws(text: &str) -> u32 {
    (text.len() - text.trim_start_matches([' ', '\t']).len()) as u32
}

/// UTF-16 length of the whitespace `text` starts with.
fn leading_space_units(text: &str) -> u32 {
    utf16_len(&text[..text.len() - text.trim_start().len()])
}

/// UTF-16 length of the whitespace `text` ends with.
fn trailing_space_units(text: &str) -> u32 {
    utf16_len(&text[text.trim_end().len()..])
}

/// The line comment token in force at `pos`.
fn line_token(comments: CommentTokensAt<'_>, pos: u32) -> Option<String> {
    comments(pos).into_iter().next().and_then(|tokens| tokens.line).filter(|token| !token.is_empty())
}

/// Where the caret goes after Comment with Line Comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaretUpdate {
    /// After the token inserted on an empty line.
    CommentStart,
    /// To the line below, at the same visible column.
    Down,
    /// A whole-line selection starts at its first line's start again.
    RestoreSelection,
}

/// Consecutive lines the selection ranges touch, commented as one.
struct Block {
    start_line: u32,
    end_line: u32,
    ranges: Vec<usize>,
    caret_update: Option<CaretUpdate>,
}

/// ⌘/: comments the caret and selected lines, or uncomments them when every line is commented.
/// Without a selection the caret moves down; on an empty line it stays after the inserted token,
/// unless the command is `repeated` (the state is the one the previous ⌘/ left). In a language
/// without line comments (CSS, HTML…) the lines get a block comment.
pub fn comment_line(
    state: &EditState,
    comments: CommentTokensAt<'_>,
    style: LineCommentStyle,
    tree: &SyntaxTree,
    repeated: bool,
) -> Option<Edit> {
    let mut order: Vec<(usize, Selection)> = state.selections.iter().copied().enumerate().collect();
    order.sort_by_key(|(_, range)| range.start());
    let mut blocks: Vec<Block> = Vec::new();
    for (index, range) in order {
        let first = state.line_at(range.start());
        let start_line = first.number;
        let mut end_line = state.line_at(range.end()).number;
        if !range.is_caret() && end_line > start_line && state.line(end_line).from == range.end() {
            end_line -= 1;
        }
        let ends_at_line_start = range.end() == state.line_at(range.end()).from;
        let whole_lines =
            range.is_caret() || (range.start() == first.from && ends_at_line_start && range.end() > range.start());
        let empty_line = first.text.trim().is_empty();
        let caret_update = if range.is_caret() && empty_line && !repeated {
            Some(CaretUpdate::CommentStart)
        } else if range.is_caret() {
            Some(CaretUpdate::Down)
        } else if whole_lines {
            Some(CaretUpdate::RestoreSelection)
        } else {
            None
        };
        match blocks.last_mut() {
            Some(block) if start_line <= block.end_line + 1 => {
                block.ranges.push(index);
                block.end_line = block.end_line.max(end_line);
                block.caret_update = caret_update;
            }
            _ => blocks.push(Block { start_line, end_line, ranges: vec![index], caret_update }),
        }
    }

    let mut tokens = Vec::with_capacity(blocks.len());
    for block in &blocks {
        let line = state.line(block.start_line);
        match line_token(comments, line.from + leading_ws(&line.text)) {
            Some(token) => tokens.push(token),
            None => return block_comment_lines(state, comments, tree),
        }
    }

    let mut all_commented = true;
    let mut commented_non_empty = false;
    for (block, token) in blocks.iter().zip(&tokens) {
        let single = block.start_line == block.end_line;
        for number in block.start_line..=block.end_line {
            let text = state.line(number).text;
            let commented = text[leading_ws(&text) as usize..].starts_with(token.as_str());
            let empty = text.trim().is_empty();
            if commented && !empty {
                commented_non_empty = true;
            }
            if !commented && (single || !empty) {
                all_commented = false;
            }
        }
        if all_commented && !single && !commented_non_empty {
            all_commented = false;
        }
    }

    let doc_len = state.doc_len();
    let mut changes: Vec<TextChange> = Vec::new();
    for (block, token) in blocks.iter().zip(&tokens) {
        // A word token (Batch `@REM`) needs a separator even when comment spacing is off.
        let add_space = style.add_space || token.ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if all_commented {
            for number in block.start_line..=block.end_line {
                let line = state.line(number);
                let ws = leading_ws(&line.text);
                let rest = &line.text[ws as usize..];
                if !rest.starts_with(token.as_str()) || line.text.trim().is_empty() {
                    continue;
                }
                let mut end = line.from + ws + utf16_len(token);
                let mut after = &rest[token.len()..];
                if add_space && let Some(stripped) = after.strip_prefix(' ') {
                    end += 1;
                    after = stripped;
                }
                // Nothing but whitespace left: the line is emptied.
                if after.trim().is_empty() {
                    changes.push(change(line.from, line.to, ""));
                } else {
                    changes.push(change(line.from + ws, end, ""));
                }
            }
            continue;
        }
        let comment_start = block.caret_update == Some(CaretUpdate::CommentStart);
        let prefix_for = |shifted: u32, line_to: u32| {
            let space = add_space && (shifted < line_to || (comment_start && shifted < doc_len));
            if space { format!("{token} ") } else { token.clone() }
        };
        if style.at_first_column {
            for number in block.start_line..=block.end_line {
                let line = state.line(number);
                changes.push(insertion(line.from, prefix_for(line.from + leading_ws(&line.text), line.to)));
            }
            continue;
        }
        let min = min_indent(state, block.start_line, block.end_line, token);
        for number in block.start_line..=block.end_line {
            let line = state.line(number);
            let tab = state.tab_size.max(1);
            let (mut columns, mut index) = (0, 0u32);
            for c in line.text.chars() {
                if columns >= min {
                    break;
                }
                match c {
                    ' ' => columns += 1,
                    '\t' => columns += tab - columns % tab,
                    _ => break,
                }
                index += 1;
            }
            let prefix = prefix_for(line.from + leading_ws(&line.text), line.to);
            if columns >= min {
                changes.push(insertion(line.from + index, prefix));
            } else {
                changes.push(change(line.from, line.from + index, state.indent_string(min) + &prefix));
            }
        }
    }

    let next = EditState { rope: apply_changes(&state.rope, &changes), ..state.clone() };
    let move_carets = !blocks.iter().any(|block| block.ranges.len() > 1 && block.start_line != block.end_line);
    let mut ranges: Vec<Selection> =
        state.selections.iter().map(|range| map_selection(*range, &changes, Assoc::Before)).collect();
    for (block, token) in blocks.iter().zip(&tokens) {
        let Some(update) = block.caret_update.filter(|_| move_carets) else {
            continue;
        };
        for &index in &block.ranges {
            let range = ranges[index];
            match update {
                CaretUpdate::CommentStart => {
                    let line = next.line(block.start_line);
                    let mut pos = line.from + leading_ws(&line.text) + utf16_len(token);
                    pos += leading_ws(&next.slice(pos, line.to));
                    ranges[index] = caret(pos.min(next.doc_len()));
                }
                CaretUpdate::Down => {
                    let line = next.line_at(range.head());
                    if line.number + 1 >= next.line_count() {
                        continue;
                    }
                    let column = visible_column(&line.text, range.head() - line.from, state.tab_size);
                    let below = next.line(line.number + 1);
                    let offset = offset_at_visible_column(&below.text, column, state.tab_size, ColumnBias::Nearest);
                    ranges[index] = caret(below.from + offset);
                }
                CaretUpdate::RestoreSelection => {
                    let from = next.line_at(range.start()).from;
                    ranges[index] =
                        if range.head() >= range.anchor() { (from, range.end()) } else { (range.end(), from) };
                }
            }
        }
    }
    Some(Edit::new(changes, ranges, state.main))
}

/// The smallest indentation of the non-blank lines, and no deeper than a line comment right above.
fn min_indent(state: &EditState, start_line: u32, end_line: u32, token: &str) -> u32 {
    let indent_of = |text: &str| state.count_column(&text[..leading_ws(text) as usize]);
    let last = state.line_count() - 1;
    let mut min: Option<u32> = None;
    let scan = |from: u32, to: u32, min: &mut Option<u32>| {
        for number in from..=to {
            let line = state.line(number);
            if line.text.trim().is_empty() && number < last {
                continue;
            }
            let indent = indent_of(&line.text);
            *min = Some(min.map_or(indent, |min| min.min(indent)));
        }
    };
    scan(start_line, end_line, &mut min);
    if min.is_none() && start_line == end_line && start_line < last {
        scan(start_line + 1, start_line + 1, &mut min);
    }
    if start_line > 0 {
        let above = state.line(start_line - 1).text;
        if above[leading_ws(&above) as usize..].starts_with(token) {
            let indent = indent_of(&above);
            min = Some(min.map_or(indent, |min| min.min(indent)));
        }
    }
    min.unwrap_or(0)
}

/// A block comment token pair.
type CommentPair = (String, String);

/// The block comment pairs in force at `pos`, the more specific first.
fn comment_pairs(comments: CommentTokensAt<'_>, pos: u32) -> Vec<CommentPair> {
    let mut pairs: Vec<CommentPair> = Vec::new();
    for pair in comments(pos).into_iter().filter_map(|tokens| tokens.block) {
        if !pair.0.is_empty() && !pair.1.is_empty() && !pairs.contains(&pair) {
            pairs.push(pair);
        }
    }
    pairs
}

/// How far around a range its comment tokens are looked for.
const BLOCK_SEARCH: u32 = 50;

/// A block comment around a range: where its text starts and ends (inside the tokens), and
/// whether one space next to each token belongs to it.
#[derive(Debug, Clone, Copy)]
struct BlockComment {
    open: u32,
    open_margin: u32,
    close: u32,
    close_margin: u32,
}

/// The comment `pair` around `from..to`: tokens just outside the range (past whitespace), or just
/// inside it when the range takes the tokens in.
fn find_block_comment(state: &EditState, (open, close): &CommentPair, from: u32, to: u32) -> Option<BlockComment> {
    let before = state.slice(from.saturating_sub(BLOCK_SEARCH), from);
    let after = state.slice(to, to.saturating_add(BLOCK_SEARCH));
    let space_before = trailing_space_units(&before);
    let space_after = leading_space_units(&after);
    if before.trim_end().ends_with(open.as_str()) && after.trim_start().starts_with(close.as_str()) {
        return Some(BlockComment {
            open: from - space_before,
            open_margin: u32::from(space_before > 0),
            close: to + space_after,
            close_margin: u32::from(space_after > 0),
        });
    }
    let inside = to - from <= 2 * BLOCK_SEARCH;
    let head = state.slice(from, if inside { to } else { from + BLOCK_SEARCH });
    let tail = if inside { head.clone() } else { state.slice(from.max(to - BLOCK_SEARCH), to) };
    let head_code = head.trim_start();
    let tail_code = tail.trim_end();
    if !head_code.starts_with(open.as_str()) || !tail_code.ends_with(close.as_str()) {
        return None;
    }
    let is_space = |c: Option<char>| c.is_some_and(char::is_whitespace);
    let after_open = head_code[open.len()..].chars().next();
    let before_close = tail_code[..tail_code.len() - close.len()].chars().next_back();
    Some(BlockComment {
        open: from + leading_space_units(&head) + utf16_len(open),
        open_margin: u32::from(is_space(after_open)),
        close: to - trailing_space_units(&tail) - utf16_len(close),
        close_margin: u32::from(is_space(before_close)),
    })
}

/// The lines the selection touches, from their first non-blank column to their end, merged.
fn selected_lines(state: &EditState) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for range in &state.selections {
        let first = state.line_at(range.start());
        let mut last = if range.end() <= first.to { first.clone() } else { state.line_at(range.end()) };
        if last.from > first.from && last.from == range.end() {
            last = if state.next_line_start(&first) == Some(range.end()) {
                first.clone()
            } else {
                state.line_at(range.end() - 1)
            };
        }
        match ranges.last_mut() {
            Some(previous) if previous.1 > first.from => previous.1 = last.to,
            _ => ranges.push((first.from + leading_space_units(&first.text), last.to)),
        }
    }
    ranges
}

fn is_comment_node(tree: &SyntaxTree, node: usize) -> bool {
    tree.name(node).to_ascii_lowercase().contains("comment")
}

/// What taking block comments out of ranges finds.
enum Uncomment {
    /// The comments come out.
    Done(Edit),
    /// The tokens belong to separate comments with live code between them: nothing changes.
    Blocked,
    /// A range is not commented.
    NotCommented,
}

/// Takes the block comment around each of `ranges` out. The pairs to look for are read at both
/// ends of a range, past its last line, and just outside matching tokens; the widest pair wins,
/// so the outer comment of a JSX `{/* … */}` is the one removed. A comment whose text holds its
/// own closing token is only taken out when the parser confirms it is one nested comment.
fn uncomment_block(
    state: &EditState,
    comments: CommentTokensAt<'_>,
    tree: &SyntaxTree,
    ranges: &[(u32, u32)],
) -> Uncomment {
    let doc_len = state.doc_len();
    let mut found: Vec<Option<(CommentPair, BlockComment)>> = Vec::with_capacity(ranges.len());
    for &(from, to) in ranges {
        let mut pairs = comment_pairs(comments, from);
        for pos in [to, state.line_at(to).to] {
            for pair in comment_pairs(comments, pos) {
                if !pairs.contains(&pair) {
                    pairs.push(pair);
                }
            }
        }
        let mut best: Option<(CommentPair, BlockComment)> = None;
        let mut index = 0;
        while index < pairs.len() {
            let pair = pairs[index].clone();
            index += 1;
            let Some(comment) = find_block_comment(state, &pair, from, to) else {
                continue;
            };
            let outside = [
                (comment.open - utf16_len(&pair.0)).saturating_sub(1),
                (comment.close + utf16_len(&pair.1) + 1).min(doc_len),
            ];
            for pos in outside {
                for outer in comment_pairs(comments, pos) {
                    if !pairs.contains(&outer) {
                        pairs.push(outer);
                    }
                }
            }
            let start = comment.open - utf16_len(&pair.0);
            if best.as_ref().is_none_or(|(widest, current)| start < current.open - utf16_len(&widest.0)) {
                best = Some((pair, comment));
            }
        }
        found.push(best);
    }
    for (pair, comment) in found.iter().flatten() {
        if !state.slice(comment.open, comment.close).contains(pair.1.as_str()) {
            continue;
        }
        let node = tree
            .resolve_inner(comment.open, 1)
            .and_then(|node| tree.ancestors(node).find(|ancestor| is_comment_node(tree, *ancestor)));
        let confirmed = node.is_some_and(|node| {
            let node = tree.node(node);
            node.from == comment.open - utf16_len(&pair.0) && node.to == comment.close + utf16_len(&pair.1)
        });
        if !confirmed {
            return Uncomment::Blocked;
        }
    }
    if found.iter().any(Option::is_none) {
        return Uncomment::NotCommented;
    }
    let mut changes = Vec::new();
    for (pair, comment) in found.iter().flatten() {
        changes.push(change(comment.open - utf16_len(&pair.0), comment.open + comment.open_margin, ""));
        changes.push(change(comment.close - comment.close_margin, comment.close + utf16_len(&pair.1), ""));
    }
    changes.sort_by_key(|change| change.from);
    Uncomment::Done(Edit::following(state, changes, Assoc::Before))
}

/// Wraps each of `ranges` in the block comment in force at its start (`open ` … ` close`); the
/// ranges already commented are left alone, and when all are, their comments come out. `None`
/// when a range has no block comment tokens.
fn toggle_block_comment(state: &EditState, comments: CommentTokensAt<'_>, ranges: &[(u32, u32)]) -> Option<Edit> {
    let mut tokens = Vec::with_capacity(ranges.len());
    for &(from, _) in ranges {
        let block = comments(from).into_iter().next()?.block;
        tokens.push(block.filter(|(open, close)| !open.is_empty() && !close.is_empty())?);
    }
    let found: Vec<Option<BlockComment>> =
        ranges.iter().zip(&tokens).map(|(&(from, to), pair)| find_block_comment(state, pair, from, to)).collect();
    let mut changes = Vec::new();
    if found.iter().any(Option::is_none) {
        for ((&(from, to), (open, close)), comment) in ranges.iter().zip(&tokens).zip(&found) {
            if comment.is_none() {
                changes.push(insertion(from, format!("{open} ")));
                changes.push(insertion(to, format!(" {close}")));
            }
        }
    } else {
        for ((open, close), comment) in tokens.iter().zip(found.iter().flatten()) {
            changes.push(change(comment.open - utf16_len(open), comment.open + comment.open_margin, ""));
            changes.push(change(comment.close - comment.close_margin, comment.close + utf16_len(close), ""));
        }
    }
    changes.sort_by_key(|change| change.from);
    Some(Edit::following(state, changes, Assoc::Before))
}

/// ⌘⇧/: wraps the selection in a block comment, or takes the block comment around it out. `None`
/// in a language without block comments.
pub fn comment_block(state: &EditState, comments: CommentTokensAt<'_>, tree: &SyntaxTree) -> Option<Edit> {
    let ranges: Vec<(u32, u32)> = state.selections.iter().map(|range| (range.start(), range.end())).collect();
    match uncomment_block(state, comments, tree, &ranges) {
        Uncomment::Done(edit) => Some(edit),
        Uncomment::Blocked => Some(Edit::select(state.selections.clone(), state.main)),
        Uncomment::NotCommented => toggle_block_comment(state, comments, &ranges),
    }
}

/// ⌘/ in a language without line comments: the block comment of the selected lines.
fn block_comment_lines(state: &EditState, comments: CommentTokensAt<'_>, tree: &SyntaxTree) -> Option<Edit> {
    let lines = selected_lines(state);
    match uncomment_block(state, comments, tree, &lines) {
        Uncomment::Done(edit) => Some(edit),
        Uncomment::Blocked => Some(Edit::select(state.selections.clone(), state.main)),
        Uncomment::NotCommented => toggle_block_comment(state, comments, &lines),
    }
}

/// The start of the line comment `token` that holds `pos` (after at least its token) on the
/// line of `pos`.
fn line_comment_start(state: &EditState, tree: &SyntaxTree, pos: u32, token: &str) -> Option<u32> {
    if pos < 1 {
        return None;
    }
    let node = tree.node(tree.resolve_inner(pos, -1)?).clone();
    if !node.name.to_ascii_lowercase().contains("comment") || node.from >= pos {
        return None;
    }
    let line = state.line_at(pos);
    if node.from < line.from || !state.slice(node.from, pos).starts_with(token) {
        return None;
    }
    (node.from + utf16_len(token) <= pos).then_some(node.from)
}

/// Enter inside a line comment with text after the caret: the rest moves to a new line that
/// continues the comment, with the comment's own spacing. A trailing comment's rest goes to a
/// commented line at the line's indentation. `None` with a selection, outside a comment or at
/// its end.
pub fn enter_in_line_comment(state: &EditState, comments: CommentTokensAt<'_>, tree: &SyntaxTree) -> Option<Edit> {
    if state.selections.iter().any(|range| !range.is_caret()) {
        return None;
    }
    let br = state.line_break.as_str();
    let mut plans: Vec<(Vec<TextChange>, u32)> = Vec::with_capacity(state.selections.len());
    for range in &state.selections {
        let pos = range.head();
        let token = line_token(comments, pos)?;
        let start = line_comment_start(state, tree, pos, &token)?;
        let line = state.line_at(pos);
        let offset = pos + leading_space_units(&state.slice(pos, line.to));
        if offset >= line.to {
            return None;
        }
        let indent = &line.text[..leading_ws(&line.text) as usize];
        let only_comment = start == line.from + utf16_len(indent);
        let token_len = utf16_len(&token);
        if state.slice(offset, offset + token_len) == token {
            // The rest is a comment already: break before it.
            let after = offset + token_len;
            let mut changes = vec![insertion(offset, format!("{br}{indent}"))];
            if after < line.to && state.slice(after, after + 1) != " " {
                changes.push(insertion(after, " "));
            }
            plans.push((changes, if only_comment { token_len + 1 } else { 0 }));
            continue;
        }
        let mut spacing = " ".to_owned();
        let mut to = pos;
        if only_comment {
            let rest = state.slice(start + token_len, line.to);
            let own = &rest[..leading_ws(&rest) as usize];
            if !own.is_empty() {
                spacing = own.to_owned();
            }
            to = offset;
        } else if state.slice(pos, pos + 1) == " " {
            spacing.clear();
        }
        let caret_at = if only_comment { token_len + utf16_len(&spacing) } else { 0 };
        plans.push((vec![change(pos, to, format!("{br}{indent}{token}{spacing}"))], caret_at));
    }
    Some(change_by_range(state, |index, range| {
        let (changes, caret_at) = plans[index].clone();
        // The caret lands on the new line, after its indent (and the token when the comment owns the line).
        let line_start = changes
            .iter()
            .find_map(|change| {
                change.insert.find(br).map(|at| change.from + utf16_len(&change.insert[..at]) + utf16_len(br))
            })
            .unwrap_or(range.head());
        let indent = leading_ws(&state.line_at(range.head()).text);
        RangeEdit { changes, range: caret(line_start + indent + caret_at) }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::CommentTokens;
    use crate::editor_commands::testing::{show, state};
    use crate::editor_commands::tree::TreeNode;

    fn slashes(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("//"), None)]
    }

    fn script(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("//"), Some(("/*", "*/")))]
    }

    fn markup(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(None, Some(("<!--", "-->")))]
    }

    fn style_sheet(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(None, Some(("/*", "*/")))]
    }

    fn python(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("#"), None)]
    }

    fn batch(_: u32) -> Vec<CommentTokens> {
        vec![CommentTokens::new(Some("@REM"), None)]
    }

    const INDENTED: LineCommentStyle = LineCommentStyle { at_first_column: false, add_space: true };

    /// TSX comment tokens as the parser's syntax contexts give them: JSX children (between the
    /// first `<div>` and the last `</div>`, outside `{…}`) take `{/* */}` first, then the
    /// script's; everything else is script.
    fn tsx_at(text: &str, pos: u32) -> Vec<CommentTokens> {
        let jsx = CommentTokens::new(None, Some(("{/*", "*/}")));
        let units: Vec<u16> = text.encode_utf16().collect();
        let find = |needle: &str, last: bool| {
            let needle: Vec<u16> = needle.encode_utf16().collect();
            let mut hits =
                (0..=units.len().saturating_sub(needle.len())).filter(|at| units[*at..].starts_with(&needle));
            if last { hits.next_back() } else { hits.next() }
        };
        let (Some(open), Some(close)) = (find("<div>", false), find("</div>", true)) else {
            return script(pos);
        };
        let children = (open + 5) as u32..=close as u32;
        if !children.contains(&pos) {
            return script(pos);
        }
        let depth = units[open + 5..pos as usize].iter().fold(0i32, |depth, unit| match unit {
            0x7b => depth + 1,
            0x7d => depth - 1,
            _ => depth,
        });
        if depth > 0 { script(pos) } else { vec![jsx, script(pos).remove(0)] }
    }

    /// Runs `command` with the TSX token contexts of the current text.
    fn tsx(
        before: &EditState,
        command: fn(&EditState, CommentTokensAt<'_>, &SyntaxTree) -> Option<Edit>,
    ) -> Option<EditState> {
        let text = before.text();
        let at = |pos: u32| tsx_at(&text, pos);
        command(before, &at, &SyntaxTree::default()).map(|edit| crate::editor_commands::apply(before, &edit))
    }

    fn line_command(state: &EditState, comments: CommentTokensAt<'_>, tree: &SyntaxTree) -> Option<Edit> {
        comment_line(state, comments, LineCommentStyle::default(), tree, false)
    }

    fn run(before: &EditState, edit: Option<Edit>) -> EditState {
        crate::editor_commands::testing::run(before, edit)
    }

    fn line(before: &EditState, comments: CommentTokensAt<'_>, style: LineCommentStyle) -> EditState {
        run(before, comment_line(before, comments, style, &SyntaxTree::default(), false))
    }

    fn line_text(state: &EditState, number: u32) -> String {
        state.line(number).text
    }

    #[test]
    fn comments_at_the_first_column_without_a_space_and_moves_the_caret_down() {
        assert_eq!(show(&line(&state("  a|b\n  cd\n"), &slashes, LineCommentStyle::default())), "//  ab\n  cd|\n");
    }

    #[test]
    fn moves_every_caret_down() {
        assert_eq!(show(&line(&state("a|\nb\nc|\nd\n"), &slashes, LineCommentStyle::default())), "//a\nb|\n//c\nd|\n");
    }

    #[test]
    fn moves_the_caret_down_to_the_same_visible_column_past_cjk_text() {
        // "//张三" is six columns wide.
        let after = line(&state("张三|x\nabcdefgh\n"), &slashes, LineCommentStyle::default());
        assert_eq!(show(&after), "//张三x\nabcdef|gh\n");
    }

    #[test]
    fn keeps_the_caret_after_the_prefix_on_an_empty_line() {
        assert_eq!(show(&line(&state("f {\n  |\n  b"), &slashes, INDENTED)), "f {\n  // |\n  b");
        assert_eq!(show(&line(&state("a\n|\nb"), &slashes, LineCommentStyle::default())), "a\n//|\nb");
    }

    #[test]
    fn uncomments_with_or_without_the_space() {
        let before = state("[//a\n  // b\n]c");
        assert_eq!(line(&before, &slashes, LineCommentStyle::default()).text(), "a\n   b\nc");
        assert_eq!(line(&before, &slashes, INDENTED).text(), "a\n  b\nc");
    }

    #[test]
    fn comments_at_the_smallest_indent_of_the_lines_when_not_at_the_first_column() {
        let after = line(&state("[  a\n    b\n\n  c]"), &slashes, INDENTED);
        assert_eq!(after.text(), "  // a\n  //   b\n  //\n  // c");
    }

    #[test]
    fn restores_a_whole_line_selection_from_its_first_lines_start() {
        assert_eq!(show(&line(&state("x\n[ab\ncd\n]"), &slashes, LineCommentStyle::default())), "x\n[//ab\n//cd\n]");
    }

    /// A JavaScript line with a line comment node over `comment` (UTF-16 offsets).
    fn with_comment(text: &str, comment: (u32, u32)) -> SyntaxTree {
        let end = utf16_len(text);
        SyntaxTree::new(vec![TreeNode::new("Script", 0, end), TreeNode::new("LineComment", comment.0, comment.1)])
    }

    #[test]
    fn continues_the_comment_with_its_own_spacing() {
        let before = state("  //  one |two");
        let tree = with_comment("  //  one two", (2, 13));
        let edit = enter_in_line_comment(&before, &slashes, &tree);
        assert!(edit.is_some());
        assert_eq!(show(&run(&before, edit)), "  //  one \n  //  |two");
    }

    #[test]
    fn splits_a_trailing_comment_onto_a_commented_line() {
        let before = state("f(); // one |two");
        let tree = SyntaxTree::new(vec![
            TreeNode::new("Script", 0, 15),
            TreeNode::new("ExpressionStatement", 0, 4),
            TreeNode::new("LineComment", 5, 15),
        ]);
        assert_eq!(show(&run(&before, enter_in_line_comment(&before, &slashes, &tree))), "f(); // one \n|// two");
    }

    #[test]
    fn does_nothing_at_the_end_of_the_comment_or_outside_one() {
        let end = state("// one|");
        assert_eq!(enter_in_line_comment(&end, &slashes, &with_comment("// one", (0, 6))), None);
        let call = state("f(|x)");
        let tree = SyntaxTree::new(vec![
            TreeNode::new("Script", 0, 4),
            TreeNode::new("CallExpression", 0, 4),
            TreeNode::new("VariableName", 0, 1),
            TreeNode::new("ArgList", 1, 4),
        ]);
        assert_eq!(enter_in_line_comment(&call, &slashes, &tree), None);
    }

    #[test]
    fn resolves_batch_by_ui_mode_and_bat_cmd_extensions_and_toggles_rem_comments() {
        let commented = line(&state("echo| hello\n"), &batch, LineCommentStyle::default());
        assert_eq!(commented.text(), "@REM echo hello\n");
        let at_start = EditState { selections: vec![caret(0)], main: 0, ..commented };
        assert_eq!(line(&at_start, &batch, LineCommentStyle::default()).text(), "echo hello\n");
    }

    fn wrap(body: &str) -> String {
        format!("const a = (\n  <div>\n    {body}\n  </div>\n);")
    }

    #[test]
    fn takes_a_jsx_line_comment_back_out_with_the_caret() {
        let before = state(&wrap("|</div>"));
        let commented = tsx(&before, line_command).expect("the line is commented");
        assert_eq!(line_text(&commented, 2), "    {/* </div> */}");
        let back = tsx(&commented, line_command).expect("the comment comes out");
        assert_eq!(show(&back), wrap("|</div>"));
    }

    #[test]
    fn takes_a_jsx_line_comment_back_out_with_the_line_selected() {
        let before = state(&wrap("[</div>]"));
        let commented = tsx(&before, line_command).expect("the line is commented");
        assert_eq!(line_text(&commented, 2), "    {/* </div> */}");
        let back = tsx(&commented, line_command).expect("the comment comes out");
        assert_eq!(line_text(&back, 2), "    </div>");
    }

    #[test]
    fn takes_a_jsx_block_comment_back_out_after_cmd_shift_slash() {
        let before = state(&wrap("[</div>]"));
        let commented = tsx(&before, comment_block).expect("the selection is commented");
        assert_eq!(line_text(&commented, 2), "    {/* </div> */}");
        let back = tsx(&commented, comment_block).expect("the comment comes out");
        assert_eq!(show(&back), wrap("[</div>]"));
    }

    #[test]
    fn takes_a_block_comment_back_out_in_a_language_with_no_line_comment() {
        let before = state("<section>\n  |<p>hi</p>\n</section>");
        let commented = line(&before, &markup, LineCommentStyle::default());
        assert_eq!(line_text(&commented, 1), "  <!-- <p>hi</p> -->");
        let back = line(&commented, &markup, LineCommentStyle::default());
        assert_eq!(show(&back), "<section>\n  |<p>hi</p>\n</section>");
    }

    #[test]
    fn wraps_several_selected_jsx_lines_once_and_unwraps_them_again() {
        let before = state("const a = (\n  <div>\n    [<b>x</b>\n    <i>y</i>]\n  </div>\n);");
        let commented = tsx(&before, comment_block).expect("the lines are commented");
        assert!(commented.text().contains("{/* <b>x</b>\n    <i>y</i> */}"));
        let back = tsx(&commented, comment_block).expect("the comment comes out");
        assert_eq!(back.text(), "const a = (\n  <div>\n    <b>x</b>\n    <i>y</i>\n  </div>\n);");
    }

    type Command = fn(&EditState, CommentTokensAt<'_>, &SyntaxTree) -> Option<Edit>;
    type Tokens = fn(u32) -> Vec<CommentTokens>;

    /// Runs `command` with the token contexts of `language` (TSX contexts follow the text).
    fn run_in(language: &str, before: &EditState, command: Command) -> Option<EditState> {
        let tokens: Tokens = match language {
            "HTML" | "Vue" => markup,
            "CSS" => style_sheet,
            "Python" => python,
            "JavaScript" | "Rust" => script,
            _ => return tsx(before, command),
        };
        command(before, &tokens, &SyntaxTree::default()).map(|edit| run(before, Some(edit)))
    }

    const ACTIONS: [(&str, Command); 2] = [("line", line_command), ("block", comment_block)];

    #[test]
    fn action_comments_round_trip_in_language_input() {
        let cases = [
            ("TSX", "const a = <div>\n  [<b>x</b>\n  <i>y</i>]\n</div>;"),
            ("TSX", "const a = <div>{\n  [value +\n  other]\n}</div>;"),
            ("TSX", "[const a = 1;\nconst b = 2;]"),
            ("HTML", "<section>\n  [<b>x</b>\n  <i>y</i>]\n</section>"),
            ("CSS", "[a { color: red; }\nb { color: blue; }]"),
            ("Vue", "<template>\n[<b>x</b>\n<i>y</i>]\n</template>"),
        ];
        for (action, command) in ACTIONS {
            for (language, input) in cases {
                let before = state(input);
                let once = run_in(language, &before, command).expect("the command applies");
                assert_ne!(once.text(), before.text(), "{action} {language}: {input}");
                let twice = run_in(language, &once, command).expect("the command applies again");
                assert_eq!(twice.text(), before.text(), "{action} {language}: {input}");
            }
        }
    }

    #[test]
    fn action_comments_round_trip_with_multiple_ranges_in_language() {
        for (action, command) in ACTIONS {
            for language in ["TSX", "HTML", "CSS", "Vue"] {
                let css = "[a { color: red; }]\n[b { color: blue; }]";
                let mut body = if language == "CSS" { css } else { "[<b>x</b>]\n[<i>y</i>]" }.to_owned();
                if action == "line" {
                    body = body.replace('[', "|").replace(']', "");
                }
                let input = if language == "TSX" { format!("const a = <div>\n{body}\n</div>;") } else { body };
                let before = state(&input);
                let once = run_in(language, &before, command).expect("the command applies");
                let twice = run_in(language, &once, command).expect("the command applies again");
                assert_eq!(twice.text(), before.text(), "{action} {language}");
                assert_eq!(twice.selections.len(), 2, "{action} {language}");
            }
        }
    }

    #[test]
    fn action_comments_do_not_join_separate_comments_across_live_code_in_language() {
        let languages =
            [("CSS", "/*", "*/"), ("JavaScript", "/*", "*/"), ("HTML", "<!--", "-->"), ("TSX", "{/*", "*/}")];
        for (action, command) in ACTIONS {
            for (language, open, close) in languages {
                let whole = format!("[{open} a {close} b {open} c {close}]");
                let bodies = [whole, format!("{open} [a {close} b {open} c] {close}")];
                for body in bodies {
                    let input = if language == "TSX" { format!("const a = <div>\n{body}\n</div>;") } else { body };
                    let before = state(&input);
                    let mut after = run_in(language, &before, command).unwrap_or_else(|| before.clone());
                    if language == "JavaScript" && action == "line" {
                        after = run_in(language, &after, command).unwrap_or(after);
                    }
                    assert_eq!(after.text(), before.text(), "{action} {language}: {input}");
                }
            }
        }
    }

    #[test]
    fn keeps_a_line_only_languages_block_command_unavailable() {
        let before = state("[a = 1\nb = 2]");
        assert_eq!(comment_block(&before, &python, &SyntaxTree::default()), None);
        let commented = run_in("Python", &before, line_command).expect("the lines are commented");
        assert_ne!(commented.text(), before.text());
        let back = run_in("Python", &commented, line_command).expect("the comments come out");
        assert_eq!(back.text(), before.text());
    }

    #[test]
    fn uncomments_a_parser_confirmed_nested_block_comment() {
        let before = state("[/* a /* b */ c */]");
        let tree = SyntaxTree::new(vec![TreeNode::new("SourceFile", 0, 17), TreeNode::new("BlockComment", 0, 17)]);
        let after = run(&before, comment_block(&before, &script, &tree));
        assert_eq!(after.text(), "a /* b */ c");
        // Without the parser's word the closing token inside means two comments: nothing changes.
        assert_eq!(run(&before, comment_block(&before, &script, &SyntaxTree::default())).text(), before.text());
    }
}
