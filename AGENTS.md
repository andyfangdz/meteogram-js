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

**Meteogram-js** is a Next.js 16 aviation weather visualization app. It fetches atmospheric forecast data from Open-Meteo and renders interactive meteograms (time-height cross-sections) showing clouds, temperature, wind, and pressure.

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
│       ├── cloud-columns.tsx           # Cloud coverage rectangles + wind barbs
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
│   ├── weather.ts                      # transformWeatherData function
│   ├── meteogram.ts                    # Grid interpolation, contour helpers
│   ├── params.ts                       # URL param serialization
│   └── serverPreferences.ts            # Server-side cookie/URL preference logic
└── __tests__/                          # Unit tests
```

### Data Flow

```
[Open-Meteo API]
       ↓
fetchWeatherDataAction (server action)
       ↓
transformWeatherData (utils/weather.ts)
       ↓
[CloudColumn[] with computed MSL heights]
       ↓
ClientWrapper (state holder, 60s refresh)
       ↓
Meteogram → CloudColumns, WeatherLines, etc.
```

### Key Abstractions

1. **ModelConfig** (`src/config/weather.ts`): Encapsulates per-model differences (time resolution, pressure levels, display parameters). Extend this class rather than branching in rendering code.

2. **CloudColumn/CloudCell** (`src/types/weather.ts`): The normalized data structure passed to visualization components. Each column is a timestep; each cell is a pressure level with cloud cover, temp, wind.

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
├── CloudColumns (cloud rectangles per cell)
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

The Next.js provider still uses Open-Meteo. The separate home-server native
service now lives in `native/`, with one Rust/Cargo application for source reads,
loading, validation, publication, retention, HTTP serving and supervision. Read
`native/README.md` for current commands. The former Python tools were retired;
commit `5f4ae08` and earlier commits preserve their historical implementations.
Do not put whole-map virtual Zarr reads in page requests: arbitrary CONUS
coordinates are served from the retained local grid.

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
| Pinned Icechunk manifest preflight, source reads, Zarr codecs, regional gathering | `native/src/source.rs` |
| Native spherical HRRR projection and nearest-cell resolution | `native/src/grid.rs` |
| Buffered parallel staging reads/Native insertion and validation/GC | `native/src/store.rs` |
| Bit-exact cache-tiled NEON/portable packing and Native block encoding | `native/src/pack.rs` |
| Async acquisition overlap, recovery, update and rollback | `native/src/pipeline.rs` |
| ClickHouse HTTP/RowBinary access and both physical readers | `native/src/db.rs` |
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
fsynced. Each candidate has a unique immutable table. Keep current/previous
revisions and a ten-minute retired grace period. GC drops only explicitly owned
tables; adoption creates an independent validated clone of its source table.

New revisions use `cell_profile_v1`: one physical row per native cell, full
pressure profiles arranged as level/lead arrays, surface profiles as lead arrays,
and exact Float32 bits in UInt32 arrays with T64 + Zstd (level 3 for HRRR,
level 1 for GFS/AIFS). Metadata explicitly
versions layout, axes, encoding and writer and is bound to its validation marker.
The API also reads legacy cell/lead tables, including rollback to them. Validation
compares ALL sorted cell IDs, physical types, array lengths and finite bits,
plus complete bit-exact staged samples. Keep physical cell rows distinct from
logical cell/lead rows in reports. Do not enable quantization or fast-math.

The updater pins source snapshots/initializations, verifies complete schema and
freshness, and tries the latest then the preceding initialization.
Before forecast-map downloads, all required chunk references are checked in the
pinned Icechunk manifests (16 concurrent checks); `preflight.json` records missing
references. This does not guarantee the referenced payload objects are readable;
preserve download/decode/full-data gates. At most two extractors run initially,
then one extractor overlaps one loader. Each extractor
has at most 16 map tasks. HRRR loading uses up to three reusable-buffer readers
and four insert workers connected by a bounded queue; other models default to
one of each. `load --workers 1..6` overrides insert concurrency for measurement.
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
The full scope, 116.36/118.03-second measured HRRR loads, compressed staging and
deployment evidence are in
`docs/plans/2026-10-18-hrrr-load-optimization.md`. Those load times include all
stored-value checks but exclude source acquisition/staging. Keep 4,096-cell
HRRR batches and the existing memory limits: an 8,192-cell/four-writer experiment
exceeded the 6 GiB server limit and was rejected.
Packing completes small cell tiles across all leads to reuse output cache lines.
Encoded maps are evicted only after both staged arrays are synced and the lead
checkpoint is durable. Resume prunes any already-checkpointed maps and retains
unfinished leads; do not require a complete encoded-map cache for recovery.
Admission reserves both full model budgets plus 20 GiB. Preserve those bounds.
Phase work times overlap; compare total elapsed times rather than adding work
counters. The public S3 configuration explicitly retains anonymous access for
STAC-authorized containers; do not let persisted S3 options enable host-credential
or EC2 metadata lookup.

HRRR native projection is spherical LCC as declared by its source. This is
separate from the app's WGS84 geopotential-to-MSL conversion. The API rejects
requested coordinates outside 24–50°N, 125–66°W before selection and permits only
micrometre-scale projection round-off at native bounds. HTTP requests never
start acquisition. `/health` checks freshness and database connectivity. Native
HRRR winds remain grid-relative. Meteorological normalization, cloud estimates,
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
