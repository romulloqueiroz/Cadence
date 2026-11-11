#!/usr/bin/env python3
"""Compare raw and explicitly delay/gain-fitted stereo renders. No parity claim."""
import argparse
from array import array
import ctypes
import hashlib
import json
import math
from pathlib import Path
import struct
import sys


def read_wav(path):
    content = Path(path).read_bytes()
    if content[:4] != b"RIFF" or content[8:12] != b"WAVE":
        raise ValueError(f"unsupported WAV: {path}")
    position, fmt, payload = 12, None, None
    while position + 8 <= len(content):
        tag, size = struct.unpack_from("<4sI", content, position)
        data = content[position+8:position+8+size]
        if len(data) != size:
            raise ValueError("truncated WAV chunk")
        if tag == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", data)
            if fmt[0] == 0xfffe:
                fmt = (struct.unpack_from("<H", data, 24)[0],) + fmt[1:]
        elif tag == b"data":
            payload = data
        position += 8 + size + (size & 1)
    if fmt is None or payload is None or fmt[1] != 2 or fmt[5] != 32:
        raise ValueError("expected 32-bit stereo float/PCM WAV")
    samples = array("f" if fmt[0] == 3 else "i")
    samples.frombytes(payload)
    if sys.byteorder != "little":
        samples.byteswap()
    if fmt[0] == 1:
        samples = array("f", (x / 2147483648.0 for x in samples))
    elif fmt[0] != 3:
        raise ValueError("unsupported WAV encoding")
    if not all(math.isfinite(x) for x in samples):
        raise ValueError("nonfinite audio")
    return samples, fmt[2]


def dot(a, b):
    return sum(x*y for x, y in zip(a, b))


def onset(audio):
    threshold = max(1e-7, max(map(abs, audio), default=0.0) * 1e-4)
    return next((i//2 for i in range(0, len(audio), 2)
                 if max(abs(audio[i]), abs(audio[i+1])) >= threshold), None)


def fit_delay(reference, candidate, limit=2048):
    """Exact integer lag search via Apple's documented vDSP float dot product.

    Uses the first stereo channel over a fixed common ~2-second window.
    Positive shift means candidate[i+shift] aligns with reference[i].
    Delay fitting is diagnostic, not compensation applied to the raw result.
    """
    lib = ctypes.CDLL("/System/Library/Frameworks/Accelerate.framework/Accelerate")
    f = lib.vDSP_dotpr
    pointer = ctypes.POINTER(ctypes.c_float)
    f.argtypes = [pointer, ctypes.c_long, pointer, ctypes.c_long,
                  pointer, ctypes.c_ulong]
    f.restype = None
    a, b = array("f", reference[::2]), array("f", candidate[::2])
    start = limit
    count = min(96000, len(a)-2*limit, len(b)-2*limit)
    if count <= 0:
        raise ValueError("audio too short for delay search")
    def ptr(values, offset):
        return ctypes.cast(values.buffer_info()[0] + offset*4, pointer)
    def dp(x, y):
        result = ctypes.c_float()
        f(x, 1, y, 1, ctypes.byref(result), count)
        return result.value
    x = ptr(a, start)
    reference_energy = dp(x, x)
    best = (0.0, 0)
    for shift in range(-limit, limit+1):
        y = ptr(b, start+shift)
        denominator = math.sqrt(max(0.0, reference_energy*dp(y, y)))
        correlation = dp(x, y)/denominator if denominator else 0.0
        if correlation > best[0]:
            best = correlation, shift
    return {"candidate_index_minus_reference": best[1],
            "left_channel_correlation": best[0], "search_limit_frames": limit,
            "window_start_frame": start, "window_frames": count}


def metrics(reference, candidate, shift=0, gain=1.0):
    start = max(0, -shift)*2
    end = min(len(reference), len(candidate)-shift*2)
    count = end-start
    if count <= 0:
        raise ValueError("no common audio")
    energy = candidate_energy = cross = error_energy = peak_error = 0.0
    for i in range(start, end):
        x, y = reference[i], candidate[i+shift*2]*gain
        error = x-y
        energy += x*x
        candidate_energy += y*y
        cross += x*y
        error_energy += error*error
        peak_error = max(peak_error, abs(error))
    return {"common_frames": count//2, "rms_error": math.sqrt(error_energy/count),
            "peak_sample_error": peak_error,
            "normalized_rms_error": math.sqrt(error_energy/energy) if energy else None,
            "snr_db": 10*math.log10(energy/error_energy) if energy and error_energy else None,
            "correlation": cross/math.sqrt(energy*candidate_energy) if energy*candidate_energy else None}


def compare(reference, candidate):
    delay = fit_delay(reference, candidate)
    shift = delay["candidate_index_minus_reference"]
    start = max(0, -shift)*2
    end = min(len(reference), len(candidate)-shift*2)
    cross = sum(reference[i]*candidate[i+shift*2] for i in range(start, end))
    energy = sum(candidate[i+shift*2]**2 for i in range(start, end))
    gain = cross/energy if energy else 0.0
    return {"raw": metrics(reference, candidate), "delay_search": delay,
            "delay_aligned": metrics(reference, candidate, shift),
            "fitted_candidate_gain": gain,
            "fitted_candidate_gain_db": 20*math.log10(abs(gain)) if gain else None,
            "delay_and_gain_fitted": metrics(reference, candidate, shift, gain)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("cadence", type=Path)
    parser.add_argument("--native-repeat", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--note-on-frame", type=int, default=4800)
    parser.add_argument("--start-frame", type=int, default=0,
                        help="first stereo frame included in the comparison")
    parser.add_argument("--end-frame", type=int, default=None,
                        help="exclusive last stereo frame; use the end of the last "
                             "native block that received input when the host skips "
                             "insert effects on later silent blocks")
    parser.add_argument("--allow-preroll", action="store_true",
                        help="record but do not reject audio before note-on, matching "
                             "the capture host's explicit --allow-preroll exception")
    args = parser.parse_args()
    paths = {"native": args.native, "cadence": args.cadence}
    if args.native_repeat is not None:
        paths["native_repeat"] = args.native_repeat
    audio = {key: read_wav(path) for key, path in paths.items()}
    if len({(len(samples), rate) for samples, rate in audio.values()}) != 1:
        raise ValueError("capture lengths/sample rates differ")
    if any(max(map(abs, samples), default=0) < 1e-6 for samples, _ in audio.values()):
        raise ValueError("silent capture; preset loading not established")
    preroll = {key: max(map(abs, samples[:args.note_on_frame*2]),default=0.0)
               for key, (samples, _) in audio.items()}
    if not 0 < args.note_on_frame < len(audio["native"][0])//2:
        raise ValueError("invalid scheduled note-on frame")
    if any(peak > 1e-6 for peak in preroll.values()) and not args.allow_preroll:
        args.output.write_text(json.dumps({"status":"rejected_non_silent_preroll", "preroll_peaks":preroll,
                                         "note_on_frame":args.note_on_frame},indent=2)+"\n")
        raise ValueError("audio exists before the scheduled note; comparison rejected")
    total_frames = len(audio["native"][0])//2
    if not 0 <= args.start_frame < (args.end_frame or total_frames) <= total_frames:
        raise ValueError("invalid comparison frame range")
    windowed = {key: (samples[args.start_frame*2:(args.end_frame or total_frames)*2], rate)
                for key, (samples, rate) in audio.items()}
    report = {"status": "comparison_recorded", "sample_rate": audio["native"][1],
              "preroll_peaks": preroll,
              "frames": total_frames,
              "comparison_frames": [args.start_frame, args.end_frame or total_frames],
              "files": {key: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                              "peak": max(map(abs, audio[key][0])), "onset_frame": onset(audio[key][0])}
                        for key, path in paths.items()},
              "native_vs_cadence": compare(windowed["native"][0], windowed["cadence"][0])}
    if "native_repeat" in windowed:
        report["native_vs_native_repeat"] = compare(windowed["native"][0], windowed["native_repeat"][0])
    report["limitations"] = ["One factory preset/note/settings case; broader FX mode coverage not established",
                             "Raw and fitted results are separate; fitting is not a parity pass",
                             "Integer delay search bounded to +/-2048 frames; no time stretching or resampling",
                             "Native repeat mode is recorded by the capture runner; reset/reload variability and other sources are not interchangeable",
                             "When --end-frame is set, the window is the caller's claim about which input blocks the native host actually processed"]
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False)+"\n")
    print(json.dumps({"status": report["status"], "native_vs_cadence": report["native_vs_cadence"],
                      "native_repeat_raw": report.get("native_vs_native_repeat", {}).get("raw")},indent=2))


if __name__ == "__main__":
    main()
