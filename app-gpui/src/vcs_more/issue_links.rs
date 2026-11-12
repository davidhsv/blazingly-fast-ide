//! Settings › Version Control › Issue Navigation: issue ids in commit messages become links.

use ide_core::workflow_settings::IssueLink;
use regex::{Captures, Regex};

/// A part of a commit message: plain text, or an issue id with the page it links to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueSegment {
    Text(String),
    Link { text: String, url: String },
}

/// `link` with `$0` replaced by the whole match and `$1`…`$9` by its groups; a group that did not
/// take part is empty.
fn url_of(link: &str, found: &Captures) -> String {
    let mut url = String::new();
    let mut chars = link.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek().and_then(|next| next.to_digit(10)).filter(|_| c == '$') {
            Some(group) => {
                url.push_str(found.get(group as usize).map_or("", |group| group.as_str()));
                chars.next();
            }
            None => url.push(c),
        }
    }
    url
}

/// Splits `text` into plain parts and issue links. The earliest match wins, the longer one of two
/// that start together, and links do not overlap. Patterns that do not compile are left out: the
/// settings page flags them.
pub fn linkify_issues(text: &str, links: &[IssueLink]) -> Vec<IssueSegment> {
    let mut matches: Vec<(usize, usize, String)> = Vec::new();
    for link in links {
        let Ok(pattern) = Regex::new(&link.pattern) else { continue };
        for found in pattern.captures_iter(text) {
            let whole = found.get(0).filter(|whole| !whole.is_empty());
            matches.extend(whole.map(|whole| (whole.start(), whole.end(), url_of(&link.link, &found))));
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out = Vec::new();
    let mut pos = 0;
    for (start, end, url) in matches {
        if start < pos {
            continue;
        }
        if start > pos {
            out.push(IssueSegment::Text(text[pos..start].to_owned()));
        }
        out.push(IssueSegment::Link { text: text[start..end].to_owned(), url });
        pos = end;
    }
    if pos < text.len() {
        out.push(IssueSegment::Text(text[pos..].to_owned()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(pattern: &str, link: &str) -> IssueLink {
        IssueLink { pattern: pattern.to_owned(), link: link.to_owned() }
    }

    fn text(text: &str) -> IssueSegment {
        IssueSegment::Text(text.to_owned())
    }

    fn issue(text: &str, url: &str) -> IssueSegment {
        IssueSegment::Link { text: text.to_owned(), url: url.to_owned() }
    }

    // src/features/vcs/__tests__/workflow.test.ts › issue navigation › links every match with $0 / $n substitution
    #[test]
    fn links_every_match_with_whole_match_and_group_substitution() {
        let links = [
            link(r"[A-Z]+-\d+", "https://jira/browse/$0"),
            link(r"#(\d+)", "https://gh/issues/$1"),
            link("(", "broken"),
        ];
        let expected = [
            text("Fix "),
            issue("OPENIDEA-12", "https://jira/browse/OPENIDEA-12"),
            text(" and "),
            issue("#7", "https://gh/issues/7"),
            text("."),
        ];
        assert_eq!(linkify_issues("Fix OPENIDEA-12 and #7.", &links), expected);
    }
}
