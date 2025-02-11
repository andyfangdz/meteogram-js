import React, {
  Dispatch,
  SetStateAction,
  createContext,
  useContext,
} from "react";
import {
  Navbar,
  NavbarBrand,
  NavbarContent,
  NavbarItem,
  NavbarMenuToggle,
  Button,
  ButtonGroup,
  Chip,
  Switch,
} from "@heroui/react";
import LocationDropdown from "./location-dropdown";
import ModelDropdown from "./model-dropdown";
import { timeFormat } from "@visx/vendor/d3-time-format";
const lastUpdateFormat = timeFormat("%H:%M:%S");

interface NavProps {
  model: WeatherModel;
  setModel: (model: WeatherModel) => void;
  location: string;
  setLocation: (location: string) => void;
  lastUpdate: Date | null;
  updateWeatherData: () => void;
  useLocalTime: boolean;
  setUseLocalTime: (value: boolean) => void;
  highlightCeilingCoverage: boolean;
  setHighlightCeilingCoverage: (value: boolean) => void;
  clampCloudCoverageAt50Pct: boolean;
  setClampCloudCoverageAt50Pct: (value: boolean) => void;
  showPressureLines: boolean;
  setShowPressureLines: (value: boolean) => void;
}

const NavContext = createContext<{
  useLocalTime: boolean;
  setUseLocalTime: (value: boolean) => void;
  highlightCeilingCoverage: boolean;
  setHighlightCeilingCoverage: (value: boolean) => void;
  clampCloudCoverageAt50Pct: boolean;
  setClampCloudCoverageAt50Pct: (value: boolean) => void;
  showPressureLines: boolean;
  setShowPressureLines: (value: boolean) => void;
}>({
  useLocalTime: false,
  setUseLocalTime: () => {},
  highlightCeilingCoverage: true,
  setHighlightCeilingCoverage: () => {},
  clampCloudCoverageAt50Pct: true,
  setClampCloudCoverageAt50Pct: () => {},
  showPressureLines: false,
  setShowPressureLines: () => {},
});

export default function Nav({
  model,
  setModel,
  location,
  setLocation,
  lastUpdate,
  updateWeatherData,
  useLocalTime,
  setUseLocalTime,
  highlightCeilingCoverage,
  setHighlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  setClampCloudCoverageAt50Pct,
  showPressureLines,
  setShowPressureLines,
}: NavProps) {
  const [isMenuOpen, setIsMenuOpen] = React.useState(false);

  const PreferencesPanel = () => (
    <div className="flex flex-col gap-4 p-4">
      <Switch isSelected={useLocalTime} onValueChange={setUseLocalTime}>
        Use Local Time
      </Switch>
      <Switch
        isSelected={highlightCeilingCoverage}
        onValueChange={setHighlightCeilingCoverage}
      >
        Highlight Ceiling Coverage
      </Switch>
      <Switch
        isSelected={clampCloudCoverageAt50Pct}
        onValueChange={setClampCloudCoverageAt50Pct}
      >
        Clamp Cloud Coverage at 50%
      </Switch>
      <Switch
        isSelected={showPressureLines}
        onValueChange={setShowPressureLines}
      >
        Show Pressure Lines
      </Switch>
    </div>
  );

  return (
    <NavContext.Provider
      value={{
        useLocalTime,
        setUseLocalTime,
        highlightCeilingCoverage,
        setHighlightCeilingCoverage,
        clampCloudCoverageAt50Pct,
        setClampCloudCoverageAt50Pct,
        showPressureLines,
        setShowPressureLines,
      }}
    >
      <Navbar className="relative z-[51]">
        <NavbarContent>
          <NavbarBrand>
            <p className="font-bold text-inherit">Meteogram</p>
          </NavbarBrand>
        </NavbarContent>

        <NavbarContent className="hidden md:flex gap-4" justify="center">
          <NavbarItem>
            <LocationDropdown location={location} setLocation={setLocation} />
          </NavbarItem>
          <NavbarItem>
            <ModelDropdown model={model} setModel={setModel} />
          </NavbarItem>
          <NavbarItem>
            <Chip>
              Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
            </Chip>
          </NavbarItem>
        </NavbarContent>

        <NavbarContent justify="end">
          <NavbarItem className="hidden md:flex">
            <ButtonGroup>
              <Button color="primary" onPress={updateWeatherData}>
                Refresh
              </Button>
            </ButtonGroup>
          </NavbarItem>
          <NavbarItem>
            <NavbarMenuToggle
              className="md:ml-4 p-2 cursor-pointer hover:bg-default-100 rounded-lg"
              aria-label={isMenuOpen ? "Close menu" : "Open menu"}
              onClick={() => setIsMenuOpen(!isMenuOpen)}
            />
          </NavbarItem>
        </NavbarContent>
      </Navbar>

      {/* Mobile Menu Overlay */}
      <div
        className={`fixed inset-0 z-50 md:hidden transition-all duration-300 ease-in-out ${
          isMenuOpen ? "visible opacity-100" : "invisible opacity-0"
        }`}
        onClick={(e) => {
          // Close menu when clicking outside
          if (e.target === e.currentTarget) {
            setIsMenuOpen(false);
          }
        }}
      >
        <div
          className={`fixed right-0 h-full w-full max-w-xs bg-background shadow-lg pt-16 transition-transform duration-300 ease-in-out ${
            isMenuOpen ? "translate-x-0" : "translate-x-full"
          }`}
          style={{ WebkitOverflowScrolling: "touch" }}
        >
          <div className="flex flex-col gap-4 p-4 overflow-y-auto h-full">
            <LocationDropdown location={location} setLocation={setLocation} />
            <ModelDropdown model={model} setModel={setModel} />
            <Chip>
              Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
            </Chip>
            <Button color="primary" onPress={updateWeatherData}>
              Refresh
            </Button>
            <PreferencesPanel />
          </div>
        </div>
      </div>

      {/* Desktop Side Panel */}
      <div
        className={`hidden md:block fixed right-0 top-[64px] h-[calc(100vh-64px)] w-80 bg-background shadow-lg transition-[right] duration-300 ease-in-out ${
          isMenuOpen ? "right-0" : "right-[-100%]"
        }`}
        style={{ WebkitOverflowScrolling: "touch" }}
      >
        <div className="overflow-y-auto h-full">
          <PreferencesPanel />
        </div>
      </div>
    </NavContext.Provider>
  );
}
