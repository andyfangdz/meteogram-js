# Full-resident CONUS benchmark on the home ARM host

**Completed October 17, 2026: one real, complete HRRR/GFS/AIFS cycle is resident
and verified.** The three tables contain **84,579,313 rows in 33.01 GiB**.
Arbitrary-location HTTP checks passed, including full-horizon comparisons with
staged source values, boundary locations and concurrent requests.

This host can accommodate the experiment and has enough free capacity for the
conservative retention scenario below. **The earlier recommendation for a
300 GB working budget needs qualification:** raw staging, obsolete parts and
merges make that budget insufficient under the conservative plan. Even compressed
staging leaves little room on a nominal 300 GB disk before the OS. Storage cleanup
and write amplification need optimization before unattended use. At four cycles/day,
measured volumes project to **2.98 TB/month of source payload and 25.8 TB/month of
logical process writes**. The current app still uses Open-Meteo.

## Host, source and scope

The actual host differs from the handoff assumption: eight ARM cores (four
Cortex-A78C and four Cortex-X1C), 30.15 GiB RAM, a KIOXIA KBG40ZNS512G BG4A
512 GB NVMe drive, and ext4. Approximately 413 GiB was free before setup/data
acquisition, versus the proposed 300 GB available budget. Linux kernel is
7.0.0-34-generic. ClickHouse is the official ARM64 **26.9.20.23** build; Python
is **3.12.15** with the pinned dependencies and recorded transitive versions.
Fetch concurrency/workers are 16. ClickHouse keeps the supplied four-thread
profile, two-thread benchmark queries, 6 GiB server and 2 GiB query memory limits.
There was no four-core host quota imposed for this run.

The supplied 500/80 Mbps uncapped Internet plan remains an assumption; this run
did not independently measure uplink line rate. Git checkout metadata was
read-only in the original workspace, so work and the commit use the writable
clone at `~/.local/share/meteogram-benchmark/repo`, on
`codex/dynamical-clickhouse-handoff`, based on `4d33beb`.

All three selected initializations are **2026-10-17 12Z**, using fresh snapshots:

| Model | Retained cells | Times | Levels | Rows | Icechunk snapshot |
| --- | ---: | ---: | ---: | ---: | --- |
| HRRR | 1,638,757 | 49 | 30 | 80,299,093 | `MDGYCWNK87AP9GNYDSDG` |
| GFS | 24,885 | 141 | 30 | 3,508,785 | `V5SK6H0M0RF697RTBZZ0` |
| AIFS | 24,885 | 31 | 8 | 771,435 | `KGX0VQA3TPWZRWK9Q7G0` |

Coverage is the native-grid intersection with **24–50°N, 125–66°W**, including
surrounding water/land, not a political boundary mask. HRRR retains all 49 hourly
leads 0–48. GFS retains 0–120 hourly and 123–180 every three hours, 141 times.
AIFS retains 0–180 every six hours, 31 times. HRRR/GFS use 30 levels from
1000 to 275 hPa; AIFS uses 1000, 925, 850, 700, 600, 500, 400 and 300 hPa.
HRRR/GFS retain six native vertical fields and AIFS five; every model also
retains 2 m temperature and dew point. Full field lists, units, grids and
per-time measurements are in the source evidence. No repeated or synthetic
forecasts were used.

## Acquisition, loading and the corrected GFS batch size

| Model | Successful source GB | Fetch/decode/stage seconds | Source CPU-seconds | Load seconds | Final resident GiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| HRRR | 6.586 | 488.26 | 952.47 | 664.67 | 31.290 |
| GFS | 17.424 | 573.54 | 767.54 | 17.08 | 1.627 |
| AIFS | 0.849 | 30.21 | 35.52 | 1.43 | 0.094 |

The acquisition used **35,882 successfully decoded GRIB messages**, totaling
24.859 GB, with **zero recorded map retries**. Per-lead interface receive counters
sum to 25.858 GB, but include transport and unrelated host traffic; they are not
an isolated retry-byte measurement. Metadata/open/resume traffic is outside the
GRIB payload count. GFS's 17.424 GB replaces the previous unmeasured 10.18 GB
projection. AIFS's measured 0.849 GB replaces its 0.52 GB projection.

Acquisition timers combine overlapping download, decode, regional gather and
raw file writes. They do **not** separately measure pure download or staging-write
elapsed time. Aggregated codec decode wall times were 209.98/407.62/17.95 seconds
for HRRR/GFS/AIFS; these overlap across workers and are not CPU time. Source CPU
totaled 1,755.53 seconds. Successful fetch/decode/stage plus database load totaled
**1,775.19 seconds (29.59 minutes)**, excluding opens, validation and other overhead.
The complete experiment, including a failed insert, repair, restart, compression
round trips and HTTP/database verification, took approximately **39.35 minutes**
from the first acquisition log's creation, 23:18:26–23:57:47 UTC.

HRRR loading used 401 inserts of up to 4,096 cells: **313.36 seconds transpose,
19.10 seconds Arrow construction and 332.14 seconds insert calls**. Server/client
CPU during that load was 1,289.71/219.47 seconds. The other model-specific CPU and
phase measurements are retained in their benchmark JSON.

The first GFS insert with 4,096 cells × 141 times exceeded the **2 GiB query
memory limit** while parsing Arrow (attempted 2.05 GiB). This was a bounded query
failure, not a host OOM. The retry rebuilt only GFS with **1,024-cell batches**
(25 inserts), using the unchanged complete staging. It passed without raising
memory limits or downloading forecasts again. HRRR's stored key counts were
revalidated on restart. AIFS used seven inserts of up to 4,096 cells. Both the
resident runner and the older disk-bounded fallback now select the smaller GFS
batch; the fallback was not used for this experiment.

## Correctness and arbitrary-location verification

Every required retained source value was finite in this cycle. Maximum observed
Float32 conversion error was less than 0.000489 m for native geopotential height,
0.00000184°C for temperature/dew point and 0.00000382 m/s for U/V. Per-field error
and finite counts are preserved; no missing values were replaced with zero.

Validation passed for:

- Total rows and bounded unique `(cell_id, lead_hour)` counts for every model;
  all retained distinct-cell counts, exact rows for every lead, and zero incorrect
  pressure-array lengths across the entire database.
- Exact Float32 database round trips at eight deterministic cells/model, across
  every retained time, level and field.
- Full HTTP profile comparisons at **20 HRRR and 18 each GFS/AIFS locations**:
  Atlanta, Seattle, Miami, Los Angeles, Denver, Boston, random cells and regional
  extrema/native edges. The checks verify requested-to-native selection, selected
  coordinates, source initialization/snapshot, complete leads and every field.
- Invalid/nonfinite coordinates returning 400, and out-of-region coordinates
  returning 404, including points just outside the retained rectangle. Null
  conversion has a separate check; this all-finite cycle did not exercise a
  naturally missing source value through the HTTP path.

A genuine boundary bug was fixed: first retained HRRR cell **1560** round-tripped
through longitude/latitude to a projected Y coordinate **6.98e-10 m** beyond its
native bound. The API now allows **1e-6 m numerical tolerance** at projected
bounds and checks requested CONUS bounds before nearest-cell selection. Three
focused regression tests and the real HTTP edge checks passed. These tests
establish source/storage fidelity, not meteorological normalization.

## Full-horizon query results with all models resident

Final database tests explicitly warm 300 distinct cells before timing 300 warm
queries, 30 direct-I/O queries and a 300-query four-client burst per model. All
queries return the model's complete retained forecast and fields.

| Model | Warm p50 / p95 / p99 ms | Direct-I/O p95 ms | Four-client p95 ms | Four-client requests/s |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 8.52 / 11.02 / 12.03 | 39.40 | 14.21 | 319.9 |
| GFS | 13.35 / 15.90 / 16.90 | 33.31 | 15.92 | 280.1 |
| AIFS | 9.40 / 11.20 / 11.93 | 25.00 | 11.96 | 373.7 |

Direct-I/O queries recorded nonzero OS reads (27.23/26.48/17.53 MB total for
HRRR/GFS/AIFS); warmed queries recorded zero OS read bytes. Direct I/O bypasses
the filesystem page cache but does not prove the physical device cache was cold.
The earlier per-model benchmark's `warm` label denotes its cache-enabled first
pass, without this explicit warm-up; use the final resident results above for
steady warmed comparisons. Full distributions and query-log evidence are saved.

Each API model had 100 single-client and 300 four-client requests, cycling through
its verified locations, with **zero errors in all 1,200 timed requests**:

| Model | One-client p50 / p95 / p99 ms | Four-client p50 / p95 / p99 ms | Four-client requests/s | Median gzip kB |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 73.7 / 84.8 / 86.9 | 130.3 / 159.3 / 187.5 | 30.6 | 65.9 |
| GFS | 57.9 / 70.6 / 72.0 | 100.2 / 136.8 / 150.8 | 39.4 | 148.3 |
| AIFS | 17.9 / 19.6 / 20.5 | 21.9 / 28.9 / 31.8 | 179.5 | 11.2 |

These include coordinate lookup, JSON, gzip, loopback transfer and client parsing.
The prototype still reloads coordinate arrays from `cells.npz` on access; that
optimization remains unmeasured. Timings exclude WAN RTT, TLS and a gateway.
At the assumed 80 Mbps uplink, median response payload alone takes approximately
6.6/14.8/1.1 ms of line time, not an end-to-end service guarantee.

Reads during ingestion had p95 **17.00/16.78/12.15 ms**, respectively, with
10,293/253/17 reads and zero errors. HRRR's maximum was 1,018.64 ms. These readers
queried already-inserted cells while the rest arrived. They do not test serving an
old complete revision during an atomic replacement or sustained public load.

## Storage, memory and operating cost

All-model resident data is **33.011 GiB**, raw staging **56.943 GiB**, and encoded
source payload cache **23.152 GiB**. Lossless per-field/4,096-cell-block ZSTD(1)
staging measurements are 27.450/1.531/0.080 GiB, totaling **29.060 GiB**. Every
compressed block decoded exactly. HRRR compression/decompression codec time was
160.89/56.48 seconds; its complete layout/round-trip measurement took 306.25
seconds. **This compressed layout is measured but is not the extractor's writer.**

Minimum sampled filesystem free space was **244.439 GiB**. Monitored disk growth
was 160.140 GiB, starting about 61 seconds after the initial extractor began;
it excludes the first part of acquisition. The whole experiment consumed roughly
169 GiB at peak relative to the rounded pre-run free-space observation. Temporary
obsolete parts alone reached an observed **68.370 GiB**, over twice the final
33.011 GiB resident cycle. Part sampling began during HRRR loading, so this is an
observed peak rather than proof of an upper bound. Ordinary cleanup reclaimed
most of that space; no forced `OPTIMIZE` was used.

Peak extractor RSS was 3.58 GiB (HRRR), 0.85 GiB (GFS), 0.71 GiB (AIFS).
ClickHouse peak observed across periodic/insert samples was about **2.08 GiB**.
Summed child RSS reached 23.78 GiB, including mapped staging/cache/shared pages;
host available memory never fell below **24.05 GiB** in the periodic samples.
Do not interpret summed RSS as anonymous allocation. Initial standalone HRRR
extraction is covered by its per-lead counters, not the runner's child sampler.

A conservative **planning scenario**, not a measured second-cycle test, is:

| Budget item | GiB |
| --- | ---: |
| Two retained cycles plus one new build | 99.03 |
| One additional cycle-sized merge output allowance | 33.01 |
| Observed obsolete-part peak, budgeted separately | 68.37 |
| One source-payload cache | 23.15 |
| Raw staging / proposed compressed staging | 56.94 / 29.06 |
| Free-space reserve | 20.00 |
| **Total with raw / compressed staging** | **300.51 / 272.63** |

That is **322.67 / 292.73 GB before OS, tools, logs, metadata and old source
caches**. It fits this host's initial 413 GiB free capacity. It does not establish
that a nominal 300 GB disk can safely run the current pipeline with the intended
retention. The prior simpler merge-only budget is 232.14/204.26 GiB, but omits the
observed obsolete-part accumulation and should not be used as the safe admission
threshold. Model-at-a-time staging/cleanup and better merge policy may lower the
conservative peak; measure them before reducing reservations.

At **four comparable cycles/day for 30 days**:

- Source payload: **2.983 TB/month**, or **3.580 TB** with an illustrative 20%
  protocol/retry allowance. The allowance is not measured retry overhead. With
  the supplied flat-rate uncapped plan, this does not add a per-GB ISP charge.
- Raw staging writes alone: **7.337 TB/month**. Source-process writes, including
  saved source chunks, project to **10.329 TB/month**.
- ClickHouse process writes were **128.898 GB** across both server attempts and
  post-load work, about **3.64×** final resident bytes. This includes merges/logs,
  not just initial table output. Projected server writes are **15.468 TB/month**;
  source plus server is **25.797 TB/month (23.46 TiB)**. This supersedes the
  earlier unvalidated 7–15 TiB/month range. Failed-attempt/background work is
  retained in these counters rather than silently excluded.
- Host NVMe block counters increased **211.717 GB** during telemetry, including
  unrelated activity and excluding the first HRRR interval. Neither process nor
  block-device counters measure NAND amplification or establish SSD lifetime.
- Electricity is **unmeasured**. At an assumed $0.25/kWh, continuous average
  incremental 5/10/20 W costs $0.90/$1.80/$3.60 per month; a dedicated host
  averaging 40/80 W costs $7.20/$14.40. The evidence includes $0.15–$0.30/kWh
  scenarios. Existing hardware/flat-rate Internet avoid a new infrastructure
  subscription, but do not eliminate power, wear, maintenance or public-access
  costs. No new paid service was provisioned.

## Recommendation and remaining production work

Proceed with development on **this actual host**, whose capacity is sufficient
for this measured single cycle and the conservative planning scenario. A service
can plausibly stay near the existing cash cost on already-owned hardware and
uncapped Internet, but this experiment does **not** establish a zero-cost,
unattended replacement: approximately 25.8 TB/month of logical writes is a
material operational tradeoff. First optimize obsolete-part lifetime/merge work,
insert sizes and staging/cache cleanup, then remeasure disk/write peaks.

Next, build a second cycle in isolated revisions/tables and verify atomic
publication while serving the previous complete cycle. Add bounded retention,
free-space admission, restart/failure handling and freshness-based publication.
The tested second-latest 12Z runs were almost 12 hours old by the end; remaining
horizon, not just nominal 48/180-hour lead coverage, must determine eligibility.
A point-oriented materialized Zarr comparison is still unperformed.

The app adapter still needs HRRR wind rotation, humidity/dew-point/cloud estimate
policies (HRRR/AIFS cloud fraction is estimated), source attribution, native
cadence and sparse AIFS levels, descending pressure order, latitude-dependent
WGS84 height conversion, terrain handling, and Open-Meteo fallback for unsupported
or insufficiently fresh/covered requests. The native API is not that adapter.
No provider switch, public service, second-cycle availability claim or WAN SLA
is delivered by this benchmark.

## Reproduction and evidence

See the [prototype commands](../../scripts/clickhouse-prototype/README.md#full-resident-host-runner)
and [updated handoff](dynamical-home-agent-handoff.md). The runner now keeps the
server and benchmark in one PID namespace, preserves failed/restart evidence,
monitors a 20 GiB disk reserve, and supports resuming later models without
rebuilding verified earlier tables. The server/API were stopped after successful
verification; all staged arrays, source cache and database remain under
`~/.local/share/meteogram-benchmark/`, outside Git.

The approximately 1 MB [evidence bundle](../benchmarks/2026-10-17-clickhouse/home-resident/README.md)
contains source snapshots/per-lead counters, all database/staging/API results,
resource time series, failure evidence and the [arithmetic worksheet](../benchmarks/2026-10-17-clickhouse/home-resident/summary.json).
Validation also included Python compilation, Ruff checks, and the three boundary
regression tests. The application code was unchanged; no frontend build was
needed for these Python benchmark and documentation changes.
