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

  // Truncate description to a reasonable length
  const truncateDescription = (desc: string, maxLength = 60) => {
    return desc.length > maxLength
      ? `${desc.substring(0, maxLength)}...`
      : desc;
  };

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
              <div className="sticky top-0 z-10 pb-2 bg-white modal-input-container">
                <Input
                  placeholder="Search location or ICAO code..."
                  value={searchQuery}
                  onChange={(e) => {
                    const query = e.target.value;
                    setSearchQuery(query);
                    handleSearch(query);
                  }}
                  className="w-full"
                  style={{ fontSize: "16px" }}
                  classNames={{
                    input: "mobile-input",
                    inputWrapper: "no-zoom-wrapper",
                  }}
                  startContent={<span className="text-default-400">🔍</span>}
                />
                {isLoading && (
                  <Spinner className="absolute right-10 top-3" size="sm" />
                )}
              </div>

              <div className="mt-3 modal-scrollable-section">
                {Object.entries(customLocations).length > 0 && (
                  <div className="mb-4">
                    <p className="text-sm font-semibold mb-2">Search Results</p>
                    {Object.entries(customLocations).map(([loc, data]) => (
                      <Button
                        key={loc}
                        className="w-full justify-start mb-2 h-auto py-2 text-left flex flex-col items-start"
                        variant="flat"
                        onPress={() => {
                          handleLocationSelect(loc);
                          onClose();
                        }}
                      >
                        <code className="font-medium">{loc}</code>
                        {data.description && (
                          <span className="text-xs text-gray-500 mt-1 line-clamp-2 overflow-hidden text-ellipsis">
                            {truncateDescription(data.description, 100)}
                          </span>
                        )}
                      </Button>
                    ))}
                  </div>
                )}
                <div>
                  <p className="text-sm font-semibold mb-2">Saved Locations</p>
                  {Object.entries(LOCATIONS).map(([loc, _]) => (
                    <Button
                      key={loc}
                      className="w-full justify-start mb-2 h-auto py-2 text-left"
                      variant="flat"
                      onPress={() => {
                        handleLocationSelect(loc);
                        onClose();
                      }}
                    >
                      <code>{loc}</code>
                    </Button>
                  ))}
                </div>
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

  // Desktop view with dropdown
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
        <Fragment>
          {Object.entries(customLocations).map(([loc, data]) => (
            <DropdownItem key={loc} textValue={loc}>
              <div className="flex flex-col">
                <code className="font-medium">{loc}</code>
                {data.description && (
                  <span className="text-xs text-gray-500 mt-1 line-clamp-2 overflow-hidden text-ellipsis">
                    {truncateDescription(data.description, 100)}
                  </span>
                )}
              </div>
            </DropdownItem>
          ))}
          {Object.entries(LOCATIONS).map(([loc, _]) => (
            <DropdownItem key={loc}>
              <code>{loc}</code>
            </DropdownItem>
          ))}
        </Fragment>
      </DropdownMenu>
    </Dropdown>
  );
}
