import React, {
  Dispatch,
  SetStateAction,
  useState,
  useCallback,
  Fragment,
  useRef,
  useEffect,
} from "react";
import {
  Dropdown,
  DropdownTrigger,
  DropdownMenu,
  DropdownItem,
  Button,
  Input,
  Spinner,
  Modal,
  ModalContent,
  ModalHeader,
  ModalBody,
  ModalFooter,
  useDisclosure,
} from "@heroui/react";
import { LOCATIONS } from "../../config/weather";
import { debouncedGeocodeLocation } from "../../services/geocoding";
import { useRouter } from "next/navigation";
import { LocationsWithDescription } from "../../types/weather";

// Shared components to use in both mobile and desktop views
const LocationButton = ({
  locationName,
  description,
  onClick,
}: {
  locationName: string;
  description?: string;
  onClick: () => void;
}) => (
  <Button
    className="w-full justify-start mb-2 h-auto py-2 text-left flex flex-col items-start"
    variant="flat"
    onPress={onClick}
  >
    <code className="font-medium">{locationName}</code>
    {description && (
      <span className="text-xs text-gray-500 mt-1 line-clamp-2 overflow-hidden text-ellipsis">
        {description}
      </span>
    )}
  </Button>
);

const SearchInput = ({
  value,
  onChange,
  onSearch,
  isLoading,
  isMobile = false,
}: {
  value: string;
  onChange: (value: string) => void;
  onSearch: (value: string) => void;
  isLoading: boolean;
  isMobile?: boolean;
}) => (
  <div
    className={`relative ${isMobile ? "sticky top-0 z-10 pb-2 bg-white modal-input-container" : ""}`}
  >
    <Input
      placeholder="Search location or ICAO code..."
      value={value}
      onChange={(e) => {
        const query = e.target.value;
        onChange(query);
        onSearch(query);
      }}
      className="w-full"
      style={isMobile ? { fontSize: "16px" } : undefined}
      classNames={{
        input: isMobile ? "mobile-input" : "",
        inputWrapper: isMobile ? "no-zoom-wrapper" : "",
      }}
      startContent={<span className="text-default-400">🔍</span>}
      onKeyDown={(e) => {
        if (e.key === " ") {
          e.stopPropagation();
        }
      }}
    />
    {isLoading && (
      <Spinner
        className={
          isMobile ? "absolute right-10 top-3" : "absolute right-2 top-2"
        }
        size="sm"
      />
    )}
  </div>
);

// Helper to process descriptions for better display
const truncateDescription = (desc: string, maxLength = 100) => {
  return desc.length > maxLength ? `${desc.substring(0, maxLength)}...` : desc;
};

export default function LocationDropdown({
  location,
  setLocation,
}: {
  location: string;
  setLocation: Dispatch<SetStateAction<string>>;
}) {
  const router = useRouter();
  const [searchQuery, setSearchQuery] = useState("");
  const [customLocations, setCustomLocations] =
    useState<LocationsWithDescription>({});
  const [isLoading, setIsLoading] = useState(false);
  const lastSearchRef = useRef<string>("");
  const { isOpen, onOpen, onClose } = useDisclosure();
  const [isMobile, setIsMobile] = useState(false);

  // Check if we're on mobile
  useEffect(() => {
    const checkMobile = () => {
      setIsMobile(window.innerWidth < 768);
    };

    checkMobile();
    window.addEventListener("resize", checkMobile);

    return () => {
      window.removeEventListener("resize", checkMobile);
    };
  }, []);

  // Extract the display name from the location string (removing @coords if present)
  const displayName = location.split("@")[0];

  const handleSearch = useCallback((query: string) => {
    if (!query) return;

    lastSearchRef.current = query;
    setIsLoading(true);

    debouncedGeocodeLocation(query).then((result) => {
      // Only update if this is still the latest search
      if (lastSearchRef.current === query) {
        setCustomLocations(result);
        setIsLoading(false);
      }
    });
  }, []);

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

  // Render location sections for mobile modal
  const renderMobileLocationSections = () => (
    <>
      {Object.entries(customLocations).length > 0 && (
        <div className="mb-4">
          <p className="text-sm font-semibold mb-2">Search Results</p>
          {Object.entries(customLocations).map(([loc, data]) => (
            <LocationButton
              key={loc}
              locationName={loc}
              description={
                data.description
                  ? truncateDescription(data.description)
                  : undefined
              }
              onClick={() => {
                handleLocationSelect(loc);
                onClose();
              }}
            />
          ))}
        </div>
      )}
      <div>
        <p className="text-sm font-semibold mb-2">Saved Locations</p>
        {Object.entries(LOCATIONS).map(([loc, _]) => (
          <LocationButton
            key={loc}
            locationName={loc}
            onClick={() => {
              handleLocationSelect(loc);
              onClose();
            }}
          />
        ))}
      </div>
    </>
  );

  // Use Modal on mobile, Dropdown on desktop
  if (isMobile) {
    return (
      <>
        <Button variant="bordered" className="capitalize" onPress={onOpen}>
          <code>{displayName}</code>
        </Button>

        <Modal
          isOpen={isOpen}
          onClose={onClose}
          placement="bottom"
          classNames={{
            base: "m-0",
            wrapper: "items-end",
            body: "p-4",
          }}
          hideCloseButton
          className="modal-mobile-ready"
        >
          <ModalContent className="modal-content-mobile">
            <ModalHeader className="flex justify-between items-center border-b pb-3">
              <span>Select Location</span>
            </ModalHeader>
            <ModalBody
              className="overflow-y-auto"
              style={{ maxHeight: "30vh" }}
            >
              <SearchInput
                value={searchQuery}
                onChange={setSearchQuery}
                onSearch={handleSearch}
                isLoading={isLoading}
                isMobile={true}
              />

              <div className="mt-3 modal-scrollable-section">
                {renderMobileLocationSections()}
              </div>
            </ModalBody>
            <ModalFooter>
              <Button fullWidth color="primary" onPress={onClose}>
                Close
              </Button>
            </ModalFooter>
          </ModalContent>
        </Modal>
      </>
    );
  }

  // Build dropdown items list
  const dropdownItems = [];

  // Search input
  dropdownItems.push(
    <DropdownItem key="search" isReadOnly>
      <SearchInput
        value={searchQuery}
        onChange={setSearchQuery}
        onSearch={handleSearch}
        isLoading={isLoading}
      />
    </DropdownItem>,
  );

  // Add search results if available
  if (Object.entries(customLocations).length > 0) {
    // Add header
    dropdownItems.push(
      <DropdownItem key="search-results-header" isReadOnly>
        <p className="text-sm font-semibold my-1">Search Results</p>
      </DropdownItem>,
    );

    // Add search results
    Object.entries(customLocations).forEach(([loc, data]) => {
      dropdownItems.push(
        <DropdownItem key={loc} textValue={loc}>
          <div className="flex flex-col">
            <code className="font-medium">{loc}</code>
            {data.description && (
              <span className="text-xs text-gray-500 mt-1 line-clamp-2 overflow-hidden text-ellipsis">
                {truncateDescription(data.description)}
              </span>
            )}
          </div>
        </DropdownItem>,
      );
    });
  }

  // Add saved locations header
  if (Object.entries(LOCATIONS).length > 0) {
    dropdownItems.push(
      <DropdownItem key="saved-locations-header" isReadOnly>
        <p className="text-sm font-semibold my-1 mt-2">Saved Locations</p>
      </DropdownItem>,
    );

    // Add saved locations
    Object.entries(LOCATIONS).forEach(([loc, _]) => {
      dropdownItems.push(
        <DropdownItem key={loc}>
          <code className="font-medium">{loc}</code>
        </DropdownItem>,
      );
    });
  }

  // Desktop view with dropdown
  return (
    <Dropdown>
      <DropdownTrigger>
        <Button
          variant="bordered"
          className="capitalize"
          onKeyDown={(e) => {
            if (e.key === " ") {
              e.stopPropagation();
            }
          }}
        >
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
        className="p-2 min-w-[320px]"
      >
        {dropdownItems}
      </DropdownMenu>
    </Dropdown>
  );
}
