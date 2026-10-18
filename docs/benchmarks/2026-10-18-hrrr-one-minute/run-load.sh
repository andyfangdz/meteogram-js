#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-hrrr-one-minute
tag="$1"
binary="$2"
stage="$3"
table="goal_hrrr_60_$tag"
args=(load --root "$bench" --data "$stage" --table "$table")
if [[ -n "${4:-}" ]]; then args+=(--workers "$4"); fi
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT now64(6) AS time,(SELECT sum(value) FROM system.errors WHERE name='MEMORY_LIMIT_EXCEEDED') AS memory_errors,(SELECT value FROM system.asynchronous_metrics WHERE metric='MemoryResident') AS server_resident_bytes FORMAT JSONEachRow" > "$bench/$tag-server-before.json"
/home/andy/.local/share/meteogram-hrrr-fast/evict-stage "$stage" > "$bench/$tag-input-cache.json"
jq -nc --argjson time "$(date +%s.%N)" '{time:$time}' > "$bench/$tag-start.json"
/usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"filesystem_inputs":%I,"filesystem_outputs":%O,"exit_code":%x}' -o "$bench/$tag-process.json" "$binary" "${args[@]}" > "$bench/$tag-load.log" 2>&1
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT now64(6) AS time,(SELECT sum(value) FROM system.errors WHERE name='MEMORY_LIMIT_EXCEEDED') AS memory_errors,(SELECT value FROM system.asynchronous_metrics WHERE metric='MemoryResident') AS server_resident_bytes FORMAT JSONEachRow" > "$bench/$tag-server-after.json"
cp "$stage/load-timings.json" "$bench/$tag-load.json"
cp "$stage/validated.json" "$bench/$tag-validation.json"
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SYSTEM STOP MERGES meteogram_native.$table"
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary 'SYSTEM FLUSH LOGS'
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT DISTINCT query_id,query_duration_ms,read_rows,read_bytes,ProfileEvents['UserTimeMicroseconds']/1000000 AS user_seconds,ProfileEvents['SystemTimeMicroseconds']/1000000 AS system_seconds,ProfileEvents['OSReadBytes'] AS os_read_bytes,extract(query,'count[(][)]=([0-9]+)') AS expected_rows,extract(query,'min[(]([a-z_]+)[)]=') AS range_key,extract(query,'min[(](?:cell_id|tile_id)[)]=([0-9]+)') AS range_first,extract(query,'max[(](?:cell_id|tile_id)[)]=([0-9]+)') AS range_last FROM system.query_log WHERE type='QueryFinish' AND query LIKE 'SELECT toUInt8(count()=%' AND has(tables,'meteogram_native.$table') FORMAT JSONEachRow" > "$bench/$tag-range-queries.jsonl"
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT count() AS queries,sum(written_rows) AS rows,sum(written_bytes) AS bytes,sum(ProfileEvents['UserTimeMicroseconds'])/1000000 AS user_seconds,sum(ProfileEvents['SystemTimeMicroseconds'])/1000000 AS system_seconds FROM system.query_log WHERE type='QueryFinish' AND query LIKE 'INSERT INTO meteogram_native.$table %' FORMAT JSONEachRow" > "$bench/$tag-insert-summary.jsonl"
curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT table,sum(rows) AS rows,sum(bytes_on_disk) AS bytes,count() AS parts FROM system.parts WHERE active AND database='meteogram_native' AND table='$table' GROUP BY table FORMAT JSONEachRow" > "$bench/$tag-parts.jsonl"
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/$tag-complete.json"
