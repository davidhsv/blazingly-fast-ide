//! The caret-anchored chooser of Insert Live Template, Surround With and the postfix expression
//! popup: what it lists, and which request is showing. What a pick does belongs to whoever asked.

/// One row of the chooser.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChooserItem {
    pub key: String,
    pub label: String,
    pub detail: Option<String>,
    /// Matched against the filter in addition to the label.
    pub search: Option<String>,
    /// A section header row ("Live templates"); not selectable.
    pub section: bool,
    /// Typing this character picks the item (see [`ChooserRequest::mnemonics`]).
    pub mnemonic: Option<String>,
    /// Where the mnemonic is underlined in the label (UTF-16); without it the label gets an
    /// `M. ` prefix.
    pub mnemonic_index: Option<u32>,
}

impl ChooserItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self { key: key.into(), label: label.into(), ..Self::default() }
    }

    /// A section header row.
    pub fn section(key: &str, label: &str) -> Self {
        Self { section: true, ..Self::new(key, label) }
    }
}

/// A chooser to show at the caret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChooserRequest {
    pub title: String,
    pub items: Vec<ChooserItem>,
    /// Initial filter.
    pub filter: String,
    /// Typing filters the items.
    pub filterable: bool,
    /// Typed characters pick items by their mnemonic (instead of filtering).
    pub mnemonics: bool,
    /// Shown when nothing matches.
    pub empty_text: Option<String>,
}

impl ChooserRequest {
    /// The selectable item a typed character picks, whatever its case.
    pub fn item_for_mnemonic(&self, typed: &str) -> Option<&ChooserItem> {
        let typed = typed.to_uppercase();
        let picks = |item: &&ChooserItem| {
            !item.section && item.mnemonic.as_deref().is_some_and(|mnemonic| mnemonic.to_uppercase() == typed)
        };
        self.items.iter().find(picks).filter(|_| self.mnemonics)
    }
}

/// The chooser on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chooser {
    request: Option<ChooserRequest>,
    /// Counts the requests shown, so that a view can tell a new one from the one it has.
    seq: u64,
}

impl Chooser {
    pub fn request(&self) -> Option<&ChooserRequest> {
        self.request.as_ref()
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Shows `request`. Returns the request it replaces, which its asker is to take as cancelled.
    pub fn show(&mut self, request: ChooserRequest) -> Option<ChooserRequest> {
        self.seq += 1;
        self.request.replace(request)
    }

    pub fn close(&mut self) {
        self.request = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a new request replaces the one showing, which is handed back as cancelled.
    #[test]
    fn a_new_request_cancels_the_one_showing() {
        let request = |title: &str| ChooserRequest { title: title.to_owned(), ..ChooserRequest::default() };
        let mut chooser = Chooser::default();
        assert_eq!(chooser.show(request("a")), None);
        assert_eq!(chooser.show(request("b")).map(|cancelled| cancelled.title).as_deref(), Some("a"));
        assert_eq!((chooser.seq(), chooser.request().map(|r| r.title.as_str())), (2, Some("b")));
        chooser.close();
        assert_eq!(chooser.request(), None);
    }

    // Not from a spec: a typed character picks by mnemonic only in a mnemonic chooser, and never
    // a section row.
    #[test]
    fn mnemonics_pick_selectable_items() {
        let item = |key: &str, mnemonic: &str| {
            let mnemonic = Some(mnemonic.to_owned());
            ChooserItem { mnemonic, ..ChooserItem::new(key, key) }
        };
        let header = ChooserItem { mnemonic: Some("T".to_owned()), ..ChooserItem::section("s", "Templates") };
        let mut request =
            ChooserRequest { items: vec![item("if", "1"), header, item("tag", "T")], ..ChooserRequest::default() };
        assert_eq!(request.item_for_mnemonic("t"), None);
        request.mnemonics = true;
        assert_eq!(request.item_for_mnemonic("t").map(|item| item.key.as_str()), Some("tag"));
        assert_eq!(request.item_for_mnemonic("1").map(|item| item.key.as_str()), Some("if"));
        assert_eq!(request.item_for_mnemonic("x"), None);
    }
}
