import React from "react";
import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import CloudColumns from "@/app/components/cloud-columns";
import MeteogramTooltip from "@/app/components/meteogram-tooltip";
import {
  parseNativeForecast,
  transformNativeForecast,
} from "@/utils/native-weather";
import { withNativeClouds } from "./__helpers__/nativeForecast";

describe("native cloud rendering", () => {
  it("renders 50 cloud rectangles and hovers their exact cloud data alongside pressure-level atmosphere", () => {
    const { data } = transformNativeForecast(
      parseNativeForecast(withNativeClouds(), "hrrr"),
      Date.parse("2026-10-18T19:15Z"),
    );
    const onHover = vi.fn();
    const { container } = render(
      <svg>
        <CloudColumns
          weatherData={[data[0]]}
          scales={{
            dateScale: (d: Date) => d.getTime() / 3600000,
            mslScale: (h: number) => 20000 - h,
            cloudScale: (c: number) => c / 100,
          }}
          barWidth={1}
          pressureLevels={data[0].cloud.map((c) => c.hpa)}
          highlightCeilingCoverage={false}
          clampCloudCoverageAt50Pct={false}
          showWindBarbs={false}
          showStabilityTint={false}
          showParcelBuoyancy={false}
          parcelProfiles={[]}
          model="gfs_hrrr"
          frozenRect={null}
          onHover={onHover}
          onFreezeChange={vi.fn()}
        />
      </svg>,
    );
    expect(container.querySelectorAll("rect.native-cloud-cell")).toHaveLength(
      50,
    );
    expect(container.querySelectorAll("rect.cloud-cell")).toHaveLength(50);
    const rect = container.querySelector('[data-model-level="49"]')!;
    fireEvent.mouseEnter(rect);
    const [date, cell, layer] = onHover.mock.calls.at(-1)!;
    expect(layer).toMatchObject({
      modelLevel: 49,
      cloudCoverage: 80,
      hpa: 985,
    });
    expect(cell.hpa).toBe(975);
    render(
      <svg>
        <MeteogramTooltip
          date={date}
          cloudCell={cell}
          nativeCloudLayer={layer}
          x={0}
          y={0}
          useLocalTime={false}
        />
      </svg>,
    );
    expect(screen.getByText("Native cloud · model level 49")).toBeTruthy();
    expect(screen.getByText("Cloud Cover: 80%")).toBeTruthy();
    expect(screen.getByText("Atmosphere at 975 hPa")).toBeTruthy();
  });
});
