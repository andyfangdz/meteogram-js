import { useMemo } from "react";
import { scaleLinear, scaleTime } from "@visx/scale";
import { CloudColumn } from "../types/weather";

interface Bounds {
  xMax: number;
  yMax: number;
}

interface MeteogramScales {
  dateScale: any;
  mslScale: any;
  pressureScale: any;
  cloudScale: any;
}

export const useMeteogramScales = (
  weatherData: CloudColumn[],
  bounds: Bounds,
  clampCloudCoverageAt50Pct: boolean,
): MeteogramScales => {
  return useMemo(() => {
    if (weatherData.length === 0) {
      return {
        dateScale: scaleTime({
          domain: [new Date(), new Date()],
          range: [0, bounds.xMax],
        }),
        mslScale: scaleLinear<number>({
          domain: [0, 20_000],
          range: [bounds.yMax, 0],
        }),
        pressureScale: scaleLinear<number>({
          domain: [250, 1000],
          range: [bounds.yMax, 0],
        }),
        cloudScale: scaleLinear<number>({
          domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
          range: [0, 1],
          clamp: true,
        }),
      };
    }

    return {
      dateScale: scaleTime({
        domain: [weatherData[0].date, weatherData[weatherData.length - 1].date],
        range: [0, bounds.xMax],
      }),
      mslScale: scaleLinear<number>({
        domain: [0, 20_000],
        range: [bounds.yMax, 0],
      }),
      pressureScale: scaleLinear<number>({
        domain: [250, 1000],
        range: [bounds.yMax, 0],
      }),
      cloudScale: scaleLinear<number>({
        domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
        range: [0, 1],
        clamp: true,
      }),
    };
  }, [bounds, weatherData, clampCloudCoverageAt50Pct]);
};
