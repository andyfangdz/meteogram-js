map(. + (.query | capture("count\\(\\)=(?<expected>[0-9]+).*cell_id>=(?<lo>[0-9]+) AND cell_id<=(?<hi>[0-9]+)") | map_values(tonumber)))
| sort_by(.lo) as $r
| {
  queries:($r|length),
  read_rows:($r|map(.read_rows)|add),
  expected_rows:($r|map(.expected)|add),
  read_bytes:($r|map(.read_bytes)|add),
  os_read_bytes:($r|map(.os_read_bytes)|add),
  user_seconds:($r|map(.user_seconds)|add),
  system_seconds:($r|map(.system_seconds)|add),
  wall_work_seconds:(($r|map(.query_duration_ms)|add)/1000),
  all_range_rows_match:all($r[];.read_rows==.expected),
  ranges_disjoint:all(range(1;($r|length));$r[.].lo>$r[.-1].hi),
  minimum_expected_rows:($r|map(.expected)|min),
  first_cell:$r[0].lo,
  last_cell:$r[-1].hi
}
