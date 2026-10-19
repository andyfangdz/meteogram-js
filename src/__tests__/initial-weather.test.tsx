import React from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ClientWrapper from "@/app/components/client-wrapper";
import MeteogramTooltip from "@/app/components/meteogram-tooltip";
import { DEFAULT_PREFERENCES } from "@/config/preferences";
import type { CloudColumn } from "@/types/weather";
import { transformNativeForecast } from "@/utils/native-weather";
import { nativeFixture } from "./__helpers__/nativeForecast";

vi.mock("next/navigation", () => ({ useRouter: () => ({ push: vi.fn() }) }));
vi.mock("@heroui/react", () => ({
  HeroUIProvider: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/context/PreferencesContext", () => ({
  PreferencesProvider: ({ children }: { children: React.ReactNode }) =>
    children,
  usePreferences: () => ({ preferences: DEFAULT_PREFERENCES }),
}));
vi.mock("@/app/components/visualization-preferences", () => ({
  default: ({ weatherData }: { weatherData: CloudColumn[] }) => {
    const column = weatherData[0];
    return (
      <svg>
        <MeteogramTooltip
          date={column.date}
          sampleDate={column.sampleDate}
          initializedAt={column.initializedAt}
          cloudCell={column.cloud[0]}
          x={0}
          y={0}
          useLocalTime={false}
        />
      </svg>
    );
  },
}));

describe("initial forecast JSON round trip", () => {
  it.each([true, false])(
    "restores tooltip dates with split interval=%s",
    (split) => {
      const original = transformNativeForecast(
        nativeFixture("gfs"),
        Date.parse("2026-10-18T19:15Z"),
      ).data[0];
      const data = [
        {
          ...original,
          date: new Date("2026-10-18T23:00Z"),
          sampleDate: split ? new Date("2026-10-18T21:00Z") : undefined,
        },
      ];
      render(
        <ClientWrapper
          initialLocation="KCDW"
          initialModel="gfs_seamless"
          initialWeatherDataStr={JSON.stringify(data)}
          initialTimestamp="2026-10-18T22:00:00Z"
          initialElevationFt={100}
          initialPreferences={DEFAULT_PREFERENCES}
        />,
      );
      expect(screen.getByText("Time: 23:00:00 UTC")).toBeTruthy();
      expect(
        Boolean(screen.queryByText("Native sample: 2026-10-18 21:00 UTC")),
      ).toBe(split);
    },
  );
});
