# Home Linux benchmark handoff (archived)

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

Branch: `codex/dynamical-clickhouse-handoff` in
<https://github.com/andyfangdz/meteogram-js>.

**Current runtime, October 18:** the service is now entirely Rust. Start with
the [native service guide](../../native/README.md) and
[full Rust migration report](2026-10-18-native-rust-service.md). The Python
commands below are historical; their code remains in commit `5f4ae08` and earlier
history. Publication, retention and rollback have since been implemented and
verified on the host.

**Completion update, October 17:** the [home full-resident report](2026-10-17-clickhouse-home-resident.md)
now records a completed HRRR/GFS/AIFS cycle and arbitrary-location checks. Its
measured storage/write/cost findings supersede the earlier projections below.
The remaining production work is a second-cycle atomic-publication/retention
test and reducing staging/merge overhead. The commands below remain reproduction
instructions; the GFS batch size has been corrected from the host measurement.

## Your assignment

Finish the **actual full-volume acquisition and ClickHouse benchmark on this
host**, then produce measured operating/storage/cost estimates. The user wants
arbitrary coordinates across the continental US, eventually replacing every
meteogram-js model Dynamical can support: HRRR, GFS and AIFS. The existing service
effectively costs them nothing, so recurring cost is a primary constraint.

Assume four CPU cores, 32 GB RAM, 300 GB available SSD capacity, 500 Mbps download,
80 Mbps upload and no data cap; inspect the real host and record any differences.
Proceed with local setup, downloads, prototype fixes and measurements. Do not
stop just to ask whether to continue the already-requested benchmark. Do not
change the app's provider or publish a public service as part of this handoff.

Read `AGENTS.md`, then these documents in order:

1. [Latest retry and measurement limits](2026-10-17-clickhouse-full-volume-retry.md).
2. [Initial benchmark and cost assumptions](2026-10-17-clickhouse-prototype-results.md).
3. [Arbitrary-location design](2026-10-17-dynamical-arbitrary-locations.md).
4. [Prototype commands and data formats](https://github.com/andyfangdz/meteogram-js/blob/5f4ae08/scripts/clickhouse-prototype/README.md).
5. [Field coverage and meteorological constraints](2026-10-14-dynamical-migration.md).

The earlier five-site/static design is superseded by arbitrary CONUS coverage.

## What is available, and what remains unproven

The branch contains all source scripts, configuration, reports and small JSON
benchmark evidence. It contains **no forecast arrays, GRIB cache, ClickHouse
binary/data directory or Python environment**. Paths beginning `/workspace/`
in historical evidence belong to the previous cloud machine, not this host.
No credentials are required for the public forecast sources.

Measured so far: all 1,638,757 HRRR cells inside the 24–50°N, 125–66°W rectangle,
but only two forecast times. That produced 3,277,514 rows, 1.207 GiB in
ClickHouse, a 31.13-second load and 6.85 ms warm-query p95. The original smaller
four-tile/four-time experiment compared codecs. The 49-time HRRR run, GFS/AIFS
acquisition, full-horizon queries and a complete update cycle remain unmeasured.

The cloud machine repeatedly lost access through its managed outbound tunnel,
returning HTTP 503 even for unrelated sites. This does not establish a Dynamical
outage and should not be assumed to affect the home connection. It also had only
about 28 GiB free disk. Do not copy its proxy configuration to this host.

Use a **fresh initialization and snapshot**. The historical October 16 snapshot
may still exist while its virtual GRIB objects have expired. Fetch defaults select
the second-latest initialization, but only successful reads of every required
field/time establish completeness. A one-time probe is not a completeness test.

## Set up an isolated benchmark

Check out the branch without overwriting unrelated work. If a checkout already
exists, use its normal remote and a separate worktree if needed. For a new clone:

```bash
git clone --branch codex/dynamical-clickhouse-handoff \
  https://github.com/andyfangdz/meteogram-js.git
cd meteogram-js
```

Run the following from the repository root. Keep large files on the SSD outside
the repository. Use a fresh experiment directory so old checkpoints cannot be
mixed with a new initialization. Preserve the same variable values across
commands; separate agent shell calls may need the exports repeated. Do not
generate a new `BENCH_RUN` timestamp for each command:

```bash
export BENCH_ROOT="$HOME/.local/share/meteogram-benchmark"
export BENCH_RUN="$BENCH_ROOT/runs/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$BENCH_RUN"
uname -m
lscpu
free -h
df -h "$BENCH_ROOT"
```

Record the actual CPU model, RAM, filesystem/device, free space and software
versions. Prefer at least **140 GiB free** for the first complete resident cycle,
including raw staging, a source cache, database growth and merge headroom. This
is an admission estimate, not a measured peak. Monitor free space throughout;
keep at least 20 GiB free and stop before that reserve is consumed. Do not start
by retaining two old cycles or all five codec variants.

Install `uv` or Python 3.12 through the host's normal tooling if absent. With uv:

```bash
uv venv --python 3.12 "$BENCH_ROOT/venv"
uv pip install --python "$BENCH_ROOT/venv/bin/python" \
  -r scripts/clickhouse-prototype/requirements.txt
export BENCH_PYTHON="$BENCH_ROOT/venv/bin/python"
```

The tested ClickHouse version is 26.9.20.23. On **x86_64**:

```bash
export BENCH_CH_VERSION=26.9.20.23
curl -fL --retry 3 \
  "https://github.com/ClickHouse/ClickHouse/releases/download/v${BENCH_CH_VERSION}-stable/clickhouse-common-static-${BENCH_CH_VERSION}-amd64.tgz" \
  -o "$BENCH_ROOT/clickhouse.tgz"
tar -xzf "$BENCH_ROOT/clickhouse.tgz" -C "$BENCH_ROOT" --strip-components=3 \
  "clickhouse-common-static-${BENCH_CH_VERSION}/usr/bin/clickhouse"
"$BENCH_ROOT/clickhouse" --version
```

On ARM, use the matching official architecture release and verify dependency
support; record that this differs from the original EPYC/x86 benchmark.

Run ClickHouse in a persistent terminal or under a local process supervisor:

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/start-server.py \
  --binary "$BENCH_ROOT/clickhouse" --work-dir "$BENCH_ROOT"
```

The supplied config uses only loopback ports 18123/19000, a separate
`meteogram_bench` database, four query threads, a 6 GiB tracked server memory
limit and a 2 GiB per-query limit. It has an empty local password. Use this
isolated server; benchmark commands drop/rebuild their named tables.

In the terminal running the benchmark, set `BENCH_SERVER_PID` to the actual
ClickHouse **server** PID shown by `ps -C clickhouse -o pid,args`, not the launcher
or watchdog PID. Check readiness with:

```bash
curl --noproxy '*' --fail http://127.0.0.1:18123/ping
```

The handoff branch bounds duplicate-key validation to 32,768-cell ID ranges,
avoiding an all-80-million-key aggregation under the 2 GiB query limit. This was
verified against the real two-time CONUS table and duplicate/missing-key cases.
The full 49-time table has still not been tested.

## Acquire and load one complete resident cycle

Use `fetch.py` and `benchmark.py` directly for the primary home experiment.
**Do not use `full-volume.py` as the primary serving setup**: that runner was
created for the cloud disk limit and drops seven of eight HRRR spatial batches.
The home goal is to retain the complete regional table simultaneously.

Start with HRRR. Stop on a failed command and inspect its log before continuing:

```bash
"$BENCH_PYTHON" -u scripts/clickhouse-prototype/fetch.py \
  --model hrrr --coverage conus --leads all --workers 16 --concurrency 16 \
  --cache-dir "$BENCH_RUN/source-cache" --output "$BENCH_RUN/hrrr" \
  > "$BENCH_RUN/hrrr-fetch.log" 2>&1

"$BENCH_PYTHON" -u scripts/clickhouse-prototype/benchmark.py \
  --data "$BENCH_RUN/hrrr" --output "$BENCH_RUN/hrrr-benchmark.json" \
  --server-pid "$BENCH_SERVER_PID" --layouts 256_zstd \
  --batch-cells 4096 --queries 300 --read-during-ingest \
  > "$BENCH_RUN/hrrr-benchmark.log" 2>&1
```

Use separate log files for reruns to preserve failed-attempt evidence. Retrying
the same fetch output pins the existing initialization/snapshot and skips
completed times; cached chunks avoid downloading successful partial work again.
Do not edit metadata to force incompatible data into an old directory. If the
run is incomplete upstream or references have expired, choose an older complete
or newer available initialization in a **new directory**, using `--run-offset`
as appropriate, and retain evidence of why the first run was rejected.

Then acquire GFS and AIFS sequentially, preserving the HRRR table:

```bash
BENCH_GFS_LEADS=$("$BENCH_PYTHON" -c \
  'print(",".join(map(str, list(range(121)) + list(range(123,181,3)))))')
BENCH_AIFS_LEADS=$("$BENCH_PYTHON" -c \
  'print(",".join(map(str, range(0,181,6))))')

"$BENCH_PYTHON" -u scripts/clickhouse-prototype/fetch.py \
  --model gfs --coverage conus --leads "$BENCH_GFS_LEADS" \
  --workers 16 --concurrency 16 --output "$BENCH_RUN/gfs" \
  > "$BENCH_RUN/gfs-fetch.log" 2>&1

"$BENCH_PYTHON" -u scripts/clickhouse-prototype/benchmark.py \
  --data "$BENCH_RUN/gfs" --output "$BENCH_RUN/gfs-benchmark.json" \
  --server-pid "$BENCH_SERVER_PID" --layouts 256_zstd \
  --batch-cells 1024 --queries 300 --read-during-ingest \
  > "$BENCH_RUN/gfs-benchmark.log" 2>&1

"$BENCH_PYTHON" -u scripts/clickhouse-prototype/fetch.py \
  --model aifs --coverage conus --leads "$BENCH_AIFS_LEADS" \
  --workers 16 --concurrency 16 --output "$BENCH_RUN/aifs" \
  > "$BENCH_RUN/aifs-fetch.log" 2>&1

"$BENCH_PYTHON" -u scripts/clickhouse-prototype/benchmark.py \
  --data "$BENCH_RUN/aifs" --output "$BENCH_RUN/aifs-benchmark.json" \
  --server-pid "$BENCH_SERVER_PID" --layouts 256_zstd \
  --batch-cells 4096 --queries 300 --read-during-ingest \
  > "$BENCH_RUN/aifs-benchmark.log" 2>&1
```

Use 1,024-cell inserts for the 141-time GFS selection. The home ARM run found
that 4,096 cells exceeded the configured 2 GiB query-memory limit while parsing
Arrow; HRRR's 49-time 4,096-cell inserts succeeded. Batch by forecast volume,
not by cell count alone.

Expected selections for the currently inspected source grids:

| Model |     Cells |                    Times | Levels |       Rows |
| ----- | --------: | -----------------------: | -----: | ---------: |
| HRRR  | 1,638,757 |         49 hourly, 0–48h |     30 | 80,299,093 |
| GFS   |    24,885 | 141 native times, 0–180h |     30 |  3,508,785 |
| AIFS  |    24,885 |    31 six-hourly, 0–180h |      8 |    771,435 |

Investigate source schema changes rather than silently forcing these counts or
omitting fields. Sixteen fetch workers mostly overlap requests; the config
still limits ClickHouse query threads to four. Measure memory and reduce workers
or insert batch size if needed; record changes. Failed attempts and retries are
part of operational cost even when they are absent from successful chunk totals.

Measure staging compression while the arrays remain available:

```bash
for BENCH_MODEL in hrrr gfs aifs; do
  "$BENCH_PYTHON" scripts/clickhouse-prototype/measure-staging.py \
    --data "$BENCH_RUN/$BENCH_MODEL" \
    --output "$BENCH_RUN/$BENCH_MODEL-staging.json" \
    --layouts by_field_4096_cell_blocks \
    > "$BENCH_RUN/$BENCH_MODEL-staging.log" 2>&1
done
```

This measures a lossless compressed layout; it does not replace the raw `.npy`
staging writer. Only clean up this experiment's regenerable large files after
verification and saving evidence. Preserve metadata and `cells.npz` for the API.
The fetcher deliberately rejects a checkpoint whose raw staging was removed;
use a fresh output directory if reacquisition is needed after cleanup.

If real free space cannot accommodate the resident experiment, the eight-batch
runner is a fallback for aggregate volume. Its telemetry currently assumes
`/sys/fs/cgroup/memory.current`; adapt that to the host or add a clearly labeled
process-RSS fallback. Do not label its aggregate results full-resident latency.

## Serve and verify arbitrary coordinates

Once all tables are verified, run the local prototype API:

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/serve.py \
  --data-root "$BENCH_RUN" --layout 256_zstd
```

Test `/forecast?model=hrrr&lat=33.6407&lon=-84.4277` on loopback port 18080,
then varied coordinates across CONUS for each model. Include locations outside
the original four tiles, edge cells, invalid/nonfinite coordinates and points
outside the retained region. Check selected native-cell coordinates, snapshot,
initialization, complete lead list, every pressure array length, finite/null
handling, HTTP status and gzip response size. Compare returned values with staged
arrays at deterministic cells; verify all times/levels/fields for those cells.

Record single-client and concurrent latency, p50/p95/p99, warm versus direct-I/O
behavior, and queries during ingestion. Direct I/O does not prove the physical
device cache is cold. API timings on loopback do not measure home Internet RTT.
The API currently reloads arrays from `cells.npz` on access; loading coordinate
arrays once is a possible measured optimization, not evidence already collected.

## Deliverables and next decisions

Commit a dated report and small machine-readable evidence on this branch. Keep
large datasets, binaries, logs with credentials and virtual environments out of
Git. Update `AGENTS.md` if workflows or architecture change. Include:

- Exact source snapshots/initializations, full geographic/time/level/field
  coverage, row/unique-key checks and Float32 round trips.
- Successful source bytes, retry overhead where observable, actual download,
  decode, staging, transpose and load durations; separate CPU from elapsed time.
- Actual complete resident table sizes, peak disk and RSS, merge amplification,
  process/host write counters and their limitations. A process write counter is
  not an SSD endurance measurement.
- Full-horizon API/database latency, response sizes and errors under load.
- A home capacity plan for two retained cycles plus a new build, staging,
  merges and free-space reserve. Measure a second-cycle build with isolated
  tables/revisions and atomic publication before claiming update availability;
  the current benchmark drops/recreates tables and cannot establish that.
- Monthly traffic at four runs/day, electricity scenarios, SSD-write estimates,
  freshness/refresh time and operational cost, with assumptions shown. Separate
  actual measurements from projections and untested production behavior.

First complete and report the single-cycle benchmark. Then recommend whether
this 300 GB machine can run the service near its current cost, and identify the
remaining implementation work. If network or source failures persist, preserve
the evidence and report the precise blocker instead of substituting repeated
data or presenting projections as completed runs.

For the eventual app adapter, preserve native Celsius decoding, pressure order,
WGS84 geopotential-to-MSL conversion using latitude, HRRR wind rotation, missing
value handling and source attribution. HRRR/AIFS cloud fraction requires a
labeled estimate; AIFS has fewer levels and six-hour cadence. Serve prepared
regional profiles; never fetch/decode entire source maps during a page request.
Unsupported models and insufficient coverage/freshness still need Open-Meteo.
