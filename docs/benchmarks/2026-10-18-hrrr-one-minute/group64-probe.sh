#!/usr/bin/env bash
set -euo pipefail
root=/home/andy/.local/share/meteogram-hrrr-one-minute
source=goal_hrrr_60_bounded01
sql() { curl --noproxy '*' --fail-with-body -sS http://127.0.0.1:18123 --data-binary "$1"; }
mapfile -t fields < <(jq -r '(.variables+.surface_variables)[]' "$root/sorted-stage/metadata.json")
tuple_fields=$(IFS=,;echo "${fields[*]}")
for group in 1 4 16; do
  table="goal_hrrr_60_group64_probe_$group"
  sql "CREATE TABLE meteogram_native.$table AS meteogram_native.$source"
  sql "SYSTEM STOP MERGES meteogram_native.$table"
  vars=();args=()
  for ((i=0;i<group;i++)); do vars+=("w$i");args+=("anyIf(cell_ids,modulo(tile_id,$group)=$i)");done
  params=$(IFS=,;echo "${vars[*]}")
  id_args=$(IFS=,;echo "${args[*]}")
  expressions=("any(revision_id)" "toUInt32(intDiv(tile_id,$group)) AS group_key" "arrayConcat($id_args)")
  for ((f=0;f<8;f++)); do
    width=1470; (( f<6 )) || width=49
    field="${fields[f]}"
    if ((group==1)); then expressions+=("any($field)");continue;fi
    args=()
    for ((i=0;i<group;i++)); do
      value="anyIf($field,modulo(tile_id,$group)=$i)"
      args+=("arraySplit((x,i) -> modulo(i-1,8)=0,$value,arrayEnumerate($value))")
    done
    field_args=$(IFS=,;echo "${args[*]}")
    expressions+=("arrayFlatten(arrayMap(($params)->arrayConcat($params),$field_args))")
  done
  columns=$(IFS=,;echo "${expressions[*]}")
  /usr/bin/time --quiet -f '{"elapsed_seconds":%e,"user_seconds":%U,"system_seconds":%S,"exit_code":%x}' -o "$root/group64-$group-process.json" curl --noproxy '*' --fail-with-body -sS http://127.0.0.1:18123 --data-binary "INSERT INTO meteogram_native.$table SELECT $columns FROM meteogram_native.$source WHERE (tile_id>=2048 AND tile_id<2112) OR (tile_id>=8192 AND tile_id<8256) OR (tile_id>=14336 AND tile_id<14400) OR (tile_id>=20480 AND tile_id<20544) GROUP BY group_key SETTINGS max_threads=1,max_insert_threads=1,max_block_size=16"
  sql "SELECT $group*64 AS tile_cells,sum(rows) AS physical_rows,sum(bytes_on_disk) AS bytes FROM system.parts WHERE active AND database='meteogram_native' AND table='$table' FORMAT JSONEachRow" > "$root/group64-$group-parts.json"
  sql "DROP TABLE meteogram_native.$table SYNC"
done
