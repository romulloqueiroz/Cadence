//! Recovered curvature interpolation shared by external modulation tables and
//! envelope segments.
//!
//! `BZoneLevelEnv::RenderValue` and `BModCurve::GetCurveValue` both use the same
//! bounded fast-exponential approximation and the same linear/curved segment
//! rules. `atkCurve` values are in -1..1 and are scaled by 16 like a native
//! curve point curvature. See `docs/kontakt-envelope-curves.md`.

// Native curve renderer's bounded exp approximation. No exponential runs in
// callbacks; this is preparation/interpolation-time arithmetic.
pub(crate) fn curve_exp(value: f32) -> f32 {
    let x = value * std::f32::consts::LOG2_E;
    let fraction = x - x.trunc();
    let bits = ((x + 127.0 + (fraction - fraction * fraction) * -0.339_71) * 8_388_608.0) as u32;
    f32::from_bits(bits)
}

/// Interpolate a rising 0..1 segment with a signed curvature already scaled by
/// the native factor of 16.
pub(crate) fn shaped_with_k(t: f32, k: f32) -> f32 {
    if k > 0.01 {
        (curve_exp(t * k) - 1.0) / (curve_exp(k) - 1.0)
    } else if k < -0.01 {
        1.0 - (curve_exp((1.0 - t) * -k) - 1.0) / (curve_exp(-k) - 1.0)
    } else {
        t
    }
}

/// Interpolate a rising segment with a stored -1..1 curvature.
pub(crate) fn shaped_fraction(t: f32, curvature: f32) -> f32 {
    shaped_with_k(t.clamp(0.0, 1.0), curvature * 16.0)
}
