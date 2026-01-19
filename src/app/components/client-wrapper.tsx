"use client";

import {
  useState,
  useEffect,
  useRef,
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

// Type for the global weather data injected via script tag
interface SerializedWeatherData {
  data: Array<{
    date: string;
    cloud: CloudColumn["cloud"];
    groundTemp: number;
  }>;
  timestamp: string;
  elevationFt: number | null;
}

declare global {
  interface Window {
    __WEATHER_DATA__?: SerializedWeatherData;
  }
}

// Parse serialized weather data from the global variable
function getInitialWeatherData(): {
  data: CloudColumn[];
  timestamp: string;
  elevationFt: number | null;
} | null {
  if (typeof window === "undefined" || !window.__WEATHER_DATA__) {
    return null;
  }
  const { data, timestamp, elevationFt } = window.__WEATHER_DATA__;
  return {
    data: data.map((column) => ({
      ...column,
      date: new Date(column.date),
    })),
    timestamp,
    elevationFt,
  };
}

interface ClientWrapperProps {
  initialLocation: string;
  initialModel: WeatherModel;
  initialPreferences: VisualizationPreferences;
  cookieReadSuccess?: boolean;
}

function ClientWrapperInternal({
  initialLocation,
  initialModel,
}: Omit<ClientWrapperProps, "initialPreferences" | "cookieReadSuccess">) {
  const router = useRouter();
  const { preferences } = usePreferences();
  const [model, setModel] = useState<WeatherModel>(initialModel);

  // Initialize state from global weather data (injected via script tag)
  const [weatherData, setWeatherData] = useState<CloudColumn[]>(() => {
    const initial = getInitialWeatherData();
    return initial?.data ?? [];
  });
  const [timestamp, setTimestamp] = useState<string>(() => {
    const initial = getInitialWeatherData();
    return initial?.timestamp ?? "";
  });
  const [elevationFt, setElevationFt] = useState<number | null>(() => {
    const initial = getInitialWeatherData();
    return initial?.elevationFt ?? null;
  });

  const [isLoading, setIsLoading] = useState(() => !getInitialWeatherData());
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
        />
      </PreferencesProvider>
    </HeroUIProvider>
  );
}
