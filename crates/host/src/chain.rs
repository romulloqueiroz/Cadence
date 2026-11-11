//! A serial effect graph. Instruments feed effects; effects feed effects.
//!
//! Bypass is a host-level flag (the plugin is not called). Latency is summed so
//! the session can compensate; parallel routing and PDC alignment come with the
//! full graph.

use crate::{Host, NoteSpec, RenderedAudio};
use plugin_api::{Event, Plugin, MAX_BLOCK_SIZE};

pub struct Node {
    pub host: Host,
    pub bypass: bool,
}

pub struct Chain {
    nodes: Vec<Node>,
    block_size: usize,
    sample_rate: f64,
    signal: [Vec<f32>; 2],
}

impl Chain {
    pub fn new(sample_rate: f64, block_size: usize) -> Self {
        let block_size = block_size.clamp(1, MAX_BLOCK_SIZE);
        Chain {
            nodes: Vec::new(),
            block_size,
            sample_rate,
            signal: [vec![0.0; block_size], vec![0.0; block_size]],
        }
    }

    pub fn add(&mut self, plugin: Box<dyn Plugin>, bypass: bool) -> &mut Self {
        let host = Host::new(plugin, self.sample_rate, self.block_size);
        self.nodes.push(Node { host, bypass });
        self
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn nodes_mut(&mut self) -> &mut [Node] {
        &mut self.nodes
    }

    pub fn block_size(&self) -> usize {
        self.block_size
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn prepare(&mut self) {
        for node in &mut self.nodes {
            node.host.prepare();
        }
    }

    pub fn reset(&mut self) {
        for node in &mut self.nodes {
            node.host.reset();
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        for node in &mut self.nodes {
            node.host.set_playing(playing);
        }
    }

    /// Total reported latency of the chain, in samples.
    pub fn latency_samples(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| node.host.capabilities().latency_samples)
            .sum()
    }

    /// Process one block through the chain. `input` seeds the first effect;
    /// instruments ignore it. Returns the final stereo signal.
    pub fn process(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        events: &[Event],
    ) -> (&[f32], &[f32]) {
        let frames = self.block_size;
        self.signal[0].fill(0.0);
        self.signal[1].fill(0.0);
        if let Some((left, right)) = input {
            copy_into(&mut self.signal[0], left);
            copy_into(&mut self.signal[1], right);
        }

        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            if node.bypass {
                continue;
            }

            let capabilities = node.host.capabilities();

            if capabilities.inputs > 0 {
                if capabilities.inputs == 1 {
                    let input = node.host.input_mut(0);
                    for (frame, value) in input.iter_mut().enumerate() {
                        *value = 0.5 * (self.signal[0][frame] + self.signal[1][frame]);
                    }
                } else {
                    for channel in 0..capabilities.inputs {
                        let destination = node.host.input_mut(channel);
                        if channel < 2 {
                            destination.copy_from_slice(&self.signal[channel]);
                        } else {
                            destination.fill(0.0);
                        }
                    }
                }
            }

            node.host.process(events);

            match capabilities.outputs {
                0 => {}
                1 => {
                    let mono = node.host.output(0);
                    self.signal[0].copy_from_slice(&mono[..frames]);
                    self.signal[1].copy_from_slice(&mono[..frames]);
                }
                _ => {
                    let left = node.host.output(0);
                    let right = node.host.output(1);
                    self.signal[0].copy_from_slice(&left[..frames]);
                    self.signal[1].copy_from_slice(&right[..frames]);
                }
            }
        }

        (&self.signal[0], &self.signal[1])
    }
}

fn copy_into(destination: &mut [f32], source: &[f32]) {
    let count = destination.len().min(source.len());
    destination[..count].copy_from_slice(&source[..count]);
}

/// Render one note through a chain, copying the final stereo output.
pub fn render_note(chain: &mut Chain, note: NoteSpec, seconds: f64) -> RenderedAudio {
    for node in chain.nodes_mut() {
        node.host.plugin_mut().set_offline_rendering(true);
    }
    let rate = chain.sample_rate();
    let block = chain.block_size();
    let total = (seconds.max(0.0) * rate) as usize;

    chain.reset();
    chain.set_playing(true);

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

        let (output_left, output_right) = chain.process(None, &events);
        let count = (total - position).min(block);
        left.extend_from_slice(&output_left[..count]);
        right.extend_from_slice(&output_right[..count]);

        position += block;
    }

    for node in chain.nodes_mut() {
        node.host.plugin_mut().set_offline_rendering(false);
    }
    RenderedAudio {
        left,
        right,
        sample_rate: rate as u32,
    }
}
