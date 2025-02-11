import { useState, useEffect, useRef } from "react";
import { CloudColumn, WeatherModel } from "../types/weather";
import { fetchWeatherApiData } from "../services/weather";
import { transformWeatherData } from "../utils/weather";

interface UseWeatherDataReturn {
  weatherData: CloudColumn[];
  lastUpdate: Date | null;
  isLoading: boolean;
  error: Error | null;
  refetch: () => void;
}

export function useWeatherData(
  model: WeatherModel,
  location: string,
  refreshInterval = 60000,
): UseWeatherDataReturn {
  const [weatherData, setWeatherData] = useState<CloudColumn[]>([]);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const timerRef = useRef<NodeJS.Timeout | null>(null);

  const fetchData = async () => {
    try {
      setIsLoading(true);
      setError(null);
      const responses = await fetchWeatherApiData(model, location);
      const transformedData = transformWeatherData(responses[0], model);
      setWeatherData(transformedData);
      setLastUpdate(new Date());
    } catch (err) {
      setError(
        err instanceof Error ? err : new Error("Failed to fetch weather data"),
      );
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchData();

    if (refreshInterval > 0) {
      timerRef.current = setInterval(fetchData, refreshInterval);
    }

    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [model, location, refreshInterval]);

  return {
    weatherData,
    lastUpdate,
    isLoading,
    error,
    refetch: fetchData,
  };
}
