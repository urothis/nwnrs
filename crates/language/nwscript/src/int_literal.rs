//! Integer literal parsing aligned with the native `NWScript` compiler.
//!
//! The upstream parser accumulates integer digits directly into an `int32_t`
//! without range checks. That means oversized decimal, hex, binary, and octal
//! literals wrap using two's-complement arithmetic instead of failing to parse.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntegerLiteralError;

pub(crate) fn parse_wrapping_decimal_i32(input: &str) -> Result<i32, IntegerLiteralError> {
    let mut chars = input.chars();
    let mut sign = 1i32;
    let mut value = 0i32;

    if matches!(chars.clone().next(), Some('-')) {
        sign = -1;
        chars.next();
    }

    let mut saw_digit = false;
    for ch in chars {
        let Some(digit) = ch.to_digit(10) else {
            return Err(IntegerLiteralError);
        };
        saw_digit = true;
        value = value.wrapping_mul(10).wrapping_add(digit as i32);
    }

    if !saw_digit {
        return Err(IntegerLiteralError);
    }

    if sign == -1 {
        value = value.wrapping_neg();
    }

    Ok(value)
}

pub(crate) fn parse_wrapping_prefixed_i32(
    input: &str,
    radix: u32,
) -> Result<i32, IntegerLiteralError> {
    let digits = input.get(2..).ok_or(IntegerLiteralError)?;
    if digits.is_empty() {
        return Err(IntegerLiteralError);
    }

    let mut value = 0i32;
    for ch in digits.chars() {
        let Some(digit) = ch.to_digit(radix) else {
            return Err(IntegerLiteralError);
        };
        value = value.wrapping_mul(radix as i32).wrapping_add(digit as i32);
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{parse_wrapping_decimal_i32, parse_wrapping_prefixed_i32};

    #[test]
    fn wraps_decimal_literals_like_upstream() {
        assert_eq!(parse_wrapping_decimal_i32("2147483647"), Ok(i32::MAX));
        assert_eq!(parse_wrapping_decimal_i32("2147483648"), Ok(i32::MIN));
        assert_eq!(parse_wrapping_decimal_i32("-2147483648"), Ok(i32::MIN));
    }

    #[test]
    fn wraps_prefixed_literals_like_upstream() {
        assert_eq!(parse_wrapping_prefixed_i32("0xffffffff", 16), Ok(-1));
        assert_eq!(parse_wrapping_prefixed_i32("0x80000000", 16), Ok(i32::MIN));
        assert_eq!(
            parse_wrapping_prefixed_i32("0b11111111111111111111111111111111", 2),
            Ok(-1)
        );
        assert_eq!(
            parse_wrapping_prefixed_i32("0o20000000000", 8),
            Ok(i32::MIN)
        );
    }
}
