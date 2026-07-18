pub(crate) fn parse_upstream_float_literal(text: &str) -> f32 {
    let mut value = 0.0_f32;
    let mut decimal = 1.0_f32;
    let mut fractional = false;
    let mut negative = false;

    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'-' if index == 0 => negative = true,
            b'.' => fractional = true,
            b'0'..=b'9' if fractional => {
                decimal /= 10.0;
                value = f32::from(byte - b'0').mul_add(decimal, value);
            }
            b'0'..=b'9' => {
                value = value.mul_add(10.0, f32::from(byte - b'0'));
            }
            _ => {}
        }
    }

    if negative { -value } else { value }
}

#[cfg(test)]
mod tests {
    use super::parse_upstream_float_literal;

    #[test]
    fn reproduces_native_stepwise_decimal_rounding() {
        assert_eq!(
            parse_upstream_float_literal("0.35").to_bits(),
            0.350_000_02_f32.to_bits()
        );
        assert_eq!(
            parse_upstream_float_literal("0.9").to_bits(),
            0.900_000_04_f32.to_bits()
        );
        assert_eq!(
            parse_upstream_float_literal("0.45").to_bits(),
            0.450_000_02_f32.to_bits()
        );
        assert_eq!(parse_upstream_float_literal("1.85"), 1.849_999_9);
        assert_eq!(parse_upstream_float_literal("3.141592"), 3.141_591_5);
    }
}
