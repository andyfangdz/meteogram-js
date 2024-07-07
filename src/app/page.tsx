"use client";
import Meteogram from "./meteogram";
import { Divider, Navbar, NavbarBrand, NextUIProvider } from "@nextui-org/react";
import { Switch } from "@nextui-org/switch";
import {Chip} from "@nextui-org/chip";
import { timeFormat } from "@visx/vendor/d3-time-format";
import { Button, ButtonGroup } from "@nextui-org/button";
import { useEffect, useState } from "react";
import fetchWeatherData, { CloudData } from "./meteo-vars";

const lastUpdateFormat = timeFormat("%H:%M:%S");

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  let [weatherData, setWeatherData] = useState<CloudData[]>([]);
  let [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  const updateWeatherData = () => {
    fetchWeatherData().then((data) => {
      setWeatherData(data);
      setLastUpdate(new Date());
      setTimeout(updateWeatherData, 60 * 1000);
    });
  };
  useEffect(updateWeatherData, []);
  return (
    <NextUIProvider>
      <main className="items-center justify-between p-24">
        <Navbar >
          <NavbarBrand>KFRG</NavbarBrand>
        <Chip>Model: <code>gfs_hrrr</code></Chip>
        <Chip>Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}</Chip>
        <ButtonGroup>
          <Button color="primary" onClick={updateWeatherData}>
            Refresh
          </Button>
        </ButtonGroup>
        </Navbar>
        <Switch isSelected={useLocalTime} onValueChange={setUseLocalTime}>
          Use Local Time
        </Switch>

        <Divider />
        <Meteogram
          width={1200}
          height={600}
          useLocalTime={useLocalTime}
          weatherData={weatherData}
        />
      </main>
    </NextUIProvider>
  );
}
