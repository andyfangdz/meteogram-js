import { HeroUIProvider } from "@heroui/react";
import ClientWrapper from "../../components/client-wrapper";
import { MODEL_NAMES } from "@/config/weather";
import { LOCATIONS } from "@/config/weather";
import { WeatherModel } from "@/types/weather";
import { notFound } from "next/navigation";
import { getWeatherData } from "@/app/actions/weather";
import { DEFAULT_PREFERENCES } from "@/config/preferences";

interface PageProps {
  params: Promise<{
    location: string;
    model: string;
  }>;
  searchParams: Promise<{
    useLocalTime?: string;
    highlightCeiling?: string;
    clampCoverage?: string;
    showPressureLines?: string;
    showWindBarbs?: string;
  }>;
}

export async function generateStaticParams() {
  return MODEL_NAMES.flatMap((model) =>
    Object.keys(LOCATIONS).map((location) => ({
      location: encodeURIComponent(location),
      model,
    })),
  );
}

export default async function Page({ params, searchParams }: PageProps) {
  const [{ location, model }, searchParamsResolved] = await Promise.all([
    params,
    searchParams,
  ]);

  const decodedLocation = decodeURIComponent(location);

  // Validate parameters
  if (
    !Object.keys(LOCATIONS).includes(decodedLocation) ||
    !MODEL_NAMES.includes(model as WeatherModel)
  ) {
    notFound();
  }

  // Parse visualization preferences with defaults
  const preferences = {
    useLocalTime:
      searchParamsResolved.useLocalTime === "true" ||
      DEFAULT_PREFERENCES.useLocalTime,
    highlightCeilingCoverage:
      searchParamsResolved.highlightCeiling === "false"
        ? false
        : DEFAULT_PREFERENCES.highlightCeilingCoverage,
    clampCloudCoverageAt50Pct:
      searchParamsResolved.clampCoverage === "false"
        ? false
        : DEFAULT_PREFERENCES.clampCloudCoverageAt50Pct,
    showPressureLines:
      searchParamsResolved.showPressureLines === "true" ||
      DEFAULT_PREFERENCES.showPressureLines,
    showWindBarbs: searchParamsResolved.showWindBarbs === "true",
  };

  // Fetch initial data on the server
  const initialData = await getWeatherData(
    model as WeatherModel,
    decodedLocation,
  );

  return (
    <HeroUIProvider>
      <div className="min-h-screen flex flex-col">
        <ClientWrapper
          initialLocation={decodedLocation}
          initialModel={model as WeatherModel}
          initialWeatherData={initialData.data}
          initialTimestamp={initialData.timestamp}
          initialPreferences={preferences}
        />
      </div>
    </HeroUIProvider>
  );
}
