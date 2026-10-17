"""Read-through cache of immutable encoded Zarr chunks for volume experiments.

The caller must namespace this directory by dataset and pinned snapshot. This
keeps the original GRIB payload, so spatial batches reuse the same download.
"""

import asyncio
import hashlib
import os
import shutil
import threading
from pathlib import Path

from zarr.core.buffer import default_buffer_prototype
from zarr.storage import MemoryStore, WrapperStore


class SourceCache(WrapperStore):
    def __init__(self, store, directory, offline=False):
        super().__init__(store)
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.stats = {"hits": 0, "hit_bytes": 0, "misses": 0, "downloaded_bytes": 0}
        self.lock = threading.Lock()
        self.key_locks = {}
        self.offline = offline

    def _with_store(self, store):
        return type(self)(store, self.directory, offline=self.offline)

    @classmethod
    def from_cache(cls, directory, variables):
        """Expose cached metadata for selected fields without reopening Icechunk."""
        directory = Path(directory)
        names = [
            *variables,
            "init_time",
            "lead_time",
            "pressure_level",
            "x",
            "y",
            "latitude",
            "longitude",
            "spatial_ref",
        ]
        keys = ["zarr.json", "pressure_level/zarr.json"] + [
            f"{prefix}{name}/zarr.json"
            for prefix in ("", "pressure_level/")
            for name in names
        ]
        metadata = {}
        for key in keys:
            digest = hashlib.sha256(key.encode()).hexdigest()
            path = directory / digest[:2] / digest
            if path.exists():
                metadata[key] = default_buffer_prototype().buffer.from_bytes(
                    path.read_bytes()
                )
        if "pressure_level/zarr.json" not in metadata:
            raise FileNotFoundError("Cached group metadata is incomplete")
        return cls(MemoryStore(metadata, read_only=True), directory, offline=True)

    def snapshot(self):
        with self.lock:
            return dict(self.stats)

    async def get(self, key, prototype, byte_range=None):
        if byte_range is not None:
            # Preserve partial-read semantics; these datasets request full chunks.
            if self.offline:
                raise NotImplementedError("Offline cache expects complete chunk reads")
            return await self._store.get(key, prototype, byte_range)
        digest = hashlib.sha256(key.encode()).hexdigest()
        target = self.directory / digest[:2] / digest
        async with self.key_locks.setdefault(key, asyncio.Lock()):
            if target.exists():
                data = target.read_bytes()
                with self.lock:
                    self.stats["hits"] += 1
                    self.stats["hit_bytes"] += len(data)
                return prototype.buffer.from_bytes(data)
            if self.offline:
                if "/c/" in key or key.endswith("/c"):
                    raise FileNotFoundError(
                        f"Required source chunk is not cached: {key}"
                    )
                return None
            result = await self._store.get(key, prototype)
            if result is None:
                return None
            data = result.to_bytes()
            if shutil.disk_usage(self.directory).free < len(data) + 2 * 1024**3:
                raise OSError("Source cache would leave less than 2 GiB free")
            target.parent.mkdir(parents=True, exist_ok=True)
            temporary = target.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_bytes(data)
            temporary.replace(target)
            with self.lock:
                self.stats["misses"] += 1
                self.stats["downloaded_bytes"] += len(data)
            return result

    async def get_partial_values(self, prototype, key_ranges):
        return await asyncio.gather(
            *(self.get(key, prototype, byte_range) for key, byte_range in key_ranges)
        )
