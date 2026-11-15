//! JDK discovery limited to the homes a caller names. One test in its own binary: it sets the
//! process's JDK candidates and download folders.
#![cfg(unix)]

use ide_sdk::java::{add_jdk_root, installed_jdks, jdk_at, rescan_jdks, set_jdk_candidates};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// A JDK home: its `release` file and a `bin/java` that only fails (nothing starts it).
fn jdk(home: &Path, version: &str) {
    std::fs::create_dir_all(home.join("bin")).unwrap();
    std::fs::write(home.join("release"), format!("JAVA_VERSION=\"{version}\"\n")).unwrap();
    let java = home.join("bin/java");
    std::fs::write(&java, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn discovery_finds_only_the_named_homes_and_the_download_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let tmp = dunce::canonicalize(tmp.path()).unwrap();
    let (a, b, empty) = (tmp.join("a"), tmp.join("b"), tmp.join("empty"));
    jdk(&a, "21.0.2");
    jdk(&b, "17.0.10");
    std::fs::create_dir_all(&empty).unwrap();
    let at = |home: &Path| jdk_at(home).expect("a JDK");

    set_jdk_candidates(Some(vec![a.clone(), empty.clone()]));
    assert_eq!(*installed_jdks(), [at(&a)], "only the named JDK, and a folder that is none is left out");

    let downloads = tmp.join("downloads");
    let downloaded = downloads.join("temurin-25");
    jdk(&downloaded, "25");
    add_jdk_root(&downloads);
    assert_eq!(*rescan_jdks(), [at(&a), at(&downloaded)], "and the IDE's downloads");

    set_jdk_candidates(Some(vec![b.clone()]));
    assert_eq!(*installed_jdks(), [at(&b), at(&downloaded)], "the list found before is forgotten");
}
