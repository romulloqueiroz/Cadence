//! Sample-timed output storage. Only the host callback accesses this local data.
//! No waits or allocations occur after construction, including on underruns.
use crate::shared::{AudioPacket, MAX_BLOCK, MAX_CHANNELS};
use plugin_api::{AudioBuffer, ProcessContext};

const CAPACITY: usize = MAX_BLOCK * 4;
pub(crate) struct Timeline {
    audio: Vec<f32>,
    valid: Vec<u64>,
    pub position: u64,
    pub epoch: u64,
    pub latency: usize,
    warm_until: u64,
    previous: Option<(i64, usize, bool)>,
}
impl Timeline {
    pub fn new() -> Self {
        Self {
            audio: vec![0.0; CAPACITY * MAX_CHANNELS],
            valid: vec![0; CAPACITY],
            position: 0,
            epoch: 0,
            latency: 0,
            warm_until: 0,
            previous: None,
        }
    }
    pub fn reset(&mut self, latency: usize) {
        self.position = 0;
        self.latency = latency;
        self.previous = None;
        self.flush();
    }
    pub fn flush(&mut self) {
        self.audio.fill(0.0);
        self.valid.fill(0);
        self.epoch = self.epoch.wrapping_add(1);
        self.warm_until = self.position.saturating_add(self.latency as u64);
    }
    /// A playing transport seek invalidates old queued audio and resets voices
    /// in the worker when it observes the new epoch. Stopped MIDI audition can
    /// report a constant host sample position, so it is not considered a seek.
    pub fn context(&mut self, context: &ProcessContext, frames: usize) -> bool {
        let seek = self.previous.is_some_and(|(pos, count, playing)| {
            playing && context.playing && context.sample_pos != pos.saturating_add(count as i64)
        });
        if seek {
            self.flush();
        }
        self.previous = Some((context.sample_pos, frames, context.playing));
        seek
    }
    pub fn stage_packet(&mut self, packet: &AudioPacket) -> bool {
        if packet.frames as usize > MAX_BLOCK || packet.output_channels as usize > MAX_CHANNELS {
            return false;
        }
        if packet.epoch == self.epoch {
            self.stage(
                packet.stream_start,
                packet.frames as usize,
                packet.output_channels as usize,
                &packet.outputs,
            );
        }
        true
    }
    pub fn stage(&mut self, start: u64, frames: usize, channels: usize, output: &[f32]) {
        for frame in 0..frames {
            let target = start
                .saturating_add(self.latency as u64)
                .saturating_add(frame as u64);
            if target < self.position || target >= self.position.saturating_add(CAPACITY as u64) {
                continue;
            }
            let index = target as usize % CAPACITY;
            for channel in 0..channels {
                let value = output[channel * frames + frame];
                self.audio[channel * CAPACITY + index] =
                    if value.is_finite() { value } else { 0.0 };
            }
            self.valid[index] = target.saturating_add(1);
        }
    }
    pub fn render(&mut self, audio: &mut AudioBuffer<'_>) -> u64 {
        let mut missing = 0;
        for frame in 0..audio.frames {
            let target = self.position + frame as u64;
            let index = target as usize % CAPACITY;
            if self.valid[index] == target + 1 {
                for channel in 0..audio.output_channels {
                    audio.outputs[channel * audio.frames + frame] =
                        self.audio[channel * CAPACITY + index];
                }
            } else if target >= self.warm_until {
                missing += 1;
            }
            self.valid[index] = 0;
        }
        self.position += audio.frames as u64;
        missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(t: &mut Timeline, frames: usize) -> (Vec<f32>, u64) {
        let mut output = vec![0.0; frames * 2];
        let mut buffer = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, frames);
        let missed = t.render(&mut buffer);
        (output, missed)
    }
    #[test]
    fn exact_delay_variable_blocks_and_wrap() {
        let mut t = Timeline::new();
        t.reset(128);
        let mut input_pos = 0;
        let mut output_pos = 0;
        for frames in [17, 47, 64, 31, 97].into_iter().cycle().take(500) {
            let samples: Vec<f32> = (0..2 * frames)
                .map(|i| (input_pos + i % frames + 1) as f32)
                .collect();
            t.stage(input_pos as u64, frames, 2, &samples);
            let (audio, missed) = render(&mut t, frames);
            assert_eq!(missed, 0);
            for frame in 0..frames {
                let expected = if output_pos + frame < 128 {
                    0.0
                } else {
                    (output_pos + frame - 128 + 1) as f32
                };
                assert_eq!(audio[frame], expected);
                assert_eq!(audio[frames + frame], expected);
            }
            input_pos += frames;
            output_pos += frames;
        }
    }
    #[test]
    fn preroll_crosses_zero_without_a_seek_but_real_seeks_still_flush() {
        let mut t = Timeline::new();
        t.reset(256);
        let mut context = ProcessContext {
            playing: true,
            ..Default::default()
        };
        for sample in [-512_i64, -256, 0, 256] {
            context.sample_pos = sample as _;
            assert!(!t.context(&context, 256), "Contiguous preroll at {sample}");
        }
        context.sample_pos = -256_i64 as _;
        assert!(
            t.context(&context, 256),
            "Backward seek across zero still flushes"
        );
        context.sample_pos = 4096;
        assert!(t.context(&context, 256), "Forward seek still flushes");
    }

    #[test]
    fn late_audio_is_never_replayed_and_seeks_flush() {
        let mut t = Timeline::new();
        t.reset(64);
        assert_eq!(render(&mut t, 64).1, 0);
        assert_eq!(render(&mut t, 64).1, 64);
        t.stage(0, 64, 2, &[1.0; 128]); // Already missed its output time.
        t.stage(64, 64, 2, &[2.0; 128]);
        assert!(render(&mut t, 64).0.iter().all(|v| *v == 2.0));
        let mut context = ProcessContext {
            playing: true,
            sample_pos: 64,
            ..Default::default()
        };
        assert!(!t.context(&context, 64));
        t.stage(128, 64, 2, &[3.0; 128]);
        context.sample_pos = 9999;
        assert!(t.context(&context, 64));
        assert!(render(&mut t, 64).0.iter().all(|v| *v == 0.0));
    }
}
