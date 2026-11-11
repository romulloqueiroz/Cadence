#!/usr/bin/env python3
"""Install the pinned JUCE revision and Cadence patch without changing other projects."""
import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
REVISION = "29396c22c93392d6738e021b83196283d6e4d850"
PATCH = ROOT / "patches/juce-cadence.patch"

def git(*args, cwd=None):
    return subprocess.run(["git", *args], cwd=cwd, check=True, stdout=subprocess.PIPE).stdout

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="https://github.com/juce-framework/JUCE.git",
                        help="JUCE upstream or a local mirror containing the pinned revision")
    args = parser.parse_args()
    destination = ROOT / "vendor/JUCE"
    destination.parent.mkdir(exist_ok=True)
    if destination.exists() and not destination.is_symlink():
        if git("rev-parse", "HEAD", cwd=destination).decode().strip() != REVISION:
            raise SystemExit("Existing JUCE has a different revision; move it aside first.")
        git("apply", "--reverse", "--check", str(PATCH), cwd=destination)
        print("Pinned JUCE patch already installed.")
        return
    # Stage completely before replacing the old cross-project symlink.
    with tempfile.TemporaryDirectory(prefix=".juce-", dir=destination.parent) as tmp:
        checkout = Path(tmp) / "JUCE"
        git("clone", "--no-checkout", args.repository, str(checkout))
        git("checkout", "--detach", REVISION, cwd=checkout)
        git("apply", "--check", str(PATCH), cwd=checkout)
        git("apply", str(PATCH), cwd=checkout)
        if destination.is_symlink():
            destination.unlink()
        shutil.move(str(checkout), destination)
    print("Installed pinned JUCE with Cadence patches.")

if __name__ == "__main__":
    main()
