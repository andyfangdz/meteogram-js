"use client";

import React from "react";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import {
  CloudColumn,
  WeatherModel,
  VisualizationPreferences,
} from "@/types/weather";
import { usePreferences } from "@/context/PreferencesContext";

interface VisualizationPreferencesProps {
  model: WeatherModel;
  setModel: (newModel: WeatherModel) => void;
  location: string;
  setLocation: (newLocation: string) => void;
  lastUpdate: Date;
  refetch: () => void;
  weatherData: CloudColumn[];
  isLoading: boolean;
  error: Error | null;
  elevationFt: number | null;
}

const VisualizationPreferencesComponent = ({
  model,
  setModel,
  location,
  setLocation,
  lastUpdate,
  refetch,
  weatherData,
  isLoading,
  error,
  elevationFt,
}: VisualizationPreferencesProps) => {
  const { preferences, setPreference } = usePreferences();

  return (
    <>
      <NavWrapper
        model={model}
        setModel={setModel}
        location={location}
        setLocation={setLocation}
        lastUpdate={lastUpdate}
        refetch={refetch}
      />
      <main className="items-center justify-between p-4">
        <MeteogramWrapper
          weatherData={weatherData}
          isLoading={isLoading}
          error={error}
          model={model}
          elevationFt={elevationFt}
        />
      </main>
    </>
  );
};

export default React.memo(VisualizationPreferencesComponent);
