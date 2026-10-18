#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-gfs-dictionary
[[ -s "$bench/preparation-complete.json" ]]
query() { curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "$1"; }
[[ -f "$bench/prepared-raw-metadata.json" ]] || cp "$bench/raw/metadata.json" "$bench/prepared-raw-metadata.json"
[[ -f "$bench/prepared-dictionary-metadata.json" ]] || cp "$bench/dictionary/metadata.json" "$bench/prepared-dictionary-metadata.json"
run() {
  local variant="$1" repeat="$2" source="$bench/dictionary" extra=()
  local label="$variant-r$repeat" table="bench_gfs_dict_v2_${variant}_r$repeat"
  local output="$bench/runs/$label"
  [[ "$variant" != raw ]] || source="$bench/raw"
  [[ "$variant" != packed ]] || extra+=(--packed)
  mkdir -p "$output"
  cp "$bench/prepared-$([[ "$variant" == raw ]] && echo raw || echo dictionary)-metadata.json" "$source/metadata.json"
  "$bench/evict-input" "$source" > "$output/input-cache.json"
  jq -nc --arg variant "$variant" --arg table "$table" --arg source "$source" --argjson repeat "$repeat" --argjson time "$(date +%s.%N)" '{variant:$variant,table:$table,source:$source,repeat:$repeat,workers:4,start:$time}' > "$output/run.json"
  echo "START $label"
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"filesystem_inputs":%I,"filesystem_outputs":%O,"exit_code":%x}' -o "$output/process.json" "$bench/candidate" load --root "$bench" --data "$source" --table "$table" --workers 4 "${extra[@]}" > "$output/load.log" 2>&1
  query "SYSTEM STOP MERGES meteogram_native.$table"
  cp "$source/load-timings.json" "$output/load.json"
  cp "$source/validated.json" "$output/validation.json"
  cp "$source/metadata.json" "$output/metadata.json"
  cp "$source/cells.bin.zst" "$output/cells.bin.zst"
  [[ "$variant" == raw ]] || ln "$source/values.dict.zst" "$output/values.dict.zst"
  query 'SYSTEM FLUSH LOGS'
  query "SELECT query_id,query_duration_ms,read_rows,read_bytes,memory_usage,query FROM system.query_log WHERE type='QueryFinish' AND has(tables,'meteogram_native.$table') AND startsWith(query,'SELECT toUInt8(count()=') ORDER BY event_time_microseconds FORMAT JSONEachRow" > "$output/range-queries.jsonl"
  query "SELECT table,sum(rows) AS storage_rows,sum(bytes_on_disk) AS bytes_on_disk,sum(data_compressed_bytes) AS data_compressed_bytes,sum(data_uncompressed_bytes) AS data_uncompressed_bytes,count() AS parts FROM system.parts WHERE active AND database='meteogram_native' AND table='$table' GROUP BY table FORMAT JSONEachRow" > "$output/parts.jsonl"
  jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$output/complete.json"
  echo "DONE $label $(jq -r .elapsed_seconds "$output/process.json")s"
}
run raw 1
run profile 1
run packed 1
run packed 2
run profile 2
run raw 2
run profile 3
run raw 3
run packed 3
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/loads-complete.json"
