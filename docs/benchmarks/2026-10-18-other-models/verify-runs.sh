#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-other-models
[[ -s "$bench/third-repeat-complete.json" ]]
for run in "$bench"/runs/*-r3; do
  label="$(basename "$run")"
  model="$(jq -r .model "$run/run.json")"
  table="$(jq -r .table "$run/run.json")"
  source="$(jq -r .source "$run/run.json")"
  for staged in "$source"/*.stage.json "$source"/*.stage.zst; do
    ln -s "$staged" "$run/$(basename "$staged")"
  done
  echo "AUDIT $label"
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"max_rss_kib":%M,"exit_code":%x}' -o "$run/audit-process.json" "$bench/candidate" validate --data "$run" --table "$table" --staged > "$run/independent-audit.json"
  root="$bench/catalogs/$label"
  mkdir -p "$root/revisions"
  for ref in "$bench"/reference/revisions/*; do
    ln -s "$ref" "$root/revisions/$(basename "$ref")"
  done
  ln -s "$run" "$root/revisions/$table"
  jq --arg model "$model" --arg table "$table" '.current |= with_entries(.value.owned=false) | .current[$model]={revision:$table,database:"meteogram_native",table:$table,owned:true}' "$bench/reference/catalog.json" > "$root/catalog.json"
  "$bench/candidate" compare-stores --root "$root" --reference-root "$bench/reference" --samples 100 --output "$run/exact.json" > "$run/exact.log"
  "$bench/candidate" check-api --root "$root" --port 18082 --compare-url http://127.0.0.1:18080 --output "$run/http.json" > "$run/http.log"
  jq --arg model "$model" '.current |= with_entries(select(.key==$model))' "$root/catalog.json" > "$root/query-catalog.json"
  query_root="$bench/query-catalogs/$label"
  mkdir -p "$query_root"
  ln -s "$root/revisions" "$query_root/revisions"
  cp "$root/query-catalog.json" "$query_root/catalog.json"
  echo "VERIFIED $label"
done
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/audits-complete.json"
# Profile measurements run after all scans/HTTP audits, with no source preparation
# or build competing for resources. Each model gets 300 warmup + 300 measured queries.
for root in "$bench"/query-catalogs/*; do
  label="$(basename "$root")"
  echo "QUERY $label"
  "$bench/candidate" benchmark --root "$root" --count 300 --clients 1 --output "$bench/runs/$label/queries.json" > "$bench/runs/$label/queries.log"
done
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/verification-complete.json"
