#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-other-models
evict=/home/andy/.local/share/meteogram-hrrr-fast/evict-stage
query() { curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "$1"; }
run() {
  local model="$1" variant="$2" workers="$3" repeat="$4" source="$5"
  local label="$model-$variant-w$workers-r$repeat"
  local table="bench_models_${model}_${variant}_w${workers}_r${repeat}"
  local output="$bench/runs/$label"
  local extra=()
  [[ "$variant" != packed ]] || extra+=(--packed)
  mkdir -p "$output"
  "$evict" "$source" > "$output/input-cache.json"
  jq -nc --arg model "$model" --arg variant "$variant" --arg table "$table" --arg source "$source" --argjson workers "$workers" --argjson repeat "$repeat" --argjson time "$(date +%s.%N)" '{model:$model,variant:$variant,workers:$workers,repeat:$repeat,table:$table,source:$source,start:$time}' > "$output/run.json"
  echo "START $label"
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"filesystem_inputs":%I,"filesystem_outputs":%O,"exit_code":%x}' -o "$output/process.json" "$bench/candidate" load --root "$bench" --data "$source" --table "$table" --workers "$workers" "${extra[@]}" > "$output/load.log" 2>&1
  query "SYSTEM STOP MERGES meteogram_native.$table"
  cp "$source/load-timings.json" "$output/load.json"
  cp "$source/validated.json" "$output/validation.json"
  cp "$source/metadata.json" "$output/metadata.json"
  cp "$source/cells.bin.zst" "$output/cells.bin.zst"
  [[ ! -f "$source/values.dict.zst" ]] || cp "$source/values.dict.zst" "$output/values.dict.zst"
  query 'SYSTEM FLUSH LOGS'
  query "SELECT query_id,query_duration_ms,read_rows,read_bytes,memory_usage,ProfileEvents['UserTimeMicroseconds']/1000000 AS user_seconds,ProfileEvents['SystemTimeMicroseconds']/1000000 AS system_seconds,query FROM system.query_log WHERE type='QueryFinish' AND has(tables,'meteogram_native.$table') AND startsWith(query,'SELECT toUInt8(count()=') ORDER BY event_time_microseconds FORMAT JSONEachRow" > "$output/range-queries.jsonl"
  query "SELECT count() AS queries,sum(query_duration_ms)/1000 AS elapsed_sum,sum(written_rows) AS rows,sum(written_bytes) AS bytes,max(memory_usage) AS max_memory_usage FROM system.query_log WHERE type='QueryFinish' AND startsWith(query,'INSERT INTO meteogram_native.$table ') FORMAT JSONEachRow" > "$output/insert-summary.jsonl"
  query "SELECT table,sum(rows) AS storage_rows,sum(bytes_on_disk) AS bytes_on_disk,sum(data_compressed_bytes) AS data_compressed_bytes,sum(data_uncompressed_bytes) AS data_uncompressed_bytes,count() AS parts FROM system.parts WHERE active AND database='meteogram_native' AND table='$table' GROUP BY table FORMAT JSONEachRow" > "$output/parts.jsonl"
  jq -nc --argjson time "$(date +%s.%N)" '{finished:$time,complete:true}' > "$output/complete.json"
  echo "DONE $label $(jq -r .elapsed_seconds "$output/process.json")s"
}
# Reverse execution order on the second repeat. Every run inserts complete
# staged input into a new table and includes every loader validation gate.
run gfs raw 1 1 "$bench/raw-gfs"
run gfs raw 4 1 "$bench/raw-gfs"
run aifs raw 1 1 "$bench/raw-aifs"
run aifs raw 4 1 "$bench/raw-aifs"
run aifs dictionary 1 1 "$bench/sorted-aifs"
run aifs dictionary 4 1 "$bench/sorted-aifs"
run aifs packed 1 1 "$bench/sorted-aifs"
run aifs packed 4 1 "$bench/sorted-aifs"
run aifs packed 4 2 "$bench/sorted-aifs"
run aifs packed 1 2 "$bench/sorted-aifs"
run aifs dictionary 4 2 "$bench/sorted-aifs"
run aifs dictionary 1 2 "$bench/sorted-aifs"
run aifs raw 4 2 "$bench/raw-aifs"
run aifs raw 1 2 "$bench/raw-aifs"
run gfs raw 4 2 "$bench/raw-gfs"
run gfs raw 1 2 "$bench/raw-gfs"
jq -nc --argjson time "$(date +%s.%N)" '{finished:$time,complete:true}' > "$bench/loads-complete.json"
