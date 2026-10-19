#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-hrrr-fast
run="$1"
variant="${2:-compressed}"
table="goal_hrrr_${variant}_run$run"
[[ -f "$bench/hrrr-compressed/conversion.json" ]]
"$bench/evict-stage" "$bench/hrrr-compressed" > "$bench/$variant-$run-input-cache.json"
jq -nc --argjson time "$(date +%s.%N)" '{time:$time}' > "$bench/$variant-$run-start.json"
/usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"max_rss_kib":%M,"filesystem_inputs":%I,"filesystem_outputs":%O,"exit_code":%x}' -o "$bench/$variant-$run-process.json" "$bench/candidate-${3:-v9}" load --root "$bench" --data "$bench/hrrr-compressed" --table "$table" > "$bench/$variant-$run-load.log" 2>&1
curl -fsS http://127.0.0.1:18123 --data-binary "SYSTEM STOP MERGES meteogram_native.$table"
cp "$bench/hrrr-compressed/load-timings.json" "$bench/$variant-$run-load.json"
cp "$bench/hrrr-compressed/validated.json" "$bench/$variant-$run-validation.json"
curl -fsS http://127.0.0.1:18123 --data-binary 'SYSTEM FLUSH LOGS'
curl -fsS http://127.0.0.1:18123 --data-binary "SELECT query_duration_ms,read_rows,read_bytes,ProfileEvents['UserTimeMicroseconds']/1000000 AS user_seconds,ProfileEvents['SystemTimeMicroseconds']/1000000 AS system_seconds,ProfileEvents['OSReadBytes'] AS os_read_bytes,query FROM system.query_log WHERE type='QueryFinish' AND query LIKE 'SELECT toUInt8(count()=%' AND query LIKE '%FROM meteogram_native.$table WHERE%' ORDER BY event_time_microseconds FORMAT JSONEachRow" > "$bench/$variant-$run-range-queries.jsonl"
curl -fsS http://127.0.0.1:18123 --data-binary "SELECT query_duration_ms,read_rows,read_bytes,ProfileEvents['UserTimeMicroseconds']/1000000 AS user_seconds,ProfileEvents['SystemTimeMicroseconds']/1000000 AS system_seconds,ProfileEvents['OSReadBytes'] AS os_read_bytes FROM system.query_log WHERE type='QueryFinish' AND query LIKE 'SELECT count()%FROM meteogram_native.$table WHERE%arrayExists%' ORDER BY event_time_microseconds DESC LIMIT 1 FORMAT JSONEachRow" > "$bench/$variant-$run-validation-query.jsonl"
curl -fsS http://127.0.0.1:18123 --data-binary "SELECT count() AS queries,sum(query_duration_ms)/1000 AS elapsed_sum,sum(ProfileEvents['UserTimeMicroseconds'])/1000000 AS user_seconds,sum(ProfileEvents['SystemTimeMicroseconds'])/1000000 AS system_seconds,sum(written_rows) AS rows,sum(written_bytes) AS bytes FROM system.query_log WHERE type='QueryFinish' AND query LIKE 'INSERT INTO meteogram_native.$table %' FORMAT JSONEachRow" > "$bench/$variant-$run-insert-query-summary.jsonl"
curl -fsS http://127.0.0.1:18123 --data-binary "SELECT table,sum(rows) AS storage_rows,sum(bytes_on_disk) AS bytes_on_disk,count() AS parts FROM system.parts WHERE active AND database='meteogram_native' AND table='$table' GROUP BY table FORMAT JSONEachRow" > "$bench/$variant-$run-parts.jsonl"
jq -nc --argjson time "$(date +%s.%N)" '{time:$time,complete:true}' > "$bench/$variant-$run-complete.json"
