"use client";

import React, { useState, useEffect } from "react";
import { timeFormat } from "@visx/vendor/d3-time-format";

const lastUpdateFormat = timeFormat("%H:%M:%S");

interface LastUpdateTimeProps {
  lastUpdate: Date | null;
}

const LastUpdateTime = ({ lastUpdate }: LastUpdateTimeProps) => {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  // Don't render anything on the server or before hydration
  if (!mounted) {
    return <span>Last Update: Loading...</span>;
  }

  return (
    <span>
      Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
    </span>
  );
};

export default React.memo(LastUpdateTime);
