"use server";

import { FEET_PER_METER, LOCATIONS, MODEL_CONFIGS, MAX_VARIABLES_PER_REQUEST } from "@/config/weather";
import type { RouteWaypoint, CloudColumn, WeatherModel } from "@/types/weather";
import { parseWaypointString, generateRouteSamplePoints, computeTimings, closestColumnByTime } from "@/utils/route";
import { transformWeatherData } from "@/utils/weather";
import { fetchWeatherApi } from "openmeteo";
import chunk from "lodash/chunk";

/**
 * Fetch elevation for multiple coordinates in a single API call.
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
 * Fetch weather data for a single route point using narrow date window.
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

  const responses = await Promise.all(
    varChunks.map((vars) => {
      const params: Record<string, unknown> = {
        cell_selection: "nearest",
        latitude,
        longitude,
        models: model,
        [modelConfig.varsKey]: vars.join(","),
        start_date: startDate,
        end_date: endDate,
        timezone: "UTC",
      };
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
  const startDate = departureTime.toISOString().split("T")[0];
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
