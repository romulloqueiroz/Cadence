//! BassInvader neural circuit recovered from the original numeric data flow.
//! Register values are decoded arithmetic, never executable target code.
#![allow(dead_code)]
use nk_format::fx::AmpSettings;
#[path = "bassinvader_numeric.rs"]
mod numeric;
const STATE: u64 = 0x10000000000;
const STACK: u64 = 0x20000000000;
const INPUT: u64 = 0x30000000000;
const CONSTANTS: &[u8] = include_bytes!("bassinvader_constants.bin");
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

#[cfg(target_arch = "aarch64")]
fn rsqrt_estimate(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vrsqrte_f32};
    unsafe { vget_lane_f32::<0>(vrsqrte_f32(vdup_n_f32(value))) }
}
#[cfg(not(target_arch = "aarch64"))]
fn rsqrt_estimate(value: f32) -> f32 {
    scalar_rsqrt_estimate(value)
}
/// Integer ARM reciprocal-square-root estimate table, independent of host SIMD.
#[cfg(any(test, not(target_arch = "aarch64")))]
fn scalar_rsqrt_estimate(value: f32) -> f32 {
    const fn table() -> [u16; 256] {
        let mut out = [0; 256];
        let mut i = 0;
        while i < 256 {
            let a = 128 + (i % 128) as u64;
            let odd = 2 * a + 1;
            let numerator = if i < 128 { 67108864u64 } else { 33554432u64 };
            let mut low = 256u64;
            let mut high = 512u64;
            while low + 1 < high {
                let mid = (low + high) / 2;
                if mid * mid * odd <= numerator {
                    low = mid
                } else {
                    high = mid
                }
            }
            if odd * (2 * low + 1) * (2 * low + 1) <= 4 * numerator {
                low += 1
            }
            out[i] = low as u16;
            i += 1;
        }
        out
    }
    const TABLE: [u16; 256] = table();
    let bits = value.to_bits();
    let exponent = ((bits >> 23) & 255) as i32;
    if value == 0.0 {
        return f32::INFINITY.copysign(value);
    }
    if value < 0.0 {
        return f32::NAN;
    }
    if !value.is_finite() || exponent == 0 {
        return 1.0 / value.sqrt();
    }
    let e = exponent - 127;
    let index = (e & 1) as usize * 128 + ((bits & 0x7fffff) >> 16) as usize;
    let q = TABLE[index] as u32;
    f32::from_bits((((126 - e.div_euclid(2)) as u32) << 23) | ((q - 256) << 15))
}
#[cfg(target_os = "macos")]
fn exp10(value: f32) -> f32 {
    unsafe extern "C" {
        fn __exp10f(value: f32) -> f32;
    }
    unsafe { __exp10f(value) }
}
#[cfg(not(target_os = "macos"))]
fn exp10(value: f32) -> f32 {
    10f32.powf(value)
}
struct Memory {
    state: Box<[u8]>,
    stack: Box<[u8]>,
    io: [u8; 512],
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

    fn new() -> Self {
        Self {
            state: vec![0; 0x5000].into_boxed_slice(),
            stack: vec![0; 0x10000].into_boxed_slice(),
            io: [0; 512],
        }
    }
    fn read(&self, addr: u64, size: usize) -> u128 {
        if addr == 0x109636c10 {
            return (INPUT + 496) as u128;
        }
        let source = if (STATE..STATE + 0x5000).contains(&addr) {
            &self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x10000).contains(&addr) {
            &self.stack[(addr - STACK) as usize..]
        } else if (INPUT..INPUT + 512).contains(&addr) {
            &self.io[(addr - INPUT) as usize..]
        } else {
            let page = addr & !4095;
            let i = numeric::CONSTANT_PAGES
                .iter()
                .position(|p| *p == page)
                .unwrap_or_else(|| panic!("BassInvader unmapped constant {addr:x}"));
            &CONSTANTS[i * 4096 + (addr & 4095) as usize..]
        };
        let mut result = [0; 16];
        result[..size].copy_from_slice(&source[..size]);
        u128::from_le_bytes(result)
    }
    fn write(&mut self, addr: u64, size: usize, value: u128) {
        let target = if (STATE..STATE + 0x5000).contains(&addr) {
            &mut self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x10000).contains(&addr) {
            &mut self.stack[(addr - STACK) as usize..]
        } else if (INPUT..INPUT + 512).contains(&addr) {
            &mut self.io[(addr - INPUT) as usize..]
        } else {
            panic!("BassInvader unmapped write {addr:x}")
        };
        target[..size].copy_from_slice(&value.to_le_bytes()[..size]);
    }
    fn f(&self, a: usize) -> f32 {
        f32::from_bits(self.read(STATE + a as u64, 4) as u32)
    }
    fn put(&mut self, a: usize, v: f32) {
        self.write(STATE + a as u64, 4, v.to_bits() as u128)
    }
    fn finish_construct(&mut self) {
        self.write(STATE + 0x540, 4, 75);
        self.write(STATE + 0x29c, 4, 1);
        for i in 0..4 {
            self.put(0x2a0 + i * 4, 1.0)
        }
    }
    fn smoothing_construct(&mut self, addr: u64) {
        for i in 0..6 {
            let data = STATE + 0x4000 + i * 0x100;
            self.write(addr + i * 24, 8, data as u128);
            self.write(addr + i * 24 + 8, 8, (data + 48) as u128);
            self.write(addr + i * 24 + 16, 8, (data + 48) as u128)
        }
    }
    fn parameter_callback(&mut self, r: &mut Registers, address: u64) {
        match address {
            0x1019c2228 => numeric::parameter0(self, r),
            0x1019c2420 => numeric::parameter1(self, r),
            0x1019c25a4 => numeric::parameter2(self, r),
            0x1019c2728 => numeric::parameter3(self, r),
            0x1019c28ac => numeric::parameter4(self, r),
            0x1019c2998 => numeric::parameter5(self, r),
            0x1019c2a84 => numeric::parameter6(self, r),
            0x1019c2b64 => numeric::parameter7(self, r),
            0x1019c2ca4 => numeric::parameter8(self, r),
            0x1019c2d84 => numeric::parameter9(self, r),
            _ => panic!("BassInvader unknown callback {address:x}"),
        }
    }
}
const CALLBACKS: [u64; 10] = [
    0x1019c2228,
    0x1019c2420,
    0x1019c25a4,
    0x1019c2728,
    0x1019c2a84,
    0x1019c2b64,
    0x1019c2998,
    0x1019c28ac,
    0x1019c2ca4,
    0x1019c2d84,
];
pub(crate) struct BassInvaderCore {
    memory: Memory,
}
impl BassInvaderCore {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            memory: self.memory.duplicate_prepared(),
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.memory.copy_prepared_from(&template.memory);
    }

    pub(crate) fn new(rate: f32, parameters: [f32; 10]) -> Self {
        let mut c = Self {
            memory: Memory::new(),
        };
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::construct(&mut c.memory, &mut r);
        c.memory.put(0x530, rate);
        let mut r = Registers::new();
        r.x[0] = STATE;
        r.v[0] = rate.to_bits() as u128;
        numeric::rate(&mut c.memory, &mut r);
        c.set_parameters(parameters);
        c.configure_smoothing(rate, 32);
        c.reset();
        c
    }
    fn configure_smoothing(&mut self, rate: f32, block: usize) {
        let count = ((rate * 0.075) / (block as f32) + 0.5) as u32;
        let count = count.max(1);
        self.memory.write(STATE + 0x29c, 4, count as u128);
        for i in 0..4 {
            self.memory
                .put(0x2a0 + i * 4, 1.0 / (count as f32 * block as f32));
        }
    }
    fn set_parameters(&mut self, values: [f32; 10]) {
        for (i, v) in values.into_iter().enumerate() {
            self.memory.put(0x4c + i * 4, v);
            for array in 0..6 {
                self.memory.put(
                    0x4000 + array * 0x100 + i * 4,
                    if array == 1 || array == 5 { 0.0 } else { v },
                );
            }
            let object = STATE + 0x3100 + i as u64 * 16;
            let vt = STATE + 0x3300 + i as u64 * 64;
            self.memory
                .write(STATE + 0x90 + i as u64 * 32, 8, object as u128);
            self.memory.write(object, 8, vt as u128);
            self.memory.write(object + 8, 8, STATE as u128);
            self.memory.write(vt + 0x30, 8, CALLBACKS[i] as u128);
            let mut r = Registers::new();
            r.x[0] = object;
            self.memory.parameter_callback(&mut r, CALLBACKS[i]);
        }
        self.memory.write(STATE + 0x290, 8, 0);
    }
    fn set_targets(&mut self, values: [f32; 10]) {
        for (i, v) in values.into_iter().enumerate() {
            self.memory.put(0x4300 + i * 4, v);
        }
        self.memory.write(STATE + 0x294, 1, 1);
        self.memory.write(STATE + 0x290, 4, 1);
    }
    fn control(&mut self) {
        let m = &mut self.memory;
        let start = if m.read(STATE + 0x294, 1) != 0 {
            m.write(STATE + 0x294, 1, 0);
            true
        } else if m.read(STATE + 0x290, 4) != 0 {
            let remaining = (m.read(STATE + 0x298, 4) as u32).wrapping_sub(1);
            m.write(STATE + 0x298, 4, remaining as u128);
            if remaining == 0 {
                for i in 0..12 {
                    let value = m.f(0x4200 + i * 4);
                    m.put(0x4000 + i * 4, value);
                    m.put(0x4400 + i * 4, value);
                    m.put(0x4100 + i * 4, 0.0);
                    m.put(0x4500 + i * 4, 0.0);
                }
                m.write(STATE + 0x290, 4, 4);
                m.write(STATE + 0x295, 1, 0);
                true
            } else {
                let stage = m.read(STATE + 0x290, 4);
                if stage == 2 {
                    m.write(STATE + 0x290, 4, 3)
                } else if stage == 4 {
                    m.write(STATE + 0x290, 4, 0)
                }
                false
            }
        } else {
            false
        };
        if start {
            let mut r = Registers::new();
            r.x[0] = STATE + 0x200;
            numeric::start_smooth(m, &mut r);
        }
    }
    pub(crate) fn reset(&mut self) {
        for a in [0x1e44, 0x1e30, 0x1e58, 0x1e98, 0x1ed8, 0x1f18] {
            self.memory.write(STATE + a, 8, 0)
        }
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::stabilize(&mut self.memory, &mut r);
    }
    pub(crate) fn process(&mut self, input: [f32; 2]) -> f32 {
        for (i, v) in input.into_iter().enumerate() {
            self.memory.write(
                INPUT + i as u64 * 8,
                8,
                (INPUT + 256 + i as u64 * 4) as u128,
            );
            self.memory.write(
                INPUT + 16 + i as u64 * 8,
                8,
                (INPUT + 272 + i as u64 * 4) as u128,
            );
            self.memory
                .write(INPUT + 256 + i as u64 * 4, 4, v.to_bits() as u128)
        }
        let mut r = Registers::new();
        r.x[0] = STATE;
        r.x[1] = INPUT;
        r.x[2] = INPUT + 16;
        r.x[3] = 1;
        numeric::process(&mut self.memory, &mut r);
        f32::from_bits(self.memory.read(INPUT + 272, 4) as u32)
    }
}
/// BParFX SetObj routes seven wire floats and three switches to ten core values.
fn parameters(settings: AmpSettings) -> [f32; 10] {
    let mut v = [0.5; 7];
    for (a, b) in v.iter_mut().zip(settings.values()) {
        *a = b.clamp(0.0, 1.0)
    }
    let flags = settings.flags();
    let flag = |i| {
        if flags.get(i).copied().unwrap_or(false) {
            1.0
        } else {
            0.0
        }
    };
    [
        v[0],
        flag(0),
        flag(1),
        flag(2),
        v[2],
        v[3],
        v[4],
        v[1],
        v[5],
        v[6],
    ]
}
/// The wrapper's two independent mono cores, native target-rate selection and
/// native integer-ratio SRC. Downsampling queues complete input groups before
/// returning their output, preserving a causal one-frame callback API.
pub(crate) struct BassInvaderAmp {
    settings: AmpSettings,
    rate: f32,
    ratio: f32,
    cores: [BassInvaderCore; 2],
    input_src: [crate::amp_resampler::AmpResampler; 2],
    output_src: [crate::amp_resampler::AmpResampler; 2],
    up: [[f32; 2]; 512],
    down: [[[f32; 2]; 512]; 2],
    queue: [[f32; 2]; 32],
    queue_head: usize,
    queue_len: usize,
    remaining: usize,
    input_group: [[f32; 2]; 16],
    input_group_len: usize,
}
impl BassInvaderAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            rate: self.rate,
            ratio: self.ratio,
            cores: std::array::from_fn(|i| self.cores[i].duplicate_prepared()),
            input_src: std::array::from_fn(|i| self.input_src[i].duplicate_prepared()),
            output_src: std::array::from_fn(|i| self.output_src[i].duplicate_prepared()),
            up: self.up,
            down: self.down,
            queue: self.queue,
            queue_head: self.queue_head,
            queue_len: self.queue_len,
            remaining: self.remaining,
            input_group: self.input_group,
            input_group_len: self.input_group_len,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.rate = template.rate;
        self.ratio = template.ratio;
        for (state, source) in self.cores.iter_mut().zip(&template.cores) {
            state.copy_prepared_from(source);
        }
        for (state, source) in self.input_src.iter_mut().zip(&template.input_src) {
            state.copy_prepared_from(source);
        }
        for (state, source) in self.output_src.iter_mut().zip(&template.output_src) {
            state.copy_prepared_from(source);
        }
        self.up = template.up;
        self.down = template.down;
        self.queue = template.queue;
        self.queue_head = template.queue_head;
        self.queue_len = template.queue_len;
        self.remaining = template.remaining;
        self.input_group = template.input_group;
        self.input_group_len = template.input_group_len;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        Self::prepared(settings, 48000.0).expect("valid BassInvader initial rate")
    }
    fn prepared(settings: AmpSettings, rate: f32) -> Result<Self, &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        let ratio = 2f64.powf((88200.0 / f64::from(rate)).log2().round()) as f32;
        let input_src = [
            crate::amp_resampler::AmpResampler::new(ratio)?,
            crate::amp_resampler::AmpResampler::new(ratio)?,
        ];
        let output_src = [
            crate::amp_resampler::AmpResampler::new(1.0 / ratio)?,
            crate::amp_resampler::AmpResampler::new(1.0 / ratio)?,
        ];
        let internal_rate = (rate * ratio).round();
        let values = parameters(settings);
        let mut cores = std::array::from_fn(|_| BassInvaderCore::new(internal_rate, values));
        for core in &mut cores {
            core.configure_smoothing(internal_rate, (ratio * 32.0).round() as usize);
        }
        Ok(Self {
            settings,
            rate,
            ratio,
            cores,
            input_src,
            output_src,
            up: [[0.0; 2]; 512],
            down: [[[0.0; 2]; 512]; 2],
            queue: [[0.0; 2]; 32],
            queue_head: 0,
            queue_len: 0,
            remaining: 0,
            input_group: [[0.0; 2]; 16],
            input_group_len: 0,
        })
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        *self = Self::prepared(self.settings, rate)?;
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        for core in &mut self.cores {
            core.set_parameters(parameters(self.settings));
            core.reset();
        }
        for src in self.input_src.iter_mut().chain(self.output_src.iter_mut()) {
            src.reset();
        }
        self.queue_head = 0;
        self.queue_len = 0;
        self.remaining = 0;
        self.input_group_len = 0;
    }
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        if self.settings == settings {
            return;
        }
        let was_mono = self.settings.flags().get(3).copied().unwrap_or(false);
        let mono = settings.flags().get(3).copied().unwrap_or(false);
        let old_values = parameters(self.settings);
        let values = parameters(settings);
        self.settings = settings;
        if old_values != values {
            for core in &mut self.cores {
                core.set_targets(values);
            }
        }
        // SetMonoMode updates the reactivated right core's dependent values while
        // preserving its dormant circuit and converter histories.
        if was_mono && !mono {
            self.cores[1].set_parameters(values);
        }
    }
    pub(crate) fn process(&mut self, l: f32, r: f32) -> [f32; 2] {
        if self.remaining == 0 {
            for core in &mut self.cores {
                core.control()
            }
            self.remaining = 32;
        }
        self.input_group[self.input_group_len] = [l, r];
        self.input_group_len += 1;
        let group = if self.ratio < 1.0 {
            (1.0 / self.ratio) as usize
        } else {
            1
        };
        if self.input_group_len == group {
            self.input_group_len = 0;
            let mono = self.settings.flags().get(3).copied().unwrap_or(false);
            let channels = if mono { 1 } else { 2 };
            let mut produced = 0;
            for core in 0..channels {
                let mut input = [[0.0; 2]; 16];
                for (target, sample) in input.iter_mut().zip(&self.input_group[..group]) {
                    *target = if core == 1 {
                        [sample[1]; 2]
                    } else if mono {
                        *sample
                    } else {
                        [sample[0]; 2]
                    };
                }
                let count = self.input_src[core].process_frames(&input[..group], &mut self.up);
                for input in self.up.iter_mut().take(count) {
                    *input = [self.cores[core].process(*input); 2];
                }
                produced =
                    self.output_src[core].process_frames(&self.up[..count], &mut self.down[core]);
            }
            for i in 0..produced {
                assert!(self.queue_len < self.queue.len());
                let l = self.down[0][i][0];
                let r = if mono { l } else { self.down[1][i][1] };
                self.queue[(self.queue_head + self.queue_len) % self.queue.len()] = [l, r];
                self.queue_len += 1;
            }
        }
        self.remaining -= 1;
        if self.queue_len == 0 {
            return [0.0; 2];
        }
        let output = self.queue[self.queue_head];
        self.queue_head = (self.queue_head + 1) % self.queue.len();
        self.queue_len -= 1;
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn settings(mono: bool) -> AmpSettings {
        let mut s = AmpSettings {
            kind: nk_format::fx::AmpKind::BassInvader,
            values: [0.5; 17],
            value_count: 7,
            flags: [false; 8],
            flag_count: 4,
            modes: [0; 4],
        };
        s.flags[3] = mono;
        s
    }
    #[test]
    fn bassinvader_native_src_composition() {
        let blob = include_bytes!("../tests/fixtures/bassinvader-wrapper-native.bin");
        let mut at = 0;
        for rate in [44100.0, 48000.0, 96000.0, 192000.0] {
            for mono in [false, true] {
                let input = &blob[at..at + 8192];
                at += 8192;
                let expected = &blob[at..at + 8192];
                at += 8192;
                let mut amp = BassInvaderAmp::new(settings(mono));
                amp.prepare(rate).unwrap();
                let delay = if rate == 192000.0 { 1 } else { 0 };
                let f = |b: &[u8], i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
                let mut worst = 0.0f32;
                for frame in 0..1024 {
                    let out = amp.process(f(input, frame * 8), f(input, frame * 8 + 4));
                    for (c, &value) in out.iter().enumerate() {
                        let want = if frame < delay {
                            0.0
                        } else {
                            f(expected, (frame - delay) * 8 + c * 4)
                        };
                        worst = worst.max((value - want).abs());
                        assert!(value.is_finite()&&value.to_bits()==want.to_bits(),"rate{rate} mono{mono} frame{frame} channel{c}: {value} vs{want} worst{worst}");
                    }
                }
                eprintln!("BASSINVADER_SRC_NATIVE rate={rate} mono={mono} delay={delay} max_abs_error={worst}");
            }
        }
    }
    #[test]
    fn bassinvader_native_mode_routing_preserves_dormant_histories() {
        let blob = include_bytes!("../tests/fixtures/bassinvader-mode-native.bin");
        let mut at = 0;
        for rate in [48000.0, 192000.0] {
            let input = &blob[at..at + 8192];
            at += 8192;
            let expected = &blob[at..at + 8192];
            at += 8192;
            let mut amp = BassInvaderAmp::new(settings(false));
            amp.prepare(rate).unwrap();
            let delay = if rate == 192000.0 { 1 } else { 0 };
            let f = |b: &[u8], i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
            let mut worst = 0.0f32;
            for frame in 0..1024 {
                if frame == 256 {
                    amp.set_settings(settings(true));
                }
                if frame == 512 {
                    amp.set_settings(settings(false));
                }
                let out = amp.process(f(input, frame * 8), f(input, frame * 8 + 4));
                for (c, &value) in out.iter().enumerate() {
                    let want = if frame < delay {
                        0.0
                    } else {
                        f(expected, (frame - delay) * 8 + c * 4)
                    };
                    worst = worst.max((value - want).abs());
                    assert!(value.is_finite()&&value.to_bits()==want.to_bits(),"mode routing rate{rate} frame{frame} channel{c}: {value} vs{want} worst{worst}");
                }
            }
        }
    }
    #[test]
    fn bassinvader_wrapper_reset_and_mono() {
        let mut amp = BassInvaderAmp::new(settings(false));
        for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(amp.prepare(rate).is_err())
        }
        let first: Vec<_> = (0..512)
            .map(|i| amp.process((i as f32 * 0.071).sin() * 0.2, 0.0))
            .collect();
        amp.reset();
        for (i, want) in first.into_iter().enumerate() {
            assert_eq!(amp.process((i as f32 * 0.071).sin() * 0.2, 0.0), want)
        }
        let mut mono = BassInvaderAmp::new(settings(true));
        let mut baseline = BassInvaderAmp::new(settings(true));
        for i in 0..512 {
            let x = (i as f32 * 0.071).sin() * 0.2;
            let out = mono.process(x, -x);
            assert_eq!(out, baseline.process(0.0, 0.0));
            assert_eq!(out[0], out[1]);
        }
    }
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn bassinvader_scalar_rsqrt_estimate_matches_arm() {
        for i in 1..100000 {
            let value = 0.001 + i as f32 * 0.1234567;
            assert_eq!(
                scalar_rsqrt_estimate(value).to_bits(),
                rsqrt_estimate(value).to_bits(),
                "{value}"
            );
        }
    }
    #[test]
    fn bassinvader_extreme_controls_finite_reset_exact() {
        for rate in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            for value in [0.0, 0.5, 1.0] {
                let mut wire = settings(value == 1.0);
                wire.values[..7].fill(value);
                wire.flags[..3].fill(value == 1.0);
                let mut amp = BassInvaderAmp::new(wire);
                amp.prepare(rate).unwrap();
                let first: Vec<_> = (0..512)
                    .map(|i| {
                        let out = amp.process(
                            (i as f32 * 0.071).sin() * 0.7,
                            (i as f32 * 0.037).cos() * 0.3,
                        );
                        assert!(
                            out.iter().all(|x| x.is_finite() && x.abs() < 64.0),
                            "rate {rate} control{value}: {out:?}"
                        );
                        out
                    })
                    .collect();
                amp.reset();
                for (i, want) in first.into_iter().enumerate() {
                    assert_eq!(
                        amp.process(
                            (i as f32 * 0.071).sin() * 0.7,
                            (i as f32 * 0.037).cos() * 0.3
                        ),
                        want,
                        "reset at{rate} control{value}"
                    );
                }
            }
        }
    }
    #[test]
    fn bassinvader_native_live_parameter_fades() {
        let data = include_bytes!("../tests/fixtures/bassinvader-live-native.bin");
        let mut at = 0;
        for rate in [48000.0, 96000.0] {
            at += 0x2000;
            let input = &data[at..at + 32768];
            at += 32768;
            let expected = &data[at..at + 32768];
            at += 32768;
            at += 0x2000;
            let mut core =
                BassInvaderCore::new(rate, [0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5]);
            for frame in 0..8192 {
                if frame == 64 {
                    core.set_targets([0.23, 1.0, 1.0, 1.0, 0.17, 0.31, 0.79, 0.91, 0.67, 0.83]);
                }
                if frame == 128 {
                    core.set_targets([0.91, 0.0, 0.0, 0.0, 0.79, 0.67, 0.37, 0.17, 0.31, 0.23]);
                }
                if frame % 32 == 0 {
                    core.control();
                }
                let v = f32::from_le_bytes(input[frame * 4..frame * 4 + 4].try_into().unwrap());
                let output = core.process([v, v]);
                let want =
                    f32::from_le_bytes(expected[frame * 4..frame * 4 + 4].try_into().unwrap());
                assert_eq!(
                    output.to_bits(),
                    want.to_bits(),
                    "rate {rate} frame{frame}: {output} vs{want}"
                );
            }
        }
    }
    #[test]
    fn bassinvader_native_constructor_and_circuit() {
        let mut m = Memory::new();
        let mut r = Registers::new();
        r.x[0] = STATE;
        numeric::construct(&mut m, &mut r);
        let native = include_bytes!("../tests/fixtures/bassinvader-construct-native.bin");
        for a in (0x540..0x2000).step_by(4) {
            assert_eq!(&m.state[a..a + 4], &native[a..a + 4], "constructor {a:x}");
        }
        let blob = include_bytes!("../tests/fixtures/bassinvader-native.bin");
        let mut at = 0;
        for rate in [44100.0, 48000.0, 64000.0, 88200.0, 96000.0] {
            for values in [
                [0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5],
                [0.23, 1.0, 1.0, 1.0, 0.17, 0.31, 0.79, 0.91, 0.67, 0.83],
                [0.0; 10],
                [1.0; 10],
            ] {
                let initial = &blob[at..at + 0x2000];
                at += 0x2000;
                let input = &blob[at..at + 2048];
                at += 2048;
                let expected = &blob[at..at + 2048];
                at += 2048;
                let _final = &blob[at..at + 0x2000];
                at += 0x2000;
                let mut core = BassInvaderCore::new(rate, values);
                let mut differences = vec![];
                for a in (0x540..0x2000).step_by(4) {
                    if core.memory.state[a..a + 4] != initial[a..a + 4] {
                        differences.push((
                            a,
                            core.memory.f(a),
                            f32::from_le_bytes(initial[a..a + 4].try_into().unwrap()),
                        ))
                    }
                }
                assert!(
                    differences.is_empty(),
                    "rate {rate} values{values:?} initial {differences:x?}"
                );
                for frame in 0..512 {
                    let value =
                        f32::from_le_bytes(input[frame * 4..frame * 4 + 4].try_into().unwrap());
                    let out = core.process([value, value]);
                    let want =
                        f32::from_le_bytes(expected[frame * 4..frame * 4 + 4].try_into().unwrap());
                    assert_eq!(
                        out.to_bits(),
                        want.to_bits(),
                        "rate {rate} frame{frame}: {out} vs {want}"
                    );
                }
            }
        }
    }
}
