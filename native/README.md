# Native forecast service

The deployed Rust service acquires, validates and serves HRRR, GFS, AIFS and
RRFS forecasts for arbitrary CONUS coordinates. It owns source reads, hourly
ClickHouse ingestion, publication, retention, the point API and supervision.
The Next.js site uses it through an authenticated Cloudflare Tunnel, with
Open-Meteo fallback.

All four models use the same lossless per-hour dictionary/tile pipeline. Current
operations are described here; [dated investigation and rollout records](../docs/README.md)
retain their original measurements and historical implementation details.

## Build and install

Requires Linux, Rust 1.95 or newer, a C/C++ compiler and a ClickHouse executable.
The home host uses Rust 1.99.1 and ClickHouse 26.9.20.23. Cargo builds the GRIB
codec dependencies; Python, NumPy and runtime compilation are not required.

```sh
cargo +1.99.1 build --release --locked --manifest-path native/Cargo.toml
cargo +1.99.1 test --locked --manifest-path native/Cargo.toml
install -m 755 native/target/release/meteogram-native "$HOME/.local/bin/meteogram-native.new"
mv "$HOME/.local/bin/meteogram-native.new" "$HOME/.local/bin/meteogram-native"
```

Use the configured Cargo target directory if it differs from `native/target`.
Keep state, source caches, database parts, credentials and binaries outside Git.
After upgrading the executable, restart the managed native unit so the API and
updater use the same version.

The native and tunnel user units are installed, enabled and active on the home
host, with user lingering enabled:

```sh
systemctl --user status meteogram-native.service meteogram-tunnel.service
systemctl --user restart meteogram-native.service
systemctl --user stop meteogram-native.service
```

For a new host, install the files from `native/systemd/` into
`~/.local/share/systemd/user/`, run `systemctl --user daemon-reload`, then enable
both units after configuring the tunnel. Stop direct instances before starting
managed ones; their ports are exclusive. For a directly managed process:

```sh
meteogram-native service \
  --root "$HOME/.local/share/meteogram-native" \
  --server-root "$HOME/.local/share/meteogram-benchmark" \
  --binary "$HOME/.local/share/meteogram-benchmark/clickhouse" \
  --poll-seconds 60
```

The supervisor starts ClickHouse on loopback 18123/19000, the API on loopback
18080 and a persistent updater. It restarts failed children and records their
PIDs in `service-status.json`. Use `systemctl --user stop` for managed units;
`meteogram-native stop --root ROOT` is for a directly launched supervisor.

## Operate

```sh
NATIVE_ROOT="$HOME/.local/share/meteogram-native"
meteogram-native status --root "$NATIVE_ROOT"
# Run one poll; the managed supervisor already polls continuously.
meteogram-native update --root "$NATIVE_ROOT"
meteogram-native update --root "$NATIVE_ROOT" --models rrfs --work-seconds 50
meteogram-native rollback --root "$NATIVE_ROOT" --model hrrr
meteogram-native gc --root "$NATIVE_ROOT"
```

`update` and `watch` ingest individual forecast hours. Availability checks open
fresh immutable Icechunk snapshots and inspect references without downloading
weather maps. An hour is ready only when every required pressure and surface
map is referenced. Download, decode and full validation still follow: a valid
reference alone does not prove its object's contents.

Completed hours are skipped. Missing hours do not block other ready hours.
Private journals and checksummed staging resume interrupted work without
replacing published hours. The updater permits two regional extractors and one
global extractor concurrently, one hour per model and up to 16 map reads per
extractor. The work budget stops starting more hours, then drains active work.
`watch.lock` prevents duplicate pollers; `update.lock` covers recovery through
publication. Source staging retains a 20 GiB free-space floor.

| Model | Forecast times                                     | Retained pressure levels | Native grid cells in CONUS |
| ----- | -------------------------------------------------- | ------------------------ | -------------------------- |
| HRRR  | Hourly to 18 h; to 48 h for 00/06/12/18 UTC cycles | 30, 1000–275 hPa         | 1,638,757                  |
| RRFS  | Hourly to 84 h                                     | 30, 1000–275 hPa         | 1,638,757                  |
| GFS   | Hourly to 120 h; every 3 h to 180 h                | 30, 1000–275 hPa         | 24,885                     |
| AIFS  | Every 6 h to 180 h                                 | 8, 1000–300 hPa          | 24,885                     |

Coverage is 24–50°N, 125–66°W, subject to each source grid. Keep every configured
cell, field, level and forecast time. HRRR/RRFS retain their source-declared
spherical Lambert grid and grid-relative winds; GFS/AIFS winds are earth-relative.
HRRR and RRFS open Icechunk with bounded eight-million-reference manifest caches and
weather payload caching disabled. Configure that cache at initial repository
open because `reopen` shares the original asset manager.

Complete-run freshness requires HRRR age ≤24 h and ≥12 h remaining; RRFS age
≤24 h and ≥48 h remaining; GFS/AIFS age ≤48 h and ≥72 h remaining. Partial runs
use the age limit and report actual progress. A new run can become the default
as soon as its first hour passes validation. The site overlays it on retained
complete runs and independently checks sufficient continuous coverage before
selecting native data.

## Storage and publication

Every model uses `lead_partition_tiles_v1`: one shared table per
model/dataset/initialization, partitioned by
`(lead_hour, intDiv(tile_id,128))`. Each row contains 256 adjacent native cells
for one forecast hour. Exact Float32 palettes encode weather values with UInt16
indices; each UInt128 packs eight independent indices. Original ordered grid
IDs are preserved. There is no averaging, thinning or quantization.

Each palette must fit 65,536 entries. The aggregate allowance is 250 million
Float32 entries per run (1 GB decimal payload), with 65,536 metadata slots.
Codebooks are versioned, checksummed and SHA-256-bound to metadata. They are
required serving files, not regenerable staging.

An hourly child descriptor uses `cell_tiles_v1` with `partitioned_lead=true` and
one matching `lead_hour`. Its weather staging is indexed Zstd v2 with UInt16
spatial deltas and byte shuffling. Coordinates use checksummed `cells.bin.zst`.
Cloud serving files use the indexed Float32 Zstd v1 codec. Both codecs are part
of the current format. Unsupported storage layouts and staging types are
rejected.

A private hourly table passes complete validation before its spatial partitions
attach to the shared run table. A second full audit and staged samples precede
atomic manifest publication. Readers query only the manifest's validated hours;
unfinished attached partitions remain invisible. Published hours, codebooks and
source-validation proofs are immutable. Grid files use hash-addressed hard links.

The loader reads aligned UInt16 frames directly into tile words, with bounded
readers, buffers and queues. HRRR/RRFS use up to six workers, two readers and
four concurrent inserts; GFS/AIFS use one worker. Each insert response is drained
through late errors, followed by a database check of every index, array length,
revision and row count in that disjoint range. Final checks include every
ordered cell ID, physical types and bit-exact complete source samples. Error
paths drain every worker before cleanup.

Only the new unpublished table has merges paused, and successful resumption is
required. Independent validation scans every stored lane, including final-tile
padding, with bounded parallelism. Audits of published data use private
descriptor copies and replace validation evidence only after success. Encoded
source maps are removed only after staged arrays and checkpoints are durable.
After publication, weather staging is removed; serving codebooks, coordinates,
cloud files and validation evidence remain.

The database process ceiling is 12 GiB; each query is capped at 2 GiB. Periodic
allocator purges and consumed-page discard hints bound memory. Optional input
prefetch runs only with 18 GiB of headroom beyond its input; otherwise reads
stream normally.

## Catalog and API

The catalog protects two distinct complete initializations per model plus the
latest complete 48-hour HRRR run, counting overlap once. Partial runs do not
replace complete backups. Up to three recent partial runs and the newest
pending long HRRR run are tracked separately. Retired manifests have a ten-minute
grace period. GC follows all parent, hourly-book and shared-table references and
only drops explicitly owned tables inside `meteogram_native`.

Readers hold the shared catalog lock through their database query. Publication
and GC hold the exclusive lock; catalog replacement is atomic and fsynced.
Cached current and archive models remain resident while selected by the catalog.
Hourly codebooks are shared across manifest advances.

| Endpoint                                | Purpose                                                          |
| --------------------------------------- | ---------------------------------------------------------------- |
| `/health`                               | ClickHouse connectivity and freshness of all four current models |
| `/runs?model=MODEL`                     | Current, building, complete and HRRR long-run selections         |
| `/forecast?model=MODEL&lat=LAT&lon=LON` | Nearest retained native-cell profile                             |

`/forecast` accepts `run=latest` (the default), `latest_complete`,
`previous_complete` and HRRR `long`. Complete selectors use the complete archive
list and never substitute a partial run. Explicit archive selections report
freshness even if stale; the default rejects stale runs. Responses identify
initialization, source snapshots, available/expected/missing leads, completion,
selected grid cell, native units and wind frame. They support gzip and use
`Cache-Control: no-store`.

Point queries project the selected eight-lane word per slot and retain each
original full array length for shape validation. No request downloads source
maps. Malformed inputs return 400, outside-grid points 404 and unavailable
forecasts 503.

All three endpoints require bearer authentication when configured. Token lookup
uses `METEOGRAM_API_TOKEN`, then `METEOGRAM_API_TOKEN_FILE`, then `<root>/api-token`.
A present invalid file fails startup. Keep the token outside Git with mode 0600.
`verify`, `check-api` and `observe` load the same token automatically. For example:

```sh
curl --noproxy '*' --compressed \
  -H "Authorization: Bearer $(cat "$NATIVE_ROOT/api-token")" \
  'http://127.0.0.1:18080/forecast?model=hrrr&lat=41.013&lon=-74.027&run=long'
```

## HRRR native clouds

HRRR retains all 50 model-level cloud coverages exactly. Version 2 stores
coverage in Float32 palettes with constant levels and checksummed 256-cell
spatial frames (`cloud-coverage.bin.zst`/`cloud-coverage.json`). The separate
`cloud-anchors.stage.zst`/`.stage.json` retain two exact native Float32 fields:
level-1 pressure and level-50 geopotential height. Every stored fraction and
anchor is compared bit for bit during staged readback. Files follow the hourly
child's immutable metadata, source binding and retention; point reads check CRCs.

Layer coordinates are derived from the atmosphere already retained for each
point. NOAA's 51 HRRR interfaces and WRF hybrid weights, anchored by the native
bottom pressure and corrected using the pressure profile's moisture column,
determine layer pressures. Geopotential height interpolates in log pressure;
the native top height anchors the extension above the 275 hPa profile limit.
A separate native top-pressure map checks the configured 15 hPa model lid.
The API declares this geometry method; the site labels estimated coordinates.
Source acquisition therefore needs 53 maps rather than 150. Its task runs
alongside pressure extraction, sharing the same 16-map concurrency limit.
All tasks drain before an hour returns an error. Retry payloads are scoped to
the pinned cloud snapshot.

Cloud references may arrive in a newer Icechunk snapshot than pressure fields.
Both snapshots are recorded, with the same dataset, initialization, hour and grid.
Backfill missing supplements or compact existing retained runs with:

```sh
meteogram-native native-clouds --root "$NATIVE_ROOT" --run latest
meteogram-native native-clouds --root "$NATIVE_ROOT" --run latest_complete --leads 1,2
meteogram-native native-clouds --root "$NATIVE_ROOT" --run long
meteogram-native compact-clouds --root "$NATIVE_ROOT"
meteogram-native cloud-availability --product noaa-hrrr-forecast-18-hour-virtual
```

Backfill preserves all weather tables, hours, metadata, codebooks and proofs.
Compaction reads the original resident cloud arrays, compares every coverage
and anchor bit, creates new immutable descriptors, and atomically replaces all
retained catalog roles. Original descriptors keep the ten-minute retirement
grace period before GC. It also converts unpublished incoming hours under the
update lock. Rerunning skips already optimized supplements. Serving accepts
version 2; the former three-field reader exists only in the conversion tool.

The ignored full-CONUS benchmark is explicitly opt-in. Set
`METEOGRAM_CLOUD_COST_SOURCE` to colon-separated resident hourly descriptor paths
and `METEOGRAM_CLOUD_COST_OUTPUT` to a nonexistent directory, then run
`cargo test --release full_conus_live_source_cost_and_exactness -- --ignored --nocapture`
from `native/`. It primes only weather references, compares independent
pressure-only/integrated runs with fresh payloads and alternating order, checks
identical weather staging bytes and every cloud fraction/anchor, and compares
every reconstructed column with native coordinates. Its runtime has four
workers and the production 16-map limit. Run database integration tests with
`cargo test -- --include-ignored --skip full_conus_live_source_cost_and_exactness`
to avoid invoking this separate live benchmark.

## Site and tunnel

`weather-api.andyfang.app` reaches loopback 18080 through the named
`meteogram-weather-api` Cloudflare Tunnel. Credentials and configuration live
outside Git under `~/.local/share/meteogram-tunnel`. Its ingress only proxies the
API, with a 404 catchall; database and metrics listeners remain on loopback.
Use a hostname covered by the configured TLS certificate. Verify public HTTPS,
an unauthenticated 401 and authenticated forecasts for all four models before
configuring a new site deployment.

The production sites are `meteogram.vercel.app` and `meteogram.flyingclub.org`.
Their server-only settings are:

```text
METEOGRAM_API_URL=https://weather-api.andyfang.app
METEOGRAM_API_TOKEN=<same secret as the native service>
```

The browser receives normalized Next.js data and never receives the token.
HRRR overlays `latest` and `latest_complete` on the complete 48-hour `long` base;
other models overlay `latest` on `latest_complete`. Requests execute concurrently
with five-second limits. The merged native data must continuously cover at least
12 h from now for HRRR, 48 h for RRFS or 72 h for GFS/AIFS; otherwise the site
uses Open-Meteo. RRFS's fallback model is labeled GFS.

The adapter preserves native levels and cadence, rotates regional winds into
true bearings, and computes latitude-dependent WGS84 MSL heights. HRRR renders
50 native cloud layers when available; GFS uses native pressure-level cloud
fraction. RRFS/AIFS and HRRR hours without a supplement use explicitly labeled
humidity estimates. Missing layers or hours are not invented. Coarse intervals
can split at overlay boundaries while retaining their original sample time.
Run strips, handoff lines and tooltips identify each supplying initialization.
Open-Meteo responses without per-interval provenance label initialization
unavailable.

## Verification and diagnostics

```sh
cargo fmt --check --manifest-path native/Cargo.toml
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets -- -D warnings
# With the isolated loopback ClickHouse server running:
cargo test --locked --manifest-path native/Cargo.toml -- --include-ignored
meteogram-native check-api --root "$NATIVE_ROOT" --port 18082
meteogram-native verify --root "$NATIVE_ROOT" --base-url http://127.0.0.1:18080
```

`check-api` starts a temporary API and checks HTTP profiles against the database,
plus invalid/outside-grid inputs and gzip. `--full` additionally performs complete
database audits. `validate --data DESCRIPTOR --table TABLE --database DATABASE`
audits a published run or hourly descriptor; `--staged` also compares available
weather staging.

`inspect MODEL`, `availability --model MODEL`, `probe --model MODEL
--pin-metadata FILE` and `inspect-dictionary --data DESCRIPTOR` inspect current
source metadata or hourly codebooks. `observe` records bounded fixed-point
availability/content/resource observations. `benchmark` measures current
point-query latency; report its cache/concurrency/timing conditions with results.
These observations do not establish a long-term uptime or ingestion-latency SLA.
