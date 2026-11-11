# Cadence

macOS standalone, AU and VST3 host/player. For a fresh checkout, run
`python3 scripts/setup-juce.py` first. See [building and sharing](docs/SHARING.md)
and [installation](docs/INSTALL-macOS.md).

Working name for the open instrument platform: a host player, an open plugin
contract, and a federated library ecosystem.

This workspace starts with the **content engine**, because Kontakt-library
compatibility is the wedge that gives the platform users and content on day one,
and because the recovered format knowledge is perishable.

## Layout

```
platform/
  crates/
    nk-format/        # Kontakt container + program model reader
    plugin-api/       # universal protocol + conformance (started)
    host/             # reference host: block scheduling + offline render (started)
    sample-adapter/   # Kontakt -> protocol adapter (started)
    plugin-package/   # manifest + content-addressed assets (started)
    plugin-sdk/       # native Rust plugin SDK + C ABI export macro (started)
    plugin-loader/    # native plugin loader (in-process, dev) (started)
    cadence-bridge/   # sandbox-backed C ABI for the JUCE app
  app/                # Cadence standalone, AU, VST3, scanner and integration checks
  plugins/
    gain/             # example native Rust effect (cdylib)
```

Planned crates:

| Crate | Purpose |
|-------|---------|
| `plugin-api` | execution contract: buffers, events, state, UI schema (started) |
| `host` | block scheduling / offline render (started); sandbox runtime next |
| `sample-adapter` | sample-library provider (Kontakt first) (started) |
| `plugin-package` | manifest, SHA-256 assets, Ed25519 signatures (started) |
| `plugin-store` | install store, publisher keys, provider, CLI (started) |
| `plugin-discovery` | signed feeds, key pinning, local index, HTTPS (started) |
| `plugin-sdk` / `plugin-loader` | native Rust plugin ABI, SDK macro, loader (started) |
| `plugin-sandbox` | worker process, shared-memory audio, isolation (started) |
| `adapter-clap` | CLAP hosting in the sandbox (started) |
| `adapter-au` | macOS Audio Unit hosting in the sandbox (started) |
| `adapter-vst3` | VST3 hosting (next; SDK/license decision) |
| `publish` | signatures, publisher feeds, discovery |

## nk-format

Reads all the container variants that matter so far:

- **Plain `.nki`** (`0x7FA89012`): zlib-compressed `K2_Container` XML — programs,
  groups, zones, modulators. The supported single-program subset now loads into the playable model and streaming sampler; see [XML playback](docs/plain-xml-playback.md).
- **NI file containers** (`hsin` item tree): the format used by Kontakt 5+
  factory content and by `.nkm`/`.nkc`. Parses the chunk tree, decodes
  **FastLZ-compressed subtrees**, reads the **preset chunk tree**
  (programs/groups/zones), decodes **program/group/zone parameters**, **AHDSR
  envelopes**, **loops**, and **`.ncw`/`.wav`** samples.
- **NI file containers** (monoliths `.nkx`/`.nki`): reads the table of contents
  and extracts embedded instruments and samples.
- `.nkm` multis are parsed **including their embedded slot programs** (so they
  inspect and render); `.nkr` resource directories and their samples are
  extracted (`k2_monolith`); `.nkc` is detected and reported.
- Multi-layer playback: main groups are summed, release groups trigger on
  note-off; pedal groups are skipped.
- **Authorization check**: reports when a chunk carries serial PIDs, which is
  the case where subtrees are genuinely encrypted.

Verified end-to-end on the factory `Piano Uno - Concert.nki`: 1 program,
6 groups, 123 zones, 126 sample references, no encryption — and rendered to
audio (note 60, default "Grand Piano" group → 3 s at 48 kHz, with gain, pan,
tune and the group's AHDSR envelope). See
[`crates/nk-format/SPEC.md`](crates/nk-format/SPEC.md) for the wire format,
provenance, and open questions.

```sh
cargo test

# Plain XML instrument
cargo run -p nk-format --bin nk-inspect -- <file.nki> --xml

# NI container: header + program/group/zone summary
cargo run -p nk-format --bin nk-inspect -- <file.nki>

# Structural dumps
cargo run -p nk-format --bin nk-inspect -- <file.nki> --tree
cargo run -p nk-format --bin nk-inspect -- <file.nki> --preset-tree

# Zone table with resolved sample paths
cargo run -p nk-format --bin nk-inspect -- <file.nki> --zones

# Machine-readable
cargo run -p nk-format --bin nk-inspect -- <file.nki> --json

# Decode a single NCW sample to WAV
cargo run -p nk-format --bin ncw-decode -- <file.ncw> out.wav

# Play one note from an installed library or monolith
cargo run -p nk-format --bin nki-render -- <file.nki> note.wav \
  --note 60 --velocity 100 --seconds 3 [--group N] [--gain 1]

# List a monolith's embedded files
cargo run -p nk-format --bin nk-inspect -- <file.nkx>

# Compatibility sweep over all installed Kontakt content
cargo run -p nk-format --bin nk-sweep
```

Builds offline against cached `flate2` / `quick-xml`.

## plugin-api — the universal protocol

The contract every plugin targets, regardless of origin: **multi-channel planar
audio with a sidechain**, sample-accurate MIDI and parameter events, declared
latency/tail, validated parameters, a **declarative UI schema** (no executable
UI), and opaque versioned state. The full specification is in
[`crates/plugin-api/PROTOCOL.md`](crates/plugin-api/PROTOCOL.md).

Kontakt is an *adapter* behind this boundary, not the protocol. Native VST3/AU/
CLAP, WASM community plugins, and clean-room reimplementations will be further
providers (`plugin_api::provider`).

`plugin_api::wire` holds the layout-stable `#[repr(C)]` types for the future
WASM/out-of-process boundary; `plugin_api::conformance::gate` is the publish
gate that only issues a certificate when the suite passes.

```sh
cargo test -p plugin-api
```

The suite checks descriptor/parameter validity, UI→parameter references, finite
and bounded output (silence, note-on, note-off), audible instruments, and state
round-tripping.

```sh
cargo test -p plugin-api
```

It checks descriptor/parameter validity, UI→parameter references, finite and
bounded output (silence, note-on, note-off), audible instruments, and state
round-tripping.

## sample-adapter (Kontakt)

Preserves Kontakt compatibility as a `Plugin`: loads a library through
`nk-format`, keeps zone attacks and loop boundaries resident, streams later
WAV/NCW pages on an owned disk thread, and renders through the same host/protocol
as everything else.

```sh
cargo run -p sample-adapter --bin sample-host -- \
  "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki" \
  /tmp/piano.wav --note 60 --velocity 100 --seconds 3
```

The default worker and `sample-host` use streaming. `--resident` selects the
explicit full-RAM reference path. Realtime callbacks never read files, wait for
pages, allocate, or free sample storage. Offline rendering explicitly waits for
pages; read failures/timeouts fail the worker/CLI render rather than reporting a
successful bounce. Late realtime pages produce a short decay/recovery and count
as dropouts. See [streaming design and limits](docs/sample-streaming.md) and the
[recovered Kontakt reference](docs/kontakt-streaming-evidence.md).

The default decoded page cache is fixed at 16 MiB for 64 voices, plus resident
attacks/loop boundaries with a 128 MiB hard budget. WAV/NCW decoding reads ranges;
monolith and resource samples retain file offsets. File handles close between
page reads. Full sample preloading remains available for reference comparisons.

## Native Rust plugins

Native plugins are Rust `cdylib`s implementing `plugin_api::Plugin` and calling
`plugin_sdk::export_plugin!`. They export a stable `#[repr(C)]` vtable; the
loader checks the ABI version and wraps them as normal `Plugin` instances.

```sh
cargo build -p gain
cargo run -p plugin-loader --bin native-host -- \
  target/debug/libgain.dylib /tmp/out.wav --note 69 --seconds 2
```

The in-process loader is a development path. Native code has no WASM sandbox,
so untrusted packages must run out-of-process (same vtable, shared-memory
audio) once the supervisor lands.

## Native plugin adapters

Existing native plugins run through the same sandbox and protocol:

```sh
# CLAP
cargo run -p adapter-clap --bin clap-inspect -- \
  "/Library/Audio/Plug-Ins/CLAP/MiniMeters - Spectrum.clap"
cargo run -p plugin-sandbox --bin sandbox-host -- \
  "/Library/Audio/Plug-Ins/CLAP/MiniMeters - Spectrum.clap" /tmp/clap.wav

# macOS Audio Units (addressed by name)
cargo run -p adapter-au --bin au-inspect
cargo run -p plugin-sandbox --bin sandbox-host -- \
  "au:AUDynamicsProcessor" /tmp/au.wav
```

Verified: MiniMeters Spectrum (CLAP, 2 in/2 out) and Apple AUNBandEQ/AUDynamicsProcessor
(AU) render through the sandbox. The CLAP path now forwards timed notes,
controllers, parameters, transport and state, with generic controls in Cadence.
Worker-hosted floating native CLAP editors and core host request dispatch are implemented; broader GUI extensions and third-party validation remain. Some AUv3
or DRM-protected components may refuse to instantiate; that is reported, not
crashed.

## Signed packages and install store

Packages are JSON manifests plus content-addressed assets, signed with the
publisher's Ed25519 key. `plugin-store` verifies and installs them, keyed by the
publisher public key.

```sh
cargo build -p gain
target/debug/platform keygen ./keys

mkdir -p ./gain-pkg/assets
cp target/debug/libgain.dylib ./gain-pkg/assets/gain.dylib
cat > ./gain-pkg/manifest.json <<'JSON'
{"schema_version":1,"id":"com.example.gain","name":"Gain","version":"1.0.0",
 "kind":"effect","api_version":1,"entry":"gain.dylib","assets":[]}
JSON

target/debug/platform pack ./keys/publisher.key ./gain-pkg --entry gain.dylib
target/debug/platform install ./gain-pkg
target/debug/platform list
target/debug/platform render com.example.gain /tmp/out.wav --note 69
```

Verified: the installed plugin is loaded through the sandbox worker and renders
identically (peak 0.200, rms 0.1103).

No Apple Developer account is needed for this: **publisher identity is our
Ed25519 signature, not Apple code signing.** Unsigned/ad-hoc native binaries
work for development; notarization is only a distribution concern later.

## Discovery

Creators host a **signed feed** (static JSON) plus their package manifest and
assets on any static host. Users subscribe to the feed URL; the first fetch pins
the publisher key, and a key change is rejected. The local index caches verified
feeds.

```sh
# Creator: sign a feed template
target/debug/platform feed sign ./keys/publisher.key ./feed-template.json ./feed.json

# User: subscribe, browse, install
target/debug/platform subscribe https://creator.example/feed.json
target/debug/platform discover
target/debug/platform install-release com.example.gain
```

Federation and moderation:

```sh
# Aggregate many publishers through an indexer (a directory of feed URLs)
target/debug/platform index add https://index.example/index.json
target/debug/platform index list

# Choose community labelers (signed moderation lists)
target/debug/platform labeler add https://labeler.example/labels.json
target/debug/platform labeler list

# Search local verified feeds; blocked plugins are hidden unless --all
target/debug/platform search piano
target/debug/platform discover --all        # show BLOCKED entries
```

Indexers only point at feeds; the client still verifies every feed, manifest
and asset. Labelers are signed documents keyed by plugin id: a block (severity
≥ 3 or `malware`/`blocked`) hides a release and prevents installation unless
`--allow-blocked` is passed. Users choose their own labelers — there is no
global authority.

Verified end-to-end with a local feed: publish → subscribe → discover → install →
sandboxed render (peak 0.200, rms 0.1103). HTTPS fetching uses `ureq`; local
paths and `file://` work for development.

## Sandbox

`plugin-sandbox` runs a native plugin in a **separate worker process** with
shared-memory audio, events and state. If the worker crashes or stops
responding, the host outputs silence instead of taking down the session.

```sh
cargo build -p gain
cargo run -p plugin-sandbox --bin sandbox-host -- \
  target/debug/libgain.dylib /tmp/out.wav --note 69 --seconds 2
```

Verified: the sandboxed gain plugin produces the same output as the in-process
loader (peak 0.200, rms 0.1103).

The realtime JUCE bridge uses an eight-slot atomic shared-memory pipeline and
never waits for a worker on the audio callback. A nonblocking semaphore post wakes
the worker directly. Compatible small callbacks combine under queue pressure
without dropping audio or MIDI, and watchdog deadlines respect the prepared audio
buffer duration. Each bridged node adds one
prepared maximum buffer of latency; JUCE compensates graph paths and reports
rack latency to the DAW. Missing output is zeroed and late samples are discarded.
Offline rendering retains the same sample delay; the Rust CLI retains its
synchronous exchange. A separate watchdog kills stalled workers and destruction
reaps them off the audio thread. Tests at 64/128/256 frames cover callback timing,
compensation and zero-underrun paced playback. This is not a hard realtime
scheduling guarantee. Process isolation is not an OS security sandbox.

## plugin-package

Packages are manifests plus immutable, content-addressed assets (IRs, model
weights, images). Assets are SHA-256 digested and validated, and exposed to
plugins through `plugin_api::AssetProvider`:

```rust
let mut package = Package::new(Manifest::new(
    "com.example.eq", "Example EQ", "1.0.0", PluginKind::Effect, "plugin.wasm"));
package.add_asset("cabinet.wav", "audio/wav", bytes)?;
package.validate()?;
```

Publisher signatures and asset verification are implemented in the publish and
install flow, and checked again when Cadence opens a package manifest.

## host

The reference host owns a `Plugin`, prepares the context, schedules events at
block boundaries, and renders offline. This exercises the contract end to end.

```sh
cargo run -p host --bin host-demo -- /tmp/demo.wav --note 69 --seconds 2
cargo test -p host
```

The demo uses a small sine instrument; tests cover finite output, block-size
invariance (64 vs 256), and note-off release to silence.

`host::chain::Chain` runs instruments into a serial effect chain with host-level
bypass and summed latency. Tests cover unity passthrough, bypass/attenuation,
latency summation, and effect conformance.

## Next steps

The ordered [Kontakt compatibility backlog](docs/kontakt-compatibility-roadmap.md)
tracks remaining sampler work and acceptance checks. Channel-aware note lifetime
handling and instrument-defined [external MIDI expression](docs/midi-expression.md)
are implemented; release-velocity/controller behavior is next.

Cadence now hosts native protocol plugins, Kontakt instruments, and CLAP through
`cadence-bridge` and the Rust worker, alongside direct JUCE AU/VST3 hosting.
The Library includes the bundled Sine Demo and Gain; **Open...** imports chosen
instruments/plugins/packages. Scanning retains isolation, cancellation, skips,
and live updates. See [`app/README.md`](app/README.md) for use, verification and limits.

1. **DAW playback + load testing** — complete Logic Pro session checks, stress
   the pipeline under load, and add CPU/resource monitoring.
2. **Streaming validation** — inspector telemetry now exposes sample/cache memory, ready-page peaks, page delivery timing, voices, queue and distinct streaming misses. Large-library Logic validation is deferred until the user has a large library available; retain slow-storage and multi-hour stress checks for that pass.
3. **Rust VST3 adapter** — the JUCE shell hosts VST3; the Rust worker still needs
   its own adapter and SDK/license decision.
4. **Adapter/editor work** — multi-plugin CLAP enumeration, core CLAP host
   callbacks, live latency/tail and parameter values, rack sidechain routing,
   rack-to-DAW automation and macOS Audio Workgroups (AU + standalone) are done.
   Worker-hosted native CLAP editors, native AU/VST3 automation, sidechain
   routing, MIDI/SysEx output and automatic CLAP structural refresh are
   implemented.
5. **Network transports** — optional Nostr/AT relays on top of feeds/indexers.
