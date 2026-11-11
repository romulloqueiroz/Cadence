# Cadence state fixtures

`legacy-cadence-chain.xml` is Cadence's own saved Sine Demo + Gain rack from
2026-11-06, before persistent slot UUIDs and rack automation were introduced.
It contains the two built-in source identifiers and their parameter/opaque state;
there are no third-party instruments, samples or plugin binaries in this fixture.

The XML was recovered from the `jucePluginState` field in the disposable Logic
verification project's saved Audio Unit plist. The regression loads it both
before and after prepare and checks actual output, rather than merely accepting
the XML or creating worker processes. The native Audio Unit integration test
also restores it inside the AU wrapper's plist envelope and verifies audible
output and serial pipeline latency. These isolated passes do not certify Logic's
own playback/bounce path.

The AU integration check additionally sends MIDI 1 and MIDI 2 universal packets
through `MusicDeviceMIDIEventList`, requires sound after each note-on, and checks
silence after the demo's exponential release. This covers a separate delivery
path from JUCE's legacy MIDI event API.
