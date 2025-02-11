"use client";

import { useState, useEffect, useRef } from "react";
import { WeatherModel, CloudColumn } from "../../types/weather";
import { useRouter, useSearchParams } from "next/navigation";
import { getWeatherData } from "../actions/weather";
import VisualizationPreferences from "./visualization-preferences";
import { DEFAULT_PREFERENCES } from "@/config/preferences";

interface VisualizationPreferences {
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
}

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialWeatherData: CloudColumn[];
  initialTimestamp: string;
  initialPreferences: VisualizationPreferences;
}

export default function ClientWrapper({
  initialLocation,
  initialModel,
  initialWeatherData,
  initialTimestamp,
  initialPreferences,
}: ClientWrapperProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
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
    const params = new URLSearchParams(searchParams);
    router.push(
      `/${encodeURIComponent(newLocation)}/${initialModel}?${params.toString()}`,
    );
  };

  const handleModelChange = (newModel: WeatherModel) => {
    const params = new URLSearchParams(searchParams);
    router.push(
      `/${encodeURIComponent(initialLocation)}/${newModel}?${params.toString()}`,
    );
  };

  const updatePreferences = (
    newPreferences: Partial<VisualizationPreferences>,
  ) => {
    const params = new URLSearchParams(searchParams);

    // Get current full preferences
    const currentPreferences = {
      useLocalTime:
        params.get("useLocalTime") === "true" ||
        DEFAULT_PREFERENCES.useLocalTime,
      highlightCeilingCoverage:
        params.get("highlightCeiling") === "false"
          ? false
          : DEFAULT_PREFERENCES.highlightCeilingCoverage,
      clampCloudCoverageAt50Pct:
        params.get("clampCoverage") === "false"
          ? false
          : DEFAULT_PREFERENCES.clampCloudCoverageAt50Pct,
      ...newPreferences,
    };

    // Only include non-default values in URL
    if (currentPreferences.useLocalTime !== DEFAULT_PREFERENCES.useLocalTime) {
      params.set("useLocalTime", currentPreferences.useLocalTime.toString());
    } else {
      params.delete("useLocalTime");
    }

    if (
      currentPreferences.highlightCeilingCoverage !==
      DEFAULT_PREFERENCES.highlightCeilingCoverage
    ) {
      params.set(
        "highlightCeiling",
        currentPreferences.highlightCeilingCoverage.toString(),
      );
    } else {
      params.delete("highlightCeiling");
    }

    if (
      currentPreferences.clampCloudCoverageAt50Pct !==
      DEFAULT_PREFERENCES.clampCloudCoverageAt50Pct
    ) {
      params.set(
        "clampCoverage",
        currentPreferences.clampCloudCoverageAt50Pct.toString(),
      );
    } else {
      params.delete("clampCoverage");
    }

    const queryString = params.toString();
    router.push(
      `/${encodeURIComponent(initialLocation)}/${initialModel}${queryString ? "?" + queryString : ""}`,
    );
  };

  return (
    <VisualizationPreferences
      model={initialModel}
      setModel={handleModelChange}
      location={initialLocation}
      setLocation={handleLocationChange}
      lastUpdate={lastUpdate}
      refetch={refreshData}
      weatherData={weatherData}
      isLoading={isLoading}
      error={error}
      preferences={initialPreferences}
      updatePreferences={updatePreferences}
    />
  );
}
