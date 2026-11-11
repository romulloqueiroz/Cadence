//! VAN51 playback and its 32-frame control clock. Serialized parameter order
//! comes from BParFX::SetObj (0x10040e4c8..0x10040e5a8).
use crate::amp_dsp::{van51_tables, TableBase};
use crate::van51_dsp::{Van51Controls, Van51State};
use crate::van51_process::Van51Process;
use nk_format::fx::AmpSettings;

fn controls(settings: AmpSettings) -> Van51Controls {
    let mut values = [0.5; 8];
    for (target, value) in values.iter_mut().zip(settings.values()) {
        *target = value.clamp(0.0, 1.0);
    }
    let flags = settings.flags();
    let flag = |index| flags.get(index).copied().unwrap_or(false);
    Van51Controls {
        values,
        lead: flag(0),
        hi_gain: flag(1),
        bright: flag(2),
        crunch: flag(3),
        mono: flag(4),
    }
}

pub(crate) struct Van51Amp {
    settings: AmpSettings,
    rate: f32,
    state: Van51State,
    process: Van51Process,
    tables: &'static [TableBase; 12],
    slice_remaining: usize,
}

impl Van51Amp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            rate: self.rate,
            state: self.state.duplicate_prepared(),
            process: self.process.duplicate_prepared(),
            tables: self.tables,
            slice_remaining: self.slice_remaining,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.rate = template.rate;
        self.state.copy_prepared_from(&template.state);
        self.process.copy_prepared_from(&template.process);
        self.tables = template.tables;
        self.slice_remaining = template.slice_remaining;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let mut amp = Self {
            settings,
            rate: 48_000.0,
            state: Van51State::new(),
            process: Van51Process::default(),
            tables: van51_tables(),
            slice_remaining: 0,
        };
        amp.prepare(48_000.0).expect("valid initial VAN51 rate");
        amp
    }

    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        self.rate = rate;
        self.state.initialize(rate);
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.state.clear();
        self.slice_remaining = 0;
    }

    /// Store targets without resetting the circuit. The control clock starts
    /// the fade when it next reaches a 32-host-frame boundary.
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        self.state.set_controls(&controls(settings));
        self.settings = settings;
    }

    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        if self.slice_remaining == 0 {
            self.state.on_control_rate();
            self.process.begin_slice(&self.state);
            self.slice_remaining = 32;
        }
        let output = self
            .process
            .process_frame(&mut self.state, self.tables, left, right);
        self.slice_remaining -= 1;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;

    pub(super) fn settings(values: [f32; 8], flags: [bool; 5]) -> AmpSettings {
        let mut settings = AmpSettings {
            kind: AmpKind::Van51,
            values: [0.0; 17],
            value_count: 8,
            flags: [false; 8],
            flag_count: 5,
            modes: [0; 4],
        };
        settings.values[..8].copy_from_slice(&values);
        settings.flags[..5].copy_from_slice(&flags);
        settings
    }

    #[test]
    fn van51_wire_order_preserves_all_eight_values_and_five_switches() {
        let values = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let native = controls(settings(values, [true, false, true, false, true]));
        assert_eq!(native.values, values);
        assert!(native.lead && native.bright && native.mono);
        assert!(!native.hi_gain && !native.crunch);
    }

    #[test]
    fn van51_prepare_rejects_invalid_rates() {
        let mut amp = Van51Amp::new(settings([0.5; 8], [false; 5]));
        for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(amp.prepare(rate).is_err());
        }
        assert_eq!(amp.rate, 48_000.0);
    }

    #[test]
    fn van51_rates_switches_and_extreme_controls_remain_finite_and_reset_exactly() {
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for control in [0.0, 0.5, 1.0] {
                for lead in [false, true] {
                    let mut amp =
                        Van51Amp::new(settings([control; 8], [lead, true, true, true, false]));
                    amp.prepare(rate).unwrap();
                    let input = |frame: usize| {
                        let x = (frame as f32 * 0.071).sin() * 0.2;
                        [x, -x * 0.3]
                    };
                    let first: Vec<_> = (0..1024)
                        .map(|frame| {
                            let [left, right] = input(frame);
                            let output = amp.process(left, right);
                            assert!(
                                output.iter().all(|v| v.is_finite() && v.abs() < 16.0),
                                "rate {rate} control {control} lead {lead}: {output:?}"
                            );
                            output
                        })
                        .collect();
                    amp.reset();
                    for (frame, expected) in first.into_iter().enumerate() {
                        let [left, right] = input(frame);
                        assert_eq!(amp.process(left, right), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn van51_mono_downmix_and_stereo_channels_are_independent() {
        let wire = settings([0.5; 8], [false; 5]);
        let mut mono_wire = wire;
        mono_wire.flags[4] = true;
        let mut mono = Van51Amp::new(mono_wire);
        let mut mono_silent = Van51Amp::new(mono_wire);
        let mut stereo = Van51Amp::new(wire);
        let mut stereo_silent = Van51Amp::new(wire);
        let mut difference = 0.0f32;
        for frame in 0..2048 {
            let x = (frame as f32 * 0.043).sin() * 0.2;
            let output = mono.process(x, -x);
            assert_eq!(output, mono_silent.process(0.0, 0.0));
            assert_eq!(output[0], output[1]);
            let baseline = stereo_silent.process(0.0, 0.0);
            let output = stereo.process(x, 0.0);
            assert_eq!(output[1], baseline[1]);
            difference = difference.max((output[0] - baseline[0]).abs());
        }
        assert!(difference > 1e-5, "input must reach the circuit");
    }

    #[test]
    fn van51_live_targets_fade_without_clearing_audio_history() {
        let mut amp = Van51Amp::new(settings([0.2; 8], [false; 5]));
        for _ in 0..64 {
            amp.process(0.1, -0.07);
        }
        let before: Vec<_> = (0x1d0..0x7f0)
            .step_by(4)
            .map(|i| amp.state.u32(i))
            .collect();
        amp.set_settings(settings([0.8; 8], [true, true, true, true, false]));
        let after: Vec<_> = (0x1d0..0x7f0)
            .step_by(4)
            .map(|i| amp.state.u32(i))
            .collect();
        assert_eq!(before, after, "setters must preserve audio state");
        for _ in 0..3072 {
            assert!(amp.process(0.1, -0.07).iter().all(|v| v.is_finite()));
        }
        for index in 0..28 {
            assert_eq!(
                amp.state.f32(0x94f0 + index * 4),
                amp.state.f32(0x95d0 + index * 4)
            );
            assert_eq!(amp.state.f32(0x9560 + index * 4), 0.0);
        }
        assert_eq!(amp.state.u8(0x9641), 0);
    }
}

#[cfg(test)]
mod native_probe_tests {
    use super::*;
    use std::io::Write;

    /// Explicit offline analysis utility. Reprocess a native dry capture to
    /// isolate the amp from sample playback, envelopes and instrument routing.
    /// No gain or timing compensation is applied to either input or output.
    /// An optional start frame leaves earlier output zero and defers circuit
    /// activation, to diagnose the native host's insert-silence policy.
    #[test]
    #[ignore = "requires CADENCE_VAN51_NATIVE_INPUT and CADENCE_VAN51_PROBE_OUTPUT"]
    fn van51_render_native_dry_capture() {
        let source = std::env::var("CADENCE_VAN51_NATIVE_INPUT").unwrap();
        let output = std::env::var("CADENCE_VAN51_PROBE_OUTPUT").unwrap();
        let start_frame: usize = std::env::var("CADENCE_VAN51_PROBE_START_FRAME")
            .unwrap_or_else(|_| "0".into())
            .parse()
            .unwrap();
        let mut values = [0.5; 8];
        if let Ok(params) = std::env::var("CADENCE_VAN51_PROBE_VALUES") {
            let params: Vec<f32> = params.split(',').map(|v| v.parse().unwrap()).collect();
            assert_eq!(params.len(), values.len());
            assert!(params
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            values.copy_from_slice(&params);
        }
        let mut flags = [false; 5];
        if let Ok(params) = std::env::var("CADENCE_VAN51_PROBE_FLAGS") {
            let params: Vec<bool> = params
                .split(',')
                .map(|v| match v.trim() {
                    "0" | "false" => false,
                    "1" | "true" => true,
                    _ => panic!("invalid VAN51 flag {v:?}"),
                })
                .collect();
            assert_eq!(params.len(), flags.len());
            flags.copy_from_slice(&params);
        }
        let decoded = nk_format::audio::load(&std::fs::read(&source).unwrap()).unwrap();
        assert_eq!(decoded.channels, 2);
        let mut amp = Van51Amp::new(super::tests::settings(values, flags));
        amp.prepare(decoded.sample_rate as f32).unwrap();
        // An observed native ProcessInt entry image isolates host lifecycle
        // choices from the reconstructed numerical circuit.
        if let Ok(path) = std::env::var("CADENCE_VAN51_PROBE_STATE") {
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(bytes.len(), crate::van51_dsp::VAN51_STATE_SIZE);
            for (at, value) in bytes.into_iter().enumerate() {
                amp.state.set_u8(at, value);
            }
            amp.slice_remaining = 0;
        }
        let size = u32::try_from(decoded.num_frames * 8).unwrap();
        let mut file = std::io::BufWriter::new(std::fs::File::create(output).unwrap());
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(size + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&3u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&decoded.sample_rate.to_le_bytes()).unwrap();
        file.write_all(&(decoded.sample_rate * 8).to_le_bytes())
            .unwrap();
        file.write_all(&8u16.to_le_bytes()).unwrap();
        file.write_all(&32u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&size.to_le_bytes()).unwrap();
        let mut peak = 0.0f32;
        for frame in 0..decoded.num_frames {
            let output = if frame < start_frame {
                [0.0; 2]
            } else {
                amp.process(decoded.samples[0][frame], decoded.samples[1][frame])
            };
            for sample in output {
                assert!(
                    sample.is_finite(),
                    "nonfinite VAN51 output at frame {frame}"
                );
                peak = peak.max(sample.abs());
                file.write_all(&sample.to_le_bytes()).unwrap();
            }
        }
        file.flush().unwrap();
        eprintln!(
            "VAN51_NATIVE_DRY_PROBE frames={} rate={} peak={} start_frame={start_frame} values={values:?} flags={flags:?}",
            decoded.num_frames, decoded.sample_rate, peak
        );
    }
}
