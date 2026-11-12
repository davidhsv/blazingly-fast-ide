//! The table editor's WHERE / ORDER BY fields: SQL completion of the table's columns (and, for
//! WHERE, keywords and functions) as if the text followed `SELECT * FROM t WHERE`, and the smart
//! keys that pair quotes and brackets.

use super::sql::analyze::{Analysis, analyze};
use super::sql::complete::{
    CandidateKind, CompletionInput, SqlCandidate, quote_ident, should_auto_complete_sql, sql_candidates,
};
use super::sql::lexer::{in_lexed_literal, len16, lex, slice16, utf16};
use ide_db::{CompletionColumn, CompletionModel, CompletionTable, DbDriver, DbTableKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Where,
    Order,
}

/// The editor preferences the smart keys follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmartKeys {
    pub pair_quotes: bool,
    pub pair_brackets: bool,
}

/// A WHERE / ORDER BY field over one table.
#[derive(Clone, Debug)]
pub struct SqlFilterInput {
    pub kind: FilterKind,
    pub driver: DbDriver,
    pub value: String,
    /// Caret, UTF-16.
    pub caret: usize,
    /// The completion popup is open.
    pub open: bool,
    pub active: usize,
    prefix: String,
    model: CompletionModel,
}

/// What a typed key did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyResult {
    /// The field handled it.
    Handled,
    /// Default handling (the character is typed).
    Default,
}

const MAX_OPTIONS: usize = 30;

impl SqlFilterInput {
    pub fn new(kind: FilterKind, driver: DbDriver, schema: &str, table: &str, columns: &[(String, String)]) -> Self {
        let clause = if kind == FilterKind::Where { "WHERE" } else { "ORDER BY" };
        let prefix = format!("SELECT * FROM {}.{} {clause} ", quote_ident(schema, driver), quote_ident(table, driver));
        let model = CompletionModel {
            default_schema: schema.to_string(),
            foreign_keys: Vec::new(),
            tables: vec![CompletionTable {
                schema: schema.to_string(),
                name: table.to_string(),
                kind: DbTableKind::Table,
                columns: columns
                    .iter()
                    .map(|(name, data_type)| CompletionColumn {
                        name: name.clone(),
                        data_type: data_type.clone(),
                        nullable: true,
                        primary_key: false,
                        has_default: false,
                    })
                    .collect(),
            }],
        };
        Self { kind, driver, value: String::new(), caret: 0, open: false, active: 0, prefix, model }
    }

    fn analysis(&self, value: &str, caret: usize) -> Analysis {
        analyze(&format!("{}{value}", self.prefix), len16(&self.prefix) + caret, self.driver)
    }

    /// The text changed (typing, paste): completion opens where it would in a console.
    pub fn change(&mut self, value: &str, caret: usize) {
        self.value = value.to_string();
        self.caret = caret;
        self.active = 0;
        self.open = should_auto_complete_sql(&self.analysis(value, caret));
    }

    /// Start of the word being completed (UTF-16 offset into the value).
    fn word_from(&self) -> usize {
        self.analysis(&self.value, self.caret).prefix.from.saturating_sub(len16(&self.prefix))
    }

    /// The popup's items: the table's columns (WHERE: also keywords and functions; ORDER BY: also
    /// ASC / DESC) starting with the typed word, without repeated labels, at most 30.
    pub fn options(&self) -> Vec<SqlCandidate> {
        let analysis = self.analysis(&self.value, self.caret);
        let units = utf16(&self.value);
        let typed = slice16(&units, self.word_from(), self.caret).to_lowercase();
        let mut candidates: Vec<SqlCandidate> = sql_candidates(CompletionInput {
            model: Some(&self.model),
            analysis: &analysis,
            driver: self.driver,
            source_name: None,
        })
        .into_iter()
        .filter(|c| {
            c.kind == CandidateKind::Column
                || (self.kind == FilterKind::Where
                    && matches!(c.kind, CandidateKind::Keyword | CandidateKind::Function))
        })
        .collect();
        if self.kind == FilterKind::Order {
            candidates.extend(["ASC", "DESC"].map(|label| SqlCandidate::new(label, CandidateKind::Keyword, 0)));
        }
        let mut out: Vec<SqlCandidate> = Vec::new();
        for c in candidates {
            if c.label.to_lowercase().starts_with(&typed) && !out.iter().any(|o| o.label == c.label) {
                out.push(c);
            }
        }
        out.truncate(MAX_OPTIONS);
        out
    }

    /// Accepting an option: the value with the word replaced (ORDER BY takes a column's bare
    /// label), the caret after it, the popup closed.
    pub fn choose(&mut self, item: &SqlCandidate) {
        let from = self.word_from();
        let replacement = if self.kind == FilterKind::Order && item.kind == CandidateKind::Column {
            item.label.as_str()
        } else {
            item.insert_text()
        };
        let units = utf16(&self.value);
        self.value = format!("{}{replacement}{}", slice16(&units, 0, from), slice16(&units, self.caret, units.len()));
        self.caret = from + len16(replacement);
        self.open = false;
    }

    /// ⌃Space.
    pub fn open_completion(&mut self) {
        self.open = true;
        self.active = 0;
    }

    /// Enter / Tab with the popup open: accepts the active option.
    pub fn accept(&mut self) -> bool {
        if !self.open {
            return false;
        }
        let options = self.options();
        let Some(item) = options.get(self.active.min(options.len().saturating_sub(1))).cloned() else { return false };
        self.choose(&item);
        true
    }

    /// ↑ / ↓ in the popup.
    pub fn move_active(&mut self, down: bool) {
        let n = self.options().len();
        if self.open && n > 0 {
            self.active = if down { (self.active + 1) % n } else { (self.active + n - 1) % n };
        }
    }

    /// A typed character with the selection `start..end`: steps over a closing quote / bracket
    /// already there, or inserts a pair (outside literals, before white space or a closer).
    pub fn type_char(&mut self, key: char, start: usize, end: usize, prefs: SmartKeys) -> KeyResult {
        let units = utf16(&self.value);
        let at = |i: usize| units.get(i).and_then(|&u| char::from_u32(u32::from(u)));
        let quote = key == '\'' || key == '"';
        if start == end && at(start) == Some(key) && (quote || ")]}".contains(key)) {
            self.caret = start + 1;
            self.open = false;
            return KeyResult::Handled;
        }
        let closer = match key {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '\'' => '\'',
            '"' => '"',
            _ => return KeyResult::Default,
        };
        let literal = in_lexed_literal(&lex(&self.value, self.driver), start).is_some();
        let before_ok = at(start).is_none_or(|c| c.is_whitespace() || ")]};,".contains(c));
        if start == end && (if quote { prefs.pair_quotes } else { prefs.pair_brackets }) && !literal && before_ok {
            self.value = format!("{}{key}{closer}{}", slice16(&units, 0, start), slice16(&units, end, units.len()));
            self.caret = start + 1;
            self.open = false;
            return KeyResult::Handled;
        }
        KeyResult::Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: SmartKeys = SmartKeys { pair_quotes: true, pair_brackets: true };

    fn field(kind: FilterKind, columns: &[(&str, &str)]) -> SqlFilterInput {
        let columns: Vec<(String, String)> = columns.iter().map(|&(n, t)| (n.to_string(), t.to_string())).collect();
        SqlFilterInput::new(kind, DbDriver::Sqlite, "main", "users", &columns)
    }

    #[test]
    fn suggests_the_current_table_columns_in() {
        for kind in [FilterKind::Where, FilterKind::Order] {
            let mut f = field(kind, &[("email", "text"), ("id", "integer")]);
            f.change("ema", 3);
            assert!(f.open, "{kind:?}");
            let labels: Vec<String> = f.options().into_iter().map(|o| o.label).collect();
            assert!(labels.iter().any(|l| l.eq_ignore_ascii_case("email")), "{kind:?}: {labels:?}");
            assert!(!labels.contains(&"id".to_string()));
            assert!(f.accept());
            assert_eq!(f.value, "email");
        }
    }

    #[test]
    fn pairs_without_opening_completion() {
        for (key, paired) in [('\'', "''"), ('"', "\"\""), ('(', "()")] {
            let mut f = field(FilterKind::Where, &[("email", "text")]);
            assert_eq!(f.type_char(key, 0, 0, ON), KeyResult::Handled);
            assert_eq!(f.value, paired);
            assert_eq!(f.caret, 1);
            assert!(!f.open);
        }
    }

    #[test]
    fn does_not_automatically_complete_after() {
        for character in [" ", "'", "\"", "("] {
            let mut f = field(FilterKind::Where, &[("email", "text")]);
            f.change(character, 1);
            assert!(!f.open, "{character:?}");
        }
    }
}
