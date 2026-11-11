//! Compact worker-only normalized automation ramps.
//!
//! Wire kind 8 is a linear ramp: frame=start, data1=parameter ID,
//! data2=duration (1..=4096 frames), value=normalized end value. It must be
//! immediately preceded in the packet by a channel-0 kind-7 parameter event at
//! the same frame/ID containing the normalized start value. SDK/native events
//! remain unchanged; only this worker expands ramps into ordinary parameters.
//!
//! Packets with ramps are completely validated before DSP and rendered one
//! frame at a time using stack storage. Parameter updates precede MIDI at equal
//! frame offsets. A new ordinary parameter overrides an earlier ramp for that
//! ID. Ramps are clipped to this packet; the sender supplies each continuation
//! and an endpoint at the next block when the end falls on a block boundary.
use crate::{
    metadata::ParameterMetadata,
    shared::{MAX_BLOCK, MAX_CHANNELS, MAX_EVENTS},
};
use plugin_api::{wire::WireEvent, AudioBuffer, Event, Plugin, ProcessContext};

pub const RAMP_KIND: u32 = 8;
const PARAMETER_KIND: u32 = 7;
const MAX_RAMPS: usize = 8;

#[derive(Clone, Copy)]
struct Ramp {
    id: u16,
    start: usize,
    end: usize,
    from: f32,
    to: f32,
}
impl Ramp {
    fn from_wire(wire: &WireEvent, preceding: &WireEvent) -> Self {
        Self {
            id: wire.data1,
            start: wire.frame as usize,
            end: wire.frame as usize + usize::from(wire.data2),
            from: preceding.value,
            to: wire.value,
        }
    }
    fn value(self, frame: usize) -> f32 {
        let t = (frame - self.start) as f64 / (self.end - self.start) as f64;
        (f64::from(self.from) + (f64::from(self.to) - f64::from(self.from)) * t) as f32
    }
}

/// Normal packets retain the original one-call filtering/sanitization path.
/// Invalid ramp packets return false before invoking the plugin.
pub fn render(
    plugin: &mut dyn Plugin,
    context: &ProcessContext,
    audio: &mut AudioBuffer<'_>,
    wires: &[WireEvent],
    events: &mut [Event; MAX_EVENTS],
    parameters: &[ParameterMetadata],
) -> bool {
    if !context.is_valid() {
        return false;
    }
    if !wires.iter().any(|wire| wire.kind == RAMP_KIND) {
        let mut count = 0;
        for wire in wires.iter().take(MAX_EVENTS).filter(|w| w.channel < 16) {
            if let Some(event) = wire
                .to_event()
                .filter(|e| e.frame < audio.frames && e.value.is_finite())
            {
                events[count] = event;
                count += 1;
            }
        }
        plugin.process(context, audio, &events[..count]);
        sanitize(audio.outputs);
        return true;
    }

    let mut order = [0usize; MAX_EVENTS];
    if !validate(context, audio, wires, parameters, &mut order) {
        return false;
    }
    let mut ramps = [None::<Ramp>; MAX_RAMPS];
    let mut cursor = 0;
    let frames = audio.frames;
    let mut inputs = [0.0f32; MAX_CHANNELS];
    let mut outputs = [0.0f32; MAX_CHANNELS];
    let mut sidechain = [0.0f32; MAX_CHANNELS];

    for frame in 0..frames {
        let mut count = 0;
        // Endpoint values from finishing ramps are emitted before retiring
        // their slots, so another ramp can start on that same sample.
        for slot in &mut ramps {
            if let Some(ramp) = *slot {
                events[count] = Event::parameter(0, u32::from(ramp.id), ramp.value(frame));
                count += 1;
                if frame == ramp.end {
                    *slot = None;
                }
            }
        }
        let begin = cursor;
        while cursor < wires.len() && wires[order[cursor]].frame as usize == frame {
            cursor += 1;
        }
        // Emit all explicit parameter values before any MIDI at this frame.
        // The required start-value event also supplies the ramp's first sample.
        for &index in &order[begin..cursor] {
            let wire = &wires[index];
            if wire.kind == PARAMETER_KIND {
                for slot in &mut ramps {
                    if slot.is_some_and(|ramp| ramp.id == wire.data1) {
                        *slot = None;
                    }
                }
                let mut event = wire.to_event().unwrap();
                event.frame = 0;
                events[count] = event;
                count += 1;
            } else if wire.kind == RAMP_KIND {
                let slot = ramps.iter_mut().find(|slot| slot.is_none()).unwrap();
                *slot = Some(Ramp::from_wire(wire, &wires[index - 1]));
            }
        }
        for &index in &order[begin..cursor] {
            let wire = &wires[index];
            if wire.kind < PARAMETER_KIND {
                let mut event = wire.to_event().unwrap();
                event.frame = 0;
                events[count] = event;
                count += 1;
            }
        }
        // Bound proof: each generated parameter belongs to a kind-8 event in
        // the input packet; all other emitted events are non-kind-8 inputs.
        // Therefore count <= wires.len() <= MAX_EVENTS for every sample.
        debug_assert!(count <= MAX_EVENTS);
        for (channel, sample) in inputs.iter_mut().enumerate().take(audio.input_channels) {
            *sample = audio.inputs[channel * frames + frame];
        }
        for (channel, sample) in outputs.iter_mut().enumerate().take(audio.output_channels) {
            *sample = audio.outputs[channel * frames + frame];
        }
        for (channel, sample) in sidechain
            .iter_mut()
            .enumerate()
            .take(audio.sidechain_channels)
        {
            *sample = audio.sidechain[channel * frames + frame];
        }
        let mut one = AudioBuffer::new(
            &mut inputs,
            audio.input_channels,
            &mut outputs,
            audio.output_channels,
            &mut sidechain,
            audio.sidechain_channels,
            1,
        );
        let mut at = *context;
        at.block_size = 1;
        at.sample_pos = context.sample_pos + frame as i64;
        at.ppq_position =
            context.ppq_position + frame as f64 * context.tempo / (60.0 * context.sample_rate);
        plugin.process(&at, &mut one, &events[..count]);
        for (channel, sample) in inputs.iter().enumerate().take(audio.input_channels) {
            audio.inputs[channel * frames + frame] = *sample;
        }
        for (channel, sample) in outputs.iter().enumerate().take(audio.output_channels) {
            audio.outputs[channel * frames + frame] = *sample;
        }
        for (channel, sample) in sidechain.iter().enumerate().take(audio.sidechain_channels) {
            audio.sidechain[channel * frames + frame] = *sample;
        }
    }
    sanitize(audio.outputs);
    true
}

fn validate(
    context: &ProcessContext,
    audio: &AudioBuffer<'_>,
    wires: &[WireEvent],
    parameters: &[ParameterMetadata],
    order: &mut [usize; MAX_EVENTS],
) -> bool {
    if wires.len() > MAX_EVENTS
        || audio.frames == 0
        || audio.frames > MAX_BLOCK
        || audio.input_channels > MAX_CHANNELS
        || audio.output_channels > MAX_CHANNELS
        || audio.sidechain_channels > MAX_CHANNELS
        || audio.inputs.len() < audio.input_channels * audio.frames
        || audio.outputs.len() < audio.output_channels * audio.frames
        || audio.sidechain.len() < audio.sidechain_channels * audio.frames
        || context
            .sample_pos
            .checked_add((audio.frames - 1) as i64)
            .is_none()
        || !(context.ppq_position
            + (audio.frames - 1) as f64 * context.tempo / (60.0 * context.sample_rate))
            .is_finite()
    {
        return false;
    }
    for (index, wire) in wires.iter().enumerate() {
        if wire.frame as usize >= audio.frames
            || wire.channel >= 16
            || !wire.value.is_finite()
            || wire.kind > RAMP_KIND
        {
            return false;
        }
        if (wire.kind == PARAMETER_KIND || wire.kind == RAMP_KIND)
            && (wire.channel != 0
                || !(0.0..=1.0).contains(&wire.value)
                || !parameters
                    .iter()
                    .any(|p| p.id == u32::from(wire.data1) && !p.read_only))
        {
            return false;
        }
        if wire.kind == RAMP_KIND {
            let Some(previous) = index.checked_sub(1).map(|i| &wires[i]) else {
                return false;
            };
            if wire.data2 == 0
                || usize::from(wire.data2) > MAX_BLOCK
                || previous.kind != PARAMETER_KIND
                || previous.channel != 0
                || previous.frame != wire.frame
                || previous.data1 != wire.data1
            {
                return false;
            }
        }
        order[index] = index;
        // Stable ordering handles ordinary MIDI sorted independently from
        // automation while retaining the required adjacent pair at equal time.
        let mut position = index;
        while position > 0 && wires[order[position - 1]].frame > wire.frame {
            order[position] = order[position - 1];
            position -= 1;
        }
        order[position] = index;
    }
    let mut active = [None::<Ramp>; MAX_RAMPS];
    for &index in &order[..wires.len()] {
        let wire = &wires[index];
        for slot in &mut active {
            if slot.is_some_and(|ramp| {
                ramp.end <= wire.frame as usize
                    || (wire.kind == PARAMETER_KIND && ramp.id == wire.data1)
            }) {
                *slot = None;
            }
        }
        if wire.kind == RAMP_KIND {
            let Some(slot) = active.iter_mut().find(|slot| slot.is_none()) else {
                return false;
            };
            *slot = Some(Ramp::from_wire(wire, &wires[index - 1]));
        }
    }
    true
}
fn sanitize(outputs: &mut [f32]) {
    for sample in outputs {
        if !sample.is_finite() {
            *sample = 0.0;
        }
    }
}
