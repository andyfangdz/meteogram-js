# Native forecast pipeline on the home server

Implemented a local native-field ingestion/query service on the same ARM host as
the [full-resident benchmark](2026-10-17-clickhouse-home-resident.md). The code is
in the writable checkout at `~/.local/share/meteogram-benchmark/repo`, on
`codex/dynamical-clickhouse-handoff`. The web application still uses Open-Meteo.
All three models are serving independently validated **2026-10-17 18Z** forecasts,
with the **12Z** cycle retained for rollback. This deployment does not include
the meteorological `CloudColumn` adapter,
a public gateway, TLS or a provider switch.

## Runtime and operation

The local API listens on **127.0.0.1:18080**. `/forecast?model=hrrr&lat=41&lon=-74`
returns a complete native profile; `/health` reports model freshness and database
availability. Invalid inputs return 400, uncovered locations 404 and unavailable
or stale forecasts 503. Queries do not fetch upstream source maps.

The supervisor runs ClickHouse on loopback 18123/19000, restarts failed child
processes, and runs ingestion at startup and six hours after successful jobs.
Failed jobs retry after 30 minutes. The next scheduled time is persisted across
restarts. State, logs, catalog and small revision artifacts are under
`~/.local/share/meteogram-native/`; the ClickHouse data and installed runtime
remain under `~/.local/share/meteogram-benchmark/`.

The [operational README](../../scripts/clickhouse-prototype/README.md#managed-native-forecast-pipeline-on-the-home-server)
contains status, update, rollback, garbage collection, startup and test commands.

## Publication and retention

Every model builds into a uniquely named table in `meteogram_native`. The loader
never drops or overwrites a serving table. Before downloading maps, a metadata
probe pins the source snapshot/initialization and checks the expected CONUS grid,
field list, pressure levels and full native forecast cadence. Unchanged/older
initializations are skipped. Latest and second-latest source initializations are
tried in order; incomplete runs do not replace current data.

Publication requires complete source markers, finite values, correct total and
unique keys, exact per-lead counts, full-table pressure-array length/finite checks,
and exact sampled Float32 comparisons with staging. It also requires a newer
initialization and enough remaining forecast horizon. HRRR permits at most
24 hours of initialization age with at least 12 hours remaining; GFS/AIFS permit
48 hours of age with at least 72 hours remaining. These are initial operational
policies, not an availability SLA.

The catalog is written to a temporary file, fsynced, renamed atomically, then its
parent directory is fsynced. Readers hold a shared lock from catalog selection
through their database read; publication and cleanup take an exclusive lock.
Each response uses one revision's coordinates, metadata and forecast rows.
Models publish independently. One updater lock serializes updates, rollback,
recovery and cleanup.

Current and previous revisions are retained per model. Older service-owned tables
are collected after a ten-minute grace period at the next update/GC. Source cache
and raw staging are removed after publication or candidate rejection. Interrupted
unpublished candidates are dropped and rebuilt, never appended to. Small failed
candidate evidence is retained for seven days. Rollback only selects a previous
revision that still passes the freshness policy.

The original benchmark artifacts remain separate. Adoption uses independent
ClickHouse table snapshots, initially sharing immutable files through local hard
links; rebuilding a benchmark table cannot alter the service snapshot. This uses
[ClickHouse's documented cloning behavior](https://clickhouse.com/blog/table-cloning).
Subsequent merges can allocate new files, so logical table byte counts must not
be equated with unique filesystem bytes. Historical benchmark tables, grid coordinates and small evidence are preserved.
After the deployment checks passed, the old benchmark raw arrays and source cache
were explicitly reclaimed; those external experiment files are not part of the
service's automatic cleanup policy.

## Disk controls

Ingestion reserves 20 GiB of free space and requires an additional
210/35/5 GiB before starting HRRR/GFS/AIFS, respectively. The extractor is monitored
for free space; the loader checks between bounded inserts. GFS uses 1,024-cell
batches and HRRR/AIFS 4,096. Service-owned tables use a 60-second obsolete-part
lifetime. These are conservative admission controls, not measured peak guarantees;
background merges and unrelated processes can also consume disk.

The measured compressed staging format is still not the writer. The pipeline
uses raw staging and reclaims it promptly. Original benchmark tables and small artifacts are still present; table snapshots
can share their physical files, so do not add all logical table sizes to estimate
unique disk usage.

## Startup configuration

The user unit is installed at
`~/.local/share/systemd/user/meteogram-native.service`, with a dependency link in
`default.target.wants/`. The host has user lingering configured. Unit syntax
verification passed. A `systemd --user --test --unit=default.target` dry run
included `meteogram-native.service -> start` and the expected default-target
Want, confirming the startup graph without starting other services.

The active process was started directly under the supervisor because this session
cannot communicate with the host's user bus (`No data available`). Live systemd
activation and a reboot have not been verified. `systemctl --user --root=/
is-enabled` reports disabled for this data-directory dependency placement; the
startup graph test, rather than that installation-state label, establishes the
configured dependency. The unit lives in the documented
[user unit search path](https://www.freedesktop.org/software/systemd/man/253/systemd.unit.html).
Do not run another supervisor while the current one owns its lock and ports.


## Completed live update and verification

The first update job ran from 00:54:52 to 01:29:56 UTC on October 18
(October 17 local time). It performed genuine new source acquisition, ingestion,
validation and publication while serving the previous complete cycle.
The latest HRRR 00Z candidate was incomplete: at one lead it decoded 180 of the
182 required GRIB messages. The candidate was rejected and cleaned up, and the
18Z initialization was selected instead. GFS and AIFS also selected 18Z.

| Model | Cells | Leads | Levels | Rows | 18Z source snapshot |
| --- | ---: | ---: | ---: | ---: | --- |
| HRRR | 1,638,757 | 49 | 30 | 80,299,093 | `G2DVQ244H9B0DTPYSY80` |
| GFS | 24,885 | 141 | 30 | 3,508,785 | `V5SK6H0M0RF697RTBZZ0` |
| AIFS | 24,885 | 31 | 8 | 771,435 | `KGX0VQA3TPWZRWK9Q7G0` |

A snapshot can contain multiple initializations: GFS/AIFS use the same snapshot
IDs as the original benchmark, but these are newly acquired **18Z data**, not
repeated 12Z values. All required retained source values were finite. Each model
passed full-table key/count/array/finite checks and exact comparisons with staging
at ten deterministic cells across every lead, level and field. The accepted
source reads totaled 24.963 GB; that excludes the rejected HRRR candidate and
metadata traffic and is not an all-attempt network measurement.

One sequential HTTP observer cycled four locations across all three models while
ingestion ran. It recorded **2,817 successful requests and zero errors**, observed
all three transitions from 12Z to 18Z, and checked that each sampled profile stayed
unchanged within its revision. This was an update-continuity check, not a public
concurrency/load test or a WAN measurement.

| Model | HTTP p50 ms | HTTP p95 ms | Maximum ms |
| --- | ---: | ---: | ---: |
| HRRR | 66.0 | 88.4 | 633.7 |
| GFS | 73.1 | 101.4 | 9,938.4 |
| AIFS | 17.3 | 37.2 | 307.2 |

**Latency limitation:** there were multi-second HTTP outliers during ingestion,
including the 9.94-second GFS response. The longest logged `native_api` database
query was 403 ms, so SQL execution alone does not explain the HTTP tail. The cause
outside measured SQL execution remains unresolved. Do not claim a near-100-ms
worst case or a public service SLA from these results.

Operational checks passed for:

- All nine pipeline tests, including real tiny-fixture insertion, exact round
  trips, corruption rejection, clone independence and real expired-table GC;
  plus the three native-coordinate regression tests.
- Actual HTTP rollback from 18Z to 12Z and back for each model. Restored responses
  matched their original hashes exactly, excluding the timing field.
- Migration of the initial borrowed benchmark references into independent
  service-owned snapshots, validated before changing the catalog reference.
- Automatic database/API recovery after deliberate `SYSTEM SHUTDOWN`, in
  **32.07 seconds**, preserving all **169,158,626 rows** across the six retained
  model tables, the catalog and the next refresh time.
- The explicit `pipeline.py stop` command, subsequent supervisor start, and final
  healthy API/row-count checks. The service is left running.
- Seven-day database query-log TTL, Python compilation, Ruff checks/formatting,
  unit syntax and startup dependency checks.

The first restart exercise exposed that ClickHouse's shutdown signal could also
stop the supervisor when they shared a process group: the installed version
[sends `SIGTERM` to group zero](https://github.com/ClickHouse/ClickHouse/blob/v26.9.20.23-stable/src/Interpreters/InterpreterSystemQuery.cpp#L426-L431).
The supervisor now starts
each child in a separate session/process group; the repeated restart check passed.
A disposable heartbeat-output failure was also injected into the initial
supervisor during migration; its cleanup released the service lock and stopped
its children. These checks do not simulate sudden power loss or a host reboot.

The persisted next update at handoff is **2026-10-18 07:29:57 UTC**
(**03:29:57 EDT**). Scheduling remains six hours after successful jobs and
30 minutes after failed jobs.

## Cleanup and disk state

After both generations were verified, cleanup removed **442 raw array files**
(61.142 GB) and **36,434 cached source files** (24.861 GB) from the completed
benchmark run. That is **86.002 GB of logical file contents**. Database tables,
independent service snapshots, grid coordinates, metadata and small benchmark
results remain. The old experiment cannot be replayed from its deleted raw/cache
files; reacquisition requires a new output directory.

The host then had approximately **370.2 GB / 344.8 GiB free**. The six active
service tables represent about **70.74 GB of logical table storage**; original
benchmark tables can share files with their adopted snapshots.

Partial telemetry, starting partway through HRRR loading, observed at least
**188.17 GB / 175.25 GiB free**, and up to **24.90 GB / 23.19 GiB** of inactive
native table parts. Those are observations over the sampled interval, not proof
of a complete-run peak or an upper bound. The original conservative admission
budgets remain in place.

## Evidence

The [deployment evidence bundle](../benchmarks/2026-10-17-native-pipeline/README.md)
contains source metadata, validation markers, HTTP transition/latency summaries,
rollback and restart checks, cleanup accounting, catalog/schedule snapshots,
partial resource samples and test results. Full per-request observations and
operational logs remain in the local service directory. No large forecast
arrays, caches, binaries or database files are committed.
