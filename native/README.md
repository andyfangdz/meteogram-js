# Native forecast service

One Rust application owns source acquisition, staging, ClickHouse ingestion,
validation, publication, retention, HTTP serving, scheduling and verification.
Build and test it with Cargo. Python, a virtual environment, NumPy, Xarray and
runtime compilation are not required. The binary uses Rust Icechunk 2.3.0 and
Gribberish 1.8.4 directly. Cargo also builds the native codec dependencies.
ClickHouse remains a separately installed database executable.

The Next.js application's optional native provider consumes this service through
an authenticated Cloudflare Tunnel. `src/utils/native-weather.ts` converts native
fields into the application's normalized meteogram columns. Open-Meteo remains
the fallback for unsupported locations/models and unavailable native forecasts.

## Build and run

Requires Linux, Rust 1.95 or newer, a C/C++ compiler, and an installed ClickHouse
binary. The ARM host uses Rust 1.99.1 and ClickHouse 26.9.20.23. No CMake or Python
build step is needed.

```sh
cargo +1.99.1 build --release --locked --manifest-path native/Cargo.toml
cargo +1.99.1 test --locked --manifest-path native/Cargo.toml
install -m 755 native/target/release/meteogram-native "$HOME/.local/bin/meteogram-native"

meteogram-native service \
  --root "$HOME/.local/share/meteogram-native" \
  --server-root "$HOME/.local/share/meteogram-benchmark" \
  --binary "$HOME/.local/share/meteogram-benchmark/clickhouse"
```

Keep state and large downloads outside the repository. The supervisor starts
ClickHouse on loopback ports 18123/19000 and the API on loopback port 18080.
It refuses occupied ports and restarts failed children. A persistent updater
polls source availability every 60 seconds and ingests individual forecast hours
as their required fields become available. `schedule.json` records polling
progress across restarts. `service-status.json` records child
PIDs. Stop the supervisor and its children with:

```sh
meteogram-native stop --root "$HOME/.local/share/meteogram-native"
```

Install `native/systemd/meteogram-native.service` as a user unit to have the
service manager supervise the Rust process. On hosts with a working user bus:

```sh
mkdir -p "$HOME/.local/share/systemd/user"
cp native/systemd/meteogram-native.service "$HOME/.local/share/systemd/user/"
systemctl --user daemon-reload
systemctl --user enable --now meteogram-native.service
```

A running direct supervisor must be stopped before starting the unit. The
included ClickHouse configuration has an empty local password and binds only
to loopback; it is for this local service.

## Operate

All API routes require `Authorization: Bearer <secret>` when a token is
configured. The service reads `METEOGRAM_API_TOKEN`, then an explicit
`METEOGRAM_API_TOKEN_FILE`, then `<root>/api-token`. A present but invalid token
file prevents startup. Without any token configuration, the API remains a local
unauthenticated service; configure authentication **before** running a tunnel.
Keep the file private (mode 0600). `check-api`, `observe` and `verify` read the
same token automatically. For the curl examples below, include
`-H "Authorization: Bearer $(cat "$HOME/.local/share/meteogram-native/api-token")"`
when authentication is enabled.

```sh
meteogram-native status --root "$HOME/.local/share/meteogram-native"
# Perform one incremental poll; the supervisor already runs the persistent watch:
meteogram-native update --root "$HOME/.local/share/meteogram-native"
meteogram-native update --root "$HOME/.local/share/meteogram-native" --models rrfs
meteogram-native watch --root "$HOME/.local/share/meteogram-native" --poll-seconds 60
meteogram-native rollback --root "$HOME/.local/share/meteogram-native" --model hrrr
meteogram-native gc --root "$HOME/.local/share/meteogram-native"

curl --noproxy '*' --compressed \
  'http://127.0.0.1:18080/forecast?model=hrrr&lat=41.013&lon=-74.027'
curl --noproxy '*' 'http://127.0.0.1:18080/health'
curl --noproxy '*' 'http://127.0.0.1:18080/runs?model=hrrr'
# Select a complete backup while the default serves the newest partial:
curl --noproxy '*' --compressed \
  'http://127.0.0.1:18080/forecast?model=hrrr&lat=41.013&lon=-74.027&run=latest_complete'
```

The updater checks fresh immutable Icechunk snapshots on each poll. Availability
checks read references without downloading whole-map weather payloads, with at
most 16 checks in flight. An hour is ready only when every required pressure and
surface map is referenced. Missing hours do not block other ready hours, and
already published hours are skipped. References can still point to unavailable
or invalid objects, so download, decode and full validation remain required.
Each acquired hour pins its initialization and snapshot; `lead_sources` in the
API response identifies those per-hour snapshots.

The default update includes HRRR, GFS, AIFS and RRFS. HRRR combines the hourly
18-hour product (19 leads) with the 48-hour product (49 leads) for 00/06/12/18 UTC
initializations. RRFS retains 85 hourly leads through 84 hours. GFS retains
hourly leads through 120 hours then three-hour leads through 180; AIFS retains
six-hour leads through 180. The region is 24–50°N, 125–66°W on each native grid.

RRFS uses `noaa-rrfs-forecast-84-hour-virtual`, with the same retained 1,638,757
cells and 30 pressure levels (1000–275 hPa) as HRRR. Both retain geopotential
height, temperature, grid-relative wind U/V, relative humidity and dew point,
plus 2 m temperature and dew point. RRFS needs a bounded eight-million-reference
cache at initial Icechunk repository open, with weather payload caching disabled;
Icechunk 2.3's `reopen` shares the original caches. Repositories persist across
polls while each poll opens a fresh immutable source session.

For complete forecasts, freshness requires HRRR age ≤24 hours and ≥12 hours of
forecast remaining; RRFS requires age ≤24 hours and ≥48 hours remaining;
GFS/AIFS require age ≤48 hours and ≥72 hours remaining. Partial forecasts use
the same age limit while explicitly reporting available/missing leads and
remaining horizon. A new run becomes the default as soon as its first hour
passes validation. `forecast_complete` distinguishes it from a complete run.
Publication requires the expected source schema, every retained cell for the
published hour, finite values, exact cell IDs and sampled bit-exact
staged/database values. Failed work leaves published hours intact and resumes
from durable staging on the next poll.

Incremental work permits two regional extractors (HRRR/RRFS) and one global
extractor (GFS/AIFS), with one hour at a time per model and at most 16 map reads
per extractor. A poll stops starting hours after its work budget, then drains
active hours. `watch.lock` prevents duplicate pollers; `update.lock` covers each
poll's recovery, acquisition, loading and publication. Source staging keeps its
20 GiB free-space floor. Explicit `update --batch` (also selected by `--pin-root`)
preserves the older complete-run benchmark mode and its whole-run admission
budgets. The normal default is incremental ingestion.

Catalog replacement is atomic and fsynced. Readers hold a shared catalog lock
through their database query; publication and garbage collection hold the
exclusive lock. The service protects the two latest complete runs per model,
plus the latest complete 48-hour HRRR run, counting overlap once. A partial run
never replaces a complete backup. Up to three recent partial runs and the newest
pending long HRRR run can be tracked separately. Retired manifests have a
ten-minute grace period. Garbage collection follows per-hour descriptors and
shared-table references and only drops explicitly owned service tables.
Crash recovery resumes private per-hour journals; retrying an unpublished
partition cannot change a published hour.

`/runs?model=MODEL` lists current, building and complete runs and the long HRRR archive.
`/forecast` defaults to the newest validated partial or complete run. Its `run`
parameter also accepts `latest_complete`, `previous_complete` and HRRR's `long`.
Explicit archive selection reports freshness even when stale; default selection
continues to reject stale runs. `/health` reports complete-backup counts and
whether the long HRRR archive is retained.

For importing a complete benchmark table, `adopt` creates an independent clone,
copies its descriptors, validates it and publishes it. It never takes ownership
of the original table. Add `--retain-only` to archive an older complete run
without changing the current selection:

```sh
meteogram-native adopt --root "$SERVICE_ROOT" --source "$MODEL_DATA" \
  --database meteogram_bench --table hrrr_256_zstd --retain-only
```

## Storage and query contract

The original physical format is `cell_profile_v1`: one row per native cell, with pressure
fields arranged as level/lead arrays and surface fields as lead arrays. Float32
bits are stored losslessly as `Array(UInt32)` with T64 and Zstd (level 3 for
HRRR, level 1 for GFS/AIFS); there is no
quantization. Legacy complete-run HRRR/RRFS acquisitions use the lossless dictionary/tile
format described below; legacy GFS/AIFS batch defaults retain raw Float32 bits.
New incremental acquisitions use exact UInt16 dictionaries and 256-cell tiles
for all four models. All models have the same
bounded 250-million-entry codebook allowance (1 GB decimal Float32 payload).
The ARM build uses the NEON
packing routine, finishing small groups
of cells across all leads to keep output profiles in CPU cache. Up to three
readers reuse bounded input/packing buffers, read full cell batches, and feed a
bounded queue of encoded ClickHouse Native blocks. Raw-bit HRRR uses four insert
workers with 4,096-cell batches; packed HRRR uses six workers, two readers and
16,384-cell batches, with at most four simultaneous inserts. RRFS uses the same
worker limits with 8,192-cell packed batches to bound its longer forecast's
Native block size. Its legacy raw path uses four workers and 2,048-cell batches.
GFS uses 2,048
cells and AIFS 4,096. Other models
default to one worker. `load --workers 1..6` selects an explicit
insert worker count. `load --packed` explicitly enables experimental 256-cell
tiles for other models with UInt16 dictionaries; it rejects raw-bit and UInt32
inputs. This does not change acquisition or scheduled-loader defaults.
Experimental packed GFS uses 4,096-cell batches, matching its staging frames.
Workers wait for each complete insert response, including
late server errors. Failed loads cancel and drain all workers before cleanup.
Only the new unpublished table has merges paused during loading and full
validation; merges must resume successfully before loading returns. On Linux,
consumed source pages receive a discard hint to preserve cache for database
parts. Other architectures use the portable Rust packing path.

Every completed insert is followed by a database query of that exact, disjoint
cell range. It checks the expected row count, revision value, every array length
and every stored value's finiteness before counting the batch as completed.
This checks the full dataset while new parts are still in cache. After all
workers join, global row counts, exact sorted cell IDs and physical types are
checked, plus ten complete bit-exact source/database profiles. Those source
samples are retained from the immutable input buffers during reading (about
357 kB for full HRRR). Only this in-memory completion proof permits the loader
to avoid a second full scan. The standalone `validate` command always performs
an independent whole-table scan, and `--staged` reads the staging files again.
Any failed attempt clears its validation marker and drains pending work.

`float32_dictionary_v1` stores exact Float32 palettes for each field/level/lead
and unsigned indices into them. It preserves every Float32 bit, including
signed zero; it does not quantize values. `values.dict.zst` is versioned,
checksummed, SHA-256-bound to metadata and required for serving. All palettes
must be finite, and every stored index must fit its own slot's palette.
Acquisition sorts palettes in Float32 total order and checkpoints them with
each durable lead. Palette files must survive weather-staging cleanup.

When all palettes fit UInt16, HRRR/RRFS use `cell_tiles_v1`: each row retains 256
adjacent native cells, preserving their ordered original IDs. For each
field/level/lead slot, 32 UInt128 words hold eight exact UInt16 indices each.
`Delta(2)/Zstd(1)` compresses the lanes. Every lane in every word is checked
against its exact palette bound after insertion, including zero padding in the
last tile. Report physical tile rows separately from logical cells and
cell/lead rows. This grouping reduces storage without averaging spatial cells.
The reader accepts 8-, 64- and 256-cell tiles, UInt16/UInt32 dictionary profiles,
and raw-bit profiles. SQL bound queries have an explicit 2 MiB parser limit.
Point queries return only the requested eight-lane word per slot, plus each
original stored array length for full-shape validation. They decode that cell's
lane and walk palettes in slot order before constructing time rows; complete
tile payloads are not transferred for a single-cell request. Full ingestion and
standalone validation still inspect every stored lane.

Incremental runs use `lead_partition_tiles_v1`: one shared table per model,
dataset and initialization, partitioned by `(lead_hour, intDiv(tile_id,128))`.
Each row stores 256 cells for one hour. A private single-hour table passes full
validation before its spatial partitions are attached to the run table; another
full check and staged samples precede atomic manifest publication. Readers query
only the manifest's validated hours, in one database request per point. Parts
attached before publication remain invisible to them. Published hours and their
independent `values.dict.zst` books are immutable. The 250-million-entry allowance
applies to the aggregate run, checked before publication and reader allocation.
Grid files share SHA-256-addressed hard links. API readers reuse decoded books
across manifest advances.

Standalone audits of published data hold a shared catalog lock and validate
through private descriptor copies. The existing marker remains readable until
a successful replacement, so auditing cannot temporarily invalidate API reads.
Each hour also retains its immutable staged-source validation proof.

`prepare-store --raw-only` exports a complete local table as exact Float32
staging without building palettes. It requires a fresh output directory and
conflicts with `--raw-output`. For large dictionaries, `encode-stage` then
builds sorted UInt16 palettes one spatial map at a time, releasing each lookup
table before processing the next map. Input/output lead matrices are each
bounded to 1 GiB, with at most two million cells. An individual palette must
fit UInt16; no quantization or truncation is used.

Compact codebooks have a uniform aggregate limit of 250 million Float32 entries
(1,000,000,000 payload bytes) and 65,536 slots across all fields, for every model.
These bounds apply during source checkpoint assembly and before
allocation in the reader. The legacy `prepare-store` dictionary builder
retains its stricter 32-million-entry **per-field hash-table** guard and is not
the bounded path for full GFS. Preparation is outside loader timing.

```sh
meteogram-native prepare-store --raw-only --source "$PINNED_DESCRIPTOR" \
  --table "$IMMUTABLE_SOURCE_TABLE" --output "$RAW_STAGE"
meteogram-native encode-stage --source "$RAW_STAGE" --output "$DICTIONARY_STAGE"
meteogram-native load --root "$BENCH_ROOT" --data "$DICTIONARY_STAGE" \
  --table "$FRESH_CANDIDATE_TABLE" --workers 4
# Add --packed to the load command to measure 256-cell tiles instead of profiles.
```

Use fresh output directories/tables. The codebook format remains `MGDICT01`,
but GFS codebooks above the old per-field reader limit and RRFS codebooks above
128 million entries require their upgraded readers before publication.
Existing HRRR/AIFS dictionaries remain readable.
These explicit commands do not change the acquisition or scheduled-loader
defaults.

For a UInt16 source of at most 8 GiB, the loader can prefetch all input files
inside the load clock when `MemAvailable >= input bytes + 18 GiB`; otherwise
it streams normally. It decodes checksum-checked, aligned staging frames
directly into UInt16 tile words. The queue and reader buffers are bounded;
consumed file pages receive discard hints. The server limit is 12 GiB (raised for concurrent incremental ingestion); the
per-query limit remains 2 GiB.
Packed loads release unused ClickHouse allocator pages initially and every
four validated batches, keeping background logging below the same RSS cap.
These releases and the final allocator purge are included in measured load time.
Independent packed-table audits scan one column at a time with two threads and
one tile per block, preserving every check under the existing 2 GiB query limit
even after parts are merged.

The reader also supports the earlier one-row-per-cell/lead format, so existing
current/previous tables need no migration. Both formats return the same JSON
rows. The source wind frame remains explicit: HRRR/RRFS grid-relative, GFS/AIFS
earth-relative. Arbitrary coordinates select the nearest retained native cell;
outside-region/domain requests return 404, malformed coordinates return 400,
and stale/unavailable forecasts return 503. Requests never download source maps.
`/health` requires all four supported models to be fresh and checks database
connectivity. Responses support
gzip and retain `Cache-Control: no-store`.

New weather staging uses `*.stage.zst` plus a versioned `*.stage.json` index.
Independent Zstd level-1 frames contain at most 4,096 cells, with reversible
Float32 byte shuffling and a checksum on every frame. Version 2 UInt16 staging
uses slot-major spatial deltas and byte shuffling, preserving every index.
The JSON index records
shape, dtype, encoding, and exact compressed offsets/lengths. Reads decode only
the required blocks into bounded buffers, including partial final blocks.
Coordinates use `cells.bin.zst`: a checksummed Zstd frame containing the
`MGCELL01` version tag, a little-endian UInt64 count, then contiguous UInt32 IDs
and Float64 latitude/longitude columns. All float bits are preserved.
Legacy NPY/NPZ descriptors remain readable. New acquisitions write neither
format. Restart the supervisor/API when upgrading this binary, before the next
publication, so every running reader supports the new coordinate format.
Per-lead checkpoint
reports make interrupted acquisitions resumable when staging remains intact.
Encoded source maps are removed only after both staged files are synced and the
lead checkpoint is durable. The cache retains unfinished leads; a resume also
prunes maps left behind after a checkpoint. Completed leads resume from staging,
so a complete encoded-map archive is not retained. After publication, weather staging
and remaining source cache are removed; descriptors,
coordinates and validation evidence remain. All service tables live in
`meteogram_native`; historical `meteogram_bench` tables are separate.
Staged files are synced before their durable checkpoint. Unreferenced failed
revision metadata expires after seven days.

## Verify and investigate

```sh
cargo fmt --check --manifest-path native/Cargo.toml
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings
# Include the integration tests while the isolated database is running:
cargo test --locked --manifest-path native/Cargo.toml -- --include-ignored

meteogram-native verify --root "$SERVICE_ROOT" --full --output "$RESULTS/verify.json"
meteogram-native benchmark --root "$SERVICE_ROOT" --count 300 --clients 4 \
  --output "$RESULTS/queries.json"
meteogram-native benchmark-pack --cells 4096 --leads 49 --repeats 5 \
  --output "$RESULTS/packing.json"
meteogram-native compare-stores --root "$CANDIDATE_ROOT" \
  --reference-root "$SERVICE_ROOT" --samples 100 --output "$RESULTS/exact.json"
meteogram-native check-api --root "$CANDIDATE_ROOT" --port 18082 \
  --compare-url http://127.0.0.1:18080 --output "$RESULTS/candidate-http.json"
meteogram-native convert-stage --source "$LEGACY_MODEL_DATA" \
  --output "$FRESH_COMPRESSED_DIRECTORY"
```

`check-api` owns a temporary API child and always stops it after verification.
`verify` checks city/interior/edge HTTP profiles against the database, invalid and outside requests,
gzip and optionally full database validation. `--compare-url` compares complete
HTTP payloads with another running service, excluding revision/query duration
and allowing <10⁻⁹-degree coordinate round-off. `compare-stores` requires
matching, nonempty model sets (including four-model catalogs) and checks every
cell ID and coordinate, source identity/schema, and complete sampled forecasts
against a second catalog. Weather values must match exactly. `benchmark` runs
an explicit warmup then measures complete profile queries; these are loopback,
filesystem-cache-enabled observations.

`benchmark-pack` compares the original lead-first traversal with several cell
tile sizes, using identical resident synthetic inputs, a warmup and rotating
execution order. It checks every output bit. These timings isolate packing;
they do not measure acquisition, staging I/O or a complete refresh.

For a new isolated full acquisition, pin metadata for each model under
`$PINS/{hrrr,gfs,aifs}/metadata.json`, then:

```sh
meteogram-native update --root "$CANDIDATE_ROOT" --pin-root "$PINS"
meteogram-native observe --until "$CANDIDATE_ROOT/last-update.json" \
  --output "$RESULTS/http-during-ingest.json" --disk-root "$CANDIDATE_ROOT"
```

Run the observer alongside the update. It records one fixed point per model,
checks immutable revision hashes, and samples free disk/host memory. It does
not establish publication latency, WAN latency, power use or a service SLA.

`fetch` supports `--metadata-only`, `--pin-metadata`, `--leads`, `--run-offset`
and `--output`; it always retains full CONUS geography. `inspect MODEL` prints
live Zarr metadata. `probe --model MODEL --pin-metadata FILE` resolves and
downloads one actual GRIB map, decodes its codec pipeline and reports its size.
`load`, `validate` and `staging-compression` expose the bounded loader, database
gates and lossless staging compression measurement. Use unique candidate tables;
never rebuild a serving table in place. `METEOGRAM_CLICKHOUSE_URL` can select a
different loopback ClickHouse HTTP endpoint for isolated tests.

The Python investigation tools were retired after the Rust migration. Their
historical versions remain in commit `5f4ae08` and its predecessors, together
with the dated benchmark reports. Historical command examples describe those
versions; current operations use this application.

The measured packing, source-preflight and cache changes are documented in
the [ingestion optimization report](../docs/plans/2026-10-18-native-ingestion-optimization.md).
The subsequent [full HRRR load and compressed staging report](../docs/plans/2026-10-18-hrrr-load-optimization.md)
records complete 116.36- and 118.03-second loads, including validation, and the
native staging format's measured storage reduction.
The [one-minute HRRR report](../docs/plans/2026-10-18-hrrr-one-minute-load.md)
records complete 50.21- and 53.70-second fresh-table loads, exact dictionary
storage, bounded insert concurrency, full validation and installation evidence.
The [GFS/AIFS benchmark report](../docs/plans/2026-10-18-other-model-load-benchmarks.md)
records three complete loads per configuration: four workers reduced median
GFS loading from 16.07 to 6.33 seconds and raw AIFS from 1.60 to 0.75 seconds.
AIFS tiles saved 40.8% of database-plus-codebook storage but increased warm
profile-query p95 from 5.75 to 10.24 ms. Full GFS dictionary preparation exceeded
the existing palette budget. These are isolated measurements; the installed
binary and service defaults were left unchanged.
The [bounded GFS dictionary follow-up](../docs/plans/2026-10-18-gfs-dictionary.md)
records a complete successful mapwise conversion at 310 MiB peak encoding RSS,
full-value roundtrip proofs, 43.9% storage savings for dictionary profiles, and
the projected point-query reader's measured latency tradeoff.

The [complete RRFS ingestion](../docs/plans/2026-10-18-rrfs-ingestion.md) records
all 85 leads, the uniform 1 GB codebook allowance, a 146.71-second validated
load, independent audits and four-model service installation.

The [incremental ingestion report](../docs/plans/2026-10-18-incremental-ingestion.md)
describes per-hour publication, complete-run retention and live verification.

## Cloudflare Tunnel and the site

Install `cloudflared`, authorize the `andyfang.app` zone with
`cloudflared tunnel login`, and create a named tunnel. Keep its credentials
outside the repository. The installed configuration lives at
`~/.local/share/meteogram-tunnel/config.yml`:

```yaml
tunnel: <tunnel-UUID>
credentials-file: /home/<user>/.local/share/meteogram-tunnel/credentials.json
metrics: 127.0.0.1:20242
ingress:
  - hostname: <your-tunnel-hostname>
    service: http://127.0.0.1:18080
  - service: http_status:404
```

Route the hostname with `cloudflared tunnel route dns <tunnel-UUID> <hostname>`.
Verify its HTTPS certificate and an unauthenticated 401 before adding the
server-only `METEOGRAM_API_URL` and `METEOGRAM_API_TOKEN` variables to Vercel.
An authenticated `/forecast` request should return 200 for each of the four
models. No database port is exposed. A hostname such as
`weather.api.andyfang.app` needs certificate coverage beyond Cloudflare's normal
single-label wildcard; validate that coverage before deploying the site.

Install `native/systemd/meteogram-tunnel.service` beside the native user unit
and enable both units on a host with a working user service manager. Stop direct
instances before starting managed instances, since the local ports are
exclusive. These unit files provide restart and boot configuration; a running
shell process alone does not establish reboot persistence.

The site adapter rotates regional winds using the grid's Lambert convergence,
converts vector speeds to km/h, computes WGS84 MSL heights, and retains the
actual pressure levels and lead times. Native GFS cloud fraction is used
directly. Other models use a pressure-dependent Sundqvist humidity cloud
estimate, following [Open-Meteo's meteorological implementation](https://github.com/open-meteo/open-meteo/blob/main/Sources/App/Helper/Meteorology.swift).
AIFS specific humidity is converted with Murphy–Koop saturation pressures and
ECMWF mixed-phase weighting. Those cloud estimates are explicitly labeled in
the site; no unavailable vertical layers or forecast hours are invented.
