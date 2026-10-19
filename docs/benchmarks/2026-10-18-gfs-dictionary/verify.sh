#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-gfs-dictionary
[[ -s "$bench/loads-complete.json" ]]
for variant in raw profile packed; do
  run="$bench/runs/$variant-r3"
  table="$(jq -r .table "$run/run.json")"
  source="$(jq -r .source "$run/run.json")"
  for staged in "$source"/*.stage.json "$source"/*.stage.zst; do ln -s "$staged" "$run/$(basename "$staged")"; done
  echo "AUDIT $variant"
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"max_rss_kib":%M,"exit_code":%x}' -o "$run/audit-process.json" "$bench/candidate" validate --data "$run" --table "$table" --staged > "$run/independent-audit.json"
  root="$bench/catalogs/$variant"
  mkdir -p "$root/revisions"
  for ref in "$bench"/reference/revisions/*; do ln -s "$ref" "$root/revisions/$(basename "$ref")"; done
  ln -s "$run" "$root/revisions/$table"
  jq --arg table "$table" '.current |= with_entries(.value.owned=false) | .current.gfs={revision:$table,database:"meteogram_native",table:$table,owned:true}' "$bench/reference/catalog.json" > "$root/catalog.json"
  "$bench/candidate" compare-stores --root "$root" --reference-root "$bench/reference" --samples 100 --output "$run/exact.json" > "$run/exact.log"
  "$bench/candidate" check-api --root "$root" --port 18082 --compare-url http://127.0.0.1:18080 --output "$run/http.json" > "$run/http.log"
  query_root="$bench/query-catalogs/$variant"
  mkdir -p "$query_root"
  ln -s "$root/revisions" "$query_root/revisions"
  jq '.current |= with_entries(select(.key=="gfs"))' "$root/catalog.json" > "$query_root/catalog.json"
  echo "VERIFIED $variant"
done
# Recover every Float32 value from both actual dictionary database layouts.
# The existing raw exporter reads all cells/fields/slots, not sampled profiles.
for variant in profile packed; do
  run="$bench/runs/$variant-r3"
  table="$(jq -r .table "$run/run.json")"
  echo "FULL ROUNDTRIP $variant"
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"max_rss_kib":%M,"exit_code":%x}' -o "$run/roundtrip-process.json" "$bench/candidate" prepare-store --raw-only --source "$run" --table "$table" --output "$bench/roundtrip-$variant" > "$run/roundtrip.log" 2>&1
done
/home/andy/.local/bin/bun "$bench/check-roundtrip.mjs"
for variant in raw profile packed; do
  echo "QUERY $variant"
  "$bench/candidate" benchmark --root "$bench/query-catalogs/$variant" --count 300 --clients 1 --output "$bench/runs/$variant-r3/queries.json" > "$bench/runs/$variant-r3/queries.log"
done
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/verification-complete.json"
