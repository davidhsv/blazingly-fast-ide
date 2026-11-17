#!/bin/sh
# usage: run_all.sh <base> [--dry-run] [--reps N] [--keep-openidea] [--allow-activate]
# The campaign: an APFS clone of the project under the base, the 10 files, the preflight, then
# N repetitions per app alternating which app runs first (rep 1: native then tauri, rep 2: tauri
# then native, ...), a check of what each run wrote into the clone, and the aggregate table.
# The display is kept awake by caffeinate, which exits with this script (-w).
set -u
base=${1:?usage: run_all.sh <base> [--dry-run] [--reps N] [--keep-openidea] [--allow-activate]}
shift
dry=""; reps=3; keep=""; act=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=--dry-run;; --reps) reps=$2; shift;; --keep-openidea) keep=1;; --allow-activate) act=--allow-activate;;
    *) echo "unknown option $1"; exit 2;;
  esac; shift
done
here=$(cd "$(dirname "$0")" && pwd)
py="/usr/bin/python3 -I"
src=/Users/davidvieira/code/Allocadia
clone="$base/project/Allocadia"
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ;; *) echo "base must be /private/tmp/openidea-measure-<random>"; exit 2;; esac
say() { echo "== $*"; }
do_() { if [ -n "$dry" ]; then echo "[dry-run] $*"; else echo "+ $*"; sh -c "$*"; fi; }

# 1. The clone: clonefile(2) copies on the same APFS volume (checked), so writes stay in the copy.
if [ ! -d "$clone" ]; then
  [ "$(stat -f %d "$src")" = "$(stat -f %d /private/tmp)" ] || { echo "no-go: the project is not on the volume of /private/tmp (no clonefile)"; exit 2; }
  do_ "mkdir -p '$base/project' && /bin/cp -Rc '$src' '$clone'"
  # The owner's per-project files would steer both apps (a project-level ide.mode, a project name):
  # moved aside in the clone only, unless the manager decides to keep them (decision D4).
  [ -z "$keep" ] && do_ "[ -e '$clone/.openidea' ] && mv '$clone/.openidea' '$base/project/openidea-of-the-owner' || true"
fi
do_ "$py '$here/measure.py' --base '$base' --select-files"

# 2. Preflight (no-go stops here).
if [ -z "$dry" ]; then "$here/preflight.sh" "$base" || exit 2; else echo "[dry-run] $here/preflight.sh $base"; fi
sock=/private/tmp/ai_l0ng_openidea_measure_si.sock
[ -e "$sock" ] && [ -z "$(lsof -t "$sock" 2>/dev/null)" ] && do_ "rm -f '$sock'"

# 3. Keep the display awake for as long as this script lives (no setting is changed).
if [ -z "$dry" ]; then /usr/bin/caffeinate -d -i -m -w $$ & echo "caffeinate pid $!"; else echo "[dry-run] caffeinate -d -i -m -w $$ &"; fi

# 4. The runs.
i=1
while [ "$i" -le "$reps" ]; do
  if [ $((i % 2)) -eq 1 ]; then order="native tauri"; else order="tauri native"; fi
  for app in $order; do
    say "rep $i: $app"
    do_ "$py '$here/measure.py' --base '$base' --project-stamp"
    if [ -n "$dry" ]; then
      $py "$here/measure.py" --base "$base" --app "$app" --rep "$i" --dry-run $act
    else
      $py "$here/measure.py" --base "$base" --app "$app" --rep "$i" $act; rc=$?
      [ $rc -eq 0 ] || { echo "run $i-$app ended with $rc: see $base/runs/$i-$app/events.jsonl"; }
      # Our own stale lock, if the app was killed.
      [ -e "$sock" ] && [ -z "$(lsof -t "$sock" 2>/dev/null)" ] && rm -f "$sock"
      if ! $py "$here/measure.py" --base "$base" --project-check; then
        echo "the run wrote into the clone outside .openidea (list above); the next run gets a fresh clone"
        mv "$clone" "$base/project/Allocadia-after-$i-$app" && /bin/cp -Rc "$src" "$clone"
        [ -z "$keep" ] && [ -e "$clone/.openidea" ] && mv "$clone/.openidea" "$base/project/openidea-of-the-owner-$i-$app"
      elif [ -z "$keep" ] && [ -d "$clone/.openidea" ]; then
        # The owner's .openidea was moved out before the first run: what is there now the app wrote
        # (changelists.json, .gitignore). Kept for inspection, then the clone is as before.
        mv "$clone/.openidea" "$base/runs/$i-$app/openidea-written-into-the-clone"
      fi
      # The machine back at rest before the next app starts.
      sleep 60
    fi
  done
  i=$((i + 1))
done

# 5. Nothing of ours is left running (recorded PIDs, and anything executing from the base).
if [ -z "$dry" ]; then
  $py "$here/stop.py" --base "$base"
  left=$(ps -axo pid=,command= | grep -F "$base/" | grep -v grep)
  [ -z "$left" ] && echo "nothing from the base is running" || echo "STILL RUNNING from the base (stop by these PIDs only): $left"
else
  echo "[dry-run] $here/stop.py --base $base; ps -axo pid=,command= | grep -F '$base/'"
fi

# 6. The table.
do_ "$py '$here/aggregate.py' --base '$base' > '$base/RESULTS.md'"
say "done: $base/RESULTS.md"
