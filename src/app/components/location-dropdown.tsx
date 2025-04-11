import React, { Dispatch, SetStateAction, useState, useCallback } from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
  Input,
  Spinner,
} from "@heroui/react";
import { LOCATIONS } from "../../config/weather";
import { geocodeLocation, debounce } from "../../services/geocoding";
import { useRouter } from "next/navigation";

export default function LocationDropdown({
  location,
  setLocation,
}: {
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
}) {
  const router = useRouter();
  const [searchQuery, setSearchQuery] = useState("");
  const [customLocations, setCustomLocations] = useState<typeof LOCATIONS>({});
  const [isLoading, setIsLoading] = useState(false);

  // Extract the display name from the location string (removing @coords if present)
  const displayName = location.split("@")[0];

  const handleSearch = useCallback(
    debounce(async (query: string) => {
      if (!query) return;

      setIsLoading(true);
      const result = await geocodeLocation(query);
      setCustomLocations(result);
      setIsLoading(false);
    }, 500),
    [],
  );

  const handleLocationSelect = (loc: string) => {
    if (loc in LOCATIONS) {
      setLocation(loc);
    } else {
      // For custom locations, append coordinates to the name
      const coords = customLocations[loc];
      if (coords) {
        const coordString = `${loc}@${coords.latitude},${coords.longitude}`;
        setLocation(coordString);
      }
    }
  };

  return (
    <Dropdown>
      <DropdownTrigger>
        <Button variant="bordered" className="capitalize">
          <code>{displayName}</code>
        </Button>
      </DropdownTrigger>
      <DropdownMenu
        aria-label="Location selection"
        variant="flat"
        disallowEmptySelection
        selectionMode="single"
        selectedKeys={[displayName]}
        onAction={(loc) => handleLocationSelect(loc as string)}
      >
        <DropdownItem key="search" isReadOnly>
          <div className="flex items-center gap-2">
            <Input
              placeholder="Search location or ICAO code..."
              value={searchQuery}
              onChange={(e) => {
                const query = e.target.value;
                setSearchQuery(query);
                handleSearch(query);
              }}
              className="w-full"
            />
            {isLoading && <Spinner size="sm" />}
          </div>
        </DropdownItem>
        {/* Show custom locations first */}
        {Object.entries(customLocations).map(([loc, _]) => (
          <DropdownItem key={loc}>
            <code>{loc}</code>
          </DropdownItem>
        ))}
        {/* Show predefined locations after */}
        {Object.entries(LOCATIONS).map(([loc, _]) => (
          <DropdownItem key={loc}>
            <code>{loc}</code>
          </DropdownItem>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}
