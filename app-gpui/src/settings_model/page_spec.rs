//! The declarative description of a built-in settings page. Option labels double as the search
//! index, so a page is found by any of its option names without being shown.

use super::staging::Scope;

/// One control of a page.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionSpec {
    /// A checkbox writing a boolean to `key`.
    Toggle { key: &'static str, label: &'static str },
    /// A number field writing to `key`, kept within `min..=max`.
    Number { key: &'static str, label: &'static str, min: f64, max: f64, integer: bool, unit: Option<&'static str> },
    /// A choice writing one of the `(value, label)` options to `key`.
    Select { key: &'static str, label: &'static str, options: Vec<(&'static str, &'static str)> },
    /// A text field writing to `key`.
    Text { key: &'static str, label: &'static str },
    /// A control of its own; `label` and `keywords` are still indexed for the search.
    Custom { label: &'static str, keywords: Vec<&'static str> },
}

impl OptionSpec {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Toggle { label, .. }
            | Self::Number { label, .. }
            | Self::Select { label, .. }
            | Self::Text { label, .. }
            | Self::Custom { label, .. } => label,
        }
    }

    /// The setting the control writes; none for a control of its own.
    pub fn key(&self) -> Option<&'static str> {
        match self {
            Self::Toggle { key, .. } | Self::Number { key, .. } | Self::Select { key, .. } | Self::Text { key, .. } => {
                Some(key)
            }
            Self::Custom { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionSpec {
    pub title: Option<&'static str>,
    pub options: Vec<OptionSpec>,
}

/// What the control of a setting shows besides its label and value. A page names only the
/// details it needs, by the setting's key ([`PageSpec::detail`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptionDetail {
    /// A line of help drawn under the control.
    pub hint: Option<&'static str>,
    /// The grey text an empty text field shows.
    pub placeholder: Option<&'static str>,
    /// How much Up and Down change a number field; 1 when not given.
    pub step: Option<f64>,
    /// The checkbox setting this option depends on: it is indented below it, and dimmed and not
    /// editable while that checkbox is off.
    pub enabled_by: Option<&'static str>,
    /// A choice drawn as one radio button per row instead of a drop-down list.
    pub radio: bool,
    /// A checkbox that is ticked while its boolean setting is false ("Use tab character" writes
    /// `editor.insertSpaces`).
    pub invert: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageSpec {
    /// The dotted path defines nesting, e.g. `languages.servers`.
    pub id: &'static str,
    pub title: &'static str,
    /// Parent group title for top-level grouping, e.g. "Editor", "Tools".
    pub group: Option<&'static str>,
    /// Where the page sorts among its siblings; fractions slot a page between two others.
    pub order: f64,
    /// Extra words the settings search matches.
    pub keywords: Vec<&'static str>,
    /// Offers the Global / This project scope switch.
    pub project_scope: bool,
    /// The scope shown first with a project open (toolchains belong to the project, like IDEA's
    /// project SDK).
    pub default_scope: Option<Scope>,
    /// Pages sharing one scope choice (Code Style and its language pages: the scheme).
    pub scope_group: Option<&'static str>,
    /// The page picks its scope itself (Code Style's Scheme): no Global / This project switch.
    pub own_scope_switch: bool,
    pub sections: Vec<SectionSpec>,
    /// The details of the controls, by setting key.
    pub details: Vec<(&'static str, OptionDetail)>,
}

impl PageSpec {
    pub fn new(id: &'static str, title: &'static str, order: impl Into<f64>) -> Self {
        Self { id, title, order: order.into(), ..Self::default() }
    }

    pub fn group(mut self, group: &'static str) -> Self {
        self.group = Some(group);
        self
    }

    pub fn keywords(mut self, keywords: &[&'static str]) -> Self {
        self.keywords = keywords.to_vec();
        self
    }

    pub fn project_scope(mut self) -> Self {
        self.project_scope = true;
        self
    }

    /// Shows the project's values first when a project is open.
    pub fn project_first(mut self) -> Self {
        self.default_scope = Some(Scope::Project);
        self
    }

    pub fn own_scope_switch(mut self) -> Self {
        self.own_scope_switch = true;
        self
    }

    pub fn section(mut self, title: Option<&'static str>, options: Vec<OptionSpec>) -> Self {
        self.sections.push(SectionSpec { title, options });
        self
    }

    /// Gives the control of setting `key` `detail`.
    pub fn detail(mut self, key: &'static str, detail: OptionDetail) -> Self {
        self.details.push((key, detail));
        self
    }

    /// The details of the control of setting `key`; none when the page names none.
    pub fn detail_of(&self, key: &str) -> Option<&OptionDetail> {
        self.details.iter().find(|(own, _)| *own == key).map(|(_, detail)| detail)
    }
}

/// The search texts of a page: its keywords, section titles, option labels, and the keywords and
/// choices of its options.
pub fn spec_labels(spec: &PageSpec) -> Vec<&'static str> {
    let mut out = spec.keywords.clone();
    for section in &spec.sections {
        out.extend(section.title);
        for option in &section.options {
            out.push(option.label());
            match option {
                OptionSpec::Custom { keywords, .. } => out.extend(keywords),
                OptionSpec::Select { options, .. } => out.extend(options.iter().map(|(_, label)| *label)),
                _ => {}
            }
        }
    }
    out
}

/// The labels of a page's options (each is a Search Everywhere entry).
pub fn option_labels(spec: &PageSpec) -> Vec<&'static str> {
    let options = spec.sections.iter().flat_map(|section| &section.options);
    options.map(OptionSpec::label).filter(|label| !label.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the search texts of a page are its keywords, section titles, option labels
    // and the keywords and choices of its options; a control of its own writes no setting.
    #[test]
    fn indexes_titles_labels_keywords_and_choices() {
        let spec = PageSpec::new("demo", "Demo", 5).keywords(&["kw"]).section(
            Some("Section"),
            vec![
                OptionSpec::Toggle { key: "a.b", label: "Toggle" },
                OptionSpec::Select { key: "a.c", label: "Choice", options: vec![("x", "First"), ("y", "Second")] },
                OptionSpec::Custom { label: "Editor", keywords: vec!["Field"] },
                OptionSpec::Text { key: "a.d", label: " " },
            ],
        );
        let labels = ["kw", "Section", "Toggle", "Choice", "First", "Second", "Editor", "Field", " "];
        assert_eq!(spec_labels(&spec), labels);
        assert_eq!(option_labels(&spec), ["Toggle", "Choice", "Editor"]);
        let keys: Vec<Option<&str>> = spec.sections[0].options.iter().map(OptionSpec::key).collect();
        assert_eq!(keys, [Some("a.b"), Some("a.c"), None, Some("a.d")]);
        assert_eq!((spec.order, spec.project_scope, spec.default_scope), (5.0, false, None));
    }

    // Not from a spec: a control's details are found by its setting's key; a key without any has
    // none.
    #[test]
    fn finds_the_details_of_a_control_by_its_key() {
        let hint = OptionDetail { hint: Some("Help"), step: Some(64.0), ..OptionDetail::default() };
        let spec = PageSpec::new("demo", "Demo", 5)
            .section(None, vec![OptionSpec::Toggle { key: "a.b", label: "Toggle" }])
            .detail("a.b", hint.clone());
        assert_eq!(spec.detail_of("a.b"), Some(&hint));
        assert_eq!(spec.detail_of("a.c"), None);
    }
}
