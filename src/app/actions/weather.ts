"use server";

import { fetchWeatherApiData } from "@/services/weather";
import { transformWeatherData } from "@/utils/weather";
import { CloudColumn, WeatherModel } from "@/types/weather";
import { LOCATIONS } from "@/config/weather";
import { fetchElevationData } from "@/services/weather";

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
      coordinates = LOCATIONS[location];
    }

    if (!coordinates) {
      throw new Error(`Location ${location} not found`);
    }

    const [weatherResponses, elevationFt] = await Promise.all([
      fetchWeatherApiData(model, location),
      fetchElevationData(coordinates.latitude, coordinates.longitude),
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
    throw new Error("Failed to fetch weather data", { cause: error });
  }
}
