//! Independent XML/PCM fixtures exercising the production loader and sampler.
//! Expected levels come from the recovered route laws; expected frequencies
//! come from octave ratios, without inspecting the sampler's internal state.

use plugin_api::{AudioBuffer, Event, EventKind, Plugin, ProcessContext};
use sample_adapter::{SamplerPlugin, StreamingConfig};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const SAMPLE_FRAMES: u32 = 160_000;
const LEVEL: f32 = 0.25;
static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    directory: PathBuf,
    instrument: PathBuf,
}

impl Fixture {
    fn new(sine: bool, routes: &str) -> Self {
        Self::new_full(sine, routes, "", "")
    }

    fn new_full(sine: bool, routes: &str, internal: &str, params: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "cadence-external-mod-audio-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let xml = format!(
            r#"<K2_Container><Programs><K2_Program name="External MIDI fixture">
            <Groups><K2_Group index="0" name="Main"><Parameters>
              <V name="keyTracking" value="no"/>{params}
            </Parameters><ExtModulators>{routes}</ExtModulators>
            <IntModulators>{internal}</IntModulators></K2_Group></Groups>
            <Zones><K2_Zone groupIdx="0"><Parameters>
              <V name="sampleStart" value="0"/>
              <V name="sampleEnd" value="159999"/>
              <V name="rootKey" value="60"/>
            </Parameters><Sample><V name="file_ex2" value="source.wav"/></Sample>
            </K2_Zone></Zones></K2_Program></Programs></K2_Container>"#
        );
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let mut bytes = vec![0; 0xaa];
        bytes[..4].copy_from_slice(&nk_format::PLAIN_MAGIC.to_le_bytes());
        bytes[8..10].copy_from_slice(&0x100u16.to_le_bytes());
        bytes.extend(encoder.finish().unwrap());
        let instrument = directory.join("expression.nki");
        fs::write(&instrument, bytes).unwrap();

        let mut wav = Vec::with_capacity(44 + SAMPLE_FRAMES as usize * 4);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + SAMPLE_FRAMES * 4).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&192_000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(SAMPLE_FRAMES * 4).to_le_bytes());
        for frame in 0..SAMPLE_FRAMES {
            let value = if sine {
                // A 200 Hz reference at 48 kHz, without relying on runtime DSP.
                ((std::f64::consts::TAU * frame as f64 / 240.0).sin() * 8192.0).round() as i16
            } else {
                8192
            };
            wav.extend_from_slice(&value.to_le_bytes());
            wav.extend_from_slice(&value.to_le_bytes());
        }
        fs::write(directory.join("source.wav"), wav).unwrap();
        Self {
            directory,
            instrument,
        }
    }

    fn load(&self, streamed: bool) -> SamplerPlugin {
        let mut plugin = SamplerPlugin::load(&self.instrument).unwrap();
        if streamed {
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
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn route(
    source: &str,
    controller: u8,
    initial: i16,
    target: &str,
    intensity: f32,
    flags: u8,
    bypass: bool,
) -> String {
    format!(
        r#"<K2_ExtMod>
          <V name="type" value="extMod"/><V name="source" value="{source}"/>
          <V name="ccNumber" value="{controller}"/>
          <V name="initialValue" value="{initial}"/>
          <V name="bypass" value="{bypass}"/>
          <V name="simulateK2EnvTimeModulation" value="yes"/>
          <Targets><Target>
            <V name="target" value="{target}"/>
            <V name="intensity" value="{intensity}"/>
            <V name="flags" value="{flags}"/>
            <V name="smoothingCoef" value="0"/>
          </Target></Targets>
        </K2_ExtMod>"#
    )
}

fn channel(mut event: Event, channel: u8) -> Event {
    event.channel = channel;
    event
}

fn pressure(kind: EventKind, channel: u8, note: u8, value: f32) -> Event {
    Event {
        frame: 0,
        kind,
        channel,
        data1: note as u16,
        data2: (value * 127.0).round() as u16,
        value,
    }
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
        assert!(output.iter().all(|value| value.is_finite()));
        result[0].extend_from_slice(&output[..count]);
        result[1].extend_from_slice(&output[count..]);
    }
    result
}

fn assert_level(audio: &[Vec<f32>; 2], frame: usize, left: f32, right: f32) {
    for (channel, gain) in [left, right].into_iter().enumerate() {
        assert!(
            (audio[channel][frame] - LEVEL * gain).abs() < 2e-6,
            "channel {channel}, frame {frame}: actual {}, expected {}",
            audio[channel][frame],
            LEVEL * gain
        );
    }
}

fn frequency(samples: &[f32]) -> f64 {
    let crossings: Vec<_> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .map(|(index, pair)| index as f64 - pair[0] as f64 / (pair[1] - pair[0]) as f64)
        .collect();
    assert!(crossings.len() > 10);
    48_000.0 * (crossings.len() - 1) as f64
        / (crossings.last().unwrap() - crossings.first().unwrap())
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|value| value * value).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn instrument_bend_depth_controls_before_and_during_note_pitch() {
    for depth in [1.0, 1.0 / 6.0] {
        let fixture = Fixture::new(true, &route("pitchBend", 0, -1, "pitch", depth, 20, false));
        let events = [
            (0, Event::pitch_bend(0, 1.0)),
            (17, Event::note_on(0, 60, 100)),
            (12_000, Event::pitch_bend(0, 0.0)),
            (24_000, Event::pitch_bend(0, -1.0)),
        ];
        for streamed in [false, true] {
            let audio = render(&mut fixture.load(streamed), 36_000, 257, &events);
            for (start, expected) in [
                (200, 200.0 * 2.0f64.powf(depth as f64)),
                (12_200, 200.0),
                (24_200, 200.0 * 2.0f64.powf(-(depth as f64))),
            ] {
                let measured = frequency(&audio[0][start..start + 10_000]);
                assert!(
                    (measured - expected).abs() < 0.03,
                    "{measured} != {expected}"
                );
            }
        }
    }
}

#[test]
fn pitch_bend_on_another_channel_does_not_retune_a_voice() {
    let fixture = Fixture::new(true, &route("pitchBend", 0, -1, "pitch", 1.0, 20, false));
    let events = [
        (0, channel(Event::note_on(0, 60, 100), 1)),
        (100, Event::pitch_bend(0, 1.0)),
        (12_000, channel(Event::pitch_bend(0, 1.0), 1)),
    ];
    for streamed in [false, true] {
        let audio = render(&mut fixture.load(streamed), 24_000, 73, &events);
        assert!((frequency(&audio[0][200..11_000]) - 200.0).abs() < 0.03);
        assert!((frequency(&audio[0][12_200..23_000]) - 400.0).abs() < 0.03);
    }
}

#[test]
fn untouched_cc_is_neutral_then_changes_live_and_new_voices() {
    let fixture = Fixture::new(false, &route("midiCC", 11, -1, "volume", 1.0, 20, false));
    let events = [
        (0, Event::note_on(0, 60, 100)),
        (64, Event::control_change(0, 11, 64)),
        (128, Event::note_on(0, 61, 100)),
        (192, Event::control_change(0, 11, 32)),
        (256, Event::control_change(0, 11, 0)),
        (320, Event::control_change(0, 11, 127)),
    ];
    for streamed in [false, true] {
        let audio = render(&mut fixture.load(streamed), 400, 29, &events);
        for (frame, gain) in [
            (1, 1.0),
            (65, 64.0 / 127.0),
            (129, 128.0 / 127.0),
            (193, 64.0 / 127.0),
            (257, 0.0),
            (321, 2.0),
        ] {
            assert_level(&audio, frame, gain, gain);
        }
    }
}

#[test]
fn cc_explicit_fallback_and_prior_controller_state_are_distinct() {
    let fixture = Fixture::new(false, &route("midiCC", 11, 64, "volume", 1.0, 20, false));
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            128,
            17,
            &[(0, Event::note_on(0, 60, 100))],
        );
        assert_level(&audio, 1, 64.0 / 127.0, 64.0 / 127.0);
        let audio = render(
            &mut fixture.load(streamed),
            128,
            17,
            &[
                (0, Event::control_change(0, 11, 32)),
                (10, Event::note_on(0, 60, 100)),
            ],
        );
        assert_level(&audio, 11, 32.0 / 127.0, 32.0 / 127.0);
    }
}

#[test]
fn cc_modulation_is_isolated_by_channel_even_for_equal_keys() {
    let fixture = Fixture::new(false, &route("midiCC", 11, -1, "volume", 1.0, 20, false));
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            128,
            17,
            &[
                (0, Event::note_on(0, 60, 100)),
                (1, channel(Event::note_on(0, 60, 100), 1)),
                (20, Event::control_change(0, 11, 0)),
                (40, channel(Event::control_change(0, 11, 64), 1)),
            ],
        );
        assert_level(&audio, 5, 2.0, 2.0);
        assert_level(&audio, 21, 1.0, 1.0);
        assert_level(&audio, 41, 64.0 / 127.0, 64.0 / 127.0);
    }
}

#[test]
fn channel_pressure_before_attack_and_live_changes_drive_pan() {
    let fixture = Fixture::new(
        false,
        &route("monoAfterTouch", 0, -1, "pan", 1.0, 20, false),
    );
    for streamed in [false, true] {
        let mut plugin = fixture.load(streamed);
        let audio = render(
            &mut plugin,
            200,
            31,
            &[
                (0, pressure(EventKind::ChannelPressure, 1, 0, 0.5)),
                (10, channel(Event::note_on(0, 60, 100), 1)),
                (40, pressure(EventKind::ChannelPressure, 0, 0, 1.0)),
                (80, pressure(EventKind::ChannelPressure, 1, 0, 1.0)),
            ],
        );
        assert_level(&audio, 11, 0.5, 1.0);
        assert_level(&audio, 41, 0.5, 1.0);
        assert_level(&audio, 81, 0.0, 1.0);
        plugin.reset();
        let audio = render(
            &mut plugin,
            64,
            31,
            &[(0, channel(Event::note_on(0, 60, 100), 1))],
        );
        assert_level(&audio, 1, 1.0, 1.0);
    }
}

#[test]
fn legacy_poly_pressure_broadcast_requires_a_live_matching_channel_and_note() {
    let fixture = Fixture::new(
        false,
        &route("polyAfterTouch", 0, -1, "pan", 1.0, 20, false),
    );
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            400,
            23,
            &[
                (0, pressure(EventKind::PolyPressure, 0, 60, 0.75)),
                (20, Event::note_on(0, 60, 100)),
                (30, Event::note_on(0, 61, 100)),
                (40, channel(Event::note_on(0, 60, 100), 1)),
                (100, pressure(EventKind::PolyPressure, 0, 60, 1.0)),
                (200, pressure(EventKind::PolyPressure, 1, 61, 1.0)),
                (300, pressure(EventKind::PolyPressure, 0, 61, 0.5)),
            ],
        );
        assert_level(&audio, 21, 1.0, 1.0);
        assert_level(&audio, 41, 3.0, 3.0);
        // XML's legacy flag defaults true: one live matching onset seeds a
        // broadcast to the group's other notes and channels.
        assert_level(&audio, 101, 0.0, 3.0);
        // Channel 1 has note 60 but no note 61, so this cannot seed a broadcast.
        assert_level(&audio, 201, 0.0, 3.0);
        assert_level(&audio, 301, 1.5, 3.0);
    }
}

#[test]
fn legacy_poly_pressure_broadcast_stays_within_the_matched_group() {
    use std::io::Read;

    let fixture = Fixture::new(
        false,
        &route("polyAfterTouch", 0, -1, "pan", 1.0, 20, false),
    );
    let original = fs::read(&fixture.instrument).unwrap();
    let mut xml = String::new();
    flate2::read::ZlibDecoder::new(&original[0xaa..])
        .read_to_string(&mut xml)
        .unwrap();
    xml = xml
        .replace(
            "</Groups>",
            r#"<K2_Group index="1" name="Unmodulated"><Parameters>
            <V name="keyTracking" value="no"/>
            </Parameters></K2_Group></Groups>"#,
        )
        .replace(
            r#"<V name="rootKey" value="60"/>"#,
            r#"<V name="rootKey" value="60"/>
            <V name="lowKey" value="60"/><V name="highKey" value="60"/>"#,
        )
        .replace(
            "</Zones>",
            r#"<K2_Zone groupIdx="1"><Parameters>
            <V name="lowKey" value="61"/><V name="highKey" value="61"/>
            <V name="sampleEnd" value="159999"/>
            </Parameters><Sample><V name="file_ex2" value="source.wav"/></Sample>
            </K2_Zone></Zones>"#,
        );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let mut bytes = original[..0xaa].to_vec();
    bytes.extend(encoder.finish().unwrap());
    fs::write(&fixture.instrument, bytes).unwrap();

    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            400,
            23,
            &[
                (0, Event::note_on(0, 60, 100)),
                (1, channel(Event::note_on(0, 60, 100), 1)),
                (2, Event::note_on(0, 61, 100)),
                (100, pressure(EventKind::PolyPressure, 0, 60, 1.0)),
                (200, pressure(EventKind::PolyPressure, 2, 60, 0.0)),
                (300, pressure(EventKind::PolyPressure, 1, 60, 0.5)),
            ],
        );
        assert_level(&audio, 3, 3.0, 3.0);
        assert_level(&audio, 101, 1.0, 3.0);
        assert_level(&audio, 201, 1.0, 3.0);
        assert_level(&audio, 301, 2.0, 3.0);
    }
}

#[test]
fn bypass_and_inversion_follow_instrument_routes() {
    for streamed in [false, true] {
        let fixture = Fixture::new(false, &route("midiCC", 11, 0, "volume", 1.0, 20, true));
        let audio = render(
            &mut fixture.load(streamed),
            100,
            19,
            &[
                (0, Event::note_on(0, 60, 100)),
                (20, Event::control_change(0, 11, 0)),
            ],
        );
        assert_level(&audio, 21, 1.0, 1.0);

        let fixture = Fixture::new(false, &route("midiCC", 11, 0, "volume", 1.0, 22, false));
        let audio = render(
            &mut fixture.load(streamed),
            100,
            19,
            &[
                (0, Event::control_change(0, 11, 32)),
                (1, Event::note_on(0, 60, 100)),
            ],
        );
        assert_level(&audio, 2, 95.0 / 127.0, 95.0 / 127.0);

        let fixture = Fixture::new(
            false,
            &route("monoAfterTouch", 0, -1, "pan", 1.0, 22, false),
        );
        let audio = render(
            &mut fixture.load(streamed),
            100,
            19,
            &[
                (0, pressure(EventKind::ChannelPressure, 0, 0, 0.5)),
                (1, Event::note_on(0, 60, 100)),
            ],
        );
        assert_level(&audio, 2, 1.0, 0.5);
    }
}

#[test]
fn attack_velocity_volume_route_cubes_only_its_own_attenuation() {
    let routes = route("velocity", 0, -1, "volume", 1.0, 20, false)
        + &route("midiCC", 11, 64, "volume", 1.0, 20, false);
    let fixture = Fixture::new(false, &routes);
    let expected = (64.0f32 / 127.0).powi(3) * (64.0 / 127.0);
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            100,
            19,
            &[(0, Event::note_on(0, 60, 64))],
        );
        assert_level(&audio, 1, expected, expected);
    }
}

#[test]
fn expression_audio_is_exact_across_blocks_resident_and_streamed_pages() {
    let routes = route("pitchBend", 0, -1, "pitch", 1.0 / 6.0, 20, false)
        + &route("midiCC", 11, -1, "volume", 0.75, 20, false)
        + &route("monoAfterTouch", 0, -1, "pan", 0.5, 20, false);
    let fixture = Fixture::new(true, &routes);
    let events = [
        (3, Event::note_on(0, 60, 100)),
        (193, Event::pitch_bend(0, 0.5)),
        (3_003, Event::control_change(0, 11, 64)),
        (13_007, pressure(EventKind::ChannelPressure, 0, 0, 0.75)),
        (28_019, Event::pitch_bend(0, -0.75)),
        (34_037, Event::control_change(0, 11, 100)),
    ];
    let mut reference = None;
    for streamed in [false, true] {
        for block in [17, 256, 511] {
            let mut plugin = fixture.load(streamed);
            let audio = render(&mut plugin, 60_000, block, &events);
            if let Some(reference) = &reference {
                assert_eq!(&audio, reference, "streamed={streamed}, block={block}");
            } else {
                reference = Some(audio);
            }
            if streamed {
                let stats = plugin.streaming_stats();
                assert!(
                    stats.pages_read > 0,
                    "parity case must cross resident attacks"
                );
                assert_eq!(stats.underrun_frames, 0);
            }
        }
    }
}

#[test]
fn unrouted_expression_has_no_global_pitch_or_gain_fallback() {
    let fixture = Fixture::new(true, "");
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            12_000,
            71,
            &[
                (0, Event::pitch_bend(0, 1.0)),
                (0, Event::control_change(0, 11, 0)),
                (0, pressure(EventKind::ChannelPressure, 0, 0, 1.0)),
                (10, Event::note_on(0, 60, 100)),
                (100, pressure(EventKind::PolyPressure, 0, 60, 1.0)),
            ],
        );
        assert!((frequency(&audio[0][200..11_000]) - 200.0).abs() < 0.03);
        assert_eq!(audio[0], audio[1]);
        // 60 samples after onset is the 200 Hz sine's positive quarter period.
        assert!((audio[0][70] - LEVEL).abs() < 1e-6);
    }
}

#[test]
fn smoothing_settles_in_milliseconds_without_block_dependent_steps() {
    let routes = route("midiCC", 11, 0, "volume", 1.0, 20, false)
        .replace("smoothingCoef\" value=\"0", "smoothingCoef\" value=\"100");
    let fixture = Fixture::new(false, &routes);
    let events = [
        (0, Event::note_on(0, 60, 100)),
        (100, Event::control_change(0, 11, 127)),
    ];
    let mut reference = None;
    for streamed in [false, true] {
        for block in [17, 511] {
            let audio = render(&mut fixture.load(streamed), 6_000, block, &events);
            assert_level(&audio, 50, 0.0, 0.0);
            // The selected continuous approximation reaches 99% in 100 ms.
            let gain = audio[0][4_900] / LEVEL;
            assert!((gain - 0.99).abs() < 3e-5, "100 ms settling gain {gain}");
            assert!(audio[0][101] > 0.0 && audio[0][101] < LEVEL * 0.01);
            assert!(audio[0][100..5_500]
                .windows(2)
                .all(|pair| pair[0] <= pair[1]));
            if let Some(reference) = &reference {
                assert_eq!(&audio, reference);
            } else {
                reference = Some(audio);
            }
        }
    }

    // Smoothing starts from the initial source value; it does not fade in
    // a brand-new note from an artificial zero when CC was received earlier.
    let audio = render(
        &mut fixture.load(false),
        100,
        17,
        &[
            (0, Event::control_change(0, 11, 64)),
            (10, Event::note_on(0, 60, 100)),
        ],
    );
    assert_level(&audio, 10, 64.0 / 127.0, 64.0 / 127.0);
}

#[test]
fn table_endpoints_and_inversion_shape_cc_volume() {
    for invert in [false, true] {
        let flags = if invert { 22 } else { 20 };
        let routes = route("midiCC", 11, 0, "volume", 1.0, flags, false).replace(
            "</Target>",
            r#"<V name="t0" value="1"/><V name="t127" value="0"/></Target>"#,
        );
        let fixture = Fixture::new(false, &routes);
        for streamed in [false, true] {
            let audio = render(
                &mut fixture.load(streamed),
                100,
                17,
                &[
                    (0, Event::note_on(0, 60, 100)),
                    (20, Event::control_change(0, 11, 64)),
                    (40, Event::control_change(0, 11, 127)),
                ],
            );
            for (frame, input) in [(1, 0.0), (21, 64.0 / 127.0), (41, 1.0)] {
                let gain = if invert { input } else { 1.0 - input };
                assert_level(&audio, frame, gain, gain);
            }
        }
    }
}

#[test]
fn pitch_bend_table_maps_bipolar_source_through_unipolar_coordinates() {
    let routes = route("pitchBend", 0, -1, "pitch", 1.0, 20, false).replace(
        "</Target>",
        r#"<V name="t0" value="1"/><V name="t127" value="0"/></Target>"#,
    );
    let fixture = Fixture::new(true, &routes);
    let events = [
        (0, Event::pitch_bend(0, 1.0)),
        (10, Event::note_on(0, 60, 100)),
        (12_000, Event::pitch_bend(0, 0.0)),
        (24_000, Event::pitch_bend(0, -1.0)),
    ];
    for streamed in [false, true] {
        let audio = render(&mut fixture.load(streamed), 36_000, 257, &events);
        for (start, expected) in [(200, 100.0), (12_200, 200.0), (24_200, 400.0)] {
            let measured = frequency(&audio[0][start..start + 10_000]);
            assert!(
                (measured - expected).abs() < 0.03,
                "{measured} != {expected}"
            );
        }
    }
}

#[test]
fn linear_curve_endpoints_and_interpolated_volume_are_preserved() {
    let routes = route("midiCC", 11, 0, "volume", 1.0, 20, false).replace(
        "</Target>",
        r#"<V name="x0" value="0"/><V name="y0" value="0.25"/>
        <V name="c0" value="0"/>
        <V name="x1" value="1"/><V name="y1" value="0.75"/>
        <V name="c1" value="0"/></Target>"#,
    );
    let fixture = Fixture::new(false, &routes);
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            100,
            17,
            &[
                (0, Event::note_on(0, 60, 100)),
                (20, Event::control_change(0, 11, 64)),
                (40, Event::control_change(0, 11, 127)),
            ],
        );
        assert_level(&audio, 1, 0.25, 0.25);
        let middle = 0.25 + 0.5 * 64.0 / 127.0;
        assert_level(&audio, 21, middle, middle);
        assert_level(&audio, 41, 0.75, 0.75);
    }
}

#[test]
fn curved_routes_follow_exponential_shape_for_both_directions_and_curvature_signs() {
    for (low, high) in [(0.2, 0.8), (0.8, 0.2)] {
        for curvature in [-0.25, 0.25] {
            let routes = route("midiCC", 11, 0, "volume", 1.0, 20, false).replace(
                "</Target>",
                &format!(
                    r#"<V name="x0" value="0"/><V name="y0" value="{low}"/>
                    <V name="c0" value="{curvature}"/>
                    <V name="x1" value="1"/><V name="y1" value="{high}"/>
                    <V name="c1" value="0"/></Target>"#
                ),
            );
            let fixture = Fixture::new(false, &routes);
            for streamed in [false, true] {
                let audio = render(
                    &mut fixture.load(streamed),
                    100,
                    17,
                    &[
                        (0, Event::note_on(0, 60, 100)),
                        (20, Event::control_change(0, 11, 32)),
                        (40, Event::control_change(0, 11, 64)),
                        (60, Event::control_change(0, 11, 96)),
                        (80, Event::control_change(0, 11, 127)),
                    ],
                );
                // Use mathematical exp, independently of Kontakt's recovered
                // fast exponential approximation compiled by the sampler.
                let signed_k: f64 = curvature * 16.0 * if high >= low { 1.0 } else { -1.0 };
                let magnitude: f64 = signed_k.abs();
                for (frame, cc) in [(1, 0), (21, 32), (41, 64), (61, 96), (81, 127)] {
                    let t = cc as f64 / 127.0;
                    let shaped = if signed_k > 0.01 {
                        (magnitude * t).exp_m1() / magnitude.exp_m1()
                    } else if signed_k < -0.01 {
                        1.0 - (magnitude * (1.0 - t)).exp_m1() / magnitude.exp_m1()
                    } else {
                        t
                    };
                    let expected = low + (high - low) * shaped;
                    for channel in &audio {
                        let actual = (channel[frame] / LEVEL) as f64;
                        assert!(
                            (actual - expected).abs() < 0.003,
                            "curve {low}->{high}, curvature={curvature}, cc={cc}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_curve_breakpoint_order_fails_explicitly() {
    for (middle, end) in [(0.5, 0.5), (0.75, 0.5)] {
        let routes = route("midiCC", 11, 0, "volume", 1.0, 20, false).replace(
            "</Target>",
            &format!(
                r#"<V name="x0" value="0"/><V name="y0" value="0"/>
                <V name="c0" value="0.5"/>
                <V name="x1" value="{middle}"/><V name="y1" value="0.5"/>
                <V name="c1" value="0"/>
                <V name="x2" value="{end}"/><V name="y2" value="1"/>
                <V name="c2" value="0"/></Target>"#
            ),
        );
        let fixture = Fixture::new(false, &routes);
        let error = match SamplerPlugin::load(&fixture.instrument) {
            Ok(_) => panic!("malformed curve order must have an explicit diagnostic"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("curve"), "{error}");
    }
}

#[test]
fn ten_ms_positive_and_negative_smoothing_are_finite_monotonic_and_block_invariant() {
    for smoothing in ["10", "-10"] {
        let routes = route("midiCC", 11, 0, "volume", 1.0, 20, false).replace(
            "smoothingCoef\" value=\"0",
            &format!("smoothingCoef\" value=\"{smoothing}"),
        );
        let fixture = Fixture::new(false, &routes);
        let events = [
            (0, Event::note_on(0, 60, 100)),
            (100, Event::control_change(0, 11, 127)),
        ];
        let mut reference = None;
        for streamed in [false, true] {
            for block in [7, 128, 257] {
                let audio = render(&mut fixture.load(streamed), 800, block, &events);
                assert_level(&audio, 99, 0.0, 0.0);
                assert!(audio[0][100..700].windows(2).all(|pair| pair[0] <= pair[1]));
                if smoothing == "10" {
                    let gain = audio[0][580] / LEVEL;
                    assert!((gain - 0.99).abs() < 0.0002, "10 ms gain {gain}");
                } else {
                    // round(480 / 128) * 32 = 128 frames for this recovered
                    // negative-coefficient ramp duration at 48 kHz.
                    assert_level(&audio, 100, 1.0 / 128.0, 1.0 / 128.0);
                    assert_level(&audio, 163, 0.5, 0.5);
                    assert_level(&audio, 227, 1.0, 1.0);
                    assert_level(&audio, 228, 1.0, 1.0);
                }
                if let Some(reference) = &reference {
                    assert_eq!(&audio, reference);
                } else {
                    reference = Some(audio);
                }
            }
        }
    }
}

fn note_off_with_velocity(frame: usize, key: u8, velocity: u8) -> Event {
    let mut event = Event::note_off(frame, key);
    event.data2 = velocity as u16;
    event.value = velocity as f32 / 127.0;
    event
}

#[test]
fn release_velocity_drives_volume_routes_only_at_termination() {
    let fixture = Fixture::new(
        false,
        &route("releaseVelocity", 0, -1, "volume", 1.0, 20, false),
    );
    for streamed in [false, true] {
        for (release, expected) in [(0u8, 0.0f32), (127, 1.0), (64, 64.0 / 127.0)] {
            let audio = render(
                &mut fixture.load(streamed),
                400,
                17,
                &[
                    (0, Event::note_on(0, 60, 100)),
                    (100, note_off_with_velocity(0, 60, release)),
                ],
            );
            // The source is unavailable while the key is held.
            assert_level(&audio, 50, 1.0, 1.0);
            // Volume is linear in release velocity; it is not cubed like the
            // attack-velocity source.
            assert_level(&audio, 130, expected, expected);
        }
    }
}

#[test]
fn note_on_velocity_zero_uses_synthesized_half_scale_release_velocity() {
    let fixture = Fixture::new(
        false,
        &route("releaseVelocity", 0, -1, "volume", 1.0, 20, false),
    );
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            400,
            17,
            &[
                (0, Event::note_on(0, 60, 100)),
                (100, Event::note_on(0, 60, 0)),
            ],
        );
        assert_level(&audio, 50, 1.0, 1.0);
        // 0x8000 in the internal 16-bit domain is 0.5 normalized.
        assert_level(&audio, 130, 0.5, 0.5);
    }
}

#[test]
fn cc121_is_delivered_as_an_ordinary_controller() {
    // No generated-script CC121 branch exists in the recovered evidence; it
    // must not reset stored controller state or panic voices.
    let fixture = Fixture::new(false, &route("midiCC", 121, -1, "volume", 1.0, 20, false));
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            300,
            31,
            &[
                (0, Event::note_on(0, 60, 100)),
                (40, Event::control_change(0, 121, 0)),
                (120, Event::control_change(0, 121, 127)),
            ],
        );
        assert_level(&audio, 20, 1.0, 1.0);
        assert_level(&audio, 80, 0.0, 0.0);
        assert_level(&audio, 160, 1.0, 1.0);
    }
}

#[test]
fn all_sound_off_fades_sounding_voices_over_ten_ms() {
    let fixture = Fixture::new(false, &route("midiCC", 11, -1, "volume", 1.0, 20, false));
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            800,
            20,
            &[
                (0, Event::note_on(0, 60, 100)),
                (100, Event::control_change(0, 120, 0)),
            ],
        );
        assert_level(&audio, 60, 1.0, 1.0);
        assert_level(&audio, 100, 1.0, 1.0);
        assert_level(&audio, 340, 0.5, 0.5);
        assert_level(&audio, 579, 1.0 / 480.0, 1.0 / 480.0);
        assert_level(&audio, 580, 0.0, 0.0);
        assert_level(&audio, 700, 0.0, 0.0);
    }
}

#[test]
fn matching_new_attack_inherits_poly_pressure() {
    let fixture = Fixture::new(
        false,
        &route("polyAfterTouch", 0, -1, "volume", 1.0, 20, false),
    );
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            200,
            13,
            &[
                (0, Event::note_on(0, 60, 100)),
                (10, pressure(EventKind::PolyPressure, 0, 60, 0.5)),
                (20, Event::note_on(0, 60, 100)),
            ],
        );
        // A poly-pressure source starts at zero until the key is touched.
        assert_level(&audio, 5, 0.0, 0.0);
        // Live pressure halves the held voice.
        assert_level(&audio, 15, 0.5, 0.5);
        // The retrigger inherits the matching key/channel pressure instead of
        // starting at zero: 0.5 + 0.5.
        assert_level(&audio, 25, 1.0, 1.0);
    }
}

#[test]
fn internal_pitch_envelope_sweeps_a_held_note() {
    let internal = r#"<K2_IntMod><Targets><Target>
        <V name="target" value="pitch"/><V name="intensity" value="1"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="500"/><V name="hold" value="0"/>
        <V name="decay" value="0"/><V name="sustain" value="1"/>
        <V name="release" value="50"/></Envelope></K2_IntMod>"#;
    let fixture = Fixture::new_full(true, "", internal, "");
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            32_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        // During the 500 ms attack the pitch rises from 200 Hz toward 400 Hz.
        let sweeping = frequency(&audio[0][6_000..11_000]);
        assert!(
            (230.0..290.0).contains(&sweeping),
            "sweeping frequency {sweeping}"
        );
        // At sustain the envelope holds one octave above the root.
        let sustained = frequency(&audio[0][26_000..31_000]);
        assert!(
            (sustained - 400.0).abs() < 0.5,
            "sustained frequency {sustained}"
        );
    }
}

#[test]
fn partial_amplitude_envelope_intensity_scales_the_reduction() {
    let internal = r#"<K2_IntMod><Targets><Target>
        <V name="target" value="volume"/><V name="intensity" value="0.5"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="0"/><V name="hold" value="0"/>
        <V name="decay" value="0"/><V name="sustain" value="0"/>
        <V name="release" value="50"/></Envelope></K2_IntMod>"#;
    let fixture = Fixture::new_full(false, "", internal, "");
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            200,
            17,
            &[(0, Event::note_on(0, 60, 100))],
        );
        // A zero sustain level at 50% intensity leaves half the signal.
        assert_level(&audio, 100, 0.5, 0.5);
    }
}

#[test]
fn group_lowpass_filter_attenuates_a_high_note() {
    let filtered = Fixture::new_full(
        true,
        "",
        "",
        r#"<V name="filterCutoff" value="50"/><V name="filterQ" value="0.707"/>"#,
    );
    let open = Fixture::new(true, "");
    for streamed in [false, true] {
        let filtered = render(
            &mut filtered.load(streamed),
            12_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        let open = render(
            &mut open.load(streamed),
            12_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        let filtered = rms(&filtered[0][4_000..10_000]);
        let open = rms(&open[0][4_000..10_000]);
        assert!(
            filtered < open * 0.2,
            "filtered {filtered} is not well below open {open}"
        );
    }
}

#[test]
fn filter_cutoff_envelope_opens_the_filter() {
    let internal = r#"<K2_IntMod><Targets><Target>
        <V name="target" value="filterCutoff"/><V name="intensity" value="6"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="100"/><V name="hold" value="0"/>
        <V name="decay" value="0"/><V name="sustain" value="1"/>
        <V name="release" value="50"/></Envelope></K2_IntMod>"#;
    let fixture = Fixture::new_full(
        true,
        "",
        internal,
        r#"<V name="filterCutoff" value="50"/><V name="filterQ" value="0.707"/>"#,
    );
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            12_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        let closed = rms(&audio[0][200..800]);
        let open = rms(&audio[0][6_000..11_000]);
        assert!(
            open > closed * 3.0,
            "open filter {open} did not clear closed {closed}"
        );
    }
}

#[test]
fn external_cc_route_opens_the_group_filter() {
    let fixture = Fixture::new_full(
        true,
        &route("midiCC", 11, 0, "filterCutoff", 6.0, 20, false),
        "",
        r#"<V name="filterCutoff" value="50"/><V name="filterQ" value="0.707"/>"#,
    );
    for streamed in [false, true] {
        let audio = render(
            &mut fixture.load(streamed),
            10_000,
            257,
            &[
                (0, Event::note_on(0, 60, 100)),
                (4_000, Event::control_change(0, 11, 127)),
            ],
        );
        let closed = rms(&audio[0][1_000..3_000]);
        let open = rms(&audio[0][6_000..9_000]);
        assert!(
            open > closed * 3.0,
            "CC did not open the filter: open {open}, closed {closed}"
        );
    }
}

#[test]
fn xml_lfo_modulates_volume_like_the_binary_payload() {
    let internal = r#"<K2_IntMod><Targets><Target>
        <V name="target" value="volume"/><V name="intensity" value="1"/>
        </Target></Targets><LFO type="sine">
        <V name="delay" value="0"/><V name="frequency" value="1"/>
        <V name="pulseWidth" value="0.5"/><V name="startPhase" value="0"/>
        </LFO></K2_IntMod>"#;
    let lfo = Fixture::new_full(true, "", internal, "");
    let reference = Fixture::new(true, "");
    for streamed in [false, true] {
        let lfo = render(
            &mut lfo.load(streamed),
            48_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        let reference = render(
            &mut reference.load(streamed),
            48_000,
            257,
            &[(0, Event::note_on(0, 60, 100))],
        );
        for (center, expected) in [(12_000usize, 1.0f32), (24_000, 0.5), (36_000, 0.0)] {
            let window = center - 600..center + 600;
            let ratio = rms(&lfo[0][window.clone()]) / rms(&reference[0][window]);
            assert!(
                (ratio - expected).abs() < 0.08,
                "0.25s phase at frame {center}: ratio {ratio}, expected {expected}"
            );
        }
    }
}

#[test]
fn key_position_from_xml_tracks_note_pitch_without_sample_key_tracking() {
    let fixture = Fixture::new(true, &route("keyPos", 0, 0, "pitch", 1.0, 16, false));
    for (note, expected_hz) in [(48, 100.0), (60, 200.0), (72, 400.0)] {
        let audio = render(
            &mut fixture.load(false),
            12_000,
            257,
            &[(0, Event::note_on(0, note, 127))],
        );
        let measured = frequency(&audio[0][2_000..10_000]);
        assert!(
            (measured - expected_hz).abs() < 0.05,
            "key {note}: {measured} Hz"
        );
    }
}
