#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-other-models
metadata="$bench/source-gfs/metadata.json"
mapfile -t fields < <(jq -r '(.variables+.surface_variables)[]' "$metadata")
nf=$(jq '.variables|length' "$metadata")
nt=$(jq '.leads|length' "$metadata")
nl=$(jq '.levels|length' "$metadata")
: > "$bench/gfs-cardinality-chunks.jsonl"
for ((f=0;f<${#fields[@]};f++)); do
  field="${fields[f]}"
  width=$nt; (( f>=nf )) || width=$((nt*nl))
  for ((first=1;first<=width;first+=512)); do
    count=$((width-first+1)); ((count<=512)) || count=512
    curl --noproxy '*' --fail-with-body -sS http://127.0.0.1:18123 --data-binary "SELECT uniqExactForEach(arraySlice($field,$first,$count)) AS counts FROM meteogram_native.bench_models_gfs_source_20261018_06 SETTINGS max_threads=1,max_block_size=512 FORMAT JSONEachRow" | jq -c --arg field "$field" --argjson first "$first" '.+{field:$field,first_slot:$first}' >> "$bench/gfs-cardinality-chunks.jsonl"
  done
done
jq -s 'group_by(.field)|map({field:.[0].field,slots:(map(.counts|length)|add),entries:(map(.counts|map(tonumber)|add)|add),max_cardinality:(map(.counts|map(tonumber)|max)|max)})' "$bench/gfs-cardinality-chunks.jsonl" > "$bench/gfs-cardinalities.json"
date -u '+%Y-%m-%dT%H:%M:%SZ' > "$bench/gfs-cardinality-finished.txt"
