//! HOTSOLO recovered circuit, including its oversampling and control clock.
//! Numeric expressions and branches are generated from decoded ARM64 data flow;
//! no Kontakt code is loaded or executed by this playback implementation.
#![allow(dead_code)]
use nk_format::fx::AmpSettings;
#[path = "hotsolo_numeric.rs"]
mod numeric;
const STATE: u64 = 0x10000000000;
const STACK: u64 = 0x20000000000;
const DATA: u64 = 0x30000000000;
const INPUT: u64 = 0x40000000000;
const CURVES: &[u8; 18432] = include_bytes!("hotsolo_curves.bin");
const CONSTANTS: &[u8] = include_bytes!("hotsolo_constants.bin");
fn fs(value: u128, lane: usize) -> f32 {
    f32::from_bits((value >> (lane * 32)) as u32)
}
fn fd(value: u128, lane: usize) -> f64 {
    f64::from_bits((value >> (lane * 64)) as u64)
}
fn lane(value: u128, lane: usize, width: usize, part: u128) -> u128 {
    let mask = (1u128 << width) - 1;
    (value & !(mask << (lane * width))) | ((part & mask) << (lane * width))
}
fn round_odd(value: f64) -> f32 {
    let rounded = value as f32;
    if rounded as f64 == value || !value.is_finite() {
        return rounded;
    }
    let mut bits = rounded.to_bits();
    if (rounded as f64).abs() > value.abs() {
        bits -= 1;
    }
    f32::from_bits(bits | 1)
}
#[cfg(target_arch = "aarch64")]
fn reciprocal_estimate(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vrecpe_f32};
    unsafe { vget_lane_f32::<0>(vrecpe_f32(vdup_n_f32(value))) }
}
#[cfg(not(target_arch = "aarch64"))]
fn reciprocal_estimate(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 255;
    if exponent == 0 || exponent >= 253 {
        return 1.0 / value;
    }
    let a = ((bits & 0x7fffff) | 0x800000) >> 15;
    let estimate = (((1u32 << 19) / (2 * a + 1)) + 1) / 2;
    f32::from_bits((bits & 0x80000000) | ((253 - exponent) << 23) | ((estimate & 255) << 15))
}
struct Registers {
    x: [u64; 33],
    v: [u128; 32],
    n: bool,
    z: bool,
    c: bool,
    o: bool,
}
impl Registers {
    fn new() -> Self {
        let mut r = Self {
            x: [0; 33],
            v: [0; 32],
            n: false,
            z: false,
            c: false,
            o: false,
        };
        r.x[31] = STACK + 0x4000;
        r
    }
    fn flags(&mut self, nzcv: u8) {
        self.n = nzcv & 8 != 0;
        self.z = nzcv & 4 != 0;
        self.c = nzcv & 2 != 0;
        self.o = nzcv & 1 != 0;
    }
    fn compare(&mut self, a: u64, b: u64, width: u32, add: bool) {
        let mask = if width == 32 { 0xffffffff } else { u64::MAX };
        let a = a & mask;
        let b = b & mask;
        let result = if add {
            a.wrapping_add(b)
        } else {
            a.wrapping_sub(b)
        } & mask;
        let sign = 1u64 << (width - 1);
        self.n = result & sign != 0;
        self.z = result == 0;
        self.c = if add {
            (a as u128 + b as u128) > (mask as u128)
        } else {
            a >= b
        };
        self.o = if add {
            (!(a ^ b) & (a ^ result) & sign) != 0
        } else {
            ((a ^ b) & (a ^ result) & sign) != 0
        };
    }
    fn float_compare(&mut self, a: f64, b: f64) {
        self.flags(if a.is_nan() || b.is_nan() {
            3
        } else if a < b {
            8
        } else if a == b {
            6
        } else {
            2
        });
    }
}
struct Memory {
    state: Box<[u8]>,
    stack: Box<[u8]>,
    data: Box<[u8]>,
    inputs: [u8; 16],
    exp: &'static [u8],
}
fn exp_table() -> &'static [u8] {
    use std::sync::OnceLock;
    static EXP: OnceLock<Box<[u8]>> = OnceLock::new();
    EXP.get_or_init(|| {
        let mut table = vec![0u8; 2401 * 8];
        for i in 0..2401 {
            let a = 2f64.powf((i as f64 - 1200.0) / 60.0) as f32;
            let b = 2f64.powf((i as f64 + 1.0 - 1200.0) / 60.0) as f32;
            table[i * 8..i * 8 + 4].copy_from_slice(&a.to_le_bytes());
            table[i * 8 + 4..i * 8 + 8].copy_from_slice(&(b - a).to_le_bytes());
        }
        table.into_boxed_slice()
    })
    .as_ref()
}
impl Memory {
    /// Allocate a duplicate on the loading/preparation thread only.
    fn duplicate_prepared(&self) -> Self {
        Self {
            state: self.state.to_vec().into_boxed_slice(),
            stack: self.stack.to_vec().into_boxed_slice(),
            data: self.data.to_vec().into_boxed_slice(),
            inputs: self.inputs,
            exp: self.exp,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    fn copy_prepared_from(&mut self, template: &Self) {
        self.state[..].copy_from_slice(&template.state[..]);
        self.stack[..].copy_from_slice(&template.stack[..]);
        self.data[..].copy_from_slice(&template.data[..]);
        self.inputs = template.inputs;
        self.exp = template.exp;
    }

    fn new() -> Self {
        Self {
            state: vec![0; 0x8000].into_boxed_slice(),
            stack: vec![0; 0x4000].into_boxed_slice(),
            data: vec![0; 0x12000].into_boxed_slice(),
            inputs: [0; 16],
            exp: exp_table(),
        }
    }
    fn read(&self, addr: u64, size: usize) -> u128 {
        let source = if (STATE..STATE + 0x8000).contains(&addr) {
            &self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x4000).contains(&addr) {
            &self.stack[(addr - STACK) as usize..]
        } else if (DATA + 0x9c000..DATA + 0xae000).contains(&addr) {
            &self.data[(addr - DATA - 0x9c000) as usize..]
        } else if (INPUT..INPUT + 16).contains(&addr) {
            &self.inputs[(addr - INPUT) as usize..]
        } else if (0x109dc2200..0x109dc2200 + 2401 * 8).contains(&addr) {
            &self.exp[(addr - 0x109dc2200) as usize..]
        } else if (0x104d46700..0x104d4af00).contains(&addr) {
            &CURVES[(addr - 0x104d46700) as usize..]
        } else {
            let page = addr & !4095;
            let index = numeric::CONSTANT_PAGES
                .iter()
                .position(|p| *p == page)
                .unwrap_or_else(|| panic!("unmapped recovered constant {addr:x}"));
            &CONSTANTS[index * 4096 + (addr & 4095) as usize..]
        };
        let mut value = [0; 16];
        value[..size].copy_from_slice(&source[..size]);
        u128::from_le_bytes(value)
    }
    fn write(&mut self, addr: u64, size: usize, value: u128) {
        let target = if (STATE..STATE + 0x8000).contains(&addr) {
            &mut self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x4000).contains(&addr) {
            &mut self.stack[(addr - STACK) as usize..]
        } else if (DATA + 0x9c000..DATA + 0xae000).contains(&addr) {
            &mut self.data[(addr - DATA - 0x9c000) as usize..]
        } else if (INPUT..INPUT + 16).contains(&addr) {
            &mut self.inputs[(addr - INPUT) as usize..]
        } else {
            panic!("unmapped recovered write {addr:x}")
        };
        target[..size].copy_from_slice(&value.to_le_bytes()[..size]);
    }
    fn f32(&self, offset: usize) -> f32 {
        f32::from_bits(self.read(STATE + offset as u64, 4) as u32)
    }
    fn put(&mut self, offset: usize, value: f32) {
        self.write(STATE + offset as u64, 4, value.to_bits() as u128);
    }
    fn zero(&mut self, addr: u64, size: usize) {
        for i in 0..size {
            self.write(addr + i as u64, 1, 0);
        }
    }
    fn copy(&mut self, dst: u64, src: u64, size: usize) {
        for i in 0..size {
            let value = self.read(src + i as u64, 1);
            self.write(dst + i as u64, 1, value);
        }
    }
    fn anticipate_volume(&mut self, addr: u64) {
        let a = self.read(addr + 0x1a4, 8);
        self.write(addr + 0x1ac, 8, a);
    }
    fn setup_table(&mut self, r: &Registers) {
        let index = ((r.x[0] - STATE - 0x27d0) / 0x820) as usize;
        let table = crate::amp_dsp::TableBase::setup_with(
            CURVES,
            index,
            fs(r.v[0], 0),
            fs(r.v[1], 0),
            fs(r.v[2], 0),
            r.x[1] & 1 != 0,
            r.x[2] & 1 != 0,
        );
        let addr = r.x[0];
        self.write(addr + 8, 4, table.anchor as u32 as u128);
        for (at, value) in [
            (12, table.fraction),
            (16, table.scale),
            (20, table.offset),
            (24, table.baseline),
        ] {
            self.write(addr + at, 4, value.to_bits() as u128);
        }
        for (i, entry) in table.entries.iter().enumerate() {
            self.write(addr + 28 + (i * 8) as u64, 4, entry[0].to_bits() as u128);
            self.write(addr + 32 + (i * 8) as u64, 4, entry[1].to_bits() as u128);
        }
    }
}

pub(crate) struct HotSoloAmp {
    memory: Memory,
    settings: AmpSettings,
    rate: f32,
    slice_remaining: usize,
}
impl HotSoloAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            memory: self.memory.duplicate_prepared(),
            settings: self.settings,
            rate: self.rate,
            slice_remaining: self.slice_remaining,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.memory.copy_prepared_from(&template.memory);
        self.settings = template.settings;
        self.rate = template.rate;
        self.slice_remaining = template.slice_remaining;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let mut amp = Self {
            memory: Memory::new(),
            settings,
            rate: 48000.0,
            slice_remaining: 0,
        };
        amp.prepare(48000.0).expect("valid HOTSOLO rate");
        amp
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        self.memory.state.fill(0);
        self.memory.stack.fill(0);
        self.memory.data.fill(0);
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::construct(&mut self.memory, &mut r);
        r = Registers::new();
        r.x[0] = STATE;
        r.v[0] = rate.to_bits() as u128;
        numeric::initialize(&mut self.memory, &mut r);
        self.memory.write(STATE + 0x124, 4, 2);
        self.memory.write(STATE + 0x20, 8, INPUT as u128);
        self.memory.write(STATE + 0x28, 8, (INPUT + 8) as u128);
        self.rate = rate;
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::reset(&mut self.memory, &mut r);
        self.slice_remaining = 0;
    }
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        let mut r = Registers::new();
        r.x[0] = STATE;
        r.x[1] = settings.flags().first().copied().unwrap_or(false) as u64;
        r.x[2] = settings.flags().get(1).copied().unwrap_or(false) as u64;
        for (arg, index) in [0, 1, 7, 6, 5, 2, 3, 4].into_iter().enumerate() {
            r.v[arg] = settings
                .values()
                .get(index)
                .copied()
                .unwrap_or(0.5)
                .clamp(0.0, 1.0)
                .to_bits() as u128;
        }
        numeric::set_controls(&mut self.memory, &mut r);
        self.settings = settings;
    }
    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        let mut r = Registers::new();
        if self.slice_remaining == 0 {
            r.x[0] = STATE;
            numeric::control_rate(&mut self.memory, &mut r);
            self.slice_remaining = 32;
        }
        self.memory.write(INPUT, 4, left.to_bits() as u128);
        self.memory.write(INPUT + 8, 4, right.to_bits() as u128);
        r = Registers::new();
        r.x[0] = STATE;
        r.x[1] = DATA;
        r.x[2] = 0;
        r.x[3] = 1;
        r.x[4] = 0;
        numeric::process(&mut self.memory, &mut r);
        let left = f32::from_bits(self.memory.read(DATA + 0x9c010, 4) as u32);
        let right = if self.memory.state[0x1c0] != 0 {
            left
        } else {
            f32::from_bits(self.memory.read(DATA + 0x9d010, 4) as u32)
        };
        self.slice_remaining -= 1;
        [left, right]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;
    fn settings(values: [f32; 8], flags: [bool; 2]) -> AmpSettings {
        let mut s = AmpSettings {
            kind: AmpKind::HotSolo,
            values: [0.; 17],
            value_count: 8,
            flags: [false; 8],
            flag_count: 2,
            modes: [0; 4],
        };
        s.values[..8].copy_from_slice(&values);
        s.flags[..2].copy_from_slice(&flags);
        s
    }
    fn normalized(m: &Memory) -> Vec<u8> {
        let mut b = m.state.to_vec();
        for at in [0, 0x20, 0x28]
            .into_iter()
            .chain((0..9).map(|i| 0x27d0 + i * 0x820))
            .chain((0x77d0..0x7828).step_by(8))
        {
            b[at..at + 8].fill(0);
        }
        b
    }
    fn number(bytes: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    #[test]
    fn hotsolo_original_arm64_initialization_and_audio() {
        let bytes = include_bytes!("../tests/fixtures/hotsolo-native.bin");
        assert_eq!(&bytes[..8], b"HSNATV01");
        let cases = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let frames = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let mut cursor = 16;
        let mut max_error = 0f32;
        for case in 0..cases {
            let rate = number(bytes, cursor);
            let values = std::array::from_fn(|i| number(bytes, cursor + 4 + i * 4));
            let flags = [bytes[cursor + 36] != 0, bytes[cursor + 37] != 0];
            cursor += 40;
            let mut amp = HotSoloAmp::new(settings(values, flags));
            amp.prepare(rate).unwrap();
            let actual = normalized(&amp.memory);
            let expected = &bytes[cursor..cursor + 0x8000];
            cursor += 0x8000;
            assert!(actual == expected, "initial snapshot case {case}");
            for at in (0x1d0..0x784c).step_by(4) {
                if (0x27d0..0x70f0).contains(&at) {
                    continue;
                }
                let a = number(&actual, at);
                let e = number(expected, at);
                assert!(
                    a == e || (a - e).abs() <= 2e-6 * (1. + e.abs()),
                    "init case {case} offset {at:x}: {a} vs {e}"
                );
            }
            for frame in 0..frames {
                if frame == 128 {
                    amp.set_settings(settings(
                        [0.78, 0.24, 0.63, 0.48, 0.9, 0.17, 0.37, 0.61],
                        [!flags[0], flags[1]],
                    ));
                }
                let input = [number(bytes, cursor), number(bytes, cursor + 4)];
                let expected = [number(bytes, cursor + 8), number(bytes, cursor + 12)];
                cursor += 16;
                let actual = amp.process(input[0], input[1]);
                for channel in 0..2 {
                    let error = (actual[channel] - expected[channel]).abs();
                    max_error = max_error.max(error);
                    assert!(
                        actual[channel].is_finite()
                            && error <= 3e-6 * (1. + expected[channel].abs()),
                        "case {case} rate {rate} frame {frame} channel {channel}: {} vs {}",
                        actual[channel],
                        expected[channel]
                    );
                }
            }
            let actual = normalized(&amp.memory);
            let expected = &bytes[cursor..cursor + 0x8000];
            for at in (0x1d0..0x784c).step_by(4) {
                if (0x27d0..0x70f0).contains(&at) {
                    continue;
                }
                let a = number(&actual, at);
                let e = number(expected, at);
                assert!(
                    a.to_bits() == e.to_bits() || (a - e).abs() <= 3e-6 * (1. + e.abs()),
                    "final case {case} offset {at:x}: {a} vs {e}"
                );
            }
            cursor += 0x8000;
        }
        assert_eq!(cursor, bytes.len());
        eprintln!(
            "HOTSOLO_NATIVE cases={cases} frames={} max_error={max_error}",
            cases * frames
        );
    }
    #[test]
    fn hotsolo_reset_rates_and_channel_layout_are_deterministic() {
        for rate in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            let wire = settings([0.47; 8], [true, false]);
            let mut amp = HotSoloAmp::new(wire);
            amp.prepare(rate).unwrap();
            let input = |i: usize| {
                [
                    (i as f32 * 0.07).sin() * 0.2,
                    (i as f32 * 0.043).cos() * 0.1,
                ]
            };
            let first: Vec<_> = (0..256)
                .map(|i| {
                    let x = input(i);
                    let y = amp.process(x[0], x[1]);
                    assert!(y.iter().all(|v| v.is_finite()));
                    y
                })
                .collect();
            amp.reset();
            for (i, expected) in first.into_iter().enumerate() {
                let x = input(i);
                assert_eq!(amp.process(x[0], x[1]), expected);
            }
            for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                assert!(amp.prepare(rate).is_err());
            }
        }
        let mut mono = HotSoloAmp::new(settings([0.5; 8], [false, true]));
        let mut silence = HotSoloAmp::new(settings([0.5; 8], [false, true]));
        let mut stereo = HotSoloAmp::new(settings([0.5; 8], [false, false]));
        let mut stereo_silence = HotSoloAmp::new(settings([0.5; 8], [false, false]));
        let mut difference = 0f32;
        for frame in 0..512 {
            let x = (frame as f32 * 0.07).sin() * 0.2;
            let out = mono.process(x, -x);
            assert_eq!(out, silence.process(0.0, 0.0));
            assert_eq!(out[0], out[1]);
            let out = stereo.process(x, 0.0);
            let dry = stereo_silence.process(0.0, 0.0);
            assert_eq!(out[1], dry[1]);
            difference = difference.max((out[0] - dry[0]).abs());
        }
        assert!(difference > 1e-5);
    }
}
