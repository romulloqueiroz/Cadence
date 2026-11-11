//! Audio-level regressions for Kontakt group-start rules, key switches,
//! controller criteria and sustain-pedal release deferral.
//!
//! Every preset and PCM buffer is generated here and loaded through the public
//! API, so these tests exercise the production playback path.

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
    fn new(program: &str, groups: &[String], zones: &[String]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cadence-group-start-audio-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let xml = format!(
            r#"<K2_Container><Programs><K2_Program name="Group start audio">
            <Parameters>{program}</Parameters><Groups>{}</Groups>
            <Zones>{}</Zones></K2_Program></Programs></K2_Container>"#,
            groups.concat(),
            zones.concat()
        );
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let mut bytes = vec![0; 0xaa];
        bytes[..4].copy_from_slice(&nk_format::PLAIN_MAGIC.to_le_bytes());
        bytes[8..10].copy_from_slice(&0x100u16.to_le_bytes());
        bytes.extend(encoder.finish().unwrap());
        let instrument = root.join("selection.nki");
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

fn criteria(entries: &[(&str, &[(&str, &str)])]) -> String {
    let mut body = String::new();
    for (mode, values) in entries {
        body.push_str("<StartCriteria>");
        body.push_str(&format!("<V name=\"mode\" value=\"{mode}\"/>"));
        for (name, value) in *values {
            body.push_str(&format!("<V name=\"{name}\" value=\"{value}\"/>"));
        }
        body.push_str("</StartCriteria>");
    }
    format!("<GroupStart>{body}</GroupStart>")
}

fn envelope(release_ms: u32) -> String {
    format!(
        r#"<IntModulators><K2_IntMod><Targets><Target>
        <V name="target" value="volume"/><V name="intensity" value="1"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="0"/><V name="decay" value="0"/>
        <V name="sustain" value="1"/><V name="release" value="{release_ms}"/>
        </Envelope></K2_IntMod></IntModulators>"#
    )
}

fn group(index: u32, release: bool, start: &str, release_ms: u32) -> String {
    format!(
        r#"<K2_Group index="{index}" name="Layer {index}"><Parameters>
        <V name="keyTracking" value="no"/><V name="releaseTrigger" value="{release}"/>
        </Parameters>{start}{}</K2_Group>"#,
        envelope(release_ms)
    )
}

fn zone(group: u32, volume: f32) -> String {
    format!(
        r#"<K2_Zone groupIdx="{group}"><Parameters>
        <V name="sampleStart" value="32"/><V name="sampleEnd" value="79999"/>
        <V name="lowKey" value="60"/><V name="highKey" value="60"/>
        <V name="lowVelocity" value="0"/><V name="highVelocity" value="127"/>
        <V name="rootKey" value="60"/><V name="zoneVolume" value="{volume}"/>
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

fn level_at(audio: &[Vec<f32>; 2], frame: usize) -> f32 {
    let left = audio[0][frame] / LEFT;
    let right = audio[1][frame] / RIGHT;
    assert!(
        (left - right).abs() < 1e-4,
        "frame={frame}: left/right levels disagree ({left} vs {right})"
    );
    left
}

fn assert_level_at(audio: &[Vec<f32>; 2], frame: usize, gain: f32) {
    let actual = level_at(audio, frame);
    assert!(
        (actual - gain).abs() < 1e-4,
        "frame={frame}: {actual} != {gain}"
    );
}

#[test]
fn round_robin_cycles_in_configured_order_and_wraps() {
    let groups = [
        group(
            0,
            false,
            &criteria(&[("cycle_round_robin", &[("cycleClass", "0")])]),
            0,
        ),
        group(
            1,
            false,
            &criteria(&[("cycle_round_robin", &[("cycleClass", "1")])]),
            0,
        ),
        group(
            2,
            false,
            &criteria(&[("cycle_round_robin", &[("cycleClass", "2")])]),
            0,
        ),
    ];
    let zones = [zone(0, 1.0), zone(1, 0.5), zone(2, 0.25)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events: Vec<_> = (0..4)
            .flat_map(|i| {
                let start = i * 20;
                [
                    (start, Event::note_on(0, 60, 100)),
                    (start + 10, Event::note_off(0, 60)),
                ]
            })
            .collect();
        let audio = render(&mut plugin, 80, 13, &events);
        for (frame, gain) in [(5, 1.0), (25, 0.5), (45, 0.25), (65, 1.0)] {
            assert_level_at(&audio, frame, gain);
        }
    }
}

#[test]
fn random_selection_draws_one_shared_ordinal_per_note() {
    let groups = [
        group(0, false, &criteria(&[("cycle_random", &[])]), 0),
        group(1, false, &criteria(&[("cycle_random", &[])]), 0),
        group(2, false, &criteria(&[("cycle_random", &[])]), 0),
    ];
    let zones = [zone(0, 1.0), zone(1, 0.5), zone(2, 0.25)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        plugin.seed_random(0x1234_5678_9abc_def0);
        let notes = 30;
        let events: Vec<_> = (0..notes)
            .flat_map(|i| {
                let start = i * 10;
                [
                    (start, Event::note_on(0, 60, 100)),
                    (start + 5, Event::note_off(0, 60)),
                ]
            })
            .collect();
        let audio = render(&mut plugin, notes * 10, 7, &events);
        let mut seen = [false; 3];
        for i in 0..notes {
            let gain = level_at(&audio, i * 10 + 2);
            let index = match gain {
                g if (g - 1.0).abs() < 1e-4 => 0,
                g if (g - 0.5).abs() < 1e-4 => 1,
                g if (g - 0.25).abs() < 1e-4 => 2,
                other => panic!("note {i}: unexpected level {other}"),
            };
            seen[index] = true;
        }
        assert_eq!(seen, [true, true, true]);
    }
}

#[test]
fn latched_key_switches_start_at_default_and_survive_out_of_range_notes() {
    let groups = [
        group(
            0,
            false,
            &criteria(&[("on_key", &[("key_min", "24"), ("key_max", "25")])]),
            0,
        ),
        group(
            1,
            false,
            &criteria(&[("on_key", &[("key_min", "26"), ("key_max", "27")])]),
            0,
        ),
    ];
    let zones = [zone(0, 1.0), zone(1, 0.5)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events = [
            // Lowest configured key is the default, so group 0 starts latched.
            (0, Event::note_on(0, 60, 100)),
            (5, Event::note_off(0, 60)),
            (10, Event::note_on(0, 26, 100)),
            (15, Event::note_off(0, 26)),
            (20, Event::note_on(0, 60, 100)),
            (25, Event::note_off(0, 60)),
            // Outside the union of configured key ranges: retains the latch.
            (30, Event::note_on(0, 100, 100)),
            (35, Event::note_off(0, 100)),
            (40, Event::note_on(0, 60, 100)),
            (45, Event::note_off(0, 60)),
        ];
        let audio = render(&mut plugin, 50, 9, &events);
        assert_level_at(&audio, 2, 1.0);
        assert_level_at(&audio, 22, 0.5);
        assert_level_at(&audio, 42, 0.5);
    }
}

#[test]
fn controller_criteria_track_channel_state_and_default_to_zero() {
    let groups = [
        group(
            0,
            false,
            &criteria(&[(
                "on_controller",
                &[("controller", "1"), ("cc_min", "64"), ("cc_max", "127")],
            )]),
            0,
        ),
        group(
            1,
            false,
            &criteria(&[(
                "on_controller",
                &[("controller", "1"), ("cc_min", "0"), ("cc_max", "63")],
            )]),
            0,
        ),
    ];
    let zones = [zone(0, 1.0), zone(1, 0.5)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events = [
            (0, Event::note_on(0, 60, 100)),
            (5, Event::note_off(0, 60)),
            (10, Event::control_change(0, 1, 100)),
            (15, Event::note_on(0, 60, 100)),
            (20, Event::note_off(0, 60)),
            (25, Event::control_change(0, 1, 10)),
            (30, Event::note_on(0, 60, 100)),
            (35, Event::note_off(0, 60)),
        ];
        let audio = render(&mut plugin, 40, 9, &events);
        assert_level_at(&audio, 2, 0.5);
        assert_level_at(&audio, 17, 1.0);
        assert_level_at(&audio, 32, 0.5);
    }
}

#[test]
fn criteria_connectors_fold_left_to_right() {
    type Check = (u8, u8, bool);
    type ConnectorCase = (&'static str, [Check; 3]);
    let cases: [ConnectorCase; 3] = [
        ("and", [(100, 100, true), (100, 0, false), (0, 100, false)]),
        (
            "and_not",
            [(100, 0, true), (100, 100, false), (0, 0, false)],
        ),
        ("or", [(100, 0, true), (0, 100, true), (0, 0, false)]),
    ];
    for (connector, checks) in cases {
        let start = format!(
            r#"<GroupStart>
            <StartCriteria><V name="mode" value="on_controller"/><V name="controller" value="1"/><V name="cc_min" value="64"/><V name="cc_max" value="127"/><V name="nextCriteria" value="{connector}"/></StartCriteria>
            <StartCriteria><V name="mode" value="on_controller"/><V name="controller" value="2"/><V name="cc_min" value="64"/><V name="cc_max" value="127"/></StartCriteria>
            </GroupStart>"#
        );
        let fixture = Fixture::new("", &[group(0, false, &start, 0)], &[zone(0, 1.0)]);
        for (cc1, cc2, selected) in checks {
            let mut plugin = fixture.load(false);
            let events = [
                (0, Event::control_change(0, 1, cc1)),
                (1, Event::control_change(0, 2, cc2)),
                (5, Event::note_on(0, 60, 100)),
                (10, Event::note_off(0, 60)),
            ];
            let audio = render(&mut plugin, 15, 8, &events);
            assert_level_at(&audio, 7, if selected { 1.0 } else { 0.0 });
        }
    }
}

#[test]
fn sustain_pedal_defers_release_until_pedal_up() {
    let fixture = Fixture::new("", &[group(0, false, "<GroupStart/>", 50)], &[zone(0, 1.0)]);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events = [
            (0, Event::note_on(0, 60, 100)),
            (5, Event::control_change(0, 64, 127)),
            (10, Event::note_off(0, 60)),
            (1000, Event::control_change(0, 64, 0)),
        ];
        let audio = render(&mut plugin, 4000, 17, &events);
        assert_level_at(&audio, 500, 1.0);
        assert_level_at(&audio, 1000, 1.0);
        assert_level_at(&audio, 2000, 1.0 - 1000.0 / 2400.0);
        assert_level_at(&audio, 3500, 0.0);
    }
}

#[test]
fn release_layers_wait_for_pedal_up_and_use_the_attack_velocity() {
    let groups = [
        // The attack layer stops immediately when released.
        group(0, false, "<GroupStart/>", 0),
        group(1, true, "<GroupStart/>", 2),
    ];
    let zones = [zone(0, 1.0), zone(1, 0.25)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events = [
            (0, Event::note_on(0, 60, 100)),
            (5, Event::control_change(0, 64, 127)),
            (10, Event::note_off(0, 60)),
            (1000, Event::control_change(0, 64, 0)),
            (3000, Event::note_off(0, 60)),
        ];
        let audio = render(&mut plugin, 4000, 17, &events);
        // Pedal down: the attack layer keeps sounding, no release layer yet.
        assert_level_at(&audio, 500, 1.0);
        // Pedal up: the attack layer stops and the release layer starts.
        assert_level_at(&audio, 1100, 0.25);
        assert_level_at(&audio, 2999, 0.25);
    }
}

#[test]
fn controller_only_cc64_mode_releases_without_deferral() {
    let program = r#"<V name="cc64Mode" value="cc64_mode_controller_only"/>"#;
    let fixture = Fixture::new(
        program,
        &[group(0, false, "<GroupStart/>", 50)],
        &[zone(0, 1.0)],
    );
    for streaming in [false, true] {
        let mut plugin = fixture.load(streaming);
        let events = [
            (0, Event::note_on(0, 60, 100)),
            (5, Event::control_change(0, 64, 127)),
            (10, Event::note_off(0, 60)),
        ];
        let audio = render(&mut plugin, 3000, 17, &events);
        // Release starts at the note-off, not at pedal-up (which never occurs).
        assert_level_at(&audio, 1210, 1.0 - 1200.0 / 2400.0);
        assert_level_at(&audio, 2500, 0.0);
    }
}

fn channel(mut event: Event, channel: u8) -> Event {
    event.channel = channel;
    event
}

#[test]
fn same_pitch_note_off_matches_only_its_original_channel() {
    let fixture = Fixture::new("", &[group(0, false, "", 0)], &[zone(0, 1.0)]);
    let events = [
        (0, channel(Event::note_on(0, 60, 40), 0)),
        (2, channel(Event::note_on(0, 60, 100), 1)),
        (5, channel(Event::note_off(0, 60), 2)),
        (10, channel(Event::note_off(0, 60), 0)),
        (20, channel(Event::note_off(0, 60), 1)),
    ];
    let mut reference = None;
    for streaming in [false, true] {
        for block in [7, 64] {
            let audio = render(&mut fixture.load(streaming), 40, block, &events);
            assert_level_at(&audio, 1, 1.0);
            assert_level_at(&audio, 6, 2.0);
            assert_level_at(&audio, 11, 1.0);
            assert_level_at(&audio, 21, 0.0);
            if let Some(reference) = &reference {
                assert_eq!(&audio, reference);
            } else {
                reference = Some(audio);
            }
        }
    }
}

#[test]
fn pedal_release_keeps_channels_and_new_attacks_independent() {
    let fixture = Fixture::new("", &[group(0, false, "", 0)], &[zone(0, 1.0)]);
    let events = [
        (0, Event::note_on(0, 60, 100)),
        (1, channel(Event::note_on(0, 60, 100), 1)),
        (2, Event::control_change(0, 64, 127)),
        (3, Event::note_off(0, 60)),
        (4, channel(Event::note_off(0, 60), 1)),
        (5, channel(Event::control_change(0, 64, 0), 1)),
        (6, Event::note_on(0, 60, 100)),
        (10, Event::control_change(0, 64, 0)),
        (20, Event::note_off(0, 60)),
    ];
    for streaming in [false, true] {
        let audio = render(&mut fixture.load(streaming), 40, 7, &events);
        assert_level_at(&audio, 3, 2.0);
        assert_level_at(&audio, 5, 1.0);
        assert_level_at(&audio, 7, 2.0);
        // Pedal up releases only the older deferred onset, not its retrigger.
        assert_level_at(&audio, 11, 1.0);
        assert_level_at(&audio, 21, 0.0);
    }
}

#[test]
fn repeated_attacks_keep_their_own_attack_velocity_for_release_layers() {
    let groups = [group(0, false, "", 0), group(1, true, "", 0)];
    let zones = [
        zone(0, 1.0),
        zone(1, 0.25).replace("highVelocity\" value=\"127", "highVelocity\" value=\"63"),
        zone(1, 0.5).replace("lowVelocity\" value=\"0", "lowVelocity\" value=\"64"),
    ];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        for pedal in [false, true] {
            let mut events = vec![
                (0, Event::note_on(0, 60, 40)),
                (2, Event::note_on(0, 60, 100)),
                (10, Event::note_off(0, 60)),
                (15, Event::note_off(0, 60)),
            ];
            if pedal {
                events.insert(0, (0, Event::control_change(0, 64, 127)));
                events.push((20, Event::note_on(0, 60, 100)));
                events.push((30, Event::control_change(0, 64, 0)));
            }
            let audio = render(&mut fixture.load(streaming), 50, 13, &events);
            assert_level_at(&audio, 5, 2.0);
            if pedal {
                assert_level_at(&audio, 16, 2.0);
                assert_level_at(&audio, 25, 3.0);
                assert_level_at(&audio, 35, 1.75);
            } else {
                assert_level_at(&audio, 11, 0.75);
                assert_level_at(&audio, 16, 0.75);
            }
        }
    }
}

#[test]
fn panic_discards_held_and_deferred_release_events() {
    let fixture = Fixture::new(
        "",
        &[group(0, false, "", 0), group(1, true, "", 0)],
        &[zone(0, 1.0), zone(1, 0.25)],
    );
    for streaming in [false, true] {
        let audio = render(
            &mut fixture.load(streaming),
            600,
            7,
            &[
                (0, Event::note_on(0, 60, 40)),
                (1, Event::control_change(0, 64, 127)),
                (2, Event::note_off(0, 60)),
                (3, channel(Event::note_on(0, 60, 100), 1)),
                (5, Event::control_change(0, 120, 0)),
                (7, channel(Event::note_off(0, 60), 1)),
                (8, Event::control_change(0, 64, 0)),
            ],
        );
        // The recovered CC120 handler fades all events over 10 ms (480 frames)
        // before requesting note-off, rather than panicking instantly.
        assert_level_at(&audio, 4, 2.0);
        assert_level_at(&audio, 5, 2.0);
        assert_level_at(&audio, 6, 2.0 * (1.0 - 1.0 / 480.0));
        // The note-off at frame 7 must not spawn a release layer: the held and
        // deferred note metadata was already discarded.
        assert_level_at(&audio, 8, 2.0 * (1.0 - 3.0 / 480.0));
        assert_level_at(&audio, 484, 2.0 / 480.0);
        assert_level_at(&audio, 485, 0.0);
        assert_level_at(&audio, 500, 0.0);
        assert_level_at(&audio, 520, 0.0);
    }
}

#[test]
fn all_notes_off_runs_release_layers_and_honors_each_channels_pedal() {
    let fixture = Fixture::new(
        "",
        &[group(0, false, "", 0), group(1, true, "", 0)],
        &[zone(0, 1.0), zone(1, 0.25)],
    );
    for streaming in [false, true] {
        let audio = render(
            &mut fixture.load(streaming),
            40,
            7,
            &[
                (0, Event::note_on(0, 60, 40)),
                (1, channel(Event::note_on(0, 60, 100), 1)),
                (2, Event::control_change(0, 64, 127)),
                (5, Event::control_change(0, 123, 0)),
                (7, Event::control_change(0, 123, 0)),
                (10, Event::control_change(0, 64, 0)),
            ],
        );
        assert_level_at(&audio, 4, 2.0);
        assert_level_at(&audio, 6, 1.25);
        assert_level_at(&audio, 8, 1.25);
        assert_level_at(&audio, 11, 0.5);
    }
}

#[test]
fn malformed_midi_cannot_wrap_into_valid_notes_channels_or_panic() {
    let fixture = Fixture::new("", &[group(0, false, "", 0)], &[zone(0, 1.0)]);
    let mut invalid_key = Event::note_off(0, 60);
    invalid_key.data1 = 316;
    let mut invalid_controller = Event::control_change(0, 120, 0);
    invalid_controller.data1 = 376;
    let events = [
        (0, channel(Event::note_on(0, 60, 100), 16)),
        (2, Event::note_on(0, 60, 100)),
        (5, channel(Event::note_off(0, 60), 16)),
        (6, invalid_key),
        (7, invalid_controller),
        (10, Event::note_off(0, 60)),
    ];
    for streaming in [false, true] {
        let audio = render(&mut fixture.load(streaming), 20, 7, &events);
        assert_level_at(&audio, 1, 0.0);
        assert_level_at(&audio, 8, 1.0);
        assert_level_at(&audio, 11, 0.0);
    }
}

fn curved_group(index: u32, curve: f32) -> String {
    format!(
        r#"<K2_Group index="{index}" name="Curved {index}"><Parameters>
        <V name="keyTracking" value="no"/>
        </Parameters><IntModulators><K2_IntMod><Targets><Target>
        <V name="target" value="volume"/><V name="intensity" value="1"/>
        </Target></Targets><Envelope type="ahdsr">
        <V name="attack" value="100"/><V name="hold" value="0"/>
        <V name="decay" value="0"/><V name="sustain" value="1"/>
        <V name="release" value="50"/><V name="atkCurving" value="{curve}"/>
        </Envelope></K2_IntMod></IntModulators></K2_Group>"#
    )
}

#[test]
fn curved_attack_follows_recovered_curvature_law() {
    // Attack is 100 ms at 48 kHz, so frame 2400 is the segment midpoint.
    let t = 2400.0 / 4800.0;
    for curve in [-0.5f32, 0.0, 0.5] {
        let fixture = Fixture::new("", &[curved_group(0, curve)], &[zone(0, 1.0)]);
        let k = curve as f64 * 16.0;
        let shaped = if k > 0.01 {
            (k * t).exp_m1() / k.exp_m1()
        } else if k < -0.01 {
            1.0 - ((-k) * (1.0 - t)).exp_m1() / (-k).exp_m1()
        } else {
            t
        };
        for streaming in [false, true] {
            for block in [17, 256, 1024] {
                let audio = render(
                    &mut fixture.load(streaming),
                    5200,
                    block,
                    &[(0, Event::note_on(0, 60, 100))],
                );
                let actual = level_at(&audio, 2400) as f64;
                assert!(
                    (actual - shaped).abs() < 0.003,
                    "curve={curve}, streamed={streaming}, block={block}: {actual} != {shaped}"
                );
                // The curve is monotonic across the attack segment.
                let previous = level_at(&audio, 2300);
                assert!(level_at(&audio, 2400) >= previous);
                // Full attack reaches unity for every curvature.
                assert!((level_at(&audio, 4800) - 1.0).abs() < 1e-4);
            }
        }
    }
}

fn group_on_channel(index: u32, channel: i64) -> String {
    format!(
        r#"<K2_Group index="{index}" name="Channel {index}"><Parameters>
        <V name="keyTracking" value="no"/><V name="midiChannel" value="{channel}"/>
        </Parameters>{}</K2_Group>"#,
        envelope(0)
    )
}

#[test]
fn group_midi_channel_filters_playback_and_zero_is_all_channels() {
    let groups = [group_on_channel(0, 1), group_on_channel(1, 2)];
    let zones = [zone(0, 1.0), zone(1, 0.5)];
    let fixture = Fixture::new("", &groups, &zones);
    for streaming in [false, true] {
        for (midi_channel, expected) in [(0u8, 1.0), (1, 0.5), (2, 0.0)] {
            let audio = render(
                &mut fixture.load(streaming),
                80,
                17,
                &[(0, channel(Event::note_on(0, 60, 100), midi_channel))],
            );
            assert_level_at(&audio, 10, expected);
        }
    }

    let groups = [group(0, false, "", 0)];
    let fixture = Fixture::new("", &groups, &[zone(0, 0.25)]);
    for streaming in [false, true] {
        for midi_channel in 0..16 {
            let audio = render(
                &mut fixture.load(streaming),
                80,
                17,
                &[(0, channel(Event::note_on(0, 60, 100), midi_channel))],
            );
            assert_level_at(&audio, 10, 0.25);
        }
    }
}
