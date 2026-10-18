# GFS/AIFS full loader benchmark evidence

Results and interpretation: [dated report](../../plans/2026-10-18-other-model-load-benchmarks.md).
This bundle records complete local staged loads, not network refresh timings.

`summary.json` contains source pins, full scope, preparation sizes, all 24
successful load processes, median/range summaries, serving storage including
codebooks, and warm complete-profile query distributions. Each of eight
configurations has three fresh-table loads. No load uses a table clone.

## Evidence map

- `runs/*/process.json`: whole CLI elapsed time and client resource counters.
- `runs/*/load.json`: loader work/elapsed counters, pipeline and bounds.
- `runs/*/validation.json`: validation result returned inside the load clock.
- `runs/*/range-summary.json`: actual QueryFinish IDs, disjoint range headers,
  expected/read row counts, byte counts and memory/time counters. Every physical
  row is covered. Raw/profile key gaps are native-grid gaps, not missing cells.
  Final repeats also retain the original `range-queries.jsonl` observations.
- `runs/*/parts.jsonl`: active physical rows, part counts and database bytes.
- `runs/*/input-cache.json`: per-input Linux discard hints before the clock.
- `runs/*-r3/independent-audit.json`: additional whole-table `validate --staged`.
- `runs/*-r3/exact.json`: all coordinates/IDs plus 100 complete profiles per
  model against the immutable reference, including unchanged-model compatibility.
- `runs/*-r3/http.json`: 27 complete HTTP comparisons, bad/outside inputs, gzip.
- `runs/*-r3/queries.json`: 300 warmup and 300 measured complete queries for
  only the changed model, one client. Codebooks are resident before timing.
- `gfs-source-metadata.json`, `aifs-source-metadata.json`: complete source pins
  and schemas; weather payloads/coordinates are not embedded.
- `gfs-prepare.log` and its process result: rejected, incomplete dictionary
  preparation; this is not a successful full loader result.
- `gfs-cardinalities.json`, chunks and script: separate complete exact-bit
  diversity diagnostic under the existing query memory cap.
- `aifs-dictionary.json`: actual per-field AIFS entry/slot counts.
- `*-prepare-process.json`, `aifs-sort-process.json`: preparation outside clocks.
- `source.sha256`, `candidate.sha256`: source/Cargo and frozen release fingerprints.
- `evidence-validation.json` and its script: parsed evidence, checked arithmetic,
  complete matrix, preservation/cleanup assertions and local document links.
  `bundle.sha256` fingerprints all other files in this evidence directory.
- `weather-stage.sha256` and verification log: completion fingerprints of all
  timed weather input payloads, checked before cleanup. Paths are relative to
  the external experiment root; payloads are not committed or retained here.
- Two `*.stage.json` examples: raw Float32 v1 and exact UInt16 v2 frame indexes.
- `tests.log`, `clippy.log`, `format.log`, `build.log`: passed source checks/build.
- `host.json`: host and unchanged memory limits.
- `http-during-benchmark.json`: 353 successful observations per model, no errors,
  one fixed NY point/model. This is not a comprehensive availability guarantee.
- Main catalog/schedule before/after, protected rows, final HTTP/health and
  `service-preservation.json`: installed binary, all twelve original tables and
  publication/schedule state preserved; no restart or installation.
- `cleanup-plan.json`, `cleanup.json`: 24 fresh experiment tables and two input
  clones dropped after verification, plus only owned weather staging removed.

## Reproduction and boundaries

The scripts record the exact commands on this host. Their external root is
`/home/andy/.local/share/meteogram-other-models`; source and result tables use
the isolated `bench_models_*` prefix. Prepare a **fresh experiment root and
unique table names** when repeating them. Do not run the cleanup script on a
different ownership set. Source metadata, coordinates, codebooks and process
logs remain in that external root; the timed tables/weather payloads have been
removed. Large artifacts and the candidate executable are outside Git.

1. Freeze the two complete pinned model tables independently from the main
   catalog. Clone only these preparation inputs and pause only their merges.
2. Prepare complete raw GFS using `prepare-store --raw-only`. Prepare complete
   raw/dictionary AIFS using `prepare-store --raw-output`, then `sort-stage`.
   Never use the incomplete failed GFS dictionary output as loader input.
3. Build one release binary and run the Rust suite, including database tests.
   `run-loads.sh` and `run-repeat3.sh` load all cells/fields/levels/leads in 24
   fresh tables, retaining all checks inside each process clock.
4. `verify-runs.sh` performs independent scans, reference/HTTP comparisons,
   then separate warm profile benchmarks with no heavy preparation/build work.
5. `summarize.mjs` checks range coverage and consolidates evidence before input
   cleanup. `finish.mjs` checks catalog/ownership/binary state, removes only the
   named experiment artifacts, then rechecks original tables and service health.
   `cleanup-failed-staging.mjs` removes the rejected export's unfinished
   `*.stage.zst.tmp` files only from its two isolated failed-output directories.

The file-discard helper is the unchanged
[`evict-stage.rs`](../2026-10-18-hrrr-load-optimization/evict-stage.rs).
Loader checks and complete SQL generation live in the fingerprinted Rust source.
Query-log strings can be truncated by ClickHouse; coverage summaries retain the
range headers and actual read counts, while standalone scans independently
verify every stored value. Physical AIFS tile rows (98) are separate from
logical cells (24,885) and cell/lead rows (771,435).

Normal acquisition and service defaults remain unchanged. `load --packed`
explicitly enables other UInt16 models for experimentation. Full GFS dictionary
preparation is rejected by the unchanged 32-million-entry per-field guard.
