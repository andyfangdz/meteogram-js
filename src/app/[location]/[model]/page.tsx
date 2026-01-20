import ClientWrapper from "../../components/client-wrapper";
import { MODEL_NAMES } from "@/config/weather";
import { LOCATIONS } from "@/config/weather";
import { WeatherModel } from "@/types/weather";
import { notFound } from "next/navigation";
import { getWeatherData } from "@/app/actions/weather";
import { getInitialPreferences } from "@/utils/serverPreferences";

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
    showIsothermLines?: string;
    [key: string]: string | undefined;
  }>;
}

// Only generate static params for predefined locations
export async function generateStaticParams() {
  return MODEL_NAMES.flatMap((model) =>
    Object.keys(LOCATIONS).map((location) => ({
      location: encodeURIComponent(location),
      model,
    })),
  );
}

// Disable static generation for dynamic routes
export const dynamic = "force-dynamic";

export default async function Page({ params, searchParams }: PageProps) {
  const [{ location, model }, searchParamsResolved] = await Promise.all([
    params,
    searchParams,
  ]);

  const decodedLocation = decodeURIComponent(location);

  // Only validate model, allow any location
  if (!MODEL_NAMES.includes(model as WeatherModel)) {
    notFound();
  }

  // Fetch initial data on the server
  const [initialData, preferencesResult] = await Promise.all([
    getWeatherData(model as WeatherModel, decodedLocation),
    getInitialPreferences(searchParamsResolved),
  ]);

  return (
    <div className="min-h-screen flex flex-col">
      <ClientWrapper
        initialLocation={decodedLocation}
        initialModel={model as WeatherModel}
        initialWeatherDataStr={JSON.stringify(initialData.data)}
        initialTimestamp={initialData.timestamp}
        initialElevationFt={initialData.elevationFt}
        initialPreferences={preferencesResult.preferences}
        cookieReadSuccess={preferencesResult.cookieReadSuccess}
      />
    </div>
  );
}
