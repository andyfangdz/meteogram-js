"use client";

import { Dispatch, SetStateAction } from "react";
import { WeatherModel } from "../../types/weather";
import Nav from "../Nav";

interface NavWrapperProps {
  model: WeatherModel;
  setModel: Dispatch<SetStateAction<WeatherModel>>;
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
  lastUpdate: Date | null;
  refetch: () => void;
  useLocalTime: boolean;
  setUseLocalTime: Dispatch<SetStateAction<boolean>>;
  highlightCeilingCoverage: boolean;
  setHighlightCeilingCoverage: Dispatch<SetStateAction<boolean>>;
  clampCloudCoverageAt50Pct: boolean;
  setClampCloudCoverageAt50Pct: Dispatch<SetStateAction<boolean>>;
  showPressureLines: boolean;
  setShowPressureLines: Dispatch<SetStateAction<boolean>>;
  showWindBarbs: boolean;
  setShowWindBarbs: Dispatch<SetStateAction<boolean>>;
}

export default function NavWrapper({
  model,
  setModel,
  location,
  setLocation,
  lastUpdate,
  refetch,
  useLocalTime,
  setUseLocalTime,
  highlightCeilingCoverage,
  setHighlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  setClampCloudCoverageAt50Pct,
  showPressureLines,
  setShowPressureLines,
  showWindBarbs,
  setShowWindBarbs,
}: NavWrapperProps) {
  return (
    <Nav
      location={location}
      setLocation={setLocation}
      model={model}
      setModel={setModel}
      updateWeatherData={refetch}
      lastUpdate={lastUpdate}
      useLocalTime={useLocalTime}
      setUseLocalTime={setUseLocalTime}
      highlightCeilingCoverage={highlightCeilingCoverage}
      setHighlightCeilingCoverage={setHighlightCeilingCoverage}
      clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
      setClampCloudCoverageAt50Pct={setClampCloudCoverageAt50Pct}
      showPressureLines={showPressureLines}
      setShowPressureLines={setShowPressureLines}
      showWindBarbs={showWindBarbs}
      setShowWindBarbs={setShowWindBarbs}
    />
  );
}
