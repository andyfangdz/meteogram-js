# Full HRRR loading and native compressed staging

The complete HRRR load from staged data, including stored-value validation,
finished in **116.36 seconds**, with a **118.03-second** fresh-table repeat,
on this server. New weather staging uses indexed, checksummed Zstd frames instead of
NPY, reducing the measured HRRR payload from **58.46 GB to 26.18 GB**. Coordinate
indexes also have a compressed native format. All Float32 and Float64 bits are
preserved; neither format requires Python.

The timed boundary begins when the load CLI starts and ends when it exits
successfully. It includes input decoding and checksums, packing, Native encoding,
all inserts, validation of every stored value, global key/type/count checks,
complete sampled source/database comparisons and resuming candidate merges.
It excludes downloading, GRIB decoding and preparing the staged input. This is
not a claim that an entire source refresh takes two minutes.

## Dataset and method

Both final runs use the same complete pinned initialization:

| Property | Value |
| --- | --- |
| Model / initialization | HRRR, 2026-10-18 00Z |
| Icechunk snapshot | `KB3VN67D9882NQ6F549G` |
| Region | Retained native cells in 24–50°N, 125–66°W |
| Native cells | 1,638,757 |
| Forecast times | All 49 hourly times, 0–48 hours |
| Pressure levels | All 30 configured levels, 1000–275 hPa |
| Fields | Six pressure fields and two surface fields |
| Logical cell/time rows | 80,299,093 |
| Physical complete-profile rows | 1,638,757 |
| Raw weather value bytes | 58,457,739,704 |

The host is the existing eight-core ARM Linux machine with approximately 32 GB
RAM and a 512 GB NVMe drive. Rust 1.99.1 and ClickHouse 26.9.20.23 were used.
Each timed run creates a new, unique table and retains the complete dataset.
No spatial batches are dropped during loading. Before each run, a small Rust
diagnostic requests Linux `POSIX_FADV_DONTNEED` only for the 98 immutable input
files. No global cache flush is used. External process I/O counters are retained.
No build, test suite or other benchmark runs during the final measurements.

The physical database format remains `cell_profile_v1`, level/lead arrays of
exact Float32 bits in `Array(UInt32) CODEC(T64,ZSTD(3))`, with index granularity
4. HRRR's Zstd level increases from 1 to 3; precision is unchanged. GFS/AIFS
retain level 1. Buffered JSON index reads avoid per-byte file reads.

## What changed

Three readers reuse bounded source, packing and Zstd decoder buffers. They feed
a bounded queue to four insert workers. Input reads are positional; consumed
input pages receive a Linux discard hint. Every insert response is drained,
including late errors in successful HTTP responses. Failures cancel and join
every worker before a candidate can be discarded.

Each insert worker now checks its complete, disjoint cell-ID range directly
from ClickHouse immediately after insertion. The query requires the expected
row count, correct revision value, correct length of every array and finite
values throughout every field. Completion is counted only after that query
passes. The successful in-memory completion barrier covers every cell exactly
once. Final global row counts, exact sorted IDs, physical types and all ten
complete sampled source/database profiles are still checked. Samples retain
356,720 bytes from the immutable source buffers actually read during ingestion.

This moves the same complete stored-value checks next to their writes, while
parts are still cached. A persistent flag cannot bypass validation. Standalone
`validate` always performs an independent whole-table scan; `--staged` also
reads source samples from disk. Only the new unpublished table has merges
paused. A successful `SYSTEM START MERGES` is required before loading returns.
The benchmark then pauses its own table again, outside the timed boundary, to
avoid background compaction competing with subsequent verification or repeats.

New staging consists of 98 `*.stage.zst` data files and their versioned JSON
indexes. Each independently checksummed Zstd frame contains at most 4,096 cells
in reversible Float32 byte-shuffled order. Readers validate shape, dtype,
encoding, frame offsets, lengths and decoded sizes, and decode bounded blocks.
Truncated, corrupted or unchecksummed frames fail the load. The index is an
application format; the payloads are standard Zstd frames.

`cells.bin.zst` contains a checksummed Zstd frame with the `MGCELL01` version tag,
a little-endian UInt64 count, contiguous UInt32 IDs and Float64 coordinate
columns. Old NPY/NPZ snapshots remain readable. New acquisitions write neither
format. The source-cache checkpoint and eviction policy is unchanged: durable
staging and checkpoint first, then discard the corresponding encoded maps.

## Measured results

| Complete CLI load | Wall seconds | Notes |
| --- | ---: | --- |
| Previously installed serial loader | 351.70 | Prior full-run evidence linked below |
| Four workers, raw staging, final whole-table scan | 147.77 | Complete full dataset |
| Compressed staging, final whole-table scan | 169.34 | Storage win; final validation overlapped compilation, so not a clean timing comparison |
| T64/LZ4 database experiment | 169.37 | Rejected: larger output increased disk I/O |
| Streamed validation, Zstd 1 | 112.95 / 120.01 | Repeat narrowly missed the strict limit |
| Final, Zstd 3, new table, run 1 | **116.36** | All validation included |
| Final, Zstd 3, new table, run 2 | **118.03** | All validation included |

Run 1's 401 validation queries read exactly 1,638,757 rows and 58,575,730,208
database bytes, with only 4,096 physical OS read bytes (12,288 in run 2). The final partial range
contains 357 cells. This directly confirms complete stored-data inspection and
the benefit of reading new parts while they remain cached. By comparison, the
compressed-input version's final whole-table scan performed about 10 GB of
physical reads. Cumulative worker times overlap and must not be summed as wall
time. The complete load's external elapsed time is the acceptance measurement.

The first final table contained all 1,638,757 complete-profile rows and
23,662,766,154 active-part bytes. The new staging payload is 26,183,140,784 bytes,
55.2% smaller than its 58,457,739,704 raw value bytes. The one-time legacy
conversion took 275.27 seconds and is reported separately; it is not included
in or presented as a faster source acquisition. Coordinates occupy 28,381,278
bytes instead of the legacy archive's 32,775,846 bytes. These are local storage
measurements, not network-traffic, electricity or SSD endurance claims.

An 8,192-cell/four-writer experiment exceeded the existing 6 GiB ClickHouse
memory limit and was rejected without publication. The limit was not increased.
The final loader retains 4,096-cell HRRR batches and measured about 2.24 GiB
client peak RSS. A 16,384-cell codec probe found Zstd level 3 used 2.7% fewer
bytes than level 1; level 6 added substantially more CPU for little further
compression. These probes selected candidates; the final timings above are
complete full-dataset measurements.

## Verification and deployment

All 28 Rust tests pass, including four real-database integration tests, format
corruption/truncation checks, exact special Float32 bit patterns, partial final
batches, failed-load cancellation and stale-marker cleanup. Formatting,
Clippy with warnings denied and the locked release build pass.

Both final tables passed 100 complete HRRR profile comparisons against the
existing serving revision, with every cell ID/coordinate checked and zero
coordinate error. Both temporary APIs passed 27 complete HTTP comparisons,
four invalid-input cases, three outside-region cases and gzip checks. After
run 2, an independent `validate --staged` passed in 24.89 seconds: a fresh
whole-table scan inspected all 1,638,757 rows and 58,569,175,180 bytes, followed
by comparisons to staged samples read again from disk. This was an additional
audit; both timed loads had already checked every stored value in their 401
disjoint ranges.

The installed executable is byte-identical to the measured binary, SHA-256
`d2916fe9799a2eed752aa2b2719d8c584a5110431d3a3f9ebe4d0a5b0bc05f23`.
The managed Rust supervisor/API was restarted so future native coordinate
files are readable. The stop-request-to-healthy interval was 17.8 seconds.
All three models are healthy; 27 pre/post-restart forecast hashes are identical.
The serving catalog and persisted refresh schedule are byte-identical, and all
12 protected serving/historical benchmark tables retain their original rows.
Only the goal's unpublished experiment tables were dropped.

Before the planned restart, the fixed-point observer completed 2,087 successful
requests per model (6,261 total), with zero errors or changed forecast hashes.
This covers the ingestion experiments, including the rejected memory-limited
candidate. It samples one point per model over loopback, not publication or
WAN performance. The planned restart is recorded separately.

After verification, 302 owned weather staging files totaling 84.66 GB were
removed. Free space was 337.42 GB. Serving data, historical benchmark tables,
metadata, coordinates and small evidence artifacts were retained. Source
payload caches were already empty after successful per-lead checkpoints.

GFS and AIFS additionally have fresh, native-format acquisition and insertion
smokes for lead zero across their entire retained geography. These are
single-time tests, not new full GFS/AIFS performance benchmarks. The complete
GFS/AIFS forecasts used in cross-catalog/API checks remain the existing serving
revisions. The Next.js application still uses Open-Meteo; this remains a native
meteorological service on loopback.

The [evidence directory](../benchmarks/2026-10-18-hrrr-load-optimization/)
contains process timings, source identity, both range-query logs, complete
validation results, comparison/API results, final test/build checks, binary and
source hashes, and protected-table checks. Baseline evidence and prior context
are in the [earlier optimization report](2026-10-18-native-ingestion-optimization.md).
Large weather files and executables remain outside Git.
