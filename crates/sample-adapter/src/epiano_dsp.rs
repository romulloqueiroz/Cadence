//! Recovered EPianoPreamps numerical core and host control mapping.
#![allow(dead_code)]
#[path = "epiano_numeric.rs"]
mod numeric;
const STATE: u64 = 0x10000000000;
const STACK: u64 = 0x20000000000;
const ARENA: u64 = 0x30000000000;
// The recovered Eigen work uses fixed matrices no larger than7x7. Each entry
// point rewinds this arena;64KiB bounds all packing/LU temporaries without
// reserving a megabyte for every polyphonic voice.
const ARENA_CAPACITY: usize = 64 * 1024;
#[cfg(test)]
static ARENA_PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const IO: u64 = 0x40000000000;
const GUARD: u64 = 0x50000000000;
const INITIAL: &[u8] = include_bytes!("epiano_initial.bin");
const CONSTANTS: &[u8] = include_bytes!("epiano_constants_canonical.bin");
#[cfg(target_arch = "aarch64")]
fn rsqrt_estimate(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vrsqrte_f32};
    unsafe { vget_lane_f32::<0>(vrsqrte_f32(vdup_n_f32(value))) }
}
#[cfg(not(target_arch = "aarch64"))]
fn rsqrt_estimate(value: f32) -> f32 {
    1.0 / value.sqrt()
}
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
        r.x[31] = STACK + 0x10000;
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
    arena: Box<[u8]>,
    io: [u8; 64],
    constants: Box<[u8]>,
    globals: [u8; 4096],
    cache: [u8; 24],
    cursor: usize,
}
impl Memory {
    /// Allocate a duplicate on the loading/preparation thread only.
    fn duplicate_prepared(&self) -> Self {
        Self {
            state: self.state.to_vec().into_boxed_slice(),
            stack: self.stack.to_vec().into_boxed_slice(),
            arena: self.arena.to_vec().into_boxed_slice(),
            constants: self.constants.to_vec().into_boxed_slice(),
            io: self.io,
            globals: self.globals,
            cache: self.cache,
            cursor: self.cursor,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    fn copy_prepared_from(&mut self, template: &Self) {
        self.state[..].copy_from_slice(&template.state[..]);
        self.stack[..].copy_from_slice(&template.stack[..]);
        self.arena[..].copy_from_slice(&template.arena[..]);
        self.constants[..].copy_from_slice(&template.constants[..]);
        self.io = template.io;
        self.globals = template.globals;
        self.cache = template.cache;
        self.cursor = template.cursor;
    }

    fn new() -> Self {
        let mut globals = [0; 4096];
        globals[0x3a0] = 1;
        let mut cache = [0; 24];
        for (i, v) in [0x4000u64, 0x80000, 0x80000].into_iter().enumerate() {
            cache[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        Self {
            state: INITIAL.into(),
            stack: vec![0; 0x10000].into_boxed_slice(),
            arena: vec![0; ARENA_CAPACITY].into_boxed_slice(),
            io: [0; 64],
            constants: CONSTANTS.into(),
            globals,
            cache,
            cursor: 0,
        }
    }
    fn read(&self, addr: u64, size: usize) -> u128 {
        if addr == GUARD {
            return 0x12345678;
        }
        if addr == 0x109924a98 {
            assert_eq!(size, 4);
            return 0x40549a78;
        }
        let source = if (0x109c91320..0x109c91338).contains(&addr) {
            &self.cache[(addr - 0x109c91320) as usize..]
        } else if (0x10996e000..0x10996f000).contains(&addr) {
            &self.globals[(addr - 0x10996e000) as usize..]
        } else if (STATE..STATE + self.state.len() as u64).contains(&addr) {
            &self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x10000).contains(&addr) {
            &self.stack[(addr - STACK) as usize..]
        } else if (ARENA..ARENA + ARENA_CAPACITY as u64).contains(&addr) {
            &self.arena[(addr - ARENA) as usize..]
        } else if (IO..IO + 64).contains(&addr) {
            &self.io[(addr - IO) as usize..]
        } else {
            let page = addr & !4095;
            let index = numeric::CONSTANT_PAGES
                .iter()
                .position(|p| *p == page)
                .unwrap_or_else(|| panic!("unmapped EP constant {addr:x}"));
            &self.constants[index * 4096 + (addr & 4095) as usize..]
        };
        let mut value = [0; 16];
        value[..size].copy_from_slice(&source[..size]);
        u128::from_le_bytes(value)
    }
    fn write(&mut self, addr: u64, size: usize, value: u128) {
        let target = if (0x109c91320..0x109c91338).contains(&addr) {
            &mut self.cache[(addr - 0x109c91320) as usize..]
        } else if (0x10996e000..0x10996f000).contains(&addr) {
            &mut self.globals[(addr - 0x10996e000) as usize..]
        } else if (STATE..STATE + self.state.len() as u64).contains(&addr) {
            &mut self.state[(addr - STATE) as usize..]
        } else if (STACK..STACK + 0x10000).contains(&addr) {
            &mut self.stack[(addr - STACK) as usize..]
        } else if (ARENA..ARENA + ARENA_CAPACITY as u64).contains(&addr) {
            &mut self.arena[(addr - ARENA) as usize..]
        } else if (IO..IO + 64).contains(&addr) {
            &mut self.io[(addr - IO) as usize..]
        } else {
            let page = addr & !4095;
            let index = numeric::CONSTANT_PAGES
                .iter()
                .position(|p| *p == page)
                .unwrap_or_else(|| panic!("unmapped EP write {addr:x}"));
            &mut self.constants[index * 4096 + (addr & 4095) as usize..]
        };
        target[..size].copy_from_slice(&value.to_le_bytes()[..size]);
    }
    fn zero(&mut self, addr: u64, size: usize) {
        for i in 0..size {
            self.write(addr + i as u64, 1, 0);
        }
    }
    fn copy(&mut self, dst: u64, src: u64, size: usize) {
        for i in 0..size {
            let v = self.read(src + i as u64, 1);
            self.write(dst + i as u64, 1, v);
        }
    }
    fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.read(STATE + at as u64, 4) as u32)
    }
    fn put(&mut self, at: usize, v: f32) {
        self.write(STATE + at as u64, 4, v.to_bits() as u128);
    }
    fn start(&mut self) -> Registers {
        self.cursor = 0;
        let mut r = Registers::new();
        r.x[0] = STATE;
        r
    }
}
fn dispatch(m: &mut Memory, r: &mut Registers, address: u64) {
    if numeric::run(m, r, address) {
        return;
    }
    match address {
        0x10434afdc => r.v[0] = fd(r.v[0], 0).tanh().to_bits() as u128,
        0x10434afe8 => r.v[0] = fs(r.v[0], 0).tanh().to_bits() as u128,
        0x104349de8 => r.v[0] = fd(r.v[0], 0).exp().to_bits() as u128,
        0x10434983c => r.v[0] = 10f32.powf(fs(r.v[0], 0)).to_bits() as u128,
        0x104349efc => r.v[0] = (fs(r.v[0], 0) % fs(r.v[1], 0)).to_bits() as u128,
        0x104349aa0 => m.zero(r.x[0], r.x[1] as usize),
        0x10434a4f0 => m.copy(r.x[0], r.x[1], r.x[2] as usize),
        0x10434a478 => {
            let size = r.x[0] as usize;
            let at = m.cursor;
            m.cursor = (at + size + 63) & !63;
            assert!(
                m.cursor <= m.arena.len(),
                "EP scratch arena capacity exceeded"
            );
            #[cfg(test)]
            ARENA_PEAK.fetch_max(m.cursor, std::sync::atomic::Ordering::Relaxed);
            r.x[0] = ARENA + at as u64;
        }
        0x104349f50 => {}
        0x1043497dc => r.x[0] = 1,
        0x1043497e8 => m.write(r.x[0], 1, 1),
        0x101ea9910 => {} // Converter owners reset their own state.
        _ => panic!("unmapped EP call {address:x}"),
    }
}
pub(crate) struct EPianoCore {
    memory: Memory,
    rate: f32,
    ratio: f32,
    block: usize,
}
impl EPianoCore {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            memory: self.memory.duplicate_prepared(),
            rate: self.rate,
            ratio: self.ratio,
            block: self.block,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.memory.copy_prepared_from(&template.memory);
        self.rate = template.rate;
        self.ratio = template.ratio;
        self.block = template.block;
    }

    pub(crate) fn new(rate: f32, ratio: f32, block: usize, params: [f32; 23]) -> Self {
        let mut core = Self {
            memory: Memory::new(),
            rate,
            ratio,
            block,
        };
        core.restart(params);
        core
    }
    /// Restore the configured initial circuit without allocating. Native Reset
    /// retains some histories, so transport restart uses this stronger lifecycle.
    pub(crate) fn restart(&mut self, params: [f32; 23]) {
        let Self {
            memory: m,
            rate,
            ratio,
            block,
        } = self;
        let (rate, ratio, block) = (*rate, *ratio, *block);
        m.state.copy_from_slice(INITIAL);
        m.stack.fill(0);
        m.arena.fill(0);
        m.io.fill(0);
        m.constants.copy_from_slice(CONSTANTS);
        m.globals.fill(0);
        m.globals[0x3a0] = 1;
        m.cursor = 0;
        m.put(0x7b0, rate);
        m.put(0x7b4, ratio);
        m.write(STATE + 0x7b8, 4, block as u128);
        let period = ((rate * 0.02 / block as f32 + 0.5) as u32).max(1);
        m.write(STATE + 0x51c, 4, period as u128);
        let delta = 1.0 / (period as f32 * block as f32);
        for at in [0x520, 0x524, 0x528, 0x52c] {
            m.put(at, delta);
        }
        let mut r = m.start();
        r.v[0] = rate.to_bits() as u128;
        r.v[1] = ratio.to_bits() as u128;
        r.x[1] = block as u64;
        numeric::set_rate(m, &mut r);
        self.set_parameters(params);
        self.reset();
    }
    pub(crate) fn set_parameters(&mut self, params: [f32; 23]) {
        for (index, value) in params.into_iter().enumerate() {
            let mut r = self.memory.start();
            r.x[1] = index as u64;
            r.v[0] = value.to_bits() as u128;
            numeric::set_parameter(&mut self.memory, &mut r);
        }
    }
    pub(crate) fn reset(&mut self) {
        let mut r = self.memory.start();
        numeric::suspend(&mut self.memory, &mut r);
        let mut r = self.memory.start();
        numeric::reset(&mut self.memory, &mut r);
    }
    pub(crate) fn control(&mut self) {
        self.memory.state[0x3c0] = 0;
        let mut r = self.memory.start();
        numeric::control_rate(&mut self.memory, &mut r);
    }
    pub(crate) fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let m = &mut self.memory;
        for (at, v) in [(0, input[0]), (8, input[1])] {
            m.write(IO + at, 4, v.to_bits() as u128);
        }
        for (at, ptr) in [(32, IO), (40, IO + 8), (48, IO + 16), (56, IO + 24)] {
            m.write(IO + at, 8, ptr as u128);
        }
        let mut r = m.start();
        r.x[1] = IO + 32;
        r.x[2] = IO + 48;
        r.x[3] = 1;
        numeric::process(m, &mut r);
        [
            f32::from_bits(m.read(IO + 16, 4) as u32),
            f32::from_bits(m.read(IO + 24, 4) as u32),
        ]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parameters(case: usize) -> [f32; 23] {
        let mut v = [
            0., 0.5, 0., 0.5, 1., 0.5, 0.5, 1., 1., 1., 1., 0., 1., 0.5, 0., 0., 0., 0., 0., 0.,
            0., 0.5, 0.,
        ];
        v[0] = (case % 5) as f32 / 4.;
        v[2] = (case % 6) as f32 / 5.;
        v[11] = (case % 7) as f32 / 6.;
        v[18] = (case % 5) as f32 / 4.;
        for (i, x) in [
            (1, 0.23),
            (3, 0.71),
            (5, 0.38),
            (6, 0.61),
            (12, 0.67),
            (13, 0.29),
            (20, 0.19),
        ] {
            v[i] = x;
        }
        for i in 0..4 {
            v[7 + i] = ((case >> i) & 1) as f32;
        }
        v[22] = (case % 2) as f32;
        v
    }
    fn word(bytes: &[u8], at: &mut usize) -> u32 {
        let v = u32::from_le_bytes(bytes[*at..*at + 4].try_into().unwrap());
        *at += 4;
        v
    }
    fn number(bytes: &[u8], at: &mut usize) -> f32 {
        f32::from_bits(word(bytes, at))
    }
    #[test]
    fn epiano_prepared_copy_uses_bounded_scratch() {
        for case in 0..21 {
            let template = EPianoCore::new(96000.0, 2.0, 64, parameters(case));
            let mut instance = template.duplicate_prepared();
            for _ in 0..64 {
                instance.process([0.2, -0.1]);
            }
            instance.copy_prepared_from(&template);
            let mut expected = template.duplicate_prepared();
            for _ in 0..64 {
                assert_eq!(
                    instance.process([0.13, -0.07]),
                    expected.process([0.13, -0.07])
                );
            }
        }
        let peak = ARENA_PEAK.load(std::sync::atomic::Ordering::Relaxed);
        assert!(peak <= ARENA_CAPACITY);
        eprintln!("EP_GROUP_SCRATCH capacity={ARENA_CAPACITY} allocator_peak={peak}");
    }
    #[test]
    fn epiano_core_matches_native_modes_and_live_controls() {
        let bytes = include_bytes!("../tests/fixtures/epiano-core-native.bin");
        assert_eq!(&bytes[..8], b"EPCORE01");
        let mut at = 8;
        let cases = word(bytes, &mut at) as usize;
        let frames = word(bytes, &mut at) as usize;
        let mut max = 0f32;
        for case in 0..cases {
            let rate = number(bytes, &mut at);
            let ratio = number(bytes, &mut at);
            let block = number(bytes, &mut at) as usize;
            assert_eq!(word(bytes, &mut at) as usize, case);
            let params = std::array::from_fn(|_| number(bytes, &mut at));
            let length = word(bytes, &mut at) as usize;
            let mut core = EPianoCore::new(rate, ratio, block, params);
            let expected = &bytes[at..at + length];
            at += length;
            for offset in (0..0x5eb8).step_by(4) {
                let a = core.memory.read(STATE + offset as u64, 4) as u32;
                let b = u32::from_le_bytes(expected[offset..offset + 4].try_into().unwrap());
                let x = f32::from_bits(a);
                let y = f32::from_bits(b);
                assert!(
                    a == b
                        || (x.is_finite()
                            && y.is_finite()
                            && (x - y).abs() <= 2e-6 * (1. + y.abs())),
                    "EP initial case {case} offset {offset:x}: {x} vs {y}"
                );
            }
            for frame in 0..frames {
                if frame == 128 {
                    core.set_parameters(parameters(case + 1));
                }
                if frame % block == 0 {
                    core.control();
                }
                let input = [number(bytes, &mut at), number(bytes, &mut at)];
                let expected = [number(bytes, &mut at), number(bytes, &mut at)];
                let actual = core.process(input);
                for c in 0..2 {
                    let err = (actual[c] - expected[c]).abs();
                    max = max.max(err);
                    assert!(
                        actual[c].is_finite() && err <= 3e-6 * (1. + expected[c].abs()),
                        "EP case {case} frame {frame} channel {c}: {} vs {}",
                        actual[c],
                        expected[c]
                    );
                }
            }
        }
        assert_eq!(at, bytes.len());
        eprintln!(
            "EP_NATIVE cases={cases} frames={} max_error={max}",
            cases * frames
        );
    }
    #[test]
    #[allow(clippy::needless_range_loop)]
    fn epiano_dense_inverse_matches_isolated_native_helpers() {
        for (case, shifted) in [false, true].into_iter().enumerate() {
            let mut m = Memory::new();
            let matrix: [[f64; 6]; 6] = std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    if (if shifted { (i + 2) % 6 } else { i }) == j {
                        6.0 + i as f64
                    } else {
                        (i + j + 1) as f64 * 0.03
                    }
                })
            });
            for i in 0..6 {
                for j in 0..6 {
                    m.write(
                        ARENA + 0x8000 + (i * 6 + j) as u64 * 8,
                        8,
                        matrix[i][j].to_bits() as u128,
                    );
                }
            }
            let bytes = include_bytes!("../tests/fixtures/epiano-matrix-native.bin");
            for i in 0..6 {
                for j in 0..6 {
                    m.write(
                        ARENA + 0xa000 + (i * 6 + j) as u64 * 8,
                        8,
                        matrix[i][j].to_bits() as u128,
                    );
                }
            }
            let mut r = m.start();
            r.x[0] = ARENA + 0xa000;
            numeric::f_1019e357c(&mut m, &mut r);
            for off in (0..0x160).step_by(8) {
                let a = m.read(ARENA + 0xa000 + off as u64, 8) as u64;
                let b = u64::from_le_bytes(
                    bytes[case * (0x180 + 288 * 3) + off..case * (0x180 + 288 * 3) + off + 8]
                        .try_into()
                        .unwrap(),
                );
                let x = f64::from_bits(a);
                let y = f64::from_bits(b);
                assert!(
                    a == b || (x - y).abs() < 1e-12,
                    "LU case{case} off{off:x}: {x} vs {y}"
                );
            }
            for i in 0..6 {
                for j in 0..6 {
                    m.write(
                        ARENA + 0xb000 + (i * 6 + j) as u64 * 8,
                        8,
                        (if i == j { 1.0f64 } else { 0.0 }).to_bits() as u128,
                    );
                }
            }
            for (off, value) in [
                (0, ARENA + 0xd000),
                (8, ARENA + 0xe000),
                (16, 6),
                (24, 6),
                (32, 6),
            ] {
                m.write(ARENA + 0xc000 + off, 8, value as u128);
            }
            for step in 0..2 {
                let mut r = m.start();
                r.x[..8].copy_from_slice(&[
                    6,
                    6,
                    ARENA + 0xa000,
                    6,
                    ARENA + 0xb000,
                    1,
                    6,
                    ARENA + 0xc000,
                ]);
                if step == 0 {
                    numeric::f_1019e40b8(&mut m, &mut r);
                } else {
                    numeric::f_1019e72c4(&mut m, &mut r);
                }
                for off in (0..288).step_by(8) {
                    let a = f64::from_bits(m.read(ARENA + 0xb000 + off as u64, 8) as u64);
                    let at = case * (0x180 + 288 * 3) + 0x180 + 288 * (step + 1) + off;
                    let b = f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
                    assert!(
                        (a - b).abs() < 1e-12,
                        "triangular case{case} step{step} off{off:x}: {a} vs {b}"
                    );
                }
            }
            let mut r = m.start();
            r.x[0] = ARENA + 0x8000;
            r.x[1] = ARENA + 0x9000;
            numeric::f_1019e3324(&mut m, &mut r);
            for i in 0..6 {
                for j in 0..6 {
                    let mut product = 0.0;
                    for k in 0..6 {
                        product += matrix[k][j]
                            * f64::from_bits(
                                m.read(ARENA + 0x9000 + (i * 6 + k) as u64 * 8, 8) as u64
                            );
                    }
                    let expected = if i == j { 1.0 } else { 0.0 };
                    assert!(
                        (product - expected).abs() < 1e-12,
                        "shifted {shifted} inverseproduct [{i},{j}] {product}"
                    );
                }
            }
        }
    }

    #[test]
    fn epiano_restart_restores_fresh_circuit_after_live_changes() {
        for rate in [44100.0, 48000.0, 96000.0, 192000.0] {
            for case in [3, 11, 17] {
                let initial = parameters(case);
                let changed = parameters(case + 1);
                let mut core = EPianoCore::new(rate, 1.0, 32, initial);
                let mut fresh = EPianoCore::new(rate, 1.0, 32, changed);
                for frame in 0..128 {
                    if frame % 32 == 0 {
                        core.control();
                    }
                    core.process([0.17, -0.23]);
                }
                core.set_parameters(changed);
                for _ in 0..67 {
                    core.process([-0.13, 0.19]);
                }
                core.restart(changed);
                for frame in 0..256 {
                    if frame % 32 == 0 {
                        core.control();
                        fresh.control();
                    }
                    let input = [
                        (frame as f32 * 0.07).sin() * 0.2,
                        (frame as f32 * 0.037).cos() * 0.1,
                    ];
                    assert_eq!(
                        core.process(input),
                        fresh.process(input),
                        "rate{rate} case{case} frame{frame}"
                    );
                }
            }
        }
    }
    #[test]
    fn epiano_control_endpoints_stay_finite_across_modes_and_rates() {
        for rate in [44100.0, 48000.0, 96000.0, 192000.0] {
            for case in 0..21 {
                for endpoint in [0.0, 0.5, 1.0] {
                    let mut params = parameters(case);
                    for index in [1, 3, 4, 5, 6, 12, 13, 20, 21] {
                        params[index] = endpoint;
                    }
                    let mut core = EPianoCore::new(rate, 1.0, 32, params);
                    for frame in 0..128 {
                        if frame == 64 {
                            for index in [1, 3, 4, 5, 6, 12, 13, 20, 21] {
                                params[index] = 1.0 - endpoint;
                            }
                            core.set_parameters(params);
                        }
                        if frame % 32 == 0 {
                            core.control();
                        }
                        let output = core.process([
                            (frame as f32 * 0.071).sin() * 0.25,
                            (frame as f32 * 0.037).cos() * 0.17,
                        ]);
                        assert!(
                            output.iter().all(|x| x.is_finite()),
                            "rate {rate} case {case} endpoint {endpoint} frame {frame}: {output:?}"
                        );
                    }
                }
            }
        }
    }
}
