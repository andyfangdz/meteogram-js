import React, { Dispatch, SetStateAction } from "react";
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
} from "@nextui-org/react";
import LocationDropdown from "./location-dropdown";
import ModelDropdown from "./model-dropdown";
import { timeFormat } from "@visx/vendor/d3-time-format";
const lastUpdateFormat = timeFormat("%H:%M:%S");

export default function Nav({
  location,
  setLocation,
  model,
  setModel,
  updateWeatherData,
  lastUpdate,
  useLocalTime,
  setUseLocalTime,
  highlightCeilingCoverage,
  sethighlightCeilingCoverage,
}: {
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
  model: string;
  setModel: Dispatch<SetStateAction<string>>;
  updateWeatherData: () => void;
  lastUpdate: Date | null;
  useLocalTime: boolean;
  setUseLocalTime: Dispatch<SetStateAction<boolean>>;
  highlightCeilingCoverage: boolean;
  sethighlightCeilingCoverage: Dispatch<SetStateAction<boolean>>;
  clampCloudCoverageAt50Pct: boolean;
  setclampCloudCoverageAt50Pct: Dispatch<SetStateAction<boolean>>;
}) {
  const [isMenuOpen, setIsMenuOpen] = React.useState(false);

  const menuItems = [
    "Profile",
    "Dashboard",
    "Activity",
    "Analytics",
    "System",
    "Deployments",
    "My Settings",
    "Team Settings",
    "Help & Feedback",
    "Log Out",
  ];

  return (
    <Navbar onMenuOpenChange={setIsMenuOpen}>
      <NavbarContent>
        <NavbarMenuToggle
          aria-label={isMenuOpen ? "Close menu" : "Open menu"}
        />
        <NavbarBrand>
          <p className="font-bold text-inherit">Meteogram</p>
        </NavbarBrand>
      </NavbarContent>

      <NavbarContent className="hidden sm:flex gap-4" justify="center">
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
        <NavbarItem>
          <ButtonGroup>
            <Button color="primary" onClick={updateWeatherData}>
              Refresh
            </Button>
          </ButtonGroup>
        </NavbarItem>
      </NavbarContent>
      <NavbarMenu>
        <NavbarMenuItem>
          <LocationDropdown location={location} setLocation={setLocation} />
        </NavbarMenuItem>
        <NavbarMenuItem>
          <ModelDropdown model={model} setModel={setModel} />
        </NavbarMenuItem>
        <NavbarMenuItem>
          <Chip>
            Last Update: {lastUpdate ? lastUpdateFormat(lastUpdate) : "Never"}
          </Chip>
        </NavbarMenuItem>
        <NavbarMenuItem>
          <Switch isSelected={useLocalTime} onValueChange={setUseLocalTime}>
            Use Local Time
          </Switch>
        </NavbarMenuItem>
        <NavbarMenuItem>
          <Switch
            isSelected={highlightCeilingCoverage}
            onValueChange={sethighlightCeilingCoverage}
          >
            Highlight Ceiling Coverage
          </Switch>
        </NavbarMenuItem>
        <NavbarMenuItem>
          <Switch
            isSelected={clampCloudCoverageAt50Pct}
            onValueChange={setclampCloudCoverageAt50Pct}
          >
            Clamp Cloud Coverage at 50%
          </Switch>
        </NavbarMenuItem>
      </NavbarMenu>
    </Navbar>
  );
}
