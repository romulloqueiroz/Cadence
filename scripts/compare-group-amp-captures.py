#!/usr/bin/env python3
"""Compare controlled group renders without fitting delay or gain."""
import argparse
from array import array
import hashlib
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("compare_audio", Path(__file__).with_name("compare-kontakt-audio.py"))
audio = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audio)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=ROOT / "app/build/verification/group-amp-native-2026-11-10/piano-controlled/relative")
    args = parser.parse_args()
    directory = args.directory
    report = {"method": __doc__, "cases": {}, "polyphony": {}, "limits": [
        "Controlled JUMP slot3, split0/8, group gain0.25/pan0.35, fixed48kHz note60/velocity100.",
        "Two voices duplicate one factory zone at the same note; distinct-pitch polyphony is outside this capture.",
        "Whole-instrument waveform differences remain; numerical circuit fixtures and routing properties do not establish full host parity."]}
    renders = {}
    for mode in ["dry", "pre", "post"]:
        for voices in [1, 2]:
            name = f"{mode}-{voices}voice"
            case = directory / name
            assert json.loads((case / "identity.json").read_text())["status"] == "confirmed"
            original, rate = audio.read_wav(case / "native.wav")
            cadence, cadence_rate = audio.read_wav(case / "cadence.wav")
            assert rate == cadence_rate == 48000 and len(original) == len(cadence) == 960000
            renders[(mode, voices, "native")] = original
            renders[(mode, voices, "cadence")] = cadence
            report["cases"][name] = {"rate": rate, "full": audio.metrics(original, cadence),
                "held_note": audio.metrics(original[9600:585600], cadence[9600:585600]),
                "held_note_frame_range": [4800, 292800],
                "native_peak": max(map(abs, original)), "cadence_peak": max(map(abs, cadence)),
                "sha256": {file: hashlib.sha256((case / file).read_bytes()).hexdigest() for file in ["native.wav", "cadence.wav"]}}
        for engine in ["native", "cadence"]:
            one, two = renders[(mode, 1, engine)], renders[(mode, 2, engine)]
            report["polyphony"][f"{engine}-{mode}"] = audio.metrics(array("f", (value * 2 for value in one)), two)
    for engine in ["native", "cadence"]:
        report[f"{engine}_pre_vs_post"] = audio.metrics(renders[("pre", 1, engine)], renders[("post", 1, engine)])
    native_delta = array("f", (b - a for a, b in zip(renders[("pre", 1, "native")], renders[("post", 1, "native")])))
    cadence_delta = array("f", (b - a for a, b in zip(renders[("pre", 1, "cadence")], renders[("post", 1, "cadence")])))
    report["native_vs_cadence_phase_difference"] = audio.metrics(native_delta, cadence_delta)
    path = directory / "cadence-comparison.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(path), "full_snr_db": {name: value["full"]["snr_db"] for name, value in report["cases"].items()},
        "polyphony_peak_errors": {name: value["peak_sample_error"] for name, value in report["polyphony"].items()}}, indent=2))


if __name__ == "__main__":
    main()
