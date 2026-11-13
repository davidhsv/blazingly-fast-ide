# Keychain audit: `app-gpui` test build (`cargo test --lib`, crate `openidea_gpui`)

Worktree: `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01`. Everything here comes from reading source files and the cargo registry sources (`keyring-4.2.0`, `keyring-core-1.0.1`, `apple-native-keyring-store-1.0.2`). Nothing was compiled or run, and the keychain was not touched.

Labels: **MEASURED BY READING** means I quote the code path that decides it. **HYPOTHESIS** means it comes from reasoning or a partial search, and the check that would confirm it is attached.

## (A) Inventory of keyring call sites

Two code paths reach the OS keychain. One is the `keyring`/`keyring-core` crates, used only by `ide-ssh`. The other is `security-framework`, used directly only by `app-gpui/src/plugins.rs` (and `src-tauri`, which is not part of this build). Repo-wide greps for `keyring`, `security_framework`, `SecKeychain`, `SecItem`, `keychain`, `Command::new("security")`, `git-credential` and `secret-tool` found nothing else in `crates/` or `app-gpui/src`. `ide-agent`, `ide-mcp`, `ide-http`, `ide-remote` and `ide-github` contain no keyring code.

### A.1 The only keyring primitive: `crates/ide-ssh/src/secrets.rs`

| line    | item                                 | what it does                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 20      | `const SERVICE = "ai.l0ng.openidea"` | service prefix                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 36-74   | `struct DefaultStore` / `impl Vault` | `available()` (50-61) returns true if `keyring_core::get_default_store()` is set. Otherwise it calls `keyring::Entry::store_status()`, which runs keyring's `LazyLock` (`keyring-4.2.0/src/v1.rs:106-121`), and that installs `apple_native_keyring_store::keychain::Store` (login keychain) as the process default. `get`/`set`/`delete` (63-73) call `keyring_core::Entry::new(service, key)?.get_password / set_password / delete_credential`. These are the only `SecItem*`/`SecKeychain*GenericPassword` calls through keyring. |
| 96-98   | `fn service(id)`                     | `"ai.l0ng.openidea/<id>"`, where `<id>` is a UUID stored in the index file (`Index.id`). It is new for each index file (`new_id()`, line 100).                                                                                                                                                                                                                                                                                                                                                                                       |
| 105-111 | `SecretFile::new` / `with_legacy`    | always `Arc::new(DefaultStore)`. `with_vault` (113) is the existing seam, a trait object `Arc<dyn Vault>`. Only ide-ssh's own unit tests and `ide-host/src/proxy.rs:104` (a unit test) use it.                                                                                                                                                                                                                                                                                                                                       |
| 117-136 | `get`                                | calls the vault **only if `index.keys.contains(key)`** (line 120). A missing index file loads as an empty index (line 201) without touching the vault.                                                                                                                                                                                                                                                                                                                                                                               |
| 138-153 | `set`                                | always calls `vault.set` (line 145), so this is the write path.                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 155-188 | `delete`                             | returns early if the index does not list the key (159-161). The plaintext branch only runs for a legacy file.                                                                                                                                                                                                                                                                                                                                                                                                                        |
| 243-278 | `migrate`                            | runs only when the index file is in an older plaintext format. Then `vault.available()` and `vault.set` run for each legacy key.                                                                                                                                                                                                                                                                                                                                                                                                     |
| 306-316 | `testing::use_memory_store()`        | `#[cfg(any(test, feature = "testing"))]`. app-gpui depends on `ide-ssh` without `testing` (`app-gpui/Cargo.toml:48`), so this helper **does not exist in the app-gpui test build**. Nothing installs a non-OS store there.                                                                                                                                                                                                                                                                                                           |

Store setup with no item access (MEASURED BY READING): `apple_native_keyring_store::keychain::Store::new()` (`keychain.rs:181`) only builds a struct, and `Cred::build` (`keychain.rs:137-156`) only builds a struct. So `available()` on its own reaches no keychain item. Items are touched only by `get_password`/`set_password`/`delete_credential`. Any test binary that links `ide-ssh` links these symbols (keyring's default `v1` feature pulls in `apple-native-keyring-store`), which explains the `SecKeychain*`/`SecItem*` imports the earlier check found. Linkage says nothing about reachability.

### A.2 Users of `SecretFile` (all through `DefaultStore`, i.e. the real keychain)

| #   | site                                                  | function(s)                                                                                                                                                                                                                                                                                                                                                              | index file / service / account                                                                                             | seam today                                         |
| --- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| S1  | `crates/ide-ssh/src/manager.rs:37`                    | `SshManager::with_files`; write `set_secret` (89-105: `saved.set` at 100 when `save`, otherwise `saved.delete` at 104); read `secret` (109-114, only if `config.save_secret`); delete in `set_configs` (68)                                                                                                                                                              | `<data>/ssh-passwords.json` (`ide-host/src/lib.rs:125`); service `ai.l0ng.openidea/<uuid>`, account = SSH configuration id | none (no constructor takes a vault)                |
| S2  | `crates/ide-ssh/src/git_creds.rs:54`                  | `init(path)` puts a process-global `INSTANCE`; read `credential` (106-111), `answer_saved` (136); write `command_finished` (181-200, `saved.set` at 189 for pending answers); delete `forget` (208-210)                                                                                                                                                                  | `<data>/git-credentials.json` (`app-gpui/src/backend.rs:113`); account `scheme://host[:port]`                              | none                                               |
| S3  | `crates/ide-db/src/secrets.rs:37`                     | `SecretStore::new/scoped/get/set/delete`; callers in `crates/ide-db/src/lib.rs`: `remember` (499 `store.set`, 501 `store.delete`), `secret` (519 `get`), `save_sources` (448 delete, 455-457 get+set when a source moves between global and project), `set_ssh_secret` (477-485), `endpoint` (579 get, 592 `ssh.set_secret`); `crates/ide-db/src/plugin.rs:176` (delete) | `<data>/db-passwords.json` (`ide-host/src/project.rs:677`); account `<project root>                                        | <source id>`or`…                                   | <id>#ssh` | none |
| S4  | `crates/ide-host/src/lib.rs:133`                      | `Host.proxy_password`; read `with_proxy_password` (`proxy.rs:18`) on every settings read (`ops/project.rs:62, 259`) and in `apply_proxy`; write `take_proxy_password` (`proxy.rs:51`) when `set_settings` gets key `proxy.password`, or `proxy` with a non-empty `password` (`ops/project.rs:167`); delete `proxy.rs:56`                                                 | `<data>/proxy-password.json`; account `httpProxy`                                                                          | field is `pub(crate)`, swapped only in a unit test |
| S5  | `crates/ide-host/src/plugins/database.rs:161,169-170` | `remove_database_sources` (plugin uninstall)                                                                                                                                                                                                                                                                                                                             | same file as S3                                                                                                            | none (deletes only)                                |

### A.3 `security-framework` directly

| site                                                                                                                                                          | function                                            | service / account                                                                                                                 | seam                                                                                                                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/plugins.rs:106-138` `KeychainSecrets` (`get_generic_password` 121, `set_generic_password` 130, `delete_generic_password` 131), installed at 176 | `ide_host::plugins::SecretStore` for plugin secrets | service `"{APP_IDENTIFIER}.plugin.<plugin>"`, i.e. `ai.l0ng.openidea.gpui.dev.plugin.<id>` in debug builds; account = secret name | `#[cfg(all(target_os = "macos", not(test)))]` on the type and the install. In test builds the host keeps its default file store. `PluginHost::set_secret_store` is a per-host trait object. `workbench/plugins_window.rs:821` `test_plugin_host` uses `MemorySecrets`. |

## (B) Reachable from tests?

### B.0 The rule that decides every case (MEASURED BY READING)

Every app-gpui test that has a backend builds it with `AppDirs::under(<tempdir>)`. That is 28 files call `Backend::init`, and every `dirs` passed is `AppDirs::under(home.path())`. The workbench helper `start()` is at `workbench/mod.rs:3910-3923`. No test calls `crate::init` / `AppDirs::resolve` (only `main.rs:23` does). Every index file therefore lives under a fresh temp `data/` dir. From A.1:

- `get` and `delete` touch the vault only for keys the index already lists. In a fresh home the index is empty until a `set` succeeds, so **every read and delete in a test is index-only: harmless.**
- A read could never return one of the owner's real entries anyway. The service is `ai.l0ng.openidea/<uuid>`, and the UUID comes from the test home's own index. The owner's entries (Tauri app or native app) sit under the UUID in the owner's real data directory.
- The keychain is reachable only through (1) a `SecretFile::set` (S1 `manager.rs:100`, S2 `git_creds.rs:189`, S3 `lib.rs:499` / `456`, S4 `proxy.rs:51`), or (2) `migrate()`, which needs a legacy plaintext file in the home. No app-gpui test writes `*-passwords.json`, `proxy-password.json` or `git-credentials.json` (grep of `app-gpui/src`). `ide_host::data_dirs::migrate` only acts on Windows (`data_dirs.rs:45-49`).

This also answers item 3 for every window at once (see B.6).

### B.1 S1 SSH secrets: NOT reachable (MEASURED BY READING)

- `workbench/settings_window.rs:115-118` `store_ssh_secret` calls `host.ssh.set_secret(.., write.save)`. It runs from the Apply loop (`settings_window.rs:274`) only for secrets staged with `SettingsDialog::stage_ssh_secret`.
- `stage_ssh_secret` has no non-test caller. Its only callers are `settings_model/staging.rs:927-1020` (model tests against `FakeBackend`, never the host) and `settings_view/dialog.rs:1532` (`settings_view::dialog::tests::enables_apply_and_reset_for_a_secret_only_edit_and_reset_discards_it`). That test has no `Backend`, presses Reset rather than Apply, and its `carry_out` helper (`dialog.rs:1373`) discards secrets with `apply_secret_done(Ok(()))`.
- `DbManager::set_ssh_secret` / `endpoint` → `ssh.set_secret` is reachable only from Database prompts (B.3) or from an SSH-tunnelled source. No test source enables SSH: `sqlite_source` uses SQLite, and `endpoint` returns early for SQLite (`ide-db/src/lib.rs:564`).
- `set_configs` (`ide-host/src/lib.rs:126`, `ops/project.rs:213`, `workbench/app_actions.rs:942`) only deletes, which is index-only.
- The deployment, remote-dev and SSH UIs (`features_b/ssh.rs`, `features_b/deployment/*`, `features_b/remote_dev/*`, `platform_more/remote*.rs`, `settings_model/ssh_connection.rs`) import only `ide_ssh` types (`FsConnection`, `RemoteEntry`) and `ide_host::environment::EnvReloaded`. No secret-saving backend call appears. `features_b::ssh::AskpassQueue::answer` (`ssh.rs:65`) only builds an `AskpassReply` value. Nothing in app-gpui sends it, because app-gpui never starts an `AskpassServer` (grep).

### B.2 S2 git HTTPS credentials: NOT reachable (MEASURED BY READING)

`git_creds::init` runs in every `Backend::start` (`backend.rs:113`), but it only builds a `SecretFile` (lazy). Writes need `command_finished` with pending answers. Those come only from `AskpassServer::answer` (`askpass.rs:105-114`) and the git command observer. app-gpui never calls `set_askpass_env`, `set_git_command_observer` or `AskpassServer::start`; the only callers are `src-tauri/src/state.rs` and an ide-vcs unit test (`cli.rs:730-745`). Reads (`answer_saved`) happen only inside the askpass server. Not reachable.

### B.3 S3 database passwords: NOT reachable (MEASURED BY READING)

Writes need `DbOp::SetPassword {save: true}` / `SetSshSecret {save: true}`, carried out by `workbench/database.rs:135-142` (`run_op`). These ops come from:

- `DatabaseStore::save_source` (`database/store.rs:1737-1757`) with `SecretChange::Set`. The Source dialog's `save_request` (`source_dialog.rs:596-611`) gives `Keep` for SQLite passwords and for sources without SSH. The only real-backend Save is `workbench::tests::new_data_source_tests_and_saves_a_sqlite_file_that_then_shows_in_the_tree`, which is SQLite with no SSH, so no `SetPassword` is sent. The `database::store::tests::*` calls with `SecretChange::Set` (`store.rs:3512, 3532`) are pure state-machine tests: the returned `Command`s are never executed.
- `prompt_answered` (`store.rs:1369-1385`) for a parked `Retry`. `workbench::tests::the_password_a_connection_asks_for_is_masked_and_answers_the_store` (`workbench/mod.rs:11530-11574`) opens a Password prompt with `ask_database(7, Prompt::Password{..})`. `ask_database` (`database.rs:873-875`) only queues `Command::Ask` and parks nothing. So `prompt_answered` finds no `parked[7]` and returns no commands (`store.rs:1371`). Even if a parked entry existed, the checkbox starts at `source.save_password`, which is `false` for `DataSource::default()` (`ide-db/src/source.rs:214`), and Enter does not toggle it. That would give `save: false`, i.e. `remember` → `store.delete`, which is index-only.
- `save_sources` with a source switching global/project (`lib.rs:452-458`) first does a `get`, which is empty in a fresh home.
- The other real-backend Database tests (`expanding_a_data_source_lists_its_tables_and_a_tables_columns`, `edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one`, `execute_shows_a_querys_rows_and_its_output_lines_and_a_failing_statement_its_error`, `dropping_a_data_source_asks_and_removes_it_from_the_project_and_the_tree`, `the_database_trees_actions_and_shortcuts_reach_the_store_and_the_backend`, `proofs_database::*`) use SQLite only. `password()` passes `saved = driver != Sqlite && save_password`, which is false (`lib.rs:522-523`), so they never `get`. Drop, rename and duplicate only delete, which is index-only.
- S5 (plugin uninstall) only deletes, which is index-only.

### B.4 S4 proxy password: NOT reachable (MEASURED BY READING)

No app-gpui code writes the keys `proxy` or `proxy.password` (grep finds only the page keyword list `settings_model/builtin_pages/behavior.rs:159`). The HTTP Proxy page is a `custom(...)` placeholder (`behavior.rs:157-176`). The `platform_more::proxy_settings::tests::*` tests only serde-parse values. Reads (`with_proxy_password` on every settings read, `apply_proxy`) do `get("httpProxy")` on an empty index, which is index-only.

### B.5 Plugin keychain (A.3): NOT reachable from tests (MEASURED BY READING)

It is compiled out under `cfg(test)`, and the test hosts use `MemorySecrets` or the host's default file store. `workbench::plugins_window::tests::the_app_plugin_host_keeps_secrets_in_a_file_under_the_temporary_home` asserts the file store (read, not run). **Separate finding, outside tests:** the gate is `not(test)`, not "isolated home". A real `openidea-gpui` binary started with `OPENIDEA_GPUI_HOME=<dir>` (measurements, e2e) still installs `KeychainSecrets` with service `ai.l0ng.openidea.gpui.dev.plugin.<id>`. That is the same service the owner's own debug build uses, so such a run can read and overwrite the owner's real dev plugin secrets. Fixed by hunk C.4.

### B.6 Item 3: windows opening on their own (MEASURED BY READING)

- Opening a project: `project_services` builds `DbManager` → `SecretStore::new` (lazy).
- Settings dialog: settings reads → `with_proxy_password` → `get` on an empty index.
- Database tool window: `LoadSources` lists sources and makes no secret call.
- Agents window: no keyring code in `ide-agent`.
- Deployment and SSH pages: no `ssh.secret`/`needs_secret` caller in app-gpui.
- Welcome screen: no secret calls.

None of these makes a vault call in a fresh home. Even in a home whose index lists a key, they would read only entries under that home's own UUID.

### B.7 Summary

| site                           | reachable from any test in `rust_tests.txt` | why not                                                                          |
| ------------------------------ | ------------------------------------------- | -------------------------------------------------------------------------------- |
| S1 `manager.rs:100` set        | no                                          | `stage_ssh_secret` has no UI caller; no SSH-tunnelled DB source in tests         |
| S2 `git_creds.rs:189` set      | no                                          | no askpass server / observer in app-gpui                                         |
| S3 `ide-db lib.rs:499/456` set | no                                          | SQLite only; the prompt test's ticket 7 is not parked and its save flag is false |
| S4 `proxy.rs:51` set           | no                                          | no proxy writes in app-gpui                                                      |
| S5 deletes, all `get`s         | they run, but index-only                    | empty index in a temp home; foreign UUID in any case                             |
| `migrate`                      | no                                          | no legacy files in test homes                                                    |
| A.3 `KeychainSecrets`          | no (compiled out)                           | but reachable at runtime under `OPENIDEA_GPUI_HOME` (B.5)                        |

How sure: high for today's code. All five write doors and the migrate door were traced to every caller. Nothing in the build enforces this, though. The next test that answers a parked Postgres password prompt with "Save password" ticked, or that wires the SSH page's password field, writes to the owner's login keychain without anyone noticing. Hence (C).

HYPOTHESIS checks (none of them reads the keychain):

- Whether any test sets up the OS store at runtime: add the tripwire from C.5 (`ide_ssh::secrets::default_store_installed()` must stay `false` in an app-gpui test process; it turns true on the first `DefaultStore::available()`).
- Watch for keychain traffic while the suite runs, without querying the keychain: `log stream --style compact --predicate 'subsystem == "com.apple.securityd" AND process BEGINSWITH "openidea_gpui"'` in one terminal, `cd app-gpui && cargo test --lib` in another.
- Linkage only (does not show reachability): `nm -m target/debug/deps/openidea_gpui-* | grep -E 'SecKeychain|SecItem'`.

## (C) Proposed hunks

Design: the existing `Vault` trait gets a second implementation, `FileVault`. It keeps the secrets in an owner-only file beside each index (`<index>.json.vault`). A process-wide switch, `keep_secrets_in_files()`, makes `SecretFile::with_legacy` (and so `new`) pick it. When the switch is on, `DefaultStore` also fails closed. app-gpui turns the switch on in `Backend::start` before the first `SecretFile` exists, whenever its dirs are isolated (`AppDirs::under`, used by every test and by `OPENIDEA_GPUI_HOME`). One sticky process-wide switch is safe with many parallel test homes because the location comes from each index's own path, never from a global home. The Tauri app never calls the switch, so its behaviour is unchanged. No new dependencies are needed: `keyring-core/sample` stays out of non-test builds.

### C.1 `crates/ide-ssh/src/secrets.rs`: switch, fail-closed default, `FileVault`

```diff
@@ use std::path::PathBuf;
-use std::path::PathBuf;
+use std::path::{Path, PathBuf};
 use std::sync::Arc;
+use std::sync::atomic::{AtomicBool, Ordering};

 const SERVICE: &str = "ai.l0ng.openidea";

 /// Serialises read-modify-write of the index files (several stores may share one).
 static LOCK: Mutex<()> = Mutex::new(());
+
+/// Set by [`keep_secrets_in_files`]: secrets go to a file beside their index, never the OS store.
+static IN_FILES: AtomicBool = AtomicBool::new(false);
+
+/// From now on every [`SecretFile`] keeps its secrets in an owner-only file beside its index
+/// instead of the OS credential store, and the OS store is never opened. For processes whose
+/// data lives in a temporary or overridden home (tests, measurements): their secrets must not
+/// reach, or read from, the user's keychain. There is no way back.
+pub fn keep_secrets_in_files() {
+    IN_FILES.store(true, Ordering::SeqCst);
+}
+
+pub fn secrets_in_files() -> bool {
+    IN_FILES.load(Ordering::SeqCst)
+}
+
+/// Whether this process has set up a default credential store (the OS one on first use): a check
+/// for tests that never opens the keychain.
+pub fn default_store_installed() -> bool {
+    keyring_core::get_default_store().is_some()
+}
@@ impl Vault for DefaultStore {
     fn available(&self) -> bool {
+        // Also guards stores made before the switch: they lose saving rather than reach the OS.
+        if secrets_in_files() {
+            return false;
+        }
         if keyring_core::get_default_store().is_some() {
             return true;
         }
@@
     fn delete(&self, service: &str, key: &str) -> StoreResult<()> {
         self.entry(service, key)?.delete_credential()
     }
 }
+
+/// Secrets in an owner-only JSON file (service → key → secret), for [`keep_secrets_in_files`].
+/// Only [`SecretFile`] calls it, always while holding [`LOCK`].
+struct FileVault(PathBuf);
+
+type Vaulted = BTreeMap<String, BTreeMap<String, String>>;
+
+impl FileVault {
+    fn beside(index: &Path) -> Self {
+        Self(index.with_extension("json.vault"))
+    }
+
+    fn read(&self) -> StoreResult<Vaulted> {
+        match std::fs::read(&self.0) {
+            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| StoreError::BadDataFormat(bytes, Box::new(e))),
+            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vaulted::new()),
+            Err(e) => Err(StoreError::PlatformFailure(Box::new(e))),
+        }
+    }
+
+    fn write(&self, all: &Vaulted) -> StoreResult<()> {
+        let bytes = serde_json::to_vec_pretty(all).map_err(|e| StoreError::PlatformFailure(Box::new(e)))?;
+        write_private(&self.0, &bytes).map_err(|e| StoreError::PlatformFailure(Box::new(e)))
+    }
+}
+
+impl Vault for FileVault {
+    fn available(&self) -> bool {
+        true
+    }
+
+    fn get(&self, service: &str, key: &str) -> StoreResult<String> {
+        self.read()?.get(service).and_then(|keys| keys.get(key)).cloned().ok_or(StoreError::NoEntry)
+    }
+
+    fn set(&self, service: &str, key: &str, secret: &str) -> StoreResult<()> {
+        let mut all = self.read()?;
+        all.entry(service.to_owned()).or_default().insert(key.to_owned(), secret.to_owned());
+        self.write(&all)
+    }
+
+    fn delete(&self, service: &str, key: &str) -> StoreResult<()> {
+        let mut all = self.read()?;
+        let keys = all.get_mut(service).ok_or(StoreError::NoEntry)?;
+        keys.remove(key).ok_or(StoreError::NoEntry)?;
+        if keys.is_empty() {
+            all.remove(service);
+        }
+        self.write(&all)
+    }
+}
@@ impl SecretFile {
     pub fn with_legacy(path: PathBuf, legacy: Legacy) -> Self {
-        Self::with_vault(path, legacy, Arc::new(DefaultStore))
+        let vault: Arc<dyn Vault> =
+            if secrets_in_files() { Arc::new(FileVault::beside(&path)) } else { Arc::new(DefaultStore) };
+        Self::with_vault(path, legacy, vault)
     }
@@
     fn write(&self, contents: &impl Serialize) -> IdeResult<()> {
         let err = |e: std::io::Error| IdeError::unavailable(format!("saved passwords: {}: {e}", self.path.display()));
-        if let Some(dir) = self.path.parent() {
-            std::fs::create_dir_all(dir).map_err(err)?;
-        }
-        let tmp = self.path.with_extension("json.tmp");
-        let mut options = std::fs::OpenOptions::new();
-        options.write(true).create(true).truncate(true);
-        #[cfg(unix)]
-        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
         let bytes = serde_json::to_vec_pretty(contents).map_err(|e| IdeError::internal(e.to_string()))?;
-        std::io::Write::write_all(&mut options.open(&tmp).map_err(err)?, &bytes).map_err(err)?;
-        std::fs::rename(&tmp, &self.path).map_err(err)
+        write_private(&self.path, &bytes).map_err(err)
     }
 }

+/// Replaces `path` with `bytes` through a sibling temporary file, owner-only on Unix.
+fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
+    if let Some(dir) = path.parent() {
+        std::fs::create_dir_all(dir)?;
+    }
+    let mut tmp = path.as_os_str().to_owned();
+    tmp.push(".tmp");
+    let tmp = PathBuf::from(tmp);
+    let mut options = std::fs::OpenOptions::new();
+    options.write(true).create(true).truncate(true);
+    #[cfg(unix)]
+    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
+    std::io::Write::write_all(&mut options.open(&tmp)?, bytes)?;
+    std::fs::rename(&tmp, path)
+}
+
 fn unavailable(why: &str) -> IdeError {
```

Note on `write_private`: for the index, the temp file is now `ssh-passwords.json.tmp`, as before (`with_extension("json.tmp")` on `x.json` gives the same name). For the vault it is `ssh-passwords.json.vault.tmp`, so the two never collide.

### C.2 `crates/ide-ssh/src/secrets.rs` tests module: `FileVault` semantics (no global switch, no keychain)

```diff
@@ mod tests {
+    #[test]
+    fn a_file_vault_keeps_secrets_beside_the_index_owner_only_and_forgets_them() {
+        let tmp = tempfile::tempdir().unwrap();
+        let path = tmp.path().join("ssh-passwords.json");
+        let file = SecretFile::with_vault(path.clone(), |v| serde_json::from_value(v).ok(), Arc::new(FileVault::beside(&path)));
+        file.set("a", "hunter2-file").unwrap();
+        let vault = path.with_extension("json.vault");
+        assert_eq!(files_containing(tmp.path(), "hunter2-file"), [vault.clone()]);
+        #[cfg(unix)]
+        {
+            use std::os::unix::fs::PermissionsExt;
+            assert_eq!(std::fs::metadata(&vault).unwrap().permissions().mode() & 0o777, 0o600);
+        }
+        let reopened = SecretFile::with_vault(path.clone(), |v| serde_json::from_value(v).ok(), Arc::new(FileVault::beside(&path)));
+        assert_eq!(reopened.get("a").as_deref(), Some("hunter2-file"));
+        reopened.delete("a");
+        assert_eq!(reopened.get("a"), None);
+        assert_eq!(files_containing(tmp.path(), "hunter2-file"), Vec::<PathBuf>::new());
+    }
+
+    #[test]
+    fn a_corrupt_file_vault_fails_the_save_instead_of_overwriting_it() {
+        let tmp = tempfile::tempdir().unwrap();
+        let path = tmp.path().join("ssh-passwords.json");
+        std::fs::write(path.with_extension("json.vault"), "{ not json").unwrap();
+        let file = SecretFile::with_vault(path.clone(), |v| serde_json::from_value(v).ok(), Arc::new(FileVault::beside(&path)));
+        assert!(file.set("a", "pw").is_err());
+        assert_eq!(std::fs::read_to_string(path.with_extension("json.vault")).unwrap(), "{ not json");
+    }
```

### C.3 `app-gpui/src/backend.rs`: isolated dirs turn the switch on

```diff
 pub struct AppDirs {
     pub config: PathBuf,
     /// Persistent user state, including credentials and recovery history.
     pub data: PathBuf,
     /// Machine-local tools and rebuildable caches.
     pub local_data: PathBuf,
+    /// Under a home of its own (tests, [`HOME_ENV`]): saved passwords stay in files there, never
+    /// in the user's keychain.
+    pub isolated: bool,
 }
@@ impl AppDirs {
         Ok(Self {
             config: dir(dirs::config_dir(), "configuration")?,
             data: dir(dirs::data_dir(), "data")?,
             local_data: dir(dirs::data_local_dir(), "local data")?,
+            isolated: false,
         })
     }

     /// `config`, `data` and `local-data` inside `home`.
     pub fn under(home: &Path) -> Self {
-        Self { config: home.join("config"), data: home.join("data"), local_data: home.join("local-data") }
+        Self { config: home.join("config"), data: home.join("data"), local_data: home.join("local-data"), isolated: true }
     }
 }
@@ fn start(dirs: AppDirs, cx: &mut App) -> anyhow::Result<(Self, UnboundedReceiver<BackendEvent>)> {
         tracing::info!(data_dir = ?dirs.data, local_data_dir = ?dirs.local_data, "application data directories");
+        // Before the first saved-password store exists (git credentials, then the host's).
+        if dirs.isolated {
+            ide_ssh::secrets::keep_secrets_in_files();
+        }
         // Saved git HTTPS credentials (askpass answers them silently).
         ide_ssh::git_creds::init(dirs.data.join("git-credentials.json"));
```

`AppDirs` has no other struct literal in `app-gpui/src` (grep `AppDirs {` matches only the definition), so adding the field breaks no caller.

### C.4 `app-gpui/src/plugins.rs:173-176`: no keychain plugin secrets under an isolated home

```diff
         // Without the keychain (other systems, test builds) the host keeps them in files under its
-        // data directory.
+        // data directory, as it does under a home of its own (`OPENIDEA_GPUI_HOME`).
         #[cfg(all(target_os = "macos", not(test)))]
-        host.set_secret_store(Arc::new(KeychainSecrets));
+        if !cx.global::<Backend>().dirs.isolated {
+            host.set_secret_store(Arc::new(KeychainSecrets));
+        }
```

(`backend` is borrowed only at lines 145-146, so `cx` is free here.)

### C.5 `app-gpui/src/backend.rs` tests: the seam holds, and the OS store is never set up

```diff
     #[test]
     fn directories_follow_the_home_override() {
         let dirs = AppDirs::under(Path::new("/tmp/x"));
         assert_eq!(dirs.config, Path::new("/tmp/x/config"));
         assert_eq!(dirs.local_data, Path::new("/tmp/x/local-data"));
+        assert!(dirs.isolated, "a home of its own keeps its saved passwords out of the keychain");
     }
+
+    /// Saved passwords of a temporary home stay in files under it. The switch is checked before
+    /// anything is saved, so a broken seam fails without writing to the keychain, and the OS
+    /// credential store is never set up in this process.
+    #[gpui_kit::test]
+    async fn saved_passwords_stay_in_files_under_a_temporary_home(cx: &mut TestAppContext) {
+        const SECRET: &str = "not-a-real-ssh-password";
+        cx.executor().allow_parking();
+        let home = tempfile::tempdir().expect("a temporary home");
+        cx.update(|cx| Backend::init(AppDirs::under(home.path()), cx)).expect("the backend starts");
+        assert!(ide_ssh::secrets::secrets_in_files(), "the backend keeps secrets in files");
+
+        cx.update(|cx| cx.global::<Backend>().host.ssh.set_secret("s1", Some(SECRET.to_owned()), true));
+        let config = ide_core::remote_settings::SshConfiguration {
+            id: "s1".into(),
+            save_secret: true,
+            ..Default::default()
+        };
+        let fresh = ide_ssh::SshManager::new(home.path().join("data/ssh-passwords.json"));
+        assert_eq!(fresh.secret(&config).as_deref(), Some(SECRET), "read back from the file");
+        let vault = std::fs::read_to_string(home.path().join("data/ssh-passwords.json.vault")).expect("the vault file");
+        assert!(vault.contains(SECRET));
+
+        let db = ide_db::secrets::SecretStore::new(home.path().join("data/db-passwords.json"), Path::new("/p"));
+        db.set("pg", SECRET).expect("the database password is saved");
+        assert_eq!(db.get("pg").as_deref(), Some(SECRET));
+        assert!(home.path().join("data/db-passwords.json.vault").is_file());
+
+        assert!(!ide_ssh::secrets::default_store_installed(), "the OS credential store was set up");
+    }
```

The last assertion is the process-wide tripwire. `DefaultStore::available()` is the only thing in the build that calls `keyring::Entry::store_status()`, and that call installs the OS store as the default (`keyring-4.2.0/src/v1.rs:106-121`). If any test in the same process had gone through `DefaultStore`, the flag would be set. Because tests run in parallel, the tripwire catches leaks from tests that have already run in the process; it does not prove anything about tests that run after it. C.1's fail-closed `available()` covers those.

To run after applying (by whoever applies it, not part of this audit): `cd crates && cargo test -p ide-ssh secrets::` and `cd app-gpui && cargo test --lib backend::tests`.

## (D) What I could not determine

1. **Nothing was executed.** All conclusions come from reading the code. No test results, `nm` output or runtime traces are behind them. Running C.5, plus the `log stream` command in B.7 during a full `cargo test --lib`, is what would turn "MEASURED BY READING" into measured.
2. **Child processes that use the keychain themselves** are outside the keyring crates:
   - HYPOTHESIS, not reachable: the backend's `git` does not disable the system `credential.helper` (Apple git ships `osxkeychain`). `ide-vcs/src/cli.rs:234` sets only `GIT_TERMINAL_PROMPT=0`, and helpers still run. A test that fetched from or pushed to an http(s) remote that asks for auth would make `git-credential-osxkeychain get` read the owner's real GitHub credential. Remotes I checked are all local: `push_sends_a_new_commit_to_origin_and_fetch_shows_what_came_in` uses a temp bare repo. The GitHub tests use `github_project` (`workbench/github.rs:1568-1596`), which uses a local bare repo with `protocol.allow never`, `protocol.file.allow always` and `url.<https>.insteadOf`. `prepare_diff` runs only through `gh_call` with a stand-in. Check: `grep -rnE '"(fetch|pull|push|clone)"|fetch_refspecs|clone\(' app-gpui/src | grep -i https`, and `grep -rn 'https://' app-gpui/src/vcs* app-gpui/src/github/clone.rs`. Hardening option (not proposed as a hunk): give git children `GIT_CONFIG_NOSYSTEM=1` / `-c credential.helper=` in test builds.
   - gh is gated in tests: `workbench/github.rs:228-235` returns an error without a stand-in, and the fake gh unsets token variables.
   - HYPOTHESIS: agent CLIs such as `claude` and `codex` keep their own credentials in the keychain. `proofs_agents.rs:49-51` installs a fake agent. I did not verify that every `workbench::tests::the_agents_*` test avoids launching the real default provider. Check: `grep -rn 'install_fake_agent\|start_task\|new_task' app-gpui/src/workbench/tests/proofs_agents.rs app-gpui/src/workbench/mod.rs`, then confirm each test that starts a turn calls `install_fake_agent` first.
3. **`crates/ide-ssh/tests/os_credential_store.rs`** writes and deletes real keychain entries (`ai.l0ng.openidea/<uuid>`), but it is `#[ignore]` and lives in another crate. It is not part of `app-gpui cargo test --lib`. A `cargo test -p ide-ssh -- --ignored` run would touch the keychain by design.
4. Other crates' own test suites (`cargo nextest run --workspace` at the root) are out of scope. They install `use_memory_store()` where they save secrets (`ide-db`, `ide-host` tests), and I did not audit every one of them.
5. C.1 to C.5 are written against the text I read. They have not been compiled. Two details I checked by reading: `PlatformError` is `Box<dyn std::error::Error + Send + Sync>` (`keyring-core-1.0.1/src/error.rs:15`), so `Box::new(io::Error)` and `Box::new(serde_json::Error)` fit; and the backend test module's `use super::*` brings in `Path` (`backend.rs:16`).
