"use client";

import { CloudColumn } from "../../types/weather";
import Meteogram from "../meteogram";

interface MeteogramWrapperProps {
  weatherData: CloudColumn[];
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  isLoading: boolean;
  error: Error | null;
}

export default function MeteogramWrapper({
  weatherData,
  useLocalTime,
  highlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  isLoading,
  error,
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
        isLoading={isLoading}
      />
    </div>
  );
}
