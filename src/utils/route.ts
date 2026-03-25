import type { RouteWaypoint, CloudColumn, CloudCell } from "../types/weather";

const EARTH_RADIUS_NM = 3440.065;

interface ParsedWaypoint {
  name: string;
  identifier: string;
}

function toRad(deg: number): number {
  return (deg * Math.PI) / 180;
}

function toDeg(rad: number): number {
  return (rad * 180) / Math.PI;
}

export function haversineDistanceNM(
  lat1: number, lon1: number, lat2: number, lon2: number,
): number {
  const dLat = toRad(lat2 - lat1);
  const dLon = toRad(lon2 - lon1);
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) * Math.sin(dLon / 2) ** 2;
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));
  return EARTH_RADIUS_NM * c;
}

export function forwardBearing(
  lat1: number, lon1: number, lat2: number, lon2: number,
): number {
  const φ1 = toRad(lat1);
  const φ2 = toRad(lat2);
  const Δλ = toRad(lon2 - lon1);
  const y = Math.sin(Δλ) * Math.cos(φ2);
  const x = Math.cos(φ1) * Math.sin(φ2) - Math.sin(φ1) * Math.cos(φ2) * Math.cos(Δλ);
  const θ = Math.atan2(y, x);
  return ((toDeg(θ) % 360) + 360) % 360;
}

export function interpolateGreatCircle(
  start: { lat: number; lon: number; name: string },
  end: { lat: number; lon: number; name: string },
  resolutionNM: number,
  startDistanceNM: number = 0,
): RouteWaypoint[] {
  const totalDist = haversineDistanceNM(start.lat, start.lon, end.lat, end.lon);
  const points: RouteWaypoint[] = [];

  points.push({
    name: start.name,
    latitude: start.lat,
    longitude: start.lon,
    distanceNM: startDistanceNM,
    isUserDefined: true,
  });

  if (totalDist <= resolutionNM) {
    points.push({
      name: end.name,
      latitude: end.lat,
      longitude: end.lon,
      distanceNM: startDistanceNM + totalDist,
      isUserDefined: true,
    });
    return points;
  }

  const φ1 = toRad(start.lat);
  const λ1 = toRad(start.lon);
  const φ2 = toRad(end.lat);
  const λ2 = toRad(end.lon);
  const d = totalDist / EARTH_RADIUS_NM;

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

  points.push({
    name: end.name,
    latitude: end.lat,
    longitude: end.lon,
    distanceNM: startDistanceNM + totalDist,
    isUserDefined: true,
  });

  return points;
}

export function parseWaypointString(waypointString: string): ParsedWaypoint[] {
  // Split on '-' but re-join parts that are bare numbers (negative coordinate continuations).
  // e.g. "KCDW-MySpot@41.5,-73.0-KFRG" → ["KCDW", "MySpot@41.5,-73.0", "KFRG"]
  const rawParts = waypointString.split("-").filter(Boolean);
  const parts: string[] = [];
  for (const raw of rawParts) {
    if (parts.length > 0 && /^[\d.]+$/.test(raw)) {
      // bare number — treat as the negative-longitude continuation of the previous part
      parts[parts.length - 1] += `-${raw}`;
    } else {
      parts.push(raw);
    }
  }
  if (parts.length < 2) {
    throw new Error("Route requires at least 2 waypoints");
  }
  return parts.map((part) => {
    const name = part.includes("@") ? part.split("@")[0] : part;
    return { name, identifier: part };
  });
}

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
  const windSpeedKnots = windSpeedKmh * 0.539957;
  const diffRad = toRad(windDirectionDeg - bearingDeg);
  return -windSpeedKnots * Math.cos(diffRad);
}

function findCellAtAltitude(cloud: CloudCell[], altitudeFt: number): CloudCell | null {
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

export function closestColumnByTime(
  weatherData: CloudColumn[],
  targetTime: Date,
): CloudColumn | null {
  if (weatherData.length === 0) return null;
  let closest = weatherData[0];
  let minDiff = Math.abs(weatherData[0].date.getTime() - targetTime.getTime());
  for (const col of weatherData) {
    const diff = Math.abs(col.date.getTime() - targetTime.getTime());
    if (diff < minDiff) { minDiff = diff; closest = col; }
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
  const results: Array<{ waypoint: RouteWaypoint; estimatedTimeOver: Date; bearingDeg: number }> = [];
  let currentTime = departureTime;

  for (let i = 0; i < waypoints.length; i++) {
    const wp = waypoints[i];
    const bearing = i < waypoints.length - 1
      ? forwardBearing(wp.latitude, wp.longitude, waypoints[i + 1].latitude, waypoints[i + 1].longitude)
      : results.length > 0 ? results[results.length - 1].bearingDeg : 0;

    results.push({ waypoint: wp, estimatedTimeOver: new Date(currentTime), bearingDeg: bearing });

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
          if (gs < 10) gs = 10; // Safety floor
        }
      }

      const hoursToNext = legDist / gs;
      currentTime = new Date(currentTime.getTime() + hoursToNext * 3600 * 1000);
    }
  }
  return results;
}

export function generateRouteSamplePoints(
  resolvedWaypoints: Array<{ name: string; latitude: number; longitude: number }>,
  resolutionNM: number,
  maxPoints: number = 30,
): RouteWaypoint[] {
  if (resolvedWaypoints.length < 2) {
    throw new Error("Route requires at least 2 waypoints");
  }

  let totalDistance = 0;
  for (let i = 0; i < resolvedWaypoints.length - 1; i++) {
    totalDistance += haversineDistanceNM(
      resolvedWaypoints[i].latitude, resolvedWaypoints[i].longitude,
      resolvedWaypoints[i + 1].latitude, resolvedWaypoints[i + 1].longitude,
    );
  }

  let effectiveRes = resolutionNM;
  const estimatedPoints = Math.ceil(totalDistance / resolutionNM) + resolvedWaypoints.length;
  if (estimatedPoints > maxPoints) {
    effectiveRes = totalDistance / (maxPoints - resolvedWaypoints.length);
  }

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

    if (i > 0) {
      legPoints.shift();
    }
    allPoints.push(...legPoints);

    cumulativeDistance += haversineDistanceNM(
      start.latitude, start.longitude, end.latitude, end.longitude,
    );
  }

  return allPoints;
}
