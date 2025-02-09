import React, { Dispatch, SetStateAction } from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
} from "@heroui/react";
import { LOCATIONS } from "./meteo-vars";

export default function LocationDropdown({
  location,
  setLocation,
}: {
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
}) {
  return (
    <Dropdown>
      <DropdownTrigger>
        <Button variant="bordered" className="capitalize">
          <code>{location}</code>
        </Button>
      </DropdownTrigger>
      <DropdownMenu
        aria-label="Single selection example"
        variant="flat"
        disallowEmptySelection
        selectionMode="single"
        selectedKeys={[location]}
        onAction={(location) => setLocation(location as string)}
      >
        {Object.keys(LOCATIONS).map((location) => (
          <DropdownItem key={location}>
            <code>{location}</code>
          </DropdownItem>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}
