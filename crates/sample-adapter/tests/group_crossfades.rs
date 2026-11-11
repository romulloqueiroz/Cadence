//! Audio-level regressions for the recovered Kontakt key/velocity fade law.
//! All presets and PCM are generated here, then loaded through the public API.

use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use sample_adapter::{SamplerPlugin, StreamingConfig};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static SERIAL: AtomicUsize = AtomicUsize::new(0);
const FRAMES: u32 = 80_000;
const LEFT: f32 = 0.25;
const RIGHT: f32 = -0.125;

struct Fixture {
    root: PathBuf,
    instrument: PathBuf,
}

impl Fixture {
    fn new(program: &str, groups: &str, zones: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cadence-crossfade-audio-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let xml = format!(
            r#"<K2_Container><Programs><K2_Program name="Crossfade audio">
            <Parameters>{program}</Parameters><Groups>{groups}</Groups>
            <Zones>{zones}</Zones></K2_Program></Programs></K2_Container>"#
        );
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let mut bytes = vec![0; 0xaa];
        bytes[..4].copy_from_slice(&nk_format::PLAIN_MAGIC.to_le_bytes());
        bytes[8..10].copy_from_slice(&0x100u16.to_le_bytes());
        bytes.extend(encoder.finish().unwrap());
        let instrument = root.join("crossfades.nki");
        fs::write(&instrument, bytes).unwrap();

        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + FRAMES * 4).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&192_000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(FRAMES * 4).to_le_bytes());
        for frame in 0..FRAMES {
            // A distinct prefix catches implementations that ignore sampleStart.
            let left = if frame < 32 { 16_384i16 } else { 8_192i16 };
            let right = if frame < 32 { -8_192i16 } else { -4_096i16 };
            wav.extend_from_slice(&left.to_le_bytes());
            wav.extend_from_slice(&right.to_le_bytes());
        }
        fs::write(root.join("level.wav"), wav).unwrap();
        Self { root, instrument }
    }

    fn load(&self, streaming: bool) -> SamplerPlugin {
        let mut plugin = SamplerPlugin::load(&self.instrument).unwrap();
        if streaming {
            plugin.stream_all(StreamingConfig::default()).unwrap();
            plugin.set_offline_rendering(true);
        } else {
            assert_eq!(plugin.preload_all(), 1);
        }
        plugin
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn group(index: u32, release: bool) -> String {
    format!(
        r#"<K2_Group index="{index}" name="Layer {index}"><Parameters>
        <V name="keyTracking" value="no"/><V name="releaseTrigger" value="{release}"/>
        </Parameters><GroupStart/>
        <IntModulators><K2_IntMod><Targets><Target>
        <V name="target" value="volume"/><V name="intensity" value="1"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="0"/><V name="decay" value="0"/>
        <V name="sustain" value="1"/><V name="release" value="2"/>
        </Envelope></K2_IntMod></IntModulators></K2_Group>"#
    )
}

fn zone(group: u32, ranges: [u8; 4], fades: [u8; 4]) -> String {
    let [low_key, high_key, low_velocity, high_velocity] = ranges;
    let [fade_low_key, fade_high_key, fade_low_velocity, fade_high_velocity] = fades;
    format!(
        r#"<K2_Zone groupIdx="{group}"><Parameters>
        <V name="sampleStart" value="32"/><V name="sampleEnd" value="79999"/>
        <V name="lowKey" value="{low_key}"/><V name="highKey" value="{high_key}"/>
        <V name="lowVelocity" value="{low_velocity}"/><V name="highVelocity" value="{high_velocity}"/>
        <V name="fadeLowKey" value="{fade_low_key}"/><V name="fadeHighKey" value="{fade_high_key}"/>
        <V name="fadeLowVelo" value="{fade_low_velocity}"/><V name="fadeHighVelo" value="{fade_high_velocity}"/>
        <V name="rootKey" value="60"/><V name="zoneVolume" value="1"/>
        </Parameters><Sample><V name="file_ex2" value="level.wav"/></Sample></K2_Zone>"#
    )
}

fn render(
    plugin: &mut SamplerPlugin,
    frames: usize,
    block: usize,
    events: &[(usize, Event)],
) -> [Vec<f32>; 2] {
    let mut context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: block,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut result = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    for position in (0..frames).step_by(block) {
        let count = block.min(frames - position);
        context.sample_pos = position as i64;
        context.block_size = count;
        let block_events: Vec<_> = events
            .iter()
            .filter(|(frame, _)| *frame >= position && *frame < position + count)
            .map(|(frame, event)| {
                let mut event = *event;
                event.frame = frame - position;
                event
            })
            .collect();
        let mut output = vec![0.0; count * 2];
        let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, count);
        plugin.process(&context, &mut audio, &block_events);
        assert!(plugin.processing_error().is_none());
        result[0].extend_from_slice(&output[..count]);
        result[1].extend_from_slice(&output[count..]);
    }
    result
}

fn assert_level(audio: &[Vec<f32>; 2], start: usize, end: usize, gain: f32) {
    for (channel, source) in [LEFT, RIGHT].into_iter().enumerate() {
        for (frame, value) in audio[channel].iter().enumerate() {
            let expected = if (start..end).contains(&frame) {
                source * gain
            } else {
                0.0
            };
            assert!(
                (*value - expected).abs() < 1e-6,
                "channel={channel} frame={frame}: {value} != {expected}"
            );
        }
    }
}

#[test]
fn audio_has_nonzero_faded_endpoints_and_exact_note_and_sample_offsets() {
    let fixture = Fixture::new(
        "",
        &group(9, false),
        &zone(9, [60, 80, 40, 100], [3, 3, 4, 4]),
    );
    for streaming in [false, true] {
        for (key, velocity, gain) in [
            (60, 40, 1.0 / 20.0),
            (61, 41, 1.0 / 5.0),
            (63, 44, 1.0),
            (78, 98, 9.0 / 20.0),
            (80, 100, 1.0 / 20.0),
            (59, 40, 0.0),
            (80, 101, 0.0),
        ] {
            let mut plugin = fixture.load(streaming);
            let audio = render(&mut plugin, 23, 8, &[(5, Event::note_on(0, key, velocity))]);
            assert_level(&audio, 5, 23, gain);
        }
    }
}

#[test]
fn audio_multiplies_all_four_fade_factors_instead_of_taking_minimum_or_equal_power() {
    let fixture = Fixture::new(
        "",
        &group(2, false),
        &zone(2, [60, 62, 20, 22], [4, 4, 4, 4]),
    );
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let audio = render(&mut plugin, 32, 7, &[(0, Event::note_on(0, 61, 21))]);
        // Each of four edge factors is 2/5, so the actual PCM amplitude is
        // reduced by 16/625, not by 2/5 or an equal-power curve.
        assert_level(&audio, 0, 32, 16.0 / 625.0);
    }
}

#[test]
fn overlapping_zone_pair_sums_to_unity_amplitude_for_key_and_velocity_fades() {
    for key_fades in [true, false] {
        let zones = if key_fades {
            zone(7, [0, 67, 0, 127], [0, 8, 0, 0]) + &zone(7, [60, 127, 0, 127], [8, 0, 0, 0])
        } else {
            zone(7, [0, 127, 0, 67], [0, 0, 0, 8]) + &zone(7, [0, 127, 60, 127], [0, 0, 8, 0])
        };
        let fixture = Fixture::new("", &group(7, false), &zones);
        for streaming in [false, true] {
            let mut plugin = fixture.load(streaming);
            for value in 59..=68 {
                plugin.reset();
                let key = if key_fades { value } else { 64 };
                let velocity = if key_fades { 100 } else { value };
                let audio = render(&mut plugin, 16, 5, &[(0, Event::note_on(0, key, velocity))]);
                assert_level(&audio, 0, 16, 1.0);
            }
        }
    }
}

#[test]
fn program_bounds_gate_original_zone_fades_without_moving_their_origins() {
    let program = r#"<V name="transpose" value="12"/><V name="lowKey" value="65"/>
        <V name="highKey" value="67"/><V name="lowVelocity" value="50"/><V name="highVelocity" value="90"/>"#;
    let fixture = Fixture::new(
        program,
        &group(8, false),
        &zone(8, [60, 80, 40, 100], [10, 0, 20, 0]),
    );
    for streaming in [false, true] {
        for (key, velocity, gain) in [
            (53, 55, 96.0 / 231.0), // mapped key 65 =>6/11; velocity55=>16/21
            (54, 55, 112.0 / 231.0),
            (55, 55, 128.0 / 231.0),
            (52, 55, 0.0),
            (56, 55, 0.0),
            (53, 49, 0.0),
            (53, 91, 0.0),
        ] {
            let mut plugin = fixture.load(streaming);
            let audio = render(&mut plugin, 16, 6, &[(0, Event::note_on(0, key, velocity))]);
            assert_level(&audio, 0, 16, gain);
        }
    }
}

#[test]
fn release_zone_fades_use_held_attack_velocity_and_do_not_duplicate_on_unmatched_off() {
    let fixture = Fixture::new(
        "",
        &group(11, true),
        &zone(11, [60, 80, 40, 100], [3, 0, 4, 0]),
    );
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let mut note_off = Event::note_off(0, 61);
        note_off.data2 = 127;
        note_off.value = 1.0;
        let audio = render(
            &mut plugin,
            47,
            9,
            &[
                (2, Event::note_off(0, 61)),
                (7, Event::note_on(0, 61, 40)),
                (17, note_off),
                (23, Event::note_off(0, 61)),
            ],
        );
        // Key61=>2/4 and held attack velocity40=>1/5. The release event's
        // deliberately different velocity must not silence or replace it.
        assert_level(&audio, 17, 47, 1.0 / 10.0);
    }
}

#[test]
fn crossfaded_audio_and_release_are_block_size_invariant_across_streamed_pages() {
    let zones = zone(3, [0, 67, 0, 127], [0, 8, 0, 0]) + &zone(3, [60, 127, 0, 127], [8, 0, 0, 0]);
    let fixture = Fixture::new("", &group(3, false), &zones);
    let events = [
        (7, Event::note_on(0, 64, 64)),
        (35_017, Event::note_off(0, 64)),
    ];
    let mut reference = None;
    for streaming in [false, true] {
        for block in [31, 64, 257] {
            let mut plugin = fixture.load(streaming);
            let audio = render(&mut plugin, 35_200, block, &events);
            for (channel, source) in [LEFT, RIGHT].into_iter().enumerate() {
                for (frame, actual) in audio[channel].iter().enumerate() {
                    let gain = if frame < 7 {
                        0.0
                    } else if frame < 35_017 {
                        1.0
                    } else if frame < 35_113 {
                        1.0 - (frame - 35_017) as f32 / 96.0
                    } else {
                        0.0
                    };
                    assert!(
                        (*actual - source * gain).abs() < 2e-6,
                        "block={block} streamed={streaming} frame={frame}"
                    );
                }
            }
            if let Some(expected) = &reference {
                assert_eq!(&audio, expected);
            } else {
                reference = Some(audio);
            }
            if streaming {
                let stats = plugin.streaming_stats();
                assert!(stats.pages_read > 0, "test did not cross resident attacks");
                assert_eq!(stats.underrun_frames, 0);
            }
        }
    }
}
