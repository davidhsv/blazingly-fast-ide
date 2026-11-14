#!/bin/sh
# usage: shot.sh <name> <wait seconds> <args to the binary...>
I="$(cd "$(dirname "$0")" && pwd)"
BIN=/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/target/debug/openidea-gpui
name="$1"; wait="$2"; shift 2
OPENIDEA_GPUI_HOME="$I/home" "$BIN" "$@" > "$I/shots/$name.log" 2>&1 &
pid=$!
echo "pid $pid"
sleep "$wait"
swift "$I/windows.swift" "$pid" > "$I/shots/$name.windows.txt" 2>&1
cat "$I/shots/$name.windows.txt"
n=0
while read id layer onscreen w h; do
  [ "$onscreen" = "true" ] || continue
  [ "${w%.*}" -gt 200 ] 2>/dev/null || continue
  n=$((n+1))
  screencapture -x -o -l "$id" "$I/shots/$name-$n.png" && echo "captured $name-$n.png (window $id ${w}x${h})"
done < "$I/shots/$name.windows.txt"
ps -o rss=,pcpu= -p "$pid" | awk '{printf "rss %.0f MB cpu %s%%\n", $1/1024, $2}'
kill "$pid" 2>/dev/null; sleep 1; kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
tail -3 "$I/shots/$name.log" | cut -c1-200
