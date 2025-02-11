import { NextResponse } from "next/server";
import { fetchWeatherApiData } from "@/services/weather";
import { transformWeatherData } from "@/utils/weather";
import { WeatherModel } from "@/types/weather";

export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const model = searchParams.get("model") as WeatherModel;
  const location = searchParams.get("location");

  if (!model || !location) {
    return NextResponse.json(
      { error: "Missing required parameters" },
      { status: 400 },
    );
  }

  try {
    const responses = await fetchWeatherApiData(model, location);
    const transformedData = transformWeatherData(responses[0], model);

    return NextResponse.json({
      data: transformedData,
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    return NextResponse.json(
      { error: "Failed to fetch weather data" },
      { status: 500 },
    );
  }
}
