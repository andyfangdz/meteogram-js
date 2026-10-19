# Full HRRR loading under one minute

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

The complete staged HRRR load, including all validation and resuming merges,
finished in **50.21 seconds**, with a **53.70-second** fresh-table repeat on
this server. Both used the same frozen executable and default settings. Every
configured native cell, forecast time, pressure level and field was retained,
with exact Float32 values. The complete resident table occupies **6.57 GB**.

The clock starts with the load CLI and stops when it exits successfully.
Downloading, GRIB decoding and preparing staging are excluded. This measures
loading an entire forecast into a fresh table, not a complete network refresh.
The Next.js application still uses Open-Meteo; this remains the private native
forecast prototype, without an application meteorological adapter.

## Complete dataset and measurement

| Property | Value |
| --- | --- |
| Initialization | HRRR, 2026-10-18 12Z |
| Pinned Icechunk snapshot | `WX5GN6QXSDKTBPQQP96G` |
| Region | Native cells within 24–50°N, 125–66°W |
| Native cells | 1,638,757 |
| Forecast times | All 49 hourly times, 0–48 hours |
| Pressure levels | All 30 configured levels, 1000–275 hPa |
| Fields | Geopotential height, temperature, wind U/V, relative humidity, dew point; surface temperature/dew point |
| Logical cell/time rows | 80,299,093 |
| Actual Float32 weather values | 14,614,434,926 |
| Physical tile rows | 6,402, retaining up to 256 distinct cells each |
| Active-part bytes | 6,568,173,689 in 101 parts |

The host is the existing eight-core ARM Linux machine with approximately 32 GB
RAM and a 512 GB NVMe drive. The build uses Rust 1.99.1 and ClickHouse 26.9.20.23.
Both runs create new, unique tables; no completed spatial batches are dropped.
The published forecast and historical benchmark tables remain separate.

Before each run, Linux discard hints target only the 98 immutable staged
weather files. No global filesystem-cache flush is used. The loader's 7 GB
sequential prefetch is **inside** the clock and takes 4.34/4.37 seconds.
External process I/O reports 13,670,584 input blocks in each run, approximately
7 GB. Kernel caches otherwise remain enabled. Lightweight availability and
memory observers run alongside the measurements; compilation, tests and other
benchmarks do not.

| Complete CLI load | External wall seconds | Result |
| --- | ---: | --- |
| Previously installed loader, same 12Z initialization | 120.24 | Complete raw-bit baseline |
| Final frozen binary, fresh table 1 | **50.21** | All checks passed |
| Same binary/defaults, fresh table 2 | **53.70** | All checks passed |

These are local observations, not a guarantee for every initialization or a
busier host. Cumulative reader/worker times overlap and cannot be added to
obtain elapsed time. Acceptance uses external process elapsed time.

## Lossless storage and bounded loading

Each field/level/lead has a palette of exact Float32 bit patterns, with sorted
UInt16 indices in the spatial grid. There is no quantization, averaging,
downsampling or omission. This forecast has 20,164,579 palette entries; its
largest palette has 24,455 values. The versioned codebook occupies 50,641,048
bytes, is checksummed and SHA-256-bound to metadata, and remains required
serving data. Signed zero and other finite Float32 bit patterns are preserved.
Larger palettes retain the UInt32 fallback.

HRRR uses `cell_tiles_v1`. Each physical row retains 256 adjacent source cells
and their ordered original IDs. Every field/level/lead slot has 32 UInt128
words, each packing eight exact UInt16 indices. Arrays retain level/lead order.
`Delta(2)/Zstd(1)` compresses the underlying lanes. The final tile has 101 real
cells and 155 unused lanes padded with valid code zero; padding is also checked
and never exposed as native cells. Readers retain support for the earlier
8-/64-cell tiles, dictionary profiles, raw Float32-bit profiles and legacy
cell/time tables.

Two readers decode checksummed UInt16 staging frames directly into tile words,
using bounded reusable buffers and a two-block queue. Six workers process
16,384-cell batches. A shared, RAII-scoped gate permits at most **four active
inserts**; other workers can validate completed ranges concurrently. Permits
are released on errors or panics, and failed loads cancel and join every worker.
Unused database allocator pages are released initially, after every four
validated batches and before final validation. All release time is included.
The existing 6 GiB server and 2 GiB query limits are unchanged; client peak RSS
was approximately 3.43 GiB in each final run.

For UInt16 inputs up to 8 GiB, prefetch requires available memory of at least
input size plus 12 GiB, preserving the existing database/client/margin budgets.
Other inputs stream normally. Consumed source pages receive discard hints.
Compact SQL constants supply one exact palette bound per logical slot, with
constant expansion across word groups. Every stored word still receives its
own eight-lane check.

## Validation and evidence

Inside each timed load, every source frame is decoded with checksum, shape and
size checks. The entire codebook is hash-checked and every palette value must
be finite. After every insert, ClickHouse checks that exact range's row count,
minimum/maximum tile ID, revision, cell count, all array lengths and **all
stored indices against their own slot's exact palette bound**. Finite palettes
plus complete valid index bounds prove every decoded weather value is finite.

Each final run has 101 successful, disjoint range checks covering all 6,402
physical rows, reading 29,232,146,592 decoded database bytes. This includes
14,615,817,216 stored indices, with padding. Global ordered cell-ID coverage,
physical types and counts are checked, together with ten complete bit-exact
profiles retained from the immutable ingestion buffers. Candidate merges must
resume successfully before the CLI returns. Only the unpublished candidate
table has its merges paused.

The standalone audit separately scanned the entire final table and reread ten
complete source profiles from staging in 13.35 seconds. Its database scan read
all 6,402 rows and 29,232,146,592 bytes. Complete comparison checks also verify
all coordinates and 100 full profiles per model against the immutable source
catalog. HTTP verification compares 27 complete forecasts with the running
service, and checks malformed/outside requests and gzip. The 39 Rust tests,
including real-database integration tests, pass, as do formatting and Clippy
with warnings denied. Corruption tests cover all UInt16 values, palette-bound
edges, every lane/group, ordered IDs, checksums, signed zero, subnormals,
legacy readers and failed-load cleanup.

Detailed measurements, binary hashes, range coverage, audit, compatibility,
memory observations and deployment records are in the
[evidence directory](../benchmarks/2026-10-18-hrrr-one-minute/README.md).

A separate warmed, single-client query benchmark measured 300 complete profiles
per model after 300 warmups per model:

| Model | Median milliseconds | p95 milliseconds |
| --- | ---: | ---: |
| HRRR, 256-cell tiles | 22.78 | 27.79 |
| GFS, existing raw-bit profiles | 9.33 | 10.98 |
| AIFS, existing raw-bit profiles | 5.33 | 6.48 |

The earlier 64-cell experiment measured HRRR at 14.42/17.37 ms median/p95.
Larger groups trade additional point-query decoding for better compression and
loading throughput. These measurements include local complete-profile queries
with warmed filesystem caches, not WAN latency or an HTTP service SLA.

## Installation and cleanup

The exact measured executable, SHA-256
`f819bbf34d1f5c49bb20afc0ad41c70e922d6b82f7cc1dfcd6e2de5debc37bc8`,
is installed at `~/.local/bin/meteogram-native`. The local service was restarted
to load its new readers, reaching healthy status 3.30 seconds after the stop
request. All 27 pre-/post-install forecast hashes match. The catalog and
persisted refresh schedule are byte-identical; all 12 protected serving and
historical benchmark tables retain their original physical row counts.
The experimental forecast was not published over the current forecast.

The availability observer completed before the planned restart, recording
2,898 successful requests per model and zero errors. Final load observations
record no increase in memory-error counts, at most four active inserts and a
sampled server RSS peak of approximately 2.74 GiB. RSS sampling is not an exact
instantaneous peak measurement.

After hashing and verifying all 199 final source payload/index/metadata assets,
cleanup removed 298 temporary weather files totaling 50,727,764,653 bytes and
the remaining two owned experiment tables. Metadata, coordinates, codebooks,
source pins and reports remain. Private catalogs pointing to dropped tables
were cleared. Serving data and the existing application provider were preserved.

## Preparation, rejected attempts and limits

The controlled input was prepared from an immutable clone of the already
acquired full 12Z forecast. Exporting raw/dictionary staging took 611.01 seconds;
sorting palettes and remapping all indices took a separate 61.81 seconds.
These preparation costs are excluded from loading. Final weather staging is
6,999,134,035 bytes, versus the earlier compressed Float32 staging's roughly
26.18 GB. The database baseline occupied 23,584,400,013 bytes; the final table
uses approximately 72% fewer active-part bytes, plus its required codebook.

Fresh HRRR acquisition now directly produces sorted palettes and version-2
UInt16 staging. A live whole-CONUS lead-zero smoke test decoded all 182 maps,
matched the controlled staged arrays and all 485,081 palette entries exactly,
and resumed without changing durable data. A fresh 49-lead acquisition was
**not** benchmarked in this optimization. GFS/AIFS retain raw Float32-bit
loading; no new full-model timing claim is made for them.

Earlier 64-cell runs reached 56.04/58.92 seconds, but the second lost three
query-completion log records when background logging reached the RSS cap.
All 101 loader checks had succeeded, and a separate whole-table audit passed;
the incomplete telemetry is retained rather than described as complete proof.
An allocator-only attempt then correctly aborted at the same cap. Smaller
12,288-cell batches completed with intact validation records and no new memory
errors but took 62.41 seconds. A bounded 16,384-cell sample measured 256-cell
groups about 7% smaller than 64-cell groups; 1,024 cells brought no further
storage benefit. The final insert gate, compact bounds constants and 256-cell
layout achieved the timings above with complete logs and no new memory errors.

The previous [two-minute load report](2026-10-18-hrrr-load-optimization.md)
documents the earlier 00Z forecast and remains historical evidence. Neither
report claims a complete network refresh, public application migration,
electricity cost, SSD endurance or a service SLA.
