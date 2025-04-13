"use client";

import {
  useState,
  useEffect,
  useRef,
  useCallback,
  SetStateAction,
} from "react";
import { WeatherModel, CloudColumn } from "../../types/weather";
import { useRouter } from "next/navigation";
import { getWeatherData } from "../actions/weather";
import VisualizationPreferencesComponent from "./visualization-preferences";
import {
  PreferencesProvider,
  usePreferences,
} from "@/context/PreferencesContext";
import { serializeVisualizationPreferences } from "@/utils/params";

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialWeatherData: CloudColumn[];
  initialTimestamp: string;
  initialElevationFt: number | null;
}

function ClientWrapperInternal({
  initialLocation,
  initialModel,
  initialWeatherData,
  initialTimestamp,
  initialElevationFt,
}: Omit<ClientWrapperProps, "initialPreferences">) {
  const router = useRouter();
  const { preferences } = usePreferences();
  const [model, setModel] = useState<WeatherModel>(initialModel);
  const [weatherData, setWeatherData] =
    useState<CloudColumn[]>(initialWeatherData);
  const [timestamp, setTimestamp] = useState<string>(initialTimestamp);
  const [elevationFt, setElevationFt] = useState<number | null>(
    initialElevationFt,
  );
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
    const resolvedLocation =
      typeof newLocation === "function"
        ? newLocation(initialLocation)
        : newLocation;
    router.push(
      `/${encodeURIComponent(resolvedLocation)}/${model}?${serializeVisualizationPreferences(preferences).toString()}`,
    );
  };

  const handleModelChange = (newModel: SetStateAction<WeatherModel>) => {
    const resolvedModel =
      typeof newModel === "function" ? newModel(initialModel) : newModel;
    router.push(
      `/${encodeURIComponent(initialLocation)}/${resolvedModel}?${serializeVisualizationPreferences(preferences).toString()}`,
    );
  };

  return (
    <VisualizationPreferencesComponent
      model={model}
      setModel={handleModelChange}
      location={initialLocation}
      setLocation={handleLocationChange}
      lastUpdate={new Date(timestamp)}
      refetch={refreshDataWithLoading}
      weatherData={weatherData}
      isLoading={isLoading}
      error={error}
      elevationFt={elevationFt}
    />
  );
}

export default function ClientWrapper(
  props: Omit<ClientWrapperProps, "initialPreferences">,
) {
  return (
    <PreferencesProvider>
      <ClientWrapperInternal {...props} />
    </PreferencesProvider>
  );
}
