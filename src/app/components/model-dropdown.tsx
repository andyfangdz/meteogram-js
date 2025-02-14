import React, { Dispatch, SetStateAction } from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
} from "@heroui/react";
import { MODEL_NAMES } from "../../config/weather";
import { WeatherModel } from "../../types/weather";

export default function ModelDropdown({
  model,
  setModel,
}: {
  model: WeatherModel;
  setModel: Dispatch<SetStateAction<WeatherModel>>;
}) {
  return (
    <Dropdown>
      <DropdownTrigger>
        <Button variant="bordered" className="capitalize">
          Model: <code>{model}</code>
        </Button>
      </DropdownTrigger>
      <DropdownMenu
        aria-label="Single selection example"
        variant="flat"
        disallowEmptySelection
        selectionMode="single"
        selectedKeys={[model]}
        onAction={(model) => setModel(model as WeatherModel)}
      >
        {MODEL_NAMES.map((model) => (
          <DropdownItem key={model}>
            <code>{model}</code>
          </DropdownItem>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}
