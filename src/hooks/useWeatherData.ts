import { useState, useEffect, useRef } from "react";
import { CloudColumn, WeatherModel } from "../types/weather";

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

      const response = await fetch(
        `/api/weather?model=${model}&location=${location}`,
      );

      if (!response.ok) {
        throw new Error("Failed to fetch weather data");
      }

      const { data, timestamp } = await response.json();

      // Convert date strings back to Date objects
      const processedData = data.map((item: any) => ({
        ...item,
        date: new Date(item.date),
      }));

      setWeatherData(processedData);
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
