#!/usr/bin/env bash
set -euo pipefail
bench=/home/andy/.local/share/meteogram-hrrr-one-minute
tag="$1"
until [[ -s "$bench/$tag-start.json" ]]; do sleep 1; done
while [[ ! -s "$bench/$tag-complete.json" ]]; do
  if [[ -s "$bench/$tag-process.json" ]] && jq -e '.exit_code!=0' "$bench/$tag-process.json" > /dev/null; then break; fi
  curl --noproxy '*' -fsS http://127.0.0.1:18123 --data-binary "SELECT now64(6) AS time,(SELECT value FROM system.asynchronous_metrics WHERE metric='MemoryResident') AS server_resident_bytes,(SELECT sum(value) FROM system.errors WHERE name='MEMORY_LIMIT_EXCEEDED') AS memory_errors,(SELECT count() FROM system.processes WHERE query LIKE 'INSERT INTO meteogram_native.goal_hrrr_60_$tag %') AS active_inserts FORMAT JSONEachRow" >> "$bench/$tag-server-memory.jsonl"
  sleep 2
done
