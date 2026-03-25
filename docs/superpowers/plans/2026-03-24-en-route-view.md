# En-Route Cross-Section View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a distance × altitude cross-section view that shows weather along a flight route, with terrain profile, planned altitude line, and wind-corrected time-over estimates.

**Architecture:** New `/route/[waypoints]/[model]` page with parallel route-specific visualization components. Route utility functions handle great circle math and sample point generation. A route-specific server action fetches weather in concurrency-limited batches and assembles the cross-section by selecting the forecast column closest to each point's estimated time-over.

**Tech Stack:** Next.js 16 App Router, Visx (scaleLinear for distance axis), Open-Meteo API, TypeScript, Vitest

**Spec:** `docs/superpowers/specs/2026-03-24-en-route-view-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `src/utils/route.ts` | Great circle math (haversine, bearing, slerp interpolation), sample point generation, groundspeed/timing calculation, cross-section assembly |
| `src/types/weather.ts` (modify) | Add RouteWaypoint, RoutePoint, RouteConfig, RouteCrossSection interfaces |
| `src/utils/params.ts` (modify) | Add parseRouteParams / serializeRouteParams for alt, tas, dep, res |
| `src/__tests__/route.test.ts` | Tests for route utility functions |
| `src/app/actions/route.ts` | Server actions: resolveRouteWaypoints, fetchRouteWeatherAction, fetchBatchElevationAction |
| `src/__tests__/route-actions.test.ts` | Tests for route server actions |
| `src/hooks/useRouteScales.ts` | Visx scales: distanceScale (linear), mslScale, cloudScale |
| `src/app/components/terrain-profile.tsx` | SVG filled polygon for elevation along route |
| `src/app/components/route-cloud-columns.tsx` | Cloud coverage rectangles using distance scale |
| `src/app/components/route-weather-lines.tsx` | Contour lines (isotherms, isotachs, etc.) using distance scale |
| `src/app/components/route-pressure-lines.tsx` | Constant-pressure paths using distance scale |
| `src/app/components/altitude-line.tsx` | Dashed cruise altitude line with wind barbs |
| `src/app/components/waypoint-markers.tsx` | Vertical dashed lines at user-defined waypoints |
| `src/app/components/route-meteogram.tsx` | SVG container composing all route visualization layers |
| `src/app/components/route-header.tsx` | Route input UI (waypoints, alt, TAS, dep time, resolution) |
| `src/app/components/route-client-wrapper.tsx` | Client state management, URL sync, manual refresh |
| `src/app/route/[waypoints]/[model]/page.tsx` | Server component page for route view |

### Modified Files

| File | Change |
|------|--------|
| `src/app/components/nav.tsx` | Add "Route" link |
| `AGENTS.md` | Document route architecture |

---

## Task 1: Route Types

**Files:**
- Modify: `src/types/weather.ts`

- [ ] **Step 1: Add route interfaces to types/weather.ts**

Add after the existing `VisualizationPreferences` interface:

```typescript
export interface RouteWaypoint {
  name: string;
  latitude: number;
  longitude: number;
  distanceNM: number;
  isUserDefined: boolean;
}

export interface RoutePoint {
  waypoint: RouteWaypoint;
  weatherData: CloudColumn[];
  elevationFt: number | null;
  estimatedTimeOver: Date;
  bearingDeg: number;
}

export interface RouteConfig {
  waypoints: RouteWaypoint[];
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: Date;
  resolutionNM: number;
}

export interface RouteCrossSection {
  routeConfig: RouteConfig;
  points: RoutePoint[];
}
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS (no type errors)

- [ ] **Step 3: Commit**

```bash
git add src/types/weather.ts
git commit -m "feat(route): add route type definitions"
```

---

## Task 2: Route URL Parameter Parsing

**Files:**
- Modify: `src/utils/params.ts`
- Modify: `src/__tests__/params.test.ts`

- [ ] **Step 1: Write failing tests for route param parsing**

Add to `src/__tests__/params.test.ts`:

```typescript
import { parseRouteParams, serializeRouteParams } from "@/utils/params";

describe("parseRouteParams", () => {
  it("parses all route params with defaults", () => {
    const result = parseRouteParams({});
    expect(result.cruiseAltitudeFt).toBe(6000);
    expect(result.tasKnots).toBe(120);
    expect(result.resolutionNM).toBe(25);
    expect(result.departureTime).toBeInstanceOf(Date);
  });

  it("parses provided route params", () => {
    const result = parseRouteParams({
      alt: "8000",
      tas: "150",
      res: "10",
      dep: "2026-03-24T14:00:00.000Z",
    });
    expect(result.cruiseAltitudeFt).toBe(8000);
    expect(result.tasKnots).toBe(150);
    expect(result.resolutionNM).toBe(10);
    expect(result.departureTime).toEqual(new Date("2026-03-24T14:00:00.000Z"));
  });

  it("clamps resolution to minimum of 5", () => {
    const result = parseRouteParams({ res: "2" });
    expect(result.resolutionNM).toBe(5);
  });
});

describe("serializeRouteParams", () => {
  it("serializes non-default route params", () => {
    const params = serializeRouteParams({
      cruiseAltitudeFt: 8000,
      tasKnots: 120, // default, should be omitted
      resolutionNM: 25, // default, should be omitted
      departureTime: new Date("2026-03-24T14:00:00.000Z"),
    });
    expect(params.get("alt")).toBe("8000");
    expect(params.has("tas")).toBe(false);
    expect(params.has("res")).toBe(false);
    expect(params.get("dep")).toBe("2026-03-24T14:00:00.000Z");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `yarn test src/__tests__/params.test.ts`
Expected: FAIL — `parseRouteParams` and `serializeRouteParams` not found

- [ ] **Step 3: Implement route param functions in params.ts**

Add to `src/utils/params.ts`:

```typescript
export interface RouteParams {
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: Date;
  resolutionNM: number;
}

const DEFAULT_ROUTE_PARAMS: RouteParams = {
  cruiseAltitudeFt: 6000,
  tasKnots: 120,
  departureTime: new Date(), // placeholder — actual default computed at parse time
  resolutionNM: 25,
};

function getNextWholeHour(): Date {
  const now = new Date();
  now.setMinutes(0, 0, 0);
  now.setHours(now.getHours() + 1);
  return now;
}

export function parseRouteParams(searchParams: Record<string, string | undefined>): RouteParams {
  const alt = searchParams.alt ? parseInt(searchParams.alt, 10) : 6000;
  const tas = searchParams.tas ? parseInt(searchParams.tas, 10) : 120;
  const res = searchParams.res ? Math.max(5, parseInt(searchParams.res, 10)) : 25;
  const dep = searchParams.dep ? new Date(searchParams.dep) : getNextWholeHour();

  return {
    cruiseAltitudeFt: isNaN(alt) ? 6000 : alt,
    tasKnots: isNaN(tas) ? 120 : tas,
    resolutionNM: isNaN(res) ? 25 : res,
    departureTime: isNaN(dep.getTime()) ? getNextWholeHour() : dep,
  };
}

export function serializeRouteParams(params: RouteParams): URLSearchParams {
  const urlParams = new URLSearchParams();
  if (params.cruiseAltitudeFt !== 6000) urlParams.set("alt", params.cruiseAltitudeFt.toString());
  if (params.tasKnots !== 120) urlParams.set("tas", params.tasKnots.toString());
  if (params.resolutionNM !== 25) urlParams.set("res", params.resolutionNM.toString());
  // Always serialize departure time (no meaningful default to compare against)
  urlParams.set("dep", params.departureTime.toISOString());
  return urlParams;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `yarn test src/__tests__/params.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/utils/params.ts src/__tests__/params.test.ts
git commit -m "feat(route): add route URL parameter parsing and serialization"
```

---

## Task 3: Great Circle Math Utilities

**Files:**
- Create: `src/utils/route.ts`
- Create: `src/__tests__/route.test.ts`

- [ ] **Step 1: Write failing tests for haversine distance**

Create `src/__tests__/route.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import {
  haversineDistanceNM,
  forwardBearing,
  interpolateGreatCircle,
} from "@/utils/route";

describe("haversineDistanceNM", () => {
  it("returns 0 for same point", () => {
    expect(haversineDistanceNM(40, -74, 40, -74)).toBeCloseTo(0, 1);
  });

  it("calculates KCDW to KFRG (~50 NM)", () => {
    // KCDW: 40.8727, -74.2838; KFRG: 40.7344, -73.4164
    const dist = haversineDistanceNM(40.8727, -74.2838, 40.7344, -73.4164);
    expect(dist).toBeGreaterThan(40);
    expect(dist).toBeLessThan(60);
  });
});

describe("forwardBearing", () => {
  it("returns ~0 for due north", () => {
    const bearing = forwardBearing(40, -74, 41, -74);
    expect(bearing).toBeCloseTo(0, 0);
  });

  it("returns ~90 for due east", () => {
    const bearing = forwardBearing(40, -74, 40, -73);
    expect(bearing).toBeGreaterThan(85);
    expect(bearing).toBeLessThan(95);
  });
});

describe("interpolateGreatCircle", () => {
  it("returns start and end for short distance", () => {
    const points = interpolateGreatCircle(
      { lat: 40, lon: -74, name: "A" },
      { lat: 40.01, lon: -74, name: "B" },
      100, // resolution larger than distance
    );
    expect(points).toHaveLength(2);
    expect(points[0].isUserDefined).toBe(true);
    expect(points[1].isUserDefined).toBe(true);
  });

  it("generates intermediate points at resolution intervals", () => {
    // ~50 NM route, 10 NM resolution → ~6 points (start + 4 intermediate + end)
    const points = interpolateGreatCircle(
      { lat: 40.8727, lon: -74.2838, name: "KCDW" },
      { lat: 40.7344, lon: -73.4164, name: "KFRG" },
      10,
    );
    expect(points.length).toBeGreaterThanOrEqual(4);
    expect(points.length).toBeLessThanOrEqual(8);
    expect(points[0].name).toBe("KCDW");
    expect(points[0].isUserDefined).toBe(true);
    expect(points[points.length - 1].name).toBe("KFRG");
    expect(points[points.length - 1].isUserDefined).toBe(true);
    // Intermediate points should not be user-defined
    expect(points[1].isUserDefined).toBe(false);
    // Distance should be monotonically increasing
    for (let i = 1; i < points.length; i++) {
      expect(points[i].distanceNM).toBeGreaterThan(points[i - 1].distanceNM);
    }
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `yarn test src/__tests__/route.test.ts`
Expected: FAIL — module not found

- [ ] **Step 3: Implement great circle functions**

Create `src/utils/route.ts`:

```typescript
import type { RouteWaypoint } from "../types/weather";

const EARTH_RADIUS_NM = 3440.065; // Mean Earth radius in nautical miles

function toRad(deg: number): number {
  return (deg * Math.PI) / 180;
}

function toDeg(rad: number): number {
  return (rad * 180) / Math.PI;
}

/**
 * Haversine distance between two points in nautical miles.
 */
export function haversineDistanceNM(
  lat1: number,
  lon1: number,
  lat2: number,
  lon2: number,
): number {
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_NM * c;
}

/**
 * Forward azimuth (initial bearing) from point 1 to point 2 in degrees [0, 360).
 */
export function forwardBearing(
  lat1: number,
  lon1: number,
  lat2: number,
  lon2: number,
): number {
  const φ1 = toRad(lat1);
  const φ2 = toRad(lat2);
  const Δλ = toRad(lon2 - lon1);
  const y = Math.sin(Δλ) * Math.cos(φ2);
  const x =
    Math.cos(φ1) * Math.sin(φ2) -
    Math.sin(φ1) * Math.cos(φ2) * Math.cos(Δλ);
  const θ = Math.atan2(y, x);
  return ((toDeg(θ) % 360) + 360) % 360;
}

/**
 * Interpolate intermediate points along the great circle arc (slerp).
 * Returns array including start and end points.
 */
export function interpolateGreatCircle(
  start: { lat: number; lon: number; name: string },
  end: { lat: number; lon: number; name: string },
  resolutionNM: number,
  startDistanceNM: number = 0,
): RouteWaypoint[] {
  const totalDist = haversineDistanceNM(start.lat, start.lon, end.lat, end.lon);
  const points: RouteWaypoint[] = [];

  // Always include start
  points.push({
    name: start.name,
    latitude: start.lat,
    longitude: start.lon,
    distanceNM: startDistanceNM,
    isUserDefined: true,
  });

  if (totalDist <= resolutionNM) {
    // No intermediate points needed
    points.push({
      name: end.name,
      latitude: end.lat,
      longitude: end.lon,
      distanceNM: startDistanceNM + totalDist,
      isUserDefined: true,
    });
    return points;
  }

  // Slerp intermediate points
  const φ1 = toRad(start.lat);
  const λ1 = toRad(start.lon);
  const φ2 = toRad(end.lat);
  const λ2 = toRad(end.lon);
  const d = totalDist / EARTH_RADIUS_NM; // angular distance

  const numSegments = Math.ceil(totalDist / resolutionNM);
  for (let i = 1; i < numSegments; i++) {
    const f = i / numSegments;
    const a = Math.sin((1 - f) * d) / Math.sin(d);
    const b = Math.sin(f * d) / Math.sin(d);
    const x = a * Math.cos(φ1) * Math.cos(λ1) + b * Math.cos(φ2) * Math.cos(λ2);
    const y = a * Math.cos(φ1) * Math.sin(λ1) + b * Math.cos(φ2) * Math.sin(λ2);
    const z = a * Math.sin(φ1) + b * Math.sin(φ2);
    const lat = toDeg(Math.atan2(z, Math.sqrt(x * x + y * y)));
    const lon = toDeg(Math.atan2(y, x));

    points.push({
      name: `WP${i}`,
      latitude: lat,
      longitude: lon,
      distanceNM: startDistanceNM + (totalDist * i) / numSegments,
      isUserDefined: false,
    });
  }

  // Always include end
  points.push({
    name: end.name,
    latitude: end.lat,
    longitude: end.lon,
    distanceNM: startDistanceNM + totalDist,
    isUserDefined: true,
  });

  return points;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `yarn test src/__tests__/route.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/utils/route.ts src/__tests__/route.test.ts
git commit -m "feat(route): add great circle math utilities"
```

---

## Task 4: Sample Point Generation & Route Resolution

**Files:**
- Modify: `src/utils/route.ts`
- Modify: `src/__tests__/route.test.ts`

- [ ] **Step 1: Write failing tests for generateRouteSamplePoints and parseWaypointString**

Add to `src/__tests__/route.test.ts`:

```typescript
import {
  generateRouteSamplePoints,
  parseWaypointString,
} from "@/utils/route";

describe("parseWaypointString", () => {
  it("parses dash-separated airport codes", () => {
    const result = parseWaypointString("KCDW-KFRG");
    expect(result).toEqual([
      { name: "KCDW", identifier: "KCDW" },
      { name: "KBOS", identifier: "KBOS" },
    ]);
  });

  it("parses custom coordinate waypoints", () => {
    const result = parseWaypointString("KCDW-MySpot@41.5,-73.0-KBOS");
    expect(result).toHaveLength(3);
    expect(result[1]).toEqual({
      name: "MySpot",
      identifier: "MySpot@41.5,-73.0",
    });
  });

  it("throws for fewer than 2 waypoints", () => {
    expect(() => parseWaypointString("KCDW")).toThrow("at least 2 waypoints");
  });
});

describe("generateRouteSamplePoints", () => {
  it("generates points for a multi-waypoint route", () => {
    const waypoints = [
      { name: "A", latitude: 40.87, longitude: -74.28 },
      { name: "B", latitude: 41.5, longitude: -73.0 },
      { name: "C", latitude: 42.37, longitude: -71.01 },
    ];
    const points = generateRouteSamplePoints(waypoints, 25);
    // Should include all 3 user-defined waypoints
    const userPoints = points.filter((p) => p.isUserDefined);
    expect(userPoints).toHaveLength(3);
    expect(userPoints[0].name).toBe("A");
    expect(userPoints[1].name).toBe("B");
    expect(userPoints[2].name).toBe("C");
    // Distance should be monotonically increasing
    for (let i = 1; i < points.length; i++) {
      expect(points[i].distanceNM).toBeGreaterThan(points[i - 1].distanceNM);
    }
  });

  it("caps total points at maxPoints", () => {
    const waypoints = [
      { name: "A", latitude: 40.0, longitude: -74.0 },
      { name: "B", latitude: 50.0, longitude: -60.0 }, // ~700+ NM
    ];
    const result = generateRouteSamplePoints(waypoints, 5, 20);
    expect(result.length).toBeLessThanOrEqual(20);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `yarn test src/__tests__/route.test.ts`
Expected: FAIL — functions not exported

- [ ] **Step 3: Implement parseWaypointString and generateRouteSamplePoints**

Add to `src/utils/route.ts`:

```typescript
interface ParsedWaypoint {
  name: string;
  identifier: string; // The raw string (airport code or Name@lat,lon)
}

/**
 * Parse a dash-separated waypoint string into individual waypoints.
 * Handles both airport codes and Name@lat,lon format.
 */
export function parseWaypointString(waypointString: string): ParsedWaypoint[] {
  const parts = waypointString.split("-").filter(Boolean);
  if (parts.length < 2) {
    throw new Error("Route requires at least 2 waypoints");
  }
  return parts.map((part) => {
    const name = part.includes("@") ? part.split("@")[0] : part;
    return { name, identifier: part };
  });
}

/**
 * Generate sample points along a multi-leg route.
 * Includes user-defined waypoints + interpolated points at resolutionNM intervals.
 * Auto-adjusts resolution if total points would exceed maxPoints.
 */
export function generateRouteSamplePoints(
  resolvedWaypoints: Array<{ name: string; latitude: number; longitude: number }>,
  resolutionNM: number,
  maxPoints: number = 50,
): RouteWaypoint[] {
  if (resolvedWaypoints.length < 2) {
    throw new Error("Route requires at least 2 waypoints");
  }

  // Calculate total route distance
  let totalDistance = 0;
  for (let i = 0; i < resolvedWaypoints.length - 1; i++) {
    totalDistance += haversineDistanceNM(
      resolvedWaypoints[i].latitude,
      resolvedWaypoints[i].longitude,
      resolvedWaypoints[i + 1].latitude,
      resolvedWaypoints[i + 1].longitude,
    );
  }

  // Auto-adjust resolution if too many points
  let effectiveRes = resolutionNM;
  const estimatedPoints = Math.ceil(totalDistance / resolutionNM) + resolvedWaypoints.length;
  if (estimatedPoints > maxPoints) {
    effectiveRes = totalDistance / (maxPoints - resolvedWaypoints.length);
  }

  // Generate points for each leg
  const allPoints: RouteWaypoint[] = [];
  let cumulativeDistance = 0;

  for (let i = 0; i < resolvedWaypoints.length - 1; i++) {
    const start = resolvedWaypoints[i];
    const end = resolvedWaypoints[i + 1];
    const legPoints = interpolateGreatCircle(
      { lat: start.latitude, lon: start.longitude, name: start.name },
      { lat: end.latitude, lon: end.longitude, name: end.name },
      effectiveRes,
      cumulativeDistance,
    );

    // Avoid duplicating waypoints at leg junctions
    if (i > 0) {
      legPoints.shift(); // Remove start of this leg (= end of previous leg)
    }
    allPoints.push(...legPoints);

    cumulativeDistance += haversineDistanceNM(
      start.latitude,
      start.longitude,
      end.latitude,
      end.longitude,
    );
  }

  return allPoints;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `yarn test src/__tests__/route.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/utils/route.ts src/__tests__/route.test.ts
git commit -m "feat(route): add waypoint parsing and sample point generation"
```

---

## Task 5: Groundspeed & Time-Over Calculation

**Files:**
- Modify: `src/utils/route.ts`
- Modify: `src/__tests__/route.test.ts`

- [ ] **Step 1: Write failing tests for computeTimings and tailwindComponent**

Add to `src/__tests__/route.test.ts`:

```typescript
import { tailwindComponent, computeTimings } from "@/utils/route";
import type { CloudColumn, RouteWaypoint } from "@/types/weather";

describe("tailwindComponent", () => {
  it("returns negative (headwind) when wind is from ahead", () => {
    // Wind FROM north (0°), heading north (0°) → headwind
    const result = tailwindComponent(20, 0, 0);
    expect(result).toBeCloseTo(-20, 1);
  });

  it("returns positive (tailwind) when wind is from behind", () => {
    // Wind FROM south (180°), heading north (0°) → tailwind
    const result = tailwindComponent(20, 180, 0);
    expect(result).toBeCloseTo(20, 1);
  });

  it("returns 0 for pure crosswind", () => {
    // Wind FROM east (90°), heading north (0°) → crosswind
    const result = tailwindComponent(20, 90, 0);
    expect(result).toBeCloseTo(0, 1);
  });
});

describe("computeTimings", () => {
  it("computes time-over for each point based on TAS and wind", () => {
    const departureTime = new Date("2026-03-24T14:00:00Z");
    const waypoints: RouteWaypoint[] = [
      { name: "A", latitude: 40, longitude: -74, distanceNM: 0, isUserDefined: true },
      { name: "B", latitude: 40.5, longitude: -73.5, distanceNM: 30, isUserDefined: false },
      { name: "C", latitude: 41, longitude: -73, distanceNM: 60, isUserDefined: true },
    ];

    // Create minimal CloudColumn data with wind at a single pressure level
    const makeWeatherData = (): CloudColumn[] => [{
      date: new Date("2026-03-24T14:00:00Z"),
      groundTemp: 15,
      cloud: [{
        hpa: 850,
        mslFt: 5000,
        geopotentialFt: 5000,
        cloudCoverage: 50,
        mslFtBottom: 4500,
        mslFtTop: 5500,
        temperature: 5,
        dewPoint: 2,
        windSpeed: 0, // Calm wind → GS = TAS
        windDirection: 0,
      }],
    }];

    const result = computeTimings(
      waypoints,
      makeWeatherData,
      6000, // cruise alt
      120, // TAS knots
      departureTime,
    );

    expect(result[0].estimatedTimeOver).toEqual(departureTime);
    // 30 NM at 120 kts = 0.25 hours = 15 minutes
    const expectedSecond = new Date(departureTime.getTime() + 15 * 60 * 1000);
    expect(result[1].estimatedTimeOver.getTime()).toBeCloseTo(expectedSecond.getTime(), -3);
    // 60 NM at 120 kts = 0.5 hours = 30 minutes
    const expectedThird = new Date(departureTime.getTime() + 30 * 60 * 1000);
    expect(result[2].estimatedTimeOver.getTime()).toBeCloseTo(expectedThird.getTime(), -3);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `yarn test src/__tests__/route.test.ts`
Expected: FAIL — functions not exported

- [ ] **Step 3: Implement tailwindComponent and computeTimings**

Add to `src/utils/route.ts`:

```typescript
import type { RouteWaypoint, CloudColumn, CloudCell } from "../types/weather";

/**
 * Calculate tailwind component in knots.
 * Positive = tailwind, negative = headwind.
 * windDirection is meteorological (direction wind blows FROM).
 */
export function tailwindComponent(
  windSpeedKmh: number,
  windDirectionDeg: number,
  bearingDeg: number,
): number {
  const windSpeedKnots = windSpeedKmh * 0.539957; // km/h to knots
  const diffRad = toRad(windDirectionDeg - bearingDeg);
  return -windSpeedKnots * Math.cos(diffRad);
}

/**
 * Find the CloudCell closest to a target altitude in feet.
 */
function findCellAtAltitude(
  cloud: CloudCell[],
  altitudeFt: number,
): CloudCell | null {
  if (cloud.length === 0) return null;
  let closest = cloud[0];
  let minDiff = Math.abs(cloud[0].mslFt - altitudeFt);
  for (const cell of cloud) {
    const diff = Math.abs(cell.mslFt - altitudeFt);
    if (diff < minDiff) {
      minDiff = diff;
      closest = cell;
    }
  }
  return closest;
}

/**
 * Find the CloudColumn closest to a target time.
 */
export function closestColumnByTime(
  weatherData: CloudColumn[],
  targetTime: Date,
): CloudColumn | null {
  if (weatherData.length === 0) return null;
  let closest = weatherData[0];
  let minDiff = Math.abs(weatherData[0].date.getTime() - targetTime.getTime());
  for (const col of weatherData) {
    const diff = Math.abs(col.date.getTime() - targetTime.getTime());
    if (diff < minDiff) {
      minDiff = diff;
      closest = col;
    }
  }
  return closest;
}

/**
 * Compute bearings and estimated time-over for each waypoint.
 * getWeatherDataForPoint returns the forecast CloudColumn[] for a given waypoint index.
 */
export function computeTimings(
  waypoints: RouteWaypoint[],
  getWeatherDataForPoint: (index: number) => CloudColumn[],
  cruiseAltitudeFt: number,
  tasKnots: number,
  departureTime: Date,
): Array<{ waypoint: RouteWaypoint; estimatedTimeOver: Date; bearingDeg: number }> {
  const results: Array<{
    waypoint: RouteWaypoint;
    estimatedTimeOver: Date;
    bearingDeg: number;
  }> = [];

  let currentTime = departureTime;

  for (let i = 0; i < waypoints.length; i++) {
    const wp = waypoints[i];
    const bearing =
      i < waypoints.length - 1
        ? forwardBearing(
            wp.latitude,
            wp.longitude,
            waypoints[i + 1].latitude,
            waypoints[i + 1].longitude,
          )
        : results.length > 0
          ? results[results.length - 1].bearingDeg
          : 0;

    results.push({
      waypoint: wp,
      estimatedTimeOver: new Date(currentTime),
      bearingDeg: bearing,
    });

    // Compute time to next point
    if (i < waypoints.length - 1) {
      const legDist = waypoints[i + 1].distanceNM - wp.distanceNM;
      const weatherData = getWeatherDataForPoint(i);
      const column = closestColumnByTime(weatherData, currentTime);
      let gs = tasKnots;

      if (column) {
        const cell = findCellAtAltitude(column.cloud, cruiseAltitudeFt);
        if (cell && isFinite(cell.windSpeed) && isFinite(cell.windDirection)) {
          const tw = tailwindComponent(cell.windSpeed, cell.windDirection, bearing);
          gs = tasKnots + tw;
          if (gs < 10) gs = 10; // Safety floor to prevent division by near-zero
        }
      }

      const hoursToNext = legDist / gs;
      currentTime = new Date(currentTime.getTime() + hoursToNext * 3600 * 1000);
    }
  }

  return results;
}
```

Note: `toRad` and `forwardBearing` are already defined in the file from Task 3.

- [ ] **Step 4: Run tests to verify they pass**

Run: `yarn test src/__tests__/route.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/utils/route.ts src/__tests__/route.test.ts
git commit -m "feat(route): add groundspeed and time-over calculation"
```

---

## Task 6: Route Server Actions

**Files:**
- Create: `src/app/actions/route.ts`

- [ ] **Step 1: Implement fetchBatchElevationAction**

Create `src/app/actions/route.ts`:

```typescript
"use server";

import { FEET_PER_METER, LOCATIONS, MODEL_CONFIGS, MODEL_NAMES } from "@/config/weather";
import type { RouteWaypoint, CloudColumn, WeatherModel, RoutePoint } from "@/types/weather";
import { parseWaypointString, generateRouteSamplePoints, computeTimings, closestColumnByTime } from "@/utils/route";
import { fetchWeatherDataAction } from "./weather";
import { transformWeatherData } from "@/utils/weather";

import { MAX_VARIABLES_PER_REQUEST } from "@/config/weather";
import chunk from "lodash/chunk";

/**
 * Fetch elevation for multiple coordinates in a single API call.
 * Open-Meteo elevation API accepts comma-separated coordinate lists.
 */
export async function fetchBatchElevationAction(
  points: Array<{ latitude: number; longitude: number }>,
): Promise<Array<number | null>> {
  if (points.length === 0) return [];

  const lats = points.map((p) => p.latitude).join(",");
  const lons = points.map((p) => p.longitude).join(",");
  const url = `https://api.open-meteo.com/v1/elevation?latitude=${lats}&longitude=${lons}`;

  try {
    const response = await fetch(url, { next: { revalidate: 3600 * 24 } });
    if (!response.ok) {
      console.error(`Batch elevation API error: ${response.status}`);
      return points.map(() => null);
    }
    const data = await response.json();
    if (data?.elevation && Array.isArray(data.elevation)) {
      return data.elevation.map((e: number) =>
        isFinite(e) ? e * FEET_PER_METER : null,
      );
    }
    return points.map(() => null);
  } catch (error) {
    console.error("Failed to fetch batch elevation:", error);
    return points.map(() => null);
  }
}

/**
 * Resolve waypoint string to coordinates.
 */
export async function resolveRouteWaypoints(
  waypointString: string,
  resolutionNM: number,
): Promise<RouteWaypoint[]> {
  const parsed = parseWaypointString(waypointString);

  const resolved = parsed.map((wp) => {
    if (wp.identifier.includes("@")) {
      const [name, coordStr] = wp.identifier.split("@");
      const [lat, lon] = coordStr.split(",").map(Number);
      if (isNaN(lat) || isNaN(lon)) {
        throw new Error(`Invalid coordinates for waypoint: ${wp.identifier}`);
      }
      return { name, latitude: lat, longitude: lon };
    }
    const loc = LOCATIONS[wp.identifier.toUpperCase()];
    if (!loc) {
      throw new Error(`Unknown location: ${wp.identifier}`);
    }
    return { name: wp.name, latitude: loc.latitude, longitude: loc.longitude };
  });

  return generateRouteSamplePoints(resolved, resolutionNM);
}

/**
 * Fetch weather data for a single route point.
 * Uses start_date/end_date for narrower time window.
 */
async function fetchWeatherForPoint(
  latitude: number,
  longitude: number,
  model: WeatherModel,
  startDate: string,
  endDate: string,
): Promise<CloudColumn[]> {
  const modelConfig = MODEL_CONFIGS[model];
  const allVars = modelConfig.getAllVariables();
  const varChunks = chunk(allVars, MAX_VARIABLES_PER_REQUEST);

  const locationStr = `Point@${latitude},${longitude}`;

  // Build params with narrow date window to reduce payload
  const responses = await Promise.all(
    varChunks.map(async (vars) => {
      const params: Record<string, any> = {
        cell_selection: "nearest",
        latitude,
        longitude,
        models: model,
        [modelConfig.varsKey]: vars.join(","),
        start_date: startDate,
        end_date: endDate,
        timezone: "UTC",
      };
      const { fetchWeatherApi } = await import("openmeteo");
      return fetchWeatherApi("https://api.open-meteo.com/v1/forecast", params);
    }),
  );

  if (responses.some((res) => !res || !res[0])) {
    throw new Error(`No weather data for point ${latitude},${longitude}`);
  }

  return transformWeatherData(
    responses.map((res) => res[0]),
    model,
    latitude,
  );
}

/**
 * Fetch weather and elevation for all route points.
 * Processes weather fetches in batches of 10 for rate limiting.
 */
export async function fetchRouteWeatherAction(
  waypoints: RouteWaypoint[],
  model: WeatherModel,
  departureTime: Date,
  cruiseAltitudeFt: number,
  tasKnots: number,
): Promise<{
  crossSectionData: CloudColumn[];
  elevations: Array<number | null>;
  routePoints: Array<{ estimatedTimeOver: Date; bearingDeg: number }>;
}> {
  // Date range for narrow fetch window
  const startDate = departureTime.toISOString().split("T")[0];
  // Rough estimate: max 10 hours of flight time
  const endDate = new Date(departureTime.getTime() + 24 * 3600 * 1000)
    .toISOString()
    .split("T")[0];

  // Fetch elevation for all points in one batch
  const elevationsPromise = fetchBatchElevationAction(waypoints);

  // Fetch weather in batches of 10
  const BATCH_SIZE = 10;
  const weatherBatches = chunk(waypoints, BATCH_SIZE);
  const allWeatherData: Array<CloudColumn[] | null> = [];

  for (const batch of weatherBatches) {
    const batchResults = await Promise.allSettled(
      batch.map((wp) =>
        fetchWeatherForPoint(wp.latitude, wp.longitude, model, startDate, endDate),
      ),
    );
    for (const result of batchResults) {
      allWeatherData.push(result.status === "fulfilled" ? result.value : null);
    }
  }

  const elevations = await elevationsPromise;

  // Compute timings
  const timingResults = computeTimings(
    waypoints,
    (index) => allWeatherData[index] ?? [],
    cruiseAltitudeFt,
    tasKnots,
    departureTime,
  );

  // Assemble cross-section: pick closest column to estimated time-over
  const crossSectionData: CloudColumn[] = waypoints.map((_, i) => {
    const weatherData = allWeatherData[i];
    const timing = timingResults[i];
    if (!weatherData || weatherData.length === 0) {
      // Return empty column for failed fetches
      return { date: timing.estimatedTimeOver, cloud: [], groundTemp: 0 };
    }
    const column = closestColumnByTime(weatherData, timing.estimatedTimeOver);
    return column ?? { date: timing.estimatedTimeOver, cloud: [], groundTemp: 0 };
  });

  return {
    crossSectionData,
    elevations,
    routePoints: timingResults.map((t) => ({
      estimatedTimeOver: t.estimatedTimeOver,
      bearingDeg: t.bearingDeg,
    })),
  };
}
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/app/actions/route.ts
git commit -m "feat(route): add route server actions for weather and elevation fetching"
```

---

## Task 7: Route Scales Hook

**Files:**
- Create: `src/hooks/useRouteScales.ts`

- [ ] **Step 1: Create useRouteScales hook**

Reference existing pattern from `src/hooks/useMeteogramScales.ts`. The route hook uses `scaleLinear` for X (distance) instead of `scaleTime`.

Create `src/hooks/useRouteScales.ts`:

```typescript
import { useMemo } from "react";
import { scaleLinear } from "@visx/scale";
import type { RouteWaypoint } from "../types/weather";

interface Bounds {
  xMax: number;
  yMax: number;
}

export interface RouteScales {
  distanceScale: ReturnType<typeof scaleLinear<number>>;
  mslScale: ReturnType<typeof scaleLinear<number>>;
  cloudScale: ReturnType<typeof scaleLinear<number>>;
}

export const useRouteScales = (
  waypoints: RouteWaypoint[],
  bounds: Bounds,
  clampCloudCoverageAt50Pct: boolean,
): RouteScales => {
  return useMemo(() => {
    const maxDistance =
      waypoints.length > 0
        ? waypoints[waypoints.length - 1].distanceNM
        : 100;

    return {
      distanceScale: scaleLinear<number>({
        domain: [0, maxDistance],
        range: [0, bounds.xMax],
      }),
      mslScale: scaleLinear<number>({
        domain: [0, 20_000],
        range: [bounds.yMax, 0],
      }),
      cloudScale: scaleLinear<number>({
        domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
        range: [0, 1],
        clamp: true,
      }),
    };
  }, [bounds, waypoints, clampCloudCoverageAt50Pct]);
};
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/hooks/useRouteScales.ts
git commit -m "feat(route): add useRouteScales hook for distance × altitude scales"
```

---

## Task 8: Terrain Profile Component

**Files:**
- Create: `src/app/components/terrain-profile.tsx`

- [ ] **Step 1: Create TerrainProfile component**

```typescript
"use client";

import React from "react";
import { formatNumber } from "../../utils/meteogram";
import type { RouteWaypoint } from "../../types/weather";
import type { RouteScales } from "../../hooks/useRouteScales";

interface TerrainProfileProps {
  waypoints: RouteWaypoint[];
  elevations: Array<number | null>;
  scales: RouteScales;
  yMax: number; // Bottom of the chart area in SVG coords
}

const TerrainProfile: React.FC<TerrainProfileProps> = ({
  waypoints,
  elevations,
  scales,
  yMax,
}) => {
  const pathD = React.useMemo(() => {
    if (waypoints.length === 0) return "";

    const points = waypoints.map((wp, i) => {
      const x = formatNumber(scales.distanceScale(wp.distanceNM));
      const elev = elevations[i] ?? 0;
      const y = formatNumber(scales.mslScale(Math.max(0, elev)));
      return `${x} ${y}`;
    });

    // Close the polygon along the bottom
    const firstX = formatNumber(scales.distanceScale(waypoints[0].distanceNM));
    const lastX = formatNumber(
      scales.distanceScale(waypoints[waypoints.length - 1].distanceNM),
    );

    return `M ${points[0]} ${points.slice(1).map((p) => `L ${p}`).join(" ")} L ${lastX} ${yMax} L ${firstX} ${yMax} Z`;
  }, [waypoints, elevations, scales, yMax]);

  if (!pathD) return null;

  return (
    <path
      d={pathD}
      fill="#8B7355"
      fillOpacity={0.6}
      stroke="#6B5B45"
      strokeWidth={1}
      pointerEvents="none"
    />
  );
};

export default React.memo(TerrainProfile);
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/app/components/terrain-profile.tsx
git commit -m "feat(route): add terrain profile SVG component"
```

---

## Task 9: Route Cloud Columns Component

**Files:**
- Create: `src/app/components/route-cloud-columns.tsx`

- [ ] **Step 1: Create RouteCloudColumns component**

Parallel to `src/app/components/cloud-columns.tsx` but uses `distanceScale` instead of `dateScale`. Wind barb thinning is distance-based (every other sample point) instead of time-based.

```typescript
"use client";

import React from "react";
import { Group } from "@visx/group";
import type { CloudColumn, CloudCell, RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";
import WindBarb from "./wind-barb";

interface RouteCloudColumnsProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  pressureLevels: number[];
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showWindBarbs: boolean;
  windBarbPointStep: number;
  windBarbPressureLevelStep: number;
  onHover: (index: number | null, cloudCell: CloudCell | null) => void;
}

const RouteCloudColumns: React.FC<RouteCloudColumnsProps> = ({
  crossSectionData,
  waypoints,
  scales,
  pressureLevels,
  highlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  showWindBarbs,
  windBarbPointStep,
  windBarbPressureLevelStep,
  onHover,
}) => {
  const pressureLevelsSet = React.useMemo(
    () => new Set(pressureLevels),
    [pressureLevels],
  );

  // Compute bar width based on distance between adjacent points
  const getBarWidth = React.useCallback(
    (index: number): number => {
      if (waypoints.length < 2) return 10;
      const prevDist = index > 0 ? waypoints[index - 1].distanceNM : waypoints[0].distanceNM;
      const nextDist = index < waypoints.length - 1 ? waypoints[index + 1].distanceNM : waypoints[waypoints.length - 1].distanceNM;
      const midLeft = index > 0 ? (prevDist + waypoints[index].distanceNM) / 2 : waypoints[index].distanceNM;
      const midRight = index < waypoints.length - 1 ? (waypoints[index].distanceNM + nextDist) / 2 : waypoints[index].distanceNM;
      return Math.max(1, scales.distanceScale(midRight) - scales.distanceScale(midLeft));
    },
    [waypoints, scales],
  );

  return (
    <>
      {crossSectionData.map((col, colIndex) => {
        if (!col.cloud || col.cloud.length === 0) return null;
        const wp = waypoints[colIndex];
        if (!wp) return null;

        const x = scales.distanceScale(wp.distanceNM);
        const barWidth = getBarWidth(colIndex);
        const filteredClouds = col.cloud.filter(
          (c) => c.hpa != null && pressureLevelsSet.has(c.hpa),
        );

        return (
          <Group key={`route-col-${colIndex}`} left={formatNumber(x - barWidth / 2)}>
            {filteredClouds.map((cloud) => {
              const coverage = clampCloudCoverageAt50Pct
                ? Math.min(cloud.cloudCoverage, 50)
                : cloud.cloudCoverage;
              const fillColor =
                cloud.cloudCoverage > 50 && highlightCeilingCoverage
                  ? `rgba(200, 200, 200, ${formatNumber(scales.cloudScale(coverage))})`
                  : `rgba(255, 255, 255, ${formatNumber(scales.cloudScale(coverage))})`;

              return (
                <rect
                  key={`cloud-${cloud.hpa}`}
                  x={0}
                  y={formatNumber(scales.mslScale(cloud.mslFtTop))}
                  width={formatNumber(barWidth)}
                  height={formatNumber(
                    scales.mslScale(cloud.mslFtBottom) - scales.mslScale(cloud.mslFtTop),
                  )}
                  fill={fillColor}
                  stroke="transparent"
                  strokeWidth={0}
                  onMouseEnter={() => onHover(colIndex, cloud)}
                  onMouseLeave={() => onHover(null, null)}
                />
              );
            })}
          </Group>
        );
      })}

      {/* Wind Barbs - distance-based thinning */}
      {showWindBarbs &&
        crossSectionData.map((col, colIndex) => {
          if (colIndex % windBarbPointStep !== 0) return null;
          const wp = waypoints[colIndex];
          if (!wp || !col.cloud) return null;
          const x = scales.distanceScale(wp.distanceNM);

          return col.cloud
            .filter(
              (cloud, levelIndex) =>
                pressureLevelsSet.has(cloud.hpa) &&
                levelIndex % windBarbPressureLevelStep === 0 &&
                isFinite(cloud.windSpeed) &&
                isFinite(cloud.windDirection),
            )
            .map((cloud) => (
              <WindBarb
                key={`wb-${colIndex}-${cloud.hpa}`}
                x={formatNumber(x)}
                y={formatNumber(scales.mslScale(cloud.geopotentialFt))}
                speed={cloud.windSpeed}
                direction={cloud.windDirection}
              />
            ));
        })}
    </>
  );
};

export default React.memo(RouteCloudColumns);
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/app/components/route-cloud-columns.tsx
git commit -m "feat(route): add RouteCloudColumns visualization component"
```

---

## Task 10: Route Weather Lines & Route Pressure Lines

**Files:**
- Create: `src/app/components/route-weather-lines.tsx`
- Create: `src/app/components/route-pressure-lines.tsx`

- [ ] **Step 1: Create RouteWeatherLines component**

Parallel to `src/app/components/weather-lines.tsx`. Uses same `findIsothermPoints`/`findIsotachPoints`/`findDewPointDepressionPoints`/`findFreezingLevels` utilities. Maps column index to distance via `waypoints[point.x].distanceNM` instead of `weatherData[point.x].date`.

Create `src/app/components/route-weather-lines.tsx`:

```typescript
"use client";

import React from "react";
import type { CloudColumn, RouteWaypoint } from "../../types/weather";
import {
  formatNumber,
  getTemperatureColor,
  getWindSpeedColor,
  findFreezingLevels,
  findIsothermPoints,
  findIsotachPoints,
  findDewPointDepressionPoints,
} from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";

const DEW_POINT_DEPRESSION_COLORS: Record<number, string> = {
  0: "#FF00FF",
  1: "#FF00FF",
  3: "#00CED1",
  5: "#FFD700",
  10: "#FF8C00",
};

const getDewPointDepressionColor = (spread: number): string => {
  return DEW_POINT_DEPRESSION_COLORS[spread] || "#888888";
};

interface RouteWeatherLinesProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  showIsothermLines: boolean;
  showIsotachLines: boolean;
  showDewPointDepressionLines: boolean;
  maxStepDistance: number;
}

const RouteWeatherLines: React.FC<RouteWeatherLinesProps> = ({
  crossSectionData,
  waypoints,
  scales,
  showIsothermLines,
  showIsotachLines,
  showDewPointDepressionLines,
  maxStepDistance,
}) => {
  const getX = React.useCallback(
    (columnIndex: number) => {
      const wp = waypoints[columnIndex];
      return wp ? formatNumber(scales.distanceScale(wp.distanceNM)) : "0";
    },
    [waypoints, scales],
  );

  const freezingPaths = React.useMemo(() => {
    const levels = findFreezingLevels(crossSectionData);
    return levels.map(({ points }) => {
      if (!points.length) return null;
      return points.reduce((path: string, point, i: number) => {
        const x = getX(point.x);
        const y = formatNumber(scales.mslScale(point.y));
        return i === 0 ? `M ${x} ${y}` : `${path} L ${x} ${y}`;
      }, "");
    });
  }, [crossSectionData, scales, getX]);

  const isothermPaths = React.useMemo(() => {
    if (!showIsothermLines) return [];
    return findIsothermPoints(crossSectionData, 2, 500, maxStepDistance).map(
      ({ temp, points }) => {
        if (!points.length) return null;
        const pathD = points.reduce((path: string, point, i: number) => {
          const x = getX(point.x);
          const y = formatNumber(scales.mslScale(point.y));
          return i === 0 ? `M ${x} ${y}` : `${path} L ${x} ${y}`;
        }, "");
        return { temp, pathD, color: getTemperatureColor(temp) };
      },
    );
  }, [crossSectionData, showIsothermLines, maxStepDistance, scales, getX]);

  const isotachPaths = React.useMemo(() => {
    if (!showIsotachLines) return [];
    return findIsotachPoints(crossSectionData, 10, 500, maxStepDistance).map(
      ({ speedKnots, points }) => {
        if (!points.length) return null;
        const pathD = points.reduce((path: string, point, i: number) => {
          const x = getX(point.x);
          const y = formatNumber(scales.mslScale(point.y));
          return i === 0 ? `M ${x} ${y}` : `${path} L ${x} ${y}`;
        }, "");
        return { speedKnots, pathD, color: getWindSpeedColor(speedKnots) };
      },
    );
  }, [crossSectionData, showIsotachLines, maxStepDistance, scales, getX]);

  const dewPointPaths = React.useMemo(() => {
    if (!showDewPointDepressionLines) return [];
    return findDewPointDepressionPoints(crossSectionData, [0, 1, 3, 5, 10]).map(
      ({ spread, points }) => {
        if (!points.length) return null;
        const pathD = points.reduce((path: string, point, i: number) => {
          const x = getX(point.x);
          const y = formatNumber(scales.mslScale(point.y));
          return i === 0 ? `M ${x} ${y}` : `${path} L ${x} ${y}`;
        }, "");
        return { spread, pathD, color: getDewPointDepressionColor(spread) };
      },
    );
  }, [crossSectionData, showDewPointDepressionLines, scales, getX]);

  return (
    <g pointerEvents="none">
      {/* Freezing levels */}
      {freezingPaths.map(
        (pathD, i) =>
          pathD && (
            <path
              key={`freeze-${i}`}
              d={pathD}
              stroke="blue"
              strokeWidth={2}
              strokeDasharray="6,3"
              fill="none"
            />
          ),
      )}

      {/* Isotherms */}
      {isothermPaths.map(
        (item, i) =>
          item && (
            <path
              key={`isotherm-${i}`}
              d={item.pathD}
              stroke={item.color}
              strokeWidth={1}
              opacity={0.7}
              fill="none"
            />
          ),
      )}

      {/* Isotachs */}
      {isotachPaths.map(
        (item, i) =>
          item && (
            <path
              key={`isotach-${i}`}
              d={item.pathD}
              stroke={item.color}
              strokeWidth={1.5}
              opacity={0.7}
              fill="none"
            />
          ),
      )}

      {/* Dew point depression */}
      {dewPointPaths.map(
        (item, i) =>
          item && (
            <path
              key={`dewpt-${i}`}
              d={item.pathD}
              stroke={item.color}
              strokeWidth={1}
              opacity={0.6}
              fill="none"
            />
          ),
      )}
    </g>
  );
};

export default React.memo(RouteWeatherLines);
```

- [ ] **Step 2: Create RoutePressureLines component**

Create `src/app/components/route-pressure-lines.tsx`:

```typescript
"use client";

import React from "react";
import type { CloudColumn, RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";

interface RoutePressureLinesProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  pressureLevels: number[];
}

const RoutePressureLines: React.FC<RoutePressureLinesProps> = ({
  crossSectionData,
  waypoints,
  scales,
  pressureLevels,
}) => {
  const pressureLinesData = React.useMemo(() => {
    return pressureLevels.map((hpa) => {
      const points = crossSectionData.map((col, i) => {
        const cloud = col.cloud.find((c) => c.hpa === hpa);
        const wp = waypoints[i];
        return {
          x: formatNumber(wp ? scales.distanceScale(wp.distanceNM) : 0),
          y: formatNumber(scales.mslScale(cloud?.geopotentialFt || 0)),
        };
      });

      const pathD = points.reduce((path: string, point, i: number) => {
        if (i === 0) return `M ${point.x} ${point.y}`;
        return `${path} L ${point.x} ${point.y}`;
      }, "");

      return { hpa, pathD, points };
    });
  }, [crossSectionData, waypoints, scales, pressureLevels]);

  return (
    <>
      {pressureLinesData.map(({ hpa, pathD, points }) => (
        <g key={`pressure-${hpa}`} pointerEvents="none">
          <path
            d={pathD}
            stroke="gray"
            strokeWidth={1}
            strokeDasharray="4,4"
            opacity={0.5}
            fill="none"
          />
          {points.length > 0 && (
            <g>
              <line x1={0} x2={6} y1={points[0].y} y2={points[0].y} stroke="black" />
              <text x={9} y={points[0].y} fontSize={11} fill="black" dominantBaseline="middle">
                {hpa}
              </text>
            </g>
          )}
        </g>
      ))}
    </>
  );
};

export default React.memo(RoutePressureLines);
```

- [ ] **Step 3: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/app/components/route-weather-lines.tsx src/app/components/route-pressure-lines.tsx
git commit -m "feat(route): add RouteWeatherLines and RoutePressureLines components"
```

---

## Task 11: Altitude Line & Waypoint Markers

**Files:**
- Create: `src/app/components/altitude-line.tsx`
- Create: `src/app/components/waypoint-markers.tsx`

- [ ] **Step 1: Create AltitudeLine component**

```typescript
"use client";

import React from "react";
import type { CloudColumn, RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";
import WindBarb from "./wind-barb";

interface AltitudeLineProps {
  cruiseAltitudeFt: number;
  waypoints: RouteWaypoint[];
  crossSectionData: CloudColumn[];
  scales: RouteScales;
  showWindBarbs: boolean;
}

const AltitudeLine: React.FC<AltitudeLineProps> = ({
  cruiseAltitudeFt,
  waypoints,
  crossSectionData,
  scales,
  showWindBarbs,
}) => {
  const y = formatNumber(scales.mslScale(cruiseAltitudeFt));
  const x1 = formatNumber(scales.distanceScale(waypoints[0]?.distanceNM ?? 0));
  const x2 = formatNumber(
    scales.distanceScale(waypoints[waypoints.length - 1]?.distanceNM ?? 0),
  );

  // Find wind at cruise altitude for barbs along the line
  const windBarbs = React.useMemo(() => {
    if (!showWindBarbs) return [];
    return crossSectionData
      .map((col, i) => {
        const wp = waypoints[i];
        if (!wp || i % 3 !== 0) return null; // Every 3rd point
        const cell = col.cloud.reduce((closest, c) => {
          return Math.abs(c.mslFt - cruiseAltitudeFt) <
            Math.abs(closest.mslFt - cruiseAltitudeFt)
            ? c
            : closest;
        }, col.cloud[0]);
        if (!cell || !isFinite(cell.windSpeed) || !isFinite(cell.windDirection))
          return null;
        return {
          x: scales.distanceScale(wp.distanceNM),
          speed: cell.windSpeed,
          direction: cell.windDirection,
        };
      })
      .filter(Boolean) as Array<{ x: number; speed: number; direction: number }>;
  }, [crossSectionData, waypoints, cruiseAltitudeFt, showWindBarbs, scales]);

  return (
    <g pointerEvents="none">
      <line
        x1={x1}
        y1={y}
        x2={x2}
        y2={y}
        stroke="#FF4500"
        strokeWidth={2}
        strokeDasharray="8,4"
        opacity={0.8}
      />
      <text
        x={Number(x2) + 5}
        y={y}
        fontSize={10}
        fill="#FF4500"
        dominantBaseline="middle"
      >
        {cruiseAltitudeFt} ft
      </text>
      {windBarbs.map((barb, i) => (
        <WindBarb
          key={`alt-wb-${i}`}
          x={formatNumber(barb.x)}
          y={y}
          speed={barb.speed}
          direction={barb.direction}
        />
      ))}
    </g>
  );
};

export default React.memo(AltitudeLine);
```

- [ ] **Step 2: Create WaypointMarkers component**

```typescript
"use client";

import React from "react";
import type { RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";

interface WaypointMarkersProps {
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  yMax: number;
  model: string;
}

const WaypointMarkers: React.FC<WaypointMarkersProps> = ({
  waypoints,
  scales,
  yMax,
  model,
}) => {
  const userWaypoints = React.useMemo(
    () => waypoints.filter((wp) => wp.isUserDefined),
    [waypoints],
  );

  return (
    <g pointerEvents="none">
      {userWaypoints.map((wp) => {
        const x = formatNumber(scales.distanceScale(wp.distanceNM));
        return (
          <g key={`marker-${wp.name}-${wp.distanceNM}`}>
            <line
              x1={x}
              y1={0}
              x2={x}
              y2={yMax}
              stroke="#333"
              strokeWidth={1}
              strokeDasharray="4,4"
              opacity={0.5}
            />
            <a href={`/${encodeURIComponent(wp.name)}/${model}`}>
              <text
                x={x}
                y={-8}
                fontSize={11}
                fontWeight="bold"
                fill="#333"
                textAnchor="middle"
                style={{ cursor: "pointer" }}
              >
                {wp.name}
              </text>
            </a>
          </g>
        );
      })}
    </g>
  );
};

export default React.memo(WaypointMarkers);
```

- [ ] **Step 3: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/app/components/altitude-line.tsx src/app/components/waypoint-markers.tsx
git commit -m "feat(route): add altitude line and waypoint marker components"
```

---

## Task 12: Route Meteogram Container

**Files:**
- Create: `src/app/components/route-meteogram.tsx`

- [ ] **Step 1: Create RouteMeteogram component**

This is the SVG container that composes all route visualization layers. Follows the pattern of `src/app/components/meteogram.tsx`.

```typescript
"use client";

import React, { useState, useCallback } from "react";
import { Group } from "@visx/group";
import { AxisLeft, AxisBottom } from "@visx/axis";
import type { CloudColumn, CloudCell, RouteWaypoint, WeatherModel } from "../../types/weather";
import { useRouteScales } from "../../hooks/useRouteScales";
import TerrainProfile from "./terrain-profile";
import RouteCloudColumns from "./route-cloud-columns";
import RouteWeatherLines from "./route-weather-lines";
import RoutePressureLines from "./route-pressure-lines";
import AltitudeLine from "./altitude-line";
import WaypointMarkers from "./waypoint-markers";
import MeteogramTooltip from "./meteogram-tooltip";
import { MODEL_CONFIGS } from "../../config/weather";
import { formatNumber } from "../../utils/meteogram";

export interface RouteMeteogramProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  elevations: Array<number | null>;
  cruiseAltitudeFt: number;
  width: number;
  height: number;
  margin?: { top: number; right: number; bottom: number; left: number };
  highlightCeilingCoverage?: boolean;
  clampCloudCoverageAt50Pct?: boolean;
  showPressureLines?: boolean;
  showWindBarbs?: boolean;
  showIsothermLines?: boolean;
  showIsotachLines?: boolean;
  showDewPointDepressionLines?: boolean;
  model: WeatherModel;
}

const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

const RouteMeteogram = React.memo(function RouteMeteogram({
  crossSectionData,
  waypoints,
  elevations,
  cruiseAltitudeFt,
  width,
  height,
  margin = defaultMargin,
  highlightCeilingCoverage = true,
  clampCloudCoverageAt50Pct = true,
  showPressureLines = false,
  showWindBarbs = true,
  showIsothermLines = false,
  showIsotachLines = false,
  showDewPointDepressionLines = false,
  model,
}: RouteMeteogramProps) {
  const [hoveredPoint, setHoveredPoint] = useState<{
    index: number;
    cloudCell: CloudCell;
  } | null>(null);

  const xMax = width - margin.left - margin.right;
  const yMax = height - margin.top - margin.bottom;

  const scales = useRouteScales(waypoints, { xMax, yMax }, clampCloudCoverageAt50Pct);

  const modelConfig = MODEL_CONFIGS[model];
  const pressureLevels = modelConfig.hpaLevels;

  // Conservative max step distance for contour interpolation
  const maxStepDistance = Math.min(6, Math.ceil(waypoints.length / 5) || 2);

  const handleHover = useCallback(
    (index: number | null, cloudCell: CloudCell | null) => {
      if (index !== null && cloudCell !== null) {
        setHoveredPoint({ index, cloudCell });
      } else {
        setHoveredPoint(null);
      }
    },
    [],
  );

  if (width < 10 || crossSectionData.length === 0) return null;

  return (
    <div style={{ position: "relative" }}>
      <svg width={width} height={height}>
        <rect x={0} y={0} width={width} height={height} fill="#87CEEB" rx={4} />
        <Group left={margin.left} top={margin.top}>
          <TerrainProfile
            waypoints={waypoints}
            elevations={elevations}
            scales={scales}
            yMax={yMax}
          />

          <RouteCloudColumns
            crossSectionData={crossSectionData}
            waypoints={waypoints}
            scales={scales}
            pressureLevels={pressureLevels}
            highlightCeilingCoverage={highlightCeilingCoverage}
            clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
            showWindBarbs={showWindBarbs}
            windBarbPointStep={2}
            windBarbPressureLevelStep={modelConfig.windBarbPressureLevelStep}
            onHover={handleHover}
          />

          <RouteWeatherLines
            crossSectionData={crossSectionData}
            waypoints={waypoints}
            scales={scales}
            showIsothermLines={showIsothermLines}
            showIsotachLines={showIsotachLines}
            showDewPointDepressionLines={showDewPointDepressionLines}
            maxStepDistance={maxStepDistance}
          />

          {showPressureLines && (
            <RoutePressureLines
              crossSectionData={crossSectionData}
              waypoints={waypoints}
              scales={scales}
              pressureLevels={pressureLevels}
            />
          )}

          <AltitudeLine
            cruiseAltitudeFt={cruiseAltitudeFt}
            waypoints={waypoints}
            crossSectionData={crossSectionData}
            scales={scales}
            showWindBarbs={showWindBarbs}
          />

          <WaypointMarkers
            waypoints={waypoints}
            scales={scales}
            yMax={yMax}
            model={model}
          />

          <AxisLeft
            scale={scales.mslScale}
            stroke="black"
            tickStroke="black"
            tickLabelProps={() => ({
              fill: "black",
              fontSize: 11,
              textAnchor: "end",
              dy: "0.33em",
            })}
          />

          <AxisBottom
            top={yMax}
            scale={scales.distanceScale}
            stroke="black"
            tickStroke="black"
            tickFormat={(v) => `${v} NM`}
            tickLabelProps={() => ({
              fill: "black",
              fontSize: 11,
              textAnchor: "middle",
            })}
          />
        </Group>
      </svg>

      {hoveredPoint && waypoints[hoveredPoint.index] && (
        <MeteogramTooltip
          date={crossSectionData[hoveredPoint.index]?.date ?? new Date()}
          cloudCell={hoveredPoint.cloudCell}
          x={scales.distanceScale(waypoints[hoveredPoint.index].distanceNM) + margin.left}
          y={scales.mslScale(hoveredPoint.cloudCell.mslFt) + margin.top}
          useLocalTime={false}
        />
      )}
    </div>
  );
});

export default RouteMeteogram;
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS (there may be minor type mismatches with MeteogramTooltip props — fix as needed during implementation)

- [ ] **Step 3: Commit**

```bash
git add src/app/components/route-meteogram.tsx
git commit -m "feat(route): add RouteMeteogram SVG container component"
```

---

## Task 13: Route Header UI

**Files:**
- Create: `src/app/components/route-header.tsx`

- [ ] **Step 1: Create RouteHeader component**

```typescript
"use client";

import React, { useState } from "react";
import { Button, Input } from "@heroui/react";
import { ModelDropdown } from "./model-dropdown";
import type { WeatherModel } from "../../types/weather";

interface RouteHeaderProps {
  waypoints: string;
  model: WeatherModel;
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: string; // ISO string for input value
  resolutionNM: number;
  onUpdate: (params: {
    waypoints: string;
    model: WeatherModel;
    cruiseAltitudeFt: number;
    tasKnots: number;
    departureTime: string;
    resolutionNM: number;
  }) => void;
  isLoading: boolean;
}

const RouteHeader: React.FC<RouteHeaderProps> = ({
  waypoints: initialWaypoints,
  model: initialModel,
  cruiseAltitudeFt: initialAlt,
  tasKnots: initialTas,
  departureTime: initialDep,
  resolutionNM: initialRes,
  onUpdate,
  isLoading,
}) => {
  const [waypoints, setWaypoints] = useState(initialWaypoints);
  const [model, setModel] = useState(initialModel);
  const [alt, setAlt] = useState(initialAlt.toString());
  const [tas, setTas] = useState(initialTas.toString());
  const [dep, setDep] = useState(initialDep);
  const [res, setRes] = useState(initialRes.toString());

  const handleUpdate = () => {
    onUpdate({
      waypoints,
      model,
      cruiseAltitudeFt: parseInt(alt, 10) || 6000,
      tasKnots: parseInt(tas, 10) || 120,
      departureTime: dep,
      resolutionNM: Math.max(5, parseInt(res, 10) || 25),
    });
  };

  return (
    <div className="flex flex-wrap items-center gap-2 p-3 bg-default-100 border-b border-default-200">
      <Input
        label="Route"
        value={waypoints}
        onValueChange={setWaypoints}
        size="sm"
        className="min-w-[200px] flex-1"
        placeholder="KCDW-SBJ-KBOS"
      />
      <ModelDropdown model={model} setModel={setModel} />
      <Input
        label="Alt (ft)"
        value={alt}
        onValueChange={setAlt}
        size="sm"
        type="number"
        className="w-24"
      />
      <Input
        label="TAS (kt)"
        value={tas}
        onValueChange={setTas}
        size="sm"
        type="number"
        className="w-24"
      />
      <Input
        label="Dep Time"
        value={dep}
        onValueChange={setDep}
        size="sm"
        type="datetime-local"
        className="w-48"
      />
      <Input
        label="Res (NM)"
        value={res}
        onValueChange={setRes}
        size="sm"
        type="number"
        className="w-20"
      />
      <Button
        color="primary"
        size="sm"
        onPress={handleUpdate}
        isLoading={isLoading}
      >
        Update
      </Button>
    </div>
  );
};

export default RouteHeader;
```

Note: `ModelDropdown` may need to be extracted from `nav.tsx` if it's not already a separate component. Check during implementation and extract if needed.

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS (may need adjustments for ModelDropdown import)

- [ ] **Step 3: Commit**

```bash
git add src/app/components/route-header.tsx
git commit -m "feat(route): add RouteHeader input UI component"
```

---

## Task 14: Route Client Wrapper

**Files:**
- Create: `src/app/components/route-client-wrapper.tsx`

- [ ] **Step 1: Create RouteClientWrapper component**

```typescript
"use client";

import React, { useState, useMemo, useCallback } from "react";
import { useRouter } from "next/navigation";
import { HeroUIProvider } from "@heroui/react";
import type { CloudColumn, RouteWaypoint, WeatherModel, VisualizationPreferences } from "../../types/weather";
import { PreferencesProvider, usePreferences } from "../../context/PreferencesContext";
import { serializeVisualizationPreferences, serializeRouteParams } from "../../utils/params";
import RouteHeader from "./route-header";
import RouteMeteogram from "./route-meteogram";
import { ParentSize } from "@visx/responsive";

interface RouteClientWrapperProps {
  initialWaypointString: string;
  initialModel: WeatherModel;
  initialCrossSectionDataStr: string;
  initialWaypoints: RouteWaypoint[];
  initialElevations: Array<number | null>;
  initialCruiseAltitudeFt: number;
  initialTasKnots: number;
  initialDepartureTime: string;
  initialResolutionNM: number;
  initialPreferences: VisualizationPreferences;
  cookieReadSuccess?: boolean;
}

function RouteClientWrapperInternal({
  initialWaypointString,
  initialModel,
  initialCrossSectionDataStr,
  initialWaypoints,
  initialElevations,
  initialCruiseAltitudeFt,
  initialTasKnots,
  initialDepartureTime,
  initialResolutionNM,
}: Omit<RouteClientWrapperProps, "initialPreferences" | "cookieReadSuccess">) {
  const router = useRouter();
  const { preferences } = usePreferences();

  const parsedInitialData = useMemo(() => {
    try {
      return JSON.parse(initialCrossSectionDataStr).map((col: any) => ({
        ...col,
        date: new Date(col.date),
      }));
    } catch {
      return [];
    }
  }, [initialCrossSectionDataStr]);

  const [isLoading, setIsLoading] = useState(false);

  const handleUpdate = useCallback(
    async (params: {
      waypoints: string;
      model: WeatherModel;
      cruiseAltitudeFt: number;
      tasKnots: number;
      departureTime: string;
      resolutionNM: number;
    }) => {
      setIsLoading(true);
      try {
        // Navigate to new URL
        const routeParams = serializeRouteParams({
          cruiseAltitudeFt: params.cruiseAltitudeFt,
          tasKnots: params.tasKnots,
          departureTime: new Date(params.departureTime),
          resolutionNM: params.resolutionNM,
        });
        const prefParams = serializeVisualizationPreferences(preferences);
        const allParams = new URLSearchParams([
          ...routeParams.entries(),
          ...prefParams.entries(),
        ]);
        router.push(
          `/route/${encodeURIComponent(params.waypoints)}/${params.model}?${allParams.toString()}`,
        );
      } catch (error) {
        console.error("Failed to update route:", error);
      } finally {
        setIsLoading(false);
      }
    },
    [router, preferences],
  );

  return (
    <>
      <RouteHeader
        waypoints={initialWaypointString}
        model={initialModel}
        cruiseAltitudeFt={initialCruiseAltitudeFt}
        tasKnots={initialTasKnots}
        departureTime={initialDepartureTime}
        resolutionNM={initialResolutionNM}
        onUpdate={handleUpdate}
        isLoading={isLoading}
      />
      <div className="flex-1">
        <ParentSize>
          {({ width }) => (
            <RouteMeteogram
              crossSectionData={parsedInitialData}
              waypoints={initialWaypoints}
              elevations={initialElevations}
              cruiseAltitudeFt={initialCruiseAltitudeFt}
              width={width}
              height={600}
              model={initialModel}
              highlightCeilingCoverage={preferences.highlightCeilingCoverage}
              clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
              showPressureLines={preferences.showPressureLines}
              showWindBarbs={preferences.showWindBarbs}
              showIsothermLines={preferences.showIsothermLines}
              showIsotachLines={preferences.showIsotachLines}
              showDewPointDepressionLines={preferences.showDewPointDepressionLines}
            />
          )}
        </ParentSize>
      </div>
    </>
  );
}

export default function RouteClientWrapper(props: RouteClientWrapperProps) {
  return (
    <HeroUIProvider>
      <PreferencesProvider
        initialPreferences={props.initialPreferences}
        cookieReadSuccess={props.cookieReadSuccess}
      >
        <RouteClientWrapperInternal
          initialWaypointString={props.initialWaypointString}
          initialModel={props.initialModel}
          initialCrossSectionDataStr={props.initialCrossSectionDataStr}
          initialWaypoints={props.initialWaypoints}
          initialElevations={props.initialElevations}
          initialCruiseAltitudeFt={props.initialCruiseAltitudeFt}
          initialTasKnots={props.initialTasKnots}
          initialDepartureTime={props.initialDepartureTime}
          initialResolutionNM={props.initialResolutionNM}
        />
      </PreferencesProvider>
    </HeroUIProvider>
  );
}
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/app/components/route-client-wrapper.tsx
git commit -m "feat(route): add RouteClientWrapper for client state management"
```

---

## Task 15: Route Page (Server Component)

**Files:**
- Create: `src/app/route/[waypoints]/[model]/page.tsx`

- [ ] **Step 1: Create the route page server component**

```typescript
import { notFound } from "next/navigation";
import type { WeatherModel } from "@/types/weather";
import { MODEL_NAMES } from "@/config/weather";
import { parseRouteParams, parseVisualizationPreferences } from "@/utils/params";
import { getInitialPreferences } from "@/utils/serverPreferences";
import { resolveRouteWaypoints, fetchRouteWeatherAction } from "@/app/actions/route";
import RouteClientWrapper from "@/app/components/route-client-wrapper";

export const dynamic = "force-dynamic";

interface PageProps {
  params: Promise<{
    waypoints: string;
    model: string;
  }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}

export default async function RoutePage({ params, searchParams }: PageProps) {
  const [{ waypoints: waypointString, model }, searchParamsResolved] =
    await Promise.all([params, searchParams]);

  const decodedWaypoints = decodeURIComponent(waypointString);

  if (!MODEL_NAMES.includes(model as WeatherModel)) {
    notFound();
  }

  // Parse route-specific params
  const flatParams: Record<string, string> = {};
  Object.entries(searchParamsResolved).forEach(([key, value]) => {
    if (typeof value === "string") flatParams[key] = value;
    else if (Array.isArray(value) && value.length > 0) flatParams[key] = value[0];
  });

  const routeParams = parseRouteParams(flatParams);
  const [preferencesResult] = await Promise.all([
    getInitialPreferences(searchParamsResolved),
  ]);

  // Resolve waypoints and fetch weather
  const resolvedWaypoints = await resolveRouteWaypoints(
    decodedWaypoints,
    routeParams.resolutionNM,
  );

  const { crossSectionData, elevations, routePoints } =
    await fetchRouteWeatherAction(
      resolvedWaypoints,
      model as WeatherModel,
      routeParams.departureTime,
      routeParams.cruiseAltitudeFt,
      routeParams.tasKnots,
    );

  return (
    <div className="min-h-screen flex flex-col">
      <RouteClientWrapper
        initialWaypointString={decodedWaypoints}
        initialModel={model as WeatherModel}
        initialCrossSectionDataStr={JSON.stringify(crossSectionData)}
        initialWaypoints={resolvedWaypoints}
        initialElevations={elevations}
        initialRoutePoints={routePoints}
        initialCruiseAltitudeFt={routeParams.cruiseAltitudeFt}
        initialTasKnots={routeParams.tasKnots}
        initialDepartureTime={routeParams.departureTime.toISOString()}
        initialResolutionNM={routeParams.resolutionNM}
        initialPreferences={preferencesResult.preferences}
        cookieReadSuccess={preferencesResult.cookieReadSuccess}
      />
    </div>
  );
}
```

- [ ] **Step 2: Verify types compile**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/app/route/[waypoints]/[model]/page.tsx
git commit -m "feat(route): add route page server component"
```

---

## Task 16: Navigation Integration

**Files:**
- Modify: `src/app/components/nav.tsx`

- [ ] **Step 1: Add Route link to navigation**

In `src/app/components/nav.tsx`, add a "Route" button/link in the NavbarContent section. The link should go to `/route/KCDW-KFRG/{currentModel}`.

Look for where the existing nav items are rendered and add:

```typescript
import Link from "next/link";

// In the navbar content, near the model/location dropdowns:
<Link
  href={`/route/KCDW-KFRG/${model}`}
  className="text-sm font-medium text-default-600 hover:text-primary"
>
  Route
</Link>
```

- [ ] **Step 2: Verify the app compiles and the link renders**

Run: `yarn ts && yarn dev`
Navigate to `http://localhost:3000/KCDW/gfs_hrrr` and verify the Route link appears.

- [ ] **Step 3: Commit**

```bash
git add src/app/components/nav.tsx
git commit -m "feat(route): add Route link to navigation bar"
```

---

## Task 17: Integration Test & End-to-End Verification

**Files:**
- Various

- [ ] **Step 1: Run all existing tests**

Run: `yarn test`
Expected: All existing tests pass (no regressions)

- [ ] **Step 2: Run type check**

Run: `yarn ts`
Expected: PASS

- [ ] **Step 3: Test the route view end-to-end**

Run: `yarn dev`
Navigate to: `http://localhost:3000/route/KCDW-KFRG/gfs_hrrr?alt=6000&tas=120`

Verify:
- Cross-section renders with cloud coverage
- Terrain profile visible along the bottom
- Altitude line at 6000 ft
- Waypoint markers at KCDW and KBOS
- Hovering shows tooltip
- Changing model in header and clicking Update navigates correctly
- Existing location view still works at `/KCDW/gfs_hrrr`
- Route link in nav works from location view

- [ ] **Step 4: Test with intermediate waypoints**

Navigate to: `http://localhost:3000/route/KCDW-KNYC-KFRG/gfs_hrrr`
Verify: Three waypoint markers visible, smooth interpolation between them.

- [ ] **Step 5: Fix any issues discovered during testing**

Address any TypeScript errors, rendering issues, or broken interactions.

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix(route): address integration issues from end-to-end testing"
```

---

## Task 18: Update AGENTS.md

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: Add route architecture documentation to AGENTS.md**

Add a new section covering:
- Route view URL structure
- New files and their purposes
- Route data flow (waypoint resolution → sample generation → weather fetch → timing calculation → cross-section assembly → visualization)
- Key constraints (50-point cap, concurrency limiting, wind sign convention)
- How to add/modify route features

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs: add en-route view architecture to AGENTS.md"
```
