#!/usr/bin/env python3
"""Build and run packaged integration checks; retain failures and permission blockers.

Run from a normal macOS Terminal when the agent environment denies POSIX IPC.
No elevated privileges, installed-plugin changes, test skips or IPC substitutions.
"""
import argparse
import ctypes
import datetime
import errno
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", type=Path, default=root / "app/build",
                        help="Configured CMake build directory (for example app/build-release)")
    args = parser.parse_args()
    build = args.build_dir.resolve()
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = build / "verification" / f"packaged-{stamp}-{os.getpid()}"
    output.mkdir(parents=True)
    report = {"status": "running", "output": str(output), "checks": {}}

    def finish(status, message, code):
        report.update(status=status, message=message)
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"{status.upper()}: {message}\nReport: {output / 'report.json'}", flush=True)
        return code

    if sys.platform != "darwin":
        return finish("blocked", "This runner targets the configured macOS AU/VST3 build.", 2)
    if not (build / "CMakeCache.txt").is_file():
        return finish("blocked", f"Configure {build} with CMake before running verification.", 2)

    # Diagnose the actual capability before attributing a launch failure to DSP.
    # The same POSIX IPC path remains in use by the real host/worker tests.
    libc = ctypes.CDLL(None, use_errno=True)
    libc.shm_open.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_uint]
    libc.shm_open.restype = ctypes.c_int
    libc.shm_unlink.argtypes = [ctypes.c_char_p]
    libc.shm_unlink.restype = ctypes.c_int
    name = f"/cadence-check-{os.getpid()}".encode()
    fd = libc.shm_open(name, os.O_CREAT | os.O_EXCL | os.O_RDWR, 0o600)
    failure = ctypes.get_errno() if fd < 0 else 0
    report["checks"]["shm_open"] = {"errno": failure, "passed": fd >= 0}
    if fd < 0:
        status = "blocked" if failure in (errno.EPERM, errno.EACCES) else "failed"
        return finish(status, f"shm_open: {os.strerror(failure)}. Run this command in a normal Terminal with POSIX IPC access.", 2 if status == "blocked" else 1)
    os.close(fd)
    libc.shm_unlink(name)

    def run(label, command, environment=None):
        print(f"Running {label}...", flush=True)
        log = output / f"{label}.log"
        try:
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=root, env=environment,
                                        stdout=stream, stderr=subprocess.STDOUT)
            code = result.returncode
        except OSError as error:
            log.write_text(str(error) + "\n")
            code = 1
        report["checks"][label] = {"exit_code": code, "log": str(log)}
        if code:
            print("\n".join(log.read_text(errors="replace").splitlines()[-25:]), flush=True)
        return code == 0

    if not run("build", ["cmake", "--build", str(build), "--target",
                         "Cadence_Standalone", "Cadence_AU", "Cadence_VST3",
                         "CadenceScanTests", "-j", "4"]):
        return finish("failed", "Packaged build failed; see build.log.", 1)
    worker = build / "rust/aarch64-apple-darwin/release/plugin-worker"
    if not worker.exists():
        worker = build / "rust/x86_64-apple-darwin/release/plugin-worker"
    report["worker_sha256"] = hashlib.sha256(worker.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="cadence-packaged-", dir="/private/tmp") as temporary:
        environment = dict(os.environ, CADENCE_TEST_TMPDIR=temporary,
                           CADENCE_WORKER_LOG_DIR=str(output / "worker-logs"))
        if not run("ctest", ["ctest", "--test-dir", str(build), "--output-on-failure",
                             "--output-junit", str(output / "ctest.xml")], environment):
            return finish("failed", "Packaged integration checks failed; see ctest.log and ctest.xml.", 1)
    return finish("passed", "Packaged build and integration checks passed. Native Kontakt audio comparison remains a separate check.", 0)


if __name__ == "__main__":
    sys.exit(main())
