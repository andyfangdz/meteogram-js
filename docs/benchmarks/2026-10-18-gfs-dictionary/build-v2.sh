#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-gfs-dictionary
export PATH=/home/andy/.local/share/meteogram-rust/rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin:/usr/bin:/bin
export CARGO_HOME=/home/andy/.cache/meteogram-native-cargo
export CARGO_TARGET_DIR=/home/andy/.cache/meteogram-native-target
cd /home/andy/.local/share/meteogram-benchmark/repo
cargo fmt --check --manifest-path native/Cargo.toml > "$bench/format-v2.log" 2>&1
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings > "$bench/clippy-v2.log" 2>&1
cargo build --release --locked --manifest-path native/Cargo.toml > "$bench/build-v2.log" 2>&1
cp "$CARGO_TARGET_DIR/release/meteogram-native" "$bench/candidate"
sha256sum "$bench/candidate" > "$bench/candidate.sha256"
sha256sum native/src/*.rs native/Cargo.toml native/Cargo.lock > "$bench/source.sha256"
echo 'FINAL CANDIDATE BUILT'
