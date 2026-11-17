#!/bin/sh
# usage: build.sh <base> [--dry-run]
# Exports HEAD of the worktree into <base>/src (git archive: the index, the tree and the stash are
# not touched), builds both release binaries from that one export, and writes <base>/config.json.
# Nothing is fetched: pnpm and cargo run offline against their caches and the lock files.
set -eu
WT=/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01
base=${1:?usage: build.sh <base> [--dry-run]}
dry=${2:-}
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ;; *) echo "base must be /private/tmp/openidea-measure-<random>"; exit 2;; esac
run() { if [ "$dry" = --dry-run ]; then echo "[dry-run] $*"; else echo "+ $*"; sh -c "$*"; fi; }

head=$(git -C "$WT" rev-parse HEAD)
echo "commit $head"
free_gib=$(df -g /private/tmp | awk 'NR==2 {print $4}')
echo "free on /private/tmp: ${free_gib} GiB"
[ "$free_gib" -ge 80 ] || { echo "no-go: less than 80 GiB free (both release targets, the export and node_modules)"; exit 2; }

run "mkdir -p '$base/src'"
run "git -C '$WT' archive --format=tar '$head' | tar -x -C '$base/src'"
run "printf '%s\n' '$head' > '$base/src/COMMIT'"

# The measured Tauri build's own identity: merged into tauri.conf.json at build time by the CLI
# (TAURI_CONFIG, node_modules/@tauri-apps/cli/CHANGELOG.md: "represents the configuration to be merged").
conf='{"identifier": "ai.l0ng.openidea.measure", "productName": "OpenIDEA Measure", "bundle": {"createUpdaterArtifacts": false}}'
if [ "$dry" = --dry-run ]; then echo "[dry-run] write $base/measure.tauri.conf.json: $conf"; else printf '%s\n' "$conf" > "$base/measure.tauri.conf.json"; fi

# Frontend dependencies from the pnpm store only (fails rather than downloads).
run "cd '$base/src' && CI=1 pnpm install --offline --frozen-lockfile"
# tauri build: runs beforeBuildCommand (pnpm build: tsc, vite build, check-webkit) and builds the
# release binary with the frontend embedded; --no-bundle: no .app, so nothing registers with
# Launch Services (no file associations, no second "Blazingly Fast IDE" for open -a).
run "cd '$base/src' && CARGO_NET_OFFLINE=true pnpm tauri build --no-bundle --config '$base/measure.tauri.conf.json' -- --locked 2>&1 | tee '$base/build-tauri.log'"
# The native app with its own release profile (fat LTO; the root workspace uses thin LTO).
# For equal LTO set CARGO_PROFILE_RELEASE_LTO=thin here: a decision for the manager (DESIGN.md).
run "cd '$base/src/app-gpui' && CARGO_NET_OFFLINE=true cargo build --release --locked --bin openidea-gpui 2>&1 | tee '$base/build-native.log'"

tauri_bin="$base/src/target/release/openidea"
native_bin="$base/src/app-gpui/target/release/openidea-gpui"
if [ "$dry" != --dry-run ]; then
  for b in "$tauri_bin" "$native_bin"; do [ -x "$b" ] || { echo "missing $b"; exit 1; }; done
  # Gate: the identity compiled into the Tauri binary (embedded Info.plist, if any, and the config).
  launchctl plist __TEXT,__info_plist "$tauri_bin" > "$base/tauri-info-plist.txt" 2>&1 || true
  grep -q 'ai.l0ng.openidea.measure' "$base/tauri-info-plist.txt" && echo "embedded Info.plist: ai.l0ng.openidea.measure" || echo "note: no embedded Info.plist naming the measure identifier (see DESIGN.md, risk R3)"
  if grep -q '"ai.l0ng.openidea"' "$base/tauri-info-plist.txt"; then echo "no-go: the binary carries the owner's identifier"; exit 1; fi
  strings "$tauri_bin" | grep -c 'ai.l0ng.openidea.measure' > "$base/tauri-identifier-count.txt" || true
  [ "$(cat "$base/tauri-identifier-count.txt")" -ge 1 ] || { echo "no-go: identifier ai.l0ng.openidea.measure not found in the binary"; exit 1; }
  python3 -I "$(dirname "$0")/lockdiff.py" "$base/src/Cargo.lock" "$base/src/app-gpui/Cargo.lock" > "$base/lockdiff.txt"
  shasum -a 256 "$tauri_bin" "$native_bin" > "$base/binaries.sha256"
  du -sh "$base/src/target" "$base/src/app-gpui/target" "$base/src/node_modules" > "$base/disk.txt" 2>&1 || true
  cat > "$base/config.json" <<EOF
{"commit": "$head", "tauri_bin": "$tauri_bin", "native_bin": "$native_bin", "project": "$base/project/Allocadia",
 "rustc": "$(rustc -V)", "node": "$(node -v)"}
EOF
  cat "$base/disk.txt"
fi
echo "built from $head"
