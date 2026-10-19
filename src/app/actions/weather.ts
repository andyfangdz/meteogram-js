"use server";

import { transformWeatherData } from "@/utils/weather";
import {
  CloudColumn,
  WeatherModel,
  WeatherApiParams,
  Locations,
  WeatherResult,
} from "@/types/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  MODEL_CONFIGS,
  LOCATIONS,
  FEET_PER_METER,
  MAX_VARIABLES_PER_REQUEST,
} from "@/config/weather";
import { fetchWeatherApi } from "openmeteo";
import chunk from "lodash/chunk";
import { mergeForecastRuns } from "@/utils/forecast-runs";
import {
  parseNativeForecast,
  transformNativeForecast,
} from "@/utils/native-weather";

function locationCoordinates(location: string) {
  const coordinates = location.includes("@")
    ? (() => {
        const parts = location.slice(location.lastIndexOf("@") + 1).split(",");
        if (parts.length !== 2 || parts.some((v) => !v.trim()))
          return undefined;
        return { latitude: Number(parts[0]), longitude: Number(parts[1]) };
      })()
    : LOCATIONS[location.toUpperCase()];
  if (
    !coordinates ||
    !Number.isFinite(coordinates.latitude) ||
    !Number.isFinite(coordinates.longitude) ||
    Math.abs(coordinates.latitude) > 90 ||
    Math.abs(coordinates.longitude) > 180
  ) {
    throw new Error("Invalid coordinates for location: " + location);
  }
  return coordinates;
}

async function nativeWeather(
  model: WeatherModel,
  latitude: number,
  longitude: number,
) {
  const nativeModel = MODEL_CONFIGS[model].nativeModel;
  const base = process.env.METEOGRAM_API_URL;
  if (
    !nativeModel ||
    !base ||
    latitude < 24 ||
    latitude > 50 ||
    longitude < -125 ||
    longitude > -66
  )
    return null;
  const origin = new URL(base);
  if (
    origin.protocol !== "https:" &&
    !(
      origin.protocol === "http:" &&
      ["127.0.0.1", "localhost", "[::1]"].includes(origin.hostname)
    )
  ) {
    throw new Error("Native weather URL must use HTTPS or loopback HTTP");
  }
  const token = process.env.METEOGRAM_API_TOKEN;
  const now = Date.now();
  const load = async (run: string) => {
    const url = new URL("/forecast", origin);
    url.search = new URLSearchParams({
      model: nativeModel,
      lat: String(latitude),
      lon: String(longitude),
      run,
    }).toString();
    const response = await fetch(url, {
      cache: "no-store",
      signal: AbortSignal.timeout(5000),
      headers: token ? { Authorization: "Bearer " + token } : {},
      redirect: "error",
    });
    if (!response.ok)
      throw new Error("Native weather returned HTTP " + response.status);
    const forecast = parseNativeForecast(await response.json(), nativeModel);
    return transformNativeForecast(forecast, now, { allowSparse: true });
  };
  const selections =
    nativeModel === "hrrr"
      ? ["long", "latest_complete", "latest"]
      : ["latest_complete", "latest"];
  const responses = await Promise.allSettled(selections.map(load));
  const runs = responses.flatMap((r) =>
    r.status === "fulfilled" ? [r.value] : [],
  );
  if (!runs.length) throw new Error("Native forecast runs unavailable");
  const merged = mergeForecastRuns(runs, now);
  // Partial overlays may bypass the API's remaining-horizon gate. Check the
  // actual surviving intervals, including holes when a complete base failed.
  const minimumHours =
    nativeModel === "hrrr" ? 12 : nativeModel === "rrfs" ? 48 : 72;
  let coveredUntil = now;
  for (const column of merged.data) {
    const start = column.date.getTime();
    if (start > coveredUntil) break;
    coveredUntil = Math.max(
      coveredUntil,
      start + (column.durationMinutes ?? 60) * 60_000,
    );
  }
  if (coveredUntil < now + minimumHours * 3_600_000)
    throw new Error("Native forecast has insufficient continuous coverage");
  return merged;
}

/**
 * Server Action to fetch elevation data for a given coordinate.
 * Converts elevation from meters to feet.
 */
export async function fetchElevationAction(
  latitude: number,
  longitude: number,
): Promise<number | null> {
  const elevationUrl = `https://api.open-meteo.com/v1/elevation?latitude=${latitude}&longitude=${longitude}`;
  try {
    const response = await fetch(elevationUrl, {
      next: { revalidate: 3600 * 24 },
      signal: AbortSignal.timeout(4000),
    });
    if (!response.ok) {
      console.error(
        `Elevation API error: ${response.status} ${response.statusText}`,
      );
      return null;
    }
    const data = await response.json();
    if (Number.isFinite(data?.elevation?.[0])) {
      const elevationInMeters = data.elevation[0];
      return elevationInMeters * FEET_PER_METER;
    }
    return null;
  } catch (error) {
    console.error("Failed to fetch elevation data:", error);
    return null;
  }
}

/**
 * Server Action to fetch weather forecast data from OpenMeteo.
 * Handles both predefined locations and custom coordinates.
 */
export async function fetchWeatherDataAction(
  model: WeatherModel,
  location: string,
  variables?: string[],
): Promise<ReturnType<typeof fetchWeatherApi>> {
  const modelConfig = MODEL_CONFIGS[model];
  if (!modelConfig) {
    throw new Error(`Invalid weather model specified: ${model}`);
  }

  let coordinates: { latitude: number; longitude: number } | undefined;
  const hasCoordinates = location.includes("@");

  if (hasCoordinates) {
    const parts = location.split("@");
    const coordString = parts[1];
    const [latitudeStr, longitudeStr] = coordString.split(",");
    const latitude = parseFloat(latitudeStr);
    const longitude = parseFloat(longitudeStr);
    if (!isNaN(latitude) && !isNaN(longitude)) {
      coordinates = { latitude, longitude };
    } else {
      throw new Error(`Invalid coordinates in location string: ${location}`);
    }
  } else {
    const upperCaseLocation = location.toUpperCase();
    if (LOCATIONS[upperCaseLocation]) {
      coordinates = LOCATIONS[upperCaseLocation];
    } else {
      throw new Error(`Location '${location}' not found or invalid.`);
    }
  }

  if (!coordinates) {
    throw new Error(
      `Could not determine coordinates for location: ${location}`,
    );
  }

  const params: WeatherApiParams = {
    ...DEFAULT_PARAMS,
    latitude: coordinates.latitude,
    longitude: coordinates.longitude,
    models: modelConfig.fallbackModel ?? model,
    [modelConfig.varsKey]: variables
      ? variables.join(",")
      : modelConfig.getAllVariables().join(","),
    ...(modelConfig.stepKey &&
      modelConfig.stepSize != null && {
        [modelConfig.stepKey]: modelConfig.stepSize,
      }),
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  };

  try {
    const responses = await fetchWeatherApi(API_URL, params);
    return responses;
  } catch (error) {
    console.error("Failed to fetch weather API data:", error);
    throw error;
  }
}

export async function getWeatherData(
  model: WeatherModel,
  location: string,
): Promise<WeatherResult> {
  try {
    const coordinates = locationCoordinates(location);
    const modelConfig = MODEL_CONFIGS[model];
    if (!modelConfig) throw new Error("Invalid weather model: " + model);
    const elevationPromise = fetchElevationAction(
      coordinates.latitude,
      coordinates.longitude,
    );
    let native = null;
    try {
      native = await nativeWeather(
        model,
        coordinates.latitude,
        coordinates.longitude,
      );
    } catch {
      // The tunnel, schema, freshness and horizon gates all fail over as one
      // provider. Do not log response bodies, authentication headers or secrets.
      console.warn("Native weather unavailable; using Open-Meteo for", model);
    }
    if (native) {
      return {
        ...native,
        timestamp: new Date().toISOString(),
        elevationFt: await elevationPromise,
      };
    }

    // Fetch variables in chunks to avoid stack overflow in OpenMeteo SDK / flatbuffers
    // when handling large numbers of variables (>100).
    const allVars = modelConfig.getAllVariables();
    const varChunks = chunk(allVars, MAX_VARIABLES_PER_REQUEST);

    const [chunkResults, elevationFt] = await Promise.all([
      Promise.all(
        varChunks.map((vars) => fetchWeatherDataAction(model, location, vars)),
      ),
      elevationPromise,
    ]);

    if (chunkResults.some((res) => !res || !res[0])) {
      throw new Error("No weather data received for some chunks");
    }

    // Flatten to get one response object per chunk
    const responses = chunkResults.map((res) => res[0]);

    const transformedData = transformWeatherData(
      responses,
      model,
      coordinates.latitude,
    );

    return {
      data: transformedData,
      timestamp: new Date().toISOString(),
      elevationFt,
      source: {
        provider: "open-meteo",
        model: modelConfig.fallbackModel ?? model,
        cloudMethod: "model",
        cadenceMinutes: modelConfig.forecastDataKey === "minutely15" ? 15 : 60,
        fallback: Boolean(
          modelConfig.nativeModel && process.env.METEOGRAM_API_URL,
        ),
      },
    };
  } catch (error) {
    console.error("Error in getWeatherData:", error);
    throw new Error("Failed to process weather data.");
  }
}
