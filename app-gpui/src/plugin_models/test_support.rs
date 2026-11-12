//! Listings for the models' tests.

use ide_host::plugins::PluginInfo;
use serde_json::{Value, json};

/// An installed, enabled, active plugin `id` without capabilities or contributions, with `fields`
/// (camelCase, as listed) replacing the defaults.
pub fn plugin(id: &str, fields: Value) -> PluginInfo {
    let mut info = json!({
        "id": id, "name": id, "version": "1", "description": "", "authors": [],
        "homepage": null, "changeNotes": null, "icon": null, "source": "installed", "path": format!("/plugins/{id}"),
        "enabled": true, "status": "active", "error": null, "hasCode": true, "capabilities": [],
        "contributions": [], "activation": [], "warnings": [], "stats": {
            "activations": 0, "activationMs": 0.0, "calls": 0, "errors": 0, "maxCallMs": 0.0,
            "p95CallMs": 0.0, "slowCalls": 0, "crashes": 0, "reclaims": 0,
        },
        "publisher": null, "previousVersion": null,
    });
    if let (Some(info), Some(fields)) = (info.as_object_mut(), fields.as_object()) {
        info.extend(fields.clone());
    }
    serde_json::from_value(info).expect("a plugin listing")
}

/// A granted or refused capability of a listing.
pub fn capability(id: &str, granted: bool) -> Value {
    json!({ "id": id, "risk": "high", "description": "", "optional": false, "granted": granted })
}
