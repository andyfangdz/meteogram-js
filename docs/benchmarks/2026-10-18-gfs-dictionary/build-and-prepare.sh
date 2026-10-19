#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-gfs-dictionary
repo=/home/andy/.local/share/meteogram-benchmark/repo
export PATH=/home/andy/.local/share/meteogram-rust/rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin:/usr/bin:/bin
export CARGO_HOME=/home/andy/.cache/meteogram-native-cargo
export CARGO_TARGET_DIR=/home/andy/.cache/meteogram-native-target
cd "$repo"
cargo fmt --manifest-path native/Cargo.toml
cargo test --locked --manifest-path native/Cargo.toml -- --include-ignored > "$bench/tests.log" 2>&1
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings > "$bench/clippy.log" 2>&1
cargo fmt --check --manifest-path native/Cargo.toml > "$bench/format.log" 2>&1
cargo build --release --locked --manifest-path native/Cargo.toml > "$bench/build.log" 2>&1
cp "$CARGO_TARGET_DIR/release/meteogram-native" "$bench/candidate"
sha256sum "$bench/candidate" > "$bench/candidate.sha256"
sha256sum native/src/*.rs native/Cargo.toml native/Cargo.lock > "$bench/source.sha256"
echo 'BUILD AND TESTS COMPLETE'
/usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"exit_code":%x}' -o "$bench/raw-prepare-process.json" "$bench/candidate" prepare-store --raw-only --source "$bench/reference/revisions/bench_gfs_dict_source_20261018_06" --table bench_gfs_dict_source_20261018_06 --output "$bench/raw" > "$bench/raw-prepare.log" 2>&1
echo 'RAW EXPORT COMPLETE'
/usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"exit_code":%x}' -o "$bench/dictionary-prepare-process.json" "$bench/candidate" encode-stage --source "$bench/raw" --output "$bench/dictionary" > "$bench/dictionary-prepare.log" 2>&1
echo 'FULL DICTIONARY COMPLETE'
"$bench/candidate" inspect-dictionary --data "$bench/dictionary" > "$bench/dictionary-cardinalities.json"
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/preparation-complete.json"
