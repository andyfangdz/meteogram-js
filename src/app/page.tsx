"use client";
import Meteogram from "./meteogram";
import {
  Divider,
  Navbar,
  NavbarBrand,
  NextUIProvider,
} from "@nextui-org/react";
import { Switch } from "@nextui-org/switch";
import { Chip } from "@nextui-org/chip";
import { timeFormat } from "@visx/vendor/d3-time-format";
import { Button, ButtonGroup } from "@nextui-org/button";
import { useEffect, useState, useRef } from "react";
import fetchWeatherData, { CloudData } from "./meteo-vars";
import ModelDropdown from "./model-dropdown";
import LocationDropdown from "./location-dropdown";
import { useTimeout } from "usehooks-ts";

const lastUpdateFormat = timeFormat("%H:%M:%S");

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  let [highlightCeilingCoverage, sethighlightCeilingCoverage] =
    useState<boolean>(true);
  let [weatherData, setWeatherData] = useState<CloudData[]>([]);
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
      <main className="items-center justify-between p-24">
        <div className="controls flex flex-row">
          <LocationDropdown location={location} setLocation={setLocation} />
          <ModelDropdown model={model} setModel={setModel} />
          <Chip>
            Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
          </Chip>
          <ButtonGroup>
            <Button color="primary" onClick={updateWeatherData}>
              Refresh
            </Button>
          </ButtonGroup>

          <Switch isSelected={useLocalTime} onValueChange={setUseLocalTime}>
            Use Local Time
          </Switch>
          <Switch
            isSelected={highlightCeilingCoverage}
            onValueChange={sethighlightCeilingCoverage}
          >
            Highlight Ceiling Coverage
          </Switch>
        </div>
        <div className="contents">
          <Meteogram
            width={1200}
            height={800}
            useLocalTime={useLocalTime}
            weatherData={weatherData}
            highlightCeilingCoverage={highlightCeilingCoverage}
          />
        </div>
      </main>
    </NextUIProvider>
  );
}
