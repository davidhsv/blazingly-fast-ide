#!/bin/sh
# usage: build.sh <base> [--dry-run]     (BUILD_COMMIT=<rev> selects the commit; default HEAD)
# Exports one commit of the worktree into <base>/src (git archive: the index, the tree and the stash are
# not touched), builds both release binaries from that one export, and writes <base>/config.json.
# Nothing is fetched: pnpm and cargo run offline against their caches and the lock files.
set -eu
WT=/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01
here=$(cd "$(dirname "$0")" && pwd)
base=${1:?usage: build.sh <base> [--dry-run]}
dry=${2:-}
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ;; *) echo "base must be /private/tmp/openidea-measure-<random>"; exit 2;; esac
run() { if [ "$dry" = --dry-run ]; then echo "[dry-run] $*"; else echo "+ $*"; sh -c "$*"; fi; }
# Free space on the data volume: logged before each build; under $1 GiB is a no-go.
disk() {
  echo "df $(date '+%F %T'): $(df -k /System/Volumes/Data | tail -1)"
  free_gib=$(df -g /System/Volumes/Data | awk 'NR==2 {print $4}')
  echo "free on /System/Volumes/Data: ${free_gib} GiB"
  [ "$free_gib" -ge "$1" ] || { echo "no-go: less than $1 GiB free"; exit 2; }
}

head=$(git -C "$WT" rev-parse --verify "${BUILD_COMMIT:-HEAD}^{commit}")
echo "commit $head (worktree HEAD $(git -C "$WT" rev-parse HEAD))"
disk 80

# pnpm 10.34.9 (the version that installed the worktree's node_modules; the pnpm on PATH is 9, which
# refuses this pnpm-workspace.yaml) from the corepack cache, through a shim that is also what
# tauri's beforeBuildCommand ("pnpm build") finds on PATH. The store is the one the worktree uses
# (from /private/tmp pnpm would otherwise pick an empty /private/tmp/.pnpm-store).
SHIM_DIR=${SHIM_DIR:-$(cd "$here/../bin" 2>/dev/null && pwd || echo "")}
[ -x "$SHIM_DIR/pnpm" ] || { echo "no-go: no pnpm shim at $SHIM_DIR/pnpm"; exit 2; }
export PATH="$SHIM_DIR:$PATH"
STORE=/Users/davidvieira/.pnpm-store
echo "pnpm $(pnpm -v), store $(cd /private/tmp && pnpm store path --store-dir "$STORE")"

run "mkdir -p '$base/src'"
run "git -C '$WT' archive --format=tar '$head' | tar -x -C '$base/src'"
run "printf '%s\n' '$head' > '$base/src/COMMIT'"

# The measured Tauri build's own identity: merged into tauri.conf.json at build time by the CLI
# (TAURI_CONFIG, node_modules/@tauri-apps/cli/CHANGELOG.md: "represents the configuration to be merged").
conf='{"identifier": "ai.l0ng.openidea.measure", "productName": "OpenIDEA Measure", "bundle": {"createUpdaterArtifacts": false}}'
if [ "$dry" = --dry-run ]; then echo "[dry-run] write $base/measure.tauri.conf.json: $conf"; else printf '%s\n' "$conf" > "$base/measure.tauri.conf.json"; fi

# Frontend dependencies from the pnpm store only (fails rather than downloads).
# Each build writes its log to a file (no pipe into tee, whose status would hide a failed build from set -e).
run "cd '$base/src' && CI=1 pnpm install --offline --frozen-lockfile --store-dir '$STORE' > '$base/pnpm-install.log' 2>&1"
# tauri build: runs beforeBuildCommand (pnpm build: tsc, vite build, check-webkit) and builds the
# release binary with the frontend embedded; --no-bundle: no .app, so nothing registers with
# Launch Services (no file associations, no second "Blazingly Fast IDE" for open -a).
disk 80
t0=$(date +%s)
run "cd '$base/src' && CARGO_NET_OFFLINE=true pnpm tauri build --no-bundle --config '$base/measure.tauri.conf.json' -- --locked > '$base/build-tauri.log' 2>&1"
echo "tauri build: $(( $(date +%s) - t0 )) s"
# The native app with its own release profile (fat LTO; the root workspace uses thin LTO).
# For equal LTO set CARGO_PROFILE_RELEASE_LTO=thin here: a decision for the manager (DESIGN.md).
disk 80
t0=$(date +%s)
run "cd '$base/src/app-gpui' && CARGO_NET_OFFLINE=true cargo build --release --locked --bin openidea-gpui > '$base/build-native.log' 2>&1"
echo "native build: $(( $(date +%s) - t0 )) s"
disk 40

tauri_bin="$base/src/target/release/openidea"
native_bin="$base/src/app-gpui/target/release/openidea-gpui"
if [ "$dry" != --dry-run ]; then
  for b in "$tauri_bin" "$native_bin"; do [ -x "$b" ] || { echo "missing $b"; exit 1; }; done
  # Gate: the identity compiled into the Tauri binary (embedded Info.plist, if any, and the config).
  launchctl plist __TEXT,__info_plist "$tauri_bin" > "$base/tauri-info-plist.txt" 2>&1 || true
  grep -q 'ai.l0ng.openidea.measure' "$base/tauri-info-plist.txt" && echo "embedded Info.plist: ai.l0ng.openidea.measure" || echo "note: no embedded Info.plist naming the measure identifier (see DESIGN.md, risk R3)"
  # The owner's identifier as a whole value, in the old-style ("...") and the XML (<string>...</string>) forms.
  if grep -qE '"ai\.l0ng\.openidea"|>ai\.l0ng\.openidea<' "$base/tauri-info-plist.txt"; then echo "no-go: the binary carries the owner's identifier"; exit 1; fi
  strings -a "$tauri_bin" | grep -c 'ai.l0ng.openidea.measure' > "$base/tauri-identifier-count.txt" || true
  [ "$(cat "$base/tauri-identifier-count.txt")" -ge 1 ] || { echo "no-go: identifier ai.l0ng.openidea.measure not found in the binary"; exit 1; }
  # For the report: every string naming ai.l0ng.openidea other than as the measure identifier
  # (keychain service prefixes are expected there, DESIGN.md part 3); and the same for the native binary.
  strings -a "$tauri_bin" | grep 'ai\.l0ng\.openidea' | grep -v 'ai\.l0ng\.openidea\.measure' | sort | uniq -c > "$base/tauri-other-identifier-strings.txt" || true
  strings -a "$native_bin" | grep 'ai\.l0ng\.openidea' | sort | uniq -c > "$base/native-identifier-strings.txt" || true
  python3 -I "$here/lockdiff.py" "$base/src/Cargo.lock" "$base/src/app-gpui/Cargo.lock" > "$base/lockdiff.txt"
  shasum -a 256 "$tauri_bin" "$native_bin" > "$base/binaries.sha256"
  ls -l "$tauri_bin" "$native_bin" > "$base/binaries-ls.txt"
  du -sh "$base/src/target" "$base/src/app-gpui/target" "$base/src/node_modules" > "$base/disk.txt" 2>&1 || true
  cat > "$base/config.json" <<EOF
{"commit": "$head", "tauri_bin": "$tauri_bin", "native_bin": "$native_bin", "project": "$base/project/Allocadia",
 "rustc": "$(rustc -V)", "node": "$(node -v)", "pnpm": "$(pnpm -v)"}
EOF
  cat "$base/disk.txt"
fi
echo "built from $head"
