#!/usr/bin/env python3
"""Check that macOS registered the version of the installed AU bundle."""
import argparse
import ctypes
import json
from pathlib import Path
import plistlib
import sys


class Description(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in
                ("type", "subtype", "manufacturer", "flags", "flags_mask")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path, nargs="?", default=
                        Path.home() / "Library/Audio/Plug-Ins/Components/Cadence.component")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("Audio Unit registration requires macOS")
    with (args.bundle / "Contents/Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    component = info["AudioComponents"][0]
    description = Description(*(int.from_bytes(component[key].encode("ascii"), "big")
                                for key in ("type", "subtype", "manufacturer")), 0, 0)
    library = ctypes.CDLL("/System/Library/Frameworks/AudioToolbox.framework/AudioToolbox")
    library.AudioComponentFindNext.argtypes = [ctypes.c_void_p, ctypes.POINTER(Description)]
    library.AudioComponentFindNext.restype = ctypes.c_void_p
    library.AudioComponentGetVersion.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
    library.AudioComponentGetVersion.restype = ctypes.c_int32
    handle = library.AudioComponentFindNext(None, ctypes.byref(description))
    version = ctypes.c_uint32()
    status = library.AudioComponentGetVersion(handle, ctypes.byref(version)) if handle else -1
    passed = status == 0 and version.value == component["version"]
    print(json.dumps({"status": "passed" if passed else "registration-mismatch",
                      "bundle": str(args.bundle.resolve()),
                      "bundle_version": info["CFBundleShortVersionString"],
                      "expected_component_version": component["version"],
                      "registered_component_version": version.value if status == 0 else None,
                      "os_status": status}, indent=2))
    if not passed:
        print("macOS has not registered this build. Close the DAW and rerun the installer; "
              "then reset and rescan Cadence in the DAW's plug-in manager if needed.", file=sys.stderr)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
