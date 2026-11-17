#!/bin/bash
cd ${W}/app-gpui || exit 1
export PATH=${S}/bin:$PATH
df -k /System/Volumes/Data | tail -1 > ${S}/verify_agv_df.log
for n in 1 2; do
  { echo "uptime before: $(uptime)"; cargo test --lib 2>&1; echo "rc=$?"; echo "uptime after: $(uptime)"; } > ${S}/verify_agv_suite$n.log
done
{ cargo test --test web_view 2>&1; echo "rc=$?"; } > ${S}/verify_agv_web.log
{ cargo clippy --all-targets -- -D warnings 2>&1; echo "rc=$?"; } > ${S}/verify_agv_clippy.log
{ cargo build 2>&1; echo "rc=$?"; } > ${S}/verify_agv_build.log
{ (cd .. && node docs/gpui-port/gen-data.mjs --check 2>&1); echo "rc=$?"; } > ${S}/verify_agv_gen.log
python3 ${S}/reach.py ${W} ${S}/verify_agv_reach.json > ${S}/verify_agv_reach.log 2>&1
cargo test --lib -- --list > ${S}/listed/lib.txt 2>/dev/null
grep ': test$' ${S}/listed/lib.txt | sed 's/: test$//' > ${S}/rust_tests.txt
echo done > ${S}/verify_agv_done
