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

  return (
    <AxisBottom
      left={left}
      top={top}
      scale={scale}
      tickFormat={(value) => {
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
