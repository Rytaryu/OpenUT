//! IEEE 754 binary16 conversion.

/// Converts binary16 bits to the equivalent binary32 value.
pub fn to_f32(bits: u16) -> f32 {
    let bits = u32::from(bits);
    let sign = (bits & 0x8000) << 16;
    let exponent = (bits >> 10) & 0x1f;
    let fraction = bits & 0x03ff;

    let magnitude = match exponent {
        // Binary16 subnormals are integral multiples of 2^-24. Each is exact in f32.
        0 => ((fraction as f32) * (1.0 / 16_777_216.0)).to_bits(),
        0x1f => 0x7f80_0000 | (fraction << 13),
        _ => ((exponent + (127 - 15)) << 23) | (fraction << 13),
    };
    f32::from_bits(sign | magnitude)
}

#[cfg(test)]
mod tests {
    use super::to_f32;

    #[test]
    fn converts_signed_zero_and_normal_values() {
        assert_eq!(to_f32(0x0000).to_bits(), 0.0_f32.to_bits());
        assert_eq!(to_f32(0x8000).to_bits(), (-0.0_f32).to_bits());
        assert_eq!(to_f32(0x3c00), 1.0);
        assert_eq!(to_f32(0xc000), -2.0);
        assert_eq!(to_f32(0x0400), 1.0 / 16_384.0);
        assert_eq!(to_f32(0x7bff), 65_504.0);
    }

    #[test]
    fn converts_subnormal_boundaries_and_sign() {
        assert_eq!(to_f32(0x0001), 1.0 / 16_777_216.0);
        assert_eq!(to_f32(0x03ff), 1_023.0 / 16_777_216.0);
        assert_eq!(to_f32(0x8001), -1.0 / 16_777_216.0);
    }

    #[test]
    fn converts_infinities_and_nan() {
        assert_eq!(to_f32(0x7c00), f32::INFINITY);
        assert_eq!(to_f32(0xfc00), f32::NEG_INFINITY);
        assert!(to_f32(0x7e01).is_nan());
        assert!(to_f32(0xfc01).is_nan());
    }
}
