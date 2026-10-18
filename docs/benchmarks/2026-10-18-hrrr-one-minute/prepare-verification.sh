#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-hrrr-one-minute
main=/home/andy/.local/share/meteogram-native
table="$1"
data="$2"
target="$bench/verify-$table"
mkdir -p "$target/revisions"
jq --arg table "$table" '{version:1,generation:1,current:(.current|with_entries(.value.owned=false)),previous:{},retired:[]}|.current.hrrr={revision:$table,database:"meteogram_native",table:$table,owned:true}' "$bench/main-catalog-before.json" > "$target/catalog.json"
for model in hrrr gfs aifs; do
  revision=$(jq -r --arg model "$model" '.current[$model].revision' "$target/catalog.json")
  source="$main/revisions/$revision"
  [[ "$model" == hrrr ]] && source="$data"
  mkdir -p "$target/revisions/$revision"
  cp "$source/metadata.json" "$source/cells.bin.zst" "$target/revisions/$revision/"
  [[ ! -f "$source/values.dict.zst" ]] || cp "$source/values.dict.zst" "$target/revisions/$revision/"
done
