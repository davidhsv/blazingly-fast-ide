#!/bin/sh
# usage: INSPECT_BASE=<neutral folder> run.sh <name> <target folder or file> [trust]
# Launches the debug binary on an isolated home with <name>.script, captures its window at every mark, then stops it by PID.
I="$(cd "$(dirname "$0")" && pwd)"; name=$1; target=$2
BIN=${W}/app-gpui/target/debug/openidea-gpui
# The app's home and the HOME it sees live under INSPECT_BASE: a neutral folder (views draw paths), not this one.
base="${INSPECT_BASE:?set INSPECT_BASE to a neutral folder such as /private/tmp/openidea-inspect-XXXXXX}"
home="$base/home-$name"; rm -rf "$home" "$I/$name.log" "$base/fakehome"; mkdir -p "$home/data" "$base/fakehome"
[ "$3" = trust ] && printf '{"projects":["%s"],"locations":[],"restricted":[]}' "$target" > "$home/data/project-trust.json"
prev=$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')
PATH="${APP_PATH:-/usr/bin:/bin:/usr/sbin:/sbin}" HOME="$base/fakehome" ZDOTDIR="$base/fakehome" OPENIDEA_GPUI_HOME="$home" OPENIDEA_GPUI_INSPECT="$I/$name.script" OPENIDEA_GPUI_INSPECT_LOG="$I/$name.log" "$BIN" "$target" > "$I/$name.stdout" 2>&1 &
pid=$!; echo $pid > "$I/app.pid"; seen=0; start=$(date +%s)
while kill -0 $pid 2>/dev/null; do
  [ $(( $(date +%s) - start )) -gt ${LIMIT:-240} ] && { echo "time limit"; break; }
  n=$(grep -c '^mark' "$I/$name.log" 2>/dev/null); n=${n:-0}
  while [ "$seen" -lt "$n" ]; do
    seen=$((seen+1)); mark=$(grep '^mark' "$I/$name.log" | sed -n "${seen}p" | cut -f2); k=0
    "$I/click" $pid 0 0 where >> "$I/$name.pointer" 2>&1
    case "$mark" in
    esac
    "$I/windows" $pid | while read id layer onscreen w h; do
      [ "$onscreen" = true ] || continue; [ "${w%.*}" -gt 150 ] 2>/dev/null || continue
      k=$((k+1)); f="$I/shots/$name-$(printf %02d $seen)-$mark-$k.png"
      screencapture -x -o -l "$id" "$f" && sips -Z 1500 "$f" >/dev/null 2>&1 && echo "$(basename "$f") ${w}x${h}"
    done
  done
  grep -q '^done' "$I/$name.log" 2>/dev/null && break
  sleep 0.2
done
[ -n "$MENUS" ] && osascript -e "tell application \"System Events\" to get name of every menu bar item of menu bar 1 of (first process whose unix id is $pid)" > "$I/$name.menus" 2>&1
ps -o rss=,%cpu= -p $pid > "$I/$name.ps" 2>/dev/null
kill $pid 2>/dev/null; sleep 1; kill -0 $pid 2>/dev/null && kill -9 $pid
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $prev) to true" 2>/dev/null
grep -E '^(fail|note|done)' "$I/$name.log"
