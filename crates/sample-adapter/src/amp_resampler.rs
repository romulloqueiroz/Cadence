//! Allocation-free frame adapter for the recovered NI integer-ratio converter.
//! Configuration snapshots contain canonical numeric state and filter kernels;
//! processing uses translated Rust arithmetic and never loads native code.
#[path = "amp_resampler_numeric.rs"]
mod numeric;
const STATE: u64 = 0x10000000000;
const STACK: u64 = 0x20000000000;
const IO: u64 = 0x30000000000;
const CONSTANTS: &[u8] = include_bytes!("amp_resampler_constants.bin");
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
#[allow(dead_code)]
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
#[allow(dead_code)]
#[cfg(target_arch = "aarch64")]
fn reciprocal_estimate(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vrecpe_f32};
    unsafe { vget_lane_f32::<0>(vrecpe_f32(vdup_n_f32(value))) }
}
#[allow(dead_code)]
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

#[allow(dead_code)]
fn rsqrt_estimate(value: f32) -> f32 {
    1.0 / value.sqrt()
}
struct Memory {
    state: Box<[u8]>,
    stack: Box<[u8]>,
    io: [u8; 0x1400],
}
impl Memory {
    /// Allocate a duplicate on the loading/preparation thread only.
    fn duplicate_prepared(&self) -> Self {
        Self {
            state: self.state.to_vec().into_boxed_slice(),
            stack: self.stack.to_vec().into_boxed_slice(),
            io: self.io,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    fn copy_prepared_from(&mut self, template: &Self) {
        self.state[..].copy_from_slice(&template.state[..]);
        self.stack[..].copy_from_slice(&template.stack[..]);
        self.io = template.io;
    }

    fn read(&self, addr: u64, size: usize) -> u128 {
        let source = if (STATE..STATE + self.state.len() as u64).contains(&addr) {
            &self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + self.stack.len() as u64).contains(&addr) {
            &self.stack[(addr - STACK) as usize..]
        } else if (IO..IO + 0x1400).contains(&addr) {
            &self.io[(addr - IO) as usize..]
        } else {
            let page = addr & !4095;
            let index = numeric::CONSTANT_PAGES
                .iter()
                .position(|p| *p == page)
                .unwrap_or_else(|| panic!("unmapped SRC constant {addr:x}"));
            &CONSTANTS[index * 4096 + (addr & 4095) as usize..]
        };
        let mut value = [0; 16];
        value[..size].copy_from_slice(&source[..size]);
        u128::from_le_bytes(value)
    }
    fn write(&mut self, addr: u64, size: usize, value: u128) {
        let target = if (STATE..STATE + self.state.len() as u64).contains(&addr) {
            &mut self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + self.stack.len() as u64).contains(&addr) {
            &mut self.stack[(addr - STACK) as usize..]
        } else if (IO..IO + 0x1400).contains(&addr) {
            &mut self.io[(addr - IO) as usize..]
        } else {
            panic!("unmapped SRC write {addr:x}")
        };
        target[..size].copy_from_slice(&value.to_le_bytes()[..size]);
    }
    fn copy(&mut self, dst: u64, src: u64, size: usize) {
        if dst > src && dst < src + size as u64 {
            for i in (0..size).rev() {
                let b = self.read(src + i as u64, 1);
                self.write(dst + i as u64, 1, b);
            }
        } else {
            for i in 0..size {
                let b = self.read(src + i as u64, 1);
                self.write(dst + i as u64, 1, b);
            }
        }
    }
    fn zero(&mut self, at: u64, size: usize) {
        for i in 0..size {
            self.write(at + i as u64, 1, 0);
        }
    }
    fn state_f32(&self, at: u64) -> f32 {
        f32::from_bits(self.read(STATE + at, 4) as u32)
    }
    fn state_i32(&self, at: u64) -> i32 {
        self.read(STATE + at, 4) as u32 as i32
    }
}
fn dispatch(m: &mut Memory, r: &mut Registers, address: u64) {
    if numeric::run(m, r, address) {
        return;
    }
    match address {
        0x104349aa0 => m.zero(r.x[0], r.x[1] as usize),
        0x10434a4f0 | 0x10434a4fc => {
            m.copy(r.x[0], r.x[1], r.x[2] as usize);
        }
        _ => panic!("unresolved recovered SRC call {address:x}"),
    }
}
pub(crate) struct AmpResampler {
    memory: Memory,
    bypass: bool,
    // The current amp wrappers use the block adapter; retain the verified
    // single-frame adapter for callers that need its downsampling buffer.
    #[allow(dead_code)]
    pending: [[f32; 2]; 16],
    pending_len: usize,
    batch_frames: usize,
}
impl AmpResampler {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            memory: self.memory.duplicate_prepared(),
            bypass: self.bypass,
            pending: self.pending,
            pending_len: self.pending_len,
            batch_frames: self.batch_frames,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.memory.copy_prepared_from(&template.memory);
        self.bypass = template.bypass;
        self.pending = template.pending;
        self.pending_len = template.pending_len;
        self.batch_frames = template.batch_frames;
    }

    pub(crate) fn new(factor: f32) -> Result<Self, &'static str> {
        if !factor.is_finite() || factor <= 0. {
            return Err("invalid amp converter ratio");
        }
        let power = factor.log2();
        if power.fract() != 0. || !(-4.0..=4.0).contains(&power) {
            return Err("unsupported amp converter ratio");
        }
        let state = configuration(power as i32).to_vec().into_boxed_slice();
        let mut result = Self {
            memory: Memory {
                state,
                stack: vec![0; 0x4000].into_boxed_slice(),
                io: [0; 0x1400],
            },
            bypass: factor == 1.,
            pending: [[0.; 2]; 16],
            pending_len: 0,
            batch_frames: if factor < 1. {
                (1. / factor) as usize
            } else {
                1
            },
        };
        result.reset();
        Ok(result)
    }
    pub(crate) fn reset(&mut self) {
        self.pending_len = 0;
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::reset(&mut self.memory, &mut r);
    }

    /// Stream one input frame. Integer downsampling collects a whole ratio
    /// before calling the native-equivalent kernel, which requires an output.
    #[allow(dead_code)]
    pub(crate) fn process_frame(&mut self, input: [f32; 2], output: &mut [[f32; 2]; 16]) -> usize {
        self.pending[self.pending_len] = input;
        self.pending_len += 1;
        if self.pending_len < self.batch_frames {
            return 0;
        }
        let inputs = self.pending;
        self.pending_len = 0;
        let mut buffer = [[0.; 2]; 512];
        let count = self.process_frames(&inputs[..self.batch_frames], &mut buffer);
        output[..count].copy_from_slice(&buffer[..count]);
        count
    }
    /// Convert a whole block. Downsampling blocks contain a whole integer
    /// ratio; use `process_frame` to collect individual source frames.
    pub(crate) fn process_frames(
        &mut self,
        input: &[[f32; 2]],
        output: &mut [[f32; 2]; 512],
    ) -> usize {
        assert!(
            input.len() <= 32,
            "amp converter accepts at most 32 source frames"
        );
        assert!(
            input.len() % self.batch_frames == 0,
            "amp converter requires whole downsampling ratio blocks"
        );
        if self.bypass {
            output[..input.len()].copy_from_slice(input);
            return input.len();
        }
        if input.is_empty() {
            return 0;
        }
        // Exact frame count from BEffectBase::ProcessChannels (0x1005c9614).
        let m = &mut self.memory;
        let whole = m.state_i32(0x18) as f32;
        let numerator = m.state_f32(0x1c);
        let denominator = m.state_f32(0x20);
        let count = (((((input.len() as i32) - m.state_i32(0xb4) + 1) as f32) * denominator
            - m.state_f32(0xb8))
            / (numerator + denominator * whole)) as usize;
        assert!(count <= 512, "recovered SRC output exceeds frame capacity");
        for channel in 0..2 {
            m.write(
                IO + channel as u64 * 8,
                8,
                (IO + 0x100 + channel as u64 * 0x100) as u128,
            );
            m.write(
                IO + 0x10 + channel as u64 * 8,
                8,
                (IO + 0x400 + channel as u64 * 0x800) as u128,
            );
        }
        for (frame, samples) in input.iter().enumerate() {
            for (channel, value) in samples.iter().enumerate() {
                m.write(
                    IO + 0x100 + channel as u64 * 0x100 + frame as u64 * 4,
                    4,
                    value.to_bits() as u128,
                );
            }
        }
        let mut r = Registers::new();
        r.x[0] = STATE;
        r.x[1] = IO;
        r.x[2] = input.len() as u64;
        r.x[3] = IO + 0x10;
        r.x[4] = count as u64;
        r.x[5] = 2;
        numeric::convert(m, &mut r);
        for (frame, target) in output.iter_mut().enumerate().take(count) {
            for (channel, sample) in target.iter_mut().enumerate() {
                *sample = f32::from_bits(
                    m.read(IO + 0x400 + channel as u64 * 0x800 + frame as u64 * 4, 4) as u32,
                );
            }
        }
        count
    }
}

fn configuration(exponent: i32) -> &'static [u8] {
    const DATA: &[u8] = include_bytes!("amp_resampler_initial.bin");
    const INDEX: [(usize, usize); 9] = [
        (0, 7808),
        (7808, 6272),
        (14080, 5504),
        (19584, 5120),
        (24704, 4096),
        (28800, 5120),
        (33920, 5504),
        (39424, 6272),
        (45696, 7808),
    ];
    let (at, size) = INDEX[(exponent + 4) as usize];
    &DATA[at..at + size]
}
#[cfg(test)]
mod tests {
    use super::*;
    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    }
    fn sample(data: &[u8], at: usize) -> [f32; 2] {
        [
            f32::from_bits(u32_at(data, at)),
            f32::from_bits(u32_at(data, at + 4)),
        ]
    }
    #[test]
    fn resampler_matches_original_native_blocks() {
        let fixture = include_bytes!("../tests/fixtures/amp-resampler-native.bin");
        assert_eq!(&fixture[..8], b"SRCNAT01");
        let cases = u32_at(fixture, 8);
        let mut cursor = 12;
        let mut worst = 0.0f32;
        for _ in 0..cases {
            let exponent = u32_at(fixture, cursor) as i32;
            let state_size = u32_at(fixture, cursor + 4) as usize;
            let frames = u32_at(fixture, cursor + 8) as usize;
            cursor += 12;
            assert_eq!(
                configuration(exponent),
                &fixture[cursor..cursor + state_size]
            );
            cursor += state_size;
            let blocks = u32_at(fixture, cursor);
            cursor += 4;
            let mut src = AmpResampler::new(2f32.powi(exponent)).unwrap();
            let mut buffer = [[0.; 2]; 512];
            let mut input = [[0.; 2]; 32];
            for block in 0..blocks {
                for (i, s) in input.iter_mut().enumerate().take(frames) {
                    *s = sample(fixture, cursor + i * 8);
                }
                cursor += frames * 8;
                let count = u32_at(fixture, cursor) as usize;
                cursor += 4;
                let actual = src.process_frames(&input[..frames], &mut buffer);
                assert_eq!(actual, count, "ratio exponent {exponent} block {block}");
                for (i, out) in buffer.iter().enumerate().take(count) {
                    let expected = sample(fixture, cursor + i * 8);
                    for c in 0..2 {
                        worst = worst.max((out[c] - expected[c]).abs());
                        assert!((out[c]-expected[c]).abs()<2e-6,"ratio exponent {exponent} block {block} frame {i} channel {c}: {} vs {} worst {worst}",out[c],expected[c]);
                    }
                }
                cursor += count * 8;
            }
            src.reset();
            let _ = src.process_frames(&input[..frames], &mut buffer);
        }
        assert_eq!(cursor, fixture.len());
    }
    #[test]
    fn resampler_matches_native_minimal_streaming_batches() {
        let fixture = include_bytes!("../tests/fixtures/amp-resampler-stream.bin");
        assert_eq!(&fixture[..8], b"SRCSTR01");
        let mut cursor = 12;
        for _ in 0..u32_at(fixture, 8) {
            let exponent = u32_at(fixture, cursor) as i32;
            let batch = u32_at(fixture, cursor + 4) as usize;
            let blocks = u32_at(fixture, cursor + 8);
            cursor += 12;
            let mut converter = AmpResampler::new(2f32.powi(exponent)).unwrap();
            let mut output = [[0.; 2]; 16];
            for block in 0..blocks {
                for i in 0..batch {
                    let input = sample(fixture, cursor);
                    cursor += 8;
                    let count = converter.process_frame(input, &mut output);
                    if i + 1 < batch {
                        assert_eq!(count, 0);
                    } else {
                        let expected = u32_at(fixture, cursor) as usize;
                        cursor += 4;
                        assert_eq!(count, expected);
                        for (frame, actual) in output.iter().enumerate().take(count) {
                            let want = sample(fixture, cursor + frame * 8);
                            for channel in 0..2 {
                                assert!((actual[channel]-want[channel]).abs()<2e-6,"exponent {exponent} block {block} frame {frame} channel {channel}: {} vs {}",actual[channel],want[channel]);
                            }
                        }
                        cursor += count * 8;
                    }
                }
            }
        }
        assert_eq!(cursor, fixture.len());
    }
    #[test]
    fn resampler_reset_repeats_and_invalid_ratios_are_rejected() {
        for factor in [0., -1., f32::NAN, f32::INFINITY, 0.03, 32., 1.5] {
            assert!(AmpResampler::new(factor).is_err());
        }
        let input: [[f32; 2]; 32] = std::array::from_fn(|i| {
            [
                (i as f32 * 0.073).sin() * 0.2,
                (i as f32 * 0.037).cos() * 0.1,
            ]
        });
        for exponent in -4..=4 {
            let mut src = AmpResampler::new(2f32.powi(exponent)).unwrap();
            let mut expected = [[0.; 2]; 512];
            let count = src.process_frames(&input, &mut expected);
            let mut actual = [[0.; 2]; 512];
            for _ in 0..3 {
                let _ = src.process_frames(&input, &mut actual);
            }
            src.reset();
            assert_eq!(src.process_frames(&input, &mut actual), count);
            assert_eq!(
                &actual[..count],
                &expected[..count],
                "reset exponent {exponent}"
            );
        }
    }
}
