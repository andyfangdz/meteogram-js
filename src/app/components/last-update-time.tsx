"use client";

import { timeFormat } from "@visx/vendor/d3-time-format";

const lastUpdateFormat = timeFormat("%H:%M:%S");

interface LastUpdateTimeProps {
  lastUpdate: Date | null;
}

export default function LastUpdateTime({ lastUpdate }: LastUpdateTimeProps) {
  return (
    <span>
      Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
    </span>
  );
}
