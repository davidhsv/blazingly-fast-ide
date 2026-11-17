#!/bin/zsh
# Formats exactly the named files (stdin mode: out-of-line modules are not followed).
cd ${W}/app-gpui
for f in "$@"; do
  tmp=$(mktemp)
  if rustfmt --edition 2024 --emit stdout < "$f" > "$tmp" 2>/tmp/fmt_err_$$; then
    cmp -s "$f" "$tmp" || { cat "$tmp" > "$f"; echo "formatted $f"; }
  else echo "rustfmt failed on $f"; cat /tmp/fmt_err_$$; fi
  rm -f "$tmp"
done
