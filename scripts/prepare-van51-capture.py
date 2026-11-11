#!/usr/bin/env python3
"""Build a native VAN51 comparison candidate from the preserved sampler graph.

Generation establishes the serialized input only. Check the native saved state
after restoration before drawing conclusions from a capture.
"""
import argparse
import importlib.util
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("jump_capture", Path(__file__).with_name("prepare-jump-capture.py"))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--donor", type=Path, default=ROOT / "app/build/verification/kontakt-comparison-20261108T012259Z-47329/native-runner-clean/component-state.bin")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--values", default=",".join(["0.5"] * 8))
    parser.add_argument("--flags", default="0,0,0,0,0")
    args = parser.parse_args()
    values = [float(value) for value in args.values.split(",")]
    flags = [int(value) for value in args.flags.split(",")]
    if len(values) != 8 or not all(0 <= value <= 1 for value in values):
        parser.error("--values requires eight normalized numbers")
    if len(flags) != 5 or not all(value in (0, 1) for value in flags):
        parser.error("--flags requires five 0/1 switches")

    def fx_chain(wet):
        if not wet:
            return capture.empty_array(8)
        # fixtypeToType's immutable table at 0x104cc6cb8 maps 32 -> 0x13.
        child = capture.b.Ser(82, b"\0" + struct.pack("<H8f5B", 0x10, *values, *flags))
        slot = capture.structured(0x25, 0x50, struct.pack("<iBiBffi", 32, 0, 0, 0, 1.0, 0.0, -1), children=child.encode())
        return b"\0" + struct.pack("<HI", 0x13, 8) + b"\1" + slot.encode() + b"\0" * 7

    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    source = args.donor.read_bytes()
    files = {}
    for name, wet in [("dry", False), ("van51", True)]:
        candidate = capture.build_preserved(source, wet, chain_builder=fx_chain)
        path = out / f"unity-{name}-candidate.nkm"
        path.write_bytes(candidate)
        files[name] = {"path": str(path), "sha256": capture.b.sha(candidate)}
    manifest = {
        "source_state": str(args.donor.resolve()),
        "source_sha256": capture.b.sha(source),
        "candidates": files,
        "van51": {"fix_type": 32, "serialized_child": 82, "version": 16, "values": values, "flags": flags},
        "program_processing": "unity/center, empty scripts, VAN51 only in wet insert 0",
        "group_processing": "retained unchanged upstream of the insert",
        "status": "candidate_generated_native_acceptance_unverified",
    }
    (out / "candidate-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
