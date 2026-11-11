use nk_format::{
    model,
    preset_chunk::{self, PresetChunk},
};

fn program() -> PresetChunk {
    let mut public = 0u32.to_le_bytes().to_vec(); // empty UTF-16 name
    public.extend_from_slice(&0f64.to_le_bytes());
    public.push(0); // transpose
    for value in [1f32, 0.0, 1.0] {
        // volume, pan, tuning
        public.extend_from_slice(&value.to_le_bytes());
    }
    public.extend_from_slice(&[0, 127, 0, 127]);
    public.extend_from_slice(&(-1i16).to_le_bytes());
    let mut empty_array = 0x13u16.to_le_bytes().to_vec();
    empty_array.extend_from_slice(&0u32.to_le_bytes());
    PresetChunk {
        id: preset_chunk::ID_PROGRAM,
        public_data: public,
        children: (0..3)
            .map(|_| PresetChunk {
                id: preset_chunk::ID_PARAMETER_ARRAY_8,
                public_data: empty_array.clone(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn program_fx_roles_keep_empty_arrays_and_reject_malformed_arrays() {
    let valid = program();
    let instrument = model::build(std::slice::from_ref(&valid)).unwrap();
    assert_eq!(instrument.fx_chains.len(), 3);
    assert!(instrument
        .fx_chains
        .iter()
        .all(|chain| chain.slots.is_empty()));
    for role in 0..3 {
        let mut malformed = valid.clone();
        malformed.children[role].public_data.truncate(1);
        assert!(
            model::build(&[malformed]).is_err(),
            "bad role {role} was dropped"
        );
    }
}

#[test]
fn ssl_geq_uses_recovered_control_curves_and_shelf_bands() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&0.9f32.to_le_bytes()); // LF gain unity -> +16.0 dB
    raw.extend_from_slice(&0.5f32.to_le_bytes()); // LF frequency -> 183.73 Hz
    raw.push(0);
    for value in [0.5f32; 6] {
        // LMF gain/frequency/Q, HMF gain/frequency/Q
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&0.636f32.to_le_bytes()); // HF gain
    raw.extend_from_slice(&0.45f32.to_le_bytes()); // HF frequency -> 6140 Hz
    raw.push(0);
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.push(0);
    raw.push(0);
    let child = nk_format::fx::FxChild {
        ser_type: 68,
        version: 0x11,
        private: Vec::new(),
        public: Vec::new(),
        children: Vec::new(),
        raw,
    };
    let bands = child.ssl_bands().unwrap();
    assert_eq!(bands.len(), 4);
    assert_eq!(bands[0].kind, nk_format::fx::SslBandKind::LowShelf);
    assert!((bands[0].frequency - 183.73).abs() < 0.01);
    assert!((bands[0].gain_db - 16.0).abs() < 0.01);
    assert_eq!(bands[1].kind, nk_format::fx::SslBandKind::Peaking);
    assert!((bands[1].frequency - 809.0).abs() < 0.01);
    assert!((bands[1].q - 0.4).abs() < 1e-6);
    assert!((bands[2].frequency - 2619.29).abs() < 0.01);
    assert_eq!(bands[3].kind, nk_format::fx::SslBandKind::HighShelf);
    assert!((bands[3].frequency - 6140.0).abs() < 0.01);
    assert!((bands[3].gain_db - 3.44).abs() < 0.05);
}

#[test]
fn amp_wire_layouts_follow_the_recovered_doread_readers() {
    fn child(ser_type: u16, version: u16, raw: Vec<u8>) -> nk_format::fx::FxChild {
        nk_format::fx::FxChild {
            ser_type,
            version,
            private: Vec::new(),
            public: Vec::new(),
            children: Vec::new(),
            raw,
        }
    }
    let floats = |count: usize| -> Vec<u8> {
        (0..count)
            .flat_map(|i| ((i as f32) * 0.1).to_le_bytes())
            .collect()
    };
    for (ser_type, version, value_count, flag_count, kind) in [
        (77, 0x10, 6, 2, nk_format::fx::AmpKind::Jump),
        (82, 0x10, 8, 5, nk_format::fx::AmpKind::Van51),
        (83, 0x10, 7, 1, nk_format::fx::AmpKind::AcBox),
        (84, 0x10, 8, 2, nk_format::fx::AmpKind::HotSolo),
        (84, 0x52, 7, 4, nk_format::fx::AmpKind::BassInvader),
        (85, 0x10, 7, 1, nk_format::fx::AmpKind::Cat),
        (97, 0x10, 17, 5, nk_format::fx::AmpKind::BassPro),
    ] {
        let mut raw = floats(value_count);
        for index in 0..flag_count {
            raw.push((index % 2) as u8);
        }
        let settings = child(ser_type, version, raw).amp_settings().unwrap();
        assert_eq!(settings.kind, kind);
        assert_eq!(settings.values().len(), value_count);
        assert_eq!(settings.flags().len(), flag_count);
        assert!(!settings.flags()[0]);
        if flag_count > 1 {
            assert!(settings.flags()[1]);
        }
    }

    let mut invader = floats(7);
    invader.extend_from_slice(&[1, 0, 1]);
    let settings = child(84, 0x51, invader).amp_settings().unwrap();
    assert_eq!(settings.kind, nk_format::fx::AmpKind::BassInvader);
    assert_eq!(settings.flags(), &[true, false, true, true]);

    let mut raw = floats(11);
    raw.extend_from_slice(&[1, 1]); // two flags, then four modes below
    for mode in [4i32, 5, 6, 4] {
        raw.extend_from_slice(&mode.to_le_bytes());
    }
    raw.extend_from_slice(&[1, 0, 1, 0]);
    let settings = child(108, 0x11, raw.clone()).amp_settings().unwrap();
    assert_eq!(settings.kind, nk_format::fx::AmpKind::EpPreamps);
    assert_eq!(settings.values().len(), 11);
    assert_eq!(settings.flags().len(), 6);
    assert_eq!(settings.modes, [4, 5, 6, 4]);

    let mut bad_mode = raw;
    bad_mode[11 * 4 + 2 + 12..11 * 4 + 2 + 16].copy_from_slice(&5i32.to_le_bytes());
    assert!(child(108, 0x11, bad_mode).amp_settings().is_none());

    let mut short = floats(5);
    short.push(0);
    assert!(child(77, 0x10, short).amp_settings().is_none());
    assert!(child(77, 0x11, floats(6)).amp_settings().is_none());
}
