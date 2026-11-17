#!/bin/zsh
# Formats exactly the named files (stdin mode: out-of-line modules are not followed).
cd ${W}/app-gpui
for f in "$@"; do
  mkdir -p ${S}/fmt_tmp; tmp=$(mktemp ${S}/fmt_tmp/out.XXXXXX); err=${S}/fmt_tmp/err_$$
  if rustfmt --edition 2024 --emit stdout < "$f" > "$tmp" 2>"$err"; then
    cmp -s "$f" "$tmp" || { cat "$tmp" > "$f"; echo "formatted $f"; }
  else echo "rustfmt failed on $f"; cat "$err"; fi
  rm -f "$tmp" "$err"
done
