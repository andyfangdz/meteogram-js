# Native ingestion optimization evidence

> Historical benchmark evidence. Results and implementation details describe the
> recorded version; use the [current native service guide](../../../native/README.md)
> for supported operation.

See the [measured report](../../archive/2026-10-18-native-ingestion-optimization.md).

- `pack-*-idle.json`: resident synthetic kernel comparisons, seven timed
  repetitions after warmup; every output bit checked.
- `baseline-*`, `tile-only-*-load.json`, `full-load-comparison.json`: complete
  real staged-data loads. The shipped implementation is the cell-tiled variant.
  Its HRRR validation overlapped follow-up compilation; no full-load speedup is
  established by these runs.
- `prefetch-*` and `prefetch-experiment.patch`: rejected read-ahead experiment;
  the hints are absent from the shipped loader.
- `incomplete-*`, `historical-incomplete*`: replay of the same incomplete pinned
  snapshot, rejected before map downloads by the new reference check.
- `cache-*`, `aifs-cache-summary.json`: completed-cache cleanup and fresh AIFS
  cache observations. Space retention changes; downloads/initial writes remain.
- Model metadata, source summaries and validation files: full geographic/lead
  coverage, physical row counts and finite/staged data gates.
- `exact-*`, `candidate-http.json`: shipped implementation compared with the
  published store and API; `main-preservation.json` verifies unchanged profile
  hashes after installation.
- `http-during-benchmark.json`: fixed-point production observations during
  acquisition, loads and cleanup. The published catalog did not change.
- `cleanup-*`, `rows-*-cleanup.jsonl`: explicit experimental-table removal plan
  and preservation of all protected row counts.
- Build/test/lint logs and SHA-256 files identify the verified source/binaries.
  Large data and executables remain outside Git.
