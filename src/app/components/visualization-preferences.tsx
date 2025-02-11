"use client";

import { useEffect } from "react";
import { usePersistedState } from "@/hooks/usePersistedState";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import { CloudColumn, WeatherModel } from "@/types/weather";

interface VisualizationPreferencesProps {
  model: WeatherModel;
  setModel: (model: WeatherModel) => void;
  location: string;
  setLocation: (location: string) => void;
  lastUpdate: Date;
  refetch: () => void;
  weatherData: CloudColumn[];
  isLoading: boolean;
  error: Error | null;
  preferences: {
    useLocalTime: boolean;
    highlightCeilingCoverage: boolean;
    clampCloudCoverageAt50Pct: boolean;
    showPressureLines: boolean;
  };
  updatePreferences: (
    prefs: Partial<{
      useLocalTime: boolean;
      highlightCeilingCoverage: boolean;
      clampCloudCoverageAt50Pct: boolean;
      showPressureLines: boolean;
    }>,
  ) => void;
}

export default function VisualizationPreferences({
  model,
  setModel,
  location,
  setLocation,
  lastUpdate,
  refetch,
  weatherData,
  isLoading,
  error,
  preferences,
  updatePreferences,
}: VisualizationPreferencesProps) {
  // Use persisted state for visualization preferences
  const [storedUseLocalTime, setStoredUseLocalTime] =
    usePersistedState<boolean>(
      "meteogram-use-local-time",
      preferences.useLocalTime,
    );
  const [storedHighlightCeiling, setStoredHighlightCeiling] =
    usePersistedState<boolean>(
      "meteogram-highlight-ceiling",
      preferences.highlightCeilingCoverage,
    );
  const [storedClampCoverage, setStoredClampCoverage] =
    usePersistedState<boolean>(
      "meteogram-clamp-coverage",
      preferences.clampCloudCoverageAt50Pct,
    );
  const [storedShowPressureLines, setStoredShowPressureLines] =
    usePersistedState<boolean>(
      "meteogram-show-pressure-lines",
      preferences.showPressureLines,
    );

  // Sync localStorage with URL params
  useEffect(() => {
    if (storedUseLocalTime !== preferences.useLocalTime) {
      updatePreferences({ useLocalTime: storedUseLocalTime });
    }
  }, [storedUseLocalTime]);

  useEffect(() => {
    if (storedHighlightCeiling !== preferences.highlightCeilingCoverage) {
      updatePreferences({ highlightCeilingCoverage: storedHighlightCeiling });
    }
  }, [storedHighlightCeiling]);

  useEffect(() => {
    if (storedClampCoverage !== preferences.clampCloudCoverageAt50Pct) {
      updatePreferences({ clampCloudCoverageAt50Pct: storedClampCoverage });
    }
  }, [storedClampCoverage]);

  useEffect(() => {
    if (storedShowPressureLines !== preferences.showPressureLines) {
      updatePreferences({ showPressureLines: storedShowPressureLines });
    }
  }, [storedShowPressureLines]);

  return (
    <>
      <NavWrapper
        model={model}
        setModel={setModel}
        location={location}
        setLocation={setLocation}
        lastUpdate={lastUpdate}
        refetch={refetch}
        useLocalTime={preferences.useLocalTime}
        setUseLocalTime={(value) => {
          setStoredUseLocalTime(value);
          updatePreferences({ useLocalTime: value });
        }}
        highlightCeilingCoverage={preferences.highlightCeilingCoverage}
        setHighlightCeilingCoverage={(value) => {
          setStoredHighlightCeiling(value);
          updatePreferences({ highlightCeilingCoverage: value });
        }}
        clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
        setClampCloudCoverageAt50Pct={(value) => {
          setStoredClampCoverage(value);
          updatePreferences({ clampCloudCoverageAt50Pct: value });
        }}
        showPressureLines={preferences.showPressureLines}
        setShowPressureLines={(value) => {
          setStoredShowPressureLines(value);
          updatePreferences({ showPressureLines: value });
        }}
      />
      <main className="items-center justify-between p-4">
        <MeteogramWrapper
          weatherData={weatherData}
          useLocalTime={preferences.useLocalTime}
          highlightCeilingCoverage={preferences.highlightCeilingCoverage}
          clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
          showPressureLines={preferences.showPressureLines}
          isLoading={isLoading}
          error={error}
        />
      </main>
    </>
  );
}
