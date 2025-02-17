"use server";

import { fetchWeatherApiData } from "@/services/weather";
import { transformWeatherData } from "@/utils/weather";
import { CloudColumn, WeatherModel } from "@/types/weather";

export async function getWeatherData(
  model: WeatherModel,
  location: string,
): Promise<{
  data: CloudColumn[];
  timestamp: string;
}> {
  try {
    const responses = await fetchWeatherApiData(model, location);
    const transformedData = transformWeatherData(responses[0], model);

    return {
      data: transformedData,
      timestamp: new Date().toISOString(),
    };
  } catch (error) {
    throw new Error("Failed to fetch weather data", { cause: error });
  }
}
