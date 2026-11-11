//! Serialized EPPreamps controls, the native 32-frame clock and rate converters.
use crate::amp_resampler::AmpResampler;
use crate::epiano_dsp::EPianoCore;
use nk_format::fx::AmpSettings;

fn parameters(settings: AmpSettings) -> [f32; 23] {
    let mut p = [
        0., 0., 0., 0.5, 1., 0.5, 0.5, 1., 1., 1., 1., 0., 1., 0.5, 0., 0., 0., 0., 0., 0., 0.,
        0.5, 0.,
    ];
    let value = |i: usize| {
        settings
            .values()
            .get(i)
            .copied()
            .unwrap_or(0.5)
            .clamp(0., 1.)
    };
    for (source, target) in [
        (0, 1),
        (1, 3),
        (2, 5),
        (3, 6),
        (4, 4),
        (5, 13),
        (6, 20),
        (7, 12),
    ] {
        p[target] = value(source);
    }
    // BNoteValue::GetFreqHz applies the instrument's note division before the
    // core's normalized speed mapping. The sampler's existing tempo is 120 BPM.
    let beats = settings.values().get(8).copied().unwrap_or(0.);
    if beats > 1e-9 {
        let hz = (2.0 / (beats * value(5).max(1.0))).clamp(0.01, 20.0);
        p[13] = (hz - 0.01).sqrt() / 4.471018;
    }
    p[0] = settings.modes[0] as f32 * 0.25;
    p[2] = settings.modes[1] as f32 / 5.;
    p[11] = settings.modes[2] as f32 / 6.;
    p[18] = settings.modes[3] as f32 * 0.25;
    for index in 0..4 {
        p[7 + index] = u8::from(settings.flags().get(2 + index).copied().unwrap_or(false)) as f32;
    }
    p[22] = u8::from(settings.flags().get(1).copied().unwrap_or(false)) as f32;
    p
}

pub(crate) struct EPianoAmp {
    settings: AmpSettings,
    ratio: f32,
    core: EPianoCore,
    input_src: AmpResampler,
    output_src: AmpResampler,
    up: [[f32; 2]; 512],
    down: [[f32; 2]; 512],
    group: [[f32; 2]; 16],
    group_len: usize,
    queue: [[f32; 2]; 32],
    queue_head: usize,
    queue_len: usize,
    remaining: usize,
}
impl EPianoAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            ratio: self.ratio,
            core: self.core.duplicate_prepared(),
            input_src: self.input_src.duplicate_prepared(),
            output_src: self.output_src.duplicate_prepared(),
            up: self.up,
            down: self.down,
            group: self.group,
            group_len: self.group_len,
            queue: self.queue,
            queue_head: self.queue_head,
            queue_len: self.queue_len,
            remaining: self.remaining,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.ratio = template.ratio;
        self.core.copy_prepared_from(&template.core);
        self.input_src.copy_prepared_from(&template.input_src);
        self.output_src.copy_prepared_from(&template.output_src);
        self.up = template.up;
        self.down = template.down;
        self.group = template.group;
        self.group_len = template.group_len;
        self.queue = template.queue;
        self.queue_head = template.queue_head;
        self.queue_len = template.queue_len;
        self.remaining = template.remaining;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        Self::prepared(settings, 48_000.).expect("valid EPPreamps initial rate")
    }
    fn prepared(settings: AmpSettings, rate: f32) -> Result<Self, &'static str> {
        if !rate.is_finite() || rate <= 0. {
            return Err("invalid amp sample rate");
        }
        let ratio = 2f64.powf((88_200. / f64::from(rate)).log2().round()) as f32;
        let input_src = AmpResampler::new(ratio)?;
        let output_src = AmpResampler::new(1. / ratio)?;
        let core = EPianoCore::new(
            (rate * ratio).round(),
            ratio,
            (ratio * 32.).round() as usize,
            parameters(settings),
        );
        Ok(Self {
            settings,
            ratio,
            core,
            input_src,
            output_src,
            up: [[0.; 2]; 512],
            down: [[0.; 2]; 512],
            group: [[0.; 2]; 16],
            group_len: 0,
            queue: [[0.; 2]; 32],
            queue_head: 0,
            queue_len: 0,
            remaining: 0,
        })
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        *self = Self::prepared(self.settings, rate)?;
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        self.core.restart(parameters(self.settings));
        self.input_src.reset();
        self.output_src.reset();
        self.group_len = 0;
        self.queue_head = 0;
        self.queue_len = 0;
        self.remaining = 0;
    }
    // Exercised by live-control integration tests; host automation dispatch
    // does not yet expose individual amp parameters.
    #[allow(dead_code)]
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        self.core.set_parameters(parameters(settings));
        self.settings = settings;
    }
    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        if self.remaining == 0 {
            self.core.control();
            self.remaining = 32;
        }
        let count = if self.ratio < 1. {
            self.group[self.group_len] = [left, right];
            self.group_len += 1;
            let required = (1. / self.ratio) as usize;
            if self.group_len == required {
                self.group_len = 0;
                self.input_src
                    .process_frames(&self.group[..required], &mut self.up)
            } else {
                0
            }
        } else {
            self.input_src
                .process_frames(&[[left, right]], &mut self.up)
        };
        for value in &mut self.up[..count] {
            *value = self.core.process(*value);
        }
        for chunk in self.up[..count].chunks(32) {
            let produced = self.output_src.process_frames(chunk, &mut self.down);
            for value in &self.down[..produced] {
                self.queue[(self.queue_head + self.queue_len) % self.queue.len()] = *value;
                self.queue_len += 1;
            }
        }
        self.remaining -= 1;
        if self.queue_len == 0 {
            return [0.; 2];
        }
        let value = self.queue[self.queue_head];
        self.queue_head = (self.queue_head + 1) % self.queue.len();
        self.queue_len -= 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;
    fn settings() -> AmpSettings {
        AmpSettings {
            kind: AmpKind::EpPreamps,
            values: [0.; 17],
            value_count: 11,
            flags: [false; 8],
            flag_count: 6,
            modes: [0; 4],
        }
    }
    #[test]
    fn epiano_serialized_controls_preserve_native_parameter_mapping() {
        let mut s = settings();
        s.values[..8].copy_from_slice(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);
        s.flags[..6].copy_from_slice(&[false, true, true, false, true, false]);
        s.modes = [4, 5, 6, 4];
        let p = parameters(s);
        for (i, v) in [
            (1, 0.1),
            (3, 0.2),
            (5, 0.3),
            (6, 0.4),
            (4, 0.5),
            (13, 0.6),
            (20, 0.7),
            (12, 0.8),
        ] {
            assert_eq!(p[i], v);
        }
        for i in [0, 2, 11, 18, 22, 7, 9] {
            assert_eq!(p[i], 1.);
        }
        assert_eq!(p[8], 0.);
        assert_eq!(p[10], 0.);
        s.values[8] = 0.5;
        assert!((parameters(s)[13] - (3.99f32).sqrt() / 4.471018).abs() < 1e-7);
    }

    #[test]
    fn epiano_wire_mapping_matches_original_setter_dispatch() {
        let bytes = include_bytes!("../tests/fixtures/epiano-wire-native.bin");
        assert_eq!(&bytes[..8], b"EPWIRE01");
        let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let mut at = 12;
        for case in 0..count {
            let mut settings = settings();
            for value in &mut settings.values[..11] {
                *value = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                at += 4;
            }
            for flag in &mut settings.flags[..6] {
                *flag = bytes[at] != 0;
                at += 1;
            }
            for mode in &mut settings.modes {
                *mode = i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                at += 4;
            }
            for (index, actual) in parameters(settings).into_iter().enumerate() {
                let expected = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                at += 4;
                assert!(
                    (actual - expected).abs() <= 1e-7 * (1. + expected.abs()),
                    "case {case} parameter {index}: {actual} != {expected}"
                );
            }
        }
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn epiano_playback_matches_original_core_and_rate_wrapper() {
        let bytes = include_bytes!("../tests/fixtures/epiano-wrapper-native.bin");
        assert_eq!(&bytes[..8], b"EPWRAP01");
        let word = |at: &mut usize| {
            let value = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
            *at += 4;
            value
        };
        let mut at = 8;
        let cases = word(&mut at) as usize;
        let frames = word(&mut at) as usize;
        let mut maximum_error = 0f32;
        for case in 0..cases {
            let rate = f32::from_bits(word(&mut at));
            let mut settings = settings();
            for value in &mut settings.values[..11] {
                *value = f32::from_bits(word(&mut at));
            }
            for flag in &mut settings.flags[..6] {
                *flag = bytes[at] != 0;
                at += 1;
            }
            for mode in &mut settings.modes {
                *mode = word(&mut at) as i32;
            }
            let mut amp = EPianoAmp::new(settings);
            amp.prepare(rate).unwrap();
            // The per-frame API must collect a complete reciprocal input
            // group above the internal rate. Native processes 32-frame blocks.
            let delay = if amp.ratio < 1. {
                (1. / amp.ratio) as usize - 1
            } else {
                0
            };
            let mut expected = Vec::with_capacity(frames);
            for frame in 0..frames {
                let input = [f32::from_bits(word(&mut at)), f32::from_bits(word(&mut at))];
                expected.push([f32::from_bits(word(&mut at)), f32::from_bits(word(&mut at))]);
                let actual = amp.process(input[0], input[1]);
                let target = if frame < delay {
                    [0.; 2]
                } else {
                    expected[frame - delay]
                };
                for channel in 0..2 {
                    let error = (actual[channel] - target[channel]).abs();
                    maximum_error = maximum_error.max(error);
                    assert!(actual[channel].is_finite() && error <= 2e-5 * (1. + target[channel].abs()),
                        "EP wrapper case {case} rate {rate} frame {frame} channel {channel}: {} != {}", actual[channel], target[channel]);
                }
            }
        }
        assert_eq!(at, bytes.len());
        eprintln!(
            "EP_WRAPPER_NATIVE cases={cases} frames={} max_error={maximum_error}",
            cases * frames
        );
    }
}
