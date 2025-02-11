"use client";
import { useState } from "react";
import { HeroUIProvider } from "@heroui/react";
import Meteogram from "./meteogram";
import Nav from "./Nav";
import { useWeatherData } from "../hooks/useWeatherData";
import { WeatherModel } from "../types/weather";

export default function Home() {
  const [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  const [highlightCeilingCoverage, setHighlightCeilingCoverage] =
    useState<boolean>(true);
  const [clampCloudCoverageAt50Pct, setClampCloudCoverageAt50Pct] =
    useState<boolean>(true);
  const [location, setLocation] = useState<string>("KFRG");
  const [model, setModel] = useState<WeatherModel>("gfs_hrrr");

  const { weatherData, lastUpdate, isLoading, error, refetch } = useWeatherData(
    model,
    location,
  );

  return (
    <HeroUIProvider>
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
      <main className="items-center justify-between p-24">
        <div className="contents">
          {error && (
            <div className="text-red-500 mb-4">
              Error loading weather data: {error.message}
            </div>
          )}
          <Meteogram
            width={1600}
            height={800}
            useLocalTime={useLocalTime}
            weatherData={weatherData}
            highlightCeilingCoverage={highlightCeilingCoverage}
            clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
            isLoading={isLoading}
          />
        </div>
      </main>
    </HeroUIProvider>
  );
}
