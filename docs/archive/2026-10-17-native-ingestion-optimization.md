# Native ingestion optimization — measured on the home server

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

Follow-up: the [Rust/profile implementation](2026-10-18-native-rust-optimization.md)
completed the same pinned cycle in 16m00s and reduced resident data to 25.935 GB.
The measurements below remain the baseline for that change.

The complete pinned HRRR/GFS/AIFS cycle finished in **20 minutes 11 seconds**,
compared with **35 minutes 4 seconds** for the earlier live update. HRRR loading,
validation and publication fell from **15:04 to 11:03**. The next model's download
now overlaps the current model's loading, removing a separate ten-minute GFS
acquisition wait. All 84,579,313 rows passed the existing publication gates.

The optimized updater is installed in the writable checkout at
`~/.local/share/meteogram-benchmark/repo`. The running supervisor starts that code
for subsequent refreshes; it does not require a database/API restart. Existing
serving revisions retain their original layout until the next forecast replaces
them. Current 18Z and previous 12Z forecasts and the persisted schedule are intact.

## What changed

- The updater stages the next model on one background worker while the main
  updater loads and validates the current model. At most one extractor and one
  loader run together. Publication stays serial.
- Before overlapping, it reserves both model admission budgets plus the 20 GiB
  reserve. If that check fails, it processes sequentially. `update --no-prefetch`
  also forces sequential work. Current disk admission thresholds are unchanged.
- New tables partition by `(revision_id, intDiv(cell_id, 32768))`, limiting merges
  to geographic groups instead of allowing merges across an entire model cycle.
  There were 59 HRRR partitions and five each for GFS/AIFS. ClickHouse merges
  only within partitions; its partition min/max indexes can prune groups for
  the existing cell-ID query. See the [official partitioning guidance](https://github.com/ClickHouse/clickhouse-docs/blob/main/docs/best-practices/partitioning_keys.mdx).
- The loader fills contiguous Arrow column buffers directly, avoiding an
  intermediate cell/lead/level/field tensor and subsequent column copies.
  Batch sizes remain 4,096 cells for HRRR/AIFS and 1,024 for GFS.
- Each successful load records gather/Arrow, insertion and validation timing in
  `load-timings.json`. Cancellation stops the fetch child and joins the worker
  before releasing the updater lock. Partial source values trigger older-run
  fallback before loading; database validation failures retain the serving data.

## Full-cycle timing

The comparison uses the same **2026-10-17 18Z** forecast initializations and pinned
source snapshots as the [previous deployment](2026-10-17-native-pipeline-deployment.md).
The optimized run reacquired all source maps, decoded and staged every retained
value, loaded all three full-CONUS tables, validated them, and published them
into a separate benchmark catalog. It ran from 02:24:45 to 02:44:56 UTC on
October 18 (October 17 local time).

| Model | Earlier acquisition | Optimized acquisition | Earlier load/validate/publish | Optimized load/validate/publish |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 8:07 | 8:28 | 15:04 | 11:03 |
| GFS | 9:43 | 10:04 | 0:19 | 0:28 |
| AIFS | 0:33 | 0:36 | 0:02 | 0:02 |

The optimized columns overlap: all **604 seconds** of GFS acquisition occurred
inside HRRR's loading interval; approximately **28 seconds** of AIFS acquisition
occurred while GFS loaded. Acquisition itself was slightly slower, and GFS's
short loading phase was slower. The overall improvement comes from overlap and
faster HRRR loading, rather than faster downloads across the board.

The previous 35:04 job included approximately 50 seconds rejecting an incomplete
00Z HRRR candidate. Its accepted-cycle interval was **34:14**. The new **20:11**
run was about **41% shorter** than that accepted-cycle interval, or **42.5% shorter**
than the entire previous job. The benchmark pins selection rather than timing
latest-cycle metadata probes and fallback. These are two measured runs, not a
controlled guarantee for every future source cycle or network condition.

HRRR's optimized loader spent **279.6 s** gathering/building Arrow columns,
**341.9 s** inserting, and **40.6 s** validating. Residual work is dominated by
staging reads and inserts. Further gains should be measured against those phases.
Updater process I/O counters can include reaped prefetched extractor children;
do not interpret `process_write_bytes` as loader-only writes or sum it again with
source child writes.

A preliminary same-host comparison used 65,536 real HRRR cells and all 49 leads
(3,211,264 rows). Original loading/validation took 17.53 and 16.20 seconds; the
optimized version took 15.85 and 15.87 seconds. This small cached sample was a
screening test, not a substitute for the full-cycle result.

## Correctness and query checks

The completed run retained all 49 HRRR, 141 GFS and 31 AIFS forecast times,
with unchanged fields, pressure levels, Float32 precision and geographic coverage.
It read **24,963,345,620 bytes** of encoded source payload with **zero map retries**.
Snapshot IDs were HRRR `G2DVQ244H9B0DTPYSY80`, GFS `V5SK6H0M0RF697RTBZZ0`, and
AIFS `KGX0VQA3TPWZRWK9Q7G0`.

All existing gates passed: full row/key/per-lead counts, array lengths and finite
values, plus exact staged comparisons at ten cells per model. After ingestion,
**100 random complete profiles per model** matched the original serving tables
exactly. A temporary API on loopback port 18081 then returned **42 complete HTTP
profiles** identical to port 18080, excluding revision identity and query timing.
Those checks included cities, random native cells and edge cells. The temporary
API was stopped afterward.

During the full run, the original serving API handled **1,704 requests with zero
errors**. One fixed coordinate per model was sampled repeatedly. Its p95 latency
was 82.65 ms HRRR, 82.23 ms GFS and 27.52 ms AIFS; the maximum was 477.65 ms.
The serving catalog was unchanged during this isolated build. This is an
availability-under-load measurement, not a live publication-lock test or proof
that the earlier 9.94-second live-update outlier has been eliminated.

A separate query comparison used the same 300 random cells for each layout,
300 explicit warm-up queries, then 300 timed single-client queries and 300
four-client queries per model/layout:

| Model | Original warm p95 | Optimized warm p95 | Original four-client p95 | Optimized four-client p95 |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 9.00 ms | 8.87 ms | 14.44 ms | 12.04 ms |
| GFS | 13.02 ms | 13.11 ms | 15.17 ms | 17.28 ms |
| AIFS | 10.02 ms | 7.11 ms | 10.24 ms | 11.97 ms |

There is no material single-client regression in this sample. Four-client GFS
and AIFS p95 were approximately two milliseconds higher; the change is not a
universal query-speed improvement. Initial random candidate reads before warm-up
had HRRR p95 33.12 ms; do not label that first pass as a warm benchmark.

## Resources, tests and cleanup

Active candidate tables occupied **35.301 GB**, effectively unchanged from the
previous layout. ClickHouse's `OSWriteBytes` event counter increased by **61.332 GB**
over the build; this includes server activity and is not physical NAND wear.
The earlier live update did not record an equivalent complete counter interval,
so no before/after write-amplification percentage is claimed.

Sampling covered the entire loading period, beginning about 146 seconds after
acquisition started. Observed inactive native parts peaked at **13.633 GB**, versus
24.901 GB in the previous partial live-update telemetry. Minimum free disk was
**236.749 GB**, and minimum observed available host memory was **26.367 GB**.
These are observed values, not new storage guarantees; conservative admission
budgets remain in place.

**18 tests passed**: nine pipeline/integration tests, six ingestion scheduling and
failure tests, and three coordinate-boundary tests. Python compilation, Ruff
lint/format checks, full-cycle gates, profile comparisons and final service
verification also passed. The source-finiteness prefetch gate has a specific
older-run fallback regression test.

After verification, the three isolated candidate tables and 98 sample arrays
(2.338 GB) were removed. Small reports and metadata remain; candidate catalog
references in the evidence describe historical, now-removed tables. The original
serving current/previous tables were preserved, and free disk returned to about
**370 GB**. The app still uses Open-Meteo, and the native API remains loopback-only.
The earlier systemd activation/reboot limitation is unchanged.

## Evidence and reproduction

[Evidence bundle](../benchmarks/2026-10-17-native-ingestion-optimization/README.md)
contains baseline and optimized event timestamps, phase timings, validation
markers, HTTP/query comparisons, test outputs and cleanup verification.

Use `benchmark-ingestion.py --source-root ~/.local/share/meteogram-native
--output /path/to/new/experiment` with the installed benchmark Python to repeat
this pinned-cycle experiment. It requires a new directory, substantial disk
headroom, and the serving updater lock. It leaves its own tables for inspection;
explicitly remove only those benchmark-owned tables after saving the evidence.
See the [prototype README](https://github.com/andyfangdz/meteogram-js/blob/5f4ae08/scripts/clickhouse-prototype/README.md) for
normal update and service commands.
