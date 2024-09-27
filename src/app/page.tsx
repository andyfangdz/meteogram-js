"use client";
import Meteogram from "./meteogram";
import { NextUIProvider } from "@nextui-org/react";
import { useEffect, useState, useRef } from "react";
import fetchWeatherData, { CloudColumn } from "./meteo-vars";
import Nav from "./Nav";

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  let [highlightCeilingCoverage, sethighlightCeilingCoverage] =
    useState<boolean>(true);
  let [clampCloudCoverageAt50Pct, setclampCloudCoverageAt50Pct] =
    useState<boolean>(true);
  let [weatherData, setWeatherData] = useState<CloudColumn[]>([]);
  let [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  let [location, setLocation] = useState<string>("KFRG");
  let [model, setModel] = useState<string>("gfs_hrrr");
  const timerRef = useRef<NodeJS.Timeout | null>(null);

  const updateWeatherData = () => {
    fetchWeatherData(model, location).then((data) => {
      setWeatherData(data);
      setLastUpdate(new Date());
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      timerRef.current = setTimeout(updateWeatherData, 60 * 1000);
    });
  };
  useEffect(updateWeatherData, [model, location]);
  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, []);
  return (
    <NextUIProvider>
      <Nav
        location={location}
        setLocation={setLocation}
        model={model}
        setModel={setModel}
        updateWeatherData={updateWeatherData}
        lastUpdate={lastUpdate}
        useLocalTime={useLocalTime}
        setUseLocalTime={setUseLocalTime}
        highlightCeilingCoverage={highlightCeilingCoverage}
        sethighlightCeilingCoverage={sethighlightCeilingCoverage}
        clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
        setclampCloudCoverageAt50Pct={setclampCloudCoverageAt50Pct}
      />
      <main className="items-center justify-between p-24">
        <div className="contents">
          <Meteogram
            width={1600}
            height={800}
            useLocalTime={useLocalTime}
            weatherData={weatherData}
            highlightCeilingCoverage={highlightCeilingCoverage}
            clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
          />
        </div>
      </main>
    </NextUIProvider>
  );
}
