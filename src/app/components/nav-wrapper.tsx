"use client";

import { WeatherModel } from "../../types/weather";
import Nav from "../Nav";

interface NavWrapperProps {
  model: WeatherModel;
  setModel: (model: WeatherModel) => void;
  location: string;
  setLocation: (location: string) => void;
  lastUpdate: Date | null;
  refetch: () => void;
  useLocalTime: boolean;
  setUseLocalTime: (value: boolean) => void;
  highlightCeilingCoverage: boolean;
  setHighlightCeilingCoverage: (value: boolean) => void;
  clampCloudCoverageAt50Pct: boolean;
  setClampCloudCoverageAt50Pct: (value: boolean) => void;
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
      sethighlightCeilingCoverage={setHighlightCeilingCoverage}
      clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
      setclampCloudCoverageAt50Pct={setClampCloudCoverageAt50Pct}
    />
  );
}
