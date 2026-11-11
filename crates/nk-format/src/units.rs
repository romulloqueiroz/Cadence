//! Recovered Kontakt effect unit conversions.
//!
//! These are direct ports of the `NatToUnity_*`, `UnityToNat_*` methods on the
//! `BEffect*` classes in the Kontakt 8.14.1 ARM64 image. "Natural" is the
//! serialized/engine value; "unity" is the normalized `0..1` (or `-1..1`)
//! control value used by the editor and automation. Keeping these formulas in
//! one place lets the FX parsers and any future ports share the exact laws.

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn clamp11(value: f32) -> f32 {
    value.clamp(-1.0, 1.0)
}

/// `BEffectCompressor::NatToUnity_Atk` (natural ms).
pub fn compressor_attack_to_unity(natural: f32) -> f32 {
    clamp01(((natural + 2.0).ln() - 1.098_612_3) / 5.811_141)
}

/// `BEffectCompressor::UnityToNat_Atk` (natural ms).
pub fn compressor_attack_from_unity(unity: f32) -> f32 {
    (clamp01(unity) * 5.811_141 + 1.098_612_3).exp() - 2.0
}

/// `BEffectCompressor::NatToUnity_Dcy` (natural ms).
pub fn compressor_release_to_unity(natural: f32) -> f32 {
    clamp01(((natural + 2.0).ln() - 3.951_243_6) / 3.873_602_2)
}

/// `BEffectCompressor::UnityToNat_Dcy` (natural ms).
pub fn compressor_release_from_unity(unity: f32) -> f32 {
    (clamp01(unity) * 3.873_602_2 + 3.951_243_6).exp() - 2.0
}

/// `BEffectCompressor::NatToUnity_Threshold` (natural `6*log2` dB domain).
pub fn compressor_threshold_to_unity(natural: f32) -> f32 {
    clamp01((natural + 68.0) / 65.0)
}

/// `BEffectCompressor::UnityToNat_Threshold`.
pub fn compressor_threshold_from_unity(unity: f32) -> f32 {
    clamp01(unity) * 65.0 - 68.0
}

/// `BEffectCompressor::NatToUnity_Ratio` (natural slope `1/R`).
pub fn compressor_ratio_to_unity(natural: f32) -> f32 {
    clamp01(1.0 - (natural.max(1e-9).ln() + 3.218_876) / 3.218_876)
}

/// `BEffectCompressor::UnityToNat_Ratio` (returns `1/R`, 1/25..1).
pub fn compressor_ratio_from_unity(unity: f32) -> f32 {
    (-clamp01(unity) * 3.218_876).exp()
}

/// `BEffectLimiter::NatToUnity_Gain` (natural dB).
pub fn limiter_gain_to_unity(natural: f32) -> f32 {
    clamp11((natural + 24.0) / 48.0 * 2.0 - 1.0)
}

/// `BEffectLimiter::UnityToNat_Gain` (returns dB).
pub fn limiter_gain_from_unity(unity: f32) -> f32 {
    (clamp11(unity) * 0.5 + 0.5) * 48.0 - 24.0
}

/// `BEffectLimiter::NatToUnity_Rls` (natural ms).
pub fn limiter_release_to_unity(natural: f32) -> f32 {
    clamp01(((natural + 2.0).ln() - 2.484_906_7) / 4.424_846_6)
}

/// `BEffectLimiter::UnityToNat_Rls` (returns ms).
pub fn limiter_release_from_unity(unity: f32) -> f32 {
    (clamp01(unity) * 4.424_846_6 + 2.484_906_7).exp() - 2.0
}

/// `BEffectDelay::NatToUnity_Time` (natural ms).
pub fn delay_time_to_unity(natural: f32) -> f32 {
    clamp01(((natural + 2.0).ln() - 1.945_910_1) / 6.027_245_5)
}

/// `BEffectDelay::UnityToNat_Time` (returns ms).
pub fn delay_time_from_unity(unity: f32) -> f32 {
    (clamp01(unity) * 6.027_245_5 + 1.945_910_1).exp() - 2.0
}

/// `BEffectReverb::NatToUnity_PD` (pre-delay, natural ms).
pub fn reverb_predelay_to_unity(natural: f32) -> f32 {
    clamp01(((natural + 2.0).ln() - std::f32::consts::LN_2) / 4.510_859_5)
}

/// `BEffectReverb::UnityToNat_PD` (returns ms).
pub fn reverb_predelay_from_unity(unity: f32) -> f32 {
    (clamp01(unity) * 4.510_859_5 + std::f32::consts::LN_2).exp() - 2.0
}

/// `BEffectCabinet::NatToUnity_TB` (`-1..1`).
pub fn cabinet_tone_to_unity(natural: f32) -> f32 {
    clamp11(natural)
}

/// `BEffectCabinet::UnityToNat_TB` (`-1..1`).
pub fn cabinet_tone_from_unity(unity: f32) -> f32 {
    clamp11(unity)
}

/// `BEffectFlanger::NatToUnity_Colour` (natural `0..5`).
pub fn flanger_colour_to_unity(natural: f32) -> f32 {
    clamp01(natural / 5.0)
}

/// `BEffectFlanger::UnityToNat_Colour` (returns `0..5`).
pub fn flanger_colour_from_unity(unity: f32) -> f32 {
    clamp01(unity) * 5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) {
        assert!((a - b).abs() <= tolerance, "{a} != {b}");
    }

    fn round_trip(nat_to_unity: fn(f32) -> f32, unity_to_nat: fn(f32) -> f32, value: f32) {
        let unity = nat_to_unity(value);
        close(unity_to_nat(unity), value, value.abs() * 1e-4 + 1e-3);
    }

    #[test]
    fn compressor_ranges_match_the_exported_curves() {
        close(compressor_attack_from_unity(0.0), 1.0, 1e-3);
        close(compressor_attack_from_unity(1.0), 1000.0, 0.5);
        close(compressor_release_from_unity(0.0), 50.0, 0.1);
        close(compressor_release_from_unity(1.0), 2500.0, 2.0);
        close(compressor_threshold_from_unity(0.0), -68.0, 1e-3);
        close(compressor_threshold_from_unity(1.0), -3.0, 1e-3);
        close(compressor_ratio_from_unity(0.0), 1.0, 1e-5);
        close(compressor_ratio_from_unity(1.0), 1.0 / 25.0, 1e-5);
        for value in [1.0, 4.0, 10.0, 50.0, 1000.0] {
            round_trip(
                compressor_attack_to_unity,
                compressor_attack_from_unity,
                value,
            );
        }
        for value in [50.0, 100.0, 500.0, 2500.0] {
            round_trip(
                compressor_release_to_unity,
                compressor_release_from_unity,
                value,
            );
        }
        for value in [-60.0, -30.0, -12.0] {
            round_trip(
                compressor_threshold_to_unity,
                compressor_threshold_from_unity,
                value,
            );
        }
        for value in [0.04, 0.1, 0.25, 1.0] {
            round_trip(
                compressor_ratio_to_unity,
                compressor_ratio_from_unity,
                value,
            );
        }
    }

    #[test]
    fn limiter_and_delay_ranges_match_the_exported_curves() {
        close(limiter_gain_from_unity(-1.0), -24.0, 1e-4);
        close(limiter_gain_from_unity(1.0), 24.0, 1e-4);
        close(limiter_release_from_unity(0.0), 10.0, 0.05);
        close(limiter_release_from_unity(1.0), 1000.0, 1.0);
        close(delay_time_from_unity(0.0), 5.0, 0.05);
        close(delay_time_from_unity(1.0), 2900.0, 10.0);
        close(reverb_predelay_from_unity(0.0), 0.0, 0.05);
        close(reverb_predelay_from_unity(1.0), 179.9, 1.0);
        for value in [-20.0, 0.0, 20.0] {
            round_trip(limiter_gain_to_unity, limiter_gain_from_unity, value);
        }
        for value in [10.0, 50.0, 500.0, 1000.0] {
            round_trip(limiter_release_to_unity, limiter_release_from_unity, value);
        }
        for value in [5.0, 50.0, 500.0, 2900.0] {
            round_trip(delay_time_to_unity, delay_time_from_unity, value);
        }
        for value in [0.0, 10.0, 100.0] {
            round_trip(reverb_predelay_to_unity, reverb_predelay_from_unity, value);
        }
    }

    #[test]
    fn cabinet_and_flanger_ranges_are_linear() {
        close(cabinet_tone_from_unity(-1.0), -1.0, 1e-6);
        close(cabinet_tone_from_unity(1.0), 1.0, 1e-6);
        close(flanger_colour_from_unity(0.5), 2.5, 1e-6);
        close(flanger_colour_to_unity(5.0), 1.0, 1e-6);
    }
}
