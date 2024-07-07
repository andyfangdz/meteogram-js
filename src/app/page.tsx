"use client";
import Meteogram from "./meteogram";
import { Divider, Navbar, NavbarBrand, NextUIProvider } from "@nextui-org/react";
import { Switch } from "@nextui-org/switch";
import {Chip} from "@nextui-org/chip";

import { Button, ButtonGroup } from "@nextui-org/button";
import { useEffect, useState } from "react";
import fetchWeatherData, { CloudData } from "./meteo-vars";

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  let [weatherData, setWeatherData] = useState<CloudData[]>([]);
  let [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  const updateWeatherData = () => {
    fetchWeatherData().then((data) => {
      setWeatherData(data);
      setLastUpdate(new Date());
    });
  };
  useEffect(updateWeatherData, []);
  return (
    <NextUIProvider>
      <main className="items-center justify-between p-24">
        <Navbar >
          <NavbarBrand>KFRG</NavbarBrand>
          {"Republic Airport, Farmingdale"}
        <Chip>Model: <code>gfs_hrrr</code></Chip>
        <Chip>Last Update: {lastUpdate?.toLocaleString() ?? "Never"}</Chip>
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
