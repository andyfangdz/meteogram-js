"use client";

import { Dispatch, SetStateAction } from "react";
import NavWrapper from "./nav-wrapper";
import MeteogramWrapper from "./meteogram-wrapper";
import {
  CloudColumn,
  WeatherModel,
  VisualizationPreferences,
  WeatherSource,
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
  source?: WeatherSource;
}

export default function VisualizationPreferencesComponent({
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
  source,
}: VisualizationPreferencesProps) {
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
        {source && (
          <div
            className="mb-3 text-sm text-default-600"
            data-weather-provider={source.provider}
          >
            <a
              className="underline underline-offset-2"
              href={
                source.provider === "dynamical"
                  ? "https://dynamical.org"
                  : "https://open-meteo.com"
              }
              target="_blank"
              rel="noreferrer"
            >
              {source.provider === "dynamical" ? "Dynamical" : "Open-Meteo"}
            </a>
            {" · "}
            {source.model.toUpperCase()}
            {source.initializedAt && (
              <>
                {" · Run "}
                <time dateTime={source.initializedAt}>
                  {source.initializedAt.slice(0, 10)}{" "}
                  {source.initializedAt.slice(11, 16)} UTC
                </time>
              </>
            )}
            {source.complete !== undefined && (
              <>
                {" · "}
                {source.complete ? "Complete" : "Partial"} (
                {source.availableHours}/{source.expectedHours} forecast hours)
              </>
            )}
            {source.cloudMethod === "humidity-estimate" && (
              <>{" · Clouds estimated from humidity"}</>
            )}
            {source.fallback && <>{" · Backup forecast"}</>}
          </div>
        )}
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
}
