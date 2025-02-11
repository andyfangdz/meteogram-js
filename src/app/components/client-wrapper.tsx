"use client";

import { useState } from "react";
import { WeatherModel } from "../../types/weather";
import { useWeatherData } from "../../hooks/useWeatherData";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import { useRouter } from "next/navigation";

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
}

export default function ClientWrapper({
  initialLocation,
  initialModel,
}: ClientWrapperProps) {
  const router = useRouter();
  const [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  const [highlightCeilingCoverage, setHighlightCeilingCoverage] =
    useState<boolean>(true);
  const [clampCloudCoverageAt50Pct, setClampCloudCoverageAt50Pct] =
    useState<boolean>(true);

  const { weatherData, lastUpdate, isLoading, error, refetch } = useWeatherData(
    initialModel,
    initialLocation,
  );

  const handleLocationChange = (newLocation: string) => {
    router.push(`/${encodeURIComponent(newLocation)}/${initialModel}`);
  };

  const handleModelChange = (newModel: WeatherModel) => {
    router.push(`/${encodeURIComponent(initialLocation)}/${newModel}`);
  };

  return (
    <>
      <NavWrapper
        model={initialModel}
        setModel={handleModelChange}
        location={initialLocation}
        setLocation={handleLocationChange}
        lastUpdate={lastUpdate}
        refetch={refetch}
        useLocalTime={useLocalTime}
        setUseLocalTime={setUseLocalTime}
        highlightCeilingCoverage={highlightCeilingCoverage}
        setHighlightCeilingCoverage={setHighlightCeilingCoverage}
        clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
        setClampCloudCoverageAt50Pct={setClampCloudCoverageAt50Pct}
      />
      <main className="items-center justify-between p-24">
        <MeteogramWrapper
          weatherData={weatherData}
          useLocalTime={useLocalTime}
          highlightCeilingCoverage={highlightCeilingCoverage}
          clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
          isLoading={isLoading}
          error={error}
        />
      </main>
    </>
  );
}
