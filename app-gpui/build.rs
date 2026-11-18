//! Embeds the icon set: every SVG under `src/ui/expui`, `src/ui/openidea` and `src/ui/brands` of
//! the webview UI, keyed by the path the icon table uses (`expui/nodes/folder.svg`,
//! `openidea/vcs/stash.svg`, `brands/rust.svg`), and passes the IDE's release version, the
//! repository workspace's, as `OPENIDEA_VERSION`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "svg") {
            out.push(path);
        }
    }
}

/// The `version` of the `[workspace.package]` table of the repository's manifest: the version the
/// IDE is released under, which this crate (a workspace of its own) does not inherit.
fn workspace_version(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
        } else if in_package && let Some(value) = line.strip_prefix("version") {
            return value.trim_start().strip_prefix('=')?.trim().strip_prefix('"')?.split('"').next();
        }
    }
    None
}

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
    let version =
        workspace_version(&text).unwrap_or_else(|| panic!("{}: no [workspace.package] version", manifest.display()));
    println!("cargo:rustc-env=OPENIDEA_VERSION={version}");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let ui =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ui").canonicalize().expect("src/ui exists next to app-gpui");
    let mut table = String::from("pub static ICON_FILES: &[(&str, &[u8])] = &[\n");
    let mut count = 0;
    for set in ["expui", "openidea", "brands"] {
        let root = ui.join(set);
        println!("cargo:rerun-if-changed={}", root.display());
        let mut files = Vec::new();
        collect(&root, &mut files);
        for file in files {
            let key = file.strip_prefix(&ui).expect("under src/ui").to_string_lossy().replace('\\', "/");
            writeln!(table, "    ({key:?}, include_bytes!({:?})),", file.display().to_string())
                .expect("writing to a string");
            count += 1;
        }
    }
    table.push_str("];\n");
    assert!(count > 500, "only {count} icons found under {}", ui.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("icon_files.rs");
    std::fs::write(&out, table).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    println!("cargo:rerun-if-changed=build.rs");
}
