"use client";

import { Dispatch, SetStateAction } from "react";
import { WeatherModel } from "../../types/weather";
import Nav from "./nav";
import { usePreferences } from "@/context/PreferencesContext";

interface NavWrapperProps {
  model: WeatherModel;
  setModel: Dispatch<SetStateAction<WeatherModel>>;
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
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

  return (
    <Nav
      location={location}
      setLocation={setLocation}
      model={model}
      setModel={setModel}
      updateWeatherData={refetch}
      lastUpdate={lastUpdate}
      useLocalTime={preferences.useLocalTime}
      setUseLocalTime={(val: boolean) => setPreference("useLocalTime", val)}
      highlightCeilingCoverage={preferences.highlightCeilingCoverage}
      setHighlightCeilingCoverage={(val: boolean) =>
        setPreference("highlightCeilingCoverage", val)
      }
      clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
      setClampCloudCoverageAt50Pct={(val: boolean) =>
        setPreference("clampCloudCoverageAt50Pct", val)
      }
      showPressureLines={preferences.showPressureLines}
      setShowPressureLines={(val: boolean) =>
        setPreference("showPressureLines", val)
      }
      showWindBarbs={preferences.showWindBarbs}
      setShowWindBarbs={(val: boolean) => setPreference("showWindBarbs", val)}
      showIsothermLines={preferences.showIsothermLines}
      setShowIsothermLines={(val: boolean) =>
        setPreference("showIsothermLines", val)
      }
    />
  );
}
