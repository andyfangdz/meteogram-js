"use client";

import { useState } from "react";
import { Input, Button, Select, SelectItem } from "@heroui/react";
import { WeatherModel } from "../../types/weather";
import { MODEL_NAMES } from "../../config/weather";

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
    <div className="flex flex-wrap gap-2 p-3 bg-background border-b border-divider items-end">
      <Input
        label="Waypoints"
        placeholder="KCDW-KFRG"
        value={localWaypoints}
        onValueChange={setLocalWaypoints}
        size="sm"
        className="min-w-[200px] flex-1"
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
        className="min-w-[160px]"
        isDisabled={isLoading}
      >
        {MODEL_NAMES.map((m) => (
          <SelectItem key={m}>{m}</SelectItem>
        ))}
      </Select>

      <Input
        label="Alt (ft)"
        type="number"
        value={localAlt}
        onValueChange={setLocalAlt}
        size="sm"
        className="w-24"
        isDisabled={isLoading}
      />

      <Input
        label="TAS (kts)"
        type="number"
        value={localTas}
        onValueChange={setLocalTas}
        size="sm"
        className="w-24"
        isDisabled={isLoading}
      />

      <Input
        label="Departure"
        type="datetime-local"
        value={localDep.replace("Z", "").substring(0, 16)}
        onChange={(e) =>
          setLocalDep(new Date(e.target.value).toISOString())
        }
        size="sm"
        className="min-w-[180px]"
        isDisabled={isLoading}
      />

      <Input
        label="Res (NM)"
        type="number"
        value={localRes}
        onValueChange={setLocalRes}
        size="sm"
        className="w-24"
        isDisabled={isLoading}
      />

      <Button
        color="primary"
        size="sm"
        onPress={handleUpdate}
        isLoading={isLoading}
        className="self-end mb-0.5"
      >
        Update
      </Button>
    </div>
  );
}
