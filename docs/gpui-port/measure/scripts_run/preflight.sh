#!/bin/sh
# usage: preflight.sh <base>
# The automated part of the go / no-go list (DESIGN.md, "Go / no-go"). Reads only: the base
# folder, the process list, the session state. Exit 0 = go, anything else = no-go.
set -u
base=${1:?usage: preflight.sh <base>}
here=$(cd "$(dirname "$0")" && pwd)
fail=0
no() { echo "NO-GO: $*"; fail=1; }
ok() { echo "ok: $*"; }
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ok "base $base";; *) no "base must be /private/tmp/openidea-measure-<random>";; esac
[ -f "$base/config.json" ] || no "no $base/config.json (run build.sh)"
py=/usr/bin/python3
tauri=$($py -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["tauri_bin"])' "$base/config.json" 2>/dev/null)
native=$($py -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["native_bin"])' "$base/config.json" 2>/dev/null)
for b in "$tauri" "$native"; do case "$b" in "$base"/*) [ -x "$b" ] && ok "binary $b" || no "missing $b";; *) no "binary outside the base: $b";; esac; done
[ -n "$tauri" ] && { strings "$tauri" | grep -q 'ai.l0ng.openidea.measure' && ok "Tauri binary carries ai.l0ng.openidea.measure" || no "Tauri binary lacks the measure identifier"; }
# Nothing of ours still running; the single-instance socket of the measure identity is not held.
running=$(ps -axo pid=,command= | grep -F "$base/" | grep -v grep)
[ -z "$running" ] && ok "no process from the base is running" || no "processes from the base are running: $running"
sock=/private/tmp/ai_l0ng_openidea_measure_si.sock
if [ -e "$sock" ]; then
  holder=$(lsof -t "$sock" 2>/dev/null)
  [ -z "$holder" ] && echo "note: stale $sock (no holder): run_all.sh removes it" || no "$sock is held by pid $holder"
fi
# The owner's app: listed, never touched.
owner=$(ps -axo pid=,command= | grep -F "/Applications/Blazingly Fast IDE.app/" | grep -v grep)
[ -n "$owner" ] && echo "owner's app running (left alone): $owner" || echo "owner's app not running now"
# Session, permissions, SPIs.
state=$($py -I "$here/macos.py" session)
echo "session: $state"
echo "$state" | grep -q '"locked": false' && ok "screen unlocked" || no "screen locked or no GUI session"
echo "$state" | grep -q '"display_asleep": false' && ok "display awake" || no "display asleep"
echo "$state" | grep -q '"accessibility": true' && ok "Accessibility (posting events)" || no "this terminal lacks Accessibility: events cannot be posted"
echo "$state" | grep -q '"screen_recording": true' && ok "Screen Recording (window titles)" || no "this terminal lacks Screen Recording: window titles read as empty, the open-file waits cannot work"
echo "$state" | grep -q '"responsible_spi": true' && ok "responsibility SPI" || no "no responsibility SPI: WebKit helpers cannot be attributed"
echo "$state" | grep -q '"disclaim_spi": true' && ok "disclaim SPI" || echo "warning: no disclaim SPI: helper attribution falls back to start time (DESIGN.md R5)"
# The project clone: same APFS volume, exists, the owner's .openidea moved aside (if so decided).
p="$base/project/Allocadia"
[ -d "$p/.git" ] && ok "clone $p" || no "no clone at $p (run_all.sh makes it)"
[ -e "$p/.openidea" ] && echo "note: the clone keeps the owner's .openidea (decision D4)" || ok "clone has no .openidea"
[ -f "$base/files.json" ] && ok "files.json: $(wc -l < "$base/files.json") lines" || no "no files.json (measure.py --select-files)"
# Language servers the two apps could find (both search PATH and /opt/homebrew/bin, /usr/local/bin).
for s in jdtls lemminx kmp-lsp vtsls typescript-language-server basedpyright pyright; do
  for d in /usr/bin /bin /usr/sbin /sbin /opt/homebrew/bin /usr/local/bin; do [ -x "$d/$s" ] && echo "language server found: $d/$s"; done
done
free_gib=$(df -g /private/tmp | awk 'NR==2 {print $4}'); [ "$free_gib" -ge 20 ] && ok "free ${free_gib} GiB" || no "less than 20 GiB free"
[ $fail -eq 0 ] && echo "GO (the manual items of DESIGN.md still apply)" || echo "NO-GO"
exit $fail
