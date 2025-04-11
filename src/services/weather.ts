import { fetchWeatherApi } from "openmeteo";
import { WeatherModel, WeatherApiParams } from "../types/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  MODEL_CONFIGS,
  LOCATIONS,
} from "../config/weather";

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
