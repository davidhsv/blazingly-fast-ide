//! System Settings › HTTP Proxy as the page edits it. The shape, the defaults and the check of
//! a manual configuration are the backend's ([`ProxySettings`], [`ProxySettings::problem`]).

use ide_core::settings::{ProxyKind, ProxySettings};

/// `proxy` with its kind switched between HTTP and SOCKS: a port that was the old kind's
/// default moves along, a custom one stays.
pub fn with_proxy_kind(proxy: &ProxySettings, kind: ProxyKind) -> ProxySettings {
    let port = if proxy.port == proxy.kind.default_port() { kind.default_port() } else { proxy.port };
    ProxySettings { kind, port, ..proxy.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::settings::{ProxyMode, Settings};
    use serde_json::{Value, json};

    fn parsed(raw: Value) -> ProxySettings {
        serde_json::from_value(raw).expect("the backend takes the proxy settings")
    }

    fn manual(kind: ProxyKind, port: u16) -> ProxySettings {
        ProxySettings { mode: ProxyMode::Manual, kind, host: "proxy.corp".to_owned(), port, ..ProxySettings::default() }
    }

    // validateProxy › defaults to auto-detect
    #[test]
    fn defaults_to_auto_detect() {
        assert_eq!(parsed(json!({})), ProxySettings::default());
        assert_eq!(ProxySettings::default().mode, ProxyMode::Auto);
        assert_eq!(Settings::default().proxy, ProxySettings::default());
    }

    // validateProxy › keeps valid values
    #[test]
    fn keeps_valid_values() {
        let raw = json!({
            "mode": "manual",
            "kind": "socks",
            "host": "10.0.0.1",
            "port": 1081,
            "noProxy": "*.example.com, 192.168.*",
            "authenticate": true,
            "login": "me",
            "password": "pw",
        });
        assert_eq!(serde_json::to_value(parsed(raw.clone())).ok(), Some(raw));
        assert_eq!(parsed(json!({ "mode": "none" })).mode, ProxyMode::None);
    }

    // validateProxy › falls back on malformed values
    #[test]
    fn falls_back_on_malformed_values() {
        // Missing values take their defaults; a value of the wrong type is refused where the
        // settings are stored instead of being replaced.
        assert_eq!(parsed(json!({ "login": "me" })), ProxySettings { login: "me".to_owned(), ..Default::default() });
        for refused in [
            json!({ "mode": "pac" }),
            json!({ "kind": "ftp" }),
            json!({ "host": 7 }),
            json!({ "port": 70000 }),
            json!({ "port": 80.5 }),
            json!({ "authenticate": "yes" }),
            json!({ "login": null }),
            json!("manual"),
        ] {
            assert!(serde_json::from_value::<ProxySettings>(refused.clone()).is_err(), "{refused}");
        }
    }

    // withProxyKind › moves a default port along and keeps a custom one
    #[test]
    fn moves_a_default_port_along_and_keeps_a_custom_one() {
        let socks = with_proxy_kind(&manual(ProxyKind::Http, 80), ProxyKind::Socks);
        assert_eq!((socks.kind, socks.port), (ProxyKind::Socks, 1080));
        assert_eq!(with_proxy_kind(&manual(ProxyKind::Socks, 1080), ProxyKind::Http).port, 80);
        assert_eq!(with_proxy_kind(&manual(ProxyKind::Http, 3128), ProxyKind::Socks).port, 3128);
    }
}
