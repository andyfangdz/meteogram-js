"use client";

import { useEffect, Dispatch, SetStateAction } from "react";
import { usePersistedState } from "@/hooks/usePersistedState";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import { CloudColumn, WeatherModel } from "@/types/weather";

interface VisualizationPreferencesProps {
  model: WeatherModel;
  setModel: Dispatch<SetStateAction<WeatherModel>>;
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
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
    showWindBarbs: boolean;
    showIsothermLines: boolean;
  };
  updatePreferences: (
    prefs: Partial<{
      useLocalTime: boolean;
      highlightCeilingCoverage: boolean;
      clampCloudCoverageAt50Pct: boolean;
      showPressureLines: boolean;
      showWindBarbs: boolean;
      showIsothermLines: boolean;
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
  const [storedShowWindBarbs, setStoredShowWindBarbs] =
    usePersistedState<boolean>(
      "meteogram-show-wind-barbs",
      preferences.showWindBarbs,
    );
  const [storedShowIsothermLines, setStoredShowIsothermLines] =
    usePersistedState<boolean>(
      "meteogram-show-isotherm-lines",
      preferences.showIsothermLines,
    );

  // Sync localStorage with URL params
  useEffect(() => {
    if (storedUseLocalTime !== preferences.useLocalTime) {
      updatePreferences({ useLocalTime: storedUseLocalTime });
    }
  }, [storedUseLocalTime, preferences.useLocalTime, updatePreferences]);

  useEffect(() => {
    if (storedHighlightCeiling !== preferences.highlightCeilingCoverage) {
      updatePreferences({ highlightCeilingCoverage: storedHighlightCeiling });
    }
  }, [
    storedHighlightCeiling,
    preferences.highlightCeilingCoverage,
    updatePreferences,
  ]);

  useEffect(() => {
    if (storedClampCoverage !== preferences.clampCloudCoverageAt50Pct) {
      updatePreferences({ clampCloudCoverageAt50Pct: storedClampCoverage });
    }
  }, [
    storedClampCoverage,
    preferences.clampCloudCoverageAt50Pct,
    updatePreferences,
  ]);

  useEffect(() => {
    if (storedShowPressureLines !== preferences.showPressureLines) {
      updatePreferences({ showPressureLines: storedShowPressureLines });
    }
  }, [
    storedShowPressureLines,
    preferences.showPressureLines,
    updatePreferences,
  ]);

  useEffect(() => {
    if (storedShowWindBarbs !== preferences.showWindBarbs) {
      updatePreferences({ showWindBarbs: storedShowWindBarbs });
    }
  }, [storedShowWindBarbs, preferences.showWindBarbs, updatePreferences]);

  useEffect(() => {
    if (storedShowIsothermLines !== preferences.showIsothermLines) {
      updatePreferences({ showIsothermLines: storedShowIsothermLines });
    }
  }, [
    storedShowIsothermLines,
    preferences.showIsothermLines,
    updatePreferences,
  ]);

  const handleSetModel = (newModel: SetStateAction<WeatherModel>) => {
    setModel(newModel);
  };

  const handleSetLocation = (newLocation: SetStateAction<string>) => {
    setLocation(newLocation);
  };

  const handleSetUseLocalTime = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredUseLocalTime(value);
      updatePreferences({ useLocalTime: value });
    }
  };

  const handleSetHighlightCeiling = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredHighlightCeiling(value);
      updatePreferences({ highlightCeilingCoverage: value });
    }
  };

  const handleSetClampCoverage = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredClampCoverage(value);
      updatePreferences({ clampCloudCoverageAt50Pct: value });
    }
  };

  const handleSetShowPressureLines = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredShowPressureLines(value);
      updatePreferences({ showPressureLines: value });
    }
  };

  const handleSetShowWindBarbs = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredShowWindBarbs(value);
      updatePreferences({ showWindBarbs: value });
    }
  };

  const handleSetShowIsothermLines = (value: SetStateAction<boolean>) => {
    if (typeof value === "boolean") {
      setStoredShowIsothermLines(value);
      updatePreferences({ showIsothermLines: value });
    }
  };

  return (
    <>
      <NavWrapper
        model={model}
        setModel={handleSetModel}
        location={location}
        setLocation={handleSetLocation}
        lastUpdate={lastUpdate}
        refetch={refetch}
        useLocalTime={preferences.useLocalTime}
        setUseLocalTime={handleSetUseLocalTime}
        highlightCeilingCoverage={preferences.highlightCeilingCoverage}
        setHighlightCeilingCoverage={handleSetHighlightCeiling}
        clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
        setClampCloudCoverageAt50Pct={handleSetClampCoverage}
        showPressureLines={preferences.showPressureLines}
        setShowPressureLines={handleSetShowPressureLines}
        showWindBarbs={preferences.showWindBarbs}
        setShowWindBarbs={handleSetShowWindBarbs}
        showIsothermLines={preferences.showIsothermLines}
        setShowIsothermLines={handleSetShowIsothermLines}
      />
      <main className="items-center justify-between p-4">
        <MeteogramWrapper
          weatherData={weatherData}
          useLocalTime={preferences.useLocalTime}
          highlightCeilingCoverage={preferences.highlightCeilingCoverage}
          clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
          showPressureLines={preferences.showPressureLines}
          showWindBarbs={preferences.showWindBarbs}
          showIsothermLines={preferences.showIsothermLines}
          isLoading={isLoading}
          error={error}
          model={model}
        />
      </main>
    </>
  );
}
