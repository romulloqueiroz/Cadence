//! Plain-data wire types for the future cross-language boundary.
//!
//! The Rust [`crate::Plugin`] trait is the reference shape. A WASM component or
//! an out-of-process sandbox will exchange this `#[repr(C)]` layout instead.
//! Nothing here allocates or performs I/O.

use crate::{Capabilities, Event, EventKind, ProcessContext, API_VERSION};

/// Wire contract version.
pub const WIRE_VERSION: u32 = 1;

pub const FLAG_PLAYING: u32 = 1 << 0;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WireProcessContext {
    pub sample_rate: f64,
    pub block_size: u32,
    pub tempo: f64,
    pub flags: u32,
    pub sample_pos: i64,
    pub ppq_position: f64,
}

impl From<ProcessContext> for WireProcessContext {
    fn from(context: ProcessContext) -> Self {
        WireProcessContext {
            sample_rate: context.sample_rate,
            block_size: context.block_size.min(u32::MAX as usize) as u32,
            tempo: context.tempo,
            flags: if context.playing { FLAG_PLAYING } else { 0 },
            sample_pos: context.sample_pos,
            ppq_position: context.ppq_position,
        }
    }
}

impl WireProcessContext {
    pub fn to_context(self) -> ProcessContext {
        ProcessContext {
            sample_rate: self.sample_rate,
            block_size: self.block_size as usize,
            tempo: self.tempo,
            playing: self.flags & FLAG_PLAYING != 0,
            sample_pos: self.sample_pos,
            ppq_position: self.ppq_position,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireCapabilities {
    pub inputs: u32,
    pub outputs: u32,
    pub sidechain_inputs: u32,
    pub latency_samples: u32,
    pub tail_samples: u32,
}

impl From<Capabilities> for WireCapabilities {
    fn from(capabilities: Capabilities) -> Self {
        WireCapabilities {
            inputs: capabilities.inputs as u32,
            outputs: capabilities.outputs as u32,
            sidechain_inputs: capabilities.sidechain_inputs as u32,
            latency_samples: capabilities.latency_samples.min(u32::MAX as usize) as u32,
            tail_samples: capabilities.tail_samples.min(u32::MAX as usize) as u32,
        }
    }
}

impl WireCapabilities {
    pub fn to_capabilities(self) -> Capabilities {
        Capabilities {
            inputs: self.inputs as usize,
            outputs: self.outputs as usize,
            sidechain_inputs: self.sidechain_inputs as usize,
            latency_samples: self.latency_samples as usize,
            tail_samples: self.tail_samples as usize,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WireEvent {
    pub frame: u32,
    pub kind: u32,
    pub channel: u16,
    pub data1: u16,
    pub data2: u16,
    _padding: u16,
    pub value: f32,
}

impl From<Event> for WireEvent {
    fn from(event: Event) -> Self {
        WireEvent {
            frame: event.frame.min(u32::MAX as usize) as u32,
            kind: kind_code(event.kind),
            channel: event.channel as u16,
            data1: event.data1,
            data2: event.data2,
            _padding: 0,
            value: event.value,
        }
    }
}

impl WireEvent {
    pub fn to_event(self) -> Option<Event> {
        Some(Event {
            frame: self.frame as usize,
            kind: kind_from_code(self.kind)?,
            channel: self.channel as u8,
            data1: self.data1,
            data2: self.data2,
            value: self.value,
        })
    }
}

fn kind_code(kind: EventKind) -> u32 {
    match kind {
        EventKind::NoteOn => 0,
        EventKind::NoteOff => 1,
        EventKind::PolyPressure => 2,
        EventKind::ControlChange => 3,
        EventKind::ProgramChange => 4,
        EventKind::PitchBend => 5,
        EventKind::ChannelPressure => 6,
        EventKind::Parameter => 7,
        EventKind::SysEx => 9,
    }
}

fn kind_from_code(code: u32) -> Option<EventKind> {
    match code {
        0 => Some(EventKind::NoteOn),
        1 => Some(EventKind::NoteOff),
        2 => Some(EventKind::PolyPressure),
        3 => Some(EventKind::ControlChange),
        4 => Some(EventKind::ProgramChange),
        5 => Some(EventKind::PitchBend),
        6 => Some(EventKind::ChannelPressure),
        7 => Some(EventKind::Parameter),
        9 => Some(EventKind::SysEx),
        _ => None,
    }
}

/// The wire contract is independent of the Rust API version, but they advance
/// together for now.
pub const SUPPORTED_API_VERSION: u32 = API_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_round_trips() {
        let context = ProcessContext {
            sample_rate: 44_100.0,
            block_size: 128,
            tempo: 90.0,
            playing: true,
            sample_pos: -12_345,
            ppq_position: 7.5,
        };
        let wire = WireProcessContext::from(context);
        let back = wire.to_context();
        assert_eq!(back.sample_rate, context.sample_rate);
        assert_eq!(back.block_size, context.block_size);
        assert_eq!(back.tempo, context.tempo);
        assert_eq!(back.playing, context.playing);
        assert_eq!(back.sample_pos, context.sample_pos);
        assert!((back.ppq_position - context.ppq_position).abs() < 1e-9);
    }

    #[test]
    fn capabilities_round_trip() {
        let capabilities = Capabilities {
            inputs: 2,
            outputs: 2,
            sidechain_inputs: 1,
            latency_samples: 64,
            tail_samples: 4800,
        };
        assert_eq!(
            WireCapabilities::from(capabilities).to_capabilities(),
            capabilities
        );
    }

    #[test]
    fn events_round_trip() {
        for event in [
            Event::note_on(5, 60, 100),
            Event::note_off(7, 60),
            Event::pitch_bend(1, -0.5),
            Event::parameter(3, 1, 0.25),
        ] {
            let wire = WireEvent::from(event);
            assert_eq!(wire.to_event(), Some(event));
        }
    }

    #[test]
    fn unknown_event_kind_is_rejected() {
        let wire = WireEvent {
            frame: 0,
            kind: 99,
            channel: 0,
            data1: 0,
            data2: 0,
            _padding: 0,
            value: 0.0,
        };
        assert!(wire.to_event().is_none());
    }
}
