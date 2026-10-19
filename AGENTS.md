# AGENTS.md — AI Agent Guide for meteogram-js

This document provides comprehensive guidance for AI agents working on this codebase. It covers architecture, common workflows, critical constraints, and verification patterns.

> **Keep this document up to date.** When making structural changes to the codebase—adding files, renaming modules, changing data flow, modifying configuration patterns, or altering key abstractions—update this document to reflect those changes. Accurate documentation prevents future agents from making incorrect assumptions.

---

## Quick Reference

| Task | Key Files | Verification |
|------|-----------|--------------|
| Add weather model | `src/config/weather.ts` | `yarn ts && yarn dev` → test new model route |
| Add location | `src/config/weather.ts` | Navigate to new location URL |
| Add preference | `src/config/preferences.ts`, `src/utils/params.ts`, `src/context/PreferencesContext.tsx`, nav components | Cookie + URL sync test |
| Modify visualization | `src/app/components/meteogram.tsx` and children | Visual inspection + `yarn ts` |
| Change data transform | `src/utils/weather.ts` | Run `yarn test` (transform tests exist) |

---

## Project Overview

**Meteogram-js** is a Next.js 16 aviation weather visualization app. It fetches atmospheric forecast data from the optional native Dynamical/ClickHouse service with Open-Meteo fallback and renders interactive meteograms (time-height cross-sections) showing clouds, temperature, wind, and pressure.

### Tech Stack
- **Framework**: Next.js 16 (App Router, React 19)
- **UI Library**: HeroUI 2.8
- **Charts**: Visx 3.12 (D3-inspired React visualization)
- **Weather API**: Open-Meteo SDK
- **Styling**: Tailwind CSS 4
- **Testing**: Vitest + React Testing Library

---

## Architecture

### Directory Structure

```
src/
├── app/
│   ├── [location]/[model]/page.tsx    # Main dynamic route (server component)
│   ├── page.tsx                        # Root redirect
│   ├── layout.tsx                      # Root layout with providers
│   ├── actions/
│   │   ├── weather.ts                  # Server actions: fetchWeatherDataAction, getWeatherData
│   │   └── geocoding.ts                # Location resolution utilities
│   └── components/
│       ├── client-wrapper.tsx          # Client boundary, state management, auto-refresh
│       ├── meteogram.tsx               # Main visualization container
│       ├── cloud-columns.tsx           # Native/pressure cloud rectangles + wind barbs
│       ├── run-boundaries.tsx          # Run strips, handoff lines and initialization labels
│       ├── weather-lines.tsx           # Isotherms, isotachs, freezing level
│       ├── pressure-lines.tsx          # Constant-pressure surface lines
│       ├── nav.tsx, nav-wrapper.tsx    # Navigation bar with preference toggles
│       └── ...                         # Other UI components
├── config/
│   ├── weather.ts                      # MODEL_CONFIGS, LOCATIONS, pressure levels
│   └── preferences.ts                  # DEFAULT_PREFERENCES
├── context/
│   └── PreferencesContext.tsx          # Client-side preference state + persistence
├── hooks/
│   └── useMeteogramScales.ts           # Memoized Visx scales
├── types/
│   └── weather.ts                      # TypeScript interfaces
├── utils/
│   ├── weather.ts                      # Open-Meteo transform and shared column geometry
│   ├── native-weather.ts               # Validated native schema, units, humidity and wind adapter
│   ├── forecast-runs.ts                # Run overlays, native intervals and initialization handoffs
│   ├── meteogram.ts                    # Grid interpolation, contour helpers
│   ├── params.ts                       # URL param serialization
│   └── serverPreferences.ts            # Server-side cookie/URL preference logic
└── __tests__/                          # Unit tests
```

### Data Flow

```
[Native API over authenticated Cloudflare Tunnel, or Open-Meteo fallback]
       ↓
getWeatherData (server action; native first for supported CONUS routes)
       ↓
transformNativeForecast or transformWeatherData
       ↓
[CloudColumn[] with computed MSL heights]
       ↓
ClientWrapper (state holder, 60s refresh)
       ↓
Meteogram → CloudColumns, WeatherLines, etc.
```

### Key Abstractions

1. **ModelConfig** (`src/config/weather.ts`): Encapsulates per-model differences (time resolution, pressure levels, display parameters). Extend this class rather than branching in rendering code.

2. **CloudColumn/CloudCell** (`src/types/weather.ts`): The normalized data structure passed to visualization components. Each column is a native display interval with initialization provenance; each atmospheric cell is a pressure level with cloud cover, temp, wind. Optional native cloud layers retain independent model heights.

3. **PreferencesContext**: Client-side state for visualization toggles. Auto-syncs to cookies and URL query params.

---

## Critical Constraints

### 1. Index Synchronization in transformWeatherData

The transformation function uses hardcoded index offsets based on pressure level count:

```typescript
// In transformWeatherData, indices are calculated as:
// cloudCover: 0 to hpaLevels.length - 1
// geopotential: hpaLevels.length to 2*hpaLevels.length - 1
// temperature: 2*hpaLevels.length to 3*hpaLevels.length - 1
// etc.
```

**If you change `hpaLevels` in any ModelConfig, verify the index math still works.** Run `yarn test` to check transform tests.

### 2. Preference Parity (Cookie ↔ URL ↔ Context)

Adding a preference requires updates in **four places**:
1. `src/config/preferences.ts` — default value
2. `src/utils/params.ts` — URL key mapping and parse/serialize
3. `src/context/PreferencesContext.tsx` — state handling
4. `src/app/components/nav-wrapper.tsx` or `nav.tsx` — UI toggle

Missing any location breaks persistence or sharing.

### 3. Data Sorting Requirement

The Meteogram expects `weatherData` sorted by **descending pressure** (ascending altitude). The transform function handles this, but if you modify data flow, preserve this invariant.

### 4. Geopotential → MSL Conversion

Heights use the **WGS84 ellipsoidal Earth model** via the `wgs84` npm package. The conversion accounts for Earth's flattening and varies with latitude:

```typescript
// Uses WGS84 ellipsoid constants from the wgs84 package
const localRadius = getWGS84LocalRadius(latitude);
msl = (localRadius * geopotential) / (localRadius - geopotential);
```

This is **not** a simple scale factor and requires the location's latitude to be accurate. The `transformWeatherData` function in `src/utils/weather.ts` accepts latitude as a parameter for this conversion. Don't replace with a linear conversion or remove the latitude dependency.

### 5. Edge Compatibility

Server actions must use only standard Node APIs—no filesystem writes, no native bindings. The app deploys to Vercel Edge.

---

## Common Workflows

### Adding a New Weather Model

1. **Define the config** in `src/config/weather.ts`:
```typescript
export const MODEL_CONFIGS: Record<WeatherModel, ModelConfig> = {
  // existing models...
  new_model: new ModelConfig({
    varsKey: "hourly",           // or "minutely_15"
    stepKey: "forecast_hourly",  // or "forecast_minutely_15"
    stepSize: 168,               // forecast hours
    forecastDataKey: "hourly",   // or "minutely15"
    windBarbStep: 3,             // show barb every N timesteps
    windBarbPressureLevelStep: 4,
    maxIsothermStepDistance: 6,
    hpaLevels: DEFAULT_HPA_LEVELS, // or custom array
  }),
};
```

2. **Add to MODEL_NAMES** array in same file

3. **Update WeatherModel type** in `src/types/weather.ts`

4. **Verify**: `yarn ts && yarn dev` → navigate to `/KCDW/new_model`

### Adding a New Location

**Predefined location**:
```typescript
// In src/config/weather.ts
export const LOCATIONS: Record<string, { lat: number; lon: number }> = {
  // existing...
  KJFK: { lat: 40.6413, lon: -73.7781 },
};
```

**Custom location** (no code change needed):
Users navigate to `/{name}@{lat},{lon}/{model}`, e.g., `/MySpot@40.73,-73.42/gfs_hrrr`

### Adding a Visualization Preference

1. **Add to type and defaults**:
```typescript
// src/types/weather.ts
interface VisualizationPreferences {
  // existing...
  showNewFeature: boolean;
}

// src/config/preferences.ts
export const DEFAULT_PREFERENCES: VisualizationPreferences = {
  // existing...
  showNewFeature: false,
};
```

2. **Add URL serialization** in `src/utils/params.ts`:
```typescript
const URL_PARAM_KEYS = {
  // existing...
  showNewFeature: 'showNewFeature',
};
```

3. **Update context** in `src/context/PreferencesContext.tsx` (add to parse/serialize)

4. **Add UI toggle** in nav components

5. **Use in visualization**:
```typescript
const { preferences } = usePreferences();
if (preferences.showNewFeature) { /* render */ }
```

### Modifying Visualization Components

The component hierarchy:
```
Meteogram (SVG container, scales, dimensions)
├── CloudColumns (native cloud layers or pressure cloud rectangles)
├── RunBoundaries (supplying initializations and handoffs)
│   └── WindBarb (per filtered cell)
├── WeatherLines (isotherms, isotachs, freezing level)
├── PressureLines (constant-pressure paths)
├── TimeAxis (X-axis)
├── AxisLeft (Y-axis)
├── HoverIndicators (crosshair on mouse)
└── MeteogramTooltip (data display)
```

**Performance notes**:
- Components are wrapped in `React.memo`
- Scales are memoized via `useMeteogramScales`
- Pass stable callback references (use `useCallback`)
- Pre-filter data in `useMemo` before mapping

---

## Testing & Verification

### Commands

```bash
yarn dev          # Local dev server
yarn build        # Production build (catches more errors)
yarn ts           # TypeScript type check
yarn lint         # ESLint
yarn format       # Prettier check
yarn test         # Vitest unit tests
yarn test:watch   # Test watch mode
```

Native CI (`.github/workflows/native.yml`) checks formatting, Clippy with warnings
denied and unit tests on x86. Local ARM verification also exercises the NEON
path; use `cargo test -- --include-ignored` with the loopback ClickHouse service
for database integration coverage. Frontend CI lives in
`.github/workflows/tests.yml`. The legacy automatic and mention-triggered Claude
workflows have been retired.

### What to Verify

| Change Type | Verification Steps |
|-------------|-------------------|
| Data transformation | `yarn test` (transform tests) |
| Type changes | `yarn ts` |
| New route/model | Navigate to URL, check console for errors |
| Preference change | Toggle in UI, refresh page (cookie), share URL (query params) |
| Visualization change | Visual inspection at multiple locations/models |
| Server action | Check console logs (server + client), verify data loads |

### Test Files

- `src/__tests__/weather.transform.test.ts` — Data transformation
- `src/__tests__/params.test.ts` — URL param parsing
- `src/__tests__/server-actions.test.ts` — API orchestration

---

## Utility Reference

### Unit Conversions (src/utils/meteogram.ts)

```typescript
kmhToKnots(kmh)      // Wind speed
hPaToInHg(hpa)       // Pressure
FEET_PER_METER       // 3.28084
```

### Color Scales

```typescript
getTemperatureColor(tempC)  // Purple (-20°C) → Blue (0°C) → Red (30°C)
```

### Grid Interpolation

```typescript
createInterpolatedGrid(data, altitudeSteps)  // For contour line generation
findIsothermPoints(data, temperatures, maxGap)
findIsotachPoints(data, windSpeeds, maxGap)
```

---

## Common Pitfalls

### 1. Forgetting to Update MODEL_NAMES

Adding a model to `MODEL_CONFIGS` without adding to `MODEL_NAMES` array means the model won't be selectable.

### 2. Breaking Preference URL Compatibility

Changing URL param keys (in `params.ts`) breaks existing shared links. Add new keys; deprecate old ones gracefully if needed.

### 3. Non-Finite Value Crashes

The transform function filters `!isFinite()` values. If you add new data fields, apply the same guards.

### 4. Isotherm Line Gaps

If `maxIsothermStepDistance` is too small for a model's time resolution, contour lines will have gaps. Match this to the model's cadence.

### 5. Cookie Read Failures

Server-side cookie reading can fail on Edge. The `cookieReadSuccess` flag triggers client-side retry. Don't remove this fallback logic.

---

## Dynamical migration and native forecast service

The Next.js provider can use this native service with Open-Meteo fallback.
Current incremental operation and
verification are documented in `docs/plans/2026-10-18-incremental-ingestion.md`.
The separate home-server native
service now lives in `native/`, with one Rust/Cargo application for source reads,
loading, validation, publication, retention, HTTP serving and supervision. Read
`native/README.md` for current commands. The former Python tools were retired;
commit `5f4ae08` and earlier commits preserve their historical implementations.
Do not put whole-map virtual Zarr reads in page requests: arbitrary CONUS
coordinates are served from the retained local grid.

The native service supports HRRR, GFS, AIFS and RRFS. Default per-hour polling
includes all four; `update --models rrfs` refreshes only RRFS. Its Dynamical source
is `noaa-rrfs-forecast-84-hour-virtual`: 85 hourly leads through 84 hours,
1,638,757 retained CONUS cells, 30 pressure levels from 1000 down to 275 hPa,
the same six pressure fields and two surface fields as HRRR. RRFS and HRRR use
their source-declared spherical LCC grid and retain grid-relative winds.
RRFS freshness requires age ≤24 hours and at least 48 forecast hours remaining.
Complete source schema/preflight and all existing publication gates apply.
RRFS overrides the source's one-million-reference manifest cache with a bounded
eight-million-reference cache at the initial repository open, with weather
payload caching disabled. Icechunk 2.3 `reopen` shares its original asset manager;
changing cache settings only there does not rebuild the cache.
New RRFS runs use exact mapwise dictionaries and automatic 256-cell tiles when
all palettes fit UInt16. At the user's request, every model now has the same
bounded 250-million-entry codebook allowance (1,000,000,000 bytes of Float32
payload). Both source checkpoint assembly and readers enforce this aggregate
limit. Do not thin fields, levels, cells or times.
Its 8,192-cell packed batches keep 85-lead Native blocks below HRRR's measured
per-block byte budget. Admission reserves 160 GiB plus the shared 20 GiB floor.
`/health` requires all four models. Independent packed-table audits scan one
column at a time with two threads and one tile per block, including after
background merges, so 85-lead arrays stay under the 2 GiB query cap;
do not remove full palette/index/key checks to reduce memory. Complete first-run
evidence is in `docs/plans/2026-10-18-rrfs-ingestion.md`.
The RRFS ingestion report predates the subsequent site integration described below.

Historical evidence remains under `docs/plans/` and `docs/benchmarks/`:

- `2026-10-14-dynamical-migration.md`: field coverage and source chunk costs.
- `2026-10-17-clickhouse-full-volume-retry.md`: actual full-CONUS HRRR geography
  for two times; its eight disk-bounded batches were not a completed full-resident
  forecast benchmark.
- `2026-10-17-clickhouse-home-resident.md`: the completed three-model home ARM
  benchmark, 84,579,313 logical cell/lead rows in 33.01 GiB using the legacy layout.
  Preserve its distinction between measured data and operating-cost scenarios.
- `2026-10-17-native-pipeline-deployment.md`: actual publication/rollback/retention
  deployment, including the observed ingestion-time HTTP tail-latency limitation.
- `2026-10-17-native-ingestion-optimization.md`: 20m11s pinned full cycle.
- `2026-10-18-native-rust-optimization.md`: the earlier mixed-language hot-path
  optimization, 16m00s and 25.935 GB, plus the measured GFS query tradeoff.
- `2026-10-18-native-rust-service.md`: the full Rust migration, fresh full-cycle
  equivalence checks, legacy rollback, deployed supervisor and restart recovery.

These older command examples describe their dated implementations. The original
host handoff remains useful for historical context; it is not the current
runtime setup guide. Large source caches, staging, database files and binaries
stay outside Git. Original `meteogram_bench` tables are separate from service
ownership and must not be deleted by production retention.

### Rust module map

| Purpose | File |
|---------|------|
| CLI and operational commands | `native/src/main.rs` |
| Catalog, locks, freshness/schema policy, durable files, legacy NPY/NPZ decoding | `native/src/state.rs` |
| Indexed checksummed Zstd staging, compressed coordinate index, legacy conversion | `native/src/stage.rs` |
| Lossless per-field/level/lead palettes, bounded/versioned/hash-bound codebook I/O | `native/src/dictionary.rs` |
| UInt128 words packing eight indices, 256-cell Native tiles and complete lane/ID validation | `native/src/packed.rs` |
| Raw export, mapwise dictionary preparation and palette sorting (diagnostic/offline) | `native/src/prepare.rs` |
| Pinned Icechunk manifest preflight, source reads, Zarr codecs, regional gathering | `native/src/source.rs` |
| Native spherical HRRR projection and nearest-cell resolution | `native/src/grid.rs` |
| Buffered parallel staging reads/Native insertion and validation/GC | `native/src/store.rs` |
| Bit-exact cache-tiled NEON/portable packing and Native block encoding | `native/src/pack.rs` |
| Legacy complete-run benchmark acquisition, update and rollback | `native/src/pipeline.rs` |
| Fresh source availability polling, durable per-hour recovery and partition attachment | `native/src/incremental.rs` |
| Immutable per-hour manifests/books, shared-table reader and complete-run retention | `native/src/runs.rs` |
| HRRR model-level cloud storage, provenance, cloud-only upgrades and point reads | `native/src/clouds.rs` |
| ClickHouse HTTP/RowBinary access and legacy/raw-bit/dictionary/tile readers | `native/src/db.rs` |
| Loopback forecast API and health | `native/src/api.rs` |
| ClickHouse/API/updater supervision and persisted schedule | `native/src/service.rs` |
| HTTP/data verification, query/packing benchmarks, staging compression | `native/src/verify.rs` |
| Fixed-point availability/content and resource observations | `native/src/observe.rs` |
| Unit and real-database integration coverage | `native/src/tests.rs` |
| Loopback database limits/query-log TTL and user unit | `native/server.xml`, `native/systemd/meteogram-native.service` |

### Service invariants

State lives in `~/.local/share/meteogram-native`; service tables use
`meteogram_native`. One updater owns `update.lock` throughout recovery/builds.
Readers hold the shared catalog lock through their database query;
publication/GC hold the exclusive lock. Catalog replacement is atomic and
fsynced. The supervisor runs a persistent 60-second availability poller with
`watch.lock`; each poll holds `update.lock`. Keep the two latest COMPLETE runs
per model plus the latest complete 48-hour HRRR run, counting overlap once.
Partial runs never displace complete backups. Catalog fields `complete_runs`,
`building` and `hrrr_long` supplement the backward-compatible current/previous
aliases. Retired manifests have a ten-minute grace period. GC follows all
manifest/book/shared-table references and drops only explicitly owned tables.
Adoption creates an independent validated clone; `adopt --retain-only` archives
a complete run without selecting it as current.

New incremental acquisitions for all four models use `lead_partition_tiles_v1`:
one shared ClickHouse table per model/dataset/initialization, partitioned by
`(lead_hour, intDiv(tile_id,128))`. Each row holds 256 cells for one hour, with
exact UInt16 dictionaries. A private single-hour table passes full validation,
then attaches its spatial partitions to the run table. A second full check and
staged samples precede atomic publication of an immutable manifest. API queries
must filter by that manifest's validated leads, so unfinished attached partitions
cannot leak. Published hours never change. Child descriptors do not own the
shared table; parent manifests do. Child `source-validation.json` preserves the
original staged-source proof. Metadata hashes and source identities bind every
book. The 250-million-entry codebook allowance is aggregate across the whole
run and checked before allocation/publication. Coordinates share SHA-256 hard
links; API caches reuse decoded books across manifest advances.

Full audits of published data must preserve its existing validation marker until
success. `store::validate` holds a shared catalog lock, validates a private
hard-linked descriptor copy and atomically replaces the marker on success.
Never remove a published marker while an audit is running.

HRRR includes `noaa-hrrr-forecast-18-hour-virtual` for hourly short runs and the
48-hour product for 00/06/12/18 UTC. Those six-hour cycles must satisfy the full
49-lead schema before counting as complete. `/forecast` defaults to the newest
validated partial run and exposes completion/available/missing leads and per-hour
source snapshots. Explicit `run=latest_complete`, `previous_complete` or `long`
selects archives and reports their freshness even if stale. `/runs?model=MODEL` lists these
retention roles; `/health` reports backup counts and long-run retention.

Raw-bit revisions use `cell_profile_v1`: one physical row per native cell, full
pressure profiles arranged as level/lead arrays, surface profiles as lead arrays,
and exact Float32 bits in UInt32 arrays with T64 + Zstd (level 3 for HRRR,
level 1 for GFS/AIFS). New HRRR/RRFS acquisitions use the lossless dictionary/tile
format detailed below. Metadata explicitly
versions layout, axes, encoding and writer and is bound to its validation marker.
The API also reads legacy cell/lead tables, including rollback to them. Validation
compares ALL sorted cell IDs, physical types, array lengths and finite bits,
plus complete bit-exact staged samples. Keep physical cell rows distinct from
logical cell/lead rows in reports. Do not enable quantization or fast-math.

The normal updater checks fresh immutable Icechunk snapshots every poll, with
repository manifest caches retained across polls. Every required pressure and
surface map must be referenced before an hour is queued; missing hours do not
block other ready hours. Availability never downloads whole-map payloads.
Each hour pins its own snapshot and initialization, fully validates all retained
cells, then publishes independently. Preserve download/decode/full-data gates:
references alone do not prove payload availability. Completed hours are skipped;
private `incoming/state` journals and staging permit retry after interruption.
Incremental concurrency allows two regional (HRRR/RRFS) extractors plus one global
(GFS/AIFS), one hour at a time per model, with at most 16 map tasks per extractor.
The server budget is 12 GiB for concurrent incremental work; the per-query cap
remains 2 GiB.
`update --batch` or `--pin-root` explicitly selects the old whole-run benchmark
workflow and its admission budgets; it is no longer the service default. HRRR/RRFS loading uses three raw-bit or two packed-tile
reusable-buffer readers and four raw-bit or six packed-tile load workers
connected by a bounded
queue; other models default to
one of each. `load --workers 1..6` overrides insert concurrency for measurement.
Packed workers share four insert permits; validation overlaps inserts without
holding a permit. Keep permits RAII-scoped so errors and panics release them.
New staging is indexed, byte-shuffled Float32 in independent checksummed Zstd
frames (`*.stage.zst` + versioned JSON indexes); new coordinates use
`cells.bin.zst`. `stage.rs` retains legacy NPY/NPZ readers for old snapshots.
Do not introduce new NPY/NPZ production writes. `convert-stage` prepares a fresh
compressed directory without modifying its source. Preserve all float bits,
shape/type checks, bounded decoding, frame checksum verification and partial
final blocks. Consumed input pages receive a Linux discard hint. Merges are
paused only for the new unpublished table and must resume before load success.
Every worker is joined before any load error returns, so cleanup
cannot race pending inserts. Insert responses are drained with
`wait_end_of_query=1` before a batch counts as completed.
Each batch additionally validates every stored value, array length, revision
and expected row count in its disjoint cell-ID range before completion. Final
global row/key/type and complete sampled bit comparisons remain mandatory.
The ten source samples are retained from the immutable ingestion buffers.
Only the loader's private in-memory proof of all validated ranges avoids a
duplicate whole-table scan; standalone `validate` always performs that scan.
Do not replace range checks with source-only checks, sampling, persisted flags,
or a row-count-only gate. Failed attempts clear stale validation markers.
The one-minute optimization work adds experimental `float32_dictionary_v1`:
UInt16 spatial indices when every palette fits (UInt32 otherwise), plus exact
Float32 palettes for each field/level/lead. UInt16 encoding checks every word
before narrowing. The metadata's `storage.index_type` controls physical decoding;
an absent key means the original UInt32 index representation.
There is no quantization. Palettes must be versioned, checksummed, hash-bound to
metadata, dimension-checked and finite; every stored index must be checked
against its own slot's palette length. Profile/API readers decode the palettes
and retain legacy Float32-bit support. Preserve both validations: finite palette
values and bounds of **all** database indices. HRRR acquisition checkpoints a
per-lead palette alongside durable arrays before source-cache eviction; a final
codebook remains required serving data and must survive staging cleanup.
`prepare-store` exports all configured cells/fields/times from an immutable local
source table into fresh raw/dictionary staging for controlled comparisons; its
preparation time must not be reported as loader performance. This prototype's
performance claims require complete measured loads; acquisition and preparation
remain outside the load clock.
`prepare-store --raw-only` exports the complete exact Float32 input without
constructing palettes, and conflicts with `--raw-output`. It retains source
coverage/shape/finite checks. The legacy all-slots `FieldBuilder` retains its
32-million-entry **per-field hash-table** budget; the original full 06Z GFS
export exceeded it. Preserve that failed measurement as historical evidence.
`encode-stage` is the bounded raw-staging-to-dictionary path: one spatial map's
lookup table at a time, sorted exact Float32 palettes, UInt16 v2 frames, no
quantization. Its raw/output lead matrices are each capped at 1 GiB and cell
count at two million; reject oversized inputs rather than thinning axes/cells.
Each palette must fit UInt16. Final codebooks/readers use a separate uniform
250-million-entry limit (1 GB decimal Float32 payload) and 65,536 total palette
slots for every model.
Check these limits and overflow **before reader allocation**. This replaces
the reader's old per-field accounting while retaining bounded compact storage;
do not restore all-lead lookup tables or remove the budgets. Version/checksum/
hash/finite checks and exact per-slot database bounds remain mandatory.
Full GFS codebooks above the old field cap require the new reader before
publication; the format tag stays `MGDICT01`. Explicit preparation/load commands
remain available for whole-run experiments; incremental acquisition uses per-hour
dictionaries for all four models.
`sort-stage` losslessly sorts palettes and remaps staged indices for experiments.
Direct HRRR acquisition sorts each map's palette in Float32 total order. Typed
UInt16 arrays use indexed staging version 2: spatial slot-major wrapping deltas,
byte shuffle, and checksummed Zstd blocks. Readers reconstruct exact UInt32 codes
and still accept version 1 Float32/UInt32 staging. Keep partial-block, random-read,
delta-wrap, overflow, checksum and legacy-format checks when changing this codec.
For UInt16 inputs of at most 8 GiB, the loader can sequentially prefetch the
immutable weather files **inside the load clock** before starting DB writes.
This requires `MemAvailable >= input bytes + 18 GiB` for the current 12 GiB
database cap, 4 GiB client budget and 2 GiB margin; otherwise it streams normally.
Every frame still receives CRC/shape/decode checks and every stored index is
validated. Consumed pages are discarded. Legacy UInt16 profile tables use T64/Zstd level 1;
raw-bit HRRR retains T64/Zstd level 3. Preserve the measured memory guards.
The current HRRR UInt16 experiment uses `cell_tiles_v1`: 256 adjacent source
rows are packed as 32 UInt128 words per field/level/lead slot, each containing
eight exact little-endian UInt16 indices. `tile_id` is the source-row ordinal
divided by 256; ordered
`cell_ids Array(UInt32)` preserves and verifies every original grid ID.
Arrays retain the level/lead slot order, and independent DB lane checks compare
**all eight indices** with that slot's palette bound. A guarded broadword
comparison is used for bounds <=32768; the complete UInt16 domain has a checked
fallback. `Delta(2)/Zstd(1)` compresses the UInt16 lanes beneath the UInt128 type.
The final incomplete tile pads unused weather lanes with valid code zero;
those lanes are checked too, but are not exposed as cells. Physical tile-row
counts must not be reported as cell counts. Keep full ordered-ID checks,
single-cell API decoding, all-lane corruption tests and legacy profile readers.
Readers also accept the earlier 8- and 64-cell tile formats. Physical array widths
include all words per slot; checks must cover every group. Full bound queries
use an explicit 2 MiB SQL limit.
Point queries project one eight-lane UInt128 word per slot in ClickHouse before
returning RowBinary, rather than transferring an entire tile. They also return
each original array length; verify it against the full stored width before
decoding the requested lane. Preserve this check even when a malformed unused
word group would not change the projected profile length. Decode dictionary
profiles in slot order before building time rows, retaining exact index bounds.
When every input is UInt16 staging v2, the tile loader decodes complete aligned
frames directly to their `[block][slot][cell]` UInt16 layout and copies adjacent
cells into UInt128 words. It does not widen or transpose the whole forecast.
Legacy inputs retain the normalized-buffer path. Both paths share checksum and
bounded decode gates; their Native bytes and ingestion-buffer source samples
must remain identical. Palette broadcasts are compact binary constants
(`base64Decode`/`reinterpret`), expanded across word groups by a constant
`arrayWithConstant` expression in lane checks, so there is no UInt128
multiplication per stored word and full HRRR queries fit the SQL parser limit.
Exact palette bounds remain unchanged.
The full scope, 116.36/118.03-second measured HRRR loads, compressed staging and
deployment evidence are in
`docs/plans/2026-10-18-hrrr-load-optimization.md`. Those load times include all
stored-value checks but exclude source acquisition/staging. Raw-bit HRRR loads
retain 4,096-cell batches; HRRR packed loads use 16,384-cell batches and two
readers/queued blocks; non-tiled HRRR UInt16 loads use 8,192-cell batches.
`load --packed` opts additional UInt16 models into 256-cell tiles in standalone
loads; legacy batch GFS/AIFS defaults use raw bits and one worker. Incremental
acquisition uses exact per-hour dictionaries and 256-cell tiles for all models.
Experimental packed GFS uses 4,096 cells to align with staging frames; raw GFS
retains 2,048 and AIFS retains 4,096. Keep the default HRRR behavior unchanged.
Packed loads purge unused ClickHouse allocator pages initially and after each
four validated batches, as well as before final validation. This keeps
background logging bounded; all purge time is included in the load clock.
Keep these releases with the current 12 GiB server budget. The earlier raw-bit
8,192-cell/four-writer experiment exceeded the then-current 6 GiB server limit
and was rejected. The server budget was subsequently raised with user approval
for concurrent incremental ingestion; the 2 GiB query cap remains unchanged.
The subsequent full 12Z HRRR loads finished in 50.21 and 53.70 seconds with the
same frozen binary, all validation included, using the lossless 256-cell layout
and four active-insert permits. Evidence and installation details are in
`docs/plans/2026-10-18-hrrr-one-minute-load.md`. These are complete staged-load
measurements, not complete network-refresh timings. Preserve this distinction.
The isolated GFS/AIFS measurements are in
`docs/plans/2026-10-18-other-model-load-benchmarks.md`: three complete fresh-table
loads per configuration, all load gates inside the clock, plus independent
audits and warm complete-profile queries. Four workers improved median GFS
16.07→6.33 seconds and raw AIFS 1.60→0.75 seconds. AIFS 256-cell tiles saved
40.8% including their required codebook but increased warm query p95
5.75→10.24 ms. GFS dictionary preparation hit the unchanged palette budget;
no full GFS dictionary/tile timing was measured. No binary/default changes were
deployed for that benchmark.
The subsequent `docs/plans/2026-10-18-gfs-dictionary.md` records successful full
GFS conversion through `encode-stage` (310 MiB peak encoder RSS), complete
638,598,870-value roundtrip proofs, and new reader measurements. Dictionary
profiles use 863,886,648 bytes including their book (43.9% below raw), with
10.19 ms warm query p95 versus 8.85 ms raw. Packed tiles use 770,938,895 bytes
but 27.03 ms p95 even after projection. Keep those tradeoffs separate from the
earlier failed all-slots-builder experiment. No service defaults were switched.
Packing completes small cell tiles across all leads to reuse output cache lines.
Encoded maps are evicted only after both staged arrays are synced and the lead
checkpoint is durable. Resume prunes any already-checkpointed maps and retains
unfinished leads; do not require a complete encoded-map cache for recovery.
Admission reserves both full model budgets plus 20 GiB. Preserve those bounds.
Phase work times overlap; compare total elapsed times rather than adding work
counters. The public S3 configuration explicitly retains anonymous access for
STAC-authorized containers; do not let persisted S3 options enable host-credential
or EC2 metadata lookup.

HRRR/RRFS native projection is spherical LCC as declared by the source. This is
separate from the app's WGS84 geopotential-to-MSL conversion. The API rejects
requested coordinates outside 24–50°N, 125–66°W before selection and permits only
micrometre-scale projection round-off at native bounds. HTTP requests never
start acquisition. `/health` checks freshness and database connectivity. Native
HRRR/RRFS winds remain grid-relative. Meteorological normalization, cloud estimates,
terrain handling, fallback and public serving remain separate app integration
work.

Build/check with `cargo build --release --locked --manifest-path native/Cargo.toml`,
`cargo test --locked --manifest-path native/Cargo.toml`, `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings` (the latter two with the same manifest).
Include ignored integration tests while the isolated database is running.
`verify`, `compare-stores`, `benchmark` and `observe` provide current Rust
verification commands. An isolated build's availability observations do not
establish live publication-lock latency or a WAN service SLA. No Python runtime,
virtual environment, external packing shared library or runtime compiler is used.

The subsequent packing, source-preflight and temporary-cache measurements are in
`docs/plans/2026-10-18-native-ingestion-optimization.md`. Keep resident kernel
timings distinct from complete load and refresh times. Cache eviction reduces
temporary retention; it does not avoid the initial map download/cache write.

### Native site provider and tunnel

`getWeatherData` reads server-only `METEOGRAM_API_URL` and
`METEOGRAM_API_TOKEN`. `ModelConfig.nativeModel` maps `best_match`/`gfs_hrrr`
to HRRR, `gfs_seamless` to GFS, `ecmwf_aifs025_single` to AIFS and `rrfs` to
RRFS. Other routes and coordinates outside the retained CONUS region keep
Open-Meteo. `fallbackModel` gives RRFS an explicitly labeled GFS backup.
Native fetches use HTTPS (loopback HTTP for development), bearer authentication,
no redirects/cache and a five-second timeout. Invalid schemas, units, freshness
or usable horizon fall back to Open-Meteo; source metadata reaches the client,
credentials do not. Elevation is an independent bounded Open-Meteo request.

`src/utils/native-weather.ts` validates exact model field/pressure grids and
expected lead sets before normalization. It preserves missing hours, native
AIFS levels and six-hour cadence, hourly regional data and the GFS three-hour
tail. `CloudColumn.durationMinutes` controls actual cell widths and the final
time-domain extent. Do not synthesize missing layers/hours. The adapter uses
the existing WGS84 MSL conversion and shared `completeCloudColumn` geometry,
rotates grid-relative HRRR/RRFS winds using the API's Lambert convergence,
and converts m/s to the app's km/h. GFS has native pressure-level cloud fraction. New HRRR hours also ingest all
50 native model-level cloud fractions with their actual geopotential heights
and pressures, independently of the 30 atmospheric pressure levels. RRFS/AIFS
and HRRR hours without a supplement use labeled humidity cloud estimates. AIFS specific humidity uses
Murphy–Koop saturation and ECMWF mixed phase; GFS dew point is derived.

HRRR cloud supplements are immutable `native-clouds.stage.zst`/`.stage.json`
files in each hourly child descriptor, with cell-major shape `[cells,50,3]`
(fraction 0..1, geopotential metres, pressure Pa). They use the existing indexed,
checksummed Float32 staging codec as durable serving data. Full finite/range/
monotonic-height/pressure checks and bit-exact staged roundtrip precede publication.
The index hash, file length, source snapshot, initialized hour and exact grid are
bound to the child's metadata hash. Point reads validate frame CRCs. These files
must survive staging cleanup and follow their child's catalog retention/GC.
Cloud references can arrive after pressure references; cloud acquisition pins
its own Icechunk snapshot for the same dataset/init/lead/grid and records
`pressure_snapshot_id` separately. Do not combine different initialized runs.
`native-clouds --root ROOT --run latest|latest_complete|long [--leads 1,2]` backfills
existing incremental hours through new child and parent revisions. Its strict
cloud-only publication gate preserves the full pressure manifest, table,
codebook and proofs; it never changes a published pressure hour or replaces an
existing cloud supplement. Normal incremental publication still must advance
hours and preserve previous books. `cloud-availability --product PRODUCT`
is a bounded metadata-only diagnostic over the latest 24 runs.

`CloudColumn.nativeCloudLayers` carries 50 separate layers. Native shading uses
midpoints between actual model heights and stops at the outer model centres;
it is clipped to the chart's altitude range. Time steps remain hourly. Pressure
profiles, wind barbs and thermodynamics remain on their original levels. The
hover state carries native cloud and atmospheric pressure cells separately;
the tooltip explicitly labels the pressure level used for temperature and wind.
The source badge identifies 50 native cloud levels and any mixed hours still
using humidity estimates. Cloud schema/provenance/rendering coverage lives in
`native-cloud-rendering.test.tsx`, `native-weather.test.ts` and `clouds.rs` tests.

Native page requests fetch authenticated profiles concurrently. HRRR uses
`run=long` as the complete 48-hour base, `latest_complete` to retain the previous
complete short run while a new one arrives, and `latest` as the newest overlay.
GFS, AIFS and RRFS use `latest_complete` and `latest`. Each response is validated
independently, including its initialization and cloud provenance. Sparse latest
runs are allowed during normalization because a complete base supplies the
remaining horizon. After merging the surviving responses, the provider requires
continuous coverage from the current time for at least 12 hours (HRRR),
48 hours (RRFS) or 72 hours (GFS/AIFS), otherwise it falls back to Open-Meteo.
The API retains lazily loaded current, previous, complete and long-run models
in its cache until their revision leaves those catalog selections.
`src/utils/forecast-runs.ts` selects the newest initialization
for each native interval, fills missing intervals from the base and preserves
native sample values/cadence. Coarse intervals can be split at overlay edges;
`sampleDate` retains their original sample time. The client restores both `date`
and optional `sampleDate` after the initial JSON serialization. No weather values
are interpolated. `initial-weather.test.tsx` covers the initial tooltip round trip.
Same-initialization responses are deduplicated with the longest horizon retained.
`CloudColumn.initializedAt` identifies every displayed interval. The generic
`run-boundaries.tsx` component marks every change with a colored run strip and
vertical line, using non-overlapping labels; tooltips include initialization and
any original coarse sample time. All four native models use this path. Standard
Open-Meteo forecast responses lack per-interval run provenance, so those routes
are explicitly labeled unavailable. Do not infer it from `generationtime_ms`
or the model-updates API. Overlay/provider/marker tests are in
`forecast-runs.test.tsx` and `native-provider.test.ts`. Measured cloud ingestion,
point latency and browser evidence are in
`docs/plans/2026-10-18-hrrr-native-clouds-and-runs.md`.

The source label shows provider, actual model, UTC initialization, partial-run
progress and cloud estimation. The route keys `ClientWrapper` by location/model
to prevent a prior route's refresh from changing the new route's data.
Tests live in `native-weather.test.ts` and `native-provider.test.ts`, with a
native response fixture in `__helpers__/nativeForecast.ts`.

The Rust API reads the token from `METEOGRAM_API_TOKEN`, an explicit token file,
or `<root>/api-token`. A configured invalid file fails startup. All three routes
(`/health`, `/runs`, `/forecast`) require bearer auth when configured; native
verification clients load that same token. API wind rotation comes from the
selected grid cell's actual source projection, never a hardcoded HRRR angle.

The active public API hostname is `weather-api.andyfang.app`.
The production domains `meteogram.vercel.app` and `meteogram.flyingclub.org`
serve native HRRR/GFS/AIFS/RRFS through that endpoint. Both home units now run
under `systemctl --user` and are enabled, with user lingering enabled. Use
`systemctl --user stop` to stop a managed service; the native CLI stop request
is for direct supervisors and a managed unit's Restart=always would restart it.
Production browser/service evidence is recorded in
`docs/plans/2026-10-18-native-site-integration.md`.
Tunnel credentials/config live outside Git under
`~/.local/share/meteogram-tunnel`; ingress only proxies loopback 18080, with a
404 catchall. Database and metrics listeners remain local. The user units are
`native/systemd/meteogram-{native,tunnel}.service`. Validate public TLS, a 401
without auth, authenticated forecasts, and actual Vercel charts before calling
the integration deployed. A direct process alone is not proof of reboot
persistence. See `native/README.md` for setup and certificate limitations.

## Development Tips

1. **Use `yarn dev` liberally** — Hot reload catches many issues early

2. **Check browser console** — Server action errors surface there

3. **Test multiple models** — Different models have different time resolutions and pressure levels

4. **Test custom locations** — The `Name@lat,lon` format exercises different code paths than predefined locations

5. **Inspect network tab** — Verify Open-Meteo requests look correct when changing data fetching

---

## File Quick Reference

| Purpose | File(s) |
|---------|---------|
| Main page component | `src/app/[location]/[model]/page.tsx` |
| Weather API calls | `src/app/actions/weather.ts` |
| Data transformation | `src/utils/weather.ts` |
| Model/location config | `src/config/weather.ts` |
| Preference defaults | `src/config/preferences.ts` |
| Preference state | `src/context/PreferencesContext.tsx` |
| URL param handling | `src/utils/params.ts` |
| Main visualization | `src/app/components/meteogram.tsx` |
| Cloud rendering | `src/app/components/cloud-columns.tsx` |
| Contour lines | `src/app/components/weather-lines.tsx` |
| Type definitions | `src/types/weather.ts` |
| Tests | `src/__tests__/*.test.ts` |
