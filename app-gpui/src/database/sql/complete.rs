//! SQL completion candidates: what fits the caret context from the analysed statement and the data
//! source's schema model — tables after FROM / JOIN (with ready-made join clauses from foreign
//! keys), columns of the tables in scope with their owner and type, join conditions after ON,
//! dialect functions and the keywords that may follow.

use super::analyze::{Analysis, Clause, RefKind, SqlContext, TableRef};
use super::dialect::{
    data_types, functions, is_reserved, join_keywords, names_eq, names_eq_opt, predicate_keywords, statement_starters,
};
use ide_db::{CompletionColumn, CompletionModel, CompletionTable, DbDriver, DbTableKind};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    Table,
    View,
    Column,
    /// Several columns at once (`id, email, name`).
    Columns,
    Keyword,
    Function,
    Schema,
    Alias,
    Join,
    Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlCandidate {
    /// Filter / display text.
    pub label: String,
    pub kind: CandidateKind,
    /// Grey text after the label: `(o)`, `(main)`, a signature.
    pub tail: Option<String>,
    /// Right-aligned: column type, data source.
    pub type_: Option<String>,
    /// Text inserted instead of `label`.
    pub insert: Option<String>,
    /// Inserted as a snippet: `${}` marks the caret.
    pub snippet: Option<String>,
    /// Ranking within the popup (higher first).
    pub boost: i32,
    /// Also append `FROM <this>` to the statement (column picked before any FROM).
    pub add_from: Option<String>,
    /// Re-open completion after inserting (`schema.`).
    pub retrigger: bool,
    /// Column decorations.
    pub primary_key: bool,
    pub foreign_key: bool,
    /// Side-panel documentation (plain text).
    pub doc: Option<String>,
}

impl SqlCandidate {
    pub fn new(label: impl Into<String>, kind: CandidateKind, boost: i32) -> Self {
        Self {
            label: label.into(),
            kind,
            tail: None,
            type_: None,
            insert: None,
            snippet: None,
            boost,
            add_from: None,
            retrigger: false,
            primary_key: false,
            foreign_key: false,
            doc: None,
        }
    }

    /// The text accepting the candidate inserts.
    pub fn insert_text(&self) -> &str {
        self.insert.as_deref().unwrap_or(&self.label)
    }
}

/// The console's automatic trigger: a word or a qualifier followed by a dot.
pub fn should_auto_complete_sql(analysis: &Analysis) -> bool {
    let typed = &analysis.prefix.text;
    analysis.context != SqlContext::None
        && (!typed.is_empty() || analysis.qualifier.is_some())
        && !typed.starts_with(|c: char| c.is_ascii_digit())
}

const MAX_ALL_COLUMNS: usize = 3000;

fn plain_ident(name: &str, driver: DbDriver) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    type CharTest = fn(char) -> bool;
    let (start, part): (CharTest, CharTest) = match driver {
        DbDriver::Postgres => (
            |c| c == '_' || c.is_ascii_lowercase(),
            |c| c == '_' || c == '$' || c.is_ascii_lowercase() || c.is_ascii_digit(),
        ),
        // Oracle folds unquoted names to upper case; a lower-case name must stay quoted.
        DbDriver::Oracle => (
            |c| c == '_' || c.is_ascii_uppercase(),
            |c| c == '_' || c == '$' || c.is_ascii_uppercase() || c.is_ascii_digit(),
        ),
        DbDriver::Clickhouse => (|c| c == '_' || c.is_ascii_alphabetic(), |c| c == '_' || c.is_ascii_alphanumeric()),
        DbDriver::Sqlite | DbDriver::Mysql | DbDriver::SqlServer => {
            (|c| c == '_' || c.is_ascii_alphabetic(), |c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        }
    };
    start(first) && chars.all(part)
}

/// `name`, quoted when the dialect needs it (case, characters, reserved words).
pub fn quote_ident(name: &str, driver: DbDriver) -> String {
    if plain_ident(name, driver) && !is_reserved(name) {
        return name.to_string();
    }
    if driver == DbDriver::Clickhouse {
        return format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""));
    }
    let q = if driver == DbDriver::Mysql { '`' } else { '"' };
    format!("{q}{}{q}", name.replace(q, &format!("{q}{q}")))
}

/// A short alias for `table` not in `taken`: `order_items` → `oi`, `users` → `u`.
pub fn alias_for(table: &str, taken: &HashSet<String>) -> String {
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in table.chars() {
        if prev.is_some_and(|p| p.is_ascii_lowercase()) && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        spaced.push(c);
        prev = Some(c);
    }
    let initials: String = spaced
        .split(|c: char| c == '_' || c == '.' || c == '$' || c == '-' || c.is_whitespace())
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .collect();
    let mut base = if initials.is_empty() { "t".to_string() } else { initials };
    if base.starts_with(|c: char| c.is_ascii_digit()) {
        base = format!("t{base}");
    }
    if is_reserved(&base) {
        base.push('1');
    }
    let mut a = base.clone();
    let mut n = 1;
    while taken.contains(&a.to_lowercase()) {
        a = format!("{base}{n}");
        n += 1;
    }
    a
}

#[derive(Clone, Copy, Debug)]
pub struct CompletionInput<'a> {
    pub model: Option<&'a CompletionModel>,
    pub analysis: &'a Analysis,
    pub driver: DbDriver,
    /// Data source name, shown next to tables.
    pub source_name: Option<&'a str>,
}

/// A column as completion knows it: from the catalog, or a name from the query text.
#[derive(Clone, Copy)]
struct Col<'a> {
    name: &'a str,
    info: Option<&'a CompletionColumn>,
}

struct Builder<'a> {
    model: Option<&'a CompletionModel>,
    a: &'a Analysis,
    driver: DbDriver,
    source_name: Option<&'a str>,
    lower: bool,
    tables: &'a [CompletionTable],
    default_schema: &'a str,
    fk_columns: HashSet<(String, String, String)>,
    out: Vec<SqlCandidate>,
}

impl<'a> Builder<'a> {
    fn eq(&self, a: &str, b: &str) -> bool {
        names_eq(self.driver, a, b)
    }

    fn q(&self, name: &str) -> String {
        quote_ident(name, self.driver)
    }

    fn kw(&self, k: &str) -> String {
        if self.lower { k.to_lowercase() } else { k.to_string() }
    }

    fn keywords(&mut self, list: &[&str], boost: i32) {
        for k in list {
            let label = self.kw(k);
            self.out.push(SqlCandidate::new(label, CandidateKind::Keyword, boost));
        }
    }

    fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&'a CompletionTable> {
        let tables = self.tables;
        match schema {
            Some(schema) => tables.iter().find(|t| self.eq(&t.schema, schema) && self.eq(&t.name, name)),
            None => tables
                .iter()
                .find(|t| self.eq(&t.schema, self.default_schema) && self.eq(&t.name, name))
                .or_else(|| tables.iter().find(|t| self.eq(&t.name, name))),
        }
    }

    fn columns_of(&self, r: &'a TableRef) -> Vec<Col<'a>> {
        match &r.columns {
            Some(names) => names.iter().map(|name| Col { name, info: None }).collect(),
            None => self
                .find_table(r.schema.as_deref(), &r.name)
                .map(|t| t.columns.iter().map(|c| Col { name: &c.name, info: Some(c) }).collect())
                .unwrap_or_default(),
        }
    }

    /// Owner shown after a column: the alias, else the table qualified by its schema (`(main.users)`).
    fn owner_of(&self, r: &TableRef) -> String {
        if r.alias.is_some() || r.kind != RefKind::Table {
            return r.ref_name().to_string();
        }
        match self.find_table(r.schema.as_deref(), &r.name) {
            Some(t) if !t.schema.is_empty() => format!("{}.{}", t.schema, t.name),
            _ => r.name.clone(),
        }
    }

    /// One candidate inserting several columns: `id, email, name`.
    fn columns_candidate(&self, r: &TableRef, names: &[&str], boost: i32, qualify: bool) -> SqlCandidate {
        let insert = names
            .iter()
            .map(|n| if qualify { format!("{}.{}", self.q(r.ref_name()), self.q(n)) } else { self.q(n) })
            .collect::<Vec<_>>()
            .join(", ");
        SqlCandidate {
            tail: Some(format!("({})", self.owner_of(r))),
            insert: Some(insert),
            ..SqlCandidate::new(names.join(", "), CandidateKind::Columns, boost)
        }
    }

    fn is_fk(&self, t: Option<&CompletionTable>, column: &str) -> bool {
        t.is_some_and(|t| self.fk_columns.contains(&(t.schema.clone(), t.name.clone(), column.to_string())))
    }

    /// How a table is written from here: unqualified in the default schema.
    fn table_text(&self, t: &CompletionTable) -> String {
        if self.eq(&t.schema, self.default_schema) || self.driver == DbDriver::Sqlite {
            self.q(&t.name)
        } else {
            format!("{}.{}", self.q(&t.schema), self.q(&t.name))
        }
    }

    fn table_candidate(&self, t: &CompletionTable, boost: i32, qualified: bool) -> SqlCandidate {
        let kind = if t.kind == DbTableKind::Table { CandidateKind::Table } else { CandidateKind::View };
        SqlCandidate {
            tail: (!t.schema.is_empty()).then(|| format!("({})", t.schema)),
            type_: self.source_name.map(str::to_string),
            insert: Some(if qualified {
                format!("{}.{}", self.q(&t.schema), self.q(&t.name))
            } else {
                self.table_text(t)
            }),
            doc: Some(table_doc(t)),
            ..SqlCandidate::new(t.name.clone(), kind, boost)
        }
    }

    fn column_candidate(
        &self,
        c: Col<'_>,
        owner: &str,
        table: Option<&CompletionTable>,
        boost: i32,
        insert: Option<String>,
    ) -> SqlCandidate {
        let data_type = c.info.map(|i| i.data_type.as_str()).filter(|t| !t.is_empty());
        let primary_key = c.info.is_some_and(|i| i.primary_key);
        let doc = data_type.map(|dt| {
            let not_null = if c.info.is_some_and(|i| !i.nullable) { " NOT NULL" } else { "" };
            let pk = if primary_key { " PRIMARY KEY" } else { "" };
            format!("{owner}.{} {dt}{not_null}{pk}", c.name)
        });
        SqlCandidate {
            tail: Some(format!("({owner})")),
            type_: data_type.map(str::to_uppercase),
            insert: Some(insert.unwrap_or_else(|| self.q(c.name))),
            primary_key,
            foreign_key: self.is_fk(table, c.name),
            doc,
            ..SqlCandidate::new(c.name, CandidateKind::Column, boost)
        }
    }

    fn table_of(&self, r: &TableRef) -> Option<&'a CompletionTable> {
        (r.kind == RefKind::Table).then(|| self.find_table(r.schema.as_deref(), &r.name)).flatten()
    }

    fn column_candidates(&self, r: &'a TableRef, owner: &str, boost: i32) -> Vec<SqlCandidate> {
        let t = self.table_of(r);
        self.columns_of(r).into_iter().map(|c| self.column_candidate(c, owner, t, boost, None)).collect()
    }

    fn table_columns(&self, t: &CompletionTable, boost: i32) -> Vec<SqlCandidate> {
        t.columns
            .iter()
            .map(|c| self.column_candidate(Col { name: &c.name, info: Some(c) }, &t.name, Some(t), boost, None))
            .collect()
    }

    fn taken_by(&self, skip: Option<&TableRef>) -> HashSet<String> {
        self.a
            .refs
            .iter()
            .filter(|r| skip.is_none_or(|s| !std::ptr::eq(*r, s)))
            .flat_map(|r| [r.ref_name().to_lowercase(), r.name.to_lowercase()])
            .collect()
    }

    fn qualified(&self, qualifier: &[String]) -> Vec<SqlCandidate> {
        let a = self.a;
        let first = qualifier[0].as_str();
        let is_table_ctx = matches!(a.context, SqlContext::Table { .. });
        if qualifier.len() == 1 && !is_table_ctx {
            let found = a.refs.iter().find(|r| {
                names_eq_opt(self.driver, r.alias.as_deref(), first) || (r.alias.is_none() && self.eq(&r.name, first))
            });
            if let Some(r) = found {
                return self.column_candidates(r, r.ref_name(), 50);
            }
        }
        if qualifier.len() == 1 {
            let in_schema: Vec<&CompletionTable> = self.tables.iter().filter(|t| self.eq(&t.schema, first)).collect();
            if !in_schema.is_empty() {
                return in_schema
                    .into_iter()
                    .map(|t| SqlCandidate { insert: Some(self.q(&t.name)), ..self.table_candidate(t, 30, false) })
                    .collect();
            }
            return match self.find_table(None, first) {
                Some(t) if !is_table_ctx => self.table_columns(t, 50),
                _ => Vec::new(),
            };
        }
        match self.find_table(Some(first), &qualifier[1]) {
            Some(t) => self.table_columns(t, 50),
            None => Vec::new(),
        }
    }

    fn build(mut self) -> Vec<SqlCandidate> {
        let a = self.a;
        if let Some(qualifier) = &a.qualifier {
            return self.qualified(qualifier);
        }
        match a.context {
            SqlContext::None => {}
            // Below the live templates (`sel`, `ins`…).
            SqlContext::Statement => self.keywords(&statement_starters(self.driver), -10),
            SqlContext::Table { join } => self.table_context(join),
            SqlContext::AfterTable { reference } => self.after_table(&a.refs[reference]),
            SqlContext::JoinCondition { reference } => {
                let conditions = condition_candidates(a, self.model, &a.refs[reference], self.lower, self.driver);
                self.out.extend(conditions);
                self.column_context(Clause::On);
            }
            SqlContext::InsertColumns { target } => self.insert_columns(&a.refs[target]),
            SqlContext::AfterInsertColumns => self.keywords(&["VALUES", "SELECT"], 20),
            SqlContext::Column { clause } => self.column_context(clause),
        }
        self.out
    }

    fn table_context(&mut self, join: bool) {
        let a = self.a;
        if join {
            let taken = self.taken_by(None);
            let joins = join_candidates(a, self.model, self.driver, &taken, self.lower);
            self.out.extend(joins);
        }
        for c in &a.ctes {
            self.out.push(SqlCandidate {
                tail: Some("(CTE)".to_string()),
                ..SqlCandidate::new(c.name.clone(), CandidateKind::Table, 40)
            });
        }
        let mut schemas: Vec<&str> = Vec::new();
        for t in self.tables {
            if !schemas.contains(&t.schema.as_str()) {
                schemas.push(&t.schema);
            }
            let local = self.eq(&t.schema, self.default_schema) || self.driver == DbDriver::Sqlite;
            let boost = match (local, t.kind) {
                (false, _) => 0,
                (true, DbTableKind::Table) => 30,
                (true, _) => 28,
            };
            let candidate = self.table_candidate(t, boost, !local);
            self.out.push(candidate);
        }
        for s in &schemas {
            if !self.eq(s, self.default_schema) || schemas.len() > 1 {
                let candidate = SqlCandidate {
                    type_: self.source_name.map(str::to_string),
                    insert: Some(format!("{}.", self.q(s))),
                    retrigger: true,
                    ..SqlCandidate::new(*s, CandidateKind::Schema, 5)
                };
                self.out.push(candidate);
            }
        }
        if join {
            self.keywords(&["LATERAL"], -10);
        }
    }

    fn after_table(&mut self, r: &TableRef) {
        let a = self.a;
        let inserting = a.verb == "INSERT" || a.verb == "REPLACE";
        if r.alias.is_none() && r.kind == RefKind::Table && !inserting {
            // An alias comes first.
            let taken = self.taken_by(Some(r));
            self.out.push(SqlCandidate::new(alias_for(&r.name, &taken), CandidateKind::Alias, 45));
        }
        if a.verb == "UPDATE" {
            self.keywords(&["SET", "AS"], 20);
        } else if inserting {
            self.keywords(&["VALUES", "SELECT", "DEFAULT VALUES"], 20);
        } else if a.verb == "DELETE" {
            let using: &[&str] = if self.driver == DbDriver::Postgres { &["USING"] } else { &[] };
            self.keywords(&[["WHERE", "AS"].as_slice(), using].concat(), 20);
        } else {
            if r.via_join {
                let on = self.kw("ON");
                let conditions = condition_candidates(a, self.model, r, self.lower, self.driver);
                self.out.extend(conditions.into_iter().map(|c| SqlCandidate {
                    label: format!("{on} {}", c.label),
                    insert: Some(format!("{on} {}", c.insert_text())),
                    ..c
                }));
                self.keywords(&["ON", "USING"], 25);
            }
            let mut list = vec!["AS", "WHERE"];
            list.extend(join_keywords(self.driver));
            list.extend(["GROUP BY", "ORDER BY", "LIMIT", "UNION", "UNION ALL"]);
            self.keywords(&list, 10);
        }
    }

    fn insert_columns(&mut self, target: &'a TableRef) {
        let t = self.table_of(target);
        let all = self.columns_of(target);
        // The columns an INSERT needs (no default), then all of them.
        let required: Vec<&str> =
            all.iter().filter(|c| !c.info.is_some_and(|i| i.has_default)).map(|c| c.name).collect();
        if required.len() > 1 {
            let candidate = self.columns_candidate(target, &required, 60, false);
            self.out.push(candidate);
        }
        if all.len() > 1 && all.len() != required.len() {
            let names: Vec<&str> = all.iter().map(|c| c.name).collect();
            let candidate = self.columns_candidate(target, &names, 55, false);
            self.out.push(candidate);
        }
        let owner = self.owner_of(target);
        let columns: Vec<SqlCandidate> = all.iter().map(|&c| self.column_candidate(c, &owner, t, 50, None)).collect();
        self.out.extend(columns);
        self.keywords(&["SELECT", "VALUES"], 5);
    }

    fn column_context(&mut self, clause: Clause) {
        let a = self.a;
        let visible = &a.refs;
        // `SET |` / `SET a = 1, |`: only the columns being assigned.
        let assigning = clause == Clause::Set && matches!(a.previous.as_deref(), Some("SET" | ","));
        let inner: Vec<&TableRef> = visible.iter().filter(|r| r.depth == 0).collect();
        // Column names in several tables of the block are inserted qualified.
        let mut count: HashMap<String, usize> = HashMap::new();
        for r in &inner {
            for c in self.columns_of(r) {
                *count.entry(c.name.to_lowercase()).or_default() += 1;
            }
        }
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for r in visible {
            let t = self.table_of(r);
            let owner = self.owner_of(r);
            let columns = self.columns_of(r);
            for &c in &columns {
                if !seen.insert((owner.clone(), c.name.to_string())) {
                    continue;
                }
                let ambiguous = r.depth > 0 || count.get(&c.name.to_lowercase()).copied().unwrap_or(0) > 1;
                let insert = ambiguous.then(|| format!("{}.{}", self.q(r.ref_name()), self.q(c.name)));
                let candidate = self.column_candidate(c, &owner, t, if r.depth == 0 { 40 } else { 20 }, insert);
                self.out.push(candidate);
            }
            if assigning {
                continue;
            }
            self.out.push(SqlCandidate::new(r.ref_name(), CandidateKind::Alias, 35));
            // The whole column list (the "expand *" item of the select list).
            let names: Vec<&str> = columns.iter().map(|c| c.name).collect();
            if clause == Clause::Select && r.depth == 0 && names.len() > 1 {
                let candidate = self.columns_candidate(r, &names, 30, inner.len() > 1);
                self.out.push(candidate);
            }
        }
        if assigning {
            return;
        }
        if inner.is_empty() && a.from_insert.is_some() && clause == Clause::Select {
            // No FROM yet: any column, and accepting it adds the FROM.
            let mut n = 0;
            for t in self.tables {
                if !self.eq(&t.schema, self.default_schema) && self.driver != DbDriver::Sqlite {
                    continue;
                }
                for c in &t.columns {
                    n += 1;
                    if n > MAX_ALL_COLUMNS + 1 {
                        break;
                    }
                    let candidate = SqlCandidate {
                        add_from: Some(self.table_text(t)),
                        ..self.column_candidate(Col { name: &c.name, info: Some(c) }, &t.name, Some(t), 0, None)
                    };
                    self.out.push(candidate);
                }
            }
        }
        for c in &a.ctes {
            if !visible.iter().any(|r| self.eq(&r.name, &c.name)) {
                self.out.push(SqlCandidate {
                    tail: Some("(CTE)".to_string()),
                    ..SqlCandidate::new(c.name.clone(), CandidateKind::Table, 0)
                });
            }
        }
        let fn_boost = if matches!(clause, Clause::Select | Clause::Having) { 15 } else { 8 };
        for f in functions(self.driver) {
            let name = if self.lower || self.driver == DbDriver::Clickhouse {
                f.name.to_string()
            } else {
                f.name.to_uppercase()
            };
            let boost =
                if f.aggregate && !matches!(clause, Clause::Select | Clause::Having) { fn_boost - 4 } else { fn_boost };
            let snippet = if f.params.is_empty() { format!("{name}()") } else { format!("{name}(${{}})") };
            self.out.push(SqlCandidate {
                tail: Some(format!("({})", f.params)),
                type_: f.returns.map(str::to_uppercase),
                snippet: Some(snippet),
                ..SqlCandidate::new(name, CandidateKind::Function, boost)
            });
        }
        let keywords = clause_keywords(clause, self.driver, a);
        let keywords: Vec<&str> = keywords.iter().map(String::as_str).collect();
        self.keywords(&keywords, 5);
    }
}

pub fn sql_candidates(input: CompletionInput<'_>) -> Vec<SqlCandidate> {
    let a = input.analysis;
    if a.context == SqlContext::None {
        return Vec::new();
    }
    let typed = &a.prefix.text;
    let has_lower = typed.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = typed.chars().any(|c| c.is_ascii_uppercase());
    let typed_lower = has_lower && !has_upper;
    let typed_upper = has_upper && !has_lower;
    let fk_columns = input
        .model
        .map(|m| {
            m.foreign_keys
                .iter()
                .flat_map(|k| k.columns.iter().map(|c| (k.schema.clone(), k.table.clone(), c.clone())))
                .collect()
        })
        .unwrap_or_default();
    Builder {
        model: input.model,
        a,
        driver: input.driver,
        source_name: input.source_name,
        lower: typed_lower || (!typed_upper && a.lower_keywords),
        tables: input.model.map(|m| m.tables.as_slice()).unwrap_or_default(),
        default_schema: input.model.map_or("", |m| m.default_schema.as_str()),
        fk_columns,
        out: Vec::new(),
    }
    .build()
}

fn clause_keywords(clause: Clause, driver: DbDriver, a: &Analysis) -> Vec<String> {
    let owned = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let pred = predicate_keywords(driver);
    match clause {
        Clause::Select => {
            let mut k = owned(&["FROM", "DISTINCT", "AS", "CASE", "WHEN", "THEN", "ELSE", "END", "NULL", "NOT"]);
            k.extend(owned(&["EXISTS", "CAST"]));
            if a.from_insert.is_none() {
                k.push("WHERE".to_string());
            }
            k
        }
        Clause::Where | Clause::On | Clause::Having => {
            let mut k = owned(&pred);
            k.extend(owned(&["GROUP BY", "ORDER BY", "LIMIT"]));
            k
        }
        Clause::Group => owned(&["HAVING", "ORDER BY", "LIMIT"]),
        Clause::Order => {
            let mut k = owned(&["ASC", "DESC", "LIMIT", "OFFSET"]);
            if driver != DbDriver::Mysql {
                k.extend(owned(&["NULLS FIRST", "NULLS LAST"]));
            }
            k
        }
        Clause::Set => owned(&["WHERE", "DEFAULT", "NULL"]),
        Clause::Values => owned(&["DEFAULT", "NULL", "TRUE", "FALSE"]),
        Clause::Limit => owned(&["OFFSET"]),
        Clause::Returning => owned(&["AS"]),
        Clause::From | Clause::Other => {
            let mut k = owned(&pred);
            k.extend(
                data_types(driver)
                    .iter()
                    .map(|t| if driver == DbDriver::Clickhouse { t.to_string() } else { t.to_uppercase() }),
            );
            k
        }
    }
}

fn kw(lower: bool, k: &str) -> String {
    if lower { k.to_lowercase() } else { k.to_string() }
}

/// Column pairs of a key: `(mine, theirs)`.
type Pairs = Vec<(String, String)>;

fn zip_pairs(mine: &[String], theirs: &[String]) -> Pairs {
    mine.iter().zip(theirs).map(|(m, t)| (m.clone(), t.clone())).collect()
}

/// `JOIN |` → whole join clauses to tables related to the ones in scope: by foreign key either
/// way, else by equal column names.
fn join_candidates(
    a: &Analysis,
    model: Option<&CompletionModel>,
    driver: DbDriver,
    taken: &HashSet<String>,
    lower: bool,
) -> Vec<SqlCandidate> {
    let Some(model) = model else { return Vec::new() };
    let eq = |x: &str, y: &str| names_eq(driver, x, y);
    let q = |n: &str| quote_ident(n, driver);
    let local = |s: &str| eq(s, &model.default_schema) || driver == DbDriver::Sqlite;
    let table_text = |schema: &str, name: &str| {
        if local(schema) { q(name) } else { format!("{}.{}", q(schema), q(name)) }
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for r in a.refs.iter().filter(|x| x.depth == 0 && x.kind == RefKind::Table) {
        let Some(rt) =
            model.tables.iter().find(|t| eq(&t.name, &r.name) && r.schema.as_deref().is_none_or(|s| eq(&t.schema, s)))
        else {
            continue;
        };
        let ra = r.ref_name();
        let mut add = |schema: &str, name: &str, pairs: Pairs, boost: i32| {
            let flat: Vec<&str> = pairs.iter().flat_map(|(m, t)| [m.as_str(), t.as_str()]).collect();
            if !seen.insert(format!("{schema}.{name}:{}", flat.join(","))) {
                return;
            }
            let alias = alias_for(name, taken);
            let cond = pairs
                .iter()
                .map(|(mine, theirs)| format!("{}.{} = {alias}.{}", q(ra), q(mine), q(theirs)))
                .collect::<Vec<_>>()
                .join(&format!(" {} ", kw(lower, "AND")));
            let text = format!("{} {alias} {} {cond}", table_text(schema, name), kw(lower, "ON"));
            let doc = pairs.iter().map(|(m, t)| format!("{ra}.{m} = {t}")).collect::<Vec<_>>().join(", ");
            out.push(SqlCandidate {
                doc: Some(format!("Join {name} on {doc}")),
                ..SqlCandidate::new(text, CandidateKind::Join, boost)
            });
        };
        for fk in &model.foreign_keys {
            // r references it: `orders o JOIN users u ON u.id = o.user_id`.
            if eq(&fk.schema, &rt.schema) && eq(&fk.table, &rt.name) {
                add(&fk.ref_schema, &fk.ref_table, zip_pairs(&fk.columns, &fk.ref_columns), 60);
            }
            // It references r: `users u JOIN orders o ON o.user_id = u.id`.
            if eq(&fk.ref_schema, &rt.schema) && eq(&fk.ref_table, &rt.name) {
                add(&fk.schema, &fk.table, zip_pairs(&fk.ref_columns, &fk.columns), 60);
            }
        }
        // Same-named key-ish columns (`user_id` ↔ `user_id`) with no declared key.
        let keyish: Vec<&CompletionColumn> =
            rt.columns.iter().filter(|c| c.primary_key || ends_with_id(&c.name)).collect();
        for t in &model.tables {
            if std::ptr::eq(t, rt) || !local(&t.schema) {
                continue;
            }
            for c in &keyish {
                if c.primary_key && eq(&c.name, "id") {
                    continue;
                }
                if t.columns.iter().any(|x| eq(&x.name, &c.name)) {
                    add(&t.schema, &t.name, vec![(c.name.clone(), c.name.clone())], 45);
                }
            }
        }
    }
    out
}

/// `…_id`, case-insensitively.
fn ends_with_id(name: &str) -> bool {
    name.len() >= 3 && name.is_char_boundary(name.len() - 3) && name[name.len() - 3..].eq_ignore_ascii_case("_id")
}

/// `JOIN t x ON |` → conditions relating `reference` to the other tables in scope.
fn condition_candidates(
    a: &Analysis,
    model: Option<&CompletionModel>,
    reference: &TableRef,
    lower: bool,
    driver: DbDriver,
) -> Vec<SqlCandidate> {
    let Some(model) = model.filter(|_| reference.kind == RefKind::Table) else { return Vec::new() };
    let eq = |x: &str, y: &str| names_eq(driver, x, y);
    let find = |r: &TableRef| {
        model.tables.iter().find(|t| eq(&t.name, &r.name) && r.schema.as_deref().is_none_or(|s| eq(&t.schema, s)))
    };
    let Some(mine) = find(reference) else { return Vec::new() };
    let ma = reference.ref_name();
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |pairs: &[(String, String)], other: &str, boost: i32| {
        let text = pairs
            .iter()
            .map(|(m, o)| format!("{other}.{o} = {ma}.{m}"))
            .collect::<Vec<_>>()
            .join(&format!(" {} ", kw(lower, "AND")));
        if seen.insert(text.clone()) {
            out.push(SqlCandidate::new(text, CandidateKind::Join, boost));
        }
    };
    for r in &a.refs {
        if std::ptr::eq(r, reference) || r.depth != 0 || r.from > reference.from {
            continue;
        }
        let t = if r.kind == RefKind::Table { find(r) } else { None };
        let oa = r.ref_name();
        if let Some(t) = t {
            for fk in &model.foreign_keys {
                if eq(&fk.table, &mine.name)
                    && eq(&fk.schema, &mine.schema)
                    && eq(&fk.ref_table, &t.name)
                    && eq(&fk.ref_schema, &t.schema)
                {
                    add(&zip_pairs(&fk.columns, &fk.ref_columns), oa, 60);
                }
                if eq(&fk.table, &t.name)
                    && eq(&fk.schema, &t.schema)
                    && eq(&fk.ref_table, &mine.name)
                    && eq(&fk.ref_schema, &mine.schema)
                {
                    add(&zip_pairs(&fk.ref_columns, &fk.columns), oa, 60);
                }
            }
        }
        let theirs: Vec<&str> = match t {
            Some(t) => t.columns.iter().map(|c| c.name.as_str()).collect(),
            None => r.columns.iter().flatten().map(String::as_str).collect(),
        };
        for c in &mine.columns {
            if ((c.primary_key && !eq(&c.name, "id")) || ends_with_id(&c.name)) && theirs.iter().any(|x| eq(x, &c.name))
            {
                add(&[(c.name.clone(), c.name.clone())], oa, 45);
            }
        }
    }
    out
}

fn table_doc(t: &CompletionTable) -> String {
    let width = t.columns.iter().map(|c| c.name.chars().count()).max().unwrap_or(0).max(4);
    let cols: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            format!(
                "  {:<width$}  {}{}{}",
                c.name,
                c.data_type,
                if c.nullable { "" } else { " NOT NULL" },
                if c.primary_key { " PK" } else { "" }
            )
        })
        .collect();
    let kind = match t.kind {
        DbTableKind::MaterializedView => "MATERIALIZED VIEW",
        DbTableKind::View => "VIEW",
        DbTableKind::Table => "TABLE",
    };
    format!("{kind} {}.{}\n{}", t.schema, t.name, cols.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{at, col, model};
    use super::super::resolve::SqlResolver;
    use super::*;
    use ide_db::ForeignKey;

    fn complete_with(sql: &str, driver: DbDriver, m: &CompletionModel) -> Vec<SqlCandidate> {
        let a = at(sql, driver);
        let mut c = sql_candidates(CompletionInput { model: Some(m), analysis: &a, driver, source_name: Some("shop") });
        c.sort_by_key(|x| std::cmp::Reverse(x.boost));
        c
    }

    fn complete(sql: &str) -> Vec<SqlCandidate> {
        complete_with(sql, DbDriver::Sqlite, &model())
    }

    fn labels(c: &[SqlCandidate], kind: Option<CandidateKind>) -> Vec<String> {
        c.iter().filter(|x| kind.is_none_or(|k| x.kind == k)).map(|x| x.label.clone()).collect()
    }

    fn find<'c>(c: &'c [SqlCandidate], label: &str) -> &'c SqlCandidate {
        c.iter().find(|x| x.label == label).unwrap_or_else(|| panic!("no candidate {label}"))
    }

    fn table(schema: &str, name: &str, columns: Vec<CompletionColumn>) -> CompletionTable {
        CompletionTable { schema: schema.into(), name: name.into(), kind: DbTableKind::Table, columns }
    }

    #[test]
    fn keeps_clickhouse_schema_table_and_column_names_distinct_by_case() {
        let catalog = CompletionModel {
            default_schema: "analytics".into(),
            foreign_keys: Vec::new(),
            tables: vec![
                table("analytics", "Events", vec![col("UpperOnly")]),
                table("analytics", "events", vec![col("Value"), col("value")]),
                table("Analytics", "events", vec![col("OtherSchema")]),
            ],
        };
        let ch = DbDriver::Clickhouse;
        assert_eq!(labels(&complete_with("select e.| from events e", ch, &catalog), None), ["Value", "value"]);
        assert_eq!(labels(&complete_with("select e.| from Events e", ch, &catalog), None), ["UpperOnly"]);
        assert_eq!(labels(&complete_with("select e.| from Analytics.events e", ch, &catalog), None), ["OtherSchema"]);
        let problems = |sql: &str| {
            SqlResolver::new(sql, ch, Some(&catalog)).problems().into_iter().map(|p| p.message).collect::<Vec<_>>()
        };
        assert!(problems("select Value, value from events").is_empty());
        assert_eq!(problems("select VALUE from events"), ["Unable to resolve column 'VALUE'"]);
        assert_eq!(problems("select UpperOnly from events"), ["Unable to resolve column 'UpperOnly'"]);
        assert!(problems("select OtherSchema from Analytics.events").is_empty());
        assert_eq!(problems("select OtherSchema from analytics.events"), ["Unable to resolve column 'OtherSchema'"]);
        let a = at("with Events as (select 1 as UpperOnly) select | from events e", ch);
        assert_eq!(a.refs.iter().find(|r| r.name == "events").map(|r| r.kind), Some(RefKind::Table));
    }

    #[test]
    fn offers_clickhouse_sql_and_keeps_case_sensitive_function_and_type_names() {
        let starters = statement_starters(DbDriver::Clickhouse);
        assert!(starters.contains(&"SHOW DATABASES") && starters.contains(&"ALTER TABLE"));
        assert!(!starters.contains(&"BEGIN") || !starters.contains(&"SET search_path TO"));
        assert!(data_types(DbDriver::Clickhouse).contains(&"UInt64"));
        let c = complete_with("SELECT | FROM users", DbDriver::Clickhouse, &model());
        assert_eq!(find(&c, "toDateTime").snippet.as_deref(), Some("toDateTime(${})"));
        assert_eq!(quote_ident("a\\b\"c", DbDriver::Clickhouse), "\"a\\\\b\\\"c\"");
    }

    #[test]
    fn offers_statement_starters_in_the_user_s_case() {
        assert!(labels(&complete("|"), None).contains(&"SELECT".to_string()));
        assert!(labels(&complete("select 1; ins|"), None).contains(&"insert into".to_string()));
    }

    #[test]
    fn offers_oracle_vocabulary_and_quotes_names_the_server_folds_to_upper_case() {
        let starters = statement_starters(DbDriver::Oracle);
        assert!(starters.contains(&"MERGE INTO") && starters.contains(&"TRUNCATE TABLE"));
        assert!(!starters.contains(&"SET search_path TO"));
        let types = data_types(DbDriver::Oracle);
        assert!(["VARCHAR2(255)", "NUMBER", "CLOB"].iter().all(|t| types.contains(t)));
        let c = complete_with("SELECT | FROM users", DbDriver::Oracle, &model());
        assert_eq!(find(&c, "NVL").snippet.as_deref(), Some("NVL(${})"));
        // Upper-case names are plain; lower-case ones need quotes (the server folds unquoted names).
        assert_eq!(quote_ident("EMPLOYEES", DbDriver::Oracle), "EMPLOYEES");
        assert_eq!(quote_ident("employees", DbDriver::Oracle), "\"employees\"");
        assert_eq!(quote_ident("a\"b", DbDriver::Oracle), "\"a\"\"b\"");
    }

    #[test]
    fn offers_tables_after_from_with_schema_and_data_source() {
        let c = complete("select * from |");
        let tables = labels(&c, Some(CandidateKind::Table));
        assert!(["users", "orders", "order_items", "products"].iter().all(|t| tables.contains(&t.to_string())));
        let view = find(&c, "big_orders");
        assert_eq!(view.kind, CandidateKind::View);
        assert_eq!(view.tail.as_deref(), Some("(main)"));
        assert_eq!(view.type_.as_deref(), Some("shop"));
    }

    #[test]
    fn suggests_whole_join_clauses_from_foreign_keys_both_directions_first() {
        let c = complete("select * from orders o join |");
        let joins = labels(&c, Some(CandidateKind::Join));
        let mut first: Vec<String> = joins[..2].to_vec();
        first.sort();
        assert_eq!(first, ["order_items oi on o.id = oi.order_id", "users u on o.user_id = u.id"]);
        assert!(joins.contains(&"big_orders bo on o.user_id = bo.user_id".to_string()));
        assert_eq!(c[0].kind, CandidateKind::Join);
    }

    #[test]
    fn avoids_alias_clashes() {
        assert_eq!(alias_for("users", &HashSet::from(["u".to_string()])), "u1");
        assert_eq!(alias_for("order_items", &HashSet::new()), "oi");
        assert_eq!(alias_for("OrderItems", &HashSet::new()), "oi");
        assert_eq!(alias_for("on_hold", &HashSet::new()), "oh");
    }

    #[test]
    fn suggests_the_join_condition_after_on() {
        let c = complete("select * from orders o join users u on |");
        assert_eq!((c[0].kind, c[0].label.as_str()), (CandidateKind::Join, "o.user_id = u.id"));
        let status = find(&c, "status");
        assert_eq!(status.kind, CandidateKind::Column);
        assert_eq!(status.tail.as_deref(), Some("(o)"));
        assert_eq!(status.type_.as_deref(), Some("TEXT"));
        // `id` is in both tables: inserted qualified.
        let mut ids: Vec<&str> = c.iter().filter(|x| x.label == "id").map(SqlCandidate::insert_text).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["o.id", "u.id"]);
    }

    #[test]
    fn offers_on_with_conditions_right_after_a_joined_table() {
        let c = complete("select * from orders o join users u |");
        assert_eq!(c[0].label, "on o.user_id = u.id");
        let keywords = labels(&c, Some(CandidateKind::Keyword));
        assert!(["on", "where", "left join"].iter().all(|k| keywords.contains(&k.to_string())));
    }

    #[test]
    fn completes_columns_of_an_alias_after_a_dot_with_key_markers() {
        let c = complete("select o.| from orders o");
        assert_eq!(labels(&c, None), ["id", "user_id", "status", "total"]);
        assert_eq!((find(&c, "id").primary_key, find(&c, "id").foreign_key), (true, false));
        assert_eq!((find(&c, "user_id").primary_key, find(&c, "user_id").foreign_key), (false, true));
    }

    #[test]
    fn completes_cte_and_derived_table_columns() {
        assert_eq!(
            labels(&complete("with r as (select id, total amount from orders) select r.| from r"), None),
            ["id", "amount"]
        );
    }

    #[test]
    fn completes_the_columns_an_insert_needs_all_of_them_then_each_one() {
        let c = complete("insert into users (|)");
        let head: Vec<(CandidateKind, &str, Option<&str>)> =
            c[..2].iter().map(|x| (x.kind, x.label.as_str(), x.tail.as_deref())).collect();
        assert_eq!(
            head,
            [
                (CandidateKind::Columns, "id, email, name", Some("(main.users)")),
                (CandidateKind::Columns, "id, email, name, created_at", Some("(main.users)")),
            ]
        );
        assert_eq!(labels(&c, Some(CandidateKind::Column)), ["id", "email", "name", "created_at"]);
    }

    #[test]
    fn suggests_an_alias_right_after_a_table_first() {
        let c = complete("select * from order_items |");
        assert_eq!((c[0].kind, c[0].label.as_str()), (CandidateKind::Alias, "oi"));
        assert_ne!(complete("select * from users u |").first().map(|x| x.kind), Some(CandidateKind::Alias));
    }

    #[test]
    fn offers_the_whole_column_list_in_the_select_list() {
        let c = complete("select | from users");
        let all = c.iter().find(|x| x.kind == CandidateKind::Columns).expect("a column list");
        assert_eq!(all.label, "id, email, name, created_at");
        assert_eq!(all.tail.as_deref(), Some("(main.users)"));
        assert_eq!(all.insert_text(), "id, email, name, created_at");
        let two = complete("select | from users u join orders o on o.user_id = u.id");
        let u = two.iter().find(|x| x.kind == CandidateKind::Columns && x.tail.as_deref() == Some("(u)"));
        assert_eq!(u.map(SqlCandidate::insert_text), Some("u.id, u.email, u.name, u.created_at"));
    }

    #[test]
    fn names_unaliased_tables_by_schema_in_column_rows() {
        assert_eq!(find(&complete("update users set |"), "email").tail.as_deref(), Some("(main.users)"));
    }

    #[test]
    fn offers_only_columns_as_set_targets() {
        for text in ["update users set |", "update users set name = 'a', |"] {
            let kinds: HashSet<CandidateKind> = complete(text).iter().map(|x| x.kind).collect();
            assert_eq!(kinds, HashSet::from([CandidateKind::Column]), "{text}");
        }
        assert!(complete("update users set name = |").iter().any(|x| x.kind == CandidateKind::Function));
    }

    #[test]
    fn offers_any_column_before_from_and_adds_the_from() {
        let c = complete("select emai|");
        let email = find(&c, "email");
        assert_eq!(email.add_from.as_deref(), Some("users"));
        assert_eq!(email.tail.as_deref(), Some("(users)"));
    }

    #[test]
    fn orders_scope_columns_before_functions_before_keywords() {
        let c = complete("select | from users");
        let first = |k| c.iter().position(|x| x.kind == k).expect("the kind is offered");
        assert!(first(CandidateKind::Column) < first(CandidateKind::Function));
        assert!(first(CandidateKind::Function) < first(CandidateKind::Keyword));
        assert_eq!(find(&c, "count").snippet.as_deref(), Some("count(${})"));
    }

    #[test]
    fn quotes_identifiers_that_need_it() {
        assert_eq!(quote_ident("Users", DbDriver::Postgres), "\"Users\"");
        assert_eq!(quote_ident("Users", DbDriver::Mysql), "Users");
        assert_eq!(quote_ident("order", DbDriver::Mysql), "`order`");
        assert_eq!(quote_ident("my col", DbDriver::Sqlite), "\"my col\"");
    }

    #[test]
    fn qualifies_tables_outside_the_default_schema() {
        let pg = CompletionModel {
            default_schema: "public".into(),
            tables: vec![table("public", "users", vec![col("id")]), table("audit", "log", vec![col("id")])],
            foreign_keys: Vec::<ForeignKey>::new(),
        };
        let c = complete_with("select * from |", DbDriver::Postgres, &pg);
        assert_eq!(find(&c, "log").insert.as_deref(), Some("audit.log"));
        assert_eq!(find(&c, "users").insert.as_deref(), Some("users"));
        assert_eq!(labels(&complete_with("select * from audit.|", DbDriver::Postgres, &pg), None), ["log"]);
        let schema = c.iter().find(|x| x.kind == CandidateKind::Schema && x.label == "audit").expect("the schema");
        assert_eq!(schema.insert.as_deref(), Some("audit."));
        assert!(schema.retrigger);
    }

    #[test]
    fn is_silent_in_strings_and_comments() {
        assert!(complete("select '|'").is_empty());
        assert!(complete("-- sel|").is_empty());
    }
}
