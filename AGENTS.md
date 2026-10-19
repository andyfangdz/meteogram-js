# AGENTS.md — AI Agent Guide for meteogram-js

This document provides comprehensive guidance for AI agents working on this codebase. It covers architecture, common workflows, critical constraints, and verification patterns.

> **Keep this document up to date.** When making structural changes to the codebase—adding files, renaming modules, changing data flow, modifying configuration patterns, or altering key abstractions—update this document to reflect those changes. Accurate documentation prevents future agents from making incorrect assumptions.

---

## Quick Reference

| Task                  | Key Files                                                                                                | Verification                                 |
| --------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| Add weather model     | `src/config/weather.ts`                                                                                  | `yarn ts && yarn dev` → test new model route |
| Add location          | `src/config/weather.ts`                                                                                  | Navigate to new location URL                 |
| Add preference        | `src/config/preferences.ts`, `src/utils/params.ts`, `src/context/PreferencesContext.tsx`, nav components | Cookie + URL sync test                       |
| Modify visualization  | `src/app/components/meteogram.tsx` and children                                                          | Visual inspection + `yarn ts`                |
| Change data transform | `src/utils/weather.ts`                                                                                   | Run `yarn test` (transform tests exist)      |

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

2. **CloudColumn/CloudCell** (`src/types/weather.ts`): The normalized data structure passed to visualization components. Each column is a native display interval with initialization provenance; each atmospheric cell is a pressure level with cloud cover, temp, wind. Optional native cloud layers retain all 50 source coverages with explicitly derived layer geometry.

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
    varsKey: "hourly", // or "minutely_15"
    stepKey: "forecast_hourly", // or "forecast_minutely_15"
    stepSize: 168, // forecast hours
    forecastDataKey: "hourly", // or "minutely15"
    windBarbStep: 3, // show barb every N timesteps
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
  showNewFeature: "showNewFeature",
};
```

3. **Update context** in `src/context/PreferencesContext.tsx` (add to parse/serialize)

4. **Add UI toggle** in nav components

5. **Use in visualization**:

```typescript
const { preferences } = usePreferences();
if (preferences.showNewFeature) {
  /* render */
}
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
path; use `cargo test -- --include-ignored --skip full_conus_live_source_cost_and_exactness`
with the loopback ClickHouse service for database integration coverage. The separate
ignored full-CONUS cloud benchmark requires explicit resident source paths and a
fresh output directory; see `native/README.md`. Frontend CI lives in
`.github/workflows/tests.yml`.

### What to Verify

| Change Type          | Verification Steps                                            |
| -------------------- | ------------------------------------------------------------- |
| Data transformation  | `yarn test` (transform tests)                                 |
| Type changes         | `yarn ts`                                                     |
| New route/model      | Navigate to URL, check console for errors                     |
| Preference change    | Toggle in UI, refresh page (cookie), share URL (query params) |
| Visualization change | Visual inspection at multiple locations/models                |
| Server action        | Check console logs (server + client), verify data loads       |

### Test Files

- `src/__tests__/weather.transform.test.ts` — Data transformation
- `src/__tests__/params.test.ts` — URL param parsing
- `src/__tests__/server-actions.test.ts` — API orchestration

---

## Utility Reference

### Unit Conversions (src/utils/meteogram.ts)

```typescript
kmhToKnots(kmh); // Wind speed
hPaToInHg(hpa); // Pressure
FEET_PER_METER; // 3.28084
```

### Color Scales

```typescript
getTemperatureColor(tempC); // Purple (-20°C) → Blue (0°C) → Red (30°C)
```

### Grid Interpolation

```typescript
createInterpolatedGrid(data, altitudeSteps); // For contour line generation
findIsothermPoints(data, temperatures, maxGap);
findIsotachPoints(data, windSpeeds, maxGap);
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

## Native forecast service

The deployed site uses the Rust service in `native/` for HRRR, GFS, AIFS and
RRFS within CONUS, with Open-Meteo fallback. Read `native/README.md` for current
setup and operating commands. `docs/README.md` separates current guides from
dated investigation and rollout evidence. Historical commands and performance
measurements describe their recorded versions.

### Rust module map

| Purpose                                                                          | File                                   |
| -------------------------------------------------------------------------------- | -------------------------------------- |
| CLI and operational commands                                                     | `native/src/main.rs`                   |
| Catalog types, locks, source schema/freshness, durable files and disk guards     | `native/src/state.rs`                  |
| Checksummed Zstd weather/cloud staging and compressed coordinate indexes         | `native/src/stage.rs`                  |
| Exact Float32 palettes, sorted spatial encoding and bounded codebook I/O         | `native/src/dictionary.rs`             |
| UInt16 packing into 256-cell tiles, Native wire encoding and full lane/ID checks | `native/src/packed.rs`                 |
| Pinned Icechunk reads, GRIB decoding, reference preflight and regional gathering | `native/src/source.rs`                 |
| Source-declared spherical Lambert projection and nearest-cell resolution         | `native/src/grid.rs`                   |
| Bounded hourly ingestion, full database validation and ownership-aware GC        | `native/src/store.rs`                  |
| Availability polling, durable hourly recovery and partition attachment           | `native/src/incremental.rs`            |
| Immutable manifests/books, retention, publication, rollback and status           | `native/src/runs.rs`                   |
| HRRR model-level clouds, provenance, cloud-only upgrades and point reads         | `native/src/clouds.rs`                 |
| Exact cloud-coverage palettes, constant levels and checked spatial frames        | `native/src/cloud_coverage.rs`         |
| Derived hybrid-pressure/log-height cloud coordinates and native anchors          | `native/src/cloud_geometry.rs`         |
| Fully compared resident-cloud conversion and immutable catalog replacement       | `native/src/cloud_compaction.rs`       |
| Loopback ClickHouse HTTP/RowBinary access and hourly profile decoding            | `native/src/db.rs`                     |
| Authenticated point API, catalog selections, model/book caches and health        | `native/src/api.rs`                    |
| ClickHouse/API/updater supervision and persisted schedule                        | `native/src/service.rs`                |
| HTTP/database verification and point-query measurements                          | `native/src/verify.rs`                 |
| Fixed-point availability/content/resource observations                           | `native/src/observe.rs`                |
| Unit and real-database integration coverage                                      | `native/src/tests.rs`                  |
| Database limits/query-log TTL and user units                                     | `native/server.xml`, `native/systemd/` |

### Ingestion and publication

One persistent 60-second poller ingests individual forecast hours. `watch.lock`
prevents duplicate pollers; `update.lock` covers each poll's recovery, source
reads, loading and publication. Each hour pins an immutable Icechunk snapshot.
All required pressure/surface references must exist before downloading maps,
and source shape, initialization, grid and every gathered value are
validated. Native units are retained for the site adapter’s schema validation. A new location request never downloads source maps.

HRRR polls the hourly 18-hour product (19 leads), with the 48-hour product
(49 leads) for 00/06/12/18 UTC cycles. RRFS retains 85 hourly leads through
84 hours. GFS has hourly leads through 120 hours then three-hour leads through
180; AIFS has six-hour leads through 180. Retain every configured CONUS cell,
field, pressure level and forecast hour. Do not thin data to fit a budget.

HRRR/RRFS use their source-declared spherical Lambert grid and retain native
grid-relative winds. This projection is separate from the site's WGS84 height
conversion. HRRR and RRFS open their repositories with bounded eight-million-reference
manifest caches and weather payload caching disabled. Configure it at the first
repository open: Icechunk `reopen` shares the original asset manager.

Two regional extractors and one global extractor may run concurrently, with one
hour per model and at most 16 map reads per extractor. The work budget stops
starting more hours; active hours drain before releasing the update lock.
Durable journals and checkpoints resume interrupted hours. An unpublished
partition can be retried without changing any already published hour.

Every model uses `lead_partition_tiles_v1`: one shared table per
model/dataset/initialization, partitioned by
`(lead_hour, intDiv(tile_id,128))`. Each tile contains 256 adjacent native cells
for exactly one forecast hour. A private hourly table passes full validation
before its spatial partitions attach to the shared table. Another complete
check and staged samples precede atomic manifest publication. Readers query
only the manifest's validated hours, so unfinished partitions remain invisible.

### Storage and validation invariants

Exact Float32 palettes encode weather values with UInt16 indices; every palette
must fit 65,536 entries. The codebook allowance is 250 million entries per run
(1,000,000,000 Float32 payload bytes), with 65,536 metadata slots. Enforce those
limits and arithmetic overflow before allocation. Preserve signed zero and all
finite Float32 bits; there is no quantization. Codebooks are versioned,
checksummed and SHA-256-bound to metadata. They are required serving data.

Hourly child descriptors use `cell_tiles_v1` with `partitioned_lead=true` and
one matching `lead_hour`; parent manifests use `lead_partition_tiles_v1`.
Each UInt128 holds eight little-endian UInt16 indices. Keep the original ordered
cell IDs, validate every lane against its own palette bound, and validate the padding
lanes in the final short tile. Physical tile rows are distinct from logical
cells and cell/hour rows. Only this hourly format is accepted.

Weather staging uses indexed Zstd v2 frames with UInt16 spatial deltas and byte
shuffling. Cloud fractions use their own exact palette/spatial-frame codec;
cloud anchors use indexed Float32 Zstd v1 frames. Coordinates use checksummed
`cells.bin.zst` with original UInt32 IDs and Float64 coordinates. The UInt16 and
Float32 staging codecs are both current: do not remove the anchor codec when
working on weather indices. Preserve frame CRCs, shape/type/length checks,
partial-frame reads, delta wrapping and overflow rejection. Encoded source maps
are evicted only after staged files and their checkpoints are synced.

The hourly loader reads aligned UInt16 frames directly into tile words, using
bounded readers, buffers and an insert queue. HRRR/RRFS use up to six workers,
two readers and four concurrent inserts; GFS/AIFS use one worker. Batch sizes
are 16,384 cells for HRRR, 8,192 for RRFS and 4,096 for GFS/AIFS. Every insert
response is drained, including late errors. A disjoint database range query
checks every stored index, array length, revision and row count before that
batch counts as complete. Deterministic source samples come from the actual
immutable ingestion buffers. Final checks retain all cell IDs, physical types
and bit-exact complete samples. Error paths join every worker before cleanup.

Only a new unpublished table has merges paused, and successful merge resumption
is part of load success. Independent audits scan every stored lane, one packed
column at a time with bounded threads/blocks, and compare staging again when
requested. Published audits use private descriptor copies, preserving the
existing proof until a successful atomic replacement. Do not substitute a row
count, persisted success flag or source-only check for full value validation.

The loopback database has a 12 GiB process ceiling and a 2 GiB per-query ceiling.
Periodic allocator purges and consumed-page discard hints keep memory bounded.
Optional staged-input prefetch reserves 18 GiB beyond input bytes; otherwise it
streams. Source staging keeps a 20 GiB free-space floor. Source instances and their clones
share only one immutable snapshot's metadata/coordinates. HRRR pressure and cloud
tasks run together with one shared 16-map semaphore; both drain before returning
an error. Cloud payload retry directories are snapshot-scoped; a newer cloud
snapshot cannot reuse another snapshot's partially downloaded maps.

### Retention and serving

State lives in `~/.local/share/meteogram-native`; service-owned tables use
`meteogram_native`. Readers hold the shared catalog lock through database reads;
publication and GC use the exclusive lock. Catalog replacement is atomic and
fsynced. Keep two distinct complete initializations per model plus the latest
complete 48-hour HRRR run, counting overlap once. Partial runs do not replace
complete backups. Retired manifests have a ten-minute grace period. GC follows
all manifest/book/shared-table references and only drops explicitly owned tables
inside the service database. Unrelated benchmark tables are outside ownership.

The API selects `latest`, `latest_complete`, `previous_complete` and HRRR `long`
from explicit catalog roles. Complete selectors never substitute partial runs.
Point queries project the selected eight-lane word per slot and also validate
the original full array lengths; projecting a valid word must not hide malformed
unused groups. Current and retained archive models stay cached until their
catalog selections release them; decoded hourly books are shared across
manifest advances. `/health` requires all four models to be fresh and checks
ClickHouse connectivity. Outside-grid points return 404; HTTP requests never
ingest data.

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
and converts m/s to the app's km/h. GFS has native pressure-level cloud fraction.
HRRR retains all 50 native model-level cloud coverages exactly, independently of
its 30 atmospheric pressure levels. RRFS/AIFS and HRRR hours without a supplement
use labeled humidity cloud estimates. AIFS specific humidity uses Murphy–Koop
saturation and ECMWF mixed phase; GFS dew point is derived.

HRRR cloud version 2 stores `cloud-coverage.bin.zst`/`cloud-coverage.json` and
`cloud-anchors.stage.zst`/`cloud-anchors.stage.json` in each hourly child.
Coverage uses exact Float32 palettes, constant planes and independently
checksummed 256-cell spatial frames. The two Float32 anchors are native level-1
pressure (Pa) and level-50 geopotential height (m). Every stored value receives
bit-exact readback before publication. Source acquisition reads 50 fraction maps,
the two anchors and a native top-pressure guard map: 53 maps rather than 150.
Index hashes, file lengths, source snapshots, initialization and exact grid are
bound to the child's metadata hash. Both data files record SHA-256 hashes, and
point reads check frame CRCs. All four files survive staging cleanup and follow
catalog retention/GC.

Cloud layer pressure and height are deliberately derived, as authorized for the
cloud cost optimization. This exception does not allow quantizing coverages or
any atmospheric weather field, removing cells, or dropping model levels/hours.
`cloud_geometry.rs` uses NOAA's 51 HRRR interfaces and WRF's cubic hybrid weights
with the native bottom pressure. Retained pressure-level dew points estimate
the moisture-column correction; heights interpolate the retained geopotential
profile in log pressure. The exact native top height bounds the extension above
275 hPa. A native level-50 pressure map verifies the configured 15 hPa model lid.
The API still returns all 50 layer coordinates, declares the derivation in
`native_clouds.geometry`, and validates finite/monotonic results for each point.
The site labels derived layer heights and estimated coordinates in the tooltip.
The existing latitude-dependent WGS84 conversion is unchanged.

Cloud references can arrive after pressure references; acquisition pins its own
Icechunk snapshot for the same dataset/init/lead/grid and records
`pressure_snapshot_id` separately. Do not combine different initialized runs.
`native-clouds --root ROOT --run latest|latest_complete|long [--leads 1,2]` backfills
missing supplements with immutable child and parent revisions.
`compact-clouds --root ROOT` converts retained three-field cloud arrays without
source downloads or weather-table writes. It compares every fraction/anchor bit
again at publication, preserves pressure metadata/codebooks/proofs and selector
roles, atomically replaces catalog references and converts unpublished incoming
hours under the update lock. Old published descriptors retain the ten-minute GC
grace period. Existing version-2 supplements are not replaced. The original
three-field decoder is limited to conversion; serving uses version 2.
`cloud-availability --product PRODUCT` is a metadata-only diagnostic over the
latest 24 runs and four sampled leads, checking the same required references.

`CloudColumn.nativeCloudLayers` carries 50 separate coverages and their derived
coordinates. Shading uses midpoints between layer centres and is clipped to the
chart's altitude range. Time steps remain hourly. Pressure profiles, wind barbs
and thermodynamics retain their original values and levels. Hover state carries
cloud layers and atmospheric cells separately; the tooltip labels the pressure
level used for temperature and wind. The source badge identifies 50 native cloud
levels, derived layer heights, and any hours still estimated from humidity.
Schema/provenance/rendering coverage lives in `native-cloud-rendering.test.tsx`,
`native-weather.test.ts`, `clouds.rs`, `cloud_coverage.rs`, `cloud_geometry.rs` and
`cloud_compaction.rs` tests.

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
`docs/archive/2026-10-18-hrrr-native-clouds-and-runs.md`.

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
`docs/archive/2026-10-18-native-site-integration.md`.
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

| Purpose               | File(s)                                |
| --------------------- | -------------------------------------- |
| Main page component   | `src/app/[location]/[model]/page.tsx`  |
| Weather API calls     | `src/app/actions/weather.ts`           |
| Data transformation   | `src/utils/weather.ts`                 |
| Model/location config | `src/config/weather.ts`                |
| Preference defaults   | `src/config/preferences.ts`            |
| Preference state      | `src/context/PreferencesContext.tsx`   |
| URL param handling    | `src/utils/params.ts`                  |
| Main visualization    | `src/app/components/meteogram.tsx`     |
| Cloud rendering       | `src/app/components/cloud-columns.tsx` |
| Contour lines         | `src/app/components/weather-lines.tsx` |
| Type definitions      | `src/types/weather.ts`                 |
| Tests                 | `src/__tests__/*.test.ts`              |
