import React, { Dispatch, SetStateAction } from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
} from "@nextui-org/react";
import { MODEL_NAMES } from "./meteo-vars";

export default function ModelDropdown({
  model,
  setModel,
}: {
  model: string;
  setModel: Dispatch<SetStateAction<string>>;
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
        onSelectionChange={(selectedKeys) =>
          setModel(selectedKeys.values().next().value)
        }
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
