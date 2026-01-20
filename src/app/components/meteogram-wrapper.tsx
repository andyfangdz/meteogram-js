"use client";

import React from "react";
import { CloudColumn, WeatherModel } from "../../types/weather";
import { usePreferences } from "@/context/PreferencesContext";
import dynamic from "next/dynamic";
import LoadingSkeleton from "./loading-skeleton";

const Meteogram = dynamic(() => import("./meteogram"), {
  ssr: false,
  loading: () => <LoadingSkeleton />,
});

interface MeteogramWrapperProps {
  weatherData: CloudColumn[];
  isLoading: boolean;
  error: Error | null;
  model: WeatherModel;
  elevationFt: number | null;
}

// Memoized to prevent re-renders when parent updates without prop changes.
// Preferences come from context, so this primarily helps with parent re-renders unrelated to props.
const MeteogramWrapper = React.memo(function MeteogramWrapper({
  weatherData,
  isLoading,
  error,
  model,
  elevationFt,
}: MeteogramWrapperProps) {
  const { preferences } = usePreferences();

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
        useLocalTime={preferences.useLocalTime}
        weatherData={weatherData}
        highlightCeilingCoverage={preferences.highlightCeilingCoverage}
        clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
        showPressureLines={preferences.showPressureLines}
        showWindBarbs={preferences.showWindBarbs}
        showIsothermLines={preferences.showIsothermLines}
        showIsotachLines={preferences.showIsotachLines}
        showDewPointDepressionLines={preferences.showDewPointDepressionLines}
        isLoading={isLoading}
        model={model}
        elevationFt={elevationFt}
      />
    </div>
  );
});

export default MeteogramWrapper;
