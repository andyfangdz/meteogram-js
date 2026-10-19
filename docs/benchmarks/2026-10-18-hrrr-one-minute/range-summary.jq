map(.expected_rows |= tonumber | .range_first |= tonumber | .range_last |= tonumber)
| sort_by(.range_first) as $r
| ($metadata[0].cells / $metadata[0].storage.tile_size | ceil) as $rows
| {
  queries: ($r | length),
  key: "tile_id",
  read_rows: ($r | map(.read_rows) | add),
  expected_rows: ($r | map(.expected_rows) | add),
  read_bytes: ($r | map(.read_bytes) | add),
  os_read_bytes: ($r | map(.os_read_bytes) | add),
  user_seconds: ($r | map(.user_seconds) | add),
  system_seconds: ($r | map(.system_seconds) | add),
  wall_work_seconds: (($r | map(.query_duration_ms) | add) / 1000),
  all_range_rows_match: all($r[]; .read_rows == .expected_rows and .expected_rows == (.range_last - .range_first + 1) and .range_key == "tile_id"),
  ranges_contiguous_disjoint: all(range(1; ($r | length)); $r[.].range_first == $r[.-1].range_last + 1),
  all_physical_rows_covered: ($r[0].range_first == 0 and $r[-1].range_last == $rows - 1 and ($r | map(.read_rows) | add) == $rows),
  logical_cells: $metadata[0].cells,
  logical_cell_lead_rows: ($metadata[0].cells * ($metadata[0].leads | length)),
  tile_size: $metadata[0].storage.tile_size,
  first_tile: $r[0].range_first,
  last_tile: $r[-1].range_last
}
