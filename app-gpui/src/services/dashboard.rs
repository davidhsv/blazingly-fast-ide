//! What the Docker dashboards of the Services tool window read out of docker's answers: sizes and
//! percentages of `docker stats`, creation times, published ports, image history lines and the
//! `docker inspect` payloads of containers, images and networks.
//!
//! Every parser is total: text docker did not produce reads as zero, empty or `None`, never as an
//! error, since a dashboard shows what it can.

use crate::platform::collate;
use crate::platform::local_time::utc_offset_minutes;
use chrono::{DateTime, Datelike as _, FixedOffset, Timelike as _};
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]+)?$").expect("the pattern is valid"));
static LEADING_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?").expect("the pattern is valid")
});
static ZONE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+[A-Z]{2,5}$").expect("the pattern is valid"));
static BUILDKIT_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*#\s*buildkit\s*$").expect("the pattern is valid"));

// ---------------------------------------------------------------------------------------------------
// Sizes and percentages

/// Bytes per unit, binary (`MiB`, `Mi`) and decimal (`MB`, `M`), as docker and Kubernetes print them.
fn unit_multiplier(suffix: &str) -> Option<f64> {
    const KI: f64 = 1024.0;
    const K: f64 = 1000.0;
    let multiplier = match suffix {
        "B" => 1.0,
        "KiB" | "Ki" => KI,
        "MiB" | "Mi" => KI.powi(2),
        "GiB" | "Gi" => KI.powi(3),
        "TiB" | "Ti" => KI.powi(4),
        "PiB" | "Pi" => KI.powi(5),
        "kB" | "KB" | "K" | "k" => K,
        "MB" | "M" => K.powi(2),
        "GB" | "G" => K.powi(3),
        "TB" | "T" => K.powi(4),
        "PB" => K.powi(5),
        _ => return None,
    };
    Some(multiplier)
}

/// `431MB`, `50MiB`, `12 kB` in bytes; 0 for anything else. An unknown unit counts as bytes.
pub fn units_to_bytes(value: &str) -> f64 {
    let Some(found) = SIZE.captures(value.trim()) else { return 0.0 };
    let Some(number) = found.get(1).and_then(|number| number.as_str().parse::<f64>().ok()) else { return 0.0 };
    let Some(suffix) = found.get(2).map(|suffix| suffix.as_str()) else { return number };
    let bare = suffix.strip_suffix(['B', 'b']).unwrap_or(suffix);
    unit_multiplier(suffix).or_else(|| unit_multiplier(bare)).map_or(number, |multiplier| number * multiplier)
}

/// Bytes in the largest binary unit that keeps the number at 1 or more (`50.0 MiB`); whole bytes
/// below 1 KiB, `0 B` for nothing.
pub fn bytes_to_units(bytes: f64, precision: usize) -> String {
    if !bytes.is_finite() || bytes <= 0.0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let (mut value, mut unit) = (bytes, 0);
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} B", value.round()) } else { format!("{value:.precision$} {}", UNITS[unit]) }
}

/// `12.5%` as 12.5; 0 for a negative or missing number.
pub fn parse_percent(raw: &str) -> f64 {
    let text = raw.replace('%', "");
    let number = LEADING_NUMBER.find(text.trim()).and_then(|number| number.as_str().parse::<f64>().ok());
    number.filter(|number| number.is_finite() && *number >= 0.0).unwrap_or(0.0)
}

/// A `used / limit` pair of `docker stats` (`512MiB / 2GiB`).
#[derive(Clone, Debug, PartialEq)]
pub struct UsagePair {
    pub used: String,
    pub limit: String,
    pub used_bytes: f64,
    pub limit_bytes: f64,
    /// Used of the limit, at most 100; 0 without a limit.
    pub percent: f64,
}

pub fn parse_usage_pair(raw: &str) -> UsagePair {
    let mut parts = raw.split('/').map(str::trim);
    let part = |part: Option<&str>| part.filter(|part| !part.is_empty()).unwrap_or("0 B").to_string();
    let used = part(parts.next());
    let limit = part(parts.next());
    let (used_bytes, limit_bytes) = (units_to_bytes(&used), units_to_bytes(&limit));
    let percent = if limit_bytes > 0.0 { (used_bytes / limit_bytes * 100.0).min(100.0) } else { 0.0 };
    UsagePair { used, limit, used_bytes, limit_bytes, percent }
}

// ---------------------------------------------------------------------------------------------------
// Creation times

/// An RFC 3339 time (`docker inspect`) or the CLI's `2026-10-01 09:12:44 +0200 CEST`, in Unix
/// milliseconds; `None` for docker's zero time and for anything else.
fn parse_docker_timestamp(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with("0001-01-01") {
        return None;
    }
    if let Ok(moment) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(moment.timestamp_millis());
    }
    let cleaned = ZONE_NAME.replace(trimmed, "");
    ["%Y-%m-%d %H:%M:%S%.f %z", "%Y-%m-%d %H:%M:%S %z"]
        .iter()
        .find_map(|format| DateTime::parse_from_str(&cleaned, format).ok())
        .map(|moment| moment.timestamp_millis())
}

/// The zone of an offset east of UTC: `UTC`, `UTC+02:00`, `UTC-05:30`.
fn zone_label(offset_minutes: i32) -> String {
    if offset_minutes == 0 {
        return "UTC".to_string();
    }
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let minutes = offset_minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// A creation time with its age: `2d 2h ago (2026-10-01 15:00:00 UTC)`. The date shows at
/// `offset_minutes` east of UTC, the user's local offset at that moment when `None`. `—` without a
/// time; text that is no time comes back as it is.
pub fn format_created_detailed(raw: Option<&str>, now_ms: f64, offset_minutes: Option<i32>) -> String {
    let trimmed = raw.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() || trimmed.starts_with("0001-01-01") {
        return "—".to_string();
    }
    let Some(ms) = parse_docker_timestamp(trimmed) else { return trimmed.to_string() };
    let elapsed = ((now_ms - ms as f64) / 1000.0).floor().max(0.0) as u64;
    let (days, hours, minutes) = (elapsed / 86_400, elapsed % 86_400 / 3_600, elapsed % 3_600 / 60);
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 || parts.is_empty() {
        parts.push(format!("{minutes}m"));
    }
    let age = parts.join(" ");
    let offset_minutes = offset_minutes.unwrap_or_else(|| utc_offset_minutes(ms as f64));
    let shown = FixedOffset::east_opt(offset_minutes * 60)
        .zip(DateTime::from_timestamp_millis(ms))
        .map(|(offset, moment)| moment.with_timezone(&offset));
    let Some(d) = shown else { return format!("{age} ago ({trimmed})") };
    let date =
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second());
    format!("{age} ago ({date} {})", zone_label(offset_minutes))
}

// ---------------------------------------------------------------------------------------------------
// Ports

/// A published (`host_port`) or only exposed port of a container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortMapping {
    pub host_ip: String,
    pub host_port: Option<String>,
    pub container_port: String,
    pub protocol: String,
    /// Where a published TCP port opens in the browser.
    pub url: Option<String>,
    pub label: String,
}

/// `80/tcp` as port and protocol; `tcp` when the protocol is left out.
fn port_spec(spec: &str) -> (&str, &str) {
    let mut parts = spec.split('/');
    let port = parts.next().unwrap_or(spec);
    (port, parts.next().unwrap_or("tcp"))
}

/// The published and exposed ports of `docker ps`'s `Ports` column (`0.0.0.0:8080->80/tcp, 5432/tcp`)
/// and of `docker inspect`'s `NetworkSettings.Ports`, the inspected ones first, each once.
pub fn parse_port_mappings(ports_text: &str, inspect_ports: Option<&Map<String, Value>>) -> Vec<PortMapping> {
    let mut out: Vec<PortMapping> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut push = |host_ip: &str, host_port: Option<&str>, container_port: &str, protocol: &str| {
        let host_ip = if host_ip == "::" { "[::]" } else { host_ip };
        let host_port = host_port.filter(|port| !port.is_empty());
        let key = format!("{host_ip}:{}->{container_port}/{protocol}", host_port.unwrap_or_default());
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        let browse_host = if matches!(host_ip, "" | "0.0.0.0" | "[::]") { "localhost" } else { host_ip };
        let url = host_port.filter(|_| protocol == "tcp").map(|port| format!("http://{browse_host}:{port}"));
        let label = match host_port {
            Some(port) => {
                let ip = if host_ip.is_empty() { "0.0.0.0" } else { host_ip };
                format!("{ip}:{port} -> {container_port}/{protocol}")
            }
            None => format!("{container_port}/{protocol}"),
        };
        out.push(PortMapping {
            host_ip: host_ip.to_string(),
            host_port: host_port.map(str::to_string),
            container_port: container_port.to_string(),
            protocol: protocol.to_string(),
            url,
            label,
        });
    };
    for (spec, bindings) in inspect_ports.into_iter().flatten() {
        let (port, protocol) = port_spec(spec);
        match bindings.as_array().filter(|bindings| !bindings.is_empty()) {
            Some(bindings) => {
                for binding in bindings.iter().filter_map(Value::as_object) {
                    let ip = binding.get("HostIp").and_then(Value::as_str).unwrap_or("0.0.0.0");
                    let host_port = binding.get("HostPort").and_then(Value::as_str);
                    push(ip, host_port, port, protocol);
                }
            }
            None => push("", None, port, protocol),
        }
    }
    for segment in ports_text.split(',').map(str::trim).filter(|segment| !segment.is_empty()) {
        match segment.find("->").filter(|arrow| *arrow > 0) {
            Some(arrow) => {
                let (left, right) = (&segment[..arrow], &segment[arrow + 2..]);
                let (port, protocol) = port_spec(right);
                let (ip, host_port) = left.rsplit_once(':').unwrap_or(("0.0.0.0", left));
                push(ip, Some(host_port), port, protocol);
            }
            None => {
                let (port, protocol) = port_spec(segment);
                push("", None, port, protocol);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------
// Image layers

const DOCKERFILE_INSTRUCTIONS: [&str; 18] = [
    "FROM",
    "RUN",
    "CMD",
    "LABEL",
    "MAINTAINER",
    "EXPOSE",
    "ENV",
    "ADD",
    "COPY",
    "ENTRYPOINT",
    "VOLUME",
    "USER",
    "WORKDIR",
    "ARG",
    "ONBUILD",
    "STOPSIGNAL",
    "HEALTHCHECK",
    "SHELL",
];

/// A layer of `docker image history` as the Dockerfile instruction that made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInstruction {
    /// `RUN`, `CMD`…, or `LAYER` for a single word that is no instruction.
    pub instruction: String,
    pub args: String,
}

/// Cleans a layer's `CreatedBy` (`/bin/sh -c #(nop)  CMD ["postgres"]`, `RUN … # buildkit`) into its
/// instruction and arguments.
pub fn split_layer_instruction(created_by: &str) -> LayerInstruction {
    let cleaned = BUILDKIT_SUFFIX.replace(created_by.trim(), "");
    let mut text: &str = &cleaned;
    let layer = |instruction: &str, args: &str| LayerInstruction {
        instruction: instruction.to_string(),
        args: args.to_string(),
    };
    if let Some(rest) = text.strip_prefix("/bin/sh -c #(nop)") {
        text = rest.trim();
    } else if let Some(body) = text.strip_prefix("/bin/sh -c") {
        let body = body.trim();
        return layer("RUN", if body.is_empty() { text } else { body });
    }
    let Some((first, rest)) = text.split_once(char::is_whitespace) else {
        let upper = text.to_uppercase();
        let known = DOCKERFILE_INSTRUCTIONS.contains(&upper.as_str());
        return if known { layer(&upper, "") } else { layer("LAYER", text) };
    };
    let first = first.to_uppercase();
    if DOCKERFILE_INSTRUCTIONS.contains(&first.as_str()) { layer(&first, rest.trim()) } else { layer("RUN", text) }
}

// ---------------------------------------------------------------------------------------------------
// Inspect payloads

/// A key-value pair shown in a table (environment, labels, options).
pub type Entry = (String, String);

type Object = Map<String, Value>;

/// The object `docker inspect` printed: the first of a list, or the value itself.
fn inspected(raw: Option<&str>) -> Option<Object> {
    let parsed: Value = serde_json::from_str(raw?).ok()?;
    match parsed {
        Value::Array(items) => items.into_iter().next()?.as_object().cloned(),
        Value::Object(object) => Some(object),
        _ => None,
    }
}

fn object<'a>(of: &'a Object, key: &str) -> Option<&'a Object> {
    of.get(key).and_then(Value::as_object)
}

fn text(of: &Object, key: &str) -> String {
    of.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn text_in(of: Option<&Object>, key: &str) -> String {
    of.map(|of| text(of, key)).unwrap_or_default()
}

fn strings(of: Option<&Object>, key: &str) -> Vec<String> {
    let items = of.and_then(|of| of.get(key)).and_then(Value::as_array);
    items.into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

/// `KEY=value` entries as pairs; an entry without `=` has an empty value.
fn env_entries(of: Option<&Object>, key: &str) -> Vec<Entry> {
    let split = |entry: String| match entry.split_once('=') {
        Some((name, value)) => (name.to_string(), value.to_string()),
        None => (entry, String::new()),
    };
    strings(of, key).into_iter().map(split).collect()
}

/// The text, number and boolean members of an object, sorted by key.
fn record_entries(of: Option<&Object>, key: &str) -> Vec<Entry> {
    let Some(record) = of.and_then(|of| object(of, key)) else { return Vec::new() };
    let mut out: Vec<Entry> = record
        .iter()
        .filter_map(|(name, value)| {
            let value = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => return None,
            };
            Some((name.clone(), value))
        })
        .collect();
    out.sort_by(|a, b| collate::compare(&a.0, &b.0));
    out
}

/// The member names of an object, in code unit order.
fn sorted_keys(of: Option<&Object>, key: &str) -> Vec<String> {
    let record = of.and_then(|of| object(of, key));
    let mut keys: Vec<String> = record.map(|record| record.keys().cloned().collect()).unwrap_or_default();
    keys.sort();
    keys
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerMountInfo {
    /// `volume`, `bind`, `tmpfs`.
    pub kind: String,
    /// The volume's name; `None` for a bind mount.
    pub name: Option<String>,
    pub source: String,
    pub destination: String,
    pub mode: String,
    pub rw: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerNetworkInfo {
    pub name: String,
    pub ip_address: String,
    pub gateway: String,
    pub mac_address: String,
}

/// What a container's dashboard shows from `docker container inspect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedContainerInspect {
    pub created: String,
    pub image_id: String,
    /// `no`, `always`, `on-failure:3`.
    pub restart_policy: String,
    pub network_mode: String,
    pub privileged: bool,
    /// The main process; `None` while the container does not run.
    pub pid: Option<i64>,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub finished_at: String,
    pub health_status: Option<String>,
    pub user: String,
    pub working_dir: String,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub env: Vec<Entry>,
    pub mounts: Vec<ContainerMountInfo>,
    /// In the order of the payload's member names.
    pub networks: Vec<ContainerNetworkInfo>,
    pub ports: Vec<PortMapping>,
    pub labels: Vec<Entry>,
}

/// A container's inspect payload; `fallback_ports` (`docker ps`'s `Ports` column) adds the ports the
/// payload does not list. `None` for text that is no inspect payload.
pub fn parse_container_inspect(raw: Option<&str>, fallback_ports: &str) -> Option<ParsedContainerInspect> {
    let obj = inspected(raw)?;
    let state = object(&obj, "State");
    let config = object(&obj, "Config");
    let host_config = object(&obj, "HostConfig");
    let net_settings = object(&obj, "NetworkSettings");
    let restart = host_config.and_then(|host| object(host, "RestartPolicy"));
    let restart_name = Some(text_in(restart, "Name")).filter(|name| !name.is_empty()).unwrap_or_else(|| "no".into());
    let retries = restart.and_then(|restart| restart.get("MaximumRetryCount")).and_then(Value::as_f64);
    let retries = retries.filter(|count| *count > 0.0).map(|count| format!(":{count}")).unwrap_or_default();
    let health = state.and_then(|state| object(state, "Health"));
    let mounts = obj.get("Mounts").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object);
    let mounts = mounts
        .map(|mount| ContainerMountInfo {
            kind: mount.get("Type").and_then(Value::as_str).unwrap_or("volume").to_string(),
            name: Some(text(mount, "Name")).filter(|name| !name.is_empty()),
            source: text(mount, "Source"),
            destination: text(mount, "Destination"),
            mode: text(mount, "Mode"),
            rw: mount.get("RW") != Some(&Value::Bool(false)),
        })
        .collect();
    let networks = net_settings.and_then(|settings| object(settings, "Networks")).into_iter().flatten();
    let networks = networks
        .filter_map(|(name, network)| network.as_object().map(|network| (name, network)))
        .map(|(name, network)| ContainerNetworkInfo {
            name: name.clone(),
            ip_address: text(network, "IPAddress"),
            gateway: text(network, "Gateway"),
            mac_address: text(network, "MacAddress"),
        })
        .collect();
    fn present<'a>(of: Option<&'a Object>, key: &str) -> Option<&'a Value> {
        of.and_then(|of| of.get(key)).filter(|value| !value.is_null())
    }
    let ports = present(net_settings, "Ports").or_else(|| present(host_config, "PortBindings"));
    Some(ParsedContainerInspect {
        created: text(&obj, "Created"),
        image_id: text(&obj, "Image"),
        restart_policy: format!("{restart_name}{retries}"),
        network_mode: text_in(host_config, "NetworkMode"),
        privileged: host_config.and_then(|host| host.get("Privileged")) == Some(&Value::Bool(true)),
        pid: state.and_then(|state| state.get("Pid")).and_then(Value::as_i64).filter(|pid| *pid > 0),
        exit_code: state.and_then(|state| state.get("ExitCode")).and_then(Value::as_i64),
        started_at: text_in(state, "StartedAt"),
        finished_at: text_in(state, "FinishedAt"),
        health_status: health.and_then(|health| health.get("Status")).and_then(Value::as_str).map(str::to_string),
        user: text_in(config, "User"),
        working_dir: text_in(config, "WorkingDir"),
        entrypoint: strings(config, "Entrypoint"),
        cmd: strings(config, "Cmd"),
        env: env_entries(config, "Env"),
        mounts,
        networks,
        ports: parse_port_mappings(fallback_ports, ports.and_then(Value::as_object)),
        labels: record_entries(config, "Labels"),
    })
}

/// What an image's dashboard shows from `docker image inspect`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedImageInspect {
    pub created: String,
    /// Bytes.
    pub size: f64,
    pub architecture: String,
    pub os: String,
    pub author: String,
    pub docker_version: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub user: String,
    pub working_dir: String,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    /// `5432/tcp`, sorted.
    pub exposed_ports: Vec<String>,
    /// Mount points the image declares, sorted.
    pub volumes: Vec<String>,
    pub env: Vec<Entry>,
    pub labels: Vec<Entry>,
}

/// An image's inspect payload; `None` for text that is no inspect payload.
pub fn parse_image_inspect(raw: Option<&str>) -> Option<ParsedImageInspect> {
    let obj = inspected(raw)?;
    let config = object(&obj, "Config");
    let root = Some(&obj);
    Some(ParsedImageInspect {
        created: text(&obj, "Created"),
        size: obj.get("Size").and_then(Value::as_f64).unwrap_or(0.0),
        architecture: text(&obj, "Architecture"),
        os: text(&obj, "Os"),
        author: text(&obj, "Author"),
        docker_version: text(&obj, "DockerVersion"),
        repo_tags: strings(root, "RepoTags"),
        repo_digests: strings(root, "RepoDigests"),
        user: text_in(config, "User"),
        working_dir: text_in(config, "WorkingDir"),
        entrypoint: strings(config, "Entrypoint"),
        cmd: strings(config, "Cmd"),
        exposed_ports: sorted_keys(config, "ExposedPorts"),
        volumes: sorted_keys(config, "Volumes"),
        env: env_entries(config, "Env"),
        labels: record_entries(config, "Labels"),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpamConfig {
    pub subnet: String,
    pub gateway: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkContainer {
    pub id: String,
    pub name: String,
    pub ipv4_address: String,
    pub mac_address: String,
}

/// What a network's dashboard shows from `docker network inspect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedNetworkInspect {
    pub created: String,
    /// The address ranges with a subnet or a gateway.
    pub ipam_configs: Vec<IpamConfig>,
    /// The attached containers, sorted by name.
    pub containers: Vec<NetworkContainer>,
    pub options: Vec<Entry>,
    pub labels: Vec<Entry>,
}

/// A network's inspect payload; `None` for text that is no inspect payload.
pub fn parse_network_inspect(raw: Option<&str>) -> Option<ParsedNetworkInspect> {
    let obj = inspected(raw)?;
    let ipam = object(&obj, "IPAM");
    let configs = ipam.and_then(|ipam| ipam.get("Config")).and_then(Value::as_array).into_iter().flatten();
    let ipam_configs = configs
        .filter_map(Value::as_object)
        .map(|config| IpamConfig { subnet: text(config, "Subnet"), gateway: text(config, "Gateway") })
        .filter(|config| !config.subnet.is_empty() || !config.gateway.is_empty())
        .collect();
    let attached = object(&obj, "Containers").into_iter().flatten();
    let mut containers: Vec<NetworkContainer> = attached
        .filter_map(|(id, container)| container.as_object().map(|container| (id, container)))
        .map(|(id, container)| NetworkContainer {
            id: id.clone(),
            name: container
                .get("Name")
                .and_then(Value::as_str)
                .map_or_else(|| ide_docker::short_id(id).to_string(), str::to_string),
            ipv4_address: text(container, "IPv4Address"),
            mac_address: text(container, "MacAddress"),
        })
        .collect();
    containers.sort_by(|a, b| collate::compare(&a.name, &b.name));
    let root = Some(&obj);
    Some(ParsedNetworkInspect {
        created: text(&obj, "Created"),
        ipam_configs,
        containers,
        options: record_entries(root, "Options"),
        labels: record_entries(root, "Labels"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units_usage_pairs_timestamps_ports_layers_and_inspect_payloads() {
        assert_eq!(units_to_bytes("50MiB"), 50.0 * 1024.0 * 1024.0);
        assert_eq!(units_to_bytes("431MB"), 431.0 * 1000.0 * 1000.0);
        assert_eq!(bytes_to_units(50.0 * 1024.0 * 1024.0, 1), "50.0 MiB");
        assert_eq!(parse_percent("12.5%"), 12.5);
        let pair = parse_usage_pair("512MiB / 2GiB");
        assert!((pair.percent - 25.0).abs() < 0.05, "{}", pair.percent);

        let now = parse_docker_timestamp("2026-10-03T17:00:00Z").expect("a time") as f64;
        let formatted = format_created_detailed(Some("2026-10-01T15:00:00Z"), now, Some(0));
        assert!(formatted.contains("2d 2h ago"), "{formatted}");
        assert!(formatted.contains("2026-10-01 15:00:00"), "{formatted}");
        let local = format_created_detailed(Some("2026-10-01T15:00:00Z"), now, None);
        assert!(local.contains("2d 2h ago"), "{local}");

        let ports = parse_port_mappings("0.0.0.0:8080->80/tcp, 5432/tcp", None);
        assert_eq!(
            ports,
            vec![
                PortMapping {
                    host_ip: "0.0.0.0".into(),
                    host_port: Some("8080".into()),
                    container_port: "80".into(),
                    protocol: "tcp".into(),
                    url: Some("http://localhost:8080".into()),
                    label: "0.0.0.0:8080 -> 80/tcp".into(),
                },
                PortMapping {
                    host_ip: String::new(),
                    host_port: None,
                    container_port: "5432".into(),
                    protocol: "tcp".into(),
                    url: None,
                    label: "5432/tcp".into(),
                },
            ]
        );

        assert_eq!(
            split_layer_instruction(r#"/bin/sh -c #(nop)  CMD ["postgres"]"#),
            LayerInstruction { instruction: "CMD".into(), args: r#"["postgres"]"#.into() }
        );
        assert_eq!(
            split_layer_instruction("RUN /bin/sh -c apt-get update # buildkit"),
            LayerInstruction { instruction: "RUN".into(), args: "/bin/sh -c apt-get update".into() }
        );

        let container = r#"[{
            "Created": "2026-10-01T15:00:00Z",
            "Image": "sha256:1111",
            "State": {
                "Pid": 4321, "ExitCode": 0, "StartedAt": "2026-10-01T15:00:01Z", "Health": { "Status": "healthy" }
            },
            "Config": {
                "User": "postgres",
                "WorkingDir": "/var/lib/postgresql",
                "Entrypoint": ["docker-entrypoint.sh"],
                "Cmd": ["postgres"],
                "Env": ["POSTGRES_DB=shop", "PGDATA=/var/lib/postgresql/data"],
                "Labels": { "com.docker.compose.project": "shop" }
            },
            "HostConfig": { "RestartPolicy": { "Name": "unless-stopped", "MaximumRetryCount": 0 } },
            "Mounts": [{
                "Type": "volume",
                "Name": "shop_pgdata",
                "Source": "/var/lib/docker/volumes/shop_pgdata/_data",
                "Destination": "/var/lib/postgresql/data",
                "Mode": "z",
                "RW": true
            }],
            "NetworkSettings": {
                "Ports": { "5432/tcp": [{ "HostIp": "127.0.0.1", "HostPort": "5432" }] },
                "Networks": { "shop_default": { "IPAddress": "172.18.0.2", "Gateway": "172.18.0.1" } }
            }
        }]"#;
        let inspect = parse_container_inspect(Some(container), "").expect("a container payload");
        assert_eq!(inspect.pid, Some(4321));
        assert_eq!(inspect.health_status.as_deref(), Some("healthy"));
        assert_eq!(inspect.restart_policy, "unless-stopped");
        let env: Vec<Entry> =
            vec![("POSTGRES_DB".into(), "shop".into()), ("PGDATA".into(), "/var/lib/postgresql/data".into())];
        assert_eq!(inspect.env, env);
        assert_eq!(inspect.mounts[0].name.as_deref(), Some("shop_pgdata"));
        assert_eq!(inspect.ports[0].url.as_deref(), Some("http://127.0.0.1:5432"));
        assert_eq!(inspect.networks[0].ip_address, "172.18.0.2");

        let image = r#"{
            "Created": "2026-09-01T08:00:00Z",
            "Size": 451936256,
            "Architecture": "arm64",
            "Os": "linux",
            "RepoTags": ["postgres:16"],
            "Config": {
                "ExposedPorts": { "5432/tcp": {} },
                "Entrypoint": ["docker-entrypoint.sh"],
                "Cmd": ["postgres"],
                "Env": ["PATH=/usr/local/bin"]
            }
        }"#;
        let image = parse_image_inspect(Some(image)).expect("an image payload");
        assert_eq!(image.os, "linux");
        assert_eq!(image.architecture, "arm64");
        assert_eq!(image.exposed_ports, vec!["5432/tcp".to_string()]);

        let network = r#"{
            "Created": "2026-10-01T09:00:00Z",
            "IPAM": { "Config": [{ "Subnet": "172.18.0.0/16", "Gateway": "172.18.0.1" }] },
            "Containers": { "c1": { "Name": "shop-db-1", "IPv4Address": "172.18.0.2/16" } }
        }"#;
        let network = parse_network_inspect(Some(network)).expect("a network payload");
        assert_eq!(network.ipam_configs[0].subnet, "172.18.0.0/16");
        assert_eq!(network.containers[0].name, "shop-db-1");
    }
}
