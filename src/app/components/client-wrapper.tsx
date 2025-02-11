"use client";

import { useState, useEffect, useRef } from "react";
import { WeatherModel, CloudColumn } from "../../types/weather";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import { useRouter } from "next/navigation";
import { getWeatherData } from "../actions/weather";

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialWeatherData: CloudColumn[];
  initialTimestamp: string;
}

export default function ClientWrapper({
  initialLocation,
  initialModel,
  initialWeatherData,
  initialTimestamp,
}: ClientWrapperProps) {
  const router = useRouter();
  const [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  const [highlightCeilingCoverage, setHighlightCeilingCoverage] =
    useState<boolean>(true);
  const [clampCloudCoverageAt50Pct, setClampCloudCoverageAt50Pct] =
    useState<boolean>(true);

  const [weatherData, setWeatherData] =
    useState<CloudColumn[]>(initialWeatherData);
  const [lastUpdate, setLastUpdate] = useState<Date>(
    new Date(initialTimestamp),
  );
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const timerRef = useRef<NodeJS.Timeout | null>(null);

  const refreshData = async () => {
    try {
      setIsLoading(true);
      setError(null);
      const { data, timestamp } = await getWeatherData(
        initialModel,
        initialLocation,
      );
      setWeatherData(data);
      setLastUpdate(new Date(timestamp));
    } catch (err) {
      setError(
        err instanceof Error ? err : new Error("Failed to fetch weather data"),
      );
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    const refreshInterval = 60000; // 1 minute
    timerRef.current = setInterval(refreshData, refreshInterval);

    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [initialModel, initialLocation]);

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
        refetch={refreshData}
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
