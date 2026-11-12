//! Custom folding regions: `//region Name` … `//endregion`, `#region` (C#, Python…) and
//! `<editor-fold desc="Name">` … `</editor-fold>`, written in the language's line or block
//! comments. The backend folds them; this names their placeholders and lists them for
//! Navigate › Custom Folding.

use super::{CommentTokens, CommentTokensAt, EditState, Line};

/// What a region marker line does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionMarker {
    /// Starts a region; the name is empty when the marker gives none.
    Start { name: String },
    /// Ends the region started last.
    End,
}

/// A custom region with both its markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomRegion {
    pub name: String,
    /// 0-based line of the start marker.
    pub start_line: u32,
    /// 0-based line of the end marker.
    pub end_line: u32,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A keyword that ends where `rest` starts is a whole word: `rest` does not go on with a letter,
/// a digit or `_`. Only ASCII counts, so a name in another script may follow the keyword directly
/// (`region要折叠的内容`).
fn at_word_end(rest: &str) -> bool {
    !rest.starts_with(is_word)
}

/// `text` without `prefix` at its start, compared without regard to ASCII case.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.split_at_checked(prefix.len()).filter(|(head, _)| head.eq_ignore_ascii_case(prefix)).map(|(_, tail)| tail)
}

fn contains_ignore_case(text: &str, word: &str) -> bool {
    text.as_bytes().windows(word.len()).any(|window| window.eq_ignore_ascii_case(word.as_bytes()))
}

/// The line may be a marker at all: a cheap test that spares most lines the parsing.
fn mentions_marker(line: &str) -> bool {
    contains_ignore_case(line, "region") || contains_ignore_case(line, "editor-fold")
}

/// `#region` / `#endregion` written as a directive of its own, whatever the comment tokens are.
fn is_directive(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|rest| {
        let rest = strip_prefix_ignore_case(rest, "end").unwrap_or(rest);
        strip_prefix_ignore_case(rest, "region").is_some_and(at_word_end)
    })
}

/// What a comment line says, without its delimiters (a directive: the line itself). `None` when
/// the line is not one whole comment in these tokens.
fn comment_body<'a>(text: &'a str, tokens: &CommentTokens) -> Option<&'a str> {
    if is_directive(text) {
        return Some(text);
    }
    if let Some((open, close)) = &tokens.block
        && let Some(rest) = text.strip_prefix(open.as_str())
    {
        let body = rest.trim_end().strip_suffix(close.as_str())?;
        return Some(if open == "/*" { body.trim_start_matches('*') } else { body });
    }
    let open = tokens.line.as_deref().filter(|open| !open.is_empty())?;
    let body = text.strip_prefix(open)?;
    Some(match open {
        "//" => body.trim_start_matches('/'),
        ";" => body.trim_start_matches(';'),
        _ => body,
    })
}

/// The value of the `desc="…"` attribute among the attributes of an `<editor-fold>` tag.
fn description(attributes: &str) -> Option<&str> {
    let lower = attributes.to_ascii_lowercase();
    lower.match_indices("desc").find_map(|(at, word)| {
        if lower[..at].ends_with(is_word) {
            return None;
        }
        let value = attributes[at + word.len()..].trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
        value.split_once('"').map(|(value, _)| value)
    })
}

/// The marker a comment's body is, if it is one.
fn marker_in(body: &str) -> Option<RegionMarker> {
    let body = body.trim();
    if body.contains(['\r', '\n']) {
        return None;
    }
    let directive = body.strip_prefix('#').unwrap_or(body);
    let closes_fold = strip_prefix_ignore_case(body, "</editor-fold")
        .and_then(|tail| tail.strip_suffix('>'))
        .is_some_and(|tail| tail.trim().is_empty());
    if closes_fold || strip_prefix_ignore_case(directive, "endregion").is_some_and(at_word_end) {
        return Some(RegionMarker::End);
    }
    if let Some(tail) = strip_prefix_ignore_case(body, "<editor-fold").filter(|tail| at_word_end(tail))
        && let Some((attributes, _)) = tail.split_once('>')
    {
        let name = description(attributes).unwrap_or_default().trim().to_owned();
        return Some(RegionMarker::Start { name });
    }
    let name = strip_prefix_ignore_case(directive, "region").filter(|tail| at_word_end(tail))?.trim();
    let name = name.strip_prefix('"').and_then(|name| name.strip_suffix('"')).unwrap_or(name).trim();
    Some(RegionMarker::Start { name: name.to_owned() })
}

/// The region marker on a line of text, if it is one.
pub fn region_marker(line: &str, tokens: &CommentTokens) -> Option<RegionMarker> {
    if !mentions_marker(line) {
        return None;
    }
    marker_in(comment_body(line.trim(), tokens)?)
}

/// The marker on `line`, read with the comment tokens in force where the line starts.
pub fn region_marker_at(line: &Line, comments: CommentTokensAt<'_>) -> Option<RegionMarker> {
    if !mentions_marker(&line.text) {
        return None;
    }
    let tokens = comments(line.from).into_iter().next().unwrap_or_default();
    region_marker(&line.text, &tokens)
}

/// Every region with both its markers, in document order of the start markers. An end marker
/// closes the region started last; one without a start is ignored.
pub fn custom_regions(state: &EditState, comments: CommentTokensAt<'_>) -> Vec<CustomRegion> {
    let mut regions: Vec<(String, u32, Option<u32>)> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for number in 0..state.line_count() {
        match region_marker_at(&state.line(number), comments) {
            Some(RegionMarker::Start { name }) => {
                open.push(regions.len());
                regions.push((name, number, None));
            }
            Some(RegionMarker::End) => {
                if let Some(index) = open.pop() {
                    regions[index].2 = Some(number);
                }
            }
            None => {}
        }
    }
    regions
        .into_iter()
        .filter_map(|(name, start_line, end)| end.map(|end_line| CustomRegion { name, start_line, end_line }))
        .collect()
}

/// The name of the region whose start marker begins at `pos` (where its fold starts), for the
/// fold's placeholder. `None` for a region without a name and anywhere else.
pub fn region_name_at(state: &EditState, comments: CommentTokensAt<'_>, pos: u32) -> Option<String> {
    let line = state.line_at(pos);
    if !state.slice(line.from, pos).trim().is_empty() {
        return None;
    }
    match region_marker_at(&line, comments)? {
        RegionMarker::Start { name } if !name.is_empty() => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::folding::{BackendFold, folds_from_ranges};
    use super::*;
    use ide_core::LanguageId;
    use ide_language::{FoldKind, FoldRange, LanguageRegistry};
    use serde_json::Value;
    use std::path::Path;

    fn start(name: &str) -> Option<RegionMarker> {
        Some(RegionMarker::Start { name: name.to_owned() })
    }

    fn region(name: &str, start_line: u32, end_line: u32) -> CustomRegion {
        CustomRegion { name: name.to_owned(), start_line, end_line }
    }

    /// A backend fold range over the first three lines.
    fn three_lines() -> [FoldRange; 1] {
        [FoldRange { start_line: 0, end_line: 2, kind: FoldKind::Region }]
    }

    fn languages() -> LanguageRegistry {
        LanguageRegistry::load(None).expect("the built-in languages load")
    }

    /// The comment tokens of a built-in language.
    fn tokens_of(registry: &LanguageRegistry, id: &LanguageId) -> CommentTokens {
        let config = registry.get(id).expect("the language is built in");
        CommentTokens { line: config.line_comment, block: config.block_comment }
    }

    fn list<'a>(fixture: &'a Value, key: &str) -> &'a [Value] {
        fixture[key].as_array().expect("the fixture holds a list")
    }

    fn text<'a>(value: &'a Value, key: &str) -> &'a str {
        value[key].as_str().expect("the fixture holds a string")
    }

    #[test]
    fn custom_regions_in_each_language_use_its_comment_tokens_with_optional_spaces() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../../fixtures/folding/regions.json")).expect("the fixture is JSON");
        let registry = languages();
        for language in list(&fixture, "languages") {
            let id = language.as_str().expect("a language id");
            let tokens = tokens_of(&registry, &LanguageId::new(id));
            let at = |_: u32| vec![tokens.clone()];
            let mut delimiters: Vec<(&str, &str)> = Vec::new();
            if let Some(line) = &tokens.line {
                delimiters.push((line.as_str(), ""));
            }
            if let Some((open, close)) = &tokens.block {
                delimiters.push((open.as_str(), close.as_str()));
            }
            for (open, close) in delimiters {
                for space in ["", " ", "  "] {
                    // A delimiter that ends in a letter needs a space before what follows it.
                    if space.is_empty() && open.ends_with(|c: char| c.is_ascii_alphanumeric()) {
                        continue;
                    }
                    let wrap = |body: &str| format!("  {open}{space}{body}{space}{close}");
                    for marker in list(&fixture, "markers") {
                        let (begin, end) = (wrap(text(marker, "start")), wrap(text(marker, "end")));
                        let name = text(marker, "name");
                        assert_eq!(region_marker(&begin, &tokens), start(name), "{id}: {begin}");
                        assert_eq!(region_marker(&end, &tokens), Some(RegionMarker::End), "{id}: {end}");

                        let state = EditState::new(&format!("{begin}\n\n{end}\n"));
                        assert_eq!(custom_regions(&state, &at), [region(name, 0, 2)], "{id}: {begin}");
                        let named = (!name.is_empty()).then_some(name);
                        assert_eq!(region_name_at(&state, &at, 2).as_deref(), named, "{id}: {begin}");
                        assert_eq!(region_name_at(&state, &at, 5), None, "{id}: {begin}");
                        let fold = BackendFold { from: 2, to: state.line(2).to, kind: FoldKind::Region, custom: true };
                        assert_eq!(folds_from_ranges(&state, &three_lines(), &at), [fold], "{id}: {begin}");

                        let nested = format!("{end}\n\n{begin}\n{begin}\n\n{end}\n{end}\n{begin}\n");
                        let regions = custom_regions(&EditState::new(&nested), &at);
                        assert_eq!(regions, [region(name, 2, 6), region(name, 3, 5)], "{id}: {nested}");
                    }
                    for invalid in list(&fixture, "invalid") {
                        let line = wrap(invalid.as_str().expect("an invalid marker"));
                        assert_eq!(region_marker(&line, &tokens), None, "{id}: {line}");
                    }
                }
            }
        }
    }

    #[test]
    fn region_marker_preserves_markers_after_extra_comment_characters() {
        let tokens = CommentTokens::new(Some("//"), Some(("/*", "*/")));
        for line in ["///region Docs", "/**region Docs*/", "// #region Docs"] {
            assert_eq!(region_marker(line, &tokens), start("Docs"), "{line}");
        }
    }

    #[test]
    fn region_marker_rejects_what_is_not_a_comment_in_xml() {
        let tokens = CommentTokens::new(None, Some(("<!--", "-->")));
        for line in ["const region = 1;", "region Name", "//region Wrong language", "<!--region Missing close"] {
            assert_eq!(region_marker(line, &tokens), None, "{line}");
        }
    }

    #[test]
    fn preserves_c_sharp_region_directives_and_their_names() {
        let tokens = tokens_of(&languages(), &LanguageId::new("csharp"));
        let at = |_: u32| vec![tokens.clone()];
        let state = EditState::new("#region \"Fields\"\nint a;\n#endregion");
        assert_eq!(custom_regions(&state, &at), [region("Fields", 0, 2)]);
        assert_eq!(region_name_at(&state, &at, 0).as_deref(), Some("Fields"));
        let folds = folds_from_ranges(&state, &three_lines(), &at);
        assert_eq!(folds.first().map(|fold| fold.custom), Some(true));
    }

    #[test]
    fn does_not_list_plain_text_as_a_region_without_comment_tokens() {
        let state = EditState::new("region Name\n\nendregion");
        assert_eq!(custom_regions(&state, &|_| Vec::new()), Vec::<CustomRegion>::new());
    }

    #[test]
    fn custom_folding_finds_the_markers_of_each_file_type() {
        let registry = languages();
        for extension in ["xml", "svg", "html", "vue", "css", "java", "py", "ts"] {
            let language = registry.detect(Path::new(&format!("/project/file.{extension}")), None);
            let tokens = tokens_of(&registry, &language);
            let (open, close) = match (&tokens.line, &tokens.block) {
                (Some(line), _) => (line.as_str(), ""),
                (None, Some((open, close))) => (open.as_str(), close.as_str()),
                (None, None) => panic!("{extension} files have no comment tokens"),
            };
            let state = EditState::new(&format!("{open}region Name{close}\ncontent\n{open}endregion{close}\n"));
            assert_eq!(custom_regions(&state, &|_| vec![tokens.clone()]), [region("Name", 0, 2)], "{extension}");
        }
    }
}
