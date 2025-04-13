import { fetchWeatherApi } from "openmeteo";
import { WeatherModel, WeatherApiParams } from "../types/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  MODEL_CONFIGS,
  LOCATIONS,
  FEET_PER_METER,
} from "../config/weather";

export async function fetchElevationData(
  latitude: number,
  longitude: number,
): Promise<number | null> {
  const elevationUrl = `https://api.open-meteo.com/v1/elevation?latitude=${latitude}&longitude=${longitude}`;
  try {
    const response = await fetch(elevationUrl);
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    const data = await response.json();
    // API returns an array, we only need the first value
    if (data?.elevation?.length > 0) {
      // Convert meters to feet
      return data.elevation[0] * FEET_PER_METER;
    }
    return null;
  } catch (error) {
    console.error("Failed to fetch elevation data:", error);
    return null; // Return null or handle error as appropriate
  }
}

export async function fetchWeatherApiData(
  model: WeatherModel,
  location: string,
) {
  const modelConfig = MODEL_CONFIGS[model];

  // Check if location contains coordinates (e.g. "KFRG@40.73443,-73.41639")
  const hasCoordinates = location.includes("@");
  let coordinates;

  if (hasCoordinates) {
    const [_, coordString] = location.split("@");
    const [latitude, longitude] = coordString.split(",").map(Number);
    coordinates = { latitude, longitude };
  } else {
    coordinates = LOCATIONS[location];
  }

  if (!coordinates) {
    throw new Error(`Location ${location} not found`);
  }

  const params: WeatherApiParams = {
    ...DEFAULT_PARAMS,
    latitude: coordinates.latitude,
    longitude: coordinates.longitude,
    models: model,
    [modelConfig.varsKey]: modelConfig.getAllVariables().join(","),
    [modelConfig.stepKey]: modelConfig.stepSize,
  };

  return fetchWeatherApi(API_URL, params);
}
