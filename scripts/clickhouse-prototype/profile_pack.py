"""Bit-exact packing with a fused Rust ARM NEON kernel and portable NumPy fallback."""

import contextlib
import ctypes
import hashlib
import os
import platform
import shutil
import subprocess
import tempfile
from functools import lru_cache
from pathlib import Path

import numpy as np


@lru_cache(maxsize=1)
def library():
    source = Path(__file__).with_suffix(".rs")
    compiler = os.environ.get("METEOGRAM_RUSTC") or shutil.which("rustc")
    if not compiler:
        local = (
            Path.home()
            / ".local/share/meteogram-rust/rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc"
        )
        compiler = str(local) if local.exists() else None
    if not compiler:
        return None
    version = subprocess.check_output([compiler, "--version"]).splitlines()[0]
    key = hashlib.sha256(
        source.read_bytes() + version + platform.machine().encode()
    ).hexdigest()[:20]
    cache = (
        Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
        / "meteogram-native"
        / key
    )
    cache.mkdir(parents=True, exist_ok=True)
    target = cache / "profile_pack.so"
    if not target.exists():
        with tempfile.TemporaryDirectory(dir=cache) as temporary:
            output = Path(temporary) / "profile_pack.so"
            subprocess.run(
                [
                    compiler,
                    "--edition=2021",
                    "--crate-type=cdylib",
                    "-C",
                    "opt-level=3",
                    "-C",
                    "panic=abort",
                    "-C",
                    "codegen-units=1",
                    str(source),
                    "-o",
                    str(output),
                ],
                check=True,
            )
            output.replace(target)
    lib = ctypes.CDLL(str(target))
    signature = [
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.c_void_p,
        *([ctypes.c_size_t] * 4),
    ]
    for name in ("pack_scalar", "pack_neon"):
        function = getattr(lib, name)
        function.argtypes = signature
        function.restype = ctypes.c_int
    lib.has_neon.restype = ctypes.c_int
    lib.encode_native.argtypes = [
        ctypes.POINTER(ctypes.c_char_p),
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.POINTER(ctypes.c_size_t),
        ctypes.c_size_t,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.c_int,
        ctypes.POINTER(ctypes.c_size_t),
    ]
    lib.encode_native.restype = ctypes.c_void_p
    lib.free_native.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    lib.free_native.restype = None
    return lib


def backend():
    lib = library()
    return "neon" if lib is not None and lib.has_neon() else "numpy"


def pack(sources, mode="auto"):
    """Return [field,cell,level,lead]; finite checking is fused into native copy."""
    if not sources or mode not in ("auto", "neon", "scalar", "numpy"):
        raise ValueError("Invalid pack request")
    shape = sources[0].shape
    if (
        len(shape) != 3
        or min(shape) <= 0
        or any(
            a.dtype != np.dtype("float32")
            or a.shape != shape
            or not a.flags.c_contiguous
            for a in sources
        )
    ):
        raise ValueError(
            "Packing requires matching contiguous Float32 cell/level/field arrays"
        )
    cells, levels, fields = shape
    count = len(sources)
    if mode == "auto":
        mode = backend()
    if mode == "numpy":
        result = np.ascontiguousarray(np.stack(sources, axis=1).transpose(3, 0, 2, 1))
        if not np.isfinite(result).all():
            raise ValueError("Non-finite staged values")
        return result
    lib = library()
    if lib is None or (mode == "neon" and not lib.has_neon()):
        raise RuntimeError("Requested native kernel unavailable")
    result = np.empty((fields, cells, levels, count), dtype=np.float32)
    pointers = (ctypes.c_void_p * count)(*[a.ctypes.data for a in sources])
    if getattr(lib, "pack_" + mode)(
        pointers, result.ctypes.data, cells, levels, fields, count
    ):
        raise ValueError("Non-finite staged values")
    return result


@contextlib.contextmanager
def native_block(ids, names, arrays, encoded):
    """Rust owns the encoded buffer until synchronous raw_insert completes."""
    if (
        ids.dtype != np.dtype("uint32")
        or not ids.flags.c_contiguous
        or len(names) != len(arrays)
        or not len(ids)
    ):
        raise ValueError("Invalid Native block")
    if any(
        a.dtype != np.dtype("float32") or not a.flags.c_contiguous or a.size % len(ids)
        for a in arrays
    ):
        raise ValueError("Invalid profile column buffer")
    lib = library()
    if lib is None:
        raise RuntimeError("Rust Native encoder unavailable")
    pointers = (ctypes.c_void_p * len(arrays))(*[a.ctypes.data for a in arrays])
    strings = (ctypes.c_char_p * len(names))(*[n.encode("ascii") for n in names])
    widths = (ctypes.c_size_t * len(arrays))(*[a.size // len(ids) for a in arrays])
    length = ctypes.c_size_t()
    pointer = lib.encode_native(
        strings,
        pointers,
        widths,
        len(arrays),
        ids.ctypes.data,
        len(ids),
        int(encoded),
        ctypes.byref(length),
    )
    if not pointer:
        raise RuntimeError("Rust Native encoder failed")
    try:
        view = memoryview((ctypes.c_ubyte * length.value).from_address(pointer)).cast(
            "B"
        )
        try:
            yield view
        finally:
            view.release()
    finally:
        lib.free_native(pointer, length.value)
