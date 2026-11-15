# Build tools (Maven and Gradle tool windows): reconciled integration notes

Supersedes `docs/gpui-port/notes/integration/build_tools_integration.md` (left unedited). Every anchor below is text quoted from the file as it is today (read on 2026-11-15); no line numbers. Draft files: `app-gpui/src/build_tools/{mod,model,view,execute}.rs`, `app-gpui/src/workbench/build_tools.rs`.

Conventions: "Read:" = checked by reading the code in this session. "Hypothesis:" = not verified, with the command that settles it. Nothing was compiled or run.

---

# Part 1. The backend seam

## 1.0 The design, and why

Two process-wide switches with today's behaviour as their default, next to the process-wide state that is already there (`EXTRA_ROOTS` and `JDKS` in `ide-sdk/src/java.rs`):

- `ide_build::maven::set_maven_user_home(Option<PathBuf>)`: the folder that stands for `~/.m2`. `default_local_repo()` and the empty-setting branch of `user_settings_file()` read it, and so does Build Project's fingerprint of `~/.m2/settings.xml` / `toolchains.xml`. `sync_maven` calls `default_local_repo()` unconditionally (`crates/ide-build/src/sync.rs`, `maven::default_local_repo(),` inside `sync_maven`), so the sync needs no code change.
- `ide_sdk::java::set_jdk_candidates(Option<Vec<PathBuf>>)`: when set, `scan()` looks only at those homes and at the folders of `add_jdk_root`. It skips `JAVA_HOME`, the `java` on `PATH`, `/Library/Java/...`, `~/Library/Java/...`, `~/.jdks`, SDKMAN and the mise/asdf installs. Setting it drops the cached scan, so the result does not depend on test order.
- `ide_host::ops::build::isolate_from_user_home(m2, jdks)` forwards to both. `app-gpui` has no `ide-sdk` dependency (Read: `app-gpui/Cargo.toml` lists no `ide-sdk`), and this avoids adding one.

Why process-wide and not per call (`SyncOptions`) or per `Host`:

- The value the tests need is the same for the whole test binary: a Maven user folder that does not exist, and no machine JDKs. So the concurrent tests cannot race on it.
- Each test names its own settings file, local repository and JDK through project settings, which the backend already honours per project.
- A per-call field would have to be filled in `ops::build::build_sync`'s `SyncOptions` literal from a new `Host` field. It would still not cover `load_linked` → `default_local_repo`, `maven_goto_declaration` or the JDK scan, which are reached from `ops::build::load` (8 callers), `project.rs` (`cap_java_home`) and `ops::sdk::sdk_locate_tools`.

What stays unchanged for the Tauri app:

- `src-tauri/` calls none of the new functions. Read: `grep -rn "set_maven_user_home\|set_jdk_candidates\|isolate_from_user_home" src-tauri` would print nothing; its only `ide_sdk::java` call is `add_jdk_root`, in `src-tauri/src/lib.rs`.
- No function signature changes, and no type gains a field. `SyncOptions` is not touched.
- No `specta::Type` type is touched, and no `///` comment is added to one, so `src/bindings.ts` comes out identical.

## 1.1 `crates/ide-build/src/maven.rs`

Hunk A: the import. Replace

```rust
use std::sync::{Mutex, OnceLock};
```

with

```rust
use std::sync::{Mutex, OnceLock, RwLock};
```

Hunk B: the switch and the default repository. Replace

```rust
/// `~/.m2/repository`, or `<localRepository>` from `~/.m2/settings.xml`.
pub fn default_local_repo() -> Option<PathBuf> {
    let m2 = dirs::home_dir()?.join(".m2");
    local_repository_of(&m2.join("settings.xml")).or(Some(m2.join("repository")))
}
```

with

```rust
/// The folder that stands for the user's `~/.m2` once [`set_maven_user_home`] names one.
static USER_HOME: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Points every lookup of the Maven user folder (`~/.m2`: its `settings.xml`, `toolchains.xml` and
/// default `repository`) at `m2`; `None` goes back to the user's. For a process that must not read
/// the user's home, such as a test binary; the IDE never calls it.
pub fn set_maven_user_home(m2: Option<PathBuf>) {
    *USER_HOME.write().unwrap_or_else(|e| e.into_inner()) = m2;
}

/// The Maven user folder: the one [`set_maven_user_home`] names, else `~/.m2`.
pub fn maven_user_home() -> Option<PathBuf> {
    let named = USER_HOME.read().unwrap_or_else(|e| e.into_inner()).clone();
    named.or_else(|| Some(dirs::home_dir()?.join(".m2")))
}

/// The repository of the Maven user folder `m2`: `<localRepository>` of its `settings.xml`, else
/// its `repository` folder.
pub fn local_repo_in(m2: &Path) -> PathBuf {
    local_repository_of(&m2.join("settings.xml")).unwrap_or_else(|| m2.join("repository"))
}

/// `~/.m2/repository`, or `<localRepository>` from `~/.m2/settings.xml`.
pub fn default_local_repo() -> Option<PathBuf> {
    maven_user_home().map(|m2| local_repo_in(&m2))
}
```

The result is the same as before when no folder is named. Before: `local_repository_of(..).or(Some(m2.join("repository")))` after `dirs::home_dir()?`. After: the same two steps.

Hunk C: the user settings file. In `pub fn user_settings_file`, replace

```rust
        return Some(dirs::home_dir()?.join(".m2").join("settings.xml"));
```

with

```rust
        return maven_user_home().map(|m2| m2.join("settings.xml"));
```

The `~/` branch below it stays on `dirs::home_dir()`: it is a path the user typed, relative to their home, not the Maven folder. `local_repository_of`'s `${user.home}` substitution also stays: it only builds a string, and no file under the home is read unless a settings file names one.

Hunk D, the unit test, in `mod tests` of `maven.rs`. Add it directly before

```rust
    #[test]
    fn a_settings_file_names_the_repository_to_read_goals_from() {
```

```rust
    #[test]
    fn a_maven_user_folder_holds_its_repository_unless_its_settings_name_another() {
        let tmp = tempfile::tempdir().unwrap();
        let m2 = tmp.path().join("m2");
        assert_eq!(local_repo_in(&m2), m2.join("repository"));
        std::fs::create_dir_all(&m2).unwrap();
        std::fs::write(m2.join("settings.xml"), "<settings><localRepository>/repo</localRepository></settings>")
            .unwrap();
        assert_eq!(local_repo_in(&m2), PathBuf::from("/repo"));
    }
```

It is pure: it never touches the switch, so it cannot race the existing test that asserts `user_settings_file` against the real `dirs::home_dir()`. That test only builds paths; it does not read them.

Who calls the changed functions (signatures unchanged):

- `default_local_repo`: `lib.rs` `load_linked` (`maven_repo.or_else(maven::default_local_repo)`); `sync.rs` `sync_maven`; `maven.rs` (`Resolver::new(local_repo.or_else(default_local_repo), &[])`, `local_repo.map(Path::to_path_buf).or_else(default_local_repo)`, `load_with_repo(root, default_local_repo())`, `load_with_linked(root, default_local_repo(), …)`); `ide-host/src/ops/build.rs` `maven_goto_declaration` (`.or_else(ide_build::maven::default_local_repo)`).
- `user_settings_file`: `lib.rs` `load_linked` only.
- The Tauri adapters (`src-tauri/src/commands/build.rs`) reach these only through `ide_host::ops::build`. They compile unchanged, with no call-site change.

## 1.2 `crates/ide-build/src/compile/fingerprint.rs`

The same folder for Build Project's fingerprint, so that the Build window's tests stop reading `~/.m2` too. The result is the same when unset. Replace

```rust
    if let Some(home) = dirs::home_dir() {
        inputs.extend([home.join(".m2/settings.xml"), home.join(".m2/toolchains.xml")]);
    }
```

with

```rust
    if let Some(m2) = crate::maven::maven_user_home() {
        inputs.extend([m2.join("settings.xml"), m2.join("toolchains.xml")]);
    }
```

If `dirs` has no other use in `fingerprint.rs` afterwards, nothing changes: it is referred to by path, with no `use` line. Read: `grep -n "dirs::" crates/ide-build/src/compile/fingerprint.rs` shows only this one.

## 1.3 New file `crates/ide-build/tests/maven_user_home.rs`

This follows the crate's style for process-wide state: one test in its own binary, like `crates/ide-sdk/tests/version_managers.rs`.

```rust
//! The Maven user folder (`~/.m2`) a caller names in place of the user's. One test in its own
//! binary: it sets the process's Maven user folder.

use ide_build::maven::{default_local_repo, maven_user_home, set_maven_user_home, user_settings_file};
use ide_core::settings::MavenSettings;
use std::path::Path;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_named_maven_user_folder_stands_for_the_users_settings_and_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let m2 = tmp.path().join("m2");
    let root = tmp.path().join("proj");
    set_maven_user_home(Some(m2.clone()));
    assert_eq!(maven_user_home(), Some(m2.clone()));
    assert_eq!(default_local_repo(), Some(m2.join("repository")), "its own repository");
    assert_eq!(user_settings_file(&root, &MavenSettings::default()), Some(m2.join("settings.xml")));

    // Its settings name the repository, and a parent POM outside the project is found there.
    let repo = tmp.path().join("elsewhere");
    let settings = format!("<settings><localRepository>{}</localRepository></settings>", repo.display());
    write(&m2.join("settings.xml"), &settings);
    write(
        &repo.join("org/demo/base/3.0/base-3.0.pom"),
        concat!(
            "<project><groupId>org.demo</groupId><artifactId>base</artifactId><version>3.0</version>",
            "<properties><java.version>17</java.version></properties></project>"
        ),
    );
    write(
        &root.join("pom.xml"),
        concat!(
            "<project><parent><groupId>org.demo</groupId><artifactId>base</artifactId><version>3.0</version>",
            "<relativePath/></parent><artifactId>app</artifactId></project>"
        ),
    );
    assert_eq!(default_local_repo(), Some(repo));
    let model = ide_build::load(&root, &tmp.path().join("cache"), None);
    let maven = model.maven.expect("the project is a Maven project");
    assert_eq!(maven.projects[0].java_version, Some(17), "read from the parent in the named repository");

    set_maven_user_home(None);
    assert_eq!(maven_user_home(), dirs::home_dir().map(|home| home.join(".m2")), "the user's again");
}
```

The last assertion only builds the path; nothing under the home is read. `dirs`, `ide-core` and `tempfile` are dependencies of `ide-build` (Read: `crates/ide-build/Cargo.toml`). The inherited `java.version` → `java_version` is the behaviour `parent_and_bom_from_the_local_repository_and_wrapper` already asserts, through `load_with_repo`.

## 1.4 `crates/ide-sdk/src/java.rs`

Hunk E: the switch, beside the download folders. Replace

```rust
/// Folders the IDE downloads JDKs into ([`add_jdk_root`]).
static EXTRA_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
```

with

```rust
/// Folders the IDE downloads JDKs into ([`add_jdk_root`]).
static EXTRA_ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The JDK homes discovery is limited to once [`set_jdk_candidates`] names them.
static CANDIDATES: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);
```

Hunk F: the scan. Replace

```rust
fn scan() -> Vec<Jdk> {
    let mut homes: Vec<PathBuf> = Vec::new();
```

with

```rust
fn scan() -> Vec<Jdk> {
    let named = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(mut homes) = named {
        let roots = EXTRA_ROOTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for root in &roots {
            homes.extend(jdk_homes(root));
        }
        return distinct_jdks(homes);
    }
    let mut homes: Vec<PathBuf> = Vec::new();
```

Then replace the end of the same function,

```rust
    for (dir, _) in ide_core::version_managers::installs(ide_core::version_managers::Tool::Java) {
        homes.extend([dir.join("Contents/Home"), dir]);
    }
    let mut out: Vec<Jdk> = Vec::new();
    for h in homes {
        if let Some(jdk) = jdk_at(&h)
            && !out.iter().any(|j| j.home == jdk.home)
        {
            out.push(jdk);
        }
    }
    out
}
```

with

```rust
    for (dir, _) in ide_core::version_managers::installs(ide_core::version_managers::Tool::Java) {
        homes.extend([dir.join("Contents/Home"), dir]);
    }
    distinct_jdks(homes)
}

/// The JDKs at `homes`, each once, in their order.
fn distinct_jdks(homes: Vec<PathBuf>) -> Vec<Jdk> {
    let mut out: Vec<Jdk> = Vec::new();
    for h in homes {
        if let Some(jdk) = jdk_at(&h)
            && !out.iter().any(|j| j.home == jdk.home)
        {
            out.push(jdk);
        }
    }
    out
}
```

Hunk G: the setter, after the cache. Replace

```rust
static JDKS: Mutex<Option<Scan>> = Mutex::new(None);
```

with

```rust
static JDKS: Mutex<Option<Scan>> = Mutex::new(None);

/// Limits JDK discovery to `homes` and the folders of [`add_jdk_root`]. `JAVA_HOME`, the `java` on
/// `PATH`, the usual install locations and the version managers' installs are not looked at;
/// `None` looks at them again. The JDKs found before are forgotten. For a process that must not
/// read the machine's JDKs, such as a test binary; the IDE never calls it.
pub fn set_jdk_candidates(homes: Option<Vec<PathBuf>>) {
    let mut named = CANDIDATES.lock().unwrap_or_else(|e| e.into_inner());
    if *named == homes {
        return;
    }
    *named = homes;
    drop(named);
    *JDKS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
```

Lock order: `scan()` runs under `JDKS`, inside `rescan`, and takes `CANDIDATES`, then `EXTRA_ROOTS`, one after the other. The setter releases `CANDIDATES` before it takes `JDKS`, so it never holds both, and no cycle exists.

Callers: `scan` is private. `installed_jdks`, `rescan_jdks` and `jdtls_jdks` are unchanged in signature and, with no candidates named, in result. Tauri compiles unchanged.

## 1.5 New file `crates/ide-sdk/tests/jdk_candidates.rs`

```rust
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
```

`dunce` and `tempfile` are dependencies of `ide-sdk` (Read: `crates/ide-sdk/Cargo.toml`). The test never sets `None`, which would scan the machine.

## 1.6 `crates/ide-host/src/ops/build.rs`

Add directly after

```rust
fn join_err(e: tokio::task::JoinError) -> IdeError {
    IdeError::internal(e.to_string())
}
```

this:

```rust

/// Keeps the build tools of this process off the user's home. Maven's user folder (`~/.m2`) is
/// `m2`, and JDK discovery finds only `jdks` and the IDE's download folders. The IDE never calls it;
/// a process that must not read the user's home (a test binary) calls it before it reads a build.
pub fn isolate_from_user_home(m2: std::path::PathBuf, jdks: Vec<std::path::PathBuf>) {
    ide_build::maven::set_maven_user_home(Some(m2));
    ide_sdk::java::set_jdk_candidates(Some(jdks));
}
```

It is a two-line forwarder, so it has no test of its own: 1.3 and 1.5 test what it forwards to. It is not an IPC command and is not registered in `rpc::Dispatcher`.

## 1.7 Checks after Part 1 (one at a time)

1. `cargo test -p ide-build` (new: `a_maven_user_folder_holds_its_repository_unless_its_settings_name_another`, and the `maven_user_home` binary).
2. `cargo clippy -p ide-build --all-targets -- -D warnings`
3. `cargo test -p ide-sdk` (new: the `jdk_candidates` binary).
4. `cargo clippy -p ide-sdk --all-targets -- -D warnings`
5. `cargo clippy -p ide-host --all-targets -- -D warnings`
6. `cargo test -p openidea export_bindings`
7. `git status --short src/ src-tauri/` must print nothing.

## 1.8 Every other place the build-tools path reads the home or runs a tool, and how the tests avoid it

| Where (Read)                                                                                                                       | What it reads or runs                                                                                                                       | How the native tests avoid it                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ide_build::sync_maven` → `maven::default_local_repo()`                                                                            | `~/.m2/settings.xml`                                                                                                                        | The switch (1.1), set by `app-gpui`'s test backend (2.9) to a folder that does not exist.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `ide_build::load_linked` → `default_local_repo` / `user_settings_file`                                                             | `~/.m2/settings.xml`, its profiles                                                                                                          | `ops::build::load` passes `maven_local_repository(root, settings)`, which is `Some` when the project sets `localRepository`. `user_settings_file` returns the configured `userSettings`. The draft's Maven fixture sets both. The switch covers the rest.                                                                                                                                                                                                                                                                                                                                |
| `ide_sdk::apply::project_jdk` → `pinned::pinned_install(root, Tool::Java)` → `pinned::global_configs()`                            | `~/.config/mise/config.toml` (through `crate::home_dir()`), unless `MISE_GLOBAL_CONFIG_FILE` / `MISE_CONFIG_DIR` / `XDG_CONFIG_HOME` is set | It is reached only when `languages.java.jdk` is empty, and the switch does not cover it. Both fixtures now set `languages.java.jdk` to a stand-in JDK in the temp tools folder (draft edit 3.1.4). `jdk_setting` → `Some(Ok(jdk))` returns before `pinned_install`.                                                                                                                                                                                                                                                                                                                      |
| `ide_sdk::apply::maven_jdk` → `installed_jdks()`                                                                                   | the machine's JDK folders, `JAVA_HOME`, `PATH`                                                                                              | It is reached only when `project_jdk` is `None`. With the stand-in JDK it is not reached; the candidates switch (`Some(vec![])`) also makes it empty.                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `ide_sdk::apply::gradle_build_env` → `installed_jdks()`                                                                            | the same                                                                                                                                    | It is called unconditionally, so the candidates switch covers it. The configured stand-in JDK wins in `gradle_run_jdk` (`configured.or_else(…)`).                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `ops::build::load` → `missing_jdk_message(&installed_jdks(), …)`                                                                   | the same                                                                                                                                    | It is reached only when `jdk` is `None` and the sync failed. The stand-in JDK and the switch cover it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `ide_sdk::apply::maven_command` → `pinned::pinned_install(root, Tool::Maven)`, `mvnd_command` (`~/.sdkman`)                        | mise config, `~/.sdkman`                                                                                                                    | `languages.java.maven.home` names the stand-in `mvn`, so `setting_path` returns before `pinned_install`. `maven.daemon` is off by default, so `mvnd_command` is not called.                                                                                                                                                                                                                                                                                                                                                                                                              |
| `ide_sdk::apply::gradle_command` → `pinned_install(build_dir, Tool::Gradle)`                                                       | mise config                                                                                                                                 | The build's `gradlew` (the stand-in) is a file, so it is returned first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `ide_sdk::apply::gradle_distribution_version`                                                                                      | `<build>/gradle/wrapper/gradle-wrapper.properties`                                                                                          | It is inside the temp project and absent.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `ide_sdk::apply::gradle_env` → `GRADLE_USER_HOME`                                                                                  | nothing, only passed to the launcher                                                                                                        | Not set unless `languages.java.gradle.userHome` is set. The stand-in `gradlew` reads nothing. The Gradle sync in `ide-build` reads no `~/.gradle`. Read: `grep -n "home_dir\|\.gradle" crates/ide-build/src/sync.rs crates/ide-build/src/gradle.rs` shows no home read.                                                                                                                                                                                                                                                                                                                  |
| `ide_sdk::libraries::dependency_groups` → `gradle_user_home()` (`~/.gradle`) and `default_maven_repository()` (`~/.m2/repository`) | External Libraries jars                                                                                                                     | The native app never asks for External Libraries. Measured: `grep -rn "sdk_project_libraries\|ops::sdk::" app-gpui/src` lists only `sdk_locate_tools`, `sdk_python_effective` and `sdk_php_effective`, in `workbench/run_context.rs`.                                                                                                                                                                                                                                                                                                                                                    |
| `ide_sdk::locate` (`sdk_locate_tools`) → `jdk_tool(name, &installed_jdks())`                                                       | the JDK scan                                                                                                                                | The candidates switch covers it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `ide-host/src/project.rs` → `cap_java_home(cfg, &installed_jdks())`                                                                | the JDK scan                                                                                                                                | The candidates switch covers it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Running `mvn`                                                                                                                      | the real Maven                                                                                                                              | The sync uses `options.maven_command`, which is `maven_command(root, settings)` = the stand-in. A generated goal goes through `ops::run::start`, which calls `prepare_maven_run` for `kind == "maven"` (Read: `crates/ide-host/src/ops/run.rs`). `prepare_maven_run` sets the command to `"mvn"` when it is not a wrapper, then `apply_to_run_configs`, in its `"maven" =>` arm, replaces it with `maven_command(root, s)` (the configured home) for a `generated_maven_goal` (Read: `crates/ide-sdk/src/apply.rs`). The stand-in prints `stand-in mvn`, and the tests assert that line. |
| Running `gradle`                                                                                                                   | the real Gradle                                                                                                                             | `gradle_command` returns the build's stand-in `gradlew`, and the tests assert `stand-in gradle`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Running `java`                                                                                                                     | a JVM                                                                                                                                       | Nothing on this path starts `java`. `java_runs` (`<jdk>/bin/java -version`) is reached only from `project_jdk_is_usable` / `java_execution_error` (jdtls and the project JDK check). If it is reached, it starts the stand-in's `bin/java`, which exits 1.                                                                                                                                                                                                                                                                                                                               |
| `ops::build::build_sync` → `bridge.configure_maven` and `lsp.java_update_project_configs`                                          | the Kotlin bridge and jdtls                                                                                                                 | Read: every project has a bridge (`kotlin_bridge: Some(kotlin_bridge)` in `project.rs`). `configure_maven` writes `kotlin-bridge-options.json` into the project's build cache, which is under the isolated app dirs, then schedules its worker. The worker `continue`s unless `settings.ide.mode.is_full()` (`kotlin_bridge.rs`, `run`). Hypothesis: the test windows are not in Full mode and start no jdtls. Check the default with `grep -n "is_full\|enum IdeMode" -A6 crates/ide-core/src/settings.rs`. A `java_update_project_configs` error is only logged at debug level.        |

The fake tool pattern is the one `ide-github`'s `GhCli::at` / `app-gpui`'s `GhStandIn` follow (Read: `workbench/github.rs` `gh_program` under `#[cfg(test)]`). The program is a script in a temp folder, and the backend is told its path through an existing setting: Maven home and the Gradle wrapper. Unlike `gh`, the build tools need no new seam for the path, because the settings already carry it.

One thing is not guarded. If the Maven home setting were ever ignored, `MavenProject.command` would fall back to `mvn` on `PATH`. Hypothesis: a guard like `located_gh()`'s panic would need a hook in `ide-build/src/process.rs`; it is not proposed. The tests' `stand-in mvn` assertions would fail, but only after the real `mvn` ran.

---

# Part 2. `app-gpui`

## 2.0 How the draft relates to the existing Build window

Read: `app-gpui/src/build_window/` with `workbench/build_window.rs` is Build › Build Project / Rebuild / Build Module / Build Artifacts. It holds the `build` tool window, `BuildRun`, `ops::build::build_run`, the `build.running` / `build.lastStatus` keys and the `build:compile` task.

The draft is the separate Maven and Gradle tool windows (`maven`, `gradle`), with `build_model`, `build_sync`, the `build.maven` / `build.gradle` keys and the `build-sync:<tool>` tasks. Nothing overlaps:

- the draft keeps its own `build_tools: BuildToolsSlot` beside `build: Option<build_window::BuildSlot>`;
- its sync loop copies the `futures::select!` loop of `start_build`;
- `Workbench::build_model()` and `build_sync_running()` do not clash with `build_run()` / `mount_build()` / `reset_build()`.

Read: no other native code loads the build model or sets `build.maven` / `build.gradle` (`grep -rn 'build_model\|"build\.maven"\|"build\.gradle"\|build_sync' app-gpui/src`, excluding the drafts). The hits are `project_tree_model::build_model`, an unrelated tree builder, and the settings page ids `build.maven` / `build.gradle`.

## 2.1 `mod` lines

`app-gpui/src/lib.rs`: kept as in the old notes, and the anchor exists. After

```rust
pub mod build_window;
```

add

```rust
pub mod build_tools;
```

`app-gpui/src/workbench/mod.rs`: kept. After

```rust
mod build_window;
```

add

```rust
mod build_tools;
```

## 2.2 `workbench/mod.rs`: the import (new, replaces the long paths of the old 2g)

After

```rust
use crate::build_window::model::BUILD_TOOL_WINDOW;
```

add

```rust
use crate::build_tools::model::{GRADLE_TOOL_WINDOW, MAVEN_TOOL_WINDOW};
```

Read: no other `MAVEN_TOOL_WINDOW` / `GRADLE_TOOL_WINDOW` exists outside the drafts.

## 2.3 `workbench/mod.rs`: the field (kept)

In `pub struct Workbench`, after

```rust
    /// The project's build.
    build: Option<build_window::BuildSlot>,
```

add

```rust
    /// The Maven and Gradle tool windows: the project's build model, its syncs and toggles.
    build_tools: build_tools::BuildToolsSlot,
```

## 2.4 `workbench/mod.rs`: its initial value (kept)

In `Workbench::new`, after the line `            build: None,` (Read: it occurs once) add

```rust
            build_tools: Default::default(),
```

## 2.5 `workbench/mod.rs`: the actions (kept)

In `pub fn register_actions(cx: &mut App)`, after

```rust
    build_window::register_actions(registry);
```

add

```rust
    build_tools::register_actions(registry);
```

## 2.6 `workbench/mod.rs`: mount and reset (kept)

In `open_project`'s spawned continuation, after `                workbench.mount_agents(window, cx);` (Read: once, followed by `                if workbench.project_root(cx).is_some() {`), add

```rust
                workbench.mount_build_tools(window, cx);
```

`        self.reset_build(window, cx);` occurs twice (Read): once in `pub fn close_project`, once in `pub fn open_project`. After each add

```rust
        self.reset_build_tools(window, cx);
```

## 2.7 `workbench/mod.rs`: the mount arm (re-anchored)

In `fn mount_panels`, directly before

```rust
                BUILD_TOOL_WINDOW => match self.mount_build(window, cx) {
```

add

```rust
                MAVEN_TOOL_WINDOW | GRADLE_TOOL_WINDOW => match self.mount_build_tool(&id, window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

It uses the neighbour's shape and the import of 2.2. The old anchor (`FIND_TOOL_WINDOW => {`) still exists, but the new place sits beside the Build window's arm. `id` is the loop's `String` (Read: `for id in missing {` / `let panel = match id.as_str() {`).

## 2.8 `workbench/mod.rs`: backend events (kept)

In `pub fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>)`, replace

```rust
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        let now = now_ms();
```

with

```rust
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        // Build files, project folders, Java settings and trust: the build model is read again.
        self.build_tools_backend_event(&event.event, cx);
        let now = now_ms();
```

## 2.9 `app-gpui/src/backend.rs`: the test backend keeps builds off the home (new)

In `fn start`, directly before

```rust
        let mut runtime = tokio::runtime::Builder::new_multi_thread();
```

add

```rust
        // A test reads no Maven user folder and none of the machine's JDKs: the folder named here
        // is one nothing creates, and a test's project names the JDK it builds with.
        #[cfg(test)]
        ide_host::ops::build::isolate_from_user_home(
            std::env::temp_dir().join(format!("openidea-gpui-tests-m2-{}", std::process::id())),
            Vec::new(),
        );
```

The value is the same on every call in one test binary, so concurrent tests do not race on it, and setting it again with an equal value does nothing (1.4, `if *named == homes`). The folder is never created, so nothing is left behind. Maven's reads of `settings.xml` there find nothing, and the default repository there holds nothing.

## 2.10 `app-gpui/src/workbench/plugin_runtime.rs`: Cancel on a sync's task (kept)

In `pub fn cancel_background_task`, add this arm directly before

```rust
            Some(CancelEffect::Producer(other)) => tracing::warn!("task {other:?} has no producer to stop it"),
```

```rust
            Some(CancelEffect::Producer(TaskId::Name(name)))
                if name.starts_with(super::build_tools::SYNC_TASK_PREFIX) =>
            {
                self.cancel_build_sync_task(&name, cx);
            }
```

Read: the arm before it already binds `TaskId::Name(name)` under a guard (`if name.starts_with("plugin:")`), so the shape compiles as that one does. `cancel_build_sync_task` returns a `bool` that is not `#[must_use]`. Without this hunk `cancel_build_sync_task` is dead code and `-D warnings` fails.

## 2.11 Hunks dropped or changed, and why

- Old 2i ("`TOOL_WINDOWS` is not edited"): kept as a decision, with one correction. The old `when` of `toolwindow.gradle` (`projectOpen && build.gradle`) was invented and lost the action after the last unlink. The webview registers `toolwindow.<id>` while the window is registered, with no `when` (Read: `src/workbench/toolwindows/actions.ts`, the `registerAction({ id: \`toolwindow.${id}\` … })`loop). The native registry is the app's, not the window's, so the draft now publishes`build.gradleWindow`(true while this window has the Gradle tool window) and gates`toolwindow.gradle`on`projectOpen && build.gradleWindow`. `toolwindow.maven`keeps`projectOpen && build.maven`, which equals "registered" for Maven (registered exactly when `has_maven`). `verbatim: declared.verbatim`is copied, as`TOOL_WINDOWS`' loop in `mod.rs` does.
- Old §4 ("Menu entries: none"): the purpose is now served by newer code. `workbench/context_menus.rs` already declares `GroupDecl { id: "gradle.analyzeDependencies", … rows: None }` and `GroupDecl { id: "maven.analyzeDependencies", … rows: None }`, so their rows stay empty with no hunk. Nothing is added to `menu_spec.rs`: the webview adds no main-menu entry for these actions (Read: `actions.json` gives no menu place, and `src/features/build-tools/index.ts` registers no menu items).
- Old "run `cargo fmt` once after pasting": dropped. The long lines of the drafts were wrapped by hand. Only lines inside string literals remain over 120 columns: the `printf` line of `FAKE_GRADLEW` in a raw string, which rustfmt does not touch.
- Old §6 (isolation): replaced by 1.8. The old claim that "the fixtures declare no Java version, so `missing_jdk_message` returns `None`" was beside the point. The scan ran anyway: `gradle_build_env` calls `installed_jdks()` unconditionally, and `maven_jdk` calls it whenever `project_jdk` is `None`. Without a JDK setting, `pinned_install` also read the global mise config under the home.

## 2.12 Edits made to the draft files

`app-gpui/src/workbench/build_tools.rs`:

1. A new `const GRADLE_WINDOW_KEY: &str = "build.gradleWindow";`. `register_build_tool_windows` sets it from `self.build_tools.gradle.registration.is_some()` after every registration change, and `reset_build_tools` clears it with the other keys (that key list was wrapped under 120 columns).
2. The `toolwindow.*` registration uses `projectOpen && build.gradleWindow` for Gradle and copies `verbatim: declared.verbatim` (2.11).
3. Tests: `fake_jdk(tools)` (a `release` file and a failing `bin/java`) and `project_settings(root, tools, more)` write `[languages.java] jdk = "<tools>/jdk"`. `maven_project` writes it before its `[languages.java.maven]` table. `gradle_project(root, tools)` now takes `tools` and writes it too; its three call sites are updated (`build_project(cx, true, gradle_project)`, and `gradle_project(root, tools)` in the links test).
4. Tests: `links_from_the_toolbar_…` now asserts `build.gradleWindow` stays true after the last unlink and runs `toolwindow.gradle` through the registry (`run_action` panics if the action is unavailable).
5. Lines over 120 columns wrapped by hand: the `modules` literal (`concat!`), the `pom()` format string, the `matches!` in `stop_cancels_…` (a `building` closure), and the `matches!` in the Gradle failure assertion (a `first_line` binding).

`app-gpui/src/build_tools/view.rs`:

6. The profile row's hand-drawn check box (14 px box, `border`, `check` icon) is now the shared `crate::app_views::checkbox(cx, String::new(), checked)` inside the same clickable `build-tool-check:<key>` element. An import `use crate::app_views::checkbox;` is added. The helper's 8 px gap before its (empty) label stays, so the row's label sits 8 px further right than an icon row's. Pass the label in and drop the separate label child if that matters.

`app-gpui/src/build_tools/execute.rs`:

7. The popup's root draws the corners of the popup slot's shadowed frame (`Workbench::popup_body`: `radius-popup-large` and `shadow-popup`) instead of `radius-small`, with `overflow_hidden()` so the rows' selection colour stays inside them. The border colour stays `border`, as Search Everywhere's root does (`everywhere/view.rs`).

Not changed, after checking:

- The tooltips already use the shared `crate::ui::tooltip::WithTooltip` (`with_tooltip_shortcut` / `with_tooltip`, signatures Read).
- The unlink confirmation is a `PromptDialog` shown through `open_vcs_dialog`, whose layer draws `super::dialog_frame` (shared).
- The Execute popup is shown through `open_popup`, whose slot draws `popup_body` (shared placement and shadow).
- Waits are `wait_long(cx, "<what>", …)`. The only `sleep`s are inside the stand-in shell scripts, polling a marker the test removes.
- Clicks go through `click_on`, which runs `run_until_parked` before reading `debug_bounds`, or through the view tests' `click`, which does the same.
- The interface font is inherited: the workbench root sets `.font_family(theme::installed_fonts::ui_family(cx))` (Read: `workbench/mod.rs`), and no draft sets a font.
- Zoom: icons go through `theme::icon` and row heights and font sizes through `theme::length`, both of which follow the zoom. The fixed paddings stay `px(…)`, as in the nearest neighbours `services/view.rs` (a tool window with a toolbar and a tree) and `everywhere/view.rs` (a popup). To scale them as well, replace each `px(N)` in `view.rs` / `execute.rs` with `theme::zoomed(cx, N)` and drop `px` from their imports.
- `futures::executor::block_on` over `Backend::with_host` in the restricted test is the existing pattern (Read: `workbench/mod.rs`, `let set = futures::executor::block_on(setting);`).

## 2.13 API check (every external call of the drafts against today's code; Read)

Unchanged and matching: `branches::run_action_later(String, Option<Value>, &mut Window, &mut App)`; `services_window::on_workbench`; `NotificationRequest::new(level, title)` and its fields `body`, `actions`, `group`, `key`; `NotificationAction { label, command }`; `TaskUpdate { id, title, message, fraction: Option<f64>, done, cancel: Change<Cancel>, uncancellable }` with `TaskUpdate::new(TaskId, &str)`; `Cancel::Producer`; `Change::{Set, Clear}`; `Tasks::upsert_task(update, now_ms)` / `remove_task(&TaskId)`; `Workbench::schedule_tasks`.

Also matching:

- `tool_windows::{Effects { persist, focus_editor }, Registration}`; `ToolWindowLayout::register(ToolWindow) -> (Registration, Effects)`, `unregister(Registration) -> Effects`, `activate(&str)`, `show(&str, bool)`, `window(&str)`.
- `Workbench::with_layout(window, cx, impl FnOnce(&mut ToolWindowLayout) -> Effects)`; `tool_window_declaration(&str) -> Option<ToolWindow>` (`icon`, `title`, `verbatim`).
- `ActionRegistry::handle` (which panics on an id missing from the manifest: every handled id is in `actions.json`), `handle_toggle(id, Fn(&Window, &App) -> bool, run)`, `register(Action)`, `contains`.
- `ActionMeta { category, icon, keywords, verbatim, .. }` with `ActionMeta::new(id, title).when(w)`.
- `set_context(window, cx, key, value)` and `window_context(window, cx).is_truthy(key)`; `UiStorage::get(&str) -> Option<&str>` and `set(&str, String)`; `changes_build_model(&[P: AsRef<str>])`.
- `PromptDialog::confirm(ConfirmRequest, window, cx)` and `ConfirmRequest { title, message, ok_label, danger }`; `PromptDialogEvent::Closed(DialogAnswer)` and `DialogAnswer::confirmed()`.
- `open_vcs_dialog(AnyView, FocusHandle, Subscription, window, cx)`, `close_vcs_dialog`, `vcs_prompt()`.
- `open_popup(&Entity<V>, Subscription, window, cx)`, `close_popup`, the `popup.view` field; `show_menu(&[MenuItem<C>], Point<Pixels>, Fn(&mut Self, C, &mut Window, &mut Context<Self>), window, cx)`.
- `open_settings(Option<&str>, bool, window, cx)`, `project_restricted(&App)`, `run_driver()`, `project_root(&App)`, `open_project_file(PathBuf, …)`, `create_config(RunConfig, …)`, `Workbench::notify(NotificationRequest, cx)`, `Panel::new(view, Option<Subscription>)`, `Content::Project { panels: HashMap<String, Panel>, .. }`.

The `MenuItem` API, `IdeEvent` and the backend types also match:

- `MenuItem::{item, action, action_with, labelled, icon, Separator, action_id, label, command, arg}`; `find::row_selection::{selection_mode, SelectMode}`; `input::claim_keys(FocusHandle, Fn(&Keystroke, &mut Window, &mut App) -> bool, &mut App) -> u64` and `release_keys`; `TextField::{new, with_placeholder, text, set_text}`; `TextFieldEvent::{Changed, Submitted, Cancelled}`.
- `IdeEvent::{FsChanged { created, modified, removed: Vec<IpcPath> }, SettingsChanged { key }, ProjectRootsChanged { .. }, ProjectTrustChanged { .. }}`; `IpcPath` derefs to `Path` and is a public tuple struct, re-exported as `ide_core::IpcPath`.
- `ide_build::{MavenModel { projects, profiles, synced_at, stale, sync_error }, MavenPlugin { group_id, artifact_id, version, prefix, goals }, MavenProfile { project_root, id, active_by_default }, GradleTask { name, group, description }}` (the view tests' exhaustive literals match); `GradleModel` / `GradleProject` / `MavenProject` derive `Default`; `MavenProject.env` exists.
- `ide_run::RunConfig` has exactly `id, name, kind, command, args, cwd, env, detected, is_test, debug_adapter, allow_parallel, extras` (the literal in `model.rs::generated` matches).
- `ops::build::{build_model(&Host, &str), build_sync(&Host, &str, BuildTool, RequestId, bool, Vec<String>, Option<String>, Channel<SyncProgress>), gradle_link_projects(&Host, &str, Vec<String>), gradle_unlink_projects(&Host, &str, Vec<String>)}`.

Test helpers and GPUI calls also match:

- `workbench::tests::{start, wait_long, click_on, run_action, notification_titles}`, all `pub(super)`. `RUN_TOOL_WINDOW` is public in `workbench/mod.rs`.
- `RunView::lines(&App)`, `RunStore.sessions` and `RunSession::is_live()`, `run::configs::blank_config(id, name, kind)`, `SettingsDialogView::selected_page(&App)`, `settings_dialog()`, `tool_windows()`, `tool_window_view`, `is_tool_window_visible`, `notifications().items()`, the `restricted-banner-trust` selector (used by an existing test in `mod.rs`).
- `TrustStore::decide(&Path, bool, bool)`, `Backend::{next_request_id, with_host}`, `Host::close_project(&str)`.
- GPUI: `ClickEvent::{modifiers, click_count}`, `Styled::line_through`, `Context::processor`, `PathPromptOptions { files, directories, multiple, prompt }`, `TestAppContext::{did_prompt_for_paths, simulate_path_prompt_response}` (also used through `VisualTestContext` in `light_edit_tabs.rs`).

The `ide_build` struct literals in `model.rs` and `execute.rs`, tests included, were checked too. Read: every `MavenProject`, `MavenDependency`, `MavenModel`, `GradleModel` and `GradleProject` literal ends in `..Default` / `..base()` / `..core.clone()`. The exhaustive ones (`MavenPlugin`, `MavenProfile`, `GradleTask`, `BuildModel`) list today's fields. `MavenDependency`'s newer `omitted`, `conflict_with` and `managed_from` are covered by `..MavenDependency::default()`; `model.rs` reads `d.omitted.is_none()` and `d.classifier`, which both exist.

Measured: `git log --oneline c0cdfe07..HEAD -- crates/ide-build/src/maven.rs crates/ide-build/src/gradle.rs crates/ide-build/src/lib.rs crates/ide-run/src/lib.rs crates/ide-host/src/ops/build.rs` printed nothing. `c0cdfe07` is the draft's commit ("gpui: drafted the Maven and Gradle tool windows"), so no backend type the draft uses changed since.

Not read line by line: `model.rs` beyond its imports, its `RunConfig` constructors and its test fixtures, and `execute.rs` lines 30–300 (`ExecuteState`). Both call only `super::model`, `crate::platform::collate::compare`, `ide_run::RunConfig` and `ide_build` types, which were checked as above.

Changed or missing: none found in what was read. The one semantic correction is the `toolwindow.gradle` condition (2.11).

## 2.14 Actions registered

Each row gives: action id, the test that runs it through the registry, and the IPC command it stands for, as adapter (`src-tauri/src/commands/build.rs` or `run.rs`) → native call site. Label, `when` and menu place were checked against `app-gpui/assets/gen/actions.json` and `src/features/build-tools/index.ts` (Read). Every id below has the same title and `when` in both. None has a default shortcut (Read: `grep -rn "maven\.\|gradle\." src/platform/keymaps/` finds nothing). None is in the native main menu (measured: `grep -n "maven\.\|gradle\." app-gpui/src/workbench/menu_spec.rs` finds nothing). Hypothesis: the webview's main menu has none either; check with `grep -rn "maven\.\|gradle\." src/workbench/ src/platform/ --include='*.ts'`.

Adapters (Read):

- `src-tauri/src/commands/build.rs`: `build_model`, `gradle_link_projects`, `gradle_unlink_projects` and `build_sync`, each a one-line call of the `ide_host::ops::build` function of the same name.
- `src-tauri/src/commands/app.rs`: `cancel_request`.
- `src-tauri/src/commands/run.rs`: `run_start`.

The native call sites are named in the table.

| id (title; `when`)                                                                                                                    | test                                                                                                                                               | IPC → native call                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `maven.reload` (Reload All Maven Projects; `projectOpen && build.maven`)                                                              | `reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree`                                                                              | `build_sync` → `ide_host::ops::build::build_sync`, in `Workbench::sync_build_tool`                                        |
| `maven.cancelReload` (Stop Reloading Maven Projects; same, hidden)                                                                    | `stop_cancels_a_running_sync_without_an_error`                                                                                                     | `cancel_request` → `Host::cancel_request`, in `cancel_build_sync`                                                         |
| `maven.executeGoal` (Execute Maven Goal…; same)                                                                                       | `execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent`                                                                             | `run_start` (through `run.runConfig`) → `ops::run::start`, in `run_build_config` → `run_action_later("run.runConfig", …)` |
| `maven.toggleOffline` (Toggle Offline Mode; same, toggle)                                                                             | `execute_maven_goal_…`                                                                                                                             | none (UiStorage `openidea.buildTools:<root>`)                                                                             |
| `maven.toggleSkipTests` (Toggle 'Skip Tests' Mode; same, toggle)                                                                      | `runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options`                                                                          | none                                                                                                                      |
| `maven.expandAll` / `maven.collapseAll` (Expand All / Collapse All; same, hidden)                                                     | `runs_a_lifecycle_phase_…`                                                                                                                         | none                                                                                                                      |
| `maven.settings` (Maven Settings…; same)                                                                                              | `runs_a_lifecycle_phase_…`                                                                                                                         | none (`file.settings` with `build.maven`; native `open_settings(Some("build.maven"), …)`)                                 |
| `gradle.reload` (Reload All Gradle Projects; `projectOpen && build.gradle`)                                                           | `gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told`                                                                                 | `build_sync` → `ops::build::build_sync`, in `request_build_sync` / `sync_build_tool`                                      |
| `gradle.cancelReload` (Stop Reloading Gradle Projects; same, hidden)                                                                  | `gradle_reload_lists_…`                                                                                                                            | `cancel_request` → `Host::cancel_request`                                                                                 |
| `gradle.executeTask` (Execute Gradle Task…; same)                                                                                     | `a_task_runs_by_its_path_with_the_builds_wrapper_and_offline`                                                                                      | `run_start` → `ops::run::start`                                                                                           |
| `gradle.toggleOffline` (Toggle Offline Mode; same, toggle)                                                                            | `a_task_runs_by_its_path_…`                                                                                                                        | none                                                                                                                      |
| `gradle.expandAll` / `gradle.collapseAll` (same, hidden)                                                                              | `gradle_reload_lists_…`                                                                                                                            | none                                                                                                                      |
| `gradle.linkProjects` (Link Gradle Projects; `projectOpen && !workspaceOpen`)                                                         | `links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build`, `does_not_link_or_start_gradle_imports_in_an_untrusted_project` | `gradle_link_projects` → `ops::build::gradle_link_projects`, in `link_gradle_projects`                                    |
| `gradle.unlinkProjects` (Unlink Gradle Project; `projectOpen && build.gradle && !workspaceOpen`, hidden)                              | `links_from_the_toolbar_…`                                                                                                                         | `gradle_unlink_projects` → `ops::build::gradle_unlink_projects`, in `unlink_confirmed`                                    |
| `toolwindow.maven` (Maven; `projectOpen && build.maven`; not in `actions.json`, which covers it with the `toolwindow.` dynamic entry) | `runs_a_lifecycle_phase_…` (through `show`)                                                                                                        | `build_model` → `ops::build::build_model`, in `load_build_model`                                                          |
| `toolwindow.gradle` (Gradle; `projectOpen && build.gradleWindow`)                                                                     | `gradle_reload_lists_…`, `links_from_the_toolbar_…`                                                                                                | `build_model` → `ops::build::build_model`                                                                                 |

Not registered, so the toolbar and menus leave their rows out (`BuildToolView::toolbar` / `context_menu` filter on `registry.contains`):

- `maven.addProjects`, `maven.unlinkProjects`;
- `maven.analyzeDependencies`, `gradle.analyzeDependencies`, together with the IPC commands they need (`gradle_dependencies`, `maven_dependency_declaration`, `maven_exclude_dependency`, `maven_goto_declaration`).

The model is read only through `build_model`; `build_run` belongs to the Build window.

## 2.15 What is not built

Unchanged from the old notes §11:

- Maven Add / Unlink Projects.
- The Dependency Analyzer and its editor kind `maven.dependencyAnalyzer`.
- The automatic syncs: the first import, the reload after a profile change, and Reload after external changes.
- The Execute popup's project chooser: a row of names, not a select.
- No Download JDK, "Open Gradle Settings" or "Choose Gradle JVM" buttons on sync errors.
- Native Workspaces.
- The Run Anything `mvn` / `gradle` provider.
- POM navigation and the version-catalog editor contributions.

## 2.16 Compile risks not settled by reading

- `view.rs`, the profile check box: `checkbox(cx, String::new(), checked)` is called inside the `uniform_list` `cx.processor` closure, where `cx: &mut Context<BuildToolView>`. It derefs to `&App`, as the neighbouring `theme::color(cx, …)` calls do.
- `view.rs::render_tree` / `tool_button`: closures passed to `.when(…)` capture `cx` while `cx.listener(…)` also borrows it later in the same chain. This was in the original draft too. If the borrow checker objects, compute the colours and listeners into locals first.
- `model.rs::gradle_run_configs`: the `&String` → `&str` and `&&GradleModel` → `&GradleModel` coercions at closure calls (old risk, unchanged). Add `.as_str()` / `*` if rejected.
- `toolbar()`: `if … { "a" } else { "b" }.to_owned()` in a struct field initialiser (old risk). Wrap the `if` in parentheses if the parser objects.
- `workbench/build_tools.rs` tests, the `building` closure: its parameter is annotated `&[Banner]`, so it is higher-ranked. If inference still fails, make it a nested `fn building(banners: &[Banner]) -> bool`.
- Hypothesis: `run_action(cx, "toolwindow.gradle", None)` after the last unlink activates a registered window that holds no build and draws its empty view. Check by running the links test.
- `build.gradleWindow` is a native-only context key; the webview has none. Measured: `grep -rln "build.gradleWindow" app-gpui/assets/gen app-gpui/src` lists only the draft, so no known-keys list rejects it. The parity inventory's context-key rows do not include it.
- The stand-in JDK has no `javac`, and its `java` exits 1. Hypothesis: anything that validates the project JDK rejects it, for example `project_jdk_is_usable` (`javac` present, `java -version` runs), reached from jdtls set-up, and may show a warning instead of using it. Its path still sets `JAVA_HOME` for the build, because `maven_build_env` / `gradle_build_env` take `project_jdk` as it is. Check with `grep -n "project_jdk_is_usable\|fn project_jdk" -B3 crates/ide-sdk/src/java.rs crates/ide-sdk/src/apply.rs`. If a notice appears in a test, add a `bin/javac` stand-in.

## 2.17 Tests most likely to fail first, and why

1. `runs_a_lifecycle_phase_…`: the static labels `["Multi Demo", "Lifecycle", "Plugins"]`, and the `arg` lines after `prepare_maven_run` adds Settings › Maven's `-s` and `-Dmaven.repo.local=` (old assumptions 2 to 5).
2. `reload_reads_the_dependency_trees_…`: `parse_dependency_tree` has to accept the stand-in's section. It also asserts no Stale banner after the sync.
3. Any test in the `app-gpui` binary that relied on a JDK found on the machine fails once the test backend names no candidates. Measured: `grep -rn "installed_jdks\|rescan_jdks" app-gpui/src` prints nothing. `grep -rn "JAVA_HOME" app-gpui/src` outside the drafts finds only the Settings page labels (`settings_model/builtin_pages/build.rs`) and a remote-dev summary test (`features_b/remote_dev/environment.rs`). Hypothesis: no test depends on a machine JDK; confirm by running `cargo test -p openidea-gpui` once after step 2 below.
4. `links_from_the_toolbar_…`: assumption 7 of the old notes (`libs/other` is not discovered before it is linked), and the new `toolwindow.gradle` assertion.

## 2.18 Order to apply (each step compiles on its own)

1. Part 1: hunks 1.1, 1.2, 1.4, 1.6 and the new files 1.3 and 1.5, then the checks of 1.7.
2. `backend.rs` hunk 2.9. It only calls the forwarder of 1.6. Run `cargo test -p openidea-gpui --lib workbench::build_window`, or any existing workbench test, to see that the test backend still starts.
3. `lib.rs`: `pub mod build_tools;` (2.1). This compiles `build_tools/{mod,model,view,execute}.rs` with their own tests. Their items are `pub` in a `pub mod`, so nothing is dead code before the workbench uses them. Run `cargo test -p openidea-gpui --lib build_tools::`.
4. All of `workbench/mod.rs`, in one step: `mod build_tools;` (2.1), the import (2.2), the field (2.3), its initial value (2.4), the registration (2.5), mount and reset (2.6), the mount arm (2.7) and the backend event (2.8). Add the `plugin_runtime.rs` arm (2.10) in the same step. Without it, `cancel_build_sync_task` is dead and `-D warnings` fails; without the `mod.rs` calls, `mount_build_tools` and the rest are dead.
5. `cargo clippy -p openidea-gpui --all-targets -- -D warnings`, then `cargo test -p openidea-gpui --lib workbench::build_tools::` and `workbench::tests::smoke_registered_actions`.
