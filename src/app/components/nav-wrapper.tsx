import { Dispatch, SetStateAction, useCallback } from "react";
import { WeatherModel, ParcelMode } from "../../types/weather";
import Nav from "./nav";
import { usePreferences } from "@/context/PreferencesContext";

interface NavWrapperProps {
  model: WeatherModel;
  setModel: (newModel: WeatherModel) => void;
  location: string;
  setLocation: (newLocation: string) => void;
  lastUpdate: Date | null;
  refetch: () => void;
}

export default function NavWrapper({
  model,
  setModel,
  location,
  setLocation,
  lastUpdate,
  refetch,
}: NavWrapperProps) {
  const { preferences, setPreference } = usePreferences();

  // Memoize all setter functions to prevent unnecessary re-renders in Nav component
  // setPreference from PreferencesContext is already stable (wrapped in useCallback)
  const setUseLocalTime = useCallback(
    (val: boolean) => setPreference("useLocalTime", val),
    [setPreference]
  );

  const setHighlightCeilingCoverage = useCallback(
    (val: boolean) => setPreference("highlightCeilingCoverage", val),
    [setPreference]
  );

  const setClampCloudCoverageAt50Pct = useCallback(
    (val: boolean) => setPreference("clampCloudCoverageAt50Pct", val),
    [setPreference]
  );

  const setShowPressureLines = useCallback(
    (val: boolean) => setPreference("showPressureLines", val),
    [setPreference]
  );

  const setShowWindBarbs = useCallback(
    (val: boolean) => setPreference("showWindBarbs", val),
    [setPreference]
  );

  const setShowIsothermLines = useCallback(
    (val: boolean) => setPreference("showIsothermLines", val),
    [setPreference]
  );

  const setShowIsotachLines = useCallback(
    (val: boolean) => setPreference("showIsotachLines", val),
    [setPreference]
  );

  const setShowDewPointDepressionLines = useCallback(
    (val: boolean) => setPreference("showDewPointDepressionLines", val),
    [setPreference]
  );

  const setShowStabilityTint = useCallback(
    (val: boolean) => setPreference("showStabilityTint", val),
    [setPreference]
  );

  const setShowCondensationLevels = useCallback(
    (val: boolean) => setPreference("showCondensationLevels", val),
    [setPreference]
  );

  const setShowParcelBuoyancy = useCallback(
    (val: boolean) => setPreference("showParcelBuoyancy", val),
    [setPreference]
  );

  const setParcelMode = useCallback(
    (val: ParcelMode) => setPreference("parcelMode", val),
    [setPreference]
  );

  return (
    <Nav
      location={location}
      setLocation={setLocation}
      model={model}
      setModel={setModel}
      updateWeatherData={refetch}
      lastUpdate={lastUpdate}
      useLocalTime={preferences.useLocalTime}
      setUseLocalTime={setUseLocalTime}
      highlightCeilingCoverage={preferences.highlightCeilingCoverage}
      setHighlightCeilingCoverage={setHighlightCeilingCoverage}
      clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
      setClampCloudCoverageAt50Pct={setClampCloudCoverageAt50Pct}
      showPressureLines={preferences.showPressureLines}
      setShowPressureLines={setShowPressureLines}
      showWindBarbs={preferences.showWindBarbs}
      setShowWindBarbs={setShowWindBarbs}
      showIsothermLines={preferences.showIsothermLines}
      setShowIsothermLines={setShowIsothermLines}
      showIsotachLines={preferences.showIsotachLines}
      setShowIsotachLines={setShowIsotachLines}
      showDewPointDepressionLines={preferences.showDewPointDepressionLines}
      setShowDewPointDepressionLines={setShowDewPointDepressionLines}
      showStabilityTint={preferences.showStabilityTint}
      setShowStabilityTint={setShowStabilityTint}
      showCondensationLevels={preferences.showCondensationLevels}
      setShowCondensationLevels={setShowCondensationLevels}
      showParcelBuoyancy={preferences.showParcelBuoyancy}
      setShowParcelBuoyancy={setShowParcelBuoyancy}
      parcelMode={preferences.parcelMode}
      setParcelMode={setParcelMode}
    />
  );
}
