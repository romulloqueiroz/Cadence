# Rack sidechain routing

Cadence now exposes a stereo sidechain input and routes it to bridged plugins
that declare sidechain ports.

## Host path

- `CadenceAudioProcessor` declares a stereo `Sidechain` input bus beside its
  stereo output. AU and VST3 hosts can route an external sidechain into it.
- `processBlock` copies the sidechain input into a preallocated buffer before
  clearing the output buffer, restores it for the graph input node, and marks
  the parent buffer not-clear so JUCE's graph does not discard it.
- The graph's audio-input node is connected to each active slot's sidechain bus
  channels. Main-chain audio still uses the normal previous-node connection.
- Native AU/VST3 slots are inspected by input-bus name. A bus named
  `Sidechain`/`side-chain` is routed even when it is bus 0 (as in an
  instrument-sidechain-only host). If no bus is named, additional input buses
  after the main bus are treated as sidechain.

## Worker path

- The CLAP adapter classifies input ports without `CLAP_AUDIO_PORT_IS_MAIN` as
  sidechain ports (instrument plugins may have a sidechain-only input).
- `Capabilities.sidechain_inputs` reports the total sidechain channels.
- `RustPluginProcessor` creates a JUCE sidechain bus, copies it into the bridge's
  planar sidechain arrays, and `cadence_bridge_process_sidechain` passes it to
  the worker. The worker already carried sidechain audio in its packet layout.
- Plugins without sidechain inputs behave as before; the new C entry point leaves
  the original process ABI untouched.

## Tests and limits

The CLAP test fixture exposes a third variant with a stereo sidechain port. The
rack test feeds a 0.5 sidechain signal, observes the expected scaled output, then
clears the sidechain and observes the dry gain. Nested Cadence AU/VST3 tests
verify that a native slot's sidechain bus is connected to the host input.

Limits: bridged plugins with sidechain ports and native AU/VST3 input buses named
`Sidechain` (or extra input buses after the main bus) are routed. The graph still
supplies a stereo rack; wider sidechain layouts are not exposed in the UI.
Native AU/VST3 audio-through-sidechain timing is not separately measured in the
test suite.
