# Home full-resident evidence, October 17, 2026

> Historical benchmark evidence. Results and implementation details describe the
> recorded version; use the [current native service guide](../../../../native/README.md)
> for supported operation.

The [report](../../../archive/2026-10-17-clickhouse-home-resident.md) describes one
complete resident CONUS HRRR/GFS/AIFS cycle on the home ARM host. This bundle is
approximately 1 MB; forecast arrays, encoded source chunks, executables, Python
environment and full logs remain outside Git.

- `*-source.json`: exact source/grid metadata and every completed lead's source
  bytes, timings, CPU, RSS, writes, finite counts and Float32 errors. Source
  metadata open time can reflect a pinned checkpoint resume; initial HRRR
  selection is preserved in `initial-acquisition.json` and original GFS command
  elapsed time in `phases.jsonl`.
- `*-benchmark.json`: ingestion, initial query passes, key validation and exact
  eight-cell Float32 checks. The `warm` query mode here is the cache-enabled first
  pass, without explicit full warm-up. HRRR's report predates the added
  `snapshot_id` field; its source file records the immutable pinned snapshot.
- `api-verification.json`: 56 real locations, all fields/times/levels checked,
  invalid/outside responses, 1,200 timed API requests, final table-wide coverage,
  final resident storage, explicitly warmed/direct-I/O/concurrent database
  queries, and OS-read query-log counters.
- `*-staging.json`: per-lead lossless compression sizes and codec timings. These
  are measurements of a proposed writer layout, not persisted compressed staging.
- `gfs-failed-insert.json`: the 4,096-cell GFS Arrow insert exceeded the configured
  2 GiB query limit; the 1,024-cell retry passed from the same source staging.
- `edge-regression.json`: the real HRRR projected edge rounding failure and fix.
- `host.json`, `host-restart-*.json`, `lscpu.txt`, `storage-device.json`,
  `filesystem.txt`, `python-packages.txt`, `clickhouse-archive.sha256`: host and
  software provenance. The original checkout `.git` was read-only; the writable
  clone started from the recorded commit. Final committed scripts include the
  fixes discovered during this experiment.
- `phases.jsonl`, `complete.json`: successful/failed subprocess phases and overall
  runner completion, including the resumed attempt. The first standalone HRRR
  acquisition predates the runner; its per-lead counters and initial log-creation
  timestamp provide the initial timing.
- `resource-series.json`: every periodic sample used for the resource summary,
  encoded as a `columns` schema plus compact `rows`. Null server fields mean the
  child was not yet visible. Server counters reset on restart: sum each attempt's
  `max(server_write_bytes) - min(server_write_bytes)`. Host block counters span
  the entire sample interval. Minimum free/available memory and peak RSS are
  sampled, not continuous maxima. Summed RSS includes shared/file-backed pages.
- `parts-telemetry.jsonl`: active and obsolete part bytes. Sampling started partway
  through HRRR loading; its maximum is an observed lower bound on the actual
  transient peak, not an upper bound. It does not count unfinished merge outputs.
- `summary.json`: reproducible arithmetic from `summarize-run.py`, with explicit
  120-cycle monthly and retention/electricity assumptions. Original detailed
  `telemetry.jsonl` is retained locally; `resource-series.json` preserves every
  sampled value used in these resource calculations without unrelated counters.

The footprint budgets separately include obsolete parts and a merge-output
allowance. Neither a second-cycle atomic publication nor sustained public/WAN
service was tested. Process/block write counters do not measure NAND writes.
