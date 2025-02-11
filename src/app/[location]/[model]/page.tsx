import { HeroUIProvider } from "@heroui/react";
import ClientWrapper from "../../components/client-wrapper";
import { MODEL_NAMES } from "@/config/weather";
import { LOCATIONS } from "@/config/weather";
import { WeatherModel } from "@/types/weather";
import { notFound } from "next/navigation";
import { getWeatherData } from "@/app/actions/weather";

interface PageProps {
  params: Promise<{
    location: string;
    model: string;
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

export default async function Page({ params }: PageProps) {
  const { location, model } = await params;
  const decodedLocation = decodeURIComponent(location);

  // Validate parameters
  if (
    !Object.keys(LOCATIONS).includes(decodedLocation) ||
    !MODEL_NAMES.includes(model as WeatherModel)
  ) {
    notFound();
  }

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
        />
      </div>
    </HeroUIProvider>
  );
}
