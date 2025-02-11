"use client";

import { useState } from "react";
import { WeatherModel } from "../types/weather";
import { useWeatherData } from "../hooks/useWeatherData";
import NavWrapper from "./components/nav-wrapper";
import MeteogramWrapper from "./components/meteogram-wrapper";

export default function ClientWrapper() {
  const [location, setLocation] = useState<string>("KFRG");
  const [model, setModel] = useState<WeatherModel>("gfs_hrrr");
  const [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  const [highlightCeilingCoverage, setHighlightCeilingCoverage] =
    useState<boolean>(true);
  const [clampCloudCoverageAt50Pct, setClampCloudCoverageAt50Pct] =
    useState<boolean>(true);

  const { weatherData, lastUpdate, isLoading, error, refetch } = useWeatherData(
    model,
    location,
  );

  return (
    <>
      <NavWrapper
        model={model}
        setModel={setModel}
        location={location}
        setLocation={setLocation}
        lastUpdate={lastUpdate}
        refetch={refetch}
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
