# Local ClickHouse weather prototype

This is an isolated benchmark and native-profile API. It does not change the
meteogram-js provider, implement production publication/retention, or deploy a
public service. Large downloads, staged arrays, database files and binaries belong
outside the repository. The benchmark drops/rebuilds its own named tables in
`meteogram_bench`; never point it at a production database.

For a fresh home Linux machine, start with the
[host-agent handoff](../../docs/plans/dynamical-home-agent-handoff.md). It includes
portable setup, full-resident acquisition commands and the remaining acceptance
checks; historical `/workspace/` paths below describe the original cloud host.

The [completed home ARM benchmark](../../docs/plans/2026-10-17-clickhouse-home-resident.md)
records all three full resident models, query verification, the GFS batch-size
correction, and revised capacity/write estimates.

## Reproduce

Python 3.12 and Linux are required. The measured server is ClickHouse
26.9.20.23, the official `v26.9.20.23-stable` amd64 static release.

```bash
mkdir -p /workspace/dynamical-benchmark
uv venv /workspace/dynamical-benchmark/venv
uv pip install --python /workspace/dynamical-benchmark/venv/bin/python \
  -r scripts/clickhouse-prototype/requirements.txt
```

Download/extract `usr/bin/clickhouse` from the official release archive:

<https://github.com/ClickHouse/ClickHouse/releases/download/v26.9.20.23-stable/clickhouse-common-static-26.9.20.23-amd64.tgz>

Run the server in a separate terminal, substituting the extracted binary path:

```bash
python scripts/clickhouse-prototype/start-server.py \
  --binary /workspace/dynamical-benchmark/clickhouse \
  --work-dir /workspace/dynamical-benchmark
```

`start-server.py` renders absolute storage paths from `server.xml`. The server
binds only to loopback, HTTP port 18123 and native port 19000. It limits tracked
server memory to 6 GiB and individual queries to 2 GiB. This configuration has
an empty local password and is **not a public deployment configuration**.

Fetch samples. These commands still download complete source maps: a small
spatial sample reduces local output, not source traffic. The first command reads
all 49 HRRR times, 30 levels, six vertical fields and two surface fields.

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/fetch.py \
  --model hrrr --leads all --workers 16 --concurrency 16 \
  --output /workspace/dynamical-benchmark/hrrr

/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/fetch.py \
  --model gfs --leads 0,6,24,72,120,180 --workers 16 --concurrency 16 \
  --output /workspace/dynamical-benchmark/gfs

/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/fetch.py \
  --model aifs --leads 0,6,12,18,24,30,36,42,48,54,60,66,72,78,84,90,96,102,108,114,120,126,132,138,144,150,156,162,168,174,180 \
  --workers 16 --concurrency 16 --output /workspace/dynamical-benchmark/aifs
```

HRRR retains four 96×96 tiles centered in California, the Rockies, the Plains
and the Northeast (36,864 cells). GFS/AIFS retain all cells in the 24–50°N,
125–66°W rectangle. Defaults select the second-latest initialization; completeness
is checked by actual successful reads, not assumed from initialization age.
Outputs pin the Icechunk snapshot and initialization for subsequent retries.

One completed forecast time is checkpointed after all fields are written and
validated. Retrying skips completed times. A transient failed map gets up to four
attempts. The codec instrumentation counts successful GRIB chunks and compressed
bytes, not decoded array sizes. Network-interface counters also include protocol
overhead and any concurrent process traffic; use codec counts for primary source
volume estimates. Failed incomplete attempts may transfer additional uncounted
bytes and need a separate retry allowance.

Find the actual server PID (the `clickhouse` server, not its watchdog) and run:

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/benchmark.py \
  --data /workspace/dynamical-benchmark/hrrr \
  --output /workspace/dynamical-benchmark/hrrr-result.json \
  --server-pid SERVER_PID
```

Repeat with each model's data/output path. The default comparison covers
8192/LZ4, 256/LZ4, 256/ZSTD(1), 512/ZSTD(1), and 256/Gorilla+ZSTD(1).
All layouts use native Float32 values, one row per cell/time, arrays over pressure,
partitions by revision and ordering by `(cell_id, lead_hour)`.

The benchmark measures:

- Bounded transpose + Arrow ingestion, server/client CPU, process writes and
  sampled server RSS; disk size, compressed/uncompressed bytes and active parts.
- Warm point queries, filesystem-cache-bypassed reads via `O_DIRECT`, and four
  concurrent clients. `system.query_log` records actual rows/bytes read and OS
  reads. Direct I/O does not remove storage-controller or host-level caches.
- Point queries against the first loaded layout while another layout is loaded.
  This exercises shared CPU/I/O contention; it is not same-table publication.
- Exact Float32 round trips at eight deterministic random cells over every
  ingested lead, pressure level and field, plus row-count/unique-key checks.

These are localhost measurements with only the sampled spatial cells loaded.
Full-region latency and ingestion times require a subsequent capacity test.
The synthetic duplication of cells/times is deliberately not used to estimate
compression. Global server write counters may include background merges and
query logs and are an operational observation, not isolated device endurance.

For the 300 GB home-disk scenario, measure lossless staging compression separately:

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/measure-staging.py \
  --data /workspace/dynamical-benchmark/hrrr \
  --output /workspace/dynamical-benchmark/staging-compression.json
```

This validates interleaved, per-field and per-field/4096-cell-block compression.
It measures the proposed layout; it does not replace the extractor's `.npy`
checkpoints or implement a production compressed staging store.

## Query arbitrary coordinates

After loading the `256_zstd` table for each available model:

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/serve.py \
  --data-root /workspace/dynamical-benchmark --layout 256_zstd

curl --noproxy '*' --compressed \
  'http://127.0.0.1:18080/forecast?model=hrrr&lat=41.013&lon=-74.027'
```

The API resolves native grid indices and rejects cells outside the ingested
sample; it never starts ingestion. Nearby coordinates selecting the same cell
share its source values. Returned HRRR winds remain explicitly grid-relative.
The output includes native fields, units, pressure levels, selected coordinates,
source run and snapshot. Production meteorological transforms, cloud estimates,
terrain lookup, authentication, tunnel, gateway/cache and app integration remain
separate work. This local API does not implement Open-Meteo fallback.

## Full-volume retry

`--coverage conus` retains all HRRR cells in the geographic rectangle.
`--spatial-shards N --shard-index I` selects one disjoint, ordered subset.
`--cache-dir PATH` caches encoded source chunks under dataset/snapshot namespaces;
repeated spatial batches decode the same GRIB messages without downloading them
again. Codec `grib_bytes` counts payload bytes **decoded**, including cache hits;
the separate `source_cache.downloaded_bytes` counter counts new cache misses.
Neither counter includes failed transfers or transport overhead.

The disk-bounded runner uses eight HRRR spatial batches, each containing the full
49-time horizon, then full-CONUS GFS (141 times through 180 hours) and AIFS
(31 times through 180 hours). It validates each batch and saves results before
removing raw staging and dropping intermediate HRRR tables. It retains the last
HRRR batch and complete GFS/AIFS tables. **This does not leave a full-CONUS HRRR
serving database**, and query results characterize batch-sized databases.

To resume the pinned experiment after source connectivity recovers, find the
current ClickHouse server PID and run:

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/full-volume.py \
  --root /workspace/dynamical-benchmark/full \
  --server-pid SERVER_PID \
  --hrrr-pin /workspace/dynamical-benchmark/hrrr/metadata.json \
  --discard-completed-staging
```

This rebuilds the prototype's `hrrr_256_zstd` table, replacing the two-time table
used by the current local API. Stop that API before running the benchmark. For a
new source initialization, use a new output/cache namespace and pin metadata
from an initial shard extraction of that initialization. Source references may
eventually expire even though the Icechunk snapshot remains identifiable.

The runner writes `*-complete.json` only after ingestion, unique-key validation,
exact Float32 checks and query measurements. It retries source acquisition up to
five times, reusing checkpoints and cached chunks. Metadata and cell IDs must
match when resuming; a different spatial selection requires a new directory.
Two GiB of disk is reserved by the fetch/cache writers. This is an experiment
guard, not a production retention or admission-control implementation.

Cached chunks can also be replayed without any source connection. Missing chunks
raise errors rather than becoming fill values. The successful two-time CONUS
replay was:

```bash
/workspace/dynamical-benchmark/venv/bin/python scripts/clickhouse-prototype/fetch.py \
  --model hrrr --coverage conus --leads 0,1 \
  --output /workspace/dynamical-benchmark/full/conus-two-hours \
  --pin-metadata /workspace/dynamical-benchmark/hrrr/metadata.json \
  --cache-dir /workspace/dynamical-benchmark/full/source-cache \
  --offline-cache --workers 16 --concurrency 16
```

The [retry report](../../docs/plans/2026-10-17-clickhouse-full-volume-retry.md)
separates this completed two-time regional benchmark from the still-incomplete
49-time experiment. The app's provider is unchanged.

## Full-resident host runner

`resident-run.py` runs the three full-CONUS extractions and loads sequentially,
retains all three `256_zstd` tables together, measures staging compression, and
runs `verify-api.py`. It launches its isolated ClickHouse child in the same
process namespace as the benchmarks, which is required in sandboxes where each
shell invocation sees different PIDs. Use a dedicated server directory and free
loopback ports 18123, 19000 and 18080. It stops its server after the experiment;
restart `start-server.py` and `serve.py` to inspect the retained data later.

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/resident-run.py \
  --root "$BENCH_RUN" --server-root "$BENCH_ROOT" \
  --binary "$BENCH_ROOT/clickhouse"
```

The runner records one-second process RSS/CPU/I/O and host disk counters in
`telemetry.jsonl`, and each command's elapsed time/exit status in `phases.jsonl`.
It terminates the active job if free space falls below 20 GiB. Use a fresh root
for a fresh cycle; rerunning resumes source checkpoints but rebuilds benchmark
tables. It waits for an existing extractor's advisory lock before resuming that
model. This is a single-cycle benchmark, not atomic publication or a retention
service. Host disk counters include other host activity and do not measure NAND
write amplification. The runner's process telemetry excludes separately launched
extractors; their per-lead reports still record peak RSS, CPU and writes.

`verify-api.py` starts a temporary local API and checks the complete time/level/
field arrays against staging at city, random and edge cells for every model.
It also checks source identity, coordinate selection, invalid/outside requests,
null conversion, gzip size, and one/four-client loopback latency. Run it while
the isolated database is available:

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/verify-api.py \
  --data-root "$BENCH_RUN" --output "$BENCH_RUN/api-verification.json"
```

After `complete.json` exists, generate the arithmetic worksheet:

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/summarize-run.py \
  --root "$BENCH_RUN" --output "$BENCH_RUN/summary.json"
```

Its capacity scenarios include two retained cycles, a new build, a merge
allowance, one source-payload cache and 20 GiB reserve. They are not a measured
multi-cycle peak. The monthly scenarios assume 120 comparable cycles and label
power, tariffs and retry allowances separately from measurements.

The resident runner uses 4,096-cell inserts for HRRR/AIFS and 1,024 for GFS.
The 141-time GFS selection exceeded the 2 GiB query limit at 4,096 cells on the
home ARM host. After a later-model failure, preserve verified earlier tables:

```bash
"$BENCH_PYTHON" scripts/clickhouse-prototype/resident-run.py \
  --root "$BENCH_RUN" --server-root "$BENCH_ROOT" \
  --binary "$BENCH_ROOT/clickhouse" --models gfs,aifs
```

Omitted models must have matching benchmark/source metadata; their stored row
and unique-key counts are revalidated before resuming. Source checkpoints are
reused; selected model tables are rebuilt. Logs and host restart records are
preserved, and telemetry identifies server attempts because process counters
reset on restart. The final verifier also rechecks whole-table cell counts,
per-lead row counts and pressure-array lengths and runs database latency tests
with an explicit warm-up pass after all three models are resident.

## Managed native forecast pipeline on the home server

`service.py` runs the local ClickHouse server, native HTTP API and periodic
updater. `pipeline.py` builds immutable per-model tables in `meteogram_native`;
`native_store.py` owns the catalog, loader, validation, locking and retention.
This is a native-source service, not the app's `CloudColumn` provider. The app
continues to use Open-Meteo. HRRR winds are grid-relative; HRRR/AIFS clouds still
require a meteorological derivation in an eventual application adapter.

The installed host paths are:

- Code: `~/.local/share/meteogram-benchmark/repo` (writable checkout).
- Python/binary/database: `~/.local/share/meteogram-benchmark/`.
- Catalog, revision metadata, schedules and operational logs:
  `~/.local/share/meteogram-native/`.
- API: `http://127.0.0.1:18080`; database HTTP/native ports 18123/19000.

The service uses loopback only and does not configure a public gateway, TLS,
remote access or a frontend provider switch.

### Publication and read consistency

Each candidate gets a unique directory and table. Extraction is pinned to one
snapshot and initialization. A metadata-only probe rejects unchanged/older runs,
unexpected field/level/grid coverage and stale runs before downloading maps.
The updater tries the latest initialization, then the second latest if the first
is incomplete. It retains all 49 HRRR, 141 GFS and 31 AIFS leads. Required values
must all be finite; a missing field/time fails the candidate, rather than filling
missing observations or shortening the forecast.

Loading uses 4096-cell batches (1024 for GFS) and never replaces an existing table.
Validation checks total and bounded unique keys, exact per-lead row counts,
pressure-array lengths and finite values over the entire table. Deterministic
sample profiles must exactly match staged Float32 values. Only then is a
validation marker written and the catalog atomically replaced with file/directory
fsync. Initialization must advance, except for an explicit rollback.

Readers hold a shared `catalog.lock` from catalog selection through the DB read.
Publication and garbage collection take its exclusive lock. A request therefore
uses metadata, grid coordinates and rows from one revision, and a table cannot be
deleted under an in-flight query. The API caches coordinate arrays per revision.
Models publish independently, because their source schedules differ.

Current and previous revisions are retained per model. Older service-owned tables
are dropped after a ten-minute grace period, on the next GC/update. Adoption
creates independent service-owned ClickHouse snapshots using `CLONE AS`;
rebuilding a benchmark table cannot change the snapshot. Local immutable files
are initially hard-linked, as described in [ClickHouse's cloning documentation](https://clickhouse.com/blog/table-cloning). Future merges can write new files.
Original benchmark artifacts are outside automated service cleanup. The dated
deployment report records this host's one-time reclamation of historical raw
arrays/cache after verification; original tables and small evidence remain. Service-owned staging
and encoded payload caches are removed after publication or candidate rejection;
small metadata, source reports and logs remain. Unpublished crashed candidates are
dropped on the next update and rebuilt in fresh tables. Discarded revision evidence
expires after seven days. `update.lock` prevents overlapping writers and recovery
from touching an active build.

### Availability and scheduling

`GET /health` returns 200 only when all three catalog models pass the freshness
policy and ClickHouse responds. `/forecast?model=hrrr&lat=41&lon=-74` serves native
profiles; invalid requests return 400, uncovered coordinates 404, and missing,
stale or temporarily unavailable data 503. There is no upstream fetch on a request.

Freshness uses actual remaining horizon as well as initialization age:

| Model | Maximum initialization age | Minimum remaining horizon |
| --- | ---: | ---: |
| HRRR | 24 hours | 12 hours |
| GFS / AIFS | 48 hours | 72 hours |

A future initialization over one hour ahead is rejected. These are explicit
initial operational policies, not evidence of a weather-product SLA. An app
adapter must interpret 503 as a reason to use Open-Meteo.

The supervisor starts an update on initial startup, waits six hours after a
successful job, and retries failed jobs after 30 minutes. It persists the next
scheduled time across restarts, restarts the API/ClickHouse after process exit,
and terminates updater/API before the database on shutdown. Children use separate
process groups so a database shutdown signal cannot stop the supervisor. ClickHouse recovery
is followed by candidate recovery before the next job. Logs are kept locally;
supervisor child logs rotate at 20 MiB on child starts, events rotate at 20 MiB,
and database query logs have a seven-day TTL. This is not yet a remote
alerting or hardened public HTTP service.

Before each model, free space must exceed a 20 GiB reserve plus an additional
210/35/5 GiB for HRRR/GFS/AIFS. Disk checks run during extraction and between load
batches. These conservative admission budgets include merge/obsolete-part room;
they are not maximum observed usage. New service tables use a 60-second obsolete
part lifetime instead of accumulating the benchmark's default lifetime. Do not
reduce the reservation without measuring subsequent update peaks.

### Host commands

From the writable checkout, using the installed Python:

```bash
NATIVE_PYTHON="$HOME/.local/share/meteogram-benchmark/venv/bin/python"
NATIVE_ROOT="$HOME/.local/share/meteogram-native"

# Inspect current / previous revisions and freshness.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/pipeline.py --root "$NATIVE_ROOT" status
curl --fail http://127.0.0.1:18080/health

# Manual update, provided another update does not hold update.lock.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/pipeline.py --root "$NATIVE_ROOT" update

# Switch one model back to its still-fresh previous revision.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/pipeline.py --root "$NATIVE_ROOT" rollback --model hrrr

# Collect expired revisions and clean abandoned candidates.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/pipeline.py --root "$NATIVE_ROOT" gc

# Stop a directly launched supervisor (also stops its API/database/updater).
"$NATIVE_PYTHON" scripts/clickhouse-prototype/pipeline.py --root "$NATIVE_ROOT" stop

# Run the supervisor directly, with no existing server on its ports.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/service.py \
  --root "$NATIVE_ROOT" \
  --server-root "$HOME/.local/share/meteogram-benchmark" \
  --binary "$HOME/.local/share/meteogram-benchmark/clickhouse"
```

Initial adoption uses `pipeline.py --root "$NATIVE_ROOT" adopt --benchmark-root
"$BENCH_RUN"` with a completed benchmark. It validates the actual resident tables
before publication, clones the database tables, and copies metadata/grid
coordinates. Existing catalog models are not overwritten by adoption. The
`isolate-imports` command migrates early borrowed catalog references into these
independent snapshots without changing their forecast initialization.

The supplied `systemd/meteogram-native.service` uses these host paths. Install it
under `~/.local/share/systemd/user/` and enable it with a link in
`default.target.wants/` (or `systemctl --user enable`). The host has user lingering
enabled. From an ordinary host terminal with access to its user bus, use:

```bash
systemctl --user daemon-reload
systemctl --user start meteogram-native.service
systemctl --user status meteogram-native.service
```

Stop any directly launched supervisor first with `pipeline.py --root "$NATIVE_ROOT" stop`: it deliberately refuses an already
occupied ClickHouse port and holds `service.lock`. A disabled or inaccessible
user bus prevents manager activation; a unit file alone does not prove the
manager started the process. `systemd --user --test --unit=default.target` can
verify the startup dependency graph without starting services. See the dated deployment report for actual status.

### Verification

```bash
"$NATIVE_PYTHON" scripts/clickhouse-prototype/test-serve.py
TEST_NATIVE_DB=1 "$NATIVE_PYTHON" scripts/clickhouse-prototype/test-pipeline.py
"$NATIVE_PYTHON" scripts/clickhouse-prototype/verify-live-update.py \
  --output "$NATIVE_ROOT/live-update-verification.json"

# Check retained row counts, HTTP profiles, freshness and log retention.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/verify-service.py --root "$NATIVE_ROOT"

# Deliberately interrupt the local API to test automatic DB/API recovery.
# Run only with an idle updater; catalog and schedule must survive unchanged.
"$NATIVE_PYTHON" scripts/clickhouse-prototype/verify-service.py \
  --root "$NATIVE_ROOT" --restart-database
```

The unit tests cover publication, monotonicity, stale/incomplete candidates,
reader locking, rollback, crash cleanup, retention and disk admission. The
opt-in DB test uses a clearly labeled tiny synthetic fixture in a uniquely named
test table, checks exact round trips/corruption rejection and removes that table.
The live observer uses real HTTP requests while genuine forecasts update; it
records revisions, latency, errors and profile stability within each revision.
A successful all-model transition is distinct from a unit test or synthetic load.
