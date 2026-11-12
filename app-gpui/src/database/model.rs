//! Pure helpers of the database tools: clipboard formats, table edits, labels, the Schemas tab's
//! patterns, connection targets and error remedies, SSH settings and Data Source from URL.

use super::vendors::{vendor_of, vendor_of_scheme};
use ide_core::error::ErrorKind;
use ide_core::remote_settings::SshConfiguration;
use ide_db::{CellValue, DataSource, DbColor, DbDriver, DbProperty, DbRowEdit, DbSsh, DbSshAuth, DbSslMode, SortKey};
use regex::Regex;
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

/// A cell's text; `None` is SQL NULL.
pub type Cell = Option<String>;

/// The number JavaScript's `Number(text)` reads (`None` for NaN): surrounding white space ignored,
/// empty text is 0, `0x` / `0o` / `0b` integers, `Infinity`.
pub fn js_number(text: &str) -> Option<f64> {
    let s = text.trim();
    if s.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = s.strip_prefix(prefix) {
            return u128::from_str_radix(digits, radix).ok().filter(|_| !digits.starts_with('+')).map(|n| n as f64);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned == "Infinity" {
        return Some(if s.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if !unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.')
        || !unsigned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Whether `text` is a finite number to JavaScript (`Number.isFinite(Number(text))`).
pub fn is_js_finite(text: &str) -> bool {
    js_number(text).is_some_and(f64::is_finite)
}

/// A number as JavaScript writes it (`String(n)`): no `.0`, exponents from 1e21 and below 1e-6.
pub fn js_number_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let abs = n.abs();
    if (1e-6..1e21).contains(&abs) {
        return format!("{n}");
    }
    let e = format!("{n:e}");
    match e.split_once('e') {
        Some((mantissa, exp)) if !exp.starts_with('-') => format!("{mantissa}e+{exp}"),
        _ => e,
    }
}

/// Tab-separated values (Excel "Copy"): NULL is empty, tabs / newlines become spaces.
pub fn to_tsv(header: Option<&[String]>, rows: &[Vec<Cell>]) -> String {
    fn clean(v: Option<&str>) -> String {
        let mut out = String::new();
        let mut in_run = false;
        for c in v.unwrap_or("").chars() {
            if matches!(c, '\t' | '\r' | '\n') {
                if !in_run {
                    out.push(' ');
                }
                in_run = true;
            } else {
                out.push(c);
                in_run = false;
            }
        }
        out
    }
    let mut lines: Vec<String> = Vec::new();
    if let Some(header) = header {
        lines.push(header.iter().map(|h| clean(Some(h))).collect::<Vec<_>>().join("\t"));
    }
    lines.extend(rows.iter().map(|r| r.iter().map(|v| clean(v.as_deref())).collect::<Vec<_>>().join("\t")));
    lines.join("\n")
}

/// Every value on a single `, `-separated line (One-Row, for `IN (…)` lists): NULL for nulls,
/// numbers unquoted, everything else single-quoted with `'` doubled. `numeric(c)` decides
/// quoting per column.
pub fn to_one_row(rows: &[Vec<Cell>], numeric: impl Fn(usize) -> bool) -> String {
    rows.iter()
        .flat_map(|r| {
            r.iter().enumerate().map(|(c, v)| match v {
                None => "NULL".to_string(),
                Some(v) if numeric(c) && is_js_finite(v) => v.clone(),
                Some(v) => format!("'{}'", v.replace('\'', "''")),
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// RFC 4180 CSV; NULL is an empty field.
pub fn to_csv(header: Option<&[String]>, rows: &[Vec<Cell>]) -> String {
    fn field(v: Option<&str>) -> String {
        let s = v.unwrap_or("");
        if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    }
    let mut lines: Vec<String> = Vec::new();
    if let Some(header) = header {
        lines.push(header.iter().map(|h| field(Some(h))).collect::<Vec<_>>().join(","));
    }
    lines.extend(rows.iter().map(|r| r.iter().map(|v| field(v.as_deref())).collect::<Vec<_>>().join(",")));
    lines.join("\r\n")
}

/// Rows of RFC 4180 text with separator `sep` (quoted fields, `""`, CR LF); a trailing newline
/// adds no row; a leading byte order mark is skipped.
pub fn parse_delimited(text: &str, sep: char) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.strip_prefix('\u{feff}').unwrap_or(text).chars().collect();
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if quoted {
            if ch != '"' {
                field.push(ch);
            } else if chars.get(i + 1) == Some(&'"') {
                field.push('"');
                i += 1;
            } else {
                quoted = false;
            }
        } else if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch == sep {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.get(i + 1) == Some(&'\n') {
                i += 1;
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(ch);
        }
        i += 1;
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The separator of delimited text: the one of `,`, tab and `;` its first line has most of.
pub fn guess_separator(text: &str) -> char {
    let first = text.split('\n').next().unwrap_or("");
    let first = first.strip_suffix('\r').unwrap_or(first);
    let count = |c: char| first.matches(c).count();
    [',', '\t', ';'].into_iter().fold(',', |best, c| if count(c) > count(best) { c } else { best })
}

/// What [`import_edits`] makes of delimited rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub edits: Vec<DbRowEdit>,
    /// The table columns the fields fill, in field order.
    pub mapped: Vec<String>,
}

/// Insert edits importing `rows` into a table with `columns`: by header name (case-insensitive;
/// unknown names skipped) or by position. `empty_as_null` turns empty fields into NULL.
pub fn import_edits(rows: &[Vec<String>], columns: &[String], header: bool, empty_as_null: bool) -> Import {
    let target = |h: &str| columns.iter().find(|c| c.to_lowercase() == h.trim().to_lowercase()).cloned();
    let map: Vec<Option<String>> = if header {
        rows.first().map(|r| r.iter().map(|h| target(h)).collect()).unwrap_or_default()
    } else {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        columns.iter().take(width).cloned().map(Some).collect()
    };
    let body = if header { rows.get(1..).unwrap_or(&[]) } else { rows };
    let edits = body
        .iter()
        .filter(|r| r.iter().any(|v| !v.is_empty()))
        .map(|r| DbRowEdit::Insert {
            values: map
                .iter()
                .enumerate()
                .filter_map(|(i, column)| {
                    let column = column.clone()?;
                    let value = match r.get(i) {
                        Some(v) if empty_as_null && v.is_empty() => None,
                        None if empty_as_null => None,
                        v => v.cloned(),
                    };
                    Some(CellValue { column, value })
                })
                .collect(),
        })
        .collect();
    Import { edits, mapped: map.into_iter().flatten().collect() }
}

/// Pending changes of the table editor. Row indices refer to the loaded page; every collection
/// keeps the order the changes were made in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingEdits {
    /// row → column → new value.
    pub updates: Vec<(usize, Vec<(String, Cell)>)>,
    pub deletes: BTreeSet<usize>,
    /// New rows: column → value (absent columns use their default).
    pub inserts: Vec<Vec<(String, Cell)>>,
}

/// Sets `column` of an ordered column → value list.
pub fn set_value(values: &mut Vec<(String, Cell)>, column: &str, value: Cell) {
    match values.iter_mut().find(|(c, _)| c == column) {
        Some(slot) => slot.1 = value,
        None => values.push((column.to_string(), value)),
    }
}

/// The value of `column` in an ordered column → value list.
pub fn value_of<'v>(values: &'v [(String, Cell)], column: &str) -> Option<&'v Cell> {
    values.iter().find(|(c, _)| c == column).map(|(_, v)| v)
}

impl PendingEdits {
    pub fn has_edits(&self) -> bool {
        !self.updates.is_empty() || !self.deletes.is_empty() || !self.inserts.is_empty()
    }

    /// The pending changes of loaded row `row`.
    pub fn updates_of(&self, row: usize) -> Option<&[(String, Cell)]> {
        self.updates.iter().find(|(r, _)| *r == row).map(|(_, c)| c.as_slice())
    }

    /// The pending changes of loaded row `row`, created empty when there are none.
    pub fn updates_of_mut(&mut self, row: usize) -> &mut Vec<(String, Cell)> {
        let i = match self.updates.iter().position(|(r, _)| *r == row) {
            Some(i) => i,
            None => {
                self.updates.push((row, Vec::new()));
                self.updates.len() - 1
            }
        };
        &mut self.updates[i].1
    }

    /// Drops the pending changes of loaded row `row`.
    pub fn clear_updates(&mut self, row: usize) {
        self.updates.retain(|(r, _)| *r != row);
    }

    /// The value shown for `row`, `col` of a page with `names` / `rows`: added rows past the
    /// loaded ones, pending changes applied.
    pub fn value(&self, rows: &[Vec<Cell>], names: &[String], row: usize, col: usize) -> Cell {
        let name = names.get(col)?;
        if row >= rows.len() {
            return self.inserts.get(row - rows.len()).and_then(|ins| value_of(ins, name)).cloned().flatten();
        }
        if let Some(v) = self.updates_of(row).and_then(|u| value_of(u, name)) {
            return v.clone();
        }
        rows.get(row).and_then(|r| r.get(col)).cloned().flatten()
    }

    /// Whether a loaded cell has a pending change.
    pub fn is_modified(&self, names: &[String], row: usize, col: usize) -> bool {
        names.get(col).is_some_and(|name| self.updates_of(row).is_some_and(|u| value_of(u, name).is_some()))
    }

    /// Sets `value` of `row`, `col`: an added row's field, or a change of a loaded row (setting it
    /// back to the loaded value drops the change).
    pub fn set_cell(&mut self, rows: &[Vec<Cell>], names: &[String], row: usize, col: usize, value: Cell) {
        let Some(name) = names.get(col) else { return };
        if row >= rows.len() {
            if let Some(ins) = self.inserts.get_mut(row - rows.len()) {
                set_value(ins, name, value);
            }
            return;
        }
        let original = rows.get(row).and_then(|r| r.get(col)).cloned().flatten();
        let updates = self.updates_of_mut(row);
        if original == value {
            updates.retain(|(c, _)| c != name);
        } else {
            set_value(updates, name, value);
        }
        if updates.is_empty() {
            self.clear_updates(row);
        }
    }

    /// Delete Rows: loaded rows (below `loaded`) are marked, added ones dropped.
    pub fn delete_rows(&mut self, loaded: usize, rows: &[usize]) {
        let gone: BTreeSet<usize> = rows.iter().filter(|&&r| r >= loaded).map(|r| r - loaded).collect();
        self.deletes.extend(rows.iter().copied().filter(|&r| r < loaded));
        let mut i = 0;
        self.inserts.retain(|_| {
            let keep = !gone.contains(&i);
            i += 1;
            keep
        });
    }
}

/// The DML requests for `edits` of a page with `columns` / `rows`, keyed by `primary_key`.
pub fn build_row_edits(
    columns: &[String],
    rows: &[Vec<Cell>],
    primary_key: &[String],
    edits: &PendingEdits,
) -> Vec<DbRowEdit> {
    let key_of = |row: usize| -> Vec<CellValue> {
        primary_key
            .iter()
            .map(|c| CellValue {
                column: c.clone(),
                value: columns
                    .iter()
                    .position(|x| x == c)
                    .and_then(|i| rows.get(row).and_then(|r| r.get(i)))
                    .cloned()
                    .flatten(),
            })
            .collect()
    };
    let cells = |values: &[(String, Cell)]| -> Vec<CellValue> {
        values.iter().map(|(column, value)| CellValue { column: column.clone(), value: value.clone() }).collect()
    };
    let mut out = Vec::new();
    for (row, changes) in &edits.updates {
        if edits.deletes.contains(row) || changes.is_empty() {
            continue;
        }
        out.push(DbRowEdit::Update { key: key_of(*row), values: cells(changes) });
    }
    out.extend(edits.deletes.iter().map(|&row| DbRowEdit::Delete { key: key_of(row) }));
    out.extend(edits.inserts.iter().map(|ins| DbRowEdit::Insert { values: cells(ins) }));
    out
}

/// The default name of a file data source: the file name without its extension (`shop.db` → `shop`).
pub fn file_source_name(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = match base.rfind('.') {
        Some(dot) if dot + 1 < base.len() => &base[..dot],
        _ => base,
    };
    if stem.is_empty() { base.to_string() } else { stem.to_string() }
}

/// The tree / menu icon of a data source (the vendor's mark).
pub fn source_icon(vendor: &str, driver: DbDriver) -> &'static str {
    vendor_of(vendor, driver).icon
}

/// `name (Vendor)`.
pub fn source_label(name: &str, driver: DbDriver, vendor: &str) -> String {
    format!("{name} ({})", vendor_of(vendor, driver).label)
}

/// The theme token of a data source colour (tree rows, console tab).
pub fn color_token(color: Option<DbColor>) -> Option<String> {
    let name = match color? {
        DbColor::Red => "red",
        DbColor::Orange => "orange",
        DbColor::Yellow => "yellow",
        DbColor::Green => "green",
        DbColor::Blue => "blue",
        DbColor::Violet => "violet",
    };
    Some(format!("--db-color-{name}"))
}

/// `*` / `?` wildcard match of the Schemas tab (case-sensitive, like the server's names).
pub fn matches_pattern(name: &str, pattern: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return name == pattern;
    }
    fn glob(n: &[char], p: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|k| glob(&n[k..], rest)),
            Some(('?', rest)) => !n.is_empty() && glob(&n[1..], rest),
            Some((c, rest)) => n.first() == Some(c) && glob(&n[1..], rest),
        }
    }
    let n: Vec<char> = name.chars().collect();
    let mut p: Vec<char> = Vec::new();
    for c in pattern.chars() {
        // Consecutive stars match what one does.
        if !(c == '*' && p.last() == Some(&'*')) {
            p.push(c);
        }
    }
    glob(&n, &p)
}

/// The table editor's ORDER BY field for a sort: `name DESC, id`.
pub fn format_order_by(sort: &[SortKey]) -> String {
    sort.iter()
        .map(|k| if k.descending { format!("{} DESC", k.column) } else { k.column.clone() })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parses the ORDER BY field (`col [ASC|DESC], …`, names matched case-insensitively, optionally
/// double-quoted); `None` when a part isn't a column.
pub fn parse_order_by(text: &str, columns: &[String]) -> Option<Vec<SortKey>> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (body, descending) = match part.rsplit_once(char::is_whitespace) {
            Some((body, dir)) if dir.eq_ignore_ascii_case("desc") => (body.trim_end(), true),
            Some((body, dir)) if dir.eq_ignore_ascii_case("asc") => (body.trim_end(), false),
            _ => (part, false),
        };
        let name = match body.strip_prefix('"') {
            Some(inner) => inner.strip_suffix('"')?,
            None => body,
        };
        if name.is_empty() || name.contains('"') {
            return None;
        }
        let wanted = name.trim().to_lowercase();
        let column = columns.iter().find(|c| c.to_lowercase() == wanted)?;
        out.push(SortKey { column: column.clone(), descending });
    }
    Some(out)
}

/// Schemas tab › Object filter: `name` matches one of its patterns (comma / space separated;
/// empty: all).
pub fn passes_object_filter(name: &str, filter: Option<&str>) -> bool {
    let patterns: Vec<&str> =
        filter.unwrap_or("").split(|c: char| c == ',' || c.is_whitespace()).filter(|p| !p.is_empty()).collect();
    let lower = name.to_lowercase();
    patterns.is_empty() || patterns.iter().any(|p| matches_pattern(&lower, &p.to_lowercase()))
}

/// The tree's quick filter (the toolbar's field): `name` contains it, case-insensitively.
pub fn passes_tree_filter(name: &str, filter: Option<&str>) -> bool {
    let q = filter.unwrap_or("").trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q)
}

/// Whether a console of `driver` gets the active-schema dropdown (a server with several schemas /
/// databases to switch between; SQLite has one database only).
pub fn schema_switchable(driver: Option<DbDriver>) -> bool {
    matches!(driver, Some(DbDriver::Postgres | DbDriver::Mysql | DbDriver::SqlServer | DbDriver::Oracle))
}

/// The schema a console's statements run in: its pick, else the connection's default.
pub fn active_schema(picked: Option<&str>, default_schema: Option<&str>) -> String {
    picked.filter(|p| !p.is_empty()).or(default_schema).unwrap_or("").to_string()
}

/// The databases / schemas the tree shows (Schemas tab): all of them, or the ones matching a
/// chosen name or pattern; `keep` (the connected database) always stays.
pub fn shown_names(names: &[String], all: Option<bool>, chosen: Option<&[String]>, keep: Option<&str>) -> Vec<String> {
    if all != Some(false) {
        return names.to_vec();
    }
    let picks: Vec<&str> = chosen.unwrap_or(&[]).iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    names.iter().filter(|n| Some(n.as_str()) == keep || picks.iter().any(|p| matches_pattern(n, p))).cloned().collect()
}

/// Connected targets idle for longer than their data source's Auto-disconnect (`last_used`: ms of
/// the last statement / introspection; `busy`: running or holding a transaction).
pub fn idle_targets<'t>(
    connected: impl IntoIterator<Item = &'t str>,
    sources: &[DataSource],
    last_used: &HashMap<String, u64>,
    busy: impl Fn(&str) -> bool,
    now: u64,
) -> Vec<String> {
    let mut out = Vec::new();
    for t in connected {
        let id = split_target(t).0;
        let secs = sources.iter().find(|s| s.id == id).and_then(|s| s.options.auto_disconnect_secs);
        let Some(secs) = secs.filter(|&s| s > 0) else { continue };
        if busy(t) {
            continue;
        }
        let used = last_used.get(t).copied().unwrap_or(now);
        if now.saturating_sub(used) >= u64::from(secs) * 1000 {
            out.push(t.to_string());
        }
    }
    out
}

/// A data source id made of `random` (a UUID or other random text): letters, digits and `-`,
/// at most 36 of them.
pub fn new_source_id(random: &str) -> String {
    random.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(36).collect()
}

/// What a failed connection needs from the user before retrying.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectRemedy {
    Password,
    SshSecret,
    TrustHostKey,
}

static CHANGED_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^SSH host key .* has changed").expect("the pattern is valid"));

/// Classifies a connection error: an unknown SSH host key to confirm, a missing or rejected SSH
/// password / passphrase, or a database password. A changed host key is never offered for trust.
pub fn connect_remedy(kind: Option<ErrorKind>, message: &str, ssh: Option<&DbSsh>) -> Option<ConnectRemedy> {
    if message.starts_with("Unknown SSH host key") {
        return Some(ConnectRemedy::TrustHostKey);
    }
    if kind != Some(ErrorKind::PermissionDenied) || CHANGED_KEY.is_match(message) {
        return None;
    }
    if ["SSH password required", "SSH key passphrase required", "cannot decrypt SSH key"]
        .iter()
        .any(|start| message.starts_with(start))
    {
        return Some(ConnectRemedy::SshSecret);
    }
    if message.starts_with("SSH authentication failed") {
        // With OpenSSH config + agent there is nothing to type unless a key is encrypted.
        return (ssh.map(|s| s.auth) != Some(DbSshAuth::OpenSsh)).then_some(ConnectRemedy::SshSecret);
    }
    Some(ConnectRemedy::Password)
}

/// The SSH `user@host:port` of a data source, for prompts.
pub fn ssh_label(ssh: Option<&DbSsh>) -> String {
    let Some(ssh) = ssh.filter(|s| !s.host.is_empty()) else { return String::new() };
    let user = if ssh.user.is_empty() { String::new() } else { format!("{}@", ssh.user) };
    let port = ssh.port.map(|p| format!(":{p}")).unwrap_or_default();
    format!("{user}{}{port}", ssh.host)
}

/// The SSH configuration a tunnel connects with; `None` for the inline fields or a missing one.
pub fn ssh_config_of<'c>(ssh: Option<&DbSsh>, configs: &'c [SshConfiguration]) -> Option<&'c SshConfiguration> {
    let id = ssh?.config.trim();
    if id.is_empty() {
        return None;
    }
    configs.iter().find(|c| c.id == id)
}

/// The SSH settings in effect: those of the chosen SSH configuration, else the inline fields.
pub fn effective_ssh(ssh: Option<&DbSsh>, configs: &[SshConfiguration]) -> Option<DbSsh> {
    let ssh = ssh?;
    Some(match ssh_config_of(Some(ssh), configs) {
        Some(c) => DbSsh {
            host: c.host.clone(),
            port: c.port,
            user: c.user.clone(),
            auth: c.auth,
            key_file: c.key_file.clone(),
            save_secret: c.save_secret,
            ..ssh.clone()
        },
        None => ssh.clone(),
    })
}

/// Whether the SSH tab names a host to tunnel through (always, when the tunnel is off).
pub fn ssh_tunnel_ready(ssh: Option<&DbSsh>, configs: &[SshConfiguration]) -> bool {
    let Some(ssh) = ssh.filter(|s| s.enabled) else { return true };
    if !ssh.config.trim().is_empty() {
        return ssh_config_of(Some(ssh), configs).is_some_and(|c| !c.host.trim().is_empty());
    }
    !ssh.host.trim().is_empty()
}

/// A connection target: a data source id for its configured database, or `<id>:<database>` for
/// another database of the same server.
pub fn split_target(target: &str) -> (&str, Option<&str>) {
    ide_db::split_target(target)
}

/// The target of database `database` of data source `id`; the connected one is the data source itself.
pub fn target_of(id: &str, database: Option<&str>, current: Option<&str>) -> String {
    match database {
        Some(db) if !db.is_empty() && Some(db) != current => format!("{id}:{db}"),
        _ => id.to_string(),
    }
}

/// Key of the Database tree node of database `name` of data source `id`.
pub fn database_key(id: &str, name: &str) -> String {
    format!("{id}/@{name}")
}

fn ssl_mode(name: &str) -> Option<DbSslMode> {
    Some(match name.to_lowercase().as_str() {
        "disable" | "disabled" => DbSslMode::Disable,
        "prefer" | "preferred" => DbSslMode::Prefer,
        "require" | "required" => DbSslMode::Require,
        "verify-ca" | "verify_ca" => DbSslMode::VerifyCa,
        "verify-full" | "verify_identity" | "verify-identity" => DbSslMode::VerifyFull,
        _ => return None,
    })
}

/// The fields Data Source from URL fills; `None` leaves a field as it is. `port: Some(None)`
/// clears the port (the driver's default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFields {
    pub driver: Option<DbDriver>,
    pub vendor: Option<String>,
    pub name: Option<String>,
    pub file: Option<String>,
    pub host: Option<String>,
    pub port: Option<Option<u16>>,
    pub user: Option<String>,
    pub database: Option<String>,
    pub ssl: Option<DbSslMode>,
    pub ssl_ca_file: Option<String>,
    pub ssl_cert_file: Option<String>,
    pub ssl_key_file: Option<String>,
    pub properties: Option<Vec<DbProperty>>,
}

impl SourceFields {
    /// Writes the filled fields into `source`.
    pub fn apply(&self, source: &mut DataSource) {
        if let Some(v) = self.driver {
            source.driver = v;
        }
        let text = |field: &Option<String>, slot: &mut String| {
            if let Some(v) = field {
                slot.clone_from(v);
            }
        };
        text(&self.vendor, &mut source.vendor);
        text(&self.name, &mut source.name);
        text(&self.host, &mut source.host);
        text(&self.user, &mut source.user);
        text(&self.database, &mut source.database);
        text(&self.ssl_ca_file, &mut source.ssl_ca_file);
        text(&self.ssl_cert_file, &mut source.ssl_cert_file);
        text(&self.ssl_key_file, &mut source.ssl_key_file);
        if let Some(file) = &self.file {
            source.file = Some(file.clone().into());
        }
        if let Some(port) = self.port {
            source.port = port;
        }
        if let Some(ssl) = self.ssl {
            source.ssl = ssl;
        }
        if let Some(properties) = &self.properties {
            source.properties.clone_from(properties);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUrl {
    pub source: SourceFields,
    pub password: Option<String>,
}

static SQLITE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^sqlite3?:(?://)?(.+)$").expect("the pattern is valid"));
static THIN_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:oracle|oracledb):thin:@(.+)$").expect("the pattern is valid"));
static SERVER_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^([a-z][a-z0-9]*)://(?:([^:@/]*)(?::([^@/]*))?@)?([^:/?,]+|\[[^\]]+\])(?::([0-9]+))?(?:,[^/?]*)?(?:/([^?]*))?(?:\?(.*))?$",
    )
    .expect("the pattern is valid")
});

/// `%XX` escapes decoded as UTF-8; `None` for a malformed escape or bytes that are not UTF-8.
fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A URL component decoded, the text itself when it is not valid percent-encoding.
fn dec(text: Option<&str>) -> String {
    let text = text.unwrap_or("");
    decode_uri_component(text).unwrap_or_else(|| text.to_string())
}

/// `name=value&…` pairs of a query string, as the web's URLSearchParams reads them (`+` is a
/// space, malformed escapes stay as written).
fn query_pairs(query: &str) -> Vec<(String, String)> {
    fn decode(text: &str) -> String {
        let bytes = text.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let escaped = bytes
                .get(i + 1..i + 3)
                .filter(|h| bytes[i] == b'%' && h.iter().all(u8::is_ascii_hexdigit))
                .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
            match escaped {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

/// Data Source from URL (and the General tab's URL field): `postgres://user:pass@host:5432/db`,
/// `mysql://…`, `mariadb://…`, wire-compatible products (`redshift://`, `tidb://`…), the `jdbc:`
/// forms of those, `jdbc:sqlite:path` / `sqlite:path`, ClickHouse `http(s)://`. SSL parameters fill
/// the SSL tab, other parameters become driver properties. `None` when the URL names no supported
/// driver.
pub fn parse_source_url(raw: &str) -> Option<ParsedUrl> {
    let trimmed = raw.trim();
    let url = match trimmed.get(..5) {
        Some(p) if p.eq_ignore_ascii_case("jdbc:") => &trimmed[5..],
        _ => trimmed,
    };
    if let Some(m) = SQLITE_URL.captures(url) {
        let path = &m[1];
        let bytes = path.as_bytes();
        let file = if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
            &path[1..]
        } else {
            path
        };
        return Some(ParsedUrl {
            source: SourceFields {
                driver: Some(DbDriver::Sqlite),
                file: Some(file.to_string()),
                name: Some(file_source_name(file)),
                ..SourceFields::default()
            },
            password: None,
        });
    }
    // JDBC thin URLs: `jdbc:oracle:thin:@//host:port/service`.
    let thin = THIN_URL.captures(url).map(|t| format!("oracle://{}", t[1].trim_start_matches('/')));
    let m = SERVER_URL.captures(thin.as_deref().unwrap_or(url))?;
    let scheme = m.get(1).map_or("", |g| g.as_str());
    let vendor = vendor_of_scheme(scheme)?;
    if vendor.driver == DbDriver::Sqlite {
        return None;
    }
    let port = match m.get(5) {
        // An out-of-range port makes the whole URL invalid.
        Some(p) => Some(p.as_str().parse::<u16>().ok().filter(|&p| p > 0)?),
        None => None,
    };
    let mut user = dec(m.get(2).map(|g| g.as_str()));
    let mut password = m.get(3).map(|g| dec(Some(g.as_str())));
    let host_raw = m.get(4).map_or("", |g| g.as_str());
    let host = host_raw.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host_raw);
    let database = dec(m.get(6).map(|g| g.as_str()));
    let mut source = SourceFields {
        driver: Some(vendor.driver),
        host: Some(host.to_string()),
        port: Some(port),
        database: Some(database.strip_suffix('/').unwrap_or(&database).to_string()),
        vendor: (vendor.id != super::vendors::driver_id(vendor.driver)).then(|| vendor.id.to_string()),
        ..SourceFields::default()
    };
    let mut properties = Vec::new();
    for (k, v) in query_pairs(m.get(7).map_or("", |g| g.as_str())) {
        match k.to_lowercase().as_str() {
            "user" => {
                if user.is_empty() {
                    user = v;
                }
            }
            "password" => {
                password.get_or_insert(v);
            }
            "sslmode" | "ssl-mode" => {
                if let Some(mode) = ssl_mode(&v) {
                    source.ssl = Some(mode);
                }
            }
            "sslrootcert" | "ssl-ca" => source.ssl_ca_file = Some(v),
            "sslcert" | "ssl-cert" => source.ssl_cert_file = Some(v),
            "sslkey" | "ssl-key" => source.ssl_key_file = Some(v),
            _ => properties.push(DbProperty { name: k, value: v }),
        }
    }
    source.user = Some(user);
    if vendor.driver == DbDriver::Clickhouse {
        if scheme.eq_ignore_ascii_case("https") {
            source.ssl = Some(DbSslMode::VerifyFull);
        } else if scheme.eq_ignore_ascii_case("http") {
            source.ssl = Some(DbSslMode::Disable);
        }
    }
    if !properties.is_empty() {
        source.properties = Some(properties);
    }
    Some(ParsedUrl { source, password })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Cell {
        Some(v.to_string())
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn copies_tsv_with_null_as_empty_and_whitespace_flattened() {
        let rows = vec![vec![s("1"), None], vec![s("x\ty"), s("line\nbreak")]];
        assert_eq!(to_tsv(Some(&strings(&["a", "b"])), &rows), "a\tb\n1\t\nx y\tline break");
        assert_eq!(to_tsv(None, &[vec![s("1")]]), "1");
    }

    #[test]
    fn copies_rfc_4180_csv() {
        let rows = vec![vec![s("1"), s("say \"hi\", ok")], vec![s("2"), None]];
        assert_eq!(to_csv(Some(&strings(&["id", "note"])), &rows), "id,note\r\n1,\"say \"\"hi\"\", ok\"\r\n2,");
    }

    #[test]
    fn turns_pending_changes_into_keyed_updates_deletes_and_inserts() {
        let columns = strings(&["id", "name", "note"]);
        let rows = vec![vec![s("1"), s("ann"), None], vec![s("2"), s("bob"), s("x")]];
        let mut e = PendingEdits::default();
        assert!(!e.has_edits());
        set_value(e.updates_of_mut(0), "note", s("hello"));
        set_value(e.updates_of_mut(1), "name", s("bobby"));
        e.deletes.insert(1);
        e.inserts.push(vec![("name".to_string(), s("cy"))]);
        assert!(e.has_edits());
        let cell = |column: &str, value: &str| CellValue { column: column.into(), value: s(value) };
        assert_eq!(
            build_row_edits(&columns, &rows, &strings(&["id"]), &e),
            [
                DbRowEdit::Update { key: vec![cell("id", "1")], values: vec![cell("note", "hello")] },
                DbRowEdit::Delete { key: vec![cell("id", "2")] },
                DbRowEdit::Insert { values: vec![cell("name", "cy")] },
            ]
        );
    }

    #[test]
    fn labels_data_sources() {
        assert_eq!(source_label("app", DbDriver::Postgres, ""), "app (PostgreSQL)");
    }

    fn ssh(auth: DbSshAuth) -> DbSsh {
        DbSsh { enabled: true, host: "bastion".into(), auth, ..DbSsh::default() }
    }

    #[test]
    fn asks_for_what_the_failed_connection_needs() {
        let denied = Some(ErrorKind::PermissionDenied);
        assert_eq!(
            connect_remedy(
                Some(ErrorKind::Protocol),
                "Unknown SSH host key for bastion:22 (ssh-ed25519 SHA256:abc)",
                None
            ),
            Some(ConnectRemedy::TrustHostKey)
        );
        let pw = ssh(DbSshAuth::Password);
        assert_eq!(
            connect_remedy(denied, "SSH password required for me@bastion:22", Some(&pw)),
            Some(ConnectRemedy::SshSecret)
        );
        assert_eq!(
            connect_remedy(denied, "SSH key passphrase required for /k", Some(&ssh(DbSshAuth::KeyPair))),
            Some(ConnectRemedy::SshSecret)
        );
        assert_eq!(
            connect_remedy(denied, "SSH authentication failed for me@bastion:22", Some(&pw)),
            Some(ConnectRemedy::SshSecret)
        );
        assert_eq!(
            connect_remedy(denied, "SSH authentication failed for me@bastion:22", Some(&ssh(DbSshAuth::OpenSsh))),
            None
        );
        assert_eq!(
            connect_remedy(denied, "[28P01] password authentication failed", None),
            Some(ConnectRemedy::Password)
        );
        assert_eq!(connect_remedy(Some(ErrorKind::Io), "connection refused", None), None);
    }

    #[test]
    fn never_offers_to_trust_a_changed_host_key() {
        let message = "SSH host key of bastion:22 has changed (known_hosts line 3); possible attack";
        assert_eq!(connect_remedy(Some(ErrorKind::PermissionDenied), message, None), None);
    }

    #[test]
    fn labels_the_ssh_host() {
        let ssh = DbSsh { host: "bastion".into(), user: "ops".into(), port: Some(2222), ..DbSsh::default() };
        assert_eq!(ssh_label(Some(&ssh)), "ops@bastion:2222");
        assert_eq!(ssh_label(None), "");
    }

    #[test]
    fn names_the_default_database_by_the_source_id_alone() {
        assert_eq!(target_of("pg", None, Some("app")), "pg");
        assert_eq!(target_of("pg", Some("app"), Some("app")), "pg");
        assert_eq!(target_of("pg", Some("other"), Some("app")), "pg:other");
    }

    #[test]
    fn splits_a_target_back_into_source_and_database() {
        assert_eq!(split_target("pg"), ("pg", None));
        assert_eq!(split_target("pg:my:db"), ("pg", Some("my:db")));
    }

    #[test]
    fn reads_postgresql_mysql_urls_plain_and_jdbc() {
        assert_eq!(
            parse_source_url("postgresql://ann:s%40cret@db.local:5433/shop?sslmode=require"),
            Some(ParsedUrl {
                source: SourceFields {
                    driver: Some(DbDriver::Postgres),
                    host: Some("db.local".into()),
                    port: Some(Some(5433)),
                    user: Some("ann".into()),
                    database: Some("shop".into()),
                    ssl: Some(DbSslMode::Require),
                    ..SourceFields::default()
                },
                password: Some("s@cret".into()),
            })
        );
        assert_eq!(
            parse_source_url("jdbc:mysql://localhost/app?user=root"),
            Some(ParsedUrl {
                source: SourceFields {
                    driver: Some(DbDriver::Mysql),
                    host: Some("localhost".into()),
                    port: Some(None),
                    user: Some("root".into()),
                    database: Some("app".into()),
                    ..SourceFields::default()
                },
                password: None,
            })
        );
    }

    #[test]
    fn reads_sqlite_and_oracle_urls_and_rejects_other_drivers() {
        assert_eq!(
            parse_source_url("jdbc:sqlite:C:/data/shop.db").map(|p| p.source),
            Some(SourceFields {
                driver: Some(DbDriver::Sqlite),
                file: Some("C:/data/shop.db".into()),
                name: Some("shop".into()),
                ..SourceFields::default()
            })
        );
        let oracle = |host: &str, port: u16, user: &str, database: &str| SourceFields {
            driver: Some(DbDriver::Oracle),
            host: Some(host.into()),
            port: Some(Some(port)),
            user: Some(user.into()),
            database: Some(database.into()),
            ..SourceFields::default()
        };
        assert_eq!(
            parse_source_url("oracle://scott:tiger@db.local:1521/ORCLPDB1"),
            Some(ParsedUrl { source: oracle("db.local", 1521, "scott", "ORCLPDB1"), password: Some("tiger".into()) })
        );
        assert_eq!(
            parse_source_url("jdbc:oracle:thin:@//db.local:1521/ORCL").map(|p| p.source),
            Some(oracle("db.local", 1521, "", "ORCL"))
        );
        assert_eq!(parse_source_url("unknown://x"), None);
    }

    #[test]
    fn reads_urls_of_compatible_products_ssl_settings_and_driver_properties() {
        let tidb = parse_source_url(
            "jdbc:tidb://root@tidb.local:4000/test?sslmode=verify-full&sslrootcert=/ca.pem&sql_mode=ANSI",
        );
        assert_eq!(
            tidb.map(|p| p.source),
            Some(SourceFields {
                driver: Some(DbDriver::Mysql),
                vendor: Some("tidb".into()),
                host: Some("tidb.local".into()),
                port: Some(Some(4000)),
                user: Some("root".into()),
                database: Some("test".into()),
                ssl: Some(DbSslMode::VerifyFull),
                ssl_ca_file: Some("/ca.pem".into()),
                properties: Some(vec![DbProperty { name: "sql_mode".into(), value: "ANSI".into() }]),
                ..SourceFields::default()
            })
        );
        assert_eq!(
            parse_source_url("redshift://r.example:5439/dev").and_then(|p| p.source.vendor).as_deref(),
            Some("redshift")
        );
        assert_eq!(parse_source_url("postgresql://h:70000/db"), None);
    }

    #[test]
    fn matches_names_and_star_question_mark_patterns() {
        assert!(matches_pattern("app_1", "app_*"));
        assert!(matches_pattern("app.1", "app?1"));
        assert!(!matches_pattern("apps", "app"));
        assert!(matches_pattern("a.b", "a.b"));
    }

    #[test]
    fn shows_all_names_or_the_chosen_ones_and_the_connected_database() {
        let names = strings(&["app", "app_test", "billing", "postgres"]);
        assert_eq!(shown_names(&names, None, None, None), names);
        assert_eq!(shown_names(&names, Some(true), Some(&strings(&["billing"])), None), names);
        assert_eq!(
            shown_names(&names, Some(false), Some(&strings(&["app*"])), Some("postgres")),
            strings(&["app", "app_test", "postgres"])
        );
        assert!(shown_names(&names, Some(false), Some(&[]), None).is_empty());
    }

    #[test]
    fn quick_filters_tree_objects_by_a_case_insensitive_substring() {
        assert!(passes_tree_filter("users", Some("user")));
        assert!(passes_tree_filter("Users", Some("user")));
        assert!(passes_tree_filter("order_items", Some("  IT  ")));
        assert!(!passes_tree_filter("users", Some("orders")));
        assert!(passes_tree_filter("users", Some("")));
        assert!(passes_tree_filter("users", Some("   ")));
        assert!(passes_tree_filter("users", None));
    }

    #[test]
    fn follows_the_console_s_pick_then_the_connection_default() {
        assert_eq!(active_schema(Some("app"), Some("public")), "app");
        assert_eq!(active_schema(None, Some("public")), "public");
        assert_eq!(active_schema(Some(""), Some("public")), "public");
        assert_eq!(active_schema(None, None), "");
    }

    #[test]
    fn offers_the_schema_dropdown_to_multi_schema_drivers_only() {
        for driver in [DbDriver::Postgres, DbDriver::Mysql, DbDriver::SqlServer, DbDriver::Oracle] {
            assert!(schema_switchable(Some(driver)));
        }
        for driver in [DbDriver::Sqlite, DbDriver::Clickhouse] {
            assert!(!schema_switchable(Some(driver)));
        }
        assert!(!schema_switchable(None));
    }

    #[test]
    fn finds_connections_idle_past_their_auto_disconnect() {
        let source = |id: &str, secs: Option<u32>| {
            let mut s = DataSource { id: id.into(), ..DataSource::default() };
            s.options.auto_disconnect_secs = secs;
            s
        };
        let sources = [source("a", Some(60)), source("b", None)];
        let used: HashMap<String, u64> =
            [("a", 0), ("a:other", 50_000), ("b", 0)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let idle =
            |busy: &[&str]| idle_targets(["a", "a:other", "b", "c"], &sources, &used, |t| busy.contains(&t), 60_000);
        assert_eq!(idle(&[]), ["a"]);
        assert!(idle(&["a"]).is_empty());
    }

    #[test]
    fn names_the_colour_tokens() {
        assert_eq!(color_token(Some(DbColor::Red)).as_deref(), Some("--db-color-red"));
        assert_eq!(color_token(None), None);
    }

    fn configs() -> Vec<SshConfiguration> {
        vec![
            SshConfiguration {
                id: "dev".into(),
                name: "Dev".into(),
                host: "box".into(),
                port: Some(2222),
                user: "me".into(),
                auth: DbSshAuth::Password,
                ..SshConfiguration::default()
            },
            SshConfiguration { id: "blank".into(), name: "Blank".into(), ..SshConfiguration::default() },
        ]
    }

    fn inline() -> DbSsh {
        DbSsh {
            enabled: true,
            config: String::new(),
            host: "bastion".into(),
            port: None,
            user: "ops".into(),
            auth: DbSshAuth::OpenSsh,
            ..DbSsh::default()
        }
    }

    #[test]
    fn uses_the_chosen_ssh_configuration_instead_of_the_inline_fields() {
        let configs = configs();
        let ssh = DbSsh { config: "dev".into(), ..inline() };
        assert_eq!(ssh_config_of(Some(&ssh), &configs).map(|c| c.id.as_str()), Some("dev"));
        let used = effective_ssh(Some(&ssh), &configs).expect("settings in effect");
        assert_eq!(ssh_label(Some(&used)), "me@box:2222");
        assert_eq!(used.auth, DbSshAuth::Password);
        assert_eq!(used.config, "dev");
    }

    #[test]
    fn keeps_the_inline_fields_when_no_configuration_or_a_missing_one_is_chosen() {
        let configs = configs();
        assert_eq!(effective_ssh(Some(&inline()), &configs), Some(inline()));
        let gone = DbSsh { config: "gone".into(), ..inline() };
        assert_eq!(ssh_config_of(Some(&gone), &configs), None);
        assert_eq!(effective_ssh(Some(&gone), &configs).map(|s| s.host), Some("bastion".to_string()));
        assert_eq!(effective_ssh(None, &configs), None);
    }

    #[test]
    fn needs_a_host_to_tunnel_through_only_when_the_tunnel_is_on() {
        let configs = configs();
        let ready = |ssh: DbSsh| ssh_tunnel_ready(Some(&ssh), &configs);
        assert!(ready(DbSsh { enabled: false, host: String::new(), ..inline() }));
        assert!(ready(inline()));
        assert!(!ready(DbSsh { host: " ".into(), ..inline() }));
        assert!(ready(DbSsh { host: String::new(), config: "dev".into(), ..inline() }));
        assert!(!ready(DbSsh { config: "blank".into(), ..inline() }));
        assert!(!ready(DbSsh { config: "gone".into(), ..inline() }));
    }

    #[test]
    fn classifies_auth_failures_by_the_configuration_s_authentication() {
        let configs = configs();
        let msg = "SSH authentication failed for me@box:2222";
        let denied = Some(ErrorKind::PermissionDenied);
        assert_eq!(connect_remedy(denied, msg, Some(&inline())), None);
        let used = effective_ssh(Some(&DbSsh { config: "dev".into(), ..inline() }), &configs);
        assert_eq!(connect_remedy(denied, msg, used.as_ref()), Some(ConnectRemedy::SshSecret));
    }

    #[test]
    fn reads_numbers_as_javascript_does() {
        assert_eq!(js_number(" 12 "), Some(12.0));
        assert_eq!(js_number(""), Some(0.0));
        assert_eq!(js_number("0x1f"), Some(31.0));
        assert_eq!(js_number("inf"), None);
        assert_eq!(js_number("1_000"), None);
        assert!(!is_js_finite("Infinity"));
        assert_eq!(js_number_string(1.5), "1.5");
        assert_eq!(js_number_string(1e21), "1e+21");
        assert_eq!(js_number_string(7.0), "7");
    }
}
