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
import { useEffect, useState } from "react";
import fetchWeatherData, { CloudData } from "./meteo-vars";
import ModelDropdown from "./model-dropdown";
import LocationDropdown from "./location-dropdown";

const lastUpdateFormat = timeFormat("%H:%M:%S");

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  let [weatherData, setWeatherData] = useState<CloudData[]>([]);
  let [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  let [location, setLocation] = useState<string>("KFRG");
  let [model, setModel] = useState<string>("gfs_hrrr");

  const updateWeatherData = () => {
    fetchWeatherData(model, location).then((data) => {
      setWeatherData(data);
      setLastUpdate(new Date());
      // setTimeout(updateWeatherData, 60 * 1000);
    });
  };
  useEffect(updateWeatherData, [model, location]);
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
        </div>
        <div className="contents">
          <Meteogram
            width={1200}
            height={800}
            useLocalTime={useLocalTime}
            weatherData={weatherData}
          />
        </div>
      </main>
    </NextUIProvider>
  );
}
