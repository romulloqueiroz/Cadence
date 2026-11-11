//! Reference host: owns a plugin, schedules events, and renders audio.
//!
//! This is the in-process reference implementation of the host side of the
//! protocol. The production host adds sandboxing, a graph and a transport, but
//! the block/event semantics are the same.

pub mod chain;
pub mod demo;

use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, Plugin, ProcessContext, MAX_BLOCK_SIZE,
    MAX_CHANNELS,
};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub struct Host {
    plugin: Box<dyn Plugin>,
    context: ProcessContext,
    block_size: usize,
    capabilities: Capabilities,
    inputs: Vec<f32>,
    outputs: Vec<f32>,
    sidechain: Vec<f32>,
}

impl Host {
    /// `block_size` is clamped to `1..=MAX_BLOCK_SIZE`. Buffers are allocated
    /// from the plugin's declared capabilities.
    pub fn new(plugin: Box<dyn Plugin>, sample_rate: f64, block_size: usize) -> Self {
        let block_size = block_size.clamp(1, MAX_BLOCK_SIZE);
        let capabilities = plugin.capabilities();
        let input_channels = capabilities.inputs.min(MAX_CHANNELS);
        let output_channels = capabilities.outputs.min(MAX_CHANNELS);
        let sidechain_channels = capabilities.sidechain_inputs.min(MAX_CHANNELS);

        Host {
            plugin,
            context: ProcessContext {
                sample_rate,
                block_size,
                tempo: 120.0,
                playing: false,
                sample_pos: 0,
                ppq_position: 0.0,
            },
            block_size,
            capabilities,
            inputs: vec![0.0; input_channels * block_size],
            outputs: vec![0.0; output_channels * block_size],
            sidechain: vec![0.0; sidechain_channels * block_size],
        }
    }

    pub fn prepare(&mut self) {
        self.plugin.prepare(&self.context);
        self.plugin.reset();
        self.context.sample_pos = 0;
    }

    pub fn plugin(&self) -> &dyn Plugin {
        self.plugin.as_ref()
    }

    pub fn plugin_mut(&mut self) -> &mut dyn Plugin {
        self.plugin.as_mut()
    }

    pub fn descriptor(&self) -> Descriptor {
        self.plugin.descriptor()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn sample_rate(&self) -> f64 {
        self.context.sample_rate
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.context.playing = playing;
    }

    /// Fill one input channel for the next block (effects). The caller owns
    /// clearing inputs; the host only clears outputs.
    pub fn input_mut(&mut self, channel: usize) -> &mut [f32] {
        let start = channel * self.block_size;
        &mut self.inputs[start..start + self.block_size]
    }

    /// The current block's output for one channel. Valid until the next
    /// `process` call.
    pub fn output(&self, channel: usize) -> &[f32] {
        let start = channel * self.block_size;
        &self.outputs[start..start + self.block_size]
    }

    /// Process exactly `block_size` frames. Event frames must be relative to
    /// this block. Outputs are cleared first; plugins add into them.
    pub fn process(&mut self, events: &[Event]) {
        self.outputs.fill(0.0);

        let mut context = self.context;
        context.block_size = self.block_size;

        let mut audio = AudioBuffer::new(
            &mut self.inputs,
            self.capabilities.inputs.min(MAX_CHANNELS),
            &mut self.outputs,
            self.capabilities.outputs.min(MAX_CHANNELS),
            &mut self.sidechain,
            self.capabilities.sidechain_inputs.min(MAX_CHANNELS),
            self.block_size,
        );
        self.plugin.process(&context, &mut audio, events);

        self.context.sample_pos = self
            .context
            .sample_pos
            .saturating_add(self.block_size as i64);
        self.context.ppq_position +=
            self.block_size as f64 / self.context.sample_rate * self.context.tempo / 60.0;
    }

    pub fn reset(&mut self) {
        self.plugin.reset();
        self.context.sample_pos = 0;
        self.context.ppq_position = 0.0;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NoteSpec {
    pub key: u8,
    pub velocity: u8,
    pub start: f64,
    pub duration: f64,
}

impl NoteSpec {
    pub fn new(key: u8, velocity: u8, start: f64, duration: f64) -> Self {
        NoteSpec {
            key,
            velocity,
            start,
            duration,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderedAudio {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: u32,
}

impl RenderedAudio {
    pub fn frames(&self) -> usize {
        self.left.len().min(self.right.len())
    }

    pub fn peak(&self) -> f32 {
        self.left
            .iter()
            .chain(&self.right)
            .fold(0.0f32, |peak, value| peak.max(value.abs()))
    }

    pub fn rms(&self) -> f64 {
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for value in self.left.iter().chain(&self.right) {
            sum += (*value as f64) * (*value as f64);
            count += 1;
        }
        if count == 0 {
            0.0
        } else {
            (sum / count as f64).sqrt()
        }
    }

    pub fn write_wav(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let frames = self.frames();
        let data_size = (frames * 2 * 2) as u32;
        let mut file = BufWriter::new(File::create(path)?);

        file.write_all(b"RIFF")?;
        file.write_all(&(36 + data_size).to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&2u16.to_le_bytes())?;
        file.write_all(&self.sample_rate.to_le_bytes())?;
        file.write_all(&(self.sample_rate * 4).to_le_bytes())?;
        file.write_all(&4u16.to_le_bytes())?;
        file.write_all(&16u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&data_size.to_le_bytes())?;

        for frame in 0..frames {
            for channel in [&self.left, &self.right] {
                let value = channel[frame].clamp(-1.0, 1.0);
                let pcm = (value * i16::MAX as f32).round() as i16;
                file.write_all(&pcm.to_le_bytes())?;
            }
        }
        file.flush()
    }
}

/// Render one note through the host. Output is the first two output channels
/// (mono is duplicated to stereo).
pub fn render_note(host: &mut Host, note: NoteSpec, seconds: f64) -> RenderedAudio {
    host.plugin_mut().set_offline_rendering(true);
    let rate = host.sample_rate();
    let block = host.block_size();
    let total = (seconds.max(0.0) * rate) as usize;
    let capability = host.capabilities();

    host.reset();
    host.set_playing(true);

    let mut left = Vec::with_capacity(total);
    let mut right = Vec::with_capacity(total);
    let mut events = Vec::with_capacity(4);

    let note_on = note.start.max(0.0);
    let note_off = note_on + note.duration.max(0.0);
    let mut on_sent = false;
    let mut off_sent = false;
    let mut position = 0usize;

    while position < total {
        let start_time = position as f64 / rate;
        let end_time = (position + block) as f64 / rate;

        events.clear();
        if !on_sent && note_on < end_time {
            let offset = ((note_on - start_time) * rate).round();
            if (0.0..block as f64).contains(&offset) {
                events.push(Event::note_on(offset as usize, note.key, note.velocity));
                on_sent = true;
            }
        }
        if !off_sent && note_off < end_time {
            let offset = ((note_off - start_time) * rate).round();
            if (0.0..block as f64).contains(&offset) {
                events.push(Event::note_off(offset as usize, note.key));
                off_sent = true;
            }
        }

        host.process(&events);

        let count = (total - position).min(block);
        if capability.outputs >= 2 {
            left.extend_from_slice(&host.output(0)[..count]);
            right.extend_from_slice(&host.output(1)[..count]);
        } else if capability.outputs == 1 {
            let mono = &host.output(0)[..count];
            left.extend_from_slice(mono);
            right.extend_from_slice(mono);
        }

        position += block;
    }

    host.plugin_mut().set_offline_rendering(false);
    RenderedAudio {
        left,
        right,
        sample_rate: rate as u32,
    }
}
