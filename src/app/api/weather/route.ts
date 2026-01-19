import { NextResponse } from "next/server";
import { fetchWeatherApi } from "openmeteo";
import { transformWeatherData } from "@/utils/weather";
import {
  API_URL,
  DEFAULT_PARAMS,
  MODEL_CONFIGS,
  LOCATIONS,
  FEET_PER_METER,
} from "@/config/weather";
import { WeatherModel } from "@/types/weather";

// Fetch elevation data for a coordinate
async function fetchElevation(
  latitude: number,
  longitude: number
): Promise<number | null> {
  try {
    const response = await fetch(
      `https://api.open-meteo.com/v1/elevation?latitude=${latitude}&longitude=${longitude}`
    );
    if (!response.ok) return null;
    const data = await response.json();
    if (data?.elevation?.length > 0) {
      return data.elevation[0] * FEET_PER_METER;
    }
    return null;
  } catch {
    return null;
  }
}

// Parse location string to get coordinates
function parseLocation(
  location: string
): { latitude: number; longitude: number } | null {
  // Check if it's a custom location with coordinates (e.g., "Name@40.73,-73.42")
  if (location.includes("@")) {
    const parts = location.split("@");
    const coordString = parts[1];
    const [latStr, lonStr] = coordString.split(",");
    const latitude = parseFloat(latStr);
    const longitude = parseFloat(lonStr);
    if (!isNaN(latitude) && !isNaN(longitude)) {
      return { latitude, longitude };
    }
    return null;
  }

  // Check predefined locations
  const predefined = LOCATIONS[location.toUpperCase()];
  if (predefined) {
    return predefined;
  }

  return null;
}

export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const model = (searchParams.get("model") || "gfs_hrrr") as WeatherModel;
  const location = searchParams.get("location") || "KCDW";

  try {
    const modelConfig = MODEL_CONFIGS[model];
    if (!modelConfig) {
      return NextResponse.json(
        { error: "Invalid model" },
        { status: 400 }
      );
    }

    const coordinates = parseLocation(location);
    if (!coordinates) {
      return NextResponse.json(
        { error: "Location not found" },
        { status: 404 }
      );
    }

    // Fetch weather data and elevation in parallel
    const params = {
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

    const [responses, elevationFt] = await Promise.all([
      fetchWeatherApi(API_URL, params),
      fetchElevation(coordinates.latitude, coordinates.longitude),
    ]);

    const transformedData = transformWeatherData(responses[0], model);

    // Serialize with date conversion
    const serializedData = transformedData.map((column) => ({
      ...column,
      date: column.date.toISOString(),
    }));

    return NextResponse.json({
      data: serializedData,
      timestamp: new Date().toISOString(),
      elevationFt,
    });
  } catch (error) {
    console.error("API route error:", error);
    return NextResponse.json(
      { error: "Failed to fetch weather data" },
      { status: 500 }
    );
  }
}
