"use client";

import {
  useState,
  useEffect,
  useRef,
  useCallback,
  SetStateAction,
} from "react";
import { WeatherModel, CloudColumn } from "../../types/weather";
import { useRouter, useSearchParams } from "next/navigation";
import { getWeatherData } from "../actions/weather";
import VisualizationPreferences from "./visualization-preferences";
import { DEFAULT_PREFERENCES } from "@/config/preferences";

interface VisualizationPreferences {
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showPressureLines: boolean;
  showWindBarbs: boolean;
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
  const [preferences, setPreferences] =
    useState<VisualizationPreferences>(initialPreferences);
  const timerRef = useRef<NodeJS.Timeout | null>(null);

  // Separate refresh functions for manual and background updates
  const refreshDataWithLoading = async () => {
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

  const refreshDataInBackground = useCallback(async () => {
    try {
      const response = await fetch(
        `/api/weather/${encodeURIComponent(initialLocation)}/${initialModel}`,
      );
      const data = await response.json();
      setWeatherData(data.weatherData);
      setLastUpdate(new Date(data.timestamp));
    } catch (error) {
      console.error("Error refreshing data:", error);
    }
  }, [initialLocation, initialModel]);

  useEffect(() => {
    const refreshInterval = 60000; // 1 minute
    timerRef.current = setInterval(refreshDataInBackground, refreshInterval);

    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [initialModel, initialLocation, refreshDataInBackground]);

  const handleLocationChange = (newLocation: SetStateAction<string>) => {
    const params = new URLSearchParams(searchParams);
    const resolvedLocation =
      typeof newLocation === "function"
        ? newLocation(initialLocation)
        : newLocation;
    router.push(
      `/${encodeURIComponent(resolvedLocation)}/${initialModel}?${params.toString()}`,
    );
  };

  const handleModelChange = (newModel: SetStateAction<WeatherModel>) => {
    const params = new URLSearchParams(searchParams);
    const resolvedModel =
      typeof newModel === "function" ? newModel(initialModel) : newModel;
    router.push(
      `/${encodeURIComponent(initialLocation)}/${resolvedModel}?${params.toString()}`,
    );
  };

  const updatePreferences = (
    newPreferences: Partial<VisualizationPreferences>,
  ) => {
    // Immediately update local state
    const updatedPreferences = { ...preferences, ...newPreferences };
    setPreferences(updatedPreferences);

    const params = new URLSearchParams(searchParams);

    // Only include non-default values in URL
    if (updatedPreferences.useLocalTime !== DEFAULT_PREFERENCES.useLocalTime) {
      params.set("useLocalTime", updatedPreferences.useLocalTime.toString());
    } else {
      params.delete("useLocalTime");
    }

    if (
      updatedPreferences.highlightCeilingCoverage !==
      DEFAULT_PREFERENCES.highlightCeilingCoverage
    ) {
      params.set(
        "highlightCeiling",
        updatedPreferences.highlightCeilingCoverage.toString(),
      );
    } else {
      params.delete("highlightCeiling");
    }

    if (
      updatedPreferences.clampCloudCoverageAt50Pct !==
      DEFAULT_PREFERENCES.clampCloudCoverageAt50Pct
    ) {
      params.set(
        "clampCoverage",
        updatedPreferences.clampCloudCoverageAt50Pct.toString(),
      );
    } else {
      params.delete("clampCoverage");
    }

    if (
      updatedPreferences.showPressureLines !==
      DEFAULT_PREFERENCES.showPressureLines
    ) {
      params.set(
        "showPressureLines",
        updatedPreferences.showPressureLines.toString(),
      );
    } else {
      params.delete("showPressureLines");
    }

    const queryString = params.toString();
    // Use shallow routing to prevent server request
    router.replace(
      `/${encodeURIComponent(initialLocation)}/${initialModel}${queryString ? "?" + queryString : ""}`,
      { scroll: false },
    );
  };

  return (
    <VisualizationPreferences
      model={initialModel}
      setModel={handleModelChange}
      location={initialLocation}
      setLocation={handleLocationChange}
      lastUpdate={lastUpdate}
      refetch={refreshDataWithLoading}
      weatherData={weatherData}
      isLoading={isLoading}
      error={error}
      preferences={preferences}
      updatePreferences={updatePreferences}
    />
  );
}
