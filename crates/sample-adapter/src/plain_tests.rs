use super::*;

const MAPPING: &str = include_str!("../../nk-format/tests/fixtures/plain-playback.xml");

fn write_plain(fixture: &Fixture, xml: &str) -> PathBuf {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let mut bytes = vec![0; 0xaa];
    bytes[..4].copy_from_slice(&nk_format::PLAIN_MAGIC.to_le_bytes());
    bytes[8..10].copy_from_slice(&0x100u16.to_le_bytes());
    bytes.extend(encoder.finish().unwrap());
    let path = fixture.directory.join("plain.nki");
    fs::write(&path, bytes).unwrap();
    path
}

fn basic_xml(group: &str, zone: &str, extra: &str) -> String {
    format!(
        r#"<K2_Container><Programs><K2_Program name="Plain test"><Parameters/>
      <Groups><K2_Group index="9" name="Pedal Release are display words">
        <Parameters>{group}</Parameters><GroupStart/>{extra}</K2_Group></Groups>
      <Zones><K2_Zone groupIdx="9"><Parameters>{zone}</Parameters>
        <Sample><V name="file_ex2" value="sample.wav"/></Sample></K2_Zone></Zones>
    </K2_Program></Programs></K2_Container>"#
    )
}

fn load_pair(fixture: &Fixture, xml: &str) -> (SamplerPlugin, SamplerPlugin) {
    let path = write_plain(fixture, xml);
    let mut resident = SamplerPlugin::load(&path).unwrap();
    assert_eq!(resident.preload_all(), 1);
    let mut streamed = SamplerPlugin::load(&path).unwrap();
    streamed.stream_all(StreamingConfig::default()).unwrap();
    streamed.set_offline_rendering(true);
    (resident, streamed)
}

#[test]
fn xml_playback_has_analytical_gain_pitch_mapping_and_inclusive_endpoint() {
    let fixture = Fixture::new(128);
    fs::create_dir_all(fixture.directory.join("Samples")).unwrap();
    fs::copy(
        &fixture.sample,
        fixture.directory.join("Samples/tone Ω.wav"),
    )
    .unwrap();
    let path = write_plain(&fixture, MAPPING);
    let reference = nk_format::audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
    for streamed in [false, true] {
        let mut plugin = SamplerPlugin::load(&path).unwrap();
        if streamed {
            plugin.stream_all(StreamingConfig::default()).unwrap();
            plugin.set_offline_rendering(true);
        } else {
            plugin.preload_all();
        }
        let context = ProcessContext {
            block_size: 32,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 64];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(2, 48, 100)],
        );
        // Input 48 transposes to zone/root 60. 1.25 * 0.8 = unity pitch;
        // program/group/zone gain is 0.5 * 0.8 * 0.25 = 0.1, pan sums to 0.
        for channel in 0..2 {
            for frame in 0..32 {
                let expected = if (2..9).contains(&frame) {
                    reference.samples[channel][3 + frame - 2] * 0.1
                } else {
                    0.0
                };
                assert!(
                    (output[channel * 32 + frame] - expected).abs() < 1e-6,
                    "stream={streamed} channel={channel} frame={frame}"
                );
            }
        }
        plugin.reset();
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 47, 100)],
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 48, 9)],
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn xml_fractional_pitch_never_interpolates_past_zone_endpoint() {
    let fixture = Fixture::new(128);
    let xml = basic_xml(
        "",
        r#"<V name="sampleStart" value="3"/><V name="sampleEnd" value="4"/><V name="zoneTune" value="0.5"/>"#,
        "",
    );
    let reference = nk_format::audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 8,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 16];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 60, 100)],
        );
        for channel in 0..2 {
            let a = reference.samples[channel][3];
            let b = reference.samples[channel][4];
            let expected = [a, 0.5 * (a + b), b, b, 0.0, 0.0, 0.0, 0.0];
            for i in 0..8 {
                assert!((output[channel * 8 + i] - expected[i]).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn xml_reverse_and_disabled_key_tracking_use_correct_sample_window() {
    let fixture = Fixture::new(128);
    let xml = basic_xml(
        r#"<V name="reverse" value="yes"/><V name="keyTracking" value="no"/>"#,
        r#"<V name="sampleStart" value="3"/><V name="sampleEnd" value="9"/>"#,
        "",
    );
    let reference = nk_format::audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 16,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 32];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 84, 100)],
        );
        for channel in 0..2 {
            for frame in 0..16 {
                let expected = if frame < 7 {
                    reference.samples[channel][9 - frame]
                } else {
                    0.0
                };
                assert!((output[channel * 16 + frame] - expected).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn xml_envelope_release_is_sample_accurate_and_can_be_shorter_than_50_ms() {
    let fixture = Fixture::new(10_000);
    let envelope = r#"<IntModulators><K2_IntMod><Targets><Target><V name="target" value="volume"/><V name="intensity" value="1"/></Target></Targets>
       <Envelope type="ahdsr"><V name="attack" value="0"/><V name="sustain" value="0.5"/><V name="release" value="2"/></Envelope></K2_IntMod></IntModulators>"#;
    let xml = basic_xml("", "", envelope);
    let reference = nk_format::audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 256,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 512];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 60, 100), Event::note_off(32, 60)],
        );
        for (frame, actual) in output.iter().take(256).enumerate() {
            let level = if frame < 32 {
                0.5
            } else if frame < 128 {
                0.5 * (1.0 - (frame - 32) as f32 / 96.0)
            } else {
                0.0
            };
            assert!(
                (*actual - reference.samples[0][frame] * level).abs() < 1e-6,
                "frame={frame}"
            );
        }
    }
}

#[test]
fn xml_release_trigger_is_explicit_and_ignores_unmatched_or_repeated_note_off() {
    let fixture = Fixture::new(10_000);
    let xml = basic_xml(r#"<V name="releaseTrigger" value="yes"/>"#, "", "");
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 64,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 128];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_off(0, 60)],
        );
        assert!(output.iter().all(|v| *v == 0.0));
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 60, 100)],
        );
        assert!(output.iter().all(|v| *v == 0.0));
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_off(7, 60)],
        );
        assert!(output[..7].iter().all(|v| *v == 0.0));
        assert!(output[8..64].iter().any(|v| v.abs() > 1e-4));
        let voices = plugin.voices.iter().flatten().count();
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_off(0, 60)],
        );
        assert_eq!(plugin.voices.iter().flatten().count(), voices);
    }
}

#[test]
fn xml_one_shot_ignores_note_off_and_sustain_loop_exits_on_release() {
    let fixture = Fixture::new(10_000);
    for mode in ["oneshot", "until_release", "until_end"] {
        let extra = format!(
            r#"<Loops><Loop><V name="mode" value="{mode}"/><V name="loopStart" value="2"/><V name="loopLength" value="4"/></Loop></Loops>"#
        );
        let xml = basic_xml("", "", "").replace("<Sample>", &format!("{extra}<Sample>"));
        let reference = nk_format::audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
        let (resident, streamed) = load_pair(&fixture, &xml);
        for mut plugin in [resident, streamed] {
            let context = ProcessContext {
                block_size: 32,
                ..Default::default()
            };
            plugin.prepare(&context);
            let mut output = [0.0; 64];
            block(
                &mut plugin,
                &context,
                &mut output,
                &[Event::note_on(0, 60, 100), Event::note_off(8, 60)],
            );
            for (frame, actual) in output.iter().take(32).enumerate() {
                let source = if mode == "oneshot" || frame < 6 {
                    frame
                } else if mode == "until_end" || frame < 8 {
                    2 + (frame - 6) % 4
                } else {
                    4 + frame - 8
                };
                assert!(
                    (*actual - reference.samples[0][source]).abs() < 1e-6,
                    "mode={mode} frame={frame}"
                );
            }
        }
    }
}

#[test]
fn xml_muted_and_solo_groups_gate_playback() {
    let fixture = Fixture::new(128);
    let xml = basic_xml(r#"<V name="muted" value="yes"/>"#, "", "");
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 16,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 32];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 60, 100)],
        );
        assert!(output.iter().all(|v| *v == 0.0));
    }
    let xml=basic_xml("","","").replace("</Groups>",r#"<K2_Group index="18"><Parameters><V name="soloed" value="yes"/></Parameters></K2_Group></Groups>"#);
    let (resident, streamed) = load_pair(&fixture, &xml);
    for mut plugin in [resident, streamed] {
        let context = ProcessContext {
            block_size: 16,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = [0.0; 32];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[Event::note_on(0, 60, 100)],
        );
        assert!(output.iter().all(|v| *v == 0.0));
    }
}

#[test]
fn xml_streaming_parity_and_realtime_callbacks_preserve_allocation_guards() {
    let fixture = Fixture::new(160_000);
    let xml = basic_xml("", "", "");
    let (mut resident, mut streamed) = load_pair(&fixture, &xml);
    let mut context = ProcessContext {
        block_size: 257,
        ..Default::default()
    };
    resident.prepare(&context);
    streamed.prepare(&context);
    let mut a = vec![0.0; 514];
    let mut b = a.clone();
    for i in 0..650 {
        let events = if i == 0 {
            vec![Event::note_on(13, 60, 100)]
        } else {
            vec![]
        };
        block(&mut resident, &context, &mut a, &events);
        block(&mut streamed, &context, &mut b, &events);
        assert_eq!(a, b, "stream/RAM mismatch at block {i}");
        context.sample_pos += 257;
    }
    assert!(streamed.streaming_stats().pages_read > 0);
    assert_eq!(streamed.streaming_stats().underrun_frames, 0);
    streamed.reset();
    streamed.set_offline_rendering(false);
    context.block_size = 64;
    let mut output = [0.0; 128];
    ALLOCS.with(|count| count.set(0));
    TRACK.with(|flag| flag.set(true));
    block(
        &mut streamed,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100), Event::note_off(32, 60)],
    );
    streamed.reset();
    TRACK.with(|flag| flag.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "XML callback allocated or freed storage"
    );
}

#[test]
fn xml_missing_samples_and_out_of_file_ranges_fail_preparation() {
    let fixture = Fixture::new(128);
    for xml in [
        basic_xml("", r#"<V name="sampleStart" value="999"/>"#, ""),
        basic_xml("", r#"<V name="sampleEnd" value="999"/>"#, ""),
        basic_xml("", "", "").replace("sample.wav", "missing.wav"),
    ] {
        let path = write_plain(&fixture, &xml);
        let mut plugin = SamplerPlugin::load(path).unwrap();
        assert!(plugin.stream_all(StreamingConfig::default()).is_err());
    }
}
