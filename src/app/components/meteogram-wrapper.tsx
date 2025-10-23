"use client";

import React from "react";
import { CloudColumn, WeatherModel } from "../../types/weather";
import Meteogram from "./meteogram";
import { usePreferences } from "@/context/PreferencesContext";

interface MeteogramWrapperProps {
  weatherData: CloudColumn[];
  isLoading: boolean;
  error: Error | null;
  model: WeatherModel;
  elevationFt: number | null;
}

const MeteogramWrapper = ({
  weatherData,
  isLoading,
  error,
  model,
  elevationFt,
}: MeteogramWrapperProps) => {
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
        isLoading={isLoading}
        model={model}
        elevationFt={elevationFt}
      />
    </div>
  );
};

export default React.memo(MeteogramWrapper);
