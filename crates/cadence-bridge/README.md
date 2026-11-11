# cadence-bridge

Version 2 C ABI from JUCE to `plugin-sandbox`. The static library contains the
host/supervisor and format dependencies; plugin execution occurs only in the
separate `plugin-worker` binary. See `include/cadence_bridge.h` for signatures,
wire layouts, capacities and ownership rules, and `../../app/README.md` for use,
verification and limitations.

Open, prepare, reset, save/load and close are non-audio operations. The caller
serializes them against process. Each handle owns immutable metadata and a
serialized mutable DSP state; atomic failure status can be queried separately.
The C boundary catches Rust unwinds, validates counts/null arguments, and uses
caller-owned planar output storage. Plugin state is opaque with its own schema;
failed/oversized saves return an error and never truncate data.

The process function converts MIDI/parameter wire events into fixed storage and
uses preallocated audio memory. No plugin code runs in the calling process.
Realtime processing publishes packets into an eight-slot atomic shared-memory
ring without waiting for the worker. Each node adds one prepared maximum buffer
of latency; the JUCE graph reports and compensates it alongside intrinsic plugin
latency. Missing output becomes silence and late samples are discarded. Atomic
pipeline statistics expose latency, pending packets, underruns and missed frames.
Offline processing (context flag bit 1) may wait, with the same sample delay and
support for larger callbacks. Realtime frames must fit the prepared maximum.

The dedicated watchdog terminates stalled workers off the audio thread. Close
stops the watchdog, kills/reaps the worker, and releases mappings and buffers.
This is not a hard realtime scheduling or OS security-sandbox guarantee.
