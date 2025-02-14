import React, {
  Dispatch,
  SetStateAction,
  createContext,
  useContext,
  useCallback,
} from "react";
import {
  Navbar,
  NavbarBrand,
  NavbarContent,
  NavbarItem,
  NavbarMenuToggle,
  NavbarMenu,
  NavbarMenuItem,
  Button,
  ButtonGroup,
  Chip,
  Switch,
} from "@heroui/react";
import { WeatherModel } from "../../types/weather";
import LocationDropdown from "./location-dropdown";
import ModelDropdown from "./model-dropdown";
import LastUpdateTime from "./last-update-time";

interface NavProps {
  model: WeatherModel;
  setModel: Dispatch<SetStateAction<WeatherModel>>;
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
  lastUpdate: Date | null;
  updateWeatherData: () => void;
  useLocalTime: boolean;
  setUseLocalTime: Dispatch<SetStateAction<boolean>>;
  highlightCeilingCoverage: boolean;
  setHighlightCeilingCoverage: Dispatch<SetStateAction<boolean>>;
  clampCloudCoverageAt50Pct: boolean;
  setClampCloudCoverageAt50Pct: Dispatch<SetStateAction<boolean>>;
  showPressureLines: boolean;
  setShowPressureLines: Dispatch<SetStateAction<boolean>>;
  showWindBarbs: boolean;
  setShowWindBarbs: Dispatch<SetStateAction<boolean>>;
  showIsothermLines: boolean;
  setShowIsothermLines: Dispatch<SetStateAction<boolean>>;
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
  showIsothermLines: boolean;
  setShowIsothermLines: (value: boolean) => void;
}>({
  useLocalTime: false,
  setUseLocalTime: () => {},
  highlightCeilingCoverage: true,
  setHighlightCeilingCoverage: () => {},
  clampCloudCoverageAt50Pct: true,
  setClampCloudCoverageAt50Pct: () => {},
  showPressureLines: false,
  setShowPressureLines: () => {},
  showIsothermLines: false,
  setShowIsothermLines: () => {},
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
  showWindBarbs,
  setShowWindBarbs,
  showIsothermLines,
  setShowIsothermLines,
}: NavProps) {
  const [isMenuOpen, setIsMenuOpen] = React.useState(false);

  const handleMenuToggle = useCallback(() => {
    setIsMenuOpen((prev) => !prev);
  }, []);

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
      <Switch isSelected={showWindBarbs} onValueChange={setShowWindBarbs}>
        Show Wind Barbs
      </Switch>
      <Switch
        isSelected={showIsothermLines}
        onValueChange={setShowIsothermLines}
      >
        Show Isotherm Lines
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
        showIsothermLines,
        setShowIsothermLines,
      }}
    >
      <Navbar
        className="relative z-[1000]"
        shouldHideOnScroll={false}
        disableScrollHandler={true}
      >
        <NavbarContent>
          <NavbarMenuToggle
            className="md:hidden"
            aria-label={isMenuOpen ? "Close menu" : "Open menu"}
          />
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
              <LastUpdateTime lastUpdate={lastUpdate} />
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
          <NavbarItem className="hidden md:flex">
            <button
              className="p-4 cursor-pointer hover:bg-default-100 rounded-lg"
              aria-label={isMenuOpen ? "Close menu" : "Open menu"}
              onClick={handleMenuToggle}
            >
              <svg
                className="w-6 h-6"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                {isMenuOpen ? (
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                ) : (
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M4 6h16M4 12h16M4 18h16"
                  />
                )}
              </svg>
            </button>
          </NavbarItem>
        </NavbarContent>

        {/* Mobile Menu */}
        <NavbarMenu>
          <div className="flex flex-col gap-4 mt-4">
            <LocationDropdown location={location} setLocation={setLocation} />
            <ModelDropdown model={model} setModel={setModel} />
            <Chip>
              <LastUpdateTime lastUpdate={lastUpdate} />
            </Chip>
            <Button color="primary" onPress={updateWeatherData}>
              Refresh
            </Button>
            <PreferencesPanel />
          </div>
        </NavbarMenu>
      </Navbar>

      {/* Desktop Side Panel */}
      <div
        className={`hidden md:block fixed right-0 top-[64px] h-[calc(100vh-64px)] w-80 bg-background shadow-lg z-[999] transition-[right] duration-300 ease-in-out ${
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
