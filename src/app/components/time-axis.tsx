"use client";

import dynamic from "next/dynamic";
import type { AxisBottom as AxisBottomType, AxisScale } from "@visx/axis";
import { timeFormat, utcFormat } from "@visx/vendor/d3-time-format";
import { scaleTime } from "@visx/scale";
import { useState, useEffect } from "react";

const AxisBottom = dynamic<React.ComponentProps<typeof AxisBottomType>>(
  () => import("@visx/axis").then((mod) => mod.AxisBottom),
  {
    ssr: false,
    loading: () => null, // Render nothing while loading
  },
);

interface TimeAxisProps {
  left: number;
  top: number;
  scale: AxisScale;
  useLocalTime: boolean;
  stroke: string;
  tickStroke: string;
}

export default function TimeAxis({
  left,
  top,
  scale,
  useLocalTime,
  stroke,
  tickStroke,
}: TimeAxisProps) {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  // Always render the component, but conditionally render the tick labels
  return (
    <AxisBottom
      left={left}
      top={top}
      scale={scale}
      tickFormat={(value: Date | number) => {
        if (!mounted) {
          return ""; // Don't show any ticks until mounted
        }
        if (value instanceof Date) {
          return useLocalTime
            ? timeFormat("%d%H")(value)
            : utcFormat("%d%HZ")(value);
        }
        const date = new Date(Number(value));
        return useLocalTime
          ? timeFormat("%d%H")(date)
          : utcFormat("%d%HZ")(date);
      }}
      stroke={stroke}
      tickStroke={tickStroke}
      tickLabelProps={{
        fill: stroke,
        fontSize: 11,
        textAnchor: "middle",
      }}
    />
  );
}
