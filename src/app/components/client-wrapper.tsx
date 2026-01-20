"use client";

import {
  useState,
  useEffect,
  useRef,
  useMemo,
  useCallback,
  SetStateAction,
} from "react";
import {
  WeatherModel,
  CloudColumn,
  VisualizationPreferences,
} from "../../types/weather";
import { useRouter } from "next/navigation";
import { getWeatherData } from "../actions/weather";
import VisualizationPreferencesComponent from "./visualization-preferences";
import {
  PreferencesProvider,
  usePreferences,
} from "@/context/PreferencesContext";
import { serializeVisualizationPreferences } from "@/utils/params";
import { HeroUIProvider } from "@heroui/react";

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialWeatherDataStr: string;
  initialTimestamp: string;
  initialElevationFt: number | null;
  initialPreferences: VisualizationPreferences;
  cookieReadSuccess?: boolean;
}

function ClientWrapperInternal({
  initialLocation,
  initialModel,
  initialWeatherDataStr,
  initialTimestamp,
  initialElevationFt,
}: Omit<ClientWrapperProps, "initialPreferences">) {
  const router = useRouter();
  const { preferences } = usePreferences();
  const [model, setModel] = useState<WeatherModel>(initialModel);

  // Parse initial data once - available during SSR
  const parsedInitialData = useMemo(() => {
    try {
      return JSON.parse(initialWeatherDataStr).map((col: any) => ({
        ...col,
        date: new Date(col.date),
      }));
    } catch (e) {
      console.error("Failed to parse initial weather data", e);
      return [];
    }
  }, [initialWeatherDataStr]);

  const [weatherData, setWeatherData] =
    useState<CloudColumn[]>(parsedInitialData);
  const [timestamp, setTimestamp] = useState<string>(initialTimestamp);

  // Synchronize state with parsedInitialData if it changes (e.g. on navigation)
  useEffect(() => {
    setWeatherData(parsedInitialData);
  }, [parsedInitialData]);
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

export default function ClientWrapper(props: ClientWrapperProps) {
  return (
    <HeroUIProvider>
      <PreferencesProvider
        initialPreferences={props.initialPreferences}
        cookieReadSuccess={props.cookieReadSuccess}
      >
        <ClientWrapperInternal
          initialLocation={props.initialLocation}
          initialModel={props.initialModel}
          initialWeatherDataStr={props.initialWeatherDataStr}
          initialTimestamp={props.initialTimestamp}
          initialElevationFt={props.initialElevationFt}
        />
      </PreferencesProvider>
    </HeroUIProvider>
  );
}
