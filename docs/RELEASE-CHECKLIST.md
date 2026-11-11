# Cadence v1 closure checklist

Target: Apple Silicon macOS standalone/AU/VST3 with the documented
[supported content subset](SUPPORTED-CONTENT.md). Complete Kontakt emulation is
outside this release's acceptance criteria. Evidence must name the source/build;
old tests and a green parser sweep do not qualify a new instrument or host.

## 1. Content scope and diagnostics

- [x] Publish the supported subset and concrete exclusions.
- [x] Surface Kontakt subset/script notices in the shipping Inspector and CLI.
- [x] Reject unsupported active binary effects and multi-program playback.
- [x] Record the exact real-library load/render smoke matrix for this build:
  all six installed Piano Uno presets produce finite audible output. This does
  not certify scripted behavior or full musical compatibility.

## 2. Content recovery

- [x] Choose sample folders explicitly, reject ambiguous matches, bound search.
- [x] Locate moved instruments without rewriting original content.
- [x] Preserve slot identity, bypass and saved parameter state through retry.
- [x] Persist recovery locations through rack/project recall; allow clearing them.
- [x] Exercise failures, successful audible recovery and moved-instrument recall
  through the actual packaged worker/bridge.

## 3. Qualification

- [x] Current workspace tests (414 passed, 5 explicit ignores), all-target
  Clippy and formatting pass.
- [x] Release standalone/AU/VST3 build, CTest and AU validation pass.
- [x] Extended paced realtime benchmark: 6000 callbacks at each of 64/128/256
  frames, zero underruns and healthy realtime workers.
- [ ] Current Logic build: playback, bounce, automation, save/close/reopen and
  recovered-library paths, followed by a session with several instances.
- [ ] Long-session/storage/polyphony stress with representative supported content.
- [x] AU/VST3 installation and matching bundled helper/signature checks pass.

Basic Logic timing/recall was verified in earlier increments. The entries above
require the current release candidate. Extend this matrix only for hosts/content
that are actually tested. Unsupported large libraries need not be qualified for
this subset; do not claim they are supported.

Current evidence: `app/build-release/verification/v1-closure-2026-11-11/`.
The disposable `Cadence-v1-Logic.logicx` copy loads the installed AU and its two
Sine/Gain workers; transport runs. Its 16-second, eight-note offline bounce fails
audio validation: peak `8.61e-10`, with no audible note segments. This remains an
unresolved qualification failure, not a passing DAW check. Both the direct rack
and the native AU wrapper render the project's legacy rack state successfully
in isolated regression tests. Further Logic UI diagnosis is currently limited by
window-focus failures with Logic's separate AU window. MIDI event-list delivery
through the native AU wrapper also passes for MIDI 1 and MIDI 2. Logic's Event
List confirms the expected eight channel-1 notes at velocity 100; adding the same
region to Studio Grand produces an audible comparison bounce (peak `0.205`).
This narrows the failure to the Cadence path in Logic; its cause is not yet
established. Saving the disposable project in the current state format, closing
and reopening it starts fresh workers, but its new bounce has the same near-zero
peak. Project save/reopen succeeds structurally; audible recall is not qualified.
The original projects were not modified.

## 4. Distribution

- [x] Reproducible patched JUCE setup, CI and portable archive packaging exist.
- [x] Dependency/material inventory and explicit public-release audit exist.
- [x] Developer ID staging/notarization workflow is prepared; original build
  bundles are preserved. Only its credential-free preflight is currently tested.
- [ ] Resolve the existing redistribution review in REDISTRIBUTION-REVIEW.json.
- [ ] Supply the intended license texts with a reviewed scope/ownership inventory.
- [ ] Install the publisher's Developer ID identity and configure a notarytool
  Keychain profile; sign, receive Apple's acceptance, and validate stapling.
- [ ] Test downloaded/quarantined installation on a second Mac, then publish.

Current archives are development builds with ad-hoc signatures. Automated
preflight does not supply credentials, establish redistribution rights or mark
these publisher/release checks complete.

Commands:

```sh
CADENCE_BENCH_CALLBACKS=6000 python3 scripts/verify-packaged.py --build-dir app/build-release
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
python3 scripts/release-audit.py
python3 scripts/release-audit.py --public   # intentionally fails on open prerequisites
python3 scripts/sign-macos.py --check
```

`release-audit.py --public` also requires `--qualification-report` and
`--distribution-report` pointing to the actual current-source qualification and
accepted/stapled distribution records. It checks the source fingerprint and
distribution checksum; a dependency inventory alone cannot pass this gate.

Once publisher prerequisites are available, use an installed certificate identity
and existing Keychain profile (never pass a password/private key to this script):

```sh
python3 scripts/sign-macos.py --identity 'Developer ID Application: ...' \
  --notary-profile CADENCE_NOTARY --output dist/Cadence-v1-macOS-arm64.dmg
```

That command signs staged copies, creates a DMG, submits it to Apple, requires
an Accepted result and staples/validates the image. Its adjacent JSON report and
SHA-256 checksum record the actual result. Review any failed Apple submission
with `notarytool log` using the returned submission ID.
