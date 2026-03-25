"use client";

import { useState } from "react";
import {
  Navbar,
  NavbarBrand,
  NavbarContent,
  NavbarItem,
  NavbarMenuToggle,
  NavbarMenu,
  Button,
  Input,
  Select,
  SelectItem,
  Chip,
} from "@heroui/react";
import Link from "next/link";
import { WeatherModel } from "../../types/weather";
import { MODEL_NAMES } from "../../config/weather";

function isoToLocalDatetimeValue(isoString: string): string {
  const date = new Date(isoString);
  if (isNaN(date.getTime())) return "";
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  const hours = String(date.getHours()).padStart(2, "0");
  const minutes = String(date.getMinutes()).padStart(2, "0");
  return `${year}-${month}-${day}T${hours}:${minutes}`;
}

function localDatetimeValueToISO(localValue: string): string {
  const date = new Date(localValue);
  if (isNaN(date.getTime())) return new Date().toISOString();
  return date.toISOString();
}

export interface RouteHeaderUpdate {
  waypoints: string;
  model: WeatherModel;
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: string;
  resolutionNM: number;
}

interface RouteHeaderProps {
  waypoints: string;
  model: WeatherModel;
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: string;
  resolutionNM: number;
  onUpdate: (update: RouteHeaderUpdate) => void;
  isLoading: boolean;
}

export default function RouteHeader({
  waypoints,
  model,
  cruiseAltitudeFt,
  tasKnots,
  departureTime,
  resolutionNM,
  onUpdate,
  isLoading,
}: RouteHeaderProps) {
  const [localWaypoints, setLocalWaypoints] = useState(waypoints);
  const [localModel, setLocalModel] = useState<WeatherModel>(model);
  const [localAlt, setLocalAlt] = useState(String(cruiseAltitudeFt));
  const [localTas, setLocalTas] = useState(String(tasKnots));
  const [localDep, setLocalDep] = useState(departureTime);
  const [localRes, setLocalRes] = useState(String(resolutionNM));
  const [isMenuOpen, setIsMenuOpen] = useState(false);

  const handleUpdate = () => {
    onUpdate({
      waypoints: localWaypoints,
      model: localModel,
      cruiseAltitudeFt: parseInt(localAlt, 10) || cruiseAltitudeFt,
      tasKnots: parseInt(localTas, 10) || tasKnots,
      departureTime: localDep,
      resolutionNM: Math.max(5, parseInt(localRes, 10) || resolutionNM),
    });
  };

  return (
    <Navbar
      maxWidth="full"
      isMenuOpen={isMenuOpen}
      onMenuOpenChange={setIsMenuOpen}
      className="border-b border-divider"
      height="auto"
    >
      {/* Mobile toggle */}
      <NavbarContent className="md:hidden" justify="start">
        <NavbarMenuToggle
          className="md:hidden"
          aria-label={isMenuOpen ? "Close menu" : "Open menu"}
        />
        <NavbarBrand>
          <Link href={`/${localWaypoints.split("-")[0] || "KCDW"}/${localModel}`} className="font-bold text-inherit hover:text-primary transition-colors">
            Meteogram
          </Link>
          <Chip size="sm" variant="flat" color="primary" className="ml-2">Route</Chip>
        </NavbarBrand>
      </NavbarContent>

      {/* Desktop: Brand + Route */}
      <NavbarContent className="hidden md:flex" justify="start">
        <NavbarBrand>
          <Link href={`/${localWaypoints.split("-")[0] || "KCDW"}/${localModel}`} className="font-bold text-inherit hover:text-primary transition-colors">
            Meteogram
          </Link>
          <Chip size="sm" variant="flat" color="primary" className="ml-2">Route</Chip>
        </NavbarBrand>
      </NavbarContent>

      {/* Desktop: Inline controls */}
      <NavbarContent className="hidden md:flex gap-2" justify="center">
        <NavbarItem>
          <Input
            placeholder="KCDW-SBJ-KIAD"
            value={localWaypoints}
            onValueChange={setLocalWaypoints}
            size="sm"
            variant="bordered"
            className="w-48"
            classNames={{ input: "font-mono text-sm" }}
            isDisabled={isLoading}
            aria-label="Waypoints"
            startContent={<span className="text-default-400 text-xs whitespace-nowrap">RTE</span>}
          />
        </NavbarItem>
        <NavbarItem>
          <Select
            selectedKeys={[localModel]}
            onSelectionChange={(keys) => {
              const selected = Array.from(keys)[0] as WeatherModel;
              if (selected) setLocalModel(selected);
            }}
            size="sm"
            variant="bordered"
            className="w-36"
            isDisabled={isLoading}
            aria-label="Model"
          >
            {MODEL_NAMES.map((m) => (
              <SelectItem key={m}>{m}</SelectItem>
            ))}
          </Select>
        </NavbarItem>
        <NavbarItem>
          <div className="flex items-center gap-1">
            <Input
              value={localAlt}
              onValueChange={setLocalAlt}
              size="sm"
              type="number"
              variant="bordered"
              className="w-20"
              classNames={{ input: "font-mono text-sm text-center" }}
              isDisabled={isLoading}
              aria-label="Altitude"
              endContent={<span className="text-default-400 text-xs">ft</span>}
            />
            <Input
              value={localTas}
              onValueChange={setLocalTas}
              size="sm"
              type="number"
              variant="bordered"
              className="w-20"
              classNames={{ input: "font-mono text-sm text-center" }}
              isDisabled={isLoading}
              aria-label="TAS"
              endContent={<span className="text-default-400 text-xs">kt</span>}
            />
          </div>
        </NavbarItem>
        <NavbarItem>
          <Input
            type="datetime-local"
            value={isoToLocalDatetimeValue(localDep)}
            onChange={(e) => setLocalDep(localDatetimeValueToISO(e.target.value))}
            size="sm"
            variant="bordered"
            className="w-48"
            classNames={{ input: "font-mono text-sm" }}
            isDisabled={isLoading}
            aria-label="Departure time"
          />
        </NavbarItem>
      </NavbarContent>

      {/* Desktop: Actions */}
      <NavbarContent className="hidden md:flex" justify="end">
        <NavbarItem>
          <Button
            color="primary"
            size="sm"
            onPress={handleUpdate}
            isLoading={isLoading}
          >
            Update
          </Button>
        </NavbarItem>
      </NavbarContent>

      {/* Mobile menu with all controls */}
      <NavbarMenu>
        <div className="flex flex-col gap-3 pt-4 pb-6">
          <Input
            label="Route"
            placeholder="KCDW-SBJ-KIAD"
            value={localWaypoints}
            onValueChange={setLocalWaypoints}
            size="sm"
            variant="bordered"
            classNames={{ input: "font-mono" }}
            isDisabled={isLoading}
          />
          <Select
            label="Model"
            selectedKeys={[localModel]}
            onSelectionChange={(keys) => {
              const selected = Array.from(keys)[0] as WeatherModel;
              if (selected) setLocalModel(selected);
            }}
            size="sm"
            variant="bordered"
            isDisabled={isLoading}
          >
            {MODEL_NAMES.map((m) => (
              <SelectItem key={m}>{m}</SelectItem>
            ))}
          </Select>
          <div className="flex gap-2">
            <Input
              label="Alt (ft)"
              value={localAlt}
              onValueChange={setLocalAlt}
              size="sm"
              type="number"
              variant="bordered"
              className="flex-1"
              classNames={{ input: "font-mono" }}
              isDisabled={isLoading}
            />
            <Input
              label="TAS (kts)"
              value={localTas}
              onValueChange={setLocalTas}
              size="sm"
              type="number"
              variant="bordered"
              className="flex-1"
              classNames={{ input: "font-mono" }}
              isDisabled={isLoading}
            />
            <Input
              label="Res (NM)"
              value={localRes}
              onValueChange={setLocalRes}
              size="sm"
              type="number"
              variant="bordered"
              className="flex-1"
              classNames={{ input: "font-mono" }}
              isDisabled={isLoading}
            />
          </div>
          <Input
            label="Departure (local)"
            type="datetime-local"
            value={isoToLocalDatetimeValue(localDep)}
            onChange={(e) => setLocalDep(localDatetimeValueToISO(e.target.value))}
            size="sm"
            variant="bordered"
            isDisabled={isLoading}
          />
          <Button
            color="primary"
            size="md"
            onPress={() => {
              handleUpdate();
              setIsMenuOpen(false);
            }}
            isLoading={isLoading}
            fullWidth
          >
            Update Route
          </Button>
        </div>
      </NavbarMenu>
    </Navbar>
  );
}
