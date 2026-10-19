# Bounded full GFS dictionaries — 2026-10-18

The complete GFS forecast now works with lossless UInt16 dictionaries. Dictionary
encoding completed in **24.81 seconds at 310.11 MiB peak RSS**, replacing the
all-slots lookup approach that failed at 3.84 GiB. Both dictionary database
layouts reproduced **all 638,598,870 original Float32 values exactly** in a
complete reconstruction, including byte-identical staging indexes and coordinates.

Dictionary **profiles** offer the better measured balance: **0.864 GB including
the codebook**, versus 1.539 GB raw, a **43.9% reduction**. Warm complete-profile
query p95 was **10.19 ms**, versus 8.85 ms raw. Packed tiles save another 93 MB
but raise p95 to 27.03 ms. These are isolated local measurements; no installed
binary, service defaults or application provider changed.

## What changed

The [earlier benchmark](2026-10-18-other-model-load-benchmarks.md) identified
38,867,743 geopotential palette entries across 4,230 level/lead slots. Each
individual palette fits UInt16; the largest has 13,956 entries. The problem
was retaining hash tables for every slot simultaneously, exceeding the existing
32-million-entry per-field builder budget.

`encode-stage` reads a raw staged lead and builds one spatial map's palette at
a time. It sorts exact Float32 bits in total order, writes UInt16 indexes using
the existing checksummed spatial-delta staging format, and releases that map's
lookup table before proceeding. Finished compact palettes remain for the final
codebook. No cell, level, lead or field is removed, and values are not quantized.

The legacy `FieldBuilder` keeps its 32-million-entry per-field hash-table guard.
Compact codebooks/readers instead enforce **128 million entries total** across
all fields: 512 MB of Float32 payload, plus a separate **65,536-slot** limit on
palette metadata. Both limits and arithmetic overflow are checked before reader
allocation. This aggregate payload bound is smaller than the old eight-field
worst case of 256 million entries. The new encoder also bounds each input/output
lead matrix to 1 GiB and native cells to two million. Oversized or non-UInt16
inputs fail explicitly instead of thinning data. Output is incomplete until all
arrays and the checksummed, hash-bound codebook are durable.

The complete measured GFS codebook contains **61,092,751 entries**, occupying
244,371,004 uncompressed value bytes and **169,063,408 compressed serving bytes**.
Every per-field slot/cardinality count matches the previous independent exact-bit
diagnostic of the full source table. Existing full HRRR codebook reading also
passed with the new bounds.

## Full scope and preparation

This uses the existing complete **2026-10-18 06Z** GFS forecast, Icechunk snapshot
`C1ZJYPP8XXY76JW0RQSG`, frozen in an independent local source-table clone.
The clone is a preparation input; every timed load performs fresh inserts.
All **24,885** retained native cells in 24–50°N, 125–66°W are present, with
**141 leads**, **30 pressure levels**, six pressure fields and two surface fields.
The forecast retains hourly leads 0–120 and three-hour leads 123–180.

Pressure fields are cloud cover, geopotential height, temperature, wind U/V and
relative humidity. Surface fields are temperature and dew point. Exact source
identity, axes and field lists are in
[summary.json](../benchmarks/2026-10-18-gfs-dictionary/summary.json).

Preparation was measured separately:

| Step | Process seconds | Peak process RSS | Result |
| --- | ---: | ---: | --- |
| Complete raw export from immutable local source | 16.02 | 1.78 GiB | 1,656,893,197 weather-stage bytes |
| Mapwise dictionary encoding | 24.81 | 310.11 MiB | 560,047,776 coded-stage bytes + 169,063,408 book bytes |

The 310 MiB figure describes dictionary encoding, not the preceding raw exporter.
These two observations are not averages. Preparation and network acquisition are
excluded from all load timings below. The dictionary was prepared once and
reused unchanged for both measurement phases; query-reader changes did not alter
the encoder, codebook or stored format.

The explicit workflow is:

```sh
meteogram-native prepare-store --raw-only --source "$PINNED_DESCRIPTOR" \
  --table "$IMMUTABLE_SOURCE_TABLE" --output "$RAW_STAGE"
meteogram-native encode-stage --source "$RAW_STAGE" --output "$DICTIONARY_STAGE"
meteogram-native load --root "$BENCH_ROOT" --data "$DICTIONARY_STAGE" \
  --table "$FRESH_CANDIDATE_TABLE" --workers 4
# Add --packed to measure 256-cell tiles instead of per-cell profiles.
```

Use the newly built binary, fresh output directories and unique candidate tables.
The tested candidate remains outside Git at
`~/.local/share/meteogram-gfs-dictionary/candidate`. The running service still
uses its previous installed binary. The codebook format remains `MGDICT01`, but
readers must be upgraded before publishing GFS books that exceed the old field
limit. Automatic GFS acquisition/scheduled loading still use raw storage.

## Fresh-table load and storage results

All final runs used the same frozen release binary, four workers and complete
input, on the eight-core ARM64 host with approximately 30.2 GiB RAM and NVMe.
ClickHouse remained 26.9.20.23 with its existing 6 GiB server and 2 GiB query
limits. Three runs per format rotated order; all observations are retained.
Only owned weather input files **and their codebook** received Linux discard
hints before each clock. This is not a claim of cold hardware caches.

| Layout | Three complete CLI loads, seconds | Median seconds | Database bytes | Required codebook bytes | Total bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| Raw Float32 bits | 5.86, 8.99, 8.09 | 8.09 | 1,539,165,379 | 0 | 1,539,165,379 |
| UInt16 dictionary profiles | 6.47, 6.43, 8.55 | 6.47 | 694,823,240 | 169,063,408 | 863,886,648 |
| UInt16 256-cell tiles | 6.11, 7.82, 9.01 | 7.82 | 601,875,487 | 169,063,408 | 770,938,895 |

Storage is active parts before further background merging, plus one required
codebook per revision; common coordinates, metadata and program bytes are
excluded. GB means 1,000,000,000 bytes. Profile reduction is **43.9%**, tile
reduction **49.9%**. Byte totals were identical across repeats. Timing ranges
overlap substantially; three observations do not establish a reliable loading
speed advantage. All formats completed in 5.86–9.01 seconds with full load gates.

Raw/profile tables have **24,885 physical rows**. Tiled tables have **98 physical
rows**, retaining the same 24,885 cells and **3,508,785 logical cell/lead rows**.
The final tile contains 53 real cells and 203 checked padding lanes. All final
load clocks include book identity/decompression/finite checks, frame checks,
complete disjoint range checks, every ordered cell ID, physical types, ten
complete bit-exact buffered profiles, allocator purges and merge resumption.
Consumed input pages receive discard hints; admitted tile input prefetch remains
inside the clock. No serving table was paused or rebuilt.

Peak loader RSS across repeats was 2.60 GiB raw, 1.77 GiB dictionary profiles
and 1.84 GiB packed. These are client processes, excluding the database.

## Query changes and final latency

Initial dictionary queries exposed an avoidable cost: packed queries transferred
the entire 256-cell tile to recover one cell. The reader now projects only that
cell's eight-lane UInt128 word per slot in ClickHouse. It also returns each
**original array length** and validates the full stored width before extracting
the selected lane. A real corruption test removes a different word group while
leaving the requested profile intact; the reader still rejects it.

Both profile and tile readers decode palette indexes in field/slot order before
constructing time rows. Axis metadata is resolved once. Every selected index
still receives its exact palette-bound check, and full ingestion validation
still checks **every lane of every stored word**, including padding.

ClickHouse's mean `result_bytes` counter fell from **13,142,340 to 416,036** for
packed point queries. Its mean `read_bytes` remained **13,140,040**: projecting
the response does not make the database read only one cell's stored tile data.
The counter sample contains 738 complete point queries in each phase.

After all scans and correctness audits, each final configuration ran **300
warmup + 300 measured complete-profile queries**, one client, covering a fixed
deterministic sequence across the retained grid. These are local database-client
observations including decoding/JSON-value construction, with the codebook
resident; HTTP encoding, WAN latency and network source acquisition are excluded.

| Layout | Final query p50, ms | Final query p95, ms | Earlier reader p95, ms |
| --- | ---: | ---: | ---: |
| Raw bits | 6.66 | 8.85 | 11.09 |
| Dictionary profiles | 8.45 | 10.19 | 21.07 |
| 256-cell tiles | 25.60 | 27.03 | 65.25 |

For this forecast, dictionary profiles save approximately 675 MB per revision
with a measured 1.35 ms p95 increase over raw. Packing saves only another
92,947,753 bytes while adding about 16.83 ms to profile p95. Dictionary profiles
are the measured compromise when reducing GFS storage; raw remains the shortest
query path. These samples do not establish cold-cache, concurrency or SLA results.

## Full correctness and service preservation

For each final layout, standalone whole-table `validate --staged` passed.
All cell IDs/coordinates and 100 complete forecasts **per model** matched the
immutable reference catalog; coordinate error was zero. Each layout passed 27
complete HTTP comparisons with the running service, four invalid requests,
three outside-region requests and gzip checks.

For **both final dictionary layouts**, an additional export recovered every
Float32 value from the actual loaded database. All **565 files per layout**
matched the original raw input by SHA-256: 282 weather payloads, 282 frame indexes
and the coordinate file. This proves the complete 638,598,870-value roundtrip,
not just sampled equality. These reconstruction audits were outside load clocks.

All **42 Rust tests**, including database tests, passed. Coverage includes signed
zero/subnormal/extreme Float32 values, axis ordering, short final frames,
oversized book rejection before allocation, slot/matrix budgets, nonfinite and
UInt16 overflow rejection, legacy formats and projected-reader corruption.
Format, Clippy with warnings denied, source hashes and artifact checks passed.

The main catalog stayed generation 21, its refresh schedule and installed binary
were unchanged, and all twelve original native/benchmark tables retained their
physical row counts. The observer recorded **611 successful fixed-point requests
per model with zero errors**; this limited loopback sample is not an availability
guarantee. Final complete API checks passed. No restart, installation, source
provider switch or public deployment occurred.

The [evidence bundle](../benchmarks/2026-10-18-gfs-dictionary/README.md) includes
the final nine loads, earlier query observations, full reconstruction hashes,
source pins, build/test logs and preservation/cleanup proofs. All 19 owned
experiment tables were removed after verification. Large weather staging was
removed; descriptors, coordinates, required immutable codebooks and binaries
remain outside Git. Original source and serving tables were preserved.
