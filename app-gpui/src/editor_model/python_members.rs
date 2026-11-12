//! Python's Implement Abstract Methods (Python servers offer no such code action): the class at
//! the caret, its methods, its bases (resolved by the caller with Go to Declaration), the
//! abstract methods of those it lacks, and the stubs written at the end of its body. Lines are
//! 0-based indices into the document's lines.

use regex::Regex;
use std::sync::LazyLock;

static CLASS_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)class\s+\w+\s*\((.*)\)\s*:").expect("a valid pattern"));
static CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*class\s").expect("a valid pattern"));
static DEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:async\s+)?def\s+(\w+)").expect("a valid pattern"));
static SIGNATURE_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*(#.*)?$|:\s*\.\.\.\s*$|:\s*\S").expect("a valid pattern"));
static SIGNATURE_BODY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*(\.\.\.|pass)?\s*(#.*)?$").expect("a valid pattern"));
static ABSTRACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@(abc\.)?abstract(method|property)\b").expect("a valid pattern"));
static ABSTRACT_METHOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@(abc\.)?abstractmethod\b").expect("a valid pattern"));
static ABSTRACT_PROPERTY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@(abc\.)?abstractproperty\b").expect("a valid pattern"));
static BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*([A-Za-z_][\w.]*)\s*(?:\[|$)").expect("a valid pattern"));
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(pass|\.\.\.)\s*$").expect("a valid pattern"));

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The header line of the class enclosing `line` (or on it), when that class has bases.
pub fn enclosing_class<S: AsRef<str>>(lines: &[S], line: usize) -> Option<usize> {
    let mut limit = usize::MAX;
    for (n, text) in lines.iter().enumerate().take(line + 1).rev() {
        let text = text.as_ref();
        if text.trim().is_empty() {
            continue;
        }
        let indent = indent_of(text);
        if n != line && indent >= limit {
            continue;
        }
        if CLASS.is_match(text) {
            return CLASS_HEADER.is_match(text).then_some(n);
        }
        limit = limit.min(indent);
        if indent == 0 && n != line {
            return None;
        }
    }
    None
}

/// The last line of the body of the class headed at `header`. Lines inside a triple-quoted
/// string belong to it, however indented.
pub fn body_end<S: AsRef<str>>(lines: &[S], header: usize) -> usize {
    let indent = indent_of(lines[header].as_ref());
    let mut last = header;
    let mut quote: Option<&str> = None;
    for (n, text) in lines.iter().enumerate().skip(header + 1) {
        let text = text.as_ref();
        let in_string = quote.is_some();
        let mut rest = text;
        while let Some(at) = rest.find(r#"""""#).into_iter().chain(rest.find("'''")).min() {
            let found = &rest[at..at + 3];
            quote = match quote {
                None => Some(if found == r#"""""# { r#"""""# } else { "'''" }),
                Some(open) if open == found => None,
                open => open,
            };
            rest = &rest[at + 3..];
        }
        if in_string {
            last = n;
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }
        if indent_of(text) <= indent {
            break;
        }
        last = n;
    }
    last
}

/// A method declared in a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonMember {
    pub name: String,
    pub is_abstract: bool,
    /// Its decorators (without `@abstractmethod`; `@abstractproperty` as `@property`) and its
    /// signature, dedented.
    pub lines: Vec<String>,
}

fn depth_change(part: &str) -> i32 {
    part.chars()
        .map(|c| match c {
            '(' | '[' => 1,
            ')' | ']' => -1,
            _ => 0,
        })
        .sum()
}

/// The methods declared directly in the class headed at `header`.
pub fn class_members<S: AsRef<str>>(lines: &[S], header: usize) -> Vec<PythonMember> {
    let end = body_end(lines, header);
    let mut out = Vec::new();
    let mut member_indent: Option<usize> = None;
    let mut decorators: Vec<String> = Vec::new();
    for (n, text) in lines.iter().enumerate().take(end + 1).skip(header + 1) {
        let text = text.as_ref();
        if text.trim().is_empty() {
            continue;
        }
        let indent = indent_of(text);
        let member = *member_indent.get_or_insert(indent);
        if indent != member {
            continue;
        }
        let trimmed = text.trim();
        if trimmed.starts_with('@') {
            decorators.push(trimmed.to_owned());
            continue;
        }
        if let Some(def) = DEF.captures(text) {
            // The signature may span lines: up to the `:` closing it.
            let mut signature: Vec<String> = Vec::new();
            let mut depth = 0;
            for part in &lines[n..=end] {
                let part = part.as_ref();
                signature.push(part[member.min(indent_of(part))..].to_owned());
                depth += depth_change(part);
                if depth <= 0 && SIGNATURE_END.is_match(part) {
                    break;
                }
            }
            // `def f(self) -> int: ...` keeps only the signature.
            if let Some(last) = signature.last_mut() {
                *last = SIGNATURE_BODY.replace(last, ":").into_owned();
            }
            let is_abstract = decorators.iter().any(|decorator| ABSTRACT.is_match(decorator));
            let mut member_lines: Vec<String> = decorators
                .iter()
                .filter(|decorator| !ABSTRACT_METHOD.is_match(decorator))
                .map(|decorator| ABSTRACT_PROPERTY.replace(decorator, "@property").into_owned())
                .collect();
            member_lines.extend(signature);
            out.push(PythonMember { name: def[1].to_owned(), is_abstract, lines: member_lines });
        }
        decorators.clear();
    }
    out
}

/// A base class of a header, and the UTF-16 column Go to Declaration resolves it at: the last
/// segment of a dotted name (`abc.ABC`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub name: String,
    pub col: u32,
}

pub fn bases(header: &str) -> Vec<Base> {
    let Some(captures) = CLASS_HEADER.captures(header) else { return Vec::new() };
    let Some(list) = captures.get(2) else { return Vec::new() };
    let mut out = Vec::new();
    let mut at = list.start();
    for segment in list.as_str().split(',') {
        if let Some(name) = BASE.captures(segment).and_then(|found| found.get(1))
            && name.as_str() != "object"
        {
            let start = at + name.start() + name.as_str().rfind('.').map_or(0, |dot| dot + 1);
            out.push(Base { name: name.as_str().to_owned(), col: header[..start].encode_utf16().count() as u32 });
        }
        at += segment.len() + 1;
    }
    out
}

/// The stubs of `members`, indented by `indent`, each with a `pass` body one `unit` deeper.
pub fn member_stubs(members: &[PythonMember], indent: &str, unit: &str) -> String {
    members
        .iter()
        .map(|member| {
            let mut lines: Vec<String> = member.lines.iter().map(|line| format!("{indent}{line}")).collect();
            lines.push(format!("{indent}{unit}pass"));
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Where the stubs go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StubEdit {
    /// The class has no body yet: after the end of the header line.
    AfterLine { line: usize, text: String },
    /// A body of only `pass` / `...`: lines `from..=to` are replaced.
    ReplaceLines { from: usize, to: usize, text: String },
}

/// The edit adding the abstract methods `missing` (those of the bases the class lacks) to the
/// class headed at `header`; `None` when there are none ("No abstract methods to implement").
pub fn implement_abstract<S: AsRef<str>>(
    lines: &[S],
    header: usize,
    abstract_members: &[PythonMember],
    unit: &str,
) -> Option<StubEdit> {
    let own: Vec<String> = class_members(lines, header).into_iter().map(|member| member.name).collect();
    let missing: Vec<PythonMember> =
        abstract_members.iter().filter(|member| !own.contains(&member.name)).cloned().collect();
    if missing.is_empty() {
        return None;
    }
    let end = body_end(lines, header);
    let body: Vec<usize> = (header + 1..=end).filter(|n| !lines[*n].as_ref().trim().is_empty()).collect();
    let header_text = lines[header].as_ref();
    let indent = match body.first() {
        Some(first) => {
            let first = lines[*first].as_ref();
            first[..indent_of(first)].to_owned()
        }
        None => format!("{}{unit}", &header_text[..indent_of(header_text)]),
    };
    let stubs = member_stubs(&missing, &indent, unit);
    Some(match body.as_slice() {
        [] => StubEdit::AfterLine { line: header, text: format!("\n{stubs}") },
        [only] if PLACEHOLDER.is_match(lines[*only].as_ref()) => {
            StubEdit::ReplaceLines { from: *only, to: end, text: stubs }
        }
        _ => StubEdit::AfterLine { line: end, text: format!("\n\n{stubs}") },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"from abc import ABC, abstractmethod


class Shape(ABC):
    @property
    @abstractmethod
    def name(self) -> str: ...

    @abstractmethod
    def area(
        self, scale: float = 1.0
    ) -> float:
        """Area."""

    def describe(self) -> str:
        return self.name


class Square(Shape):
    def __init__(self, side: float):
        self.side = side
"#;

    fn src() -> Vec<&'static str> {
        SRC.split('\n').collect()
    }

    #[test]
    fn finds_the_class_enclosing_a_line() {
        let lines = src();
        assert_eq!(enclosing_class(&lines, 3), Some(3));
        assert_eq!(enclosing_class(&lines, 12), Some(3));
        assert_eq!(enclosing_class(&lines, 20), Some(18));
        assert_eq!(enclosing_class(&lines, 0), None);
    }

    #[test]
    fn lists_methods_with_their_decorators_and_signatures() {
        let members = class_members(&src(), 3);
        let names: Vec<(&str, bool)> =
            members.iter().map(|member| (member.name.as_str(), member.is_abstract)).collect();
        assert_eq!(names, [("name", true), ("area", true), ("describe", false)]);
        assert_eq!(members[0].lines, ["@property", "def name(self) -> str:"]);
        assert_eq!(members[1].lines, ["def area(", "    self, scale: float = 1.0", ") -> float:"]);
        assert_eq!(
            bases("class Shape(abc.ABC, Generic[T], metaclass=ABCMeta):"),
            [Base { name: "abc.ABC".into(), col: 16 }, Base { name: "Generic".into(), col: 21 },]
        );
    }

    #[test]
    fn stops_at_a_nested_class_without_bases() {
        let nested = ["class Outer(Base):", "    class Inner:", "        x = 1", ""];
        assert_eq!(enclosing_class(&nested, 2), None);
    }

    #[test]
    fn keeps_a_body_going_through_an_unindented_string() {
        let lines = [
            "class A(ABC):",
            "    @abc.abstractproperty",
            "    def doc(self):",
            r#"        return """"#,
            "text",
            r#"""""#,
            "    @abstractmethod",
            "    def run(self): ...",
            "x = 1",
        ];
        let members = class_members(&lines, 0);
        let signatures: Vec<Vec<String>> = members.iter().map(|member| member.lines.clone()).collect();
        assert_eq!(signatures, vec![vec!["@property", "def doc(self):"], vec!["def run(self):"]]);
        assert!(members.iter().all(|member| member.is_abstract));
    }

    #[test]
    fn writes_stubs_at_the_bodys_indentation() {
        let members = class_members(&src(), 3);
        let stubs = member_stubs(std::slice::from_ref(&members[0]), "    ", "    ");
        assert_eq!(stubs, "    @property\n    def name(self) -> str:\n        pass");
        let square = ["class Square(Shape):", "    pass"];
        let edit = implement_abstract(&square, 0, &members[..2], "    ");
        let Some(StubEdit::ReplaceLines { from: 1, to: 1, text }) = edit else { panic!("replaces the placeholder") };
        assert!(text.starts_with("    @property\n    def name(self) -> str:\n        pass\n\n    def area("));
    }
}
