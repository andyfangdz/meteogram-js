# En-Route Cross-Section View — Design Spec

## Summary

Add an en-route view to meteogram-js that renders a distance × altitude cross-section of weather along a flight route. The user defines departure/destination (and optional intermediate waypoints), a cruise altitude, TAS, and departure time. The app samples weather at regular intervals along the great circle path and displays a cross-section showing the weather each point will experience at the estimated time-over.

## URL Structure

**Route**: `src/app/route/[waypoints]/[model]/page.tsx`

**URL format**:
```
/route/KCDW-SBJ-KBOS/gfs_hrrr?alt=6000&tas=120&dep=2026-03-24T14:00Z&res=25
```

- **Path**: `/route/{waypoints}/{model}` — waypoints are dash-separated airport codes or `Name@lat,lon` strings
- **Query params**:
  - `alt` — cruise altitude in feet MSL (default: 6000)
  - `tas` — true airspeed in knots (default: 120)
  - `dep` — departure time as ISO 8601 string (default: next whole hour)
  - `res` — sample resolution in NM (default: 25, min: 5)
  - All existing preference params (`showIsothermLines`, `showWindBarbs`, etc.)
- Model must be in `MODEL_NAMES` (same validation as existing route)

## Data Model

### New Types (`src/types/weather.ts`)

```typescript
interface RouteWaypoint {
  name: string;              // Airport code, custom name, or auto-generated
  latitude: number;
  longitude: number;
  distanceNM: number;        // Cumulative distance from departure
  isUserDefined: boolean;    // true for user waypoints, false for interpolated sample points
}

interface RoutePoint {
  waypoint: RouteWaypoint;
  weatherData: CloudColumn[];       // Forecast at this point (narrow time window); ephemeral — discarded after cross-section assembly
  elevationFt: number | null;
  estimatedTimeOver: Date;           // Computed from GS calculation (always set — falls back to GS=TAS if wind data missing)
  bearingDeg: number;                // Bearing to next point (degrees true); last point uses preceding leg's bearing
}

interface RouteConfig {
  waypoints: RouteWaypoint[];       // All points (user-defined + interpolated)
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: Date;
  resolutionNM: number;
}

interface RouteCrossSection {
  routeConfig: RouteConfig;
  points: RoutePoint[];
}
```

### Cross-Section Assembly

For each `RoutePoint`, select the single `CloudColumn` from its forecast data closest to the `estimatedTimeOver`. This produces one column per sample point — the cross-section slice.

The assembled cross-section is a `CloudColumn[]` array where each element represents one route sample point (not a timestep). The `date` field on each column reflects the estimated time-over at that point. The visualization components receive this array and use the column index to look up the corresponding `RouteWaypoint.distanceNM` for X positioning via the distance scale.

```typescript
// Assembly pseudocode
function assembleRouteCrossSection(points: RoutePoint[]): CloudColumn[] {
  return points.map(point => {
    const targetTime = point.estimatedTimeOver;
    // Find the forecast column closest to the target time
    return closestColumnByTime(point.weatherData, targetTime);
  });
}
```

## Route Computation

### Waypoint Resolution

1. Parse dash-separated waypoints from URL path
2. Resolve each to coordinates via `LOCATIONS` lookup or `Name@lat,lon` parsing (same as existing location resolution)
3. Validate: minimum 2 waypoints (departure + destination)

### Sample Point Generation

1. For each consecutive pair of waypoints, compute great circle path
2. Generate interpolated points every `res` NM along the path
3. User-defined waypoints are always included (flagged with `isUserDefined: true`)
4. Cap total sample points at 50; warn if exceeded

### Great Circle Math

- Use Haversine formula for distance between points
- Use forward azimuth for bearing (needed for wind component calculation)
- Interpolate intermediate points along the great circle arc using the spherical intermediate point formula (slerp)

## Groundspeed & Time-Over Calculation

For each sample point, starting from departure:

1. Find the pressure level closest to `cruiseAltitudeFt` in the forecast data
2. Extract wind speed and direction at that level for the forecast time closest to estimated arrival
3. Compute route bearing to the next point
4. Calculate headwind/tailwind component. Wind direction from Open-Meteo is meteorological (direction wind blows FROM). Tailwind component = `-windSpeed * cos(windDirection - bearing)` (positive = tailwind, negative = headwind). Example: wind FROM north (0°), aircraft heading north (0°) → `-ws * cos(0)` = negative = headwind. Correct.
5. `groundspeed = TAS + tailwindComponent`
6. `timeToNextPoint = legDistanceNM / groundspeed`
7. Accumulate `estimatedTimeOver` from departure time

This is iterative — each point's time-over depends on the previous point's, and the wind lookup depends on the time-over. One pass is sufficient (no need to iterate to convergence).

## Server Actions

### New file: `src/app/actions/route.ts`

**`resolveRouteWaypoints(waypointString: string, resolutionNM: number): RouteWaypoint[]`**
- Parses waypoint string, resolves coordinates, generates interpolated sample points

**`fetchRouteWeatherAction(waypoints: RouteWaypoint[], model: WeatherModel, departureTime: Date): Promise<RoutePoint[]>`**
- Fetches weather + elevation for all sample points
- Uses existing `getWeatherData()` for weather (one call per point — Open-Meteo's forecast API does not support multi-location batch requests)
- Batches elevation lookups: Open-Meteo's elevation API accepts comma-separated coordinate lists, so all elevations can be fetched in a single request via a new `fetchBatchElevationAction()`
- Concurrency limiting: processes weather fetches in batches of 10 to avoid overwhelming Open-Meteo's rate limits (free tier)
- Requests a narrower time window via Open-Meteo's `start_date` / `end_date` params (date-level granularity). For a flight departing 2026-03-24T14:00Z with ~3h estimated flight time, requests just `2026-03-24` to `2026-03-25`. This requires a new route-specific fetch function rather than reusing `getWeatherData()`, which uses `stepSize` for forecast length
- Uses `Promise.allSettled` within each batch to handle individual point failures gracefully (gaps in cross-section)

**`computeRouteTimings(points: RoutePoint[], cruiseAltFt: number, tasKnots: number, departureTime: Date): RoutePoint[]`**
- Computes groundspeed and estimatedTimeOver for each point
- Returns updated RoutePoint array with timing data

## Visualization

### Component: `RouteMeteogram` (`src/app/components/route-meteogram.tsx`)

SVG cross-section with:
- **X axis**: distance along route (NM), linear scale. User-defined waypoints get labeled tick marks.
- **Y axis**: altitude in feet MSL (same as existing meteogram)

### Rendering Layers (bottom to top)

1. **Terrain profile** — filled polygon from elevation data at each sample point, shaded brown/gray
2. **Cloud columns** — new `RouteCloudColumns` component (parallel to `CloudColumns`). Same visual rendering (cloud coverage rectangles, wind barbs) but uses a linear distance scale for X positioning instead of `dateScale`. Cannot reuse `CloudColumns` directly because it is hardcoded to `scales.dateScale(d.date)` and uses time-based wind barb thinning (`windBarbStep`). For the route view, wind barb thinning should be distance-based (e.g., every N sample points).
3. **Contour lines** — new `RouteWeatherLines` component (parallel to `WeatherLines`). Reuses the same `findIsothermPoints`/`findIsotachPoints`/`findDewPointDepressionPoints` utility functions from `meteogram.ts`, but with distance-indexed data. Note: contour interpolation between spatially separated points is an approximation — adjacent columns may have very different atmospheric profiles. Set `maxStepDistance` conservatively (e.g., 2× the sample resolution) to avoid misleading interpolation across large gaps.
4. **Pressure lines** — new `RoutePressureLines` (parallel to `PressureLines`), using distance scale instead of `dateScale`
5. **Planned altitude line** — dashed horizontal line at `cruiseAltitudeFt`, with wind barbs along it showing wind at cruise level at each sample point
6. **Waypoint markers** — vertical dashed lines at user-defined waypoints with labels (airport codes/names)

### Interactions

- Hover crosshair + tooltip: waypoint name (if near one), distance from departure, altitude, cloud cover, temperature, wind, estimated time-over
- Same preference toggles as existing meteogram (isotherms, isotachs, wind barbs, etc.)

### Scale Differences from Existing Meteogram

- X scale: `scaleLinear` (distance in NM) instead of `scaleBand` (time)
- The `useMeteogramScales` hook won't be reused directly; `RouteMeteogram` creates its own scales via a new `useRouteScales` hook

## Route Input UI

### Component: `RouteHeader` (`src/app/components/route-header.tsx`)

Compact header bar above the cross-section:

- **Route field**: text input showing waypoints (e.g., "KCDW - SBJ - KBOS"). Editable, uses existing geocoding search for autocomplete.
- **Cruise altitude**: number input (feet), default 6000
- **TAS**: number input (knots), default 120
- **Departure time**: datetime picker, defaults to next whole hour
- **Resolution**: small input for sample interval in NM, default 25
- **Update button**: re-fetches weather and re-renders; updates URL

### State Management

`RouteClientWrapper` (`src/app/components/route-client-wrapper.tsx`):
- Holds route configuration state
- Manages weather data state
- Handles URL sync (updates URL when config changes)
- No auto-refresh (manual refresh only via Update button, due to heavier API load)
- Integrates with existing `PreferencesContext` for visualization toggles

## Navigation Integration

- Add a "Route" link to the existing nav bar that navigates to `/route/KCDW-KBOS/gfs_hrrr` (or the user's current model) as a starting point
- In the route view, waypoint labels in the cross-section are clickable links to the single-location meteogram (e.g., clicking "KBOS" navigates to `/KBOS/gfs_hrrr`)
- The route view shares the same model selector as the existing nav

## New Files

| File | Purpose |
|------|---------|
| `src/app/route/[waypoints]/[model]/page.tsx` | Route page (server component) |
| `src/app/actions/route.ts` | Route-specific server actions |
| `src/app/components/route-meteogram.tsx` | Cross-section visualization |
| `src/app/components/route-client-wrapper.tsx` | Client state management |
| `src/app/components/route-header.tsx` | Route input UI |
| `src/app/components/terrain-profile.tsx` | Terrain polygon rendering |
| `src/app/components/route-cloud-columns.tsx` | Cloud rectangles with distance-based X |
| `src/app/components/route-weather-lines.tsx` | Contour lines with distance-based X |
| `src/app/components/route-pressure-lines.tsx` | Pressure lines with distance-based X |
| `src/app/components/altitude-line.tsx` | Planned altitude overlay |
| `src/app/components/waypoint-markers.tsx` | Vertical waypoint labels |
| `src/hooks/useRouteScales.ts` | Visx scales for distance × altitude |
| `src/utils/route.ts` | Great circle math, sample point generation |
| `src/__tests__/route.test.ts` | Route computation tests |

## Modified Files

| File | Change |
|------|--------|
| `src/types/weather.ts` | Add RouteWaypoint, RoutePoint, RouteConfig, RouteCrossSection types |
| `src/config/weather.ts` | No changes needed |
| `src/utils/params.ts` | Add route-specific param parsing (alt, tas, dep, res) |
| `src/app/components/nav.tsx` | Add "Route" link |
| `AGENTS.md` | Document new route architecture |

## Constraints & Edge Cases

1. **Max sample points**: Cap at 50 to keep API load reasonable. If `routeDistance / res > 50`, automatically increase `res` to fit within the cap and display a notice to the user (e.g., "Resolution adjusted to 35 NM to stay within 50 sample points").
2. **Failed point fetches**: Use `Promise.allSettled`. Show gaps in cross-section for failed points rather than failing the whole view.
3. **Short routes**: Routes under 10 NM should still work (minimum 2 sample points: departure + destination).
4. **Wind calm / variable**: If wind data is missing or calm at cruise altitude, fall back to GS = TAS.
5. **Cruise altitude above available data**: If cruise altitude exceeds the highest pressure level in the model, use the highest available level for wind calculation.
6. **Edge compatibility**: All new server actions use standard Node APIs only (no filesystem, no native bindings).

## Out of Scope (Future)

- Map view showing the route
- Multiple cruise altitudes (step climbs)
- SID/STAR integration
- Auto-refresh on the route view
- Route storage/persistence beyond URL
