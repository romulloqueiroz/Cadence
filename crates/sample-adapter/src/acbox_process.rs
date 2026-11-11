//! Scalar reconstruction of 0x1005d8b0c, checked against original ARM64 execution.
//! Explicit SIMD lane shuffles preserve the parallel nonlinear bank and supply.
use super::{tables, State};
#[derive(Default)]
pub(super) struct Process {
    left: [f32; 16],
}
impl Process {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(super) fn duplicate_prepared(&self) -> Self {
        Self { left: self.left }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(super) fn copy_prepared_from(&mut self, template: &Self) {
        self.left = template.left;
    }

    pub(super) fn begin(&mut self, s: &State) {
        self.left = std::array::from_fn(|i| s.f(0x3eb0 + i * 4))
    }
    pub(super) fn frame(&mut self, s: &mut State, l: f32, r: f32) -> [f32; 2] {
        let mono = s.bytes[0x1c0] != 0;
        let input = if mono {
            [l * 0.5 + r * 0.5, 0.0]
        } else {
            [l, r]
        };
        let count = if mono { 1 } else { 2 };
        let mut out = [0.0; 2];
        for ch in 0..count {
            let b = ch * 0x140;
            s.put(b + 0x1d0, input[ch]);
            for phase in 0..4 {
                let c: [f32; 16] = if ch + 1 == count {
                    std::array::from_fn(|i| {
                        let v = s.f(0x3eb0 + i * 4) + s.f(0x3ef0 + i * 4);
                        s.put(0x3eb0 + i * 4, v);
                        v
                    })
                } else {
                    for (i, v) in self.left.iter_mut().enumerate() {
                        *v += s.f(0x3ef0 + i * 4)
                    }
                    self.left
                };
                let fir = 0x4130 - phase * 0x10;
                let x = s.vec(b + 0x1d0);
                let taps = s.vec(fir);
                let signal = (x[0] * taps[0] + x[2] * taps[2]) + (x[1] * taps[1] + x[3] * taps[3]);
                let value = step(s, b, &c, signal);
                s.put(b + 0x1ec - phase * 4, value);
            }
            // SIMD multiplication, accumulation across four history rows, then pairwise sum.
            let products: [f32; 16] =
                std::array::from_fn(|i| s.f(b + 0x1e0 + 4 * i) * s.f(0x4140 + 4 * i));
            let sum: [f32; 4] = std::array::from_fn(|i| {
                ((products[i] + products[i + 4]) + products[i + 8]) + products[i + 12]
            });
            out[ch] = (sum[0] + sum[2]) + (sum[1] + sum[3]);
            for i in (0..12).rev() {
                s.put(b + 0x1f0 + i * 4, s.f(b + 0x1e0 + i * 4))
            }
            s.put(b + 0x1dc, s.f(b + 0x1d8));
            s.put(b + 0x1d8, s.f(b + 0x1d4));
            s.put(b + 0x1d4, s.f(b + 0x1d0));
        }
        if mono {
            out[1] = out[0];
            for ch in 1..16 {
                s.put(ch * 0x140 + 0x308, s.f(0x308));
                s.put(ch * 0x140 + 0x30c, s.f(0x30c))
            }
        }
        out
    }
}
fn lookup(s: &State, table: usize, value: f32, coeff: usize) -> f32 {
    let low = s.f(coeff + 0x30);
    let high = s.f(coeff + 0x20);
    let idx = (value.clamp(low, high) - low) as usize;
    let pair = tables()[table].entries[idx.min(255)];
    pair[0] + pair[1] * (value - (low + idx as f32))
}
fn power_lookup(value: f32) -> f32 {
    let idx = (value.clamp(-118.0, 137.0) + 118.0) as usize;
    let pair = tables()[4].entries[idx.min(255)];
    pair[0] + pair[1] * (value - (-118.0 + idx as f32))
}
#[cfg(target_arch = "aarch64")]
fn reciprocal(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vmul_f32, vrecpe_f32, vrecps_f32};
    unsafe {
        let v = vdup_n_f32(value);
        let estimate = vrecpe_f32(v);
        let refined = vmul_f32(estimate, vrecps_f32(estimate, v));
        let x = vget_lane_f32::<0>(refined);
        (x + x) - value * (x * x)
    }
}
#[cfg(not(target_arch = "aarch64"))]
fn reciprocal(value: f32) -> f32 {
    scalar_reciprocal(value)
}
fn step(s: &mut State, b: usize, c: &[f32; 16], input: f32) -> f32 {
    let src = s.vec(b + 0x220);
    let old = s.vec(b + 0x230);
    let h = [input, src[1], src[2], src[3]];
    let hp: [f32; 4] = std::array::from_fn(|i| h[i] - old[i]);
    let k = s.vec(0x3f90);
    s.setvec(b + 0x230, std::array::from_fn(|i| old[i] + k[i] * hp[i]));
    let supply = s.vec(b + 0x240);
    let feedback = s.vec(b + 0x290);
    let scale = s.vec(0x3fa0);
    let offset = s.vec(0x3fb0);
    let coupling = s.vec(0x4020);
    let positions: [f32; 4] = std::array::from_fn(|i| {
        feedback[i] * coupling[i] + (offset[i] + scale[i] * (supply[i] * hp[i]))
    });
    let shaped: [f32; 4] = std::array::from_fn(|i| lookup(s, i, positions[i], 0x3fa0 + i * 4));
    let voltage = s.vec(b + 0x250);
    let old = s.vec(b + 0x280);
    let k = s.vec(0x4010);
    let lp: [f32; 4] = std::array::from_fn(|i| old[i] + k[i] * (voltage[i] * shaped[i] - old[i]));
    s.setvec(b + 0x280, lp);
    let src = [shaped[0], shaped[1], lp[2], lp[3]];
    let k = s.vec(0x4040);
    let feedback: [f32; 4] = std::array::from_fn(|i| feedback[i] + k[i] * (src[i] - feedback[i]));
    s.setvec(b + 0x290, feedback);
    let mixed = [
        c[0] * lp[2] + c[2] * feedback[2],
        c[1] * lp[3] + c[3] * feedback[3],
        c[2] * feedback[2] + c[2] * feedback[2],
        c[3] * feedback[3] + c[3] * feedback[3],
    ];
    let old2 = s.vec(b + 0x2b0);
    let old = s.vec(b + 0x2a0);
    let h = [lp[0], lp[1], old2[3], old2[3]];
    let hp: [f32; 4] = std::array::from_fn(|i| h[i] - old[i]);
    let k = s.vec(0x4050);
    s.setvec(b + 0x2a0, std::array::from_fn(|i| old[i] + k[i] * hp[i]));
    let old = old2;
    let h = [hp[0], hp[1], hp[1], lp[0]];
    let hp2: [f32; 4] = std::array::from_fn(|i| h[i] - old[i]);
    let k = s.vec(0x4060);
    let lp2: [f32; 4] = std::array::from_fn(|i| old[i] + k[i] * hp2[i]);
    s.setvec(b + 0x2b0, lp2);
    let gains = [
        c[4] * hp[0] + c[8] * hp2[0],
        c[5] * hp[1] + c[9] * hp2[1],
        c[6] * hp[2] + c[10] * lp2[2],
        c[7] * hp[3] + c[11] * lp2[2],
    ];
    s.setvec(
        b + 0x220,
        [gains[0], gains[0], gains[1] + gains[2], gains[1] + gains[2]],
    );
    let old = s.vec(b + 0x2d0);
    let diff: [f32; 4] = std::array::from_fn(|i| mixed[i] - old[i]);
    let k = s.vec(0x4080);
    s.setvec(b + 0x2d0, std::array::from_fn(|i| old[i] + k[i] * diff[i]));
    let output_filters = s.vec(b + 0x2c0);
    let common = output_filters[0] * s.f(0x4030);
    let mut phase = s.f(b + 0x308) + s.f(0x3e94);
    let mut sign = s.f(b + 0x30c);
    if phase > 1.0 {
        phase += -2.0;
        sign = -sign;
        s.put(b + 0x30c, sign)
    }
    s.put(b + 0x308, phase);
    let tremolo = (-phase).mul_add(phase, 1.0) * sign * c[12];
    let slow = s.vec(b + 0x2f0);
    let power_supply = s.vec(b + 0x260);
    let a = s.vec(0x40a0);
    let bias = s.vec(0x40b0);
    let scale = s.vec(0x4090);
    let power_pos: [f32; 4] = std::array::from_fn(|i| {
        let x = (diff[i] - common) + if i < 2 { tremolo } else { 0.0 };
        a[i] + power_supply[i] * (bias[i] + scale[i] * ([0.056, 0.056, 0.0, 0.0][i] * slow[0] + x))
    });
    let v = s.vec(b + 0x270);
    let power = [
        power_lookup(power_pos[0]) * v[0],
        power_lookup(power_pos[1]) * v[1],
    ];
    let sum = power[0] + power[1];
    let difference = power[0] - power[1];
    let h = [sum, sum, difference, difference];
    let k = s.vec(0x4070);
    let output_filters: [f32; 4] =
        std::array::from_fn(|i| output_filters[i] + k[i] * (h[i] - output_filters[i]));
    s.setvec(b + 0x2c0, output_filters);
    let mut phase = s.f(b + 0x300) + s.f(0x40f0);
    if phase >= 1.0 {
        phase -= 2.0
    }
    s.put(b + 0x300, phase);
    let supply = s.f(b + 0x304);
    let total = s.f(0x40f4);
    let rectifier = (total - (supply + total * (phase * phase))).max(0.0);
    let supply = (supply + s.f(0x40fc) * sum) + s.f(0x40f8) * rectifier;
    s.put(b + 0x304, supply);
    let old = s.vec(b + 0x2e0);
    let input = [supply, output_filters[2], slow[0], old[2]];
    let gain = [1.0, 1.0, 0.96, 0.91];
    let fb = [0.84000003, 1.0, 1.0, 1.0];
    let diff: [f32; 4] = std::array::from_fn(|i| input[i] * gain[i] - (slow[i] + old[i] * fb[i]));
    let k = s.vec(0x40e0);
    let filtered: [f32; 4] = std::array::from_fn(|i| old[i] + k[i] * diff[i]);
    s.setvec(b + 0x2e0, filtered);
    let slow0 = slow[0] + k[0] * filtered[0];
    s.put(b + 0x2f0, slow0);
    let voltages = [supply, slow0, filtered[2], filtered[3]];
    let inverses = voltages.map(reciprocal);
    s.setvec(b + 0x250, [filtered[2], filtered[3], slow0, slow0]);
    s.setvec(
        b + 0x240,
        [inverses[2], inverses[3], inverses[1], inverses[1]],
    );
    s.setvec(b + 0x270, [supply; 4]);
    s.setvec(b + 0x260, [inverses[0]; 4]);
    s.f(0x3e9c) * diff[1]
}

/// ARM reciprocal-estimate integer rounding, used away from ARM64 as well.
#[cfg(any(test, not(target_arch = "aarch64")))]
fn scalar_reciprocal(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 255;
    if !(1..=252).contains(&exponent) {
        return 1.0 / value;
    }
    let mantissa = 256 + ((bits & 0x7fffff) >> 15);
    let estimate = (((1 << 19) / (mantissa * 2 + 1)) + 1) >> 1;
    let seed =
        f32::from_bits((bits & 0x80000000) | ((253 - exponent) << 23) | ((estimate - 256) << 15));
    let x = seed * (-value).mul_add(seed, 2.0);
    (x + x) - value * (x * x)
}
#[cfg(all(test, target_arch = "aarch64"))]
mod reciprocal_tests {
    #[test]
    fn acbox_scalar_reciprocal_matches_arm64() {
        for i in 1..10000 {
            let value = 0.01 + i as f32 * 0.234567;
            assert_eq!(
                super::scalar_reciprocal(value).to_bits(),
                super::reciprocal(value).to_bits()
            )
        }
    }
}
