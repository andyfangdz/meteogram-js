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
  showIsothermLines: boolean;
}

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialWeatherData: CloudColumn[];
  initialTimestamp: string;
  initialElevationFt: number | null;
  initialPreferences: VisualizationPreferences;
}

export default function ClientWrapper({
  initialLocation,
  initialModel,
  initialWeatherData,
  initialTimestamp,
  initialElevationFt,
  initialPreferences,
}: ClientWrapperProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [model, setModel] = useState<WeatherModel>(initialModel);
  const [weatherData, setWeatherData] =
    useState<CloudColumn[]>(initialWeatherData);
  const [timestamp, setTimestamp] = useState<string>(initialTimestamp);
  const [elevationFt, setElevationFt] = useState<number | null>(
    initialElevationFt,
  );
  const [preferences, setPreferences] =
    useState<VisualizationPreferences>(initialPreferences);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const timerRef = useRef<NodeJS.Timeout | null>(null);

  // Separate refresh functions for manual and background updates
  const refreshDataWithLoading = async () => {
    try {
      setIsLoading(true);
      setError(null);
      const {
        data,
        timestamp: newTimestamp,
        elevationFt: newElevationFt,
      } = await getWeatherData(model, initialLocation);
      setWeatherData(data);
      setTimestamp(newTimestamp);
      setElevationFt(newElevationFt);
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
      const {
        data,
        timestamp: newTimestamp,
        elevationFt: newElevationFt,
      } = await getWeatherData(model, initialLocation);
      setWeatherData(data);
      setTimestamp(newTimestamp);
      setElevationFt(newElevationFt);
    } catch (error) {
      console.error("Error refreshing data:", error);
    }
  }, [initialLocation, model]);

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
      `/${encodeURIComponent(resolvedLocation)}/${model}?${params.toString()}`,
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

    if (
      updatedPreferences.showWindBarbs !== DEFAULT_PREFERENCES.showWindBarbs
    ) {
      params.set("showWindBarbs", updatedPreferences.showWindBarbs.toString());
    } else {
      params.delete("showWindBarbs");
    }

    if (
      updatedPreferences.showIsothermLines !==
      DEFAULT_PREFERENCES.showIsothermLines
    ) {
      params.set(
        "showIsothermLines",
        updatedPreferences.showIsothermLines.toString(),
      );
    } else {
      params.delete("showIsothermLines");
    }

    const queryString = params.toString();
    // Use shallow routing to prevent server request
    router.replace(
      `/${encodeURIComponent(initialLocation)}/${model}${queryString ? "?" + queryString : ""}`,
      { scroll: false },
    );
  };

  return (
    <VisualizationPreferences
      model={model}
      setModel={handleModelChange}
      location={initialLocation}
      setLocation={handleLocationChange}
      lastUpdate={new Date(timestamp)}
      refetch={refreshDataWithLoading}
      weatherData={weatherData}
      isLoading={isLoading}
      error={error}
      preferences={preferences}
      updatePreferences={updatePreferences}
      elevationFt={elevationFt}
    />
  );
}
