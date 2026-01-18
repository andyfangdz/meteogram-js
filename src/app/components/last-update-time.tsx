"use client";

import { timeFormat } from "@visx/vendor/d3-time-format";
import { useSyncExternalStore } from "react";

const lastUpdateFormat = timeFormat("%H:%M:%S");

interface LastUpdateTimeProps {
  lastUpdate: Date | null;
}

function subscribe() {
  return () => {};
}

function getSnapshot() {
  return true;
}

function getServerSnapshot() {
  return false;
}

export default function LastUpdateTime({ lastUpdate }: LastUpdateTimeProps) {
  const mounted = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  // Don't render anything on the server or before hydration
  if (!mounted) {
    return <span>Last Update: Loading...</span>;
  }

  return (
    <span>
      Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
    </span>
  );
}
