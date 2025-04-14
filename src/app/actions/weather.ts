"use server";

import { transformWeatherData } from "@/utils/weather";
import {
  CloudColumn,
  WeatherModel,
  WeatherApiParams,
  Locations,
} from "@/types/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  MODEL_CONFIGS,
  LOCATIONS,
  FEET_PER_METER,
} from "@/config/weather";
import { fetchWeatherApi } from "openmeteo";

/**
 * Server Action to fetch elevation data for a given coordinate.
 * Converts elevation from meters to feet.
 */
export async function fetchElevationAction(
  latitude: number,
  longitude: number,
): Promise<number | null> {
  const elevationUrl = `https://api.open-meteo.com/v1/elevation?latitude=${latitude}&longitude=${longitude}`;
  console.log(`Fetching elevation from: ${elevationUrl}`);
  try {
    const response = await fetch(elevationUrl, {
      next: { revalidate: 3600 * 24 },
    });
    if (!response.ok) {
      console.error(
        `Elevation API error: ${response.status} ${response.statusText}`,
      );
      return null;
    }
    const data = await response.json();
    if (data?.elevation?.length > 0) {
      const elevationInMeters = data.elevation[0];
      return elevationInMeters * FEET_PER_METER;
    }
    console.warn("Elevation data not found in response:", data);
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
): Promise<ReturnType<typeof fetchWeatherApi>> {
  console.log(`Fetching weather for model: ${model}, location: ${location}`);
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
    models: model,
    [modelConfig.varsKey]: modelConfig.getAllVariables().join(","),
    ...(modelConfig.stepKey &&
      modelConfig.stepSize != null && {
        [modelConfig.stepKey]: modelConfig.stepSize,
      }),
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
  };

  console.log("Calling OpenMeteo API with params:", params);
  try {
    const responses = await fetchWeatherApi(API_URL, params);
    console.log(`Received ${responses.length} response(s) from OpenMeteo.`);
    return responses;
  } catch (error) {
    console.error("Failed to fetch weather API data:", error);
    throw error;
  }
}

export async function getWeatherData(
  model: WeatherModel,
  location: string,
): Promise<{
  data: CloudColumn[];
  timestamp: string;
  elevationFt: number | null;
}> {
  try {
    const hasCoordinates = location.includes("@");
    let coordinates;

    if (hasCoordinates) {
      const [_, coordString] = location.split("@");
      const [latitude, longitude] = coordString.split(",").map(Number);
      coordinates = { latitude, longitude };
    } else {
      coordinates = LOCATIONS[location.toUpperCase()];
    }

    if (!coordinates) {
      throw new Error(`Coordinates for location '${location}' not found.`);
    }

    const [weatherResponses, elevationFt] = await Promise.all([
      fetchWeatherDataAction(model, location),
      fetchElevationAction(coordinates.latitude, coordinates.longitude),
    ]);

    if (!weatherResponses || !weatherResponses[0]) {
      throw new Error("No weather data received");
    }

    const transformedData = transformWeatherData(weatherResponses[0], model);

    return {
      data: transformedData,
      timestamp: new Date().toISOString(),
      elevationFt,
    };
  } catch (error) {
    console.error("Error in getWeatherData:", error);
    throw new Error("Failed to process weather data.");
  }
}
