"use client";

import { CloudColumn, WeatherModel } from "../../types/weather";
import Meteogram from "./meteogram";

interface MeteogramWrapperProps {
  weatherData: CloudColumn[];
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showPressureLines: boolean;
  showWindBarbs: boolean;
  showIsothermLines: boolean;
  isLoading: boolean;
  error: Error | null;
  model: WeatherModel;
  elevationFt: number | null;
}

export default function MeteogramWrapper({
  weatherData,
  useLocalTime,
  highlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  showPressureLines,
  showWindBarbs,
  showIsothermLines,
  isLoading,
  error,
  model,
  elevationFt,
}: MeteogramWrapperProps) {
  return (
    <div className="contents">
      {error && (
        <div className="text-red-500 mb-4">
          Error loading weather data: {error.message}
        </div>
      )}
      <Meteogram
        width={1600}
        height={800}
        useLocalTime={useLocalTime}
        weatherData={weatherData}
        highlightCeilingCoverage={highlightCeilingCoverage}
        clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
        showPressureLines={showPressureLines}
        showWindBarbs={showWindBarbs}
        showIsothermLines={showIsothermLines}
        isLoading={isLoading}
        model={model}
        elevationFt={elevationFt}
      />
    </div>
  );
}
