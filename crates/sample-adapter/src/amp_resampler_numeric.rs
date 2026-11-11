//! NI SAMPLE RATE CONVERTER numeric source translated from recovered ARM64 data flow.
//! Rebuild with tests/fixtures/amp_resampler_translate.py. No target instructions execute.
#![allow(
    unused_parens,
    unused_variables,
    unused_assignments,
    unused_mut,
    unused_imports,
    unreachable_code,
    dead_code,
    clippy::all
)]
use super::{
    dispatch, fd, fs, lane, reciprocal_estimate, round_odd, rsqrt_estimate, Memory, Registers,
};
pub(super) fn convert(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327118704u64;
    loop {
        match pc {
            4327118704 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-48i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(32u64));
                r.compare(r.x[4] as u32 as u64, r.x[2] as u32 as u64, 32, true);
                if r.z {
                    pc = 4327118784;
                    continue;
                }
                pc = 4327118728;
            }
            4327118728 => {
                r.x[19] = (r.x[3]);
                r.x[20] = (r.x[1]);
                {
                    let addr = (r.x[0]).wrapping_add(0u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[8] as u32 as u64, 2u64, 32, false);
                if !r.z {
                    pc = 4327118800;
                    continue;
                }
                pc = 4327118748;
            }
            4327118748 => {
                if r.x[5] as u32 as u64 == 0 {
                    pc = 4327118784;
                    continue;
                }
                pc = 4327118752;
            }
            4327118752 => {
                r.x[21] = (((r.x[2]) & 0xffffffffu64) << 2);
                r.x[22] = (r.x[5] as u32 as u64) as u32 as u64;
                pc = 4327118760;
            }
            4327118760 => {
                {
                    let addr = (r.x[19]).wrapping_add(0u64);
                    r.x[0] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (addr.wrapping_add(8u64));
                }
                {
                    let addr = (r.x[20]).wrapping_add(0u64);
                    r.x[1] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[20] = (addr.wrapping_add(8u64));
                }
                r.x[2] = (r.x[21]);
                dispatch(m, r, 0x10434a4f0);
                r.compare(r.x[22], 1u64, 64, false);
                r.x[22] = ((r.x[22]).wrapping_sub(1u64));
                if !r.z {
                    pc = 4327118760;
                    continue;
                }
                pc = 4327118784;
            }
            4327118784 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                pc = 4327118788;
            }
            4327118788 => {
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                pc = 4327118792;
            }
            4327118792 => {
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                pc = 4327118796;
            }
            4327118796 => {
                return;
            }
            4327118800 => {
                {
                    let addr = (r.x[0]).wrapping_add(8u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[9] as u32 as u64 == 0 {
                    pc = 4327118936;
                    continue;
                }
                pc = 4327118808;
            }
            4327118808 => {
                r.x[10] = (0u64);
                r.x[11] = (24u64) as u32 as u64;
                pc = 4327118816;
            }
            4327118816 => {
                r.x[12] = ((r.x[10]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[20]).wrapping_add(r.x[12]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[0]).wrapping_add(56u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[14]).wrapping_add(r.x[12]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(r.x[12]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[0]).wrapping_add(80u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                pc = 4327118840;
            }
            4327118840 => {
                {
                    let addr = (r.x[14]).wrapping_add(r.x[12]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                pc = 4327118844;
            }
            4327118844 => {
                {
                    let addr = (r.x[0]).wrapping_add(152u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[14] = ((r.x[10]).wrapping_mul(r.x[11]));
                {
                    let addr = (r.x[13]).wrapping_add(r.x[14]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[0]).wrapping_add(104u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[14]).wrapping_add(r.x[12]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                r.x[10] = ((r.x[10]).wrapping_add(1u64));
                r.compare(r.x[10], r.x[9], 64, false);
                if !r.z {
                    pc = 4327118816;
                    continue;
                }
                pc = 4327118876;
            }
            4327118876 => {
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[9] as u32 as u64) as u128);
                    m.write(addr.wrapping_add(4), 4, (r.x[2] as u32 as u64) as u128);
                }
                pc = 4327118880;
            }
            4327118880 => {
                {
                    let addr = (r.x[0]).wrapping_add(52u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[4] as u32 as u64) as u128);
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.compare(r.x[8] as u32 as u64, 5u64, 32, false);
                if r.c && !r.z {
                    pc = 4327118784;
                    continue;
                }
                pc = 4327118896;
            }
            4327118896 => {
                r.x[9] = (4438630400u64);
                r.x[9] = ((r.x[9]).wrapping_add(1384u64));
                r.x[10] = (4327118784u64);
                {
                    let addr = (r.x[9]).wrapping_add(r.x[8]);
                    r.x[11] = (m.read(addr.wrapping_add(0), 1) as u64) as u32 as u64;
                }
                r.x[10] = ((r.x[10]).wrapping_add((r.x[11]).wrapping_shl(2)));
                let target = r.x[10];
                if (4327118704..=4327119116).contains(&target) {
                    pc = target;
                    continue;
                }
                dispatch(m, r, target);
                return;
            }
            4327118920 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                generic(m, r);
                return;
            }
            4327118936 => {
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[9] as u32 as u64) as u128);
                    m.write(addr.wrapping_add(4), 4, (r.x[2] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[0]).wrapping_add(52u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[4] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[0]).wrapping_add(180u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[0]).wrapping_add(24u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.v[1] = ((r.x[4] as u32 as u64) as u32 as f64).to_bits() as u128;
                r.x[8] = ((r.x[9] as u32 as u64)
                    .wrapping_mul(r.x[4] as u32 as u64)
                    .wrapping_add(r.x[8] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[0]).wrapping_add(28u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 4);
                    r.v[0] = m.read(addr.wrapping_add(4), 4);
                }
                r.v[2] = ((fs(r.v[2], 0)) as f64).to_bits() as u128;
                r.v[1] = (fd(r.v[1], 0) * fd(r.v[2], 0)).to_bits() as u128;
                r.v[2] = ((fs(r.v[0], 0)) as f64).to_bits() as u128;
                r.v[3] = (fd(r.v[1], 0) / fd(r.v[2], 0)).to_bits() as u128;
                r.x[9] = ((fd(r.v[3], 0)) as u32 as u64) as u32 as u64;
                r.v[3] = ((r.x[9] as u32 as u64) as u32 as f64).to_bits() as u128;
                r.v[1] = ((-fd(r.v[3], 0)).mul_add(fd(r.v[2], 0), fd(r.v[1], 0))).to_bits() as u128;
                r.v[1] = ((fd(r.v[1], 0)) as f32).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[1] = (fs(r.v[2], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_add(r.x[9] as u32 as u64)) as u32 as u64;
                r.float_compare(fs(r.v[1], 0) as f64, fs(r.v[0], 0) as f64);
                if r.n != r.o {
                    pc = 4327119032;
                    continue;
                }
                pc = 4327119020;
            }
            4327119020 => {
                r.v[0] = (fs(r.v[1], 0) - fs(r.v[0], 0)).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                pc = 4327119032;
            }
            4327119032 => {
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[2] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[0]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                return;
            }
            4327119056 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                up(m, r);
                return;
            }
            4327119072 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                down(m, r);
                return;
            }
            4327119088 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                double(m, r);
                return;
            }
            4327119104 => {
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                half(m, r);
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn reset(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327119120u64;
    loop {
        match pc {
            4327119120 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-48i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(32u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(8u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[9] as u32 as u64 == 0 {
                    pc = 4327119272;
                    continue;
                }
                pc = 4327119148;
            }
            4327119148 => {
                {
                    let addr = (r.x[19]).wrapping_add(0u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[20] = ((r.x[9]).wrapping_sub(1u64));
                r.x[21] = (24u64) as u32 as u64;
                pc = 4327119176;
                continue;
            }
            4327119164 => {
                r.x[9] = (r.x[20]);
                r.x[20] = ((r.x[20]).wrapping_sub(1u64));
                if r.x[9] as u32 as u64 == 0 {
                    pc = 4327119272;
                    continue;
                }
                pc = 4327119176;
            }
            4327119176 => {
                r.x[9] = ((r.x[8] as u32 as u64) | (2u64)) as u32 as u64;
                r.compare(r.x[9] as u32 as u64, 2u64, 32, false);
                if r.z {
                    pc = 4327119164;
                    continue;
                }
                pc = 4327119188;
            }
            4327119188 => {
                r.x[22] = (r.x[20] & 4294967295u64);
                {
                    let addr = (r.x[19]).wrapping_add(152u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[9] = (((r.x[22] as u32 as u64) as u32 as u64)
                    .wrapping_mul((r.x[21] as u32 as u64) as u32 as u64)
                    .wrapping_add(r.x[9]));
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.x[0] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[9] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                r.x[9] = ((r.x[9]).wrapping_sub(r.x[0]));
                r.compare(r.x[9], 1u64, 64, false);
                if r.n != r.o {
                    pc = 4327119248;
                    continue;
                }
                pc = 4327119216;
            }
            4327119216 => {
                r.x[8] = ((r.x[9]).wrapping_shr((4u64) as u32));
                r.compare(r.x[9], 15u64, 64, false);
                r.x[9] = ((r.c && !r.z) as u64) as u32 as u64;
                r.x[8] = ((r.x[8]).wrapping_sub(r.x[9]));
                r.x[8] = ((r.x[8]).wrapping_shl((4u64) as u32));
                r.x[1] = ((r.x[8]).wrapping_add(16u64));
                dispatch(m, r, 0x104349aa0);
                {
                    let addr = (r.x[19]).wrapping_add(0u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327119248;
            }
            4327119248 => {
                r.x[9] = ((r.x[8] as u32 as u64).wrapping_sub(3u64)) as u32 as u64;
                r.compare(r.x[9] as u32 as u64, 3u64, 32, false);
                if r.c && !r.z {
                    pc = 4327119164;
                    continue;
                }
                pc = 4327119260;
            }
            4327119260 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                pc = 4327119164;
                continue;
            }
            4327119272 => {
                r.x[8] = (1u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[8]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(48u64));
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn generic(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327120112u64;
    loop {
        match pc {
            4327120112 => {
                r.x[31] = ((r.x[31]).wrapping_sub(112u64));
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[28]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[27]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[26]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[25]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[24]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[23]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(96u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(96u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(180u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[0]).wrapping_add(48u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[8] as u32 as u64, r.x[9] as u32 as u64, 32, false);
                r.x[23] = (if !r.c {
                    r.x[8] as u32 as u64
                } else {
                    r.x[9] as u32 as u64
                }) as u32 as u64;
                if r.x[23] as u32 as u64 == 0 {
                    pc = 4327120272;
                    continue;
                }
                pc = 4327120168;
            }
            4327120168 => {
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[20] = (((r.x[23]) & 0xffffffffu64) << 2);
                r.x[8] = ((r.x[24] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[25] = (((r.x[8]) & 0xffffffffu64) << 3);
                r.x[26] = ((r.x[23]).wrapping_shl((2u64) as u32));
                pc = 4327120188;
            }
            4327120188 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[21] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[24] = ((r.x[24] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[27] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(188u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[23] as u32 as u64)) as u32 as u64;
                r.x[1] = ((r.x[21]).wrapping_add(r.x[26]));
                r.x[22] = (((r.x[8]) & 0xffffffffu64) << 2);
                r.x[0] = (r.x[21]);
                r.x[2] = (r.x[22]);
                dispatch(m, r, 0x10434a4fc);
                r.x[0] = ((r.x[21]).wrapping_add(r.x[22]));
                {
                    let addr = (r.x[27]).wrapping_add(r.x[25]);
                    r.x[1] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[2] = (r.x[20]);
                dispatch(m, r, 0x10434a4f0);
                {
                    let addr = (r.x[27]).wrapping_add(r.x[25]);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[8] = ((r.x[8]).wrapping_add(r.x[26]));
                {
                    let addr = (r.x[27]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[8]) as u128);
                }
                r.x[25] = ((r.x[25]).wrapping_sub(8u64));
                if r.x[24] as u32 as u64 != 0 {
                    pc = 4327120188;
                    continue;
                }
                pc = 4327120268;
            }
            4327120268 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327120272;
            }
            4327120272 => {
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[23] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(52u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[13] as u32 as u64 != 0 {
                    pc = 4327120328;
                    continue;
                }
                pc = 4327120288;
            }
            4327120288 => {
                {
                    let addr = (r.x[31]).wrapping_add(96u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[23] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[25] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[28] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[27] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                r.x[31] = ((r.x[31]).wrapping_add(112u64));
                return;
            }
            4327120320 => {
                {
                    let addr = (r.x[31]).wrapping_add(12u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327120324;
            }
            4327120324 => {
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327120288;
                    continue;
                }
                pc = 4327120328;
            }
            4327120328 => {
                {
                    let addr = (r.x[19]).wrapping_add(176u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327120452;
                    continue;
                }
                pc = 4327120336;
            }
            4327120336 => {
                r.x[8] = (0u64);
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[19]).wrapping_add(196u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                {
                    let a = (r.v[0] >> 0) & 0xffffffff;
                    r.v[1] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let addr = (r.x[19]).wrapping_add(200u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                pc = 4327120360;
            }
            4327120360 => {
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.v[3] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = if fs(r.v[1], 0) >= fs(r.v[2], 0) {
                        0xffffffffu128
                    } else {
                        0u128
                    };
                    let a1: u128 = if fs(r.v[1], 1) >= fs(r.v[2], 1) {
                        0xffffffffu128
                    } else {
                        0u128
                    };
                    let a2: u128 = if fs(r.v[1], 2) >= fs(r.v[2], 2) {
                        0xffffffffu128
                    } else {
                        0u128
                    };
                    let a3: u128 = if fs(r.v[1], 3) >= fs(r.v[2], 3) {
                        0xffffffffu128
                    } else {
                        0u128
                    };
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[12] = ((r.x[9]).wrapping_add(48u64));
                {
                    let addr = (r.x[9]).wrapping_add(32u64);
                    r.v[4] = m.read(addr.wrapping_add(0), 16);
                }
                r.v[3] = ((r.v[3] & !r.v[2]) | (r.v[4] & r.v[2]));
                {
                    let addr = (r.x[10]).wrapping_add((r.x[8]).wrapping_shl(4));
                    m.write(addr.wrapping_add(0), 16, r.v[3]);
                }
                {
                    let addr = (r.x[19]).wrapping_add(192u64);
                    r.x[11] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327120392;
            }
            4327120392 => {
                r.x[9] = ((r.x[12]).wrapping_add(32u64));
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[4] = m.read(addr.wrapping_add(0), 16);
                    r.v[5] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[3], 0) * fs(r.v[0], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[3], 1) * fs(r.v[0], 0)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[3], 2) * fs(r.v[0], 0)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[3], 3) * fs(r.v[0], 0)).to_bits() as u128;
                    r.v[3] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.v[5] = (r.v[5] & r.v[2]);
                r.v[4] = (r.v[4] & !r.v[2]);
                r.v[4] = (r.v[5] | r.v[4]);
                {
                    let a0: u128 = (fs(r.v[3], 0) + fs(r.v[4], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[3], 1) + fs(r.v[4], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[3], 2) + fs(r.v[4], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[3], 3) + fs(r.v[4], 3)).to_bits() as u128;
                    r.v[3] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[10]).wrapping_add((r.x[8]).wrapping_shl(4));
                    m.write(addr.wrapping_add(0), 16, r.v[3]);
                }
                r.x[12] = (r.x[9]);
                r.compare(r.x[11] as u32 as u64, 1u64, 32, false);
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327120392;
                    continue;
                }
                pc = 4327120436;
            }
            4327120436 => {
                r.x[8] = ((r.x[8]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(176u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[8], r.x[10], 64, false);
                if !r.c {
                    pc = 4327120360;
                    continue;
                }
                pc = 4327120452;
            }
            4327120452 => {
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327120516;
                continue;
            }
            4327120464 => {
                {
                    let a = (r.v[0] >> 64) & 0xffffffff;
                    r.v[1] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[1] = lane(r.v[1], 1, 32, ((r.v[0] >> 96) & 0xffffffff));
                r.v[1] = lane(r.v[1], 1, 64, ((r.v[0] >> 0) & 0xffffffffffffffff));
                {
                    let a0: u128 = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[0] >> 32) & 0xffffffff;
                    r.v[1] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[19]).wrapping_add(80u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[10] = ((r.x[8]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    r.x[11] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[12] = ((r.x[11]).wrapping_add(4u64));
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[12]) as u128);
                }
                {
                    let addr = (r.x[11]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327120592;
                    continue;
                }
                pc = 4327120516;
            }
            4327120516 => {
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9])
                        .wrapping_add(((r.x[8] as u32 as u64) as u32 as u64).wrapping_shl(3));
                    r.x[11] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[11]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let addr = (r.x[10]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[19]).wrapping_add(176u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[9] as u32 as u64, 1u64, 32, false);
                r.x[9] = ((r.x[9] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327120464;
                    continue;
                }
                pc = 4327120556;
            }
            4327120556 => {
                r.x[10] = ((r.x[10]).wrapping_add(16u64));
                r.x[11] = ((r.x[11]).wrapping_add(16u64));
                pc = 4327120564;
            }
            4327120564 => {
                {
                    let addr = (r.x[11]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                    r.x[11] = (addr.wrapping_add(16u64));
                }
                {
                    let addr = (r.x[10]).wrapping_add(0u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.x[10] = (addr.wrapping_add(16u64));
                }
                {
                    let a0: u128 = (fs(r.v[1], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.compare(r.x[9] as u32 as u64, 1u64, 32, false);
                r.x[9] = ((r.x[9] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327120564;
                    continue;
                }
                pc = 4327120588;
            }
            4327120588 => {
                pc = 4327120464;
                continue;
            }
            4327120592 => {
                {
                    let addr = (r.x[19]).wrapping_add(24u64);
                    r.x[25] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[19]).wrapping_add(28u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 4);
                    r.v[0] = m.read(addr.wrapping_add(4), 4);
                }
                r.v[1] = (fs(r.v[2], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.float_compare(fs(r.v[1], 0) as f64, fs(r.v[0], 0) as f64);
                if r.z || r.n != r.o {
                    pc = 4327120632;
                    continue;
                }
                pc = 4327120620;
            }
            4327120620 => {
                r.v[0] = (fs(r.v[1], 0) - fs(r.v[0], 0)).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[25] = ((r.x[25] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                pc = 4327120632;
            }
            4327120632 => {
                if r.x[25] as u32 as u64 == 0 {
                    pc = 4327120324;
                    continue;
                }
                pc = 4327120636;
            }
            4327120636 => {
                {
                    let addr = (r.x[31]).wrapping_add(12u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[13] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(48u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[9] =
                    ((r.x[25] as u32 as u64).wrapping_add(r.x[23] as u32 as u64)) as u32 as u64;
                r.compare(r.x[9] as u32 as u64, r.x[8] as u32 as u64, 32, false);
                r.x[8] = ((r.x[9] as u32 as u64).wrapping_sub(r.x[8] as u32 as u64)) as u32 as u64;
                if !r.c || r.z {
                    pc = 4327120780;
                    continue;
                }
                pc = 4327120656;
            }
            4327120656 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                r.x[25] =
                    ((r.x[25] as u32 as u64).wrapping_sub(r.x[8] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[20] = (((r.x[25]) & 0xffffffffu64) << 2);
                r.x[8] = ((r.x[26] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[27] = (((r.x[8]) & 0xffffffffu64) << 3);
                pc = 4327120680;
            }
            4327120680 => {
                r.x[26] = ((r.x[26] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[27]);
                    r.x[21] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(188u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[25] as u32 as u64)) as u32 as u64;
                r.x[28] = ((r.x[25]).wrapping_shl((2u64) as u32));
                r.x[1] = ((r.x[21]).wrapping_add(r.x[28]));
                r.x[22] = (((r.x[8]) & 0xffffffffu64) << 2);
                r.x[0] = (r.x[21]);
                r.x[2] = (r.x[22]);
                dispatch(m, r, 0x10434a4fc);
                r.x[0] = ((r.x[21]).wrapping_add(r.x[22]));
                {
                    let addr = (r.x[24]).wrapping_add(r.x[27]);
                    r.x[1] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[2] = (r.x[20]);
                dispatch(m, r, 0x10434a4f0);
                {
                    let addr = (r.x[24]).wrapping_add(r.x[27]);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[8] = ((r.x[8]).wrapping_add(r.x[28]));
                {
                    let addr = (r.x[24]).wrapping_add(r.x[27]);
                    m.write(addr.wrapping_add(0), 8, (r.x[8]) as u128);
                }
                r.x[27] = ((r.x[27]).wrapping_sub(8u64));
                if r.x[26] as u32 as u64 != 0 {
                    pc = 4327120680;
                    continue;
                }
                pc = 4327120764;
            }
            4327120764 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[9] =
                    ((r.x[25] as u32 as u64).wrapping_add(r.x[23] as u32 as u64)) as u32 as u64;
                r.x[23] = ((r.x[9] as u32 as u64).wrapping_add(r.x[8] as u32 as u64)) as u32 as u64;
                pc = 4327120320;
                continue;
            }
            4327120780 => {
                {
                    let addr = (r.x[31]).wrapping_add(8u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[9] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[23] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[27] = (r.x[25] as u32 as u64) as u32 as u64;
                r.x[20] = (((r.x[25]) & 0xffffffffu64) << 2);
                r.x[8] = ((r.x[23] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[28] = (((r.x[8]) & 0xffffffffu64) << 3);
                pc = 4327120804;
            }
            4327120804 => {
                r.x[23] = ((r.x[23] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[28]);
                    r.x[21] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(188u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[25] as u32 as u64)) as u32 as u64;
                r.x[26] = ((r.x[27]).wrapping_shl((2u64) as u32));
                r.x[1] = ((r.x[21]).wrapping_add(r.x[26]));
                r.x[22] = (((r.x[8]) & 0xffffffffu64) << 2);
                r.x[0] = (r.x[21]);
                r.x[2] = (r.x[22]);
                dispatch(m, r, 0x10434a4fc);
                r.x[0] = ((r.x[21]).wrapping_add(r.x[22]));
                {
                    let addr = (r.x[24]).wrapping_add(r.x[28]);
                    r.x[1] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[2] = (r.x[20]);
                dispatch(m, r, 0x10434a4f0);
                {
                    let addr = (r.x[24]).wrapping_add(r.x[28]);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[8] = ((r.x[8]).wrapping_add(r.x[26]));
                {
                    let addr = (r.x[24]).wrapping_add(r.x[28]);
                    m.write(addr.wrapping_add(0), 8, (r.x[8]) as u128);
                }
                r.x[28] = ((r.x[28]).wrapping_sub(8u64));
                if r.x[23] as u32 as u64 != 0 {
                    pc = 4327120804;
                    continue;
                }
                pc = 4327120888;
            }
            4327120888 => {
                {
                    let addr = (r.x[31]).wrapping_add(8u64);
                    r.x[23] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                pc = 4327120320;
                continue;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn up(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327121200u64;
    loop {
        match pc {
            4327121200 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-80i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[26]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[25]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[24]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[23]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(64u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327121904;
                    continue;
                }
                pc = 4327121236;
            }
            4327121236 => {
                r.x[22] = (0u64);
                r.x[23] = (0u64) as u32 as u64;
                r.x[24] = (32u64) as u32 as u64;
                pc = 4327121320;
                continue;
            }
            4327121252 => {
                {
                    let addr = (r.x[16]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[26] = (8u64) as u32 as u64;
                pc = 4327121260;
            }
            4327121260 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[0] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[21] = ((r.x[26] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                r.x[2] = (r.x[21]);
                dispatch(m, r, 0x10434a4fc);
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[0] = ((r.x[8]).wrapping_add(
                    ((r.x[26] as u32 as u64) as u32 as i32 as i64 as u64).wrapping_shl(2),
                ));
                r.x[2] =
                    ((r.x[24] as u32 as u64).wrapping_sub(r.x[21] as u32 as u64)) as u32 as u64;
                r.x[1] = (r.x[20]);
                dispatch(m, r, 0x10434a4f0);
                r.x[22] = ((r.x[22]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[22], r.x[8], 64, false);
                if r.c {
                    pc = 4327121888;
                    continue;
                }
                pc = 4327121320;
            }
            4327121320 => {
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[25] = ((r.x[22]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[19]).wrapping_add(80u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[25]);
                    r.x[1] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[10] = ((r.x[1]).wrapping_add(32u64));
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                r.x[13] = ((fs(r.v[0], 0)) as u32 as u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(48u64);
                    r.x[12] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                    r.x[11] = (m.read(addr.wrapping_add(4), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[14] as u32 as u64 == 0 {
                    pc = 4327121416;
                    continue;
                }
                pc = 4327121368;
            }
            4327121368 => {
                r.x[14] = (r.x[20]);
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[14] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(3));
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[12] = ((r.x[12] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let a = (r.v[0] >> 0) & 0xffffffff;
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                if r.x[13] as u32 as u64 != 0 {
                    pc = 4327121496;
                    continue;
                }
                pc = 4327121400;
            }
            4327121400 => {
                r.x[14] = (0u64);
                r.x[15] = (0u64);
                if r.x[11] as u32 as u64 != 0 {
                    pc = 4327121524;
                    continue;
                }
                pc = 4327121412;
            }
            4327121412 => {
                pc = 4327121252;
                continue;
            }
            4327121416 => {
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327121428;
                    continue;
                }
                pc = 4327121420;
            }
            4327121420 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                pc = 4327121488;
                continue;
            }
            4327121428 => {
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[25]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[13] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[1]).wrapping_add(4u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                }
                {
                    let addr = (r.x[1]).wrapping_add(20u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[1]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[1]).wrapping_add(28u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[1]).wrapping_add(24u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[13] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add((r.x[22]).wrapping_shl(2));
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[1]).wrapping_add(28u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[13] = (1u64) as u32 as u64;
                pc = 4327121488;
            }
            4327121488 => {
                r.x[14] = ((r.x[16]).wrapping_add((r.x[22]).wrapping_shl(2)));
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                pc = 4327121496;
            }
            4327121496 => {
                r.x[14] = ((r.x[13] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(4u64)) as u32 as u64;
                r.x[15] = (((r.x[14]) & 0xffffffffu64) << 4);
                r.x[14] = ((r.x[10]).wrapping_add(r.x[15]));
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[17] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[15] = ((r.x[17]).wrapping_add(r.x[15]));
                if r.x[11] as u32 as u64 == 0 {
                    pc = 4327121252;
                    continue;
                }
                pc = 4327121524;
            }
            4327121524 => {
                r.x[26] = (8u64) as u32 as u64;
                pc = 4327121556;
                continue;
            }
            4327121532 => {
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                    r.x[16] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[16]) as u128);
                }
                r.x[12] = ((r.x[12] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                pc = 4327121548;
            }
            4327121548 => {
                r.compare(r.x[11] as u32 as u64, 1u64, 32, false);
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327121844;
                    continue;
                }
                pc = 4327121556;
            }
            4327121556 => {
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327121764;
                    continue;
                }
                pc = 4327121560;
            }
            4327121560 => {
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.v[3] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[2], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 32) & 0xffffffff;
                    r.v[2] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[2] = lane(r.v[2], 1, 32, ((r.v[1] >> 64) & 0xffffffff));
                r.v[4] = ((r.v[1] >> 32) | (r.v[1] << 96));
                r.v[4] = lane(r.v[4], 0, 64, ((r.v[2] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[4]);
                }
                {
                    let addr = (r.x[15]).wrapping_add(16u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[14]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                }
                {
                    let addr = (r.x[15]).wrapping_add(32u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[14]).wrapping_add(32u64);
                    r.v[3] = m.read(addr.wrapping_add(0), 16);
                    r.v[4] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[14]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                }
                {
                    let addr = (r.x[15]).wrapping_add(48u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[4], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[4], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[4], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[4], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.x[15] = ((r.x[15]).wrapping_add(64u64));
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[14]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[25]);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    m.write(addr, 4, r.v[1] >> 0);
                    r.x[16] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[14]).wrapping_add(28u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[14]).wrapping_add(12u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[14]).wrapping_add(44u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[14]).wrapping_add(28u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[14]).wrapping_add(60u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[14]).wrapping_add(44u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[14]).wrapping_add(60u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[14] = ((r.x[14]).wrapping_add(64u64));
                {
                    let addr = (r.x[9]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[16]) as u128);
                }
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[13] as u32 as u64, r.x[16] as u32 as u64, 32, false);
                if !r.z {
                    pc = 4327121548;
                    continue;
                }
                pc = 4327121760;
            }
            4327121760 => {
                pc = 4327121828;
                continue;
            }
            4327121764 => {
                {
                    let addr = (r.x[9]).wrapping_add((r.x[22]).wrapping_shl(3));
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                if r.x[26] as u32 as u64 == 0 {
                    pc = 4327121788;
                    continue;
                }
                pc = 4327121772;
            }
            4327121772 => {
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.x[1] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.x[26] = ((r.x[26] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                pc = 4327121796;
                continue;
            }
            4327121788 => {
                {
                    let addr = (r.x[20]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.x[20] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                pc = 4327121796;
            }
            4327121796 => {
                r.x[14] = ((r.x[14]).wrapping_add(4u64));
                {
                    let addr = (r.x[9]).wrapping_add((r.x[22]).wrapping_shl(3));
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[14] = (r.x[10]);
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[13] as u32 as u64, r.x[16] as u32 as u64, 32, false);
                if !r.z {
                    pc = 4327121548;
                    continue;
                }
                pc = 4327121828;
            }
            4327121828 => {
                r.x[13] = (0u64) as u32 as u64;
                if r.x[12] as u32 as u64 != 0 {
                    pc = 4327121532;
                    continue;
                }
                pc = 4327121836;
            }
            4327121836 => {
                r.x[23] = (1u64) as u32 as u64;
                pc = 4327121548;
                continue;
            }
            4327121844 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                if r.x[26] as u32 as u64 != 0 {
                    pc = 4327121260;
                    continue;
                }
                pc = 4327121856;
            }
            4327121856 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(3));
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[20]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                    r.v[1] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let addr = (r.x[8]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                r.x[22] = ((r.x[22]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[22], r.x[8], 64, false);
                if !r.c {
                    pc = 4327121320;
                    continue;
                }
                pc = 4327121888;
            }
            4327121888 => {
                if (r.x[23] as u32 as u64 & (1u64 << 0)) == 0 {
                    pc = 4327121904;
                    continue;
                }
                pc = 4327121892;
            }
            4327121892 => {
                r.x[8] = (1u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                pc = 4327121908;
                continue;
            }
            4327121904 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                pc = 4327121908;
            }
            4327121908 => {
                {
                    let addr = (r.x[19]).wrapping_add(52u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[10] = ((r.x[8] as u32 as u64)
                    .checked_div(r.x[9] as u32 as u64)
                    .unwrap_or(0)) as u32 as u64;
                r.x[8] = ((r.x[8] as u32 as u64)
                    .wrapping_sub((r.x[10] as u32 as u64).wrapping_mul(r.x[9] as u32 as u64)))
                    as u32 as u64;
                r.v[0] = ((r.x[8] as u32 as u64) as u32 as f32).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[2] = ((r.x[9] as u32 as u64) as u32 as f32).to_bits() as u128;
                r.v[0] = (fs(r.v[1], 0) + fs(r.v[0], 0)).to_bits() as u128;
                r.float_compare(fs(r.v[0], 0) as f64, fs(r.v[2], 0) as f64);
                r.v[1] = 0u64 as u128;
                r.v[1] = (if r.n != r.o {
                    fs(r.v[1], 0)
                } else {
                    fs(r.v[2], 0)
                })
                .to_bits() as u128;
                r.v[0] = (fs(r.v[0], 0) - fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[23] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[25] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(80u64));
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn down(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327121984u64;
    loop {
        match pc {
            4327121984 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-96i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[28]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[27]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[26]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[25]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[24]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[23]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(80u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327122796;
                    continue;
                }
                pc = 4327122024;
            }
            4327122024 => {
                r.x[20] = (0u64);
                r.x[21] = (0u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[22] = ((r.x[8] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[8] = ((r.x[22] as u32 as u64).wrapping_shl((4u64) as u32)) as u32 as u64;
                r.x[23] = ((r.x[8] as u32 as u64) as u32 as i32 as i64 as u64);
                pc = 4327122088;
                continue;
            }
            4327122052 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[8] = ((r.x[8]).wrapping_add((r.x[23]).wrapping_shl(2)));
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                    r.v[1] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let addr = (r.x[8]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                pc = 4327122072;
            }
            4327122072 => {
                r.x[20] = ((r.x[20]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[20], r.x[8], 64, false);
                if r.c {
                    pc = 4327122780;
                    continue;
                }
                pc = 4327122088;
            }
            4327122088 => {
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[9] = ((r.x[20]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[8]).wrapping_add(r.x[9]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[9]);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(80u64);
                    r.x[11] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[12] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[12] = ((r.x[12]).wrapping_add((r.x[20]).wrapping_shl(2)));
                {
                    let addr = (r.x[19]).wrapping_add(52u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[27] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[17] = (r.x[14]);
                if r.x[16] as u32 as u64 != 0 {
                    pc = 4327122180;
                    continue;
                }
                pc = 4327122140;
            }
            4327122140 => {
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327122180;
                    continue;
                }
                pc = 4327122144;
            }
            4327122144 => {
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[9]);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[16] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[9]);
                    m.write(addr.wrapping_add(0), 8, (r.x[16]) as u128);
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[27] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[9]);
                    r.x[17] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[16] = (r.x[27]);
                pc = 4327122180;
            }
            4327122180 => {
                r.x[1] = ((r.x[14]).wrapping_add((r.x[23]).wrapping_shl(2)));
                r.x[24] =
                    ((r.x[27] as u32 as u64).wrapping_sub(r.x[16] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(48u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[14] as u32 as u64 == 0 {
                    pc = 4327122052;
                    continue;
                }
                pc = 4327122196;
            }
            4327122196 => {
                r.x[16] =
                    ((r.x[22] as u32 as u64).wrapping_sub(r.x[24] as u32 as u64)) as u32 as u64;
                r.x[25] = ((r.x[15])
                    .wrapping_add(((r.x[16] as u32 as u64) as u32 as u64).wrapping_shl(2)));
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[16] = ((r.x[24] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                r.x[16] = ((r.x[16] as u32 as u64).wrapping_sub(4u64)) as u32 as u64;
                r.x[0] = ((((((r.x[16]) << 32) as i64) >> 32) as u64).wrapping_shl(4));
                r.x[16] = ((r.x[15]).wrapping_add(r.x[0]));
                r.x[15] = ((r.x[17]).wrapping_add(r.x[0]));
                r.x[26] = (8u64) as u32 as u64;
                pc = 4327122448;
                continue;
            }
            4327122236 => {
                {
                    let addr = (r.x[8]).wrapping_add(r.x[9]);
                    r.x[17] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[17]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                    r.x[17] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[9]);
                    m.write(addr.wrapping_add(0), 8, (r.x[17]) as u128);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.v[3] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[2], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 32) & 0xffffffff;
                    r.v[2] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[2] = lane(r.v[2], 1, 32, ((r.v[1] >> 64) & 0xffffffff));
                r.v[4] = ((r.v[1] >> 32) | (r.v[1] << 96));
                r.v[4] = lane(r.v[4], 0, 64, ((r.v[2] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[4]);
                }
                {
                    let addr = (r.x[16]).wrapping_add(16u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                }
                {
                    let addr = (r.x[16]).wrapping_add(32u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[15]).wrapping_add(32u64);
                    r.v[3] = m.read(addr.wrapping_add(0), 16);
                    r.v[4] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                }
                {
                    let addr = (r.x[16]).wrapping_add(48u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                }
                r.x[16] = ((r.x[16]).wrapping_add(64u64));
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[2], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) + fs(r.v[4], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) + fs(r.v[4], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) + fs(r.v[4], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) + fs(r.v[4], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[0] >> 32) & 0xffffffff;
                    r.v[2] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[2] = lane(r.v[2], 1, 32, ((r.v[0] >> 64) & 0xffffffff));
                r.v[0] = ((r.v[0] >> 32) | (r.v[0] << 96));
                r.v[0] = lane(r.v[0], 0, 64, ((r.v[2] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[1], 0) + fs(r.v[0], 0)).to_bits() as u128;
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    r.x[15] = ((r.x[15]).wrapping_add(60u64));
                    let addr = r.x[15];
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[15]).wrapping_add((-32i64 as u64));
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[15]).wrapping_add((-48i64 as u64));
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[15]).wrapping_add((-16i64 as u64));
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[15]).wrapping_add((-32i64 as u64));
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[15]).wrapping_add((-16i64 as u64));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[17] = (r.x[15]);
                r.x[15] = ((r.x[15]).wrapping_add(4u64));
                {
                    let addr = (r.x[17]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[24] = ((r.x[24] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327122600;
                    continue;
                }
                pc = 4327122448;
            }
            4327122448 => {
                r.compare(r.x[24] as u32 as u64, r.x[22] as u32 as u64, 32, false);
                if !r.z {
                    pc = 4327122236;
                    continue;
                }
                pc = 4327122456;
            }
            4327122456 => {
                if r.x[26] as u32 as u64 == 0 {
                    pc = 4327122480;
                    continue;
                }
                pc = 4327122460;
            }
            4327122460 => {
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[1] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                r.x[26] = ((r.x[26] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                pc = 4327122500;
                continue;
            }
            4327122480 => {
                {
                    let addr = (r.x[25]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[15] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[25] = ((r.x[25]).wrapping_add((r.x[15]).wrapping_shl(2)));
                pc = 4327122500;
            }
            4327122500 => {
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[9]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[15] = ((r.x[15]).wrapping_add(4u64));
                {
                    let addr = (r.x[8]).wrapping_add(r.x[9]);
                    m.write(addr.wrapping_add(0), 8, (r.x[15]) as u128);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[9]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327122580;
                    continue;
                }
                pc = 4327122528;
            }
            4327122528 => {
                {
                    let addr = (r.x[19]).wrapping_add(248u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[11]).wrapping_add(r.x[9]);
                    r.x[17] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[17]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[17] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[9]);
                    m.write(addr.wrapping_add(0), 8, (r.x[17]) as u128);
                }
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[24] = (-1i64 as u64) as u32 as u64;
                r.x[17] = (r.x[12]);
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[24] = ((r.x[24] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327122448;
                    continue;
                }
                pc = 4327122576;
            }
            4327122576 => {
                pc = 4327122600;
                continue;
            }
            4327122580 => {
                r.x[21] = (1u64) as u32 as u64;
                r.x[24] = (-1i64 as u64) as u32 as u64;
                r.x[24] = ((r.x[24] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327122448;
                    continue;
                }
                pc = 4327122600;
            }
            4327122600 => {
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[27] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[0] = ((r.x[8]).wrapping_add((r.x[23]).wrapping_shl(2)));
                if r.x[26] as u32 as u64 == 0 {
                    pc = 4327122684;
                    continue;
                }
                pc = 4327122620;
            }
            4327122620 => {
                r.x[2] = ((r.x[26] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                dispatch(m, r, 0x10434a4fc);
                r.compare(r.x[26] as u32 as u64, 8u64, 32, false);
                r.x[8] = ((r.x[26] as u32 as u64).wrapping_sub(8u64)) as u32 as u64;
                if r.z {
                    pc = 4327122072;
                    continue;
                }
                pc = 4327122636;
            }
            4327122636 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[10] =
                    ((r.x[26] as u32 as u64).wrapping_add(r.x[23] as u32 as u64)) as u32 as u64;
                r.x[9] = ((r.x[9]).wrapping_add(
                    ((r.x[10] as u32 as u64) as u32 as i32 as i64 as u64).wrapping_shl(2),
                ));
                {
                    let addr = (r.x[19]).wrapping_add(252u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[10] = ((r.x[10]).wrapping_shl((2u64) as u32));
                pc = 4327122660;
            }
            4327122660 => {
                {
                    let addr = (r.x[25]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[9] = (addr.wrapping_add(4u64));
                }
                r.x[25] = ((r.x[25]).wrapping_add(r.x[10]));
                r.compare(r.x[8] as u32 as u64, 1u64, 32, true);
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                if !r.c {
                    pc = 4327122660;
                    continue;
                }
                pc = 4327122680;
            }
            4327122680 => {
                pc = 4327122072;
                continue;
            }
            4327122684 => {
                {
                    let addr = (r.x[25]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[8] = ((r.x[27]).wrapping_shl((2u64) as u32));
                r.x[9] = ((r.x[25]).wrapping_add(r.x[8]));
                r.x[10] = ((r.x[9]).wrapping_add(r.x[8]));
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(4u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[10]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(8u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[9] = ((r.x[10]).wrapping_add(r.x[8]));
                r.x[10] = ((r.x[9]).wrapping_add(r.x[8]));
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(12u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[10]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[9] = ((r.x[10]).wrapping_add(r.x[8]));
                r.x[10] = ((r.x[9]).wrapping_add(r.x[8]));
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(20u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[10]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(24u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[8]);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[0]).wrapping_add(28u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                pc = 4327122072;
                continue;
            }
            4327122780 => {
                if (r.x[21] as u32 as u64 & (1u64 << 0)) == 0 {
                    pc = 4327122792;
                    continue;
                }
                pc = 4327122784;
            }
            4327122784 => {
                r.x[8] = (0u64) as u32 as u64;
                pc = 4327122796;
                continue;
            }
            4327122792 => {
                r.x[8] =
                    ((r.x[27] as u32 as u64).wrapping_sub(r.x[24] as u32 as u64)) as u32 as u64;
                pc = 4327122796;
            }
            4327122796 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[23] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[25] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[28] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[27] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(96u64));
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn double(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327122828u64;
    loop {
        match pc {
            4327122828 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-80i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[26]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[25]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[24]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[23]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(64u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327123448;
                    continue;
                }
                pc = 4327122864;
            }
            4327122864 => {
                r.x[22] = (0u64);
                r.x[23] = (0u64) as u32 as u64;
                r.x[24] = (32u64) as u32 as u64;
                pc = 4327122920;
                continue;
            }
            4327122880 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                pc = 4327122888;
            }
            4327122888 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(3));
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[20]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                    r.v[1] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let addr = (r.x[8]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                r.x[22] = ((r.x[22]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[22], r.x[8], 64, false);
                if r.c {
                    pc = 4327123432;
                    continue;
                }
                pc = 4327122920;
            }
            4327122920 => {
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[25] = ((r.x[22]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[19]).wrapping_add(80u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[25]);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                r.x[13] = ((fs(r.v[0], 0)) as u32 as u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(48u64);
                    r.x[12] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                    r.x[11] = (m.read(addr.wrapping_add(4), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[14] as u32 as u64 == 0 {
                    pc = 4327122996;
                    continue;
                }
                pc = 4327122964;
            }
            4327122964 => {
                r.x[14] = (r.x[20]);
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[14] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(3));
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[14]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[12] = ((r.x[12] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.x[11] as u32 as u64 != 0 {
                    pc = 4327123008;
                    continue;
                }
                pc = 4327122992;
            }
            4327122992 => {
                pc = 4327123364;
                continue;
            }
            4327122996 => {
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327123300;
                    continue;
                }
                pc = 4327123000;
            }
            4327123000 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                if r.x[11] as u32 as u64 == 0 {
                    pc = 4327123364;
                    continue;
                }
                pc = 4327123008;
            }
            4327123008 => {
                r.x[14] = ((r.x[14]).wrapping_add((r.x[22]).wrapping_shl(2)));
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.x[26] = (8u64) as u32 as u64;
                r.x[1] = (r.x[9]);
                pc = 4327123056;
                continue;
            }
            4327123028 => {
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.x[1] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[10]).wrapping_add((r.x[22]).wrapping_shl(3));
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.x[26] = ((r.x[26] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[13] = (1u64) as u32 as u64;
                pc = 4327123048;
            }
            4327123048 => {
                r.compare(r.x[11] as u32 as u64, 1u64, 32, false);
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327123284;
                    continue;
                }
                pc = 4327123056;
            }
            4327123056 => {
                if r.x[13] as u32 as u64 != 0 {
                    pc = 4327123096;
                    continue;
                }
                pc = 4327123060;
            }
            4327123060 => {
                {
                    let addr = (r.x[10]).wrapping_add((r.x[22]).wrapping_shl(3));
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[14] = ((r.x[13]).wrapping_add(4u64));
                if r.x[26] as u32 as u64 != 0 {
                    pc = 4327123028;
                    continue;
                }
                pc = 4327123072;
            }
            4327123072 => {
                {
                    let addr = (r.x[20]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.x[20] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[10]).wrapping_add((r.x[22]).wrapping_shl(3));
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.compare(r.x[11] as u32 as u64, 1u64, 32, false);
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327122880;
                    continue;
                }
                pc = 4327123092;
            }
            4327123092 => {
                r.x[26] = (0u64) as u32 as u64;
                pc = 4327123096;
            }
            4327123096 => {
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[9]).wrapping_add(32u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.v[3] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[4] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[4] = lane(r.v[4], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[5] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[5] = lane(r.v[5], 0, 64, ((r.v[4] >> 0) & 0xffffffffffffffff));
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 96) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let a0: u128 = (r.v[3] >> 0) & 0xffffffff;
                    let a1: u128 = (r.v[1] >> 0) & 0xffffffff;
                    let a2: u128 = (r.v[3] >> 32) & 0xffffffff;
                    let a3: u128 = (r.v[1] >> 32) & 0xffffffff;
                    r.v[3] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.v[1] = ((r.v[3] >> 96) | (r.v[1] << 32));
                r.v[1] = lane(r.v[1], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[9]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 16, r.v[5]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[13]).wrapping_add(16u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[9]).wrapping_add(64u64);
                    r.v[3] = m.read(addr.wrapping_add(0), 16);
                    r.v[4] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[3], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[3], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[3], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[3], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[3] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[3] >> 32) & 0xffffffff;
                    r.v[5] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[5] = lane(r.v[5], 1, 32, ((r.v[3] >> 64) & 0xffffffff));
                r.v[3] = ((r.v[3] >> 32) | (r.v[3] << 96));
                r.v[3] = lane(r.v[3], 0, 64, ((r.v[5] >> 0) & 0xffffffffffffffff));
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[4], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[4], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[4], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[4], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 96) & 0xffffffff;
                    r.v[4] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let a0: u128 = (r.v[4] >> 0) & 0xffffffff;
                    let a1: u128 = (r.v[1] >> 0) & 0xffffffff;
                    let a2: u128 = (r.v[4] >> 32) & 0xffffffff;
                    let a3: u128 = (r.v[1] >> 32) & 0xffffffff;
                    r.v[4] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.v[1] = ((r.v[4] >> 96) | (r.v[1] << 32));
                r.v[1] = lane(r.v[1], 0, 64, ((r.v[4] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[9]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 16, r.v[3]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[25]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr, 4, r.v[2] >> 0);
                    r.x[13] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[9]).wrapping_add(76u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.v[2] = m.read(addr.wrapping_add(4), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(44u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[9]).wrapping_add(48u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(76u64);
                    m.write(addr.wrapping_add(0), 4, r.v[2] & 0xffffffff);
                    m.write(addr.wrapping_add(4), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[9]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[13] = (0u64) as u32 as u64;
                if r.x[12] as u32 as u64 == 0 {
                    pc = 4327123276;
                    continue;
                }
                pc = 4327123256;
            }
            4327123256 => {
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[14]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                    r.x[14] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[14]) as u128);
                }
                r.x[12] = ((r.x[12] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                pc = 4327123048;
                continue;
            }
            4327123276 => {
                r.x[23] = (1u64) as u32 as u64;
                pc = 4327123048;
                continue;
            }
            4327123284 => {
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[22]).wrapping_shl(2));
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                if r.x[26] as u32 as u64 != 0 {
                    pc = 4327123372;
                    continue;
                }
                pc = 4327123296;
            }
            4327123296 => {
                pc = 4327122888;
                continue;
            }
            4327123300 => {
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[25]);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[13] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[10]).wrapping_add(r.x[25]);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[9]).wrapping_add(4u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                }
                {
                    let addr = (r.x[9]).wrapping_add(20u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[13]) as u128);
                }
                {
                    let addr = (r.x[9]).wrapping_add(28u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[9]).wrapping_add(24u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[13] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[14]).wrapping_add((r.x[22]).wrapping_shl(2));
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(28u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[11] = ((r.x[11] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[13] = (1u64) as u32 as u64;
                if r.x[11] as u32 as u64 != 0 {
                    pc = 4327123008;
                    continue;
                }
                pc = 4327123364;
            }
            4327123364 => {
                r.x[26] = (8u64) as u32 as u64;
                r.x[1] = (r.x[9]);
                pc = 4327123372;
            }
            4327123372 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[0] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[21] = ((r.x[26] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                r.x[2] = (r.x[21]);
                dispatch(m, r, 0x10434a4fc);
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[25]);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[0] = ((r.x[8]).wrapping_add(
                    ((r.x[26] as u32 as u64) as u32 as i32 as i64 as u64).wrapping_shl(2),
                ));
                r.x[2] =
                    ((r.x[24] as u32 as u64).wrapping_sub(r.x[21] as u32 as u64)) as u32 as u64;
                r.x[1] = (r.x[20]);
                dispatch(m, r, 0x10434a4f0);
                r.x[22] = ((r.x[22]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[22], r.x[8], 64, false);
                if !r.c {
                    pc = 4327122920;
                    continue;
                }
                pc = 4327123432;
            }
            4327123432 => {
                if (r.x[23] as u32 as u64 & (1u64 << 0)) == 0 {
                    pc = 4327123448;
                    continue;
                }
                pc = 4327123436;
            }
            4327123436 => {
                r.x[8] = (1u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                pc = 4327123452;
                continue;
            }
            4327123448 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                pc = 4327123452;
            }
            4327123452 => {
                {
                    let addr = (r.x[19]).wrapping_add(52u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[8] = (r.x[8] as u32 as u64 & 1u64) as u32 as u64;
                r.v[0] = ((r.x[8] as u32 as u64) as u32 as f32).to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[1], 0) + fs(r.v[0], 0)).to_bits() as u128;
                r.v[1] = (-2.0f32).to_bits() as u128;
                r.v[1] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                r.v[2] = (2.0f32).to_bits() as u128;
                r.float_compare(fs(r.v[0], 0) as f64, fs(r.v[2], 0) as f64);
                r.v[0] = (if r.n != r.o {
                    fs(r.v[0], 0)
                } else {
                    fs(r.v[1], 0)
                })
                .to_bits() as u128;
                {
                    let addr = (r.x[19]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[23] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[25] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(80u64));
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn half(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327123520u64;
    loop {
        match pc {
            4327123520 => {
                {
                    r.x[31] = ((r.x[31]).wrapping_add((-96i64 as u64)));
                    let addr = r.x[31];
                    m.write(addr.wrapping_add(0), 8, (r.x[28]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[27]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[26]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[25]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[24]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[23]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[22]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[21]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[20]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[19]) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    m.write(addr.wrapping_add(0), 8, (r.x[29]) as u128);
                    m.write(addr.wrapping_add(8), 8, (r.x[30]) as u128);
                }
                r.x[29] = ((r.x[31]).wrapping_add(80u64));
                r.x[19] = (r.x[0]);
                {
                    let addr = (r.x[0]).wrapping_add(44u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[8] as u32 as u64 == 0 {
                    pc = 4327124364;
                    continue;
                }
                pc = 4327123560;
            }
            4327123560 => {
                r.x[20] = (0u64);
                r.x[21] = (0u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[22] = (2u64) as u32 as u64;
                r.x[23] = (8u64) as u32 as u64;
                r.x[24] = (7u64) as u32 as u64;
                r.x[25] = (16u64) as u32 as u64;
                pc = 4327123680;
                continue;
            }
            4327123592 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[26]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(64u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(8u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(68u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(16u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(72u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(24u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(76u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(32u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(80u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(40u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(84u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(48u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(88u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[26]).wrapping_add(56u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[9]).wrapping_add(92u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                pc = 4327123664;
            }
            4327123664 => {
                r.x[20] = ((r.x[20]).wrapping_add(1u64));
                {
                    let addr = (r.x[19]).wrapping_add(44u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[20], r.x[9], 64, false);
                if r.c {
                    pc = 4327124344;
                    continue;
                }
                pc = 4327123680;
            }
            4327123680 => {
                {
                    let addr = (r.x[19]).wrapping_add(56u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[10] = ((r.x[20]).wrapping_shl((3u64) as u32));
                {
                    let addr = (r.x[19]).wrapping_add(80u64);
                    r.x[11] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    r.x[14] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(224u64);
                    r.x[12] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[12] = ((r.x[12]).wrapping_add((r.x[20]).wrapping_shl(2)));
                {
                    let addr = (r.x[19]).wrapping_add(52u64);
                    r.x[13] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.compare(r.x[16] as u32 as u64, 0u64, 32, false);
                if r.z {
                    r.compare(r.x[13] as u32 as u64, 0u64, 32, false);
                } else {
                    r.flags(4);
                }
                if r.z {
                    pc = 4327123760;
                    continue;
                }
                pc = 4327123728;
            }
            4327123728 => {
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[10]);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[16] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[16]) as u128);
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[16] = (2u64) as u32 as u64;
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[16] as u32 as u64) as u128);
                }
                pc = 4327123760;
            }
            4327123760 => {
                r.x[1] = ((r.x[15]).wrapping_add(64u64));
                r.x[27] =
                    ((r.x[22] as u32 as u64).wrapping_sub(r.x[16] as u32 as u64)) as u32 as u64;
                r.x[15] = ((r.x[16] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[26] = ((r.x[14]).wrapping_add(
                    ((r.x[15] as u32 as u64) as u32 as i32 as i64 as u64).wrapping_shl(2),
                ));
                {
                    let addr = (r.x[19]).wrapping_add(48u64);
                    r.x[14] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                if r.x[14] as u32 as u64 == 0 {
                    pc = 4327124132;
                    continue;
                }
                pc = 4327123784;
            }
            4327123784 => {
                r.x[28] = (8u64) as u32 as u64;
                pc = 4327123984;
                continue;
            }
            4327123792 => {
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    let a = m.read(addr, 4);
                    r.v[0] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                    r.x[15] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[15]) as u128);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 16);
                    r.v[3] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[2], 0) + fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[2], 1) + fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[2], 2) + fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[2], 3) + fs(r.v[1], 3)).to_bits() as u128;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[2] >> 32) & 0xffffffff;
                    r.v[4] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[4] = lane(r.v[4], 1, 32, ((r.v[2] >> 64) & 0xffffffff));
                r.v[2] = ((r.v[2] >> 32) | (r.v[2] << 96));
                r.v[2] = lane(r.v[2], 0, 64, ((r.v[4] >> 0) & 0xffffffffffffffff));
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[3], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[3], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[3], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[3], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 96) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let a0: u128 = (r.v[3] >> 0) & 0xffffffff;
                    let a1: u128 = (r.v[1] >> 0) & 0xffffffff;
                    let a2: u128 = (r.v[3] >> 32) & 0xffffffff;
                    let a3: u128 = (r.v[1] >> 32) & 0xffffffff;
                    r.v[3] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.v[1] = ((r.v[3] >> 96) | (r.v[1] << 32));
                r.v[1] = lane(r.v[1], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[2]);
                    m.write(addr.wrapping_add(16), 16, r.v[1]);
                }
                {
                    let addr = (r.x[19]).wrapping_add(128u64);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(16u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                }
                {
                    let a0: u128 = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) * fs(r.v[1], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) * fs(r.v[1], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) * fs(r.v[1], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let addr = (r.x[15]).wrapping_add(32u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 16);
                    r.v[2] = m.read(addr.wrapping_add(16), 16);
                }
                {
                    let a0: u128 = (fs(r.v[1], 0) + fs(r.v[0], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[1], 1) + fs(r.v[0], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[1], 2) + fs(r.v[0], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[1], 3) + fs(r.v[0], 3)).to_bits() as u128;
                    r.v[1] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[1] >> 32) & 0xffffffff;
                    r.v[3] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                r.v[3] = lane(r.v[3], 1, 32, ((r.v[1] >> 64) & 0xffffffff));
                r.v[1] = ((r.v[1] >> 32) | (r.v[1] << 96));
                r.v[1] = lane(r.v[1], 0, 64, ((r.v[3] >> 0) & 0xffffffffffffffff));
                {
                    let a0: u128 = (fs(r.v[0], 0) + fs(r.v[2], 0)).to_bits() as u128;
                    let a1: u128 = (fs(r.v[0], 1) + fs(r.v[2], 1)).to_bits() as u128;
                    let a2: u128 = (fs(r.v[0], 2) + fs(r.v[2], 2)).to_bits() as u128;
                    let a3: u128 = (fs(r.v[0], 3) + fs(r.v[2], 3)).to_bits() as u128;
                    r.v[0] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                {
                    let a = (r.v[0] >> 96) & 0xffffffff;
                    r.v[2] = (a << 0) | (a << 32) | (a << 64) | (a << 96);
                }
                {
                    let a0: u128 = (r.v[2] >> 0) & 0xffffffff;
                    let a1: u128 = (r.v[0] >> 0) & 0xffffffff;
                    let a2: u128 = (r.v[2] >> 32) & 0xffffffff;
                    let a3: u128 = (r.v[0] >> 32) & 0xffffffff;
                    r.v[2] = (a0 << 0) | (a1 << 32) | (a2 << 64) | (a3 << 96);
                }
                r.v[0] = ((r.v[2] >> 96) | (r.v[0] << 32));
                r.v[0] = lane(r.v[0], 0, 64, ((r.v[2] >> 0) & 0xffffffffffffffff));
                {
                    let addr = (r.x[15]).wrapping_add(32u64);
                    m.write(addr.wrapping_add(0), 16, r.v[1]);
                    m.write(addr.wrapping_add(16), 16, r.v[0]);
                }
                {
                    let addr = (r.x[8]).wrapping_add(r.x[10]);
                    r.x[16] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[16]).wrapping_add(12u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[15] = (r.x[16]);
                {
                    r.x[15] = ((r.x[15]).wrapping_add(16u64));
                    let addr = r.x[15];
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                }
                {
                    let addr = (r.x[15]).wrapping_add(28u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                    r.v[2] = m.read(addr.wrapping_add(4), 4);
                }
                {
                    let addr = (r.x[16]).wrapping_add(12u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                {
                    let addr = (r.x[15]).wrapping_add(28u64);
                    m.write(addr.wrapping_add(0), 4, r.v[2] & 0xffffffff);
                    m.write(addr.wrapping_add(4), 4, r.v[0] & 0xffffffff);
                }
                r.x[27] = (1u64) as u32 as u64;
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if r.z {
                    pc = 4327124124;
                    continue;
                }
                pc = 4327123984;
            }
            4327123984 => {
                if r.x[27] as u32 as u64 == 0 {
                    pc = 4327123792;
                    continue;
                }
                pc = 4327123988;
            }
            4327123988 => {
                if r.x[28] as u32 as u64 == 0 {
                    pc = 4327124076;
                    continue;
                }
                pc = 4327123992;
            }
            4327123992 => {
                {
                    let addr = (r.x[1]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[1] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                r.x[28] = ((r.x[28] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[15] = ((r.x[15]).wrapping_add(4u64));
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[15]) as u128);
                }
                r.x[27] = (0u64) as u32 as u64;
                if r.x[13] as u32 as u64 == 0 {
                    pc = 4327124112;
                    continue;
                }
                pc = 4327124032;
            }
            4327124032 => {
                {
                    let addr = (r.x[19]).wrapping_add(248u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) * fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[11]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[15] = (addr.wrapping_add(4u64));
                }
                {
                    let addr = (r.x[11]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[15]) as u128);
                }
                r.x[13] = ((r.x[13] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                r.x[15] = (r.x[12]);
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, (0u64) as u128);
                }
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327123984;
                    continue;
                }
                pc = 4327124072;
            }
            4327124072 => {
                pc = 4327124124;
                continue;
            }
            4327124076 => {
                {
                    let addr = (r.x[26]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[26] = (addr.wrapping_add(8u64));
                }
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    r.v[1] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[0] = (fs(r.v[0], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[12]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    r.x[15] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[15] = ((r.x[15]).wrapping_add(4u64));
                {
                    let addr = (r.x[9]).wrapping_add(r.x[10]);
                    m.write(addr.wrapping_add(0), 8, (r.x[15]) as u128);
                }
                r.x[27] = (0u64) as u32 as u64;
                if r.x[13] as u32 as u64 != 0 {
                    pc = 4327124032;
                    continue;
                }
                pc = 4327124112;
            }
            4327124112 => {
                r.x[21] = (1u64) as u32 as u64;
                r.compare(r.x[14] as u32 as u64, 1u64, 32, false);
                r.x[14] = ((r.x[14] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327123984;
                    continue;
                }
                pc = 4327124124;
            }
            4327124124 => {
                if r.x[28] as u32 as u64 != 0 {
                    pc = 4327124136;
                    continue;
                }
                pc = 4327124128;
            }
            4327124128 => {
                pc = 4327123592;
                continue;
            }
            4327124132 => {
                r.x[28] = (8u64) as u32 as u64;
                pc = 4327124136;
            }
            4327124136 => {
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                {
                    let addr = (r.x[8]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[0] = ((r.x[8]).wrapping_add(64u64));
                r.x[2] = ((r.x[28] as u32 as u64).wrapping_shl((2u64) as u32)) as u32 as u64;
                dispatch(m, r, 0x10434a4fc);
                {
                    let addr = (r.x[19]).wrapping_add(104u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.compare(r.x[28] as u32 as u64, 8u64, 32, false);
                if r.z {
                    pc = 4327123664;
                    continue;
                }
                pc = 4327124168;
            }
            4327124168 => {
                {
                    let addr = (r.x[8]).wrapping_add((r.x[20]).wrapping_shl(3));
                    r.x[13] = (m.read(addr.wrapping_add(0), 8) as u64);
                }
                r.x[12] = ((r.x[28] as u32 as u64).wrapping_add(16u64)) as u32 as u64;
                r.x[9] = ((r.x[13]).wrapping_add(
                    ((r.x[12] as u32 as u64) as u32 as i32 as i64 as u64).wrapping_shl(2),
                ));
                r.x[10] =
                    ((r.x[23] as u32 as u64).wrapping_sub(r.x[28] as u32 as u64)) as u32 as u64;
                r.x[11] =
                    ((r.x[24] as u32 as u64).wrapping_sub(r.x[28] as u32 as u64)) as u32 as u64;
                r.compare(r.x[11] as u32 as u64, 16u64, 32, false);
                if !r.c {
                    pc = 4327124324;
                    continue;
                }
                pc = 4327124196;
            }
            4327124196 => {
                r.x[14] = ((r.x[12] as u32 as u64) as u32 as i32 as i64 as u64);
                r.x[12] = ((r.x[14]).wrapping_add(r.x[11]));
                r.x[12] = ((r.x[13]).wrapping_add((r.x[12]).wrapping_shl(2)));
                r.x[12] = ((r.x[12]).wrapping_add(4u64));
                r.x[15] = ((r.x[26]).wrapping_add((r.x[11]).wrapping_shl(3)));
                r.x[15] = ((r.x[15]).wrapping_add(4u64));
                r.compare(r.x[9], r.x[15], 64, false);
                if !r.c {
                    r.compare(r.x[26], r.x[12], 64, false);
                } else {
                    r.flags(2);
                }
                if !r.c {
                    pc = 4327124324;
                    continue;
                }
                pc = 4327124232;
            }
            4327124232 => {
                r.x[11] = ((r.x[11]).wrapping_add(1u64));
                r.compare((r.x[11]) & (15u64), 0, 64, false);
                r.c = false;
                r.o = false;
                r.x[12] = ((r.x[11]) & (15u64));
                r.x[12] = (if r.z { r.x[25] } else { r.x[12] });
                r.x[11] = ((r.x[11]).wrapping_sub(r.x[12]));
                r.x[10] =
                    ((r.x[10] as u32 as u64).wrapping_sub(r.x[11] as u32 as u64)) as u32 as u64;
                r.x[9] = ((r.x[9]).wrapping_add((r.x[11]).wrapping_shl(2)));
                r.x[12] = ((r.x[26]).wrapping_add((r.x[11]).wrapping_shl(3)));
                r.x[13] = ((r.x[13]).wrapping_add((r.x[14]).wrapping_shl(2)));
                r.x[13] = ((r.x[13]).wrapping_add(32u64));
                r.x[14] = ((r.x[26]).wrapping_add(64u64));
                pc = 4327124272;
            }
            4327124272 => {
                r.x[15] = ((r.x[14]).wrapping_sub(64u64));
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[0] = lane(r.v[0], 0, 32, m.read(addr + 0, 4));
                    r.v[1] = lane(r.v[1], 0, 32, m.read(addr + 4, 4));
                    r.v[0] = lane(r.v[0], 1, 32, m.read(addr + 8, 4));
                    r.v[1] = lane(r.v[1], 1, 32, m.read(addr + 12, 4));
                    r.v[0] = lane(r.v[0], 2, 32, m.read(addr + 16, 4));
                    r.v[1] = lane(r.v[1], 2, 32, m.read(addr + 20, 4));
                    r.v[0] = lane(r.v[0], 3, 32, m.read(addr + 24, 4));
                    r.v[1] = lane(r.v[1], 3, 32, m.read(addr + 28, 4));
                }
                r.x[15] = ((r.x[14]).wrapping_sub(32u64));
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[2] = lane(r.v[2], 0, 32, m.read(addr + 0, 4));
                    r.v[3] = lane(r.v[3], 0, 32, m.read(addr + 4, 4));
                    r.v[2] = lane(r.v[2], 1, 32, m.read(addr + 8, 4));
                    r.v[3] = lane(r.v[3], 1, 32, m.read(addr + 12, 4));
                    r.v[2] = lane(r.v[2], 2, 32, m.read(addr + 16, 4));
                    r.v[3] = lane(r.v[3], 2, 32, m.read(addr + 20, 4));
                    r.v[2] = lane(r.v[2], 3, 32, m.read(addr + 24, 4));
                    r.v[3] = lane(r.v[3], 3, 32, m.read(addr + 28, 4));
                }
                r.x[15] = (r.x[14]);
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[4] = lane(r.v[4], 0, 32, m.read(addr + 0, 4));
                    r.v[5] = lane(r.v[5], 0, 32, m.read(addr + 4, 4));
                    r.v[4] = lane(r.v[4], 1, 32, m.read(addr + 8, 4));
                    r.v[5] = lane(r.v[5], 1, 32, m.read(addr + 12, 4));
                    r.v[4] = lane(r.v[4], 2, 32, m.read(addr + 16, 4));
                    r.v[5] = lane(r.v[5], 2, 32, m.read(addr + 20, 4));
                    r.v[4] = lane(r.v[4], 3, 32, m.read(addr + 24, 4));
                    r.v[5] = lane(r.v[5], 3, 32, m.read(addr + 28, 4));
                    r.x[15] = (addr.wrapping_add(32u64));
                }
                {
                    let addr = (r.x[15]).wrapping_add(0u64);
                    r.v[6] = lane(r.v[6], 0, 32, m.read(addr + 0, 4));
                    r.v[7] = lane(r.v[7], 0, 32, m.read(addr + 4, 4));
                    r.v[6] = lane(r.v[6], 1, 32, m.read(addr + 8, 4));
                    r.v[7] = lane(r.v[7], 1, 32, m.read(addr + 12, 4));
                    r.v[6] = lane(r.v[6], 2, 32, m.read(addr + 16, 4));
                    r.v[7] = lane(r.v[7], 2, 32, m.read(addr + 20, 4));
                    r.v[6] = lane(r.v[6], 3, 32, m.read(addr + 24, 4));
                    r.v[7] = lane(r.v[7], 3, 32, m.read(addr + 28, 4));
                }
                {
                    let addr = (r.x[13]).wrapping_add((-32i64 as u64));
                    m.write(addr.wrapping_add(0), 16, r.v[0]);
                    m.write(addr.wrapping_add(16), 16, r.v[2]);
                }
                {
                    let addr = (r.x[13]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 16, r.v[4]);
                    m.write(addr.wrapping_add(16), 16, r.v[6]);
                    r.x[13] = (addr.wrapping_add(64u64));
                }
                r.x[14] = ((r.x[14]).wrapping_add(128u64));
                r.compare(r.x[11], 16u64, 64, false);
                r.x[11] = ((r.x[11]).wrapping_sub(16u64));
                if !r.z {
                    pc = 4327124272;
                    continue;
                }
                pc = 4327124320;
            }
            4327124320 => {
                r.x[26] = (r.x[12]);
                pc = 4327124324;
            }
            4327124324 => {
                {
                    let addr = (r.x[26]).wrapping_add(0u64);
                    r.v[0] = m.read(addr.wrapping_add(0), 4);
                    r.x[26] = (addr.wrapping_add(8u64));
                }
                {
                    let addr = (r.x[9]).wrapping_add(0u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                    r.x[9] = (addr.wrapping_add(4u64));
                }
                r.compare(r.x[10] as u32 as u64, 1u64, 32, false);
                r.x[10] = ((r.x[10] as u32 as u64).wrapping_sub(1u64)) as u32 as u64;
                if !r.z {
                    pc = 4327124324;
                    continue;
                }
                pc = 4327124340;
            }
            4327124340 => {
                pc = 4327123664;
                continue;
            }
            4327124344 => {
                if (r.x[21] as u32 as u64 & (1u64 << 0)) == 0 {
                    pc = 4327124356;
                    continue;
                }
                pc = 4327124348;
            }
            4327124348 => {
                r.x[8] = (0u64) as u32 as u64;
                pc = 4327124364;
                continue;
            }
            4327124356 => {
                r.x[8] = (2u64) as u32 as u64;
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[27] as u32 as u64)) as u32 as u64;
                pc = 4327124364;
            }
            4327124364 => {
                {
                    let addr = (r.x[19]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                {
                    let addr = (r.x[31]).wrapping_add(80u64);
                    r.x[29] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[30] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(64u64);
                    r.x[20] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[19] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(48u64);
                    r.x[22] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[21] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(32u64);
                    r.x[24] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[23] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(16u64);
                    r.x[26] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[25] = (m.read(addr.wrapping_add(8), 8) as u64);
                }
                {
                    let addr = (r.x[31]).wrapping_add(0u64);
                    r.x[28] = (m.read(addr.wrapping_add(0), 8) as u64);
                    r.x[27] = (m.read(addr.wrapping_add(8), 8) as u64);
                    r.x[31] = (addr.wrapping_add(96u64));
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn update_status(m: &mut Memory, r: &mut Registers) {
    let mut pc = 4327117520u64;
    loop {
        match pc {
            4327117520 => {
                {
                    let addr = (r.x[0]).wrapping_add(180u64);
                    r.x[8] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[0]).wrapping_add(24u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                {
                    let addr = (r.x[0]).wrapping_add(52u64);
                    r.x[10] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.v[1] = ((r.x[10] as u32 as u64) as u32 as f64).to_bits() as u128;
                r.x[8] = ((r.x[10] as u32 as u64)
                    .wrapping_mul(r.x[9] as u32 as u64)
                    .wrapping_add(r.x[8] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[0]).wrapping_add(28u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 4);
                    r.v[0] = m.read(addr.wrapping_add(4), 4);
                }
                r.v[2] = ((fs(r.v[2], 0)) as f64).to_bits() as u128;
                r.v[1] = (fd(r.v[1], 0) * fd(r.v[2], 0)).to_bits() as u128;
                r.v[2] = ((fs(r.v[0], 0)) as f64).to_bits() as u128;
                r.v[3] = (fd(r.v[1], 0) / fd(r.v[2], 0)).to_bits() as u128;
                r.x[9] = ((fd(r.v[3], 0)) as u32 as u64) as u32 as u64;
                r.v[3] = ((r.x[9] as u32 as u64) as u32 as f64).to_bits() as u128;
                r.v[1] = ((-fd(r.v[3], 0)).mul_add(fd(r.v[2], 0), fd(r.v[1], 0))).to_bits() as u128;
                r.v[1] = ((fd(r.v[1], 0)) as f32).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    r.v[2] = m.read(addr.wrapping_add(0), 4);
                }
                r.v[1] = (fs(r.v[2], 0) + fs(r.v[1], 0)).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[1] & 0xffffffff);
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_add(r.x[9] as u32 as u64)) as u32 as u64;
                r.float_compare(fs(r.v[1], 0) as f64, fs(r.v[0], 0) as f64);
                if r.n != r.o {
                    pc = 4327117612;
                    continue;
                }
                pc = 4327117600;
            }
            4327117600 => {
                r.v[0] = (fs(r.v[1], 0) - fs(r.v[0], 0)).to_bits() as u128;
                {
                    let addr = (r.x[0]).wrapping_add(184u64);
                    m.write(addr.wrapping_add(0), 4, r.v[0] & 0xffffffff);
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_add(1u64)) as u32 as u64;
                pc = 4327117612;
            }
            4327117612 => {
                {
                    let addr = (r.x[0]).wrapping_add(48u64);
                    r.x[9] = (m.read(addr.wrapping_add(0), 4) as u64) as u32 as u64;
                }
                r.x[8] = ((r.x[8] as u32 as u64).wrapping_sub(r.x[9] as u32 as u64)) as u32 as u64;
                {
                    let addr = (r.x[0]).wrapping_add(180u64);
                    m.write(addr.wrapping_add(0), 4, (r.x[8] as u32 as u64) as u128);
                }
                return;
            }
            _ => panic!("invalid recovered control flow {}", pc),
        }
    }
}
pub(super) fn run(m: &mut Memory, r: &mut Registers, address: u64) -> bool {
    match address {
        0x101ea9770 => {
            convert(m, r);
            true
        }
        0x101ea9910 => {
            reset(m, r);
            true
        }
        0x101ea9cf0 => {
            generic(m, r);
            true
        }
        0x101eaa130 => {
            up(m, r);
            true
        }
        0x101eaa440 => {
            down(m, r);
            true
        }
        0x101eaa78c => {
            double(m, r);
            true
        }
        0x101eaaa40 => {
            half(m, r);
            true
        }
        0x101ea92d0 => {
            update_status(m, r);
            true
        }
        _ => false,
    }
}
pub(super) const CONSTANT_PAGES: &[u64] = &[0x108902000];
