"use client";
import Image from "next/image";
import Meteogram from "./meteogram";
import { NextUIProvider } from "@nextui-org/react";
import { Switch } from "@nextui-org/switch";
import { useState } from "react";

export default function Home() {
  let [useLocalTime, setUseLocalTime] = useState<boolean>(false);
  return (
    <NextUIProvider>
      <main className="items-center justify-between p-24">
        <Switch isSelected={useLocalTime} onValueChange={setUseLocalTime}>
          Use Local Time
        </Switch>
        <Meteogram
          width={1200}
          height={600}
          events={true}
          useLocalTime={useLocalTime}
        />
      </main>
    </NextUIProvider>
  );
}
