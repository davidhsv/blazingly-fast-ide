//! Data source types of New ▸ Data Source: the drivers the backend speaks, and products that speak
//! their wire protocol (a preset of port, icon and URL scheme).

use ide_db::{DataSource, DbDriver, DbSslMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DbVendor {
    /// Stored in `DataSource::vendor`; a driver's own entry has the driver's id.
    pub id: &'static str,
    pub label: &'static str,
    pub driver: DbDriver,
    pub port: Option<u16>,
    pub icon: &'static str,
    /// JDBC URL scheme (`jdbc:<scheme>://…`).
    pub scheme: &'static str,
    /// Managed services that only accept encrypted connections.
    pub ssl: Option<DbSslMode>,
}

const fn v(
    id: &'static str,
    label: &'static str,
    driver: DbDriver,
    port: Option<u16>,
    icon: &'static str,
    scheme: &'static str,
) -> DbVendor {
    DbVendor { id, label, driver, port, icon, scheme, ssl: None }
}

const fn secure(vendor: DbVendor) -> DbVendor {
    DbVendor { ssl: Some(DbSslMode::Require), ..vendor }
}

/// Every type, sorted by label.
pub static VENDORS: [DbVendor; 18] = [
    v("aurora-mysql", "Amazon Aurora MySQL", DbDriver::Mysql, Some(3306), "db-aurora", "mysql"),
    secure(v("redshift", "Amazon Redshift", DbDriver::Postgres, Some(5439), "db-redshift", "redshift")),
    v("clickhouse", "ClickHouse", DbDriver::Clickhouse, Some(8123), "db-clickhouse", "clickhouse"),
    v("cockroachdb", "CockroachDB", DbDriver::Postgres, Some(26257), "db-cockroach", "postgresql"),
    v("greenplum", "Greenplum", DbDriver::Postgres, Some(5432), "db-greenplum", "postgresql"),
    v("mariadb", "MariaDB", DbDriver::Mysql, Some(3306), "db-mariadb", "mariadb"),
    v("sqlServer", "Microsoft SQL Server", DbDriver::SqlServer, Some(1433), "db-sqlserver", "sqlserver"),
    v("mysql", "MySQL", DbDriver::Mysql, Some(3306), "db-mysql", "mysql"),
    secure(v("neon", "Neon", DbDriver::Postgres, Some(5432), "db-neon", "postgresql")),
    v("oceanbase", "OceanBase (MySQL mode)", DbDriver::Mysql, Some(2881), "db-oceanbase", "oceanbase"),
    v("oracle", "Oracle", DbDriver::Oracle, Some(1521), "db-oracle", "oracle"),
    v("postgres", "PostgreSQL", DbDriver::Postgres, Some(5432), "db-postgres", "postgresql"),
    v("singlestore", "SingleStore", DbDriver::Mysql, Some(3306), "db-singlestore", "singlestore"),
    v("sqlite", "SQLite", DbDriver::Sqlite, None, "db-sqlite", "sqlite"),
    secure(v("supabase", "Supabase", DbDriver::Postgres, Some(5432), "db-supabase", "postgresql")),
    v("tidb", "TiDB", DbDriver::Mysql, Some(4000), "db-tidb", "mysql"),
    v("timescaledb", "TimescaleDB", DbDriver::Postgres, Some(5432), "db-timescale", "postgresql"),
    v("yugabytedb", "YugabyteDB", DbDriver::Postgres, Some(5433), "db-yugabyte", "postgresql"),
];

/// The id a driver has on the wire (and as its own type's id).
pub fn driver_id(driver: DbDriver) -> &'static str {
    match driver {
        DbDriver::Sqlite => "sqlite",
        DbDriver::Postgres => "postgres",
        DbDriver::Mysql => "mysql",
        DbDriver::SqlServer => "sqlServer",
        DbDriver::Clickhouse => "clickhouse",
        DbDriver::Oracle => "oracle",
    }
}

pub fn vendor_by_id(id: &str) -> Option<&'static DbVendor> {
    VENDORS.iter().find(|v| v.id == id)
}

/// The type of a data source: its preset, else its driver's own entry.
pub fn vendor_of(vendor: &str, driver: DbDriver) -> &'static DbVendor {
    match vendor_by_id(vendor) {
        Some(v) if !vendor.is_empty() && v.driver == driver => v,
        // Every driver has its own entry (SQLite's is the last resort).
        _ => vendor_by_id(driver_id(driver)).unwrap_or(&VENDORS[13]),
    }
}

/// The fields a new data source of a type starts with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorPreset {
    pub driver: DbDriver,
    /// Empty for a driver's own type.
    pub vendor: String,
    pub port: Option<u16>,
    pub ssl: Option<DbSslMode>,
    pub user: Option<String>,
    pub database: Option<String>,
}

impl VendorPreset {
    /// Writes the preset into `source`.
    pub fn apply(&self, source: &mut DataSource) {
        source.driver = self.driver;
        source.vendor.clone_from(&self.vendor);
        source.port = self.port;
        if let Some(ssl) = self.ssl {
            source.ssl = ssl;
        }
        if let Some(user) = &self.user {
            source.user.clone_from(user);
        }
        if let Some(database) = &self.database {
            source.database.clone_from(database);
        }
    }
}

pub fn vendor_preset(v: &DbVendor) -> VendorPreset {
    let mut preset = VendorPreset {
        driver: v.driver,
        vendor: if v.id == driver_id(v.driver) { String::new() } else { v.id.to_string() },
        port: v.port,
        ssl: v.ssl,
        user: None,
        database: None,
    };
    if v.driver == DbDriver::Clickhouse {
        preset.user = Some("default".into());
        preset.database = Some("default".into());
        preset.ssl = Some(DbSslMode::Disable);
    }
    preset
}

/// The type a URL scheme (JDBC or plain) names.
pub fn vendor_of_scheme(scheme: &str) -> Option<&'static DbVendor> {
    let id = match scheme.to_lowercase().as_str() {
        "clickhouse" | "http" | "https" => "clickhouse",
        "postgres" | "postgresql" | "pgsql" => "postgres",
        "redshift" => "redshift",
        "cockroachdb" | "cockroach" => "cockroachdb",
        "yugabytedb" => "yugabytedb",
        "timescaledb" => "timescaledb",
        "greenplum" => "greenplum",
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "tidb" => "tidb",
        "singlestore" | "memsql" => "singlestore",
        "oceanbase" => "oceanbase",
        "oracle" | "oracledb" => "oracle",
        "sqlserver" | "mssql" => "sqlServer",
        _ => return None,
    };
    vendor_by_id(id)
}

/// `encodeURIComponent`: everything but letters, digits and `-_.!~*'()` percent-encoded as UTF-8.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn bracketed(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() }
}

/// The JDBC URL of a data source's fields (the General tab's URL preview).
pub fn source_url(s: &DataSource) -> String {
    let v = vendor_of(&s.vendor, s.driver);
    if v.driver == DbDriver::Sqlite {
        let file = s.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        return format!("jdbc:sqlite:{file}");
    }
    let host = s.host.trim();
    let host = bracketed(if host.is_empty() { "localhost" } else { host });
    let db = if s.database.is_empty() { String::new() } else { format!("/{}", encode_uri_component(&s.database)) };
    if v.driver == DbDriver::Clickhouse {
        let secure = !matches!(s.ssl, DbSslMode::Disable | DbSslMode::Prefer);
        let port = s.port.unwrap_or(if secure { 8443 } else { 8123 });
        return format!("{}://{host}:{port}{db}", if secure { "https" } else { "http" });
    }
    let port = s.port.or(v.port).map(|p| format!(":{p}")).unwrap_or_default();
    format!("jdbc:{}://{host}{port}{db}", v.scheme)
}

#[cfg(test)]
mod tests {
    use super::super::model::{parse_source_url, source_label};
    use super::*;
    use crate::platform::collate;

    fn vendor(id: &str) -> &'static DbVendor {
        vendor_by_id(id).expect("a known type")
    }

    #[test]
    fn presets_wire_compatible_products_on_the_existing_drivers() {
        let preset = |driver, vendor: &str, port| VendorPreset {
            driver,
            vendor: vendor.into(),
            port: Some(port),
            ssl: None,
            user: None,
            database: None,
        };
        assert_eq!(vendor_preset(vendor("cockroachdb")), preset(DbDriver::Postgres, "cockroachdb", 26257));
        assert_eq!(vendor_preset(vendor("postgres")), preset(DbDriver::Postgres, "", 5432));
        assert_eq!(vendor_preset(vendor("supabase")).ssl, Some(DbSslMode::Require));
        assert_eq!(vendor_preset(vendor("oracle")), preset(DbDriver::Oracle, "", 1521));
        assert!(VENDORS.iter().all(|v| vendor_by_id(driver_id(v.driver)).is_some()));
        let labels: Vec<&str> = VENDORS.iter().map(|v| v.label).collect();
        let mut sorted = labels.clone();
        sorted.sort_by(|a, b| collate::compare(a, b));
        assert_eq!(labels, sorted);
    }

    #[test]
    fn falls_back_to_the_driver_for_unknown_or_mismatched_presets() {
        assert_eq!(vendor_of("tidb", DbDriver::Mysql).label, "TiDB");
        assert_eq!(vendor_of("tidb", DbDriver::Postgres).id, "postgres");
        assert_eq!(vendor_of("", DbDriver::Mysql).id, "mysql");
        assert_eq!(source_label("tidb", DbDriver::Mysql, "tidb"), "tidb (TiDB)");
    }

    #[test]
    fn previews_the_jdbc_url_of_the_fields() {
        let source = |driver, vendor: &str, host: &str, port, database: &str| DataSource {
            driver,
            vendor: vendor.into(),
            host: host.into(),
            port,
            database: database.into(),
            ..DataSource::default()
        };
        assert_eq!(
            source_url(&source(DbDriver::Postgres, "", "localhost", Some(5432), "postgres")),
            "jdbc:postgresql://localhost:5432/postgres"
        );
        assert_eq!(source_url(&source(DbDriver::Postgres, "redshift", "r", None, "")), "jdbc:redshift://r:5439");
        assert_eq!(
            source_url(&source(DbDriver::Mysql, "mariadb", "::1", Some(3307), "a b")),
            "jdbc:mariadb://[::1]:3307/a%20b"
        );
        let sqlite = DataSource { file: Some(String::from("app.db").into()), ..DataSource::default() };
        assert_eq!(source_url(&sqlite), "jdbc:sqlite:app.db");
    }

    #[test]
    fn adds_clickhouse_with_native_http_defaults_and_https_cloud_urls() {
        assert_eq!(
            vendor_preset(vendor("clickhouse")),
            VendorPreset {
                driver: DbDriver::Clickhouse,
                vendor: String::new(),
                port: Some(8123),
                ssl: Some(DbSslMode::Disable),
                user: Some("default".into()),
                database: Some("default".into()),
            }
        );
        let ch = |ssl, host: &str, database: &str| DataSource {
            driver: DbDriver::Clickhouse,
            ssl,
            host: host.into(),
            database: database.into(),
            ..DataSource::default()
        };
        assert_eq!(source_url(&ch(DbSslMode::Prefer, "localhost", "default")), "http://localhost:8123/default");
        assert_eq!(
            source_url(&ch(DbSslMode::VerifyFull, "db.example", "analytics")),
            "https://db.example:8443/analytics"
        );
        let parsed = parse_source_url("https://reader:s%40cret@db.example:8443/analytics?max_execution_time=30")
            .expect("a ClickHouse URL");
        assert_eq!(parsed.source.driver, Some(DbDriver::Clickhouse));
        assert_eq!(parsed.source.host.as_deref(), Some("db.example"));
        assert_eq!(parsed.source.port, Some(Some(8443)));
        assert_eq!(parsed.source.database.as_deref(), Some("analytics"));
        assert_eq!(parsed.source.user.as_deref(), Some("reader"));
        assert_eq!(parsed.source.ssl, Some(DbSslMode::VerifyFull));
        assert_eq!(parsed.password.as_deref(), Some("s@cret"));
        let ssl = |url: &str| parse_source_url(url).and_then(|p| p.source.ssl);
        assert_eq!(ssl("http://localhost/default"), Some(DbSslMode::Disable));
        let driver = parse_source_url("jdbc:clickhouse://localhost/default").and_then(|p| p.source.driver);
        assert_eq!(driver, Some(DbDriver::Clickhouse));
        assert_eq!(ssl("https://db.example/default?sslmode=disable"), Some(DbSslMode::VerifyFull));
    }
}
