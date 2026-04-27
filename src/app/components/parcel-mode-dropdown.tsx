import React from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
} from "@heroui/react";
import { ParcelMode } from "../../types/weather";
import { PARCEL_MODES, PARCEL_MODE_LABELS } from "../../utils/condensation";

interface ParcelModeDropdownProps {
  parcelMode: ParcelMode;
  setParcelMode: (mode: ParcelMode) => void;
}

export default function ParcelModeDropdown({
  parcelMode,
  setParcelMode,
}: ParcelModeDropdownProps) {
  return (
    <Dropdown>
      <DropdownTrigger>
        <Button variant="bordered" size="sm" className="capitalize">
          Parcel: <code className="ml-1">{PARCEL_MODE_LABELS[parcelMode]}</code>
        </Button>
      </DropdownTrigger>
      <DropdownMenu
        aria-label="Parcel mode"
        variant="flat"
        disallowEmptySelection
        selectionMode="single"
        selectedKeys={[parcelMode]}
        onAction={(key) => setParcelMode(key as ParcelMode)}
      >
        {PARCEL_MODES.map((mode) => (
          <DropdownItem key={mode}>{PARCEL_MODE_LABELS[mode]}</DropdownItem>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}
