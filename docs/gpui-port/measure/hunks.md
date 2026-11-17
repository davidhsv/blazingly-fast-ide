# App changes for the measurement

None is required. The design drives both release builds with the same OS-level input (key events posted by PID through `CGEventPostToPid`, `scripts/macos.py`), and every isolation it needs comes from a build-time config merge (`pnpm tauri build --config`), environment variables and files written into fresh folders. No tracked file is edited for the measurement.

The hunks below are optional, for the integrator to apply only if the manager decides so. Each says why the design does without it.

## H1 (optional, not recommended): the native script driver in a release build, behind a Cargo feature

Why it is not needed: the OS-level driver works the same in both apps. The script driver exists only in the native app, so using it would drive the two apps differently. It also runs actions through the registry rather than through the key path.

Where it would help: as a cross-check in a pilot. If posted keys turn out not to reach the native window (risk R1 in DESIGN.md), the native scenarios could still be driven by `action⇥…` steps. They would then be measured with a different driver, and the results would have to say so.

Anchors are at HEAD `6bf48f7b6`, `app-gpui/`. They were read with `git show HEAD:…`.

`app-gpui/Cargo.toml`, a new section after the `[dev-dependencies]` block that ends with `ide-mcp = { path = "../crates/ide-mcp" }`:

```diff
 ide-mcp = { path = "../crates/ide-mcp" }

+[features]
+# The inspection script driver (`inspect.rs`) in a release build, for measurements only.
+inspect = []
+
 [lints.rust]
```

`app-gpui/src/lib.rs`, lines 24-25:

```diff
-#[cfg(debug_assertions)]
+#[cfg(any(debug_assertions, feature = "inspect"))]
 pub mod inspect;
```

`app-gpui/src/main.rs`, lines 40-41:

```diff
-        #[cfg(debug_assertions)]
+        #[cfg(any(debug_assertions, feature = "inspect"))]
         if let Some(script) = std::env::var_os("OPENIDEA_GPUI_INSPECT") {
```

The build would be `cargo build --release --locked --features inspect`, and the run would set `OPENIDEA_GPUI_INSPECT`. Hypothesis: nothing else that `inspect.rs` uses is gated on `debug_assertions`. Its only `cfg` is `#[cfg(test)] mod tests` at line 338, read at HEAD. Whether every gpui-kit API it calls is available in release was not read, because that source lives in the cargo registry outside the worktree. A `cargo check --release --features inspect` would settle it.

## Not hunks: changes made without touching the code

- **The Tauri build's identity.** `scripts/build.sh` passes `--config <base>/measure.tauri.conf.json` with `{"identifier": "ai.l0ng.openidea.measure", "productName": "OpenIDEA Measure", "bundle": {"createUpdaterArtifacts": false}}`. The debug per-worktree identity (`src-tauri/src/dev.rs`) cannot be used, because it is compiled only with `debug_assertions` (`src-tauri/src/lib.rs:9-10`, `87-88`). Forcing debug assertions into a release would also compile the MCP automation bridge (`lib.rs:158-160`) and `export_bindings`, which rewrites the tracked `src/bindings.ts` (`lib.rs:120-122`, `54-79`).
- **Equal LTO**, if the manager decides it. Setting `CARGO_PROFILE_RELEASE_LTO=thin` on the native build overrides `lto = true` in `app-gpui/Cargo.toml:96-101`, with no edit to the file.
