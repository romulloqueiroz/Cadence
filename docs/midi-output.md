# MIDI output and SysEx

Cadence now exposes MIDI output from native AU/VST3 slots and from bridged CLAP
slots.

## Native AU/VST3

- `CadenceAudioProcessor` reports `producesMidi()` and creates a graph MIDI
  output node.
- Every active slot whose processor `producesMidi()` is connected to that node.
  JUCE merges the slot MIDI buffers; native SysEx travels unchanged.
- The plugin is built with `NEEDS_MIDI_OUTPUT TRUE`, so AU/VST3/Standalone hosts
  expose a MIDI output.

## Bridged CLAP

- `plugin_api::EventKind::SysEx` uses `data1` as an offset into an output SysEx
  buffer and `data2` as its length.
- The CLAP adapter implements a bounded output collector for note, pressure,
  controller, program, pitch-bend and SysEx events. Only CLAP currently returns
  output; other Rust adapters return none.
- Private worker protocol version 9 (for that feature; current layout is 10)
  adds output event/SysEx fields to both the pipelined packet and offline block.
- `cadence_bridge_take_output_midi` returns one completed packet's group. The
  C++ processor holds it for the next callback, matching the pipeline's
  one-buffer audio delay, then emits it through the graph MIDI output node.
- SysEx is capped at 16 KiB per packet and output events at 256 per packet.
  Larger output is dropped rather than blocking.

## Tests and limits

- The CLAP fixture emits a MIDI note and a SysEx message when it receives note
  126; the test verifies both reach the host.
- Nested Cadence AU/VST3 tests verify native MIDI-output graph routing
  structurally; the native instances do not currently emit test MIDI.
- Bridged MIDI output is intentionally best-effort under late worker completion:
  the one-buffer delay can grow if the worker misses its slot. Native SysEx is
  the general host path; CLAP SysEx is the worker path.
