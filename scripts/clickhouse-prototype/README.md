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
