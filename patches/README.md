# JUCE dependency

Base: JUCE commit `29396c22c93392d6738e021b83196283d6e4d850`.
Run `python3 scripts/setup-juce.py` from a clean checkout.

`juce-cadence.patch` preserves all seven modified files from the development
checkout, including negative-preroll rounding, host automation, workgroup,
standalone and graph/hosting changes. Some original files have line-ending
changes; the patch intentionally preserves the tested bytes. Do not replace it
with an unpatched upstream checkout. Updating JUCE requires porting the patch
and rerunning Rust and packaged integration checks.

JUCE's license is in `vendor/JUCE/LICENSE.md` after setup; the patch does not
change its licensing. The dependency checkout is ignored, the patch is tracked.
