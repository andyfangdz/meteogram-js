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
| Add route feature | `src/utils/route.ts`, `src/app/actions/route-actions.ts`, route components | `yarn ts && yarn build` |

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
│   ├── route/[waypoints]/[model]/page.tsx  # Route cross-section page
│   ├── page.tsx                        # Root redirect
│   ├── layout.tsx                      # Root layout with providers
│   ├── actions/
│   │   ├── weather.ts                  # Server actions: fetchWeatherDataAction, getWeatherData
│   │   ├── geocoding.ts                # Location resolution utilities
│   │   └── route-actions.ts            # Route-specific server actions
│   └── components/
│       ├── client-wrapper.tsx          # Client boundary, state management, auto-refresh
│       ├── meteogram.tsx               # Main visualization container
│       ├── cloud-columns.tsx           # Cloud coverage rectangles + wind barbs
│       ├── weather-lines.tsx           # Isotherms, isotachs, freezing level
│       ├── pressure-lines.tsx          # Constant-pressure surface lines
│       ├── nav.tsx, nav-wrapper.tsx    # Navigation bar with preference toggles
│       ├── route-client-wrapper.tsx    # Client boundary for route view
│       ├── route-meteogram.tsx         # Route cross-section visualization container
│       ├── route-cloud-columns.tsx     # Cloud coverage along route
│       ├── route-weather-lines.tsx     # Isotherms/isotachs along route
│       ├── route-pressure-lines.tsx    # Pressure surface lines along route
│       ├── terrain-profile.tsx         # Terrain elevation profile
│       ├── altitude-line.tsx           # Cruise altitude indicator
│       ├── waypoint-markers.tsx        # Waypoint tick marks on X-axis
│       ├── route-header.tsx            # Route summary header
│       └── ...                         # Other UI components
├── config/
│   ├── weather.ts                      # MODEL_CONFIGS, LOCATIONS, pressure levels
│   └── preferences.ts                  # DEFAULT_PREFERENCES
├── context/
│   └── PreferencesContext.tsx          # Client-side preference state + persistence
├── hooks/
│   ├── useMeteogramScales.ts           # Memoized Visx scales
│   └── useRouteScales.ts               # Memoized Visx scales for route view
├── types/
│   └── weather.ts                      # TypeScript interfaces
├── utils/
│   ├── weather.ts                      # transformWeatherData function
│   ├── meteogram.ts                    # Grid interpolation, contour helpers
│   ├── params.ts                       # URL param serialization
│   ├── serverPreferences.ts            # Server-side cookie/URL preference logic
│   └── route.ts                        # Great circle math, sample points, timing
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

Route cross-section flow:

```
[Route URL: /route/KCDW-KFRG/gfs_hrrr?alt=6000&tas=120]
       ↓
resolveRouteWaypoints → generateRouteSamplePoints
       ↓
fetchRouteWeatherAction (batched, 10 at a time)
       ↓
computeTimings (wind-corrected groundspeed)
       ↓
assembleRouteCrossSection (pick closest column per point)
       ↓
RouteClientWrapper → RouteMeteogram
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

## Route View

The en-route cross-section shows weather along a flight path rather than at a single point over time. The X-axis represents distance (nautical miles along the route) rather than time.

### URL Structure

```
/route/{waypoints}/{model}?alt=6000&tas=120&dep=&res=
```

| Param | Description |
|-------|-------------|
| `waypoints` | Dash-separated list of waypoints (e.g., `KCDW-KFRG` or `KCDW-Name@40.73,-73.42-KFRG`) |
| `alt` | Cruise altitude in feet MSL |
| `tas` | True airspeed in knots (used for wind-corrected timing) |
| `dep` | Departure time (ISO string or empty for "now") |
| `res` | Sample resolution in NM (optional, defaults to auto) |

### Waypoint Formats

- **Airport code**: `KCDW` — resolved via geocoding to lat/lon
- **Custom point**: `Name@lat,lon` — same format as single-location URLs
- Waypoints are dash-separated in the URL path segment

### Sample Point Generation

- Route is divided into sample points every N nautical miles using great circle math (`src/utils/route.ts`)
- Point count is capped at 50 to limit API calls
- Each sample point fetches weather independently via `fetchRouteWeatherAction`
- API requests are batched 10 at a time to stay within rate limits

### Key Constraint: File Naming in app/

**Do NOT name any file `route.ts` inside the `src/app/` directory tree.** Next.js App Router treats files named `route.ts` (or `route.js`) as HTTP route handlers. A file at e.g. `src/app/actions/route.ts` would be interpreted as a handler for the `/actions` path, not a module. Route-related server actions live in `src/app/actions/route-actions.ts`.

### Route Component Hierarchy

```
RouteMeteogram (SVG container, distance-based X scale)
├── RouteCloudColumns (cloud rectangles per sample point)
│   └── WindBarb (per filtered cell)
├── RouteWeatherLines (isotherms, isotachs along route)
├── RoutePressureLines (constant-pressure paths)
├── TerrainProfile (filled elevation profile at bottom)
├── AltitudeLine (horizontal cruise altitude indicator)
├── WaypointMarkers (tick marks + labels at waypoint positions)
├── DistanceAxis (X-axis in NM)
├── AxisLeft (Y-axis in feet/pressure)
└── RouteMeteogramTooltip (data display on hover)
```

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
- `src/__tests__/route.test.ts` — Route computation (great circle, timing, waypoint parsing)

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
| Route page | `src/app/route/[waypoints]/[model]/page.tsx` |
| Route server actions | `src/app/actions/route-actions.ts` |
| Route math utilities | `src/utils/route.ts` |
| Route visualization | `src/app/components/route-meteogram.tsx` |
| Route cloud rendering | `src/app/components/route-cloud-columns.tsx` |
| Route contour lines | `src/app/components/route-weather-lines.tsx` |
| Terrain profile | `src/app/components/terrain-profile.tsx` |
| Altitude indicator | `src/app/components/altitude-line.tsx` |
| Waypoint markers | `src/app/components/waypoint-markers.tsx` |
| Route scales hook | `src/hooks/useRouteScales.ts` |
