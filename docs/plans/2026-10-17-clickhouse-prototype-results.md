# ClickHouse prototype: measured results and home-server estimates

**Later retry:** the [full-volume retry report](2026-10-17-clickhouse-full-volume-retry.md)
adds measured full-CONUS coverage for two HRRR times: 3.28 million rows, 1.207 GiB,
31.13-second load, and 6.85 ms warm-query p95. The network failed again before
the full horizon or GFS/AIFS acquisition completed. Its updated temporal
projections and SSD-write caveats supersede the corresponding initial estimates
below; the original measurements are preserved here.

Measured on 2026-10-17. **ClickHouse is viable for this query pattern.** The
prototype loads real Dynamical profiles and answers arbitrary coordinates inside
the ingested sample. Its database queries take milliseconds. The user's proposed
**four cores, 32 GB RAM, 300 GB SSD, 500/80 Mbps uncapped connection** look workable
with two retained runs and compressed staging, subject to a full-run capacity
test. Disk capacity and write volume need explicit bounds; Internet overages are
not a cost under the supplied uncapped assumption.

The important limit: the managed workspace's outbound tunnel began returning
HTTP 503 connection failures after four complete HRRR forecast times. Retries
also failed for unrelated sites. We therefore measured **four real times over
four spatial tiles**, not a complete 49-time or full-CONUS database. GFS/AIFS
source acquisitions did not complete. Full-run figures below are explicitly
projections or planning scenarios; none is a measured full production cycle.

## What was built and tested

- An isolated ClickHouse **26.9.20.23** server on loopback, using four available
  CPU cores under a cgroup quota and a 16 GiB memory limit. Host CPU: AMD EPYC
  9V74. Storage is a VM overlay filesystem; its physical backing is unknown.
- A resumable Python extractor with a pinned Icechunk snapshot, explicit
  full-map decode/gather loops, Float32 validation, bounded concurrency, GRIB
  byte counting and completed-time checkpoints. The initial vectorized Xarray
  implementation exhausted memory; the explicit map loop resolved that issue.
- Five MergeTree layout/codec comparisons, a larger-insert experiment, exact
  round-trip checks, warm/direct-I/O queries and concurrent readers.
- A local `/forecast?model=hrrr&lat=...&lon=...` API that maps coordinates to
  native cells. Four previously unqueried coordinates across the sample regions
  returned correct profiles. Invalid coordinates return 400 and unretained cells
  return 404. Requests never trigger source extraction.

The app still uses Open-Meteo. The prototype returns native source fields,
including explicitly grid-relative HRRR winds; production meteorological
transforms, publication, retention, public access and app integration remain work.

Reproduction instructions: [prototype README](../../scripts/clickhouse-prototype/README.md).
Machine-readable evidence: [source](../benchmarks/2026-10-17-clickhouse/source-measurements.json),
[layout comparison](../benchmarks/2026-10-17-clickhouse/layout-comparison.json),
[larger inserts](../benchmarks/2026-10-17-clickhouse/bulk-insert-comparison.json),
[API checks](../benchmarks/2026-10-17-clickhouse/api-checks.json), and
[estimate arithmetic](../benchmarks/2026-10-17-clickhouse/estimates.json).
The later [staging-compression measurements](../benchmarks/2026-10-17-clickhouse/staging-compression.json)
address the user's 300 GB disk constraint.

## Real sample

HRRR initialization: **2026-10-16 18Z**, Icechunk snapshot
`8QX1NG8ZDNGCXNFVESV0`. Retained four 96×96 tiles centered near California,
the Rockies, the Plains and the Northeast: **36,864 distinct cells**.

The completed sample contains lead hours 0, 1, 2 and 3, all 30 pressure levels
from 1000 through 275 hPa, six vertical fields, and 2 m temperature/dew point.
That is **147,456 cell/time rows and 26,836,992 Float32 values**, with no invented
or repeated forecast times used for compression estimates.

All required values in these tiles were finite. Eight deterministic random cells
per layout were checked at every retained time, level and field; values round-trip
exactly after the initial Float32 conversion. Maximum observed conversion error
was 0.00049 m for height, 0.00000153°C for temperature/dew point, and zero for
these sampled U/V winds. This checks storage fidelity, not a complete meteorology
validation or the treatment of missing/below-ground fields elsewhere.

## Query performance and compression

Each row stores arrays over pressure levels, partitioned by revision and sorted
by `(cell_id, lead_hour)`. Both surface fields are scalars. These measurements
return all four retained times and every field at one arbitrary ingested cell.

| Layout                              | Final sample disk | Warm query p95 | Direct-I/O query p95 |
| ----------------------------------- | ----------------: | -------------: | -------------------: |
| 8192 rows/granule, LZ4              |         75.15 MiB |        13.1 ms |              29.8 ms |
| 256 rows/granule, LZ4               |         75.66 MiB |         6.8 ms |              17.3 ms |
| **256 rows/granule, ZSTD(1)**       |     **60.11 MiB** |     **7.9 ms** |          **18.4 ms** |
| 512 rows/granule, ZSTD(1)           |         59.58 MiB |         8.9 ms |              19.6 ms |
| 256 rows/granule, Gorilla + ZSTD(1) |         72.96 MiB |         9.3 ms |              21.2 ms |

There were 120 distinct-cell warm queries and 30 direct-I/O queries per layout.
Direct I/O bypassed the guest filesystem page cache; positive OS read-byte
counters confirm that path. It does not flush host or device caches. No ClickHouse
query-result cache was used. Results are localhost timings, excluding Internet
latency, TLS, a tunnel or the frontend.

**Use 256-row granules and ZSTD(1) as the first production candidate.** Smaller
granules cut the common index scan from 8,192 to 256 rows. Zstandard saved about
20% against LZ4 here; Gorilla did not help. The array values plus scalar fields
occupied 102.38 MiB before database encoding and about 60 MiB on disk, roughly
1.7× smaller. ClickHouse's own uncompressed count is larger because it includes
array offsets and keys; do not confuse the two compression denominators.

Four concurrent clients achieved about **424 queries/second** on the original
256/ZSTD sample, with p95 11.6 ms. The larger-insert rerun achieved 441/s. These
are short sample bursts, not full-horizon/full-region production capacity claims.
Querying an existing table while loading another kept p95 near 16 ms in the
larger-batch experiment, with no query errors, but that overlap lasted under one
second and does not characterize sustained production merges.

The complete local HTTP path, including coordinate lookup, JSON and gzip, took
**27–38 ms** for the four functional API requests. Each compressed response was
about 6.7 kB for four times. A 49-time response, a larger database and home Internet
latency remain unmeasured. Sub-second service is a reasonable acceptance target;
the measurements do not establish a full-CONUS latency SLA.

## Ingestion and source traffic

Every HRRR time required **182 GRIB messages**: 30 levels × six fields, plus two
surface fields. Their measured compressed sizes were 132.0–133.3 MB per time;
the four complete times totaled **529.91 MB**. Those same maps contain every
native HRRR grid cell, so retaining more cells increases staging/database work
without multiplying the required source messages.

- Four concurrent reads took 40.3 seconds for lead 0.
- Sixteen concurrent reads took 15.4, 10.8 and 11.1 seconds for leads 1–3.
- Peak loader RSS in the successful 16-reader measurements was about **0.97 GiB**.
- Successful-read process CPU totaled 24.8 seconds across all four times.
- The interface byte counts were about 7% above codec payloads; those counters
  are not isolated from all other workspace activity. Failed partial attempts
  and retries add traffic beyond the successful codec totals.

The concurrency comparison used different lead times and was not a controlled
repeated A/B experiment. It nevertheless demonstrates that waiting on source
requests dominated these reads. Increasing concurrency is useful within measured
memory and source-service limits; it does not remove the volume requirement.

ClickHouse insert batching mattered substantially. Loading 147,456 rows as
36 small inserts took about 3.0 seconds for 256/ZSTD; five larger inserts took
**0.94 seconds**, including the local transpose and Arrow transfer. That is about
157,000 rows/second. The larger-batch load wrote approximately 64.2 MB from the
server process versus a final table of 63.2 MB; the earlier small-batch load wrote
157.7 MB while background merges/query logs were active. These counters illustrate
write amplification, not an isolated long-term SSD-wear benchmark.

The larger-insert experiment sampled server RSS at approximately 0.9–1.0 GiB.
The production writer should target substantial sorted batches and cap memory,
parts and merge concurrency. At full horizon, the same cell batch contains many
more rows; tune by byte/row budget, not by blindly reusing a fixed cell count.

## CONUS storage estimates

The actual HRRR projection contains **1,638,757 native cells** in the proposed
24–50°N, 125–66°W bounding box. This includes surrounding land/water and is not
a political-border mask. The full HRRR domain has 1,905,141 cells.

For a 49-time HRRR run, scaling the larger-insert sample by
`1,638,757 / 36,864 × 49 / 4` gives **32.1 GiB per run on disk** and about
**8.5 minutes of ClickHouse loading** at the measured sample throughput.
Both are extrapolations; longer-time compression, geographic variability,
larger sorting buffers and sustained merges were not measured.

| Retained selection per cycle                             | Float32 values including two surface fields |                            Illustrative ClickHouse disk |
| -------------------------------------------------------- | ------------------------------------------: | ------------------------------------------------------: |
| HRRR box, 49 hourly times, 30 levels                     |                                   54.44 GiB |             **32.1 GiB**, measured-sample extrapolation |
| GFS box, 141 native times through 180h, 30 levels        |                                    2.38 GiB | ~1.40 GiB, using HRRR's ratio as an unmeasured scenario |
| AIFS box, 31 six-hourly times through 180h, eight levels |                                    0.12 GiB | ~0.07 GiB, using HRRR's ratio as an unmeasured scenario |
| Combined scenario                                        |                                   56.94 GiB |                                               ~33.5 GiB |

Raw sizes follow `cells × times × (levels × fields × 4 + 8)` bytes. GFS/AIFS
compression may differ materially from HRRR. Allowing roughly **25–45 GiB for
HRRR per run** is a planning reserve around the sample projection, not a
statistical confidence interval.

Keep two complete runs per model. Nominally that is about 67 GiB compressed.
Add about 57 GiB for one cycle's uncompressed regional staging, another 34 GiB
for a new compressed cycle and potentially another 34 GiB during merges/builds.
The resulting nominal working set is about **190 GiB before headroom**. Plan for
**200–300 GiB of free working capacity** for this uncompressed-staging design,
validate peak usage, and remove obsolete runs/checkpoints automatically. That
would be too close to the user's 300 GB SSD capacity; use the compressed-staging
revision below.

At four cycles/day, uncompressed staging alone writes about **6.7 TiB/month** for
all three selections. Initial database output adds around 3.9 TiB/month in this
scenario; merges can add several more. Plan around **11–20 TiB/month of logical
writes** until a sustained full-run measurement replaces this range. Compression
of staging or a more direct tiled transpose could reduce that, at CPU/complexity
cost. Device-level write amplification and endurance depend on the actual SSD.

### Revised staging for the user's 300 GB SSD

An additional lossless experiment compressed each variable separately in blocks
of 4,096 cells using ZSTD(1). The sample shrank from **102.38 MiB to 45.39 MiB**
(44.3% of its original size), and every block decoded exactly. Compression took
about 0.35 seconds and decompression about 0.11 seconds for the sample. This is a
codec/layout measurement; the extractor still writes `.npy` checkpoints and a
production compressed staging writer is not implemented yet.

This layout permits a spatial block to be read across all forecast times without
repeatedly decompressing complete maps. Scaling its ratio to the selected
regional fields gives about **25 GiB of staging for one complete three-model
cycle**, subject to the same unmeasured GFS/AIFS caveat. Nominal peak working data
becomes roughly **160 GiB (172 GB)**: two completed cycles, one compressed staging
cycle, one new database cycle and one extra merge/build copy.

For the proposed machine:

- Treat 300 GB as about 279 GiB before filesystem/OS usage. Budget approximately
  **170–220 GB of working data** initially; larger compression variation can
  exceed that range. Use free space, not nominal drive capacity, as the limit.
- Retain only the active and previous complete run. Stage/build one model at a
  time, release successful staging promptly, and prune obsolete completed runs
  after publication and in-flight-reader leases expire.
- Preserve at least **50–60 GB free**, reserve the measured expected build/merge
  space before starting work, and skip a cycle when that reserve is unavailable.
  Continue serving the last acceptable complete run or use Open-Meteo fallback.
- Bound concurrent merges and use large sorted inserts. The 32 GB RAM allocation
  permits substantially larger buffers than the sample needed, but full-region
  extraction/transpose memory must still be capped and measured.

Compressed staging reduces the estimated logical write range to approximately
**7–15 TiB/month** including initial database writes and a merge allowance.
This is meaningful SSD activity even when the retained data is small. Actual
device writes, free-space peaks and the SSD's endurance rating must establish its
replacement implications; no drive model or endurance budget was provided.

## Home Internet estimates

HRRR is the strongest measured basis: `mean measured GRIB bytes/time × 49` gives
**6.49 GB per complete run**, or **779 GB per 30-day month** at four runs/day,
before overhead, metadata, retries or other household use. The four-time sample
does not establish the average size of all 49 hours or different weather days.

GFS/AIFS compressed bytes were not measured because of the network failure.
For capacity planning only, applying HRRR's observed GRIB bytes per native grid
value to their full source-map sizes gives:

| Source    | Payload per cycle | Payload/month at four cycles/day | Basis                                   |
| --------- | ----------------: | -------------------------------: | --------------------------------------- |
| HRRR      |           6.49 GB |                          0.78 TB | Four measured times extrapolated to 49  |
| GFS       |          10.18 GB |                          1.22 TB | Unmeasured cross-model packing scenario |
| AIFS      |           0.52 GB |                          0.06 TB | Unmeasured cross-model packing scenario |
| All three |          17.19 GB |                          2.06 TB | Mixed measured/extrapolated scenario    |

If GFS/AIFS bytes per value are half or twice that assumption, the combined
payload becomes **1.42–3.35 TB/month**. This is a sensitivity range, not a
confidence bound. A 20% transport/retry allowance makes the middle scenario
approximately **2.47 TB/month**. The earlier regional crop saves local disk;
it cannot avoid the global GFS/AIFS GRIB source messages.

**Do not assume this fits a 1 TB home data cap.** HRRR alone would consume much of
it, and GFS may dominate total download volume despite its small retained US
grid. Unlimited home data materially improves the economics. This is the first
quantity to verify when source access recovers.

For the 17.19 GB/cycle scenario, pure transfer lower bounds are:

| Sustained download speed         | Download-only minutes/cycle |
| -------------------------------- | --------------------------: |
| 50 Mbps                          |                        45.8 |
| 100 Mbps                         |                        22.9 |
| 300 Mbps                         |                         7.6 |
| **500 Mbps (user's connection)** |                     **4.6** |
| 1 Gbps                           |                         2.3 |

These exclude request latency, CPU, staging, ingestion and retries. HRRR's source
timings project to roughly 10 minutes at the observed 16-request throughput,
plus the approximately 8.5-minute local database load. They are sample-based
projections, not a stopwatch measurement of a complete run. For all three models
on a similar four-core machine with sustained 100 Mbps download, reserve roughly
**45–90 minutes per six-hour cycle** initially. That operating envelope has
headroom over the scenario's transfer/load arithmetic but is not a measured
upper bound; full-run acceptance must establish actual completion time.

On the user's **500 Mbps uncapped** connection, the scenario's pure transfer
floor is 4.6 minutes, but tens of thousands of requests, decoding and local
insertion will dominate well before that line rate is fully useful. Reserve
**30–60 minutes per six-hour cycle** as an initial operating allowance, not a
measured completion guarantee. The 80 Mbps uplink is ample for modest profile
traffic: a hypothetical 100 kB compressed response takes about 10 ms of line time
and the theoretical line-rate ceiling is 100 such responses/second, before
protocol overhead, other traffic or API work. Full 49-time response sizes were
not measured. Cloud caching reduces repeated home-uplink traffic.

## Monthly spend and recommendation

With existing hardware and uncapped flat-rate Internet, ClickHouse itself adds
no service subscription. A dedicated machine averaging 20/40/80 W costs
**$3.60/$7.20/$14.40 per 30-day month at $0.25/kWh**. Average wall power was not
measured in this VM. Include idle time, and use only incremental power if the
machine would already be running.

Keep the regional database at home and cache small requested profiles in the
cloud. Low-volume serving may fit the previously documented R2/Workers free
allowances, subject to account usage, request count and cache retention. Domain,
tunnel, ISP overages, hardware purchase/replacement and maintenance are separate
unknowns; a $0 hosting bill does not make the complete system free.

**Proceed with the proposed four-core, 32 GB, 300 GB machine**, using compressed
staging, short retention and free-space reservations. The supplied uncapped
500/80 Mbps connection removes the main potential bandwidth-billing problem.
The prototype supports moving forward with local ClickHouse; it does not yet
justify a cloud subscription or bigger hardware purchase. Before replacing the current near-free source,
finish a complete 49-time HRRR/CONUS ingestion and measure GFS/AIFS source bytes,
then verify sustained loading, retention cleanup and point queries during merges.
