import { fetchWeatherApi } from "openmeteo";
import { WeatherModel, WeatherApiParams } from "../types/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  LOCATIONS,
  MODEL_CONFIGS,
} from "../config/weather";

export async function fetchWeatherApiData(
  model: WeatherModel,
  location: string,
) {
  const modelConfig = MODEL_CONFIGS[model];

  const params: WeatherApiParams = {
    ...DEFAULT_PARAMS,
    ...LOCATIONS[location],
    models: model,
    [modelConfig.varsKey]: modelConfig.getAllVariables().join(","),
    [modelConfig.stepKey]: modelConfig.stepSize,
  };
  return fetchWeatherApi(API_URL, params);
}
