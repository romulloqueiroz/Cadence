#!/usr/bin/env python3
"""Capture and compare Kontakt/Cadence without asserting that they match.

Run in a normal macOS Terminal. Automatic native state loading is validated;
if unsupported, the native editor permits explicit manual NKI loading.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parents[1]
    build = root / "app/build"
    evidence = build / "verification/native-kontakt-parity-2026-11-08"
    source = Path("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki")
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = build / "verification" / f"kontakt-comparison-{stamp}-{os.getpid()}"
    output.mkdir(parents=True)
    report = {"status": "running", "source_nki": str(source), "output": str(output), "checks": {}}

    def finish(status, message, code):
        report.update(status=status, message=message)
        (output / "report.json").write_text(json.dumps(report, indent=2)+"\n")
        print(f"{status.upper()}: {message}\nReport: {output / 'report.json'}", flush=True)
        return code

    def run(label, command, timeout=300):
        print(f"Running {label}...", flush=True)
        log = output / f"{label}.log"
        try:
            with log.open("w") as stream:
                result = subprocess.run([str(x) for x in command], cwd=root,
                                        stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
            code = result.returncode
        except subprocess.TimeoutExpired:
            with log.open("a") as stream:
                stream.write("\nOwned capture/build process timed out and was killed/waited.\n")
            code = 124
        except OSError as error:
            log.write_text(str(error)+"\n")
            code = 1
        report["checks"][label] = {"exit_code": code, "log": str(log)}
        if code:
            print("\n".join(log.read_text(errors="replace").splitlines()[-12:]), flush=True)
        return code == 0

    if sys.platform != "darwin" or not source.is_file():
        return finish("blocked", "macOS with installed Kontakt 8 and Piano Uno - Concert is required.", 2)
    report["source_nki_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
    if not run("configure", ["cmake", "-S", root/"app", "-B", build,
                             "-DCADENCE_BUILD_KONTAKT_CAPTURE=ON"]):
        return finish("failed", "Capture host configuration failed.", 1)
    if not run("build-capture", ["cmake", "--build", build, "--target", "CadenceKontaktCapture", "-j", "4"]):
        return finish("failed", "Capture host build failed.", 1)
    triple = "aarch64-apple-darwin" if platform.machine() == "arm64" else "x86_64-apple-darwin"
    if not run("cadence-render", ["cargo", "run", "--release", "--offline", "--target", triple,
                                  "--target-dir", build/"rust", "-p", "sample-adapter", "--bin", "sample-host", "--",
                                  source, output/"cadence.wav", "--seconds", "10", "--rate", "48000",
                                  "--block", "256", "--note", "60", "--velocity", "100", "--float-wav"]):
        return finish("failed", "Cadence reference render failed.", 1)
    if not run("bank-fixture", [sys.executable, evidence/"build_native_bank.py", "--output", output/"concert-bank.nkm"]):
        return finish("failed", "Native state fixture construction failed.", 1)
    capture = build/"CadenceKontaktCapture_artefacts/Debug/Cadence Kontakt Capture.app/Contents/MacOS/Cadence Kontakt Capture"
    scan = output/"native-scan"
    if not run("native-scan", [capture, "--scan", "--output", scan], 120):
        return finish("blocked", "Native plugin discovery failed.", 2)
    description = scan/"plugin-description.xml"
    native = output/"native-automatic"
    captured = run("native-automatic", [capture, "--description", description, "--component", output/"concert-bank.nkm", "--output", native], 120)
    confirmed = captured and run("native-identity", [sys.executable, evidence/"validate_native_state.py",
                                                     native/"component-state.bin", "--output", native/"identity.json"])
    if not confirmed:
        print("Automatic preset capture/identity was not established.\n"
              "In the native Kontakt editor that opens, load exactly:\n"
              f"{source}\n"
              "Then click the Capture button above Kontakt. Closing the window cancels.\n"
              "This is the native plugin; do not substitute a Cadence render.", flush=True)
        native = output/"native-interactive"
        if not run("native-interactive", [capture, "--description", description, "--interactive", "--output", native], 900):
            return finish("blocked", "Native capture could not complete; inspect native logs. No parity result exists.", 2)
        if not (native/"operator-confirmation.txt").is_file():
            return finish("blocked", "Native preset identity was not confirmed by the operator.", 2)
        confirmed = run("interactive-identity", [sys.executable, evidence/"validate_native_state.py",
                                                 native/"component-state.bin", "--output", native/"identity.json"])
        identity = json.loads((native/"identity.json").read_text())
        if identity["status"] == "rejected":
            return finish("failed", "Native state contradicts the requested instrument. Comparison rejected.", 1)
        report["native_identity"] = "payload verified" if confirmed else "operator confirmed; state decoder could not verify"
    else:
        report["native_identity"] = "saved native payload verified"
        report["native_bank_global_settings"] = "Experimental donor bank/container retained; gain/delay fitting is diagnostic only. See concert-bank.manifest.json."
    repeat_audio = native/"native-repeat.wav"
    if repeat_audio.is_file():
        report["native_repeat_mode"] = "same loaded native instance after reset; no saved-state reload"
    else:
        repeat = output/"native-repeat"
        if not run("native-repeat", [capture, "--description", description, "--state", native/"native-state.bin", "--output", repeat], 120):
            return finish("blocked", "Native saved-state repeat failed. Comparison is incomplete.", 2)
        repeat_audio = repeat/"native.wav"
        report["native_repeat_mode"] = "new native instance from captured state"
    if not run("compare", [sys.executable, root/"scripts/compare-kontakt-audio.py", native/"native.wav", output/"cadence.wav",
                           "--native-repeat", repeat_audio, "--output", output/"comparison.json"]):
        return finish("failed", "Audio comparison failed validation.", 1)
    report["comparison"] = str(output/"comparison.json")
    report["native_state_sha256"] = hashlib.sha256((native/"native-state.bin").read_bytes()).hexdigest()
    report["capture_host_sha256"] = hashlib.sha256(capture.read_bytes()).hexdigest()
    report["native_plugin_sha256"] = hashlib.sha256(Path("/Library/Audio/Plug-Ins/VST3/Kontakt 8.vst3/Contents/MacOS/Kontakt 8").read_bytes()).hexdigest()
    return finish("comparison_recorded", "Native/Cadence audio and native repeat were captured and compared. This is not a parity pass; review comparison.json.", 0)


if __name__ == "__main__":
    sys.exit(main())
