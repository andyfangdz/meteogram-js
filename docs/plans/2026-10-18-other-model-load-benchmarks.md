# Full GFS and AIFS loader benchmarks — 2026-10-18

Four insert workers reduced median complete GFS loading from **16.07 to 6.33
seconds (2.54×)** and raw AIFS from **1.60 to 0.75 seconds (2.13×)**. AIFS's
256-cell layout saved **40.8%** of database-plus-codebook storage, but warm
complete-profile query p95 increased from **5.75 to 10.24 ms**. GFS dictionary
preparation exceeded the existing memory guard; no full GFS dictionary/tile
load was measured.

These results extend the [HRRR investigation](2026-10-18-hrrr-one-minute-load.md)
with actual complete datasets. They measure loading already prepared local
input, including validation. Network acquisition, preparation and publication
are outside the clock. The running service, installed binary, model defaults
and Next.js provider were unchanged.

## Complete scope and pinned sources

Both sources are the already acquired **2026-10-18 06Z** forecasts, independently
cloned from immutable serving tables before preparation. Every retained native
cell in 24–50°N, 125–66°W, every configured field and every configured lead/level
was loaded. There was no spatial sampling, temporal thinning or quantization.

| Model | Icechunk snapshot | Cells | Leads | Pressure levels | Pressure + surface fields | Float32 values |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| GFS | `C1ZJYPP8XXY76JW0RQSG` | 24,885 | 141 | 30 | 6 + 2 | 638,598,870 |
| AIFS | `W2VTRZNGHSMWYWKERRR0` | 24,885 | 31 | 8 | 5 + 2 | 32,400,270 |

GFS retains hourly leads 0–120 and three-hour leads 123–180; AIFS retains
six-hour leads 0–180. GFS pressure fields are cloud cover, geopotential height,
temperature, wind U/V and relative humidity. AIFS has geopotential height,
temperature, wind U/V and **specific humidity**. Both retain surface temperature
and dew point. Exact lead/level arrays and source identities are in
[summary.json](../benchmarks/2026-10-18-other-models/summary.json).

The host is the same eight-core ARM64 Windows Dev Kit 2023, with approximately
30.2 GiB RAM and an NVMe SSD, running ClickHouse 26.9.20.23. The existing 6 GiB
server and 2 GiB query limits were unchanged. All variants used one frozen
release binary. The one-worker baseline uses the same implementation as the
parallel variants, with only worker count/layout changed.

## Three complete loads per configuration

Each number below is a whole CLI process elapsed time from `/usr/bin/time`.
Every run created a **fresh table and inserted the entire staged input**.
Cloning was used only to freeze preparation sources, never to implement a
timed load. Repeat two reversed the configuration order; a third repeat retained
the observed startup/I/O variability. All 24 successful runs are retained in
the evidence; no outlier was discarded.

| Model / format | Workers | Three loads, seconds | Median, seconds | Database + codebook, MB |
| --- | ---: | --- | ---: | ---: |
| GFS raw bits | 1 | 15.02, 16.07, 16.07 | 16.07 | 1,539.17 |
| GFS raw bits | 4 | 10.00, 5.98, 6.33 | 6.33 | 1,539.17 |
| AIFS raw bits | 1 | 5.15, 1.57, 1.60 | 1.60 | 72.24 |
| AIFS raw bits | 4 | 0.75, 0.74, 0.75 | 0.75 | 72.24 |
| AIFS dictionary profiles | 1 | 1.42, 1.49, 1.49 | 1.49 | 51.87 |
| AIFS dictionary profiles | 4 | 0.85, 0.83, 0.82 | 0.83 | 51.87 |
| AIFS 256-cell tiles | 1 | 1.39, 1.28, 1.24 | 1.28 | 42.79 |
| AIFS 256-cell tiles | 4 | 0.75, 0.72, 1.80 | 0.75 | 42.79 |

MB means 1,000,000 bytes. Database bytes are active parts immediately after
loading, before further background merges. Shared coordinate/metadata files
and the program are excluded. Required codebook bytes are included:

| AIFS format | Database bytes | Required codebook bytes | Total bytes | Reduction from raw |
| --- | ---: | ---: | ---: | ---: |
| Raw bits | 72,243,037 | 0 | 72,243,037 | — |
| Dictionary profiles | 41,295,054 | 10,570,946 | 51,866,000 | 28.2% |
| 256-cell tiles | 32,219,956 | 10,570,946 | 42,790,902 | 40.8% |

All worker counts/repeats produced identical part-byte totals for their format.
GFS has 24,885 physical rows and 3,508,785 logical cell/lead rows. AIFS profiles
have 24,885 physical rows and 771,435 logical cell/lead rows. AIFS tiles have
**98 physical rows**, still representing all 24,885 cells; the final tile has
53 real cells and 203 padded lanes. Padding is validated and never returned as
native cells.

Before each clock, only that run's weather input files received Linux
`POSIX_FADV_DONTNEED` hints. No global cache drop was used, and this is not a
claim of cold hardware/disk caches. UInt16 tile input prefetch, when admitted
by the existing memory guard, occurred **inside** the clock. Raw GFS used
2,048-cell batches; all AIFS variants used 4,096. Four-worker raw loads used
three bounded readers; tile loads used two readers and at most four concurrent
inserts. Initial/periodic/final allocator purges, insert completion, all checks
and successful merge resumption remained timed. Only each experiment's table
was paused again after timing to keep subsequent measurements quiet.

Maximum observed loader RSS across repeats was 1.02 GiB for one-worker GFS and
2.21 GiB for four-worker GFS. Four-worker raw AIFS peaked at 267.96 MiB; packed
AIFS at 137.40 MiB. These client figures exclude the separate database process.
Work counters overlap; adding read/insert/validation work would not reproduce
elapsed time.

## Query tradeoff

After all loads and correctness audits, each configuration ran 300 explicit
warmup queries followed by 300 measured queries, one client, using the third
repeat's table. Each query returns a complete native cell profile across all
leads, levels and fields. Coordinates span the retained grid using the same
deterministic sequence. Only the changed model was timed. These are warm
loopback database-client measurements, including profile decoding, with the
codebook already loaded; they exclude HTTP/JSON serving and network acquisition.

| Model / format | Workers used to build table | Query p50, ms | Query p95, ms |
| --- | ---: | ---: | ---: |
| GFS raw bits | 1 | 9.21 | 11.32 |
| GFS raw bits | 4 | 9.22 | 10.82 |
| AIFS raw bits | 1 | 4.89 | 5.97 |
| AIFS raw bits | 4 | 5.00 | 5.75 |
| AIFS dictionary profiles | 1 | 5.67 | 6.99 |
| AIFS dictionary profiles | 4 | 5.84 | 7.52 |
| AIFS 256-cell tiles | 1 | 8.17 | 10.30 |
| AIFS 256-cell tiles | 4 | 8.14 | 10.24 |

GFS's parallel loading improvement did not produce a measured query regression.
AIFS tiling retained the raw four-worker median load time while saving 29.45 MB,
but added about 4.50 ms to p95 queries. Its smaller forecast makes the storage
saving modest in absolute terms. Raw AIFS with four workers is the measured
choice for shortest profile latency; dictionary profiles/tiles are storage
tradeoffs, not an across-the-board speed improvement. These samples do not
establish cold-cache, concurrent-client, WAN or production-SLA behavior.

## Why GFS dictionaries were not completed

The existing preparer rejected GFS after 17.56 seconds, at 3.84 GiB maximum
client RSS, with `Dictionary field exceeds bounded palette budget`. It did
not complete the full export. The guard remains **32,000,000 entries per
field**, summed over all level/lead slots; it is distinct from UInt16's bound
for an individual slot.

A separate bounded diagnostic counted exact UInt32 Float32-bit cardinalities
over **every** configured field/slot in the complete source table. GFS
geopotential alone needs **38,867,743 entries** over 4,230 slots. Its largest
individual palette has 13,956 entries, so index width is not the obstacle.
All fields together need 61,092,751 entries. See
[gfs-cardinalities.json](../benchmarks/2026-10-18-other-models/gfs-cardinalities.json)
and the bounded diagnostic script. The partial export was excluded from loading;
`prepare-store --raw-only` exported the complete raw input instead. Raising the
guard was not used to force a result. There is no measured full GFS dictionary
storage size or load/query timing in this report.

AIFS's full dictionary contains 3,647,350 entries and fits the existing limits.
Preparing its raw and initial dictionary inputs took 4.38 seconds; sorting and
writing UInt16 staging took another 0.86 seconds. Full GFS raw export took
24.22 seconds. These are separate process measurements, excluded from every
loader number above. This local-table preparation does not measure or replace
native source acquisition.

## Correctness, isolation and reproducibility

Every timed load checked checksummed input frames, source shape/type and all
source values or dictionary indices. Each completed insert validated **every
stored value** in its exact disjoint range: revision, row/array lengths and
finite raw values or every dictionary lane's own palette bound. Final checks
covered global counts, every ordered native cell ID, physical types and ten
complete bit-exact profiles retained from immutable ingestion buffers. All
ranges completed and merges resumed before each process returned success.

For all eight final configurations, independent `validate --staged` whole-table
scans passed. `compare-stores` checked all cell IDs/coordinates and 100 complete
profiles **per model** against the immutable reference catalog, with zero
coordinate error. Each configuration also passed 27 complete HTTP profile
comparisons against the running service, four invalid requests, three outside
requests and gzip checks. The 39 Rust tests, including the database integration
tests and new raw-export/non-HRRR tile coverage, passed; format and Clippy with
warnings denied passed.

The [evidence directory](../benchmarks/2026-10-18-other-models/README.md) contains
per-run process/load/validation/range proofs, storage counts, final independent
audits, query distributions, HTTP comparisons, source pins and hashes, scripts
and build/test logs. Large binaries, codebooks, staged weather and database parts
are outside Git. Isolated tables and weather staging were removed after the
evidence and hashes were checked; source metadata, coordinates and codebooks
remain outside Git for inspection.

All original twelve native/benchmark tables retained their physical row counts.
The main catalog remained generation 21, the next scheduled update remained
unchanged, and the installed binary hash was preserved. Continuous fixed-point
HTTP observations and final full API checks are recorded in the evidence. This
includes 353 successful fixed-point observations per model with zero errors.
The benchmark did not restart or replace the running service. The new
`load --packed` and `prepare-store --raw-only` options are explicit investigation
tools; normal GFS/AIFS acquisition and scheduled loading still use raw bits and
one load worker.

The subsequent [bounded GFS dictionary implementation](2026-10-18-gfs-dictionary.md)
resolves the preparation limit with mapwise encoding and a separate compact-book
budget. It records complete successful GFS dictionary/profile/tile measurements;
the failed preparation above remains the result of this earlier experiment.
