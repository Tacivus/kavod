use std::{
    cmp, fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecimalError {
    #[error("arithmetic overflow")]
    Overflow,

    #[error("division by zero")]
    DivisionByZero,

    #[error("scale exceeds maximum ({})", Decimal::MAX_SCALE)]
    ScaleOverflow,

    #[error("NaN or infinity is not a valid decimal value")]
    NotFinite,

    #[error("Zero sign with non-zero value")]
    MismatchedSign,

    #[error("invalid decimal format")]
    InvalidFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

#[derive(Debug, Clone, Copy)]
pub struct Decimal {
    inner: u128,
    scale: u8,
    sign: Sign,
}

impl Decimal {
    pub const MAX_SCALE: u8 = 38;
    pub const ZERO: Self = Self {
        inner: 0,
        scale: 0,
        sign: Sign::Zero,
    };

    pub const fn as_raw(&self) -> u128 {
        self.inner
    }
    pub const fn scale(&self) -> u8 {
        self.scale
    }
    pub const fn sign(&self) -> Sign {
        self.sign
    }
    pub const fn is_zero(&self) -> bool {
        matches!(self.sign, Sign::Zero)
    }
    pub const fn is_positive(&self) -> bool {
        matches!(self.sign, Sign::Positive)
    }
    pub const fn is_negative(&self) -> bool {
        matches!(self.sign, Sign::Negative)
    }

    // Strips trailing zeros: (100, 2) → (1, 0), (500, 0) → (5, 2)
    /// Zero always returns (0, 0).
    const fn normalized_parts(&self) -> (u128, u8) {
        if self.inner == 0 || matches!(self.sign, Sign::Zero) {
            return (0, 0);
        }
        let mut inner = self.inner;
        let mut scale = self.scale;
        while scale > 0 && inner.is_multiple_of(10) {
            inner /= 10;
            scale -= 1;
        }
        (inner, scale)
    }

    /// Trims all zero and returns the most "normalized" verion possible
    pub fn normalized(self) -> Self {
        if self.inner == 0 || self.sign == Sign::Zero {
            return Self::ZERO;
        }
        let (inner, scale) = self.normalized_parts();
        Self {
            inner,
            scale,
            sign: self.sign,
        }
    }

    /// Construct from raw parts. Zero inner is unconditionally
    /// normalized to Sign::Zero.
    pub fn from_raw(inner: u128, scale: u8, sign: Sign) -> Result<Self, DecimalError> {
        if scale > Self::MAX_SCALE {
            return Err(DecimalError::ScaleOverflow);
        }

        if sign == Sign::Zero && inner != 0 {
            return Err(DecimalError::MismatchedSign);
        }

        if inner == 0 {
            return Ok(Self::ZERO);
        }

        Ok(Self { inner, scale, sign })
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        // Handle zero case
        if self.is_zero() {
            return Ok(other);
        }
        if other.is_zero() {
            return Ok(self);
        }

        // Need to "round up" becuase we cannot "divide" the inner of the larger
        let max_scale = self.scale.max(other.scale);

        let a = rescale_up(self.inner, self.scale, max_scale).ok_or(DecimalError::Overflow)?;
        let b = rescale_up(other.inner, other.scale, max_scale).ok_or(DecimalError::Overflow)?;

        match (self.sign, other.sign) {
            (Sign::Positive, Sign::Positive) => {
                let inner = a.checked_add(b).ok_or(DecimalError::Overflow)?;
                Self::from_raw(inner, max_scale, Sign::Positive)
            }
            (Sign::Negative, Sign::Negative) => {
                let inner = a.checked_add(b).ok_or(DecimalError::Overflow)?;
                Self::from_raw(inner, max_scale, Sign::Negative)
            }
            (Sign::Positive, Sign::Negative) | (Sign::Negative, Sign::Positive) => {
                let (larger, smaller, result_sign) = if a >= b {
                    (a, b, self.sign)
                } else {
                    (b, a, other.sign)
                };
                let inner = larger - smaller;
                Self::from_raw(inner, max_scale, result_sign)
            }
            _ => unreachable!(), // zero check above
        }
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        // Sub is just negated add
        let negated = match other.sign {
            Sign::Positive => Decimal {
                inner: other.inner,
                scale: other.scale,
                sign: Sign::Negative,
            },
            Sign::Negative => Decimal {
                inner: other.inner,
                scale: other.scale,
                sign: Sign::Positive,
            },
            Sign::Zero => other,
        };
        self.checked_add(negated)
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        // Handle zero case
        if self.is_zero() || other.is_zero() {
            return Ok(Self::ZERO);
        }

        let sign = match (self.sign, other.sign) {
            (Sign::Positive, Sign::Positive) | (Sign::Negative, Sign::Negative) => Sign::Positive,
            _ => Sign::Negative,
        };

        let inner = self
            .inner
            .checked_mul(other.inner)
            .ok_or(DecimalError::Overflow)?;
        let scale = self.scale + other.scale;

        if scale > Self::MAX_SCALE {
            // Strip trailing zeros first — the excess scale may just be zeros
            let (inner, new_scale) = strip_trailing_zeros(inner, scale);
            if new_scale > Self::MAX_SCALE {
                // Truncate least significant digits
                let drop = new_scale - Self::MAX_SCALE;
                let inner = inner / 10u128.pow(drop as u32);
                Self::from_raw(inner, Self::MAX_SCALE, sign)
            } else {
                Self::from_raw(inner, new_scale, sign)
            }
        } else {
            Self::from_raw(inner, scale, sign)
        }
    }

    pub fn ckecked_div(self, other: Self) -> Result<Self, DecimalError> {
        // Handle zero case
        if other.is_zero() {
            return Err(DecimalError::DivisionByZero);
        }
        if self.is_zero() {
            return Ok(Self::ZERO);
        }

        let sign = match (self.sign, other.sign) {
            (Sign::Positive, Sign::Positive) | (Sign::Negative, Sign::Negative) => Sign::Positive,
            _ => Sign::Negative,
        };

        let mut remainder = self.inner;
        let divisor = other.inner;

        let int_quot = remainder / divisor;
        remainder %= divisor;

        let mut result = int_quot;
        let mut result_scale: u8 = 0;

        // Compute fractional digits until remainder exhausted, scale limit,
        // or the accumulated value would overflow u128.
        while remainder > 0 && result_scale < Self::MAX_SCALE {
            remainder = remainder.checked_mul(10).ok_or(DecimalError::Overflow)?;
            let digit = remainder / divisor;

            result = result
                .checked_mul(10)
                .and_then(|r| r.checked_add(digit))
                .ok_or(DecimalError::Overflow)?;

            remainder %= divisor;
            result_scale += 1;
        }

        // Adjust decimal point by the scale difference between operands.
        if other.scale >= self.scale {
            let diff = other.scale - self.scale;
            result = result
                .checked_mul(10u128.pow(diff as u32))
                .ok_or(DecimalError::Overflow)?;
            Self::from_raw(result, result_scale, sign)
        } else {
            let diff = self.scale - other.scale;
            let new_scale = result_scale.saturating_add(diff);
            if new_scale > Self::MAX_SCALE {
                let drop = new_scale - Self::MAX_SCALE;
                result /= 10u128.pow(drop as u32);
                Self::from_raw(result, Self::MAX_SCALE, sign)
            } else {
                Self::from_raw(result, new_scale, sign)
            }
        }
    }
}

fn rescale_up(inner: u128, from_scale: u8, to_scale: u8) -> Option<u128> {
    let diff = (to_scale - from_scale) as usize;
    let mut value = inner;
    for _ in 0..diff {
        value = value.checked_mul(10)?;
    }
    Some(value)
}

/// Compare a / 10^a_scale  vs  b / 10^b_scale  (both assumed non-zero, same sign).
fn cmp_magnitude(a_inner: u128, a_scale: u8, b_inner: u128, b_scale: u8) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;

    // Strip trailing zeros — reduces rescaling work, makes overflow less likely.
    let (a, a_s) = strip_trailing_zeros(a_inner, a_scale);
    let (b, b_s) = strip_trailing_zeros(b_inner, b_scale);

    // Compare  a * 10^b_s  vs  b * 10^a_s
    match (rescale_up(a, 0, b_s), rescale_up(b, 0, a_s)) {
        (Some(av), Some(bv)) => av.cmp(&bv),
        (None, Some(_)) => Greater,
        (Some(_), None) => Less,
        (None, None) => {
            // Both overflowed.  Reduce exponents by min(b_s, a_s).
            // Now one exponent is 0 → its side is guaranteed to fit.
            let reduce = b_s.min(a_s);
            let av = rescale_up(a, 0, b_s - reduce);
            let bv = rescale_up(b, 0, a_s - reduce);
            match (av, bv) {
                (Some(av), Some(bv)) => av.cmp(&bv),
                (None, Some(_)) => Greater,
                (Some(_), None) => Less,
                (None, None) => unreachable!(),
            }
        }
    }
}

fn strip_trailing_zeros(mut inner: u128, mut scale: u8) -> (u128, u8) {
    while scale > 0 && inner.is_multiple_of(10) {
        inner /= 10;
        scale -= 1;
    }
    (inner, scale)
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        // Handle zeros
        if self.is_zero() {
            return if other.is_zero() {
                cmp::Ordering::Equal
            } else if other.sign == Sign::Negative {
                cmp::Ordering::Greater
            } else {
                cmp::Ordering::Less
            };
        }
        if other.is_zero() {
            return if self.sign == Sign::Negative {
                cmp::Ordering::Less
            } else {
                cmp::Ordering::Greater
            };
        }

        // Different signs
        match (self.sign, other.sign) {
            (Sign::Negative, Sign::Positive) => return cmp::Ordering::Less,
            (Sign::Positive, Sign::Negative) => return cmp::Ordering::Greater,
            _ => {}
        }

        let is_negative = self.sign == Sign::Negative;

        let ordering = cmp_magnitude(self.inner, self.scale, other.inner, other.scale);
        if is_negative {
            ordering.reverse()
        } else {
            ordering
        }
    }
}

impl Eq for Decimal {}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        // Both zero → equal
        if self.is_zero() && other.is_zero() {
            return true;
        }

        // Different signs → not equal
        if self.sign != other.sign {
            return false;
        }

        // Same sign, compare normalized parts
        self.normalized_parts() == other.normalized_parts()
    }
}

impl Hash for Decimal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.is_zero() {
            // All zeros hash identically
            0u128.hash(state);
            0u8.hash(state);
            Sign::Zero.hash(state);
        } else {
            let (inner, scale) = self.normalized_parts();
            inner.hash(state);
            scale.hash(state);
            self.sign.hash(state);
        }
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(DecimalError::InvalidFormat);
        }

        let (s, sign) = if let Some(rest) = s.strip_prefix('-') {
            (rest, Sign::Negative)
        } else {
            (s, Sign::Positive)
        };

        let mut inner: u128 = 0;
        let mut scale: u8 = 0;
        let mut seen_decimal = false;
        let mut seen_digit = false;
        let mut integer_digits = 0u8;

        for ch in s.chars() {
            match ch {
                '.' => {
                    if seen_decimal {
                        return Err(DecimalError::InvalidFormat);
                    }
                    seen_decimal = true;
                }
                '0'..='9' => {
                    seen_digit = true;
                    let digit = (ch as u8 - b'0') as u128;

                    if !seen_decimal {
                        // Reject leading zeros: second digit after initial 0
                        if integer_digits == 1 && inner == 0 {
                            return Err(DecimalError::InvalidFormat);
                        }
                        integer_digits += 1;
                    }

                    inner = inner
                        .checked_mul(10)
                        .and_then(|v| v.checked_add(digit))
                        .ok_or(DecimalError::Overflow)?;

                    if seen_decimal {
                        scale += 1;
                        if scale > Self::MAX_SCALE {
                            return Err(DecimalError::ScaleOverflow);
                        }
                    }
                }
                _ => return Err(DecimalError::InvalidFormat),
            }
        }

        // Reject: no digits at all
        if !seen_digit {
            return Err(DecimalError::InvalidFormat);
        }

        // Reject: trailing dot with no fractional digits (e.g. "1.")
        if seen_decimal && scale == 0 {
            return Err(DecimalError::InvalidFormat);
        }

        // Reject: negative zero (e.g. "-0", "-0.0")
        if inner == 0 && sign == Sign::Negative {
            return Err(DecimalError::InvalidFormat);
        }

        Self::from_raw(inner, scale, sign)
    }
}
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return write!(f, "0");
        }

        if self.sign == Sign::Negative {
            write!(f, "-")?;
        }

        let s = self.inner.to_string();
        let scale = self.scale as usize;

        if scale == 0 {
            return f.write_str(&s);
        }

        if s.len() <= scale {
            write!(f, "0.{:0>width$}", s, width = scale)
        } else {
            let split = s.len() - scale;
            write!(f, "{}.{}", &s[..split], &s[split..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hasher;

    fn assert_positive(d: Decimal, inner: u128, scale: u8, str: &str) {
        assert_eq!(d.as_raw(), inner);
        assert_eq!(d.scale(), scale);
        assert_eq!(d.sign(), Sign::Positive);
        assert_eq!(d.to_string(), str);
    }

    fn assert_negative(d: Decimal, inner: u128, scale: u8, str: &str) {
        assert_eq!(d.as_raw(), inner);
        assert_eq!(d.scale(), scale);
        assert_eq!(d.sign(), Sign::Negative);
        assert_eq!(d.to_string(), str);
    }

    fn assert_zero(d: Decimal) {
        assert_eq!(d, Decimal::ZERO);
        assert_eq!(d.to_string(), "0");
    }

    fn hash_of(d: &Decimal) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        d.hash(&mut h);
        h.finish()
    }

    // ------------------------------------------------------------------------
    // from_raw tests
    // ------------------------------------------------------------------------

    /// Invariant: from_raw preserves all fields and to_string yields correct decimal representation.
    #[test]
    fn from_raw_positive() {
        let d = Decimal::from_raw(12345, 2, Sign::Positive).unwrap();
        assert_positive(d, 12345, 2, "123.45");
    }

    /// Invariant: negative sign is preserved and displayed with leading minus.
    #[test]
    fn from_raw_negative() {
        let d = Decimal::from_raw(42, 1, Sign::Negative).unwrap();
        assert_negative(d, 42, 1, "-4.2");
    }

    /// Invariant: inner=0 always normalizes to ZERO regardless of sign and scale.
    #[test]
    fn from_raw_zero_inner_normalizes_to_zero() {
        let d = Decimal::from_raw(0, 5, Sign::Positive).unwrap();
        assert_zero(d);
    }

    /// Invariant: inner!=0 && sign==Sign::Zero fails
    #[test]
    fn from_non_zero_inner_zero_sign() {
        let d = Decimal::from_raw(1, 5, Sign::Zero);
        assert_eq!(d, Err(DecimalError::MismatchedSign));
    }

    /// Invariant: scale > MAX_SCALE fails; scale == MAX_SCALE succeeds.
    #[test]
    fn scale_too_big() {
        let d = Decimal::from_raw(1, Decimal::MAX_SCALE + 1, Sign::Positive);
        assert_eq!(d, Err(DecimalError::ScaleOverflow));

        let d = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        assert_positive(
            d,
            1,
            Decimal::MAX_SCALE,
            &format!("0.{}1", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    // ------------------------------------------------------------------------
    // from_str tests
    // ------------------------------------------------------------------------

    /// Invariant: positive integer parses correctly.
    #[test]
    fn from_str_positive_integer() {
        let d = Decimal::from_str("123").unwrap();
        assert_positive(d, 123, 0, "123");
    }

    /// Invariant: negative integer parses with leading minus.
    #[test]
    fn from_str_negative_integer() {
        let d = Decimal::from_str("-456").unwrap();
        assert_negative(d, 456, 0, "-456");
    }

    /// Invariant: decimal with fractional part parses correctly.
    #[test]
    fn from_str_positive_decimal() {
        let d = Decimal::from_str("123.45").unwrap();
        assert_positive(d, 12345, 2, "123.45");
    }

    /// Invariant: negative decimal with fractional part parses correctly.
    #[test]
    fn from_str_negative_decimal() {
        let d = Decimal::from_str("-4.2").unwrap();
        assert_negative(d, 42, 1, "-4.2");
    }

    /// Invariant: "0" yields ZERO.
    #[test]
    fn from_str_zero() {
        let d = Decimal::from_str("0").unwrap();
        assert_zero(d);
    }

    /// Invariant: "-0" returns InvalidFormat.
    #[test]
    fn from_str_negative_zero_rejected() {
        let err = Decimal::from_str("-0");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: "-0.0" returns InvalidFormat.
    #[test]
    fn from_str_negative_zero_decimal_rejected() {
        let err = Decimal::from_str("-0.0");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: leading zeros in integer part return InvalidFormat.
    #[test]
    fn from_str_leading_zeros_rejected() {
        let err = Decimal::from_str("007");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: double zero returns InvalidFormat.
    #[test]
    fn from_str_double_zero_rejected() {
        let err = Decimal::from_str("00");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: trailing decimal point without fractional digits returns InvalidFormat.
    #[test]
    fn from_str_trailing_dot_rejected() {
        let err = Decimal::from_str("1.");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: leading/trailing whitespace returns InvalidFormat.
    #[test]
    fn from_str_whitespace_rejected() {
        let err = Decimal::from_str("  42.5  ");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: leading dot without integer part parses correctly.
    #[test]
    fn from_str_leading_dot() {
        let d = Decimal::from_str(".5").unwrap();
        assert_positive(d, 5, 1, "0.5");
    }

    /// Invariant: leading zero dot without integer part parses correctly.
    #[test]
    fn from_str_leading_zero_dot() {
        let d = Decimal::from_str("0.5").unwrap();
        assert_positive(d, 5, 1, "0.5");
    }

    /// Invariant: negative leading dot parses correctly.
    #[test]
    fn from_str_negative_leading_dot() {
        let d = Decimal::from_str("-.5").unwrap();
        assert_negative(d, 5, 1, "-0.5");
    }

    /// Invariant: negative fractional without integer part parses correctly.
    #[test]
    fn from_str_negative_zero_fraction() {
        let d = Decimal::from_str("-0.5").unwrap();
        assert_negative(d, 5, 1, "-0.5");
    }

    /// Invariant: trailing fractional zeros are preserved as-is.
    #[test]
    fn from_str_trailing_fractional_zeros() {
        let d = Decimal::from_str("1.00").unwrap();
        assert_positive(d, 100, 2, "1.00");
    }

    /// Invariant: empty string returns InvalidFormat.
    #[test]
    fn from_str_empty() {
        let err = Decimal::from_str("");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: only a decimal point with no digits returns InvalidFormat.
    #[test]
    fn from_str_only_dot() {
        let err = Decimal::from_str(".");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: multiple decimal points returns InvalidFormat.
    #[test]
    fn from_str_double_dot() {
        let err = Decimal::from_str("1.2.3");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: non-numeric characters return InvalidFormat.
    #[test]
    fn from_str_letters() {
        let err = Decimal::from_str("abc");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: only a minus sign with no digits returns InvalidFormat.
    #[test]
    fn from_str_only_minus() {
        let err = Decimal::from_str("-");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: value exceeding u128::MAX returns Overflow.
    #[test]
    fn from_str_overflow() {
        let err = Decimal::from_str("340282366920938463463374607431768211456");
        assert_eq!(err, Err(DecimalError::Overflow));
    }

    /// Invariant: more than MAX_SCALE fractional digits returns ScaleOverflow.
    #[test]
    fn from_str_scale_overflow() {
        let zeros = "0".repeat(Decimal::MAX_SCALE as usize + 1);
        let input = format!("0.{}1", zeros);
        let err = Decimal::from_str(&input);
        assert_eq!(err, Err(DecimalError::ScaleOverflow));
    }

    /// Invariant: parsing u128::MAX exactly succeeds with inner=u128::MAX, scale=0.
    #[test]
    fn from_str_u128_max() {
        let s = u128::MAX.to_string(); // "340282366920938463463374607431768211455"
        let d = Decimal::from_str(&s).unwrap();
        assert_positive(d, u128::MAX, 0, &s);
    }

    /// Invariant: parsing (u128::MAX + 1) returns Overflow.
    #[test]
    fn from_str_past_u128_max() {
        // u128::MAX + 1 = 340282366920938463463374607431768211456
        let err = Decimal::from_str("340282366920938463463374607431768211456");
        assert_eq!(err, Err(DecimalError::Overflow));
    }

    /// Invariant: positive sign prefix '+' returns InvalidFormat.
    #[test]
    fn from_str_positive_sign_rejected() {
        let err = Decimal::from_str("+123");
        assert_eq!(err, Err(DecimalError::InvalidFormat));
    }

    /// Invariant: a value that parses then displays then re-parses equals itself
    /// (roundtrip identity).
    #[test]
    fn from_str_display_roundtrip() {
        for s in [
            "0",
            "1",
            "-1",
            "123.45",
            "-123.45",
            "0.5",
            ".5",
            "-.5",
            "0.001",
            "100.00",
            "0.00000000000000000000000000000000000001",
        ] {
            let d = Decimal::from_str(s).unwrap();
            assert_eq!(
                Decimal::from_str(&d.to_string()).unwrap(),
                d,
                "roundtrip failed for {s}"
            );
        }
    }

    // ------------------------------------------------------------------------
    // normalized tests
    // ------------------------------------------------------------------------

    /// Invariant: normalizing zero returns zero
    #[test]
    fn normalized_parts_zero() {
        assert_eq!(Decimal::ZERO.normalized_parts(), (0, 0));
    }

    /// Invariant: normalizing (100, 2) strips trailing zeros to (1, 0).
    #[test]
    fn normalized_trailing_fractional_zeros() {
        let d = Decimal::from_raw(100, 2, Sign::Positive).unwrap();
        let n = d.normalized();
        assert_positive(n, 1, 0, "1");
    }

    /// Invariant: normalizing (500, 0) integer has no effect.
    #[test]
    fn normalized_integer_no_effect() {
        let d = Decimal::from_raw(500, 0, Sign::Positive).unwrap();
        let n = d.normalized();
        assert_positive(n, 500, 0, "500");
    }

    /// Invariant: normalizing zero returns ZERO.
    #[test]
    fn normalized_zero() {
        assert_zero(Decimal::ZERO.normalized());
    }

    /// Invariant: normalizing negative value strips trailing zeros but preserves sign.
    #[test]
    fn normalized_negative() {
        let d = Decimal::from_raw(1200, 2, Sign::Negative).unwrap(); // -12.00
        let n = d.normalized();
        assert_negative(n, 12, 0, "-12");
    }

    /// Invariant: normalizing an already-normalized value has no effect (idempotent).
    #[test]
    fn normalized_idempotent() {
        let d = Decimal::from_raw(15, 1, Sign::Positive).unwrap(); // 1.5
        let n = d.normalized();
        assert_eq!(n.as_raw(), 15);
        assert_eq!(n.scale(), 1);
        assert_positive(n, 15, 1, "1.5");
    }

    // ------------------------------------------------------------------------
    // rescale_up tests
    // ------------------------------------------------------------------------

    /// Invariant: rescaling from lower to higher scale multiplies by powers of 10.
    #[test]
    fn rescale_up_basic() {
        assert_eq!(rescale_up(5, 0, 2), Some(500));
        assert_eq!(rescale_up(123, 2, 7), Some(123_00000));
    }

    /// Invariant: rescaling to the same scale returns the inner unchanged.
    #[test]
    fn rescale_up_same_scale() {
        assert_eq!(rescale_up(42, 3, 3), Some(42));
    }

    /// Invariant: rescaling from higher to lower scale is not supported (this
    /// function only scales up; the caller is responsible for direction).
    #[test]
    fn rescale_up_overflow() {
        // Multiplying u128::MAX by 10 overflows
        assert_eq!(rescale_up(u128::MAX, 0, 1), None);
    }

    // ------------------------------------------------------------------------
    // PartialEq tests
    // ------------------------------------------------------------------------

    /// Invariant: identical representations are equal.
    #[test]
    fn partial_eq_same_value_same_repr() {
        let a = Decimal::from_raw(10, 1, Sign::Positive).unwrap();
        let b = Decimal::from_raw(10, 1, Sign::Positive).unwrap();
        assert_eq!(a, b);
        assert_positive(a, 10, 1, "1.0");
        assert_positive(b, 10, 1, "1.0");
    }

    /// Invariant: trailing zeros in fractional part do not affect equality.
    #[test]
    fn partial_eq_same_value_trailing_zeros() {
        let a = Decimal::from_raw(100, 2, Sign::Positive).unwrap(); // 1.00
        let b = Decimal::from_raw(10, 1, Sign::Positive).unwrap(); // 1.0
        assert_eq!(a, b);
        assert_positive(a, 100, 2, "1.00");
        assert_positive(b, 10, 1, "1.0");
    }

    /// Invariant: integer vs decimal representation of the same value are equal.
    #[test]
    fn partial_eq_integer_equals_decimal() {
        let a = Decimal::from_raw(1, 0, Sign::Positive).unwrap(); // 1
        let b = Decimal::from_raw(10, 1, Sign::Positive).unwrap(); // 1.0
        let c = Decimal::from_raw(100, 2, Sign::Positive).unwrap(); // 1.00
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, c);
        assert_positive(a, 1, 0, "1");
        assert_positive(b, 10, 1, "1.0");
        assert_positive(c, 100, 2, "1.00");
    }

    /// Invariant: different signs are not equal.
    #[test]
    fn partial_eq_different_signs_not_equal() {
        let pos = Decimal::from_raw(10, 1, Sign::Positive).unwrap(); // 1.0
        let neg = Decimal::from_raw(10, 1, Sign::Negative).unwrap(); // -1.0
        assert_ne!(pos, neg);
        assert_positive(pos, 10, 1, "1.0");
        assert_negative(neg, 10, 1, "-1.0");
    }

    /// Invariant: different numerical values are not equal.
    #[test]
    fn partial_eq_different_values_not_equal() {
        let a = Decimal::from_raw(10, 1, Sign::Positive).unwrap(); // 1.0
        let b = Decimal::from_raw(20, 1, Sign::Positive).unwrap(); // 2.0
        assert_ne!(a, b);
        assert_positive(a, 10, 1, "1.0");
        assert_positive(b, 20, 1, "2.0");
    }

    /// Invariant: different values that look similar under scaling are not equal.
    #[test]
    fn partial_eq_different_values_different_scale_not_equal() {
        let a = Decimal::from_raw(15, 1, Sign::Positive).unwrap(); // 1.5
        let b = Decimal::from_raw(105, 2, Sign::Positive).unwrap(); // 1.05
        assert_ne!(a, b);
        assert_positive(a, 15, 1, "1.5");
        assert_positive(b, 105, 2, "1.05");
    }

    /// Invariant: all zero forms are equal.
    #[test]
    fn partial_eq_all_zeros_equal() {
        let a = Decimal::ZERO;
        let b = Decimal::from_raw(0, 5, Sign::Positive).unwrap();
        let c = Decimal::from_str("0").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, c);
        assert_zero(a);
        assert_zero(b);
        assert_zero(c);
    }

    /// Invariant: a value equals its own normalized form.
    #[test]
    fn partial_eq_normalized_roundtrip() {
        let a = Decimal::from_raw(100, 2, Sign::Positive).unwrap(); // 1.00
        let b = a.normalized(); // 1
        assert_eq!(a, b);
        assert_positive(a, 100, 2, "1.00");
        assert_positive(b, 1, 0, "1");
    }

    /// Invariant: negative values with trailing zeros compare equal.
    #[test]
    fn partial_eq_negative_trailing_zeros() {
        let a = Decimal::from_raw(100, 2, Sign::Negative).unwrap(); // -1.00
        let b = Decimal::from_raw(10, 1, Sign::Negative).unwrap(); // -1.0
        assert_eq!(a, b);
        assert_negative(a, 100, 2, "-1.00");
        assert_negative(b, 10, 1, "-1.0");
    }

    /// Invariant: zero is not equal to any non-zero value.
    #[test]
    fn partial_eq_zero_not_equal_to_nonzero() {
        let zero = Decimal::ZERO;
        let one = Decimal::from_raw(1, 0, Sign::Positive).unwrap();
        let neg_one = Decimal::from_raw(1, 0, Sign::Negative).unwrap();
        assert_ne!(zero, one);
        assert_ne!(zero, neg_one);
        assert_zero(zero);
        assert_positive(one, 1, 0, "1");
        assert_negative(neg_one, 1, 0, "-1");
    }

    // ------------------------------------------------------------------------
    // Hash tests
    // ------------------------------------------------------------------------

    /// Invariant: identical representations produce the same hash.
    #[test]
    fn hash_same_representation() {
        let a = Decimal::from_raw(10, 1, Sign::Positive).unwrap();
        let b = Decimal::from_raw(10, 1, Sign::Positive).unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_positive(a, 10, 1, "1.0");
        assert_positive(b, 10, 1, "1.0");
    }

    /// Invariant: semantically equal values with different representations
    /// produce the same hash (PartialEq/Hash contract).
    #[test]
    fn hash_equal_values_same_hash() {
        let a = Decimal::from_raw(100, 2, Sign::Positive).unwrap(); // 1.00
        let b = Decimal::from_raw(10, 1, Sign::Positive).unwrap(); // 1.0
        let c = Decimal::from_raw(1, 0, Sign::Positive).unwrap(); // 1
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_eq!(hash_of(&b), hash_of(&c));
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_positive(a, 100, 2, "1.00");
        assert_positive(b, 10, 1, "1.0");
        assert_positive(c, 1, 0, "1");
    }

    /// Invariant: all zero forms produce the same hash.
    #[test]
    fn hash_all_zeros_same_hash() {
        let a = Decimal::ZERO;
        let b = Decimal::from_raw(0, 5, Sign::Positive).unwrap();
        let c = Decimal::from_str("0").unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_eq!(hash_of(&b), hash_of(&c));
        assert_zero(a);
        assert_zero(b);
        assert_zero(c);
    }

    /// Invariant: a value and its normalized form produce the same hash.
    #[test]
    fn hash_normalized_roundtrip() {
        let a = Decimal::from_raw(100, 2, Sign::Positive).unwrap(); // 1.00
        let b = a.normalized(); // 1
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_eq!(a, b);
        assert_positive(a, 100, 2, "1.00");
        assert_positive(b, 1, 0, "1");
    }

    /// Invariant: negative values with trailing zeros hash the same.
    #[test]
    fn hash_negative_trailing_zeros() {
        let a = Decimal::from_raw(100, 2, Sign::Negative).unwrap(); // -1.00
        let b = Decimal::from_raw(10, 1, Sign::Negative).unwrap(); // -1.0
        let c = Decimal::from_raw(1, 0, Sign::Negative).unwrap(); // -1
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_eq!(hash_of(&b), hash_of(&c));
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_negative(a, 100, 2, "-1.00");
        assert_negative(b, 10, 1, "-1.0");
        assert_negative(c, 1, 0, "-1");
    }

    /// Invariant: equal values generate the same hash, unequal values
    /// generating the same hash is a collision (unlikely but not a correctness
    /// violation).
    #[test]
    fn hash_collision_smoke_test() {
        // These are not expected to collide; if they do, the test makes it visible.
        let a = Decimal::from_str("123.45").unwrap();
        let b = Decimal::from_str("-123.45").unwrap();
        let c = Decimal::from_str("0").unwrap();
        let d = Decimal::from_str("1").unwrap();

        // Different sign
        assert_ne!(hash_of(&a), hash_of(&b));
        // Zero vs non-zero
        assert_ne!(hash_of(&c), hash_of(&d));
        assert_positive(a, 12345, 2, "123.45");
        assert_negative(b, 12345, 2, "-123.45");
        assert_zero(c);
        assert_positive(d, 1, 0, "1");
    }

    // ------------------------------------------------------------------------
    // Ord / PartialOrd tests
    // ------------------------------------------------------------------------

    /// Invariant: zero and zero are equal in ordering.
    #[test]
    fn ord_zero_equals_zero() {
        let a = Decimal::ZERO;
        let b = Decimal::from_str("0").unwrap();
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Equal));
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_zero(a);
        assert_zero(b);
    }

    /// Invariant: zero is less than any positive value.
    #[test]
    fn ord_zero_less_than_positive() {
        let zero = Decimal::ZERO;
        let pos = Decimal::from_str("1.0").unwrap();
        assert!(zero < pos);
        assert!(pos > zero);
        assert_zero(zero);
        assert_positive(pos, 10, 1, "1.0");
    }

    /// Invariant: any negative value is less than zero.
    #[test]
    fn ord_negative_less_than_zero() {
        let neg = Decimal::from_str("-1.0").unwrap();
        let zero = Decimal::ZERO;
        assert!(neg < zero);
        assert!(zero > neg);
        assert_negative(neg, 10, 1, "-1.0");
        assert_zero(zero);
    }

    /// Invariant: negative is less than positive.
    #[test]
    fn ord_negative_less_than_positive() {
        let neg = Decimal::from_str("-1.0").unwrap();
        let pos = Decimal::from_str("1.0").unwrap();
        assert!(neg < pos);
        assert!(pos > neg);
        assert_negative(neg, 10, 1, "-1.0");
        assert_positive(pos, 10, 1, "1.0");
    }

    /// Invariant: same scale positives are ordered by their inner value.
    #[test]
    fn ord_same_scale_positives() {
        let a = Decimal::from_str("1.0").unwrap();
        let b = Decimal::from_str("2.0").unwrap();
        assert!(a < b);
        assert!(b > a);
        assert_positive(a, 10, 1, "1.0");
        assert_positive(b, 20, 1, "2.0");
    }

    /// Invariant: same scale negatives are ordered with magnitudes reversed.
    #[test]
    fn ord_same_scale_negatives() {
        let a = Decimal::from_str("-2.0").unwrap();
        let b = Decimal::from_str("-1.0").unwrap();
        assert!(a < b);
        assert!(b > a);
        assert_negative(a, 20, 1, "-2.0");
        assert_negative(b, 10, 1, "-1.0");
    }

    /// Invariant: different scale but same numerical value are equal in ordering.
    #[test]
    fn ord_trailing_zeros_equal() {
        let a = Decimal::from_str("1.00").unwrap();
        let b = Decimal::from_str("1.0").unwrap();
        let c = Decimal::from_str("1").unwrap();
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
        assert_eq!(b.cmp(&c), std::cmp::Ordering::Equal);
        assert_eq!(a.cmp(&c), std::cmp::Ordering::Equal);
        assert_positive(a, 100, 2, "1.00");
        assert_positive(b, 10, 1, "1.0");
        assert_positive(c, 1, 0, "1");
    }

    /// Invariant: positive values with different scales compare correctly
    /// after rescaling.
    #[test]
    fn ord_different_scale_positives() {
        // 1.1 vs 1.09 → rescale both to scale 2: 110 vs 109 → 1.1 > 1.09
        let a = Decimal::from_str("1.1").unwrap();
        let b = Decimal::from_str("1.09").unwrap();
        assert!(a > b);
        assert!(b < a);
        assert_positive(a, 11, 1, "1.1");
        assert_positive(b, 109, 2, "1.09");
    }

    /// Invariant: negative values with different scales compare correctly
    /// after rescaling (ordering reverses).
    #[test]
    fn ord_different_scale_negatives() {
        // -1.1 vs -1.09 → same magnitude as above but sign flips:
        // 1.1 > 1.09  →  -1.1 < -1.09
        let a = Decimal::from_str("-1.1").unwrap();
        let b = Decimal::from_str("-1.09").unwrap();
        assert!(a < b);
        assert!(b > a);
        assert_negative(a, 11, 1, "-1.1");
        assert_negative(b, 109, 2, "-1.09");
    }

    /// Invariant: integer comparison works with different scales.
    #[test]
    fn ord_integer_vs_fraction() {
        // 2 > 1.999
        let a = Decimal::from_str("2").unwrap();
        let b = Decimal::from_str("1.999").unwrap();
        assert!(a > b);
        assert_positive(a, 2, 0, "2");
        assert_positive(b, 1999, 3, "1.999");
    }

    /// Invariant: partially-ordered comparison returns Some.
    #[test]
    fn ord_partial_cmp_always_returns_some() {
        let a = Decimal::from_str("-3.14").unwrap();
        let b = Decimal::from_str("2.718").unwrap();
        // No floats, no NaN → total order; partial_cmp should always return Some.
        assert!(a.partial_cmp(&b).is_some());
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Less));
        assert_negative(a, 314, 2, "-3.14");
        assert_positive(b, 2718, 3, "2.718");
    }

    /// Invariant: sort works correctly on a list of mixed decimals.
    #[test]
    fn ord_sort_integrity() {
        let mut vals = [
            Decimal::from_str("0").unwrap(),
            Decimal::from_str("2.5").unwrap(),
            Decimal::from_str("-1.0").unwrap(),
            Decimal::from_str("0.1").unwrap(),
            Decimal::from_str("-2.5").unwrap(),
            Decimal::from_str("1.00").unwrap(),
        ];
        vals.sort();
        assert_eq!(
            vals.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
            vec!["-2.5", "-1.0", "0", "0.1", "1.00", "2.5"]
        );
    }

    /// Invariant: when cross-multiplying and a overflows but b fits,
    /// a has larger magnitude (overflow means > u128::MAX >= b).
    #[test]
    fn ord_overflow_a_not_b() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, 38, Sign::Positive).unwrap();
        assert!(a > b);
        assert!(b < a);
    }

    /// Invariant: when cross-multiplying and b overflows but a fits,
    /// b has larger magnitude.
    #[test]
    fn ord_overflow_b_not_a() {
        let a = Decimal::from_raw(1, 38, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: negative single-overflow respects sign reversal.
    #[test]
    fn ord_overflow_negative() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Negative).unwrap();
        let b = Decimal::from_raw(1, 38, Sign::Negative).unwrap();
        // a is a huge negative, b is a tiny negative → a < b
        assert!(a < b);
        assert!(b > a);
    }

    // ------------------------------------------------------------------------
    // Ord — both-overflow paths (both rescale_up overflow → reduce and retry)
    // ------------------------------------------------------------------------

    /// Invariant: both cross-products overflow; after reducing exponents
    /// by the minimum scale, both fit and compare as equal.
    #[test]
    fn ord_both_overflow_reduce_both_fit_equal() {
        // a = u128::MAX / 10^5,  b = u128::MAX / 10^5  (equal)
        let a = Decimal::from_raw(u128::MAX, 5, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX, 5, Sign::Positive).unwrap();
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    /// Invariant: both cross-products overflow; after reduction both fit
    /// and a > b.
    #[test]
    fn ord_both_overflow_reduce_both_fit_gt() {
        // a = u128::MAX / 10^1,  b = (u128::MAX - 1) / 10^1  → a > b
        let a = Decimal::from_raw(u128::MAX, 1, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX - 1, 1, Sign::Positive).unwrap();
        assert!(a > b);
        assert!(b < a);
    }

    /// Invariant: negative version of both-overflow-reduce-both-fit reverses
    /// the comparison.
    #[test]
    fn ord_both_overflow_reduce_both_fit_neg() {
        // -a < -b  because  -(u128::MAX/10) < -((u128::MAX-1)/10)
        let a = Decimal::from_raw(u128::MAX, 1, Sign::Negative).unwrap();
        let b = Decimal::from_raw(u128::MAX - 1, 1, Sign::Negative).unwrap();
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: both overflow; after reduction a fits, b still overflows
    /// → b has larger magnitude → a < b.
    #[test]
    fn ord_both_overflow_reduce_one_still_overflow() {
        // a = u128::MAX / 10^20,  b = u128::MAX / 10^5  → a < b
        // After reduction by 5: b still overflows at 10^15
        let a = Decimal::from_raw(u128::MAX, 20, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX, 5, Sign::Positive).unwrap();
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: negative version of reduce-and-still-overflow reverses
    /// the comparison.
    #[test]
    fn ord_both_overflow_reduce_still_overflow_neg() {
        // -a > -b  because  -u128::MAX/10^20 > -u128::MAX/10^5
        let a = Decimal::from_raw(u128::MAX, 20, Sign::Negative).unwrap();
        let b = Decimal::from_raw(u128::MAX, 5, Sign::Negative).unwrap();
        assert!(a > b);
        assert!(b < a);
    }

    /// Invariant: trailing zeros are normalized away before comparison,
    /// shrinking values so the cross-multiplication fits without overflow.
    #[test]
    fn ord_trailing_zeros_normalized_before_compare() {
        // a = 10^35 / 10^5 → normalized to 10^30 / 10^0 = 10^30
        // b = 10^34 / 10^0 → already normalized                            = 10^34
        let a = Decimal::from_raw(10u128.pow(35), 5, Sign::Positive).unwrap();
        let b = Decimal::from_raw(10u128.pow(34), 0, Sign::Positive).unwrap();
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: sorting a list that includes overflow-triggering values
    /// produces the correct ascending order.
    #[test]
    fn ord_sort_integrity_extended() {
        let mut vals = [
            Decimal::from_raw(u128::MAX, 0, Sign::Negative).unwrap(), // huge negative
            Decimal::from_raw(10u128.pow(31), 5, Sign::Negative).unwrap(), // -10^26
            Decimal::from_raw(1, 0, Sign::Negative).unwrap(),         // -1
            Decimal::ZERO,
            Decimal::from_raw(1, 38, Sign::Positive).unwrap(), // tiny positive
            Decimal::from_raw(1, 0, Sign::Positive).unwrap(),  // 1
            Decimal::from_raw(10u128.pow(31), 5, Sign::Positive).unwrap(), // 10^26
            Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap(), // huge positive
        ];
        vals.sort();
        // Verify each consecutive pair is non-decreasing
        for i in 1..vals.len() {
            assert!(
                vals[i - 1] <= vals[i],
                "mismatch at index {i}: {} > {}",
                vals[i - 1],
                vals[i]
            );
        }
    }

    // ------------------------------------------------------------------------
    // add tests
    // ------------------------------------------------------------------------

    /// Invariant: adding zero to zero yields zero.
    #[test]
    fn add_zero_plus_zero() {
        let r = Decimal::ZERO.checked_add(Decimal::ZERO).unwrap();
        assert_zero(r);
    }

    /// Invariant: adding zero to a positive value returns that value unchanged.
    #[test]
    fn add_zero_plus_positive() {
        let a = Decimal::from_str("5.5").unwrap();
        assert_eq!(Decimal::ZERO.checked_add(a).unwrap(), a);
    }

    /// Invariant: adding zero to a negative value returns that value unchanged.
    #[test]
    fn add_zero_plus_negative() {
        let a = Decimal::from_str("-3").unwrap();
        assert_eq!(Decimal::ZERO.checked_add(a).unwrap(), a);
    }

    /// Invariant: a positive value plus zero returns itself.
    #[test]
    fn add_positive_plus_zero() {
        let a = Decimal::from_str("7").unwrap();
        assert_eq!(a.checked_add(Decimal::ZERO).unwrap(), a);
    }

    /// Invariant: a negative value plus zero returns itself.
    #[test]
    fn add_negative_plus_zero() {
        let a = Decimal::from_str("-9").unwrap();
        assert_eq!(a.checked_add(Decimal::ZERO).unwrap(), a);
    }

    /// Invariant: two positive values at the same scale sum correctly.
    #[test]
    fn add_positive_same_scale() {
        let a = Decimal::from_str("1.5").unwrap();
        let b = Decimal::from_str("2.5").unwrap();
        assert_eq!(a.checked_add(b).unwrap(), Decimal::from_str("4.0").unwrap());
    }

    /// Invariant: two negative values at the same scale sum correctly,
    /// preserving negative sign.
    #[test]
    fn add_negative_same_scale() {
        let a = Decimal::from_str("-1.5").unwrap();
        let b = Decimal::from_str("-2.5").unwrap();
        assert_eq!(
            a.checked_add(b).unwrap(),
            Decimal::from_str("-4.0").unwrap()
        );
    }

    /// Invariant: positive values with different scales sum correctly
    /// after rescaling both operands to the maximum scale.
    #[test]
    fn add_positive_diff_scale_integers_and_fractions() {
        assert_eq!(
            Decimal::from_str("1.0")
                .unwrap()
                .checked_add(Decimal::from_str("0.5").unwrap())
                .unwrap(),
            Decimal::from_str("1.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("1.1")
                .unwrap()
                .checked_add(Decimal::from_str("1.09").unwrap())
                .unwrap(),
            Decimal::from_str("2.19").unwrap()
        );
        assert_eq!(
            Decimal::from_str("100")
                .unwrap()
                .checked_add(Decimal::from_str("0.001").unwrap())
                .unwrap(),
            Decimal::from_str("100.001").unwrap()
        );
    }

    /// Invariant: negative values with different scales sum correctly.
    #[test]
    fn add_negative_diff_scale() {
        assert_eq!(
            Decimal::from_str("-1.0")
                .unwrap()
                .checked_add(Decimal::from_str("-0.5").unwrap())
                .unwrap(),
            Decimal::from_str("-1.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-1.1")
                .unwrap()
                .checked_add(Decimal::from_str("-1.09").unwrap())
                .unwrap(),
            Decimal::from_str("-2.19").unwrap()
        );
    }

    /// Invariant: when signs differ, the smaller magnitude is subtracted
    /// from the larger and the result takes the sign of the larger magnitude.
    #[test]
    fn add_opposite_signs_same_scale() {
        assert_eq!(
            Decimal::from_str("3.5")
                .unwrap()
                .checked_add(Decimal::from_str("-2.5").unwrap())
                .unwrap(),
            Decimal::from_str("1.0").unwrap()
        );
        assert_eq!(
            Decimal::from_str("2.5")
                .unwrap()
                .checked_add(Decimal::from_str("-3.5").unwrap())
                .unwrap(),
            Decimal::from_str("-1.0").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-3.5")
                .unwrap()
                .checked_add(Decimal::from_str("2.5").unwrap())
                .unwrap(),
            Decimal::from_str("-1.0").unwrap()
        );
    }

    /// Invariant: opposite signs with different scales work after rescaling.
    #[test]
    fn add_opposite_signs_diff_scale() {
        assert_eq!(
            Decimal::from_str("1.1")
                .unwrap()
                .checked_add(Decimal::from_str("-1.09").unwrap())
                .unwrap(),
            Decimal::from_str("0.01").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-1.1")
                .unwrap()
                .checked_add(Decimal::from_str("1.09").unwrap())
                .unwrap(),
            Decimal::from_str("-0.01").unwrap()
        );
    }

    /// Invariant: x + (-x) yields zero for any non-zero x, regardless of scale.
    #[test]
    fn add_annihilates_to_zero() {
        assert_eq!(
            Decimal::from_str("5")
                .unwrap()
                .checked_add(Decimal::from_str("-5").unwrap())
                .unwrap(),
            Decimal::ZERO
        );
        assert_eq!(
            Decimal::from_str("1.00")
                .unwrap()
                .checked_add(Decimal::from_str("-1.0").unwrap())
                .unwrap(),
            Decimal::ZERO
        );
        // Very large match
        assert_eq!(
            Decimal::from_str("1000000.123456789")
                .unwrap()
                .checked_add(Decimal::from_str("-1000000.123456789").unwrap())
                .unwrap(),
            Decimal::ZERO
        );
    }

    /// Invariant: catastrophic cancellation — subtracting two nearly equal
    /// values preserves the few remaining significant digits.
    #[test]
    fn add_catastrophic_cancellation() {
        let a = Decimal::from_raw(1000000_123456789, 9, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1000000_123456788, 9, Sign::Negative).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_positive(r, 1, 9, "0.000000001");
    }

    /// Invariant: rescaling a u128::MAX value to match a higher-scale operand
    /// overflows because it requires multiplying by a power of ten.
    #[test]
    fn add_rescale_up_overflow() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, 1, Sign::Positive).unwrap();
        assert_eq!(a.checked_add(b), Err(DecimalError::Overflow));
    }

    /// Invariant: rescale overflow also triggers when the tiny operand has
    /// MAX_SCALE and the large operand must be scaled up 38 places.
    #[test]
    fn add_rescale_up_overflow_max_scale() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        assert_eq!(a.checked_add(b), Err(DecimalError::Overflow));
    }

    /// Invariant: rescaling the large operand to match the tiny one also
    /// overflows regardless of argument order.
    #[test]
    fn add_rescale_up_overflow_reverse_args() {
        let a = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        assert_eq!(a.checked_add(b), Err(DecimalError::Overflow));
    }

    /// Invariant: adding two values whose sum exceeds u128::MAX returns Overflow.
    #[test]
    fn add_sum_overflow() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, 0, Sign::Positive).unwrap();
        assert_eq!(a.checked_add(b), Err(DecimalError::Overflow));
    }

    /// Invariant: when two large same-scale values sum to exactly u128::MAX,
    /// the addition succeeds.
    #[test]
    fn add_sum_at_u128_max() {
        let a = Decimal::from_raw(u128::MAX - 1, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, 0, Sign::Positive).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_positive(r, u128::MAX, 0, &u128::MAX.to_string());
    }

    /// Invariant: negative version of sum-at-boundary also succeeds.
    #[test]
    fn add_sum_at_u128_max_negative() {
        let a = Decimal::from_raw(u128::MAX - 1, 0, Sign::Negative).unwrap();
        let b = Decimal::from_raw(1, 0, Sign::Negative).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_negative(r, u128::MAX, 0, &format!("-{}", u128::MAX));
    }

    /// Invariant: addition is commutative.
    #[test]
    fn add_commutative() {
        let a = Decimal::from_str("123.45").unwrap();
        let b = Decimal::from_str("-67.89").unwrap();
        assert_eq!(a.checked_add(b).unwrap(), b.checked_add(a).unwrap());
    }

    /// Invariant: zero is the additive identity for non-zero values.
    #[test]
    fn add_zero_identity() {
        for s in ["0", "1", "-1", "999.999", "-0.001"] {
            let v = Decimal::from_str(s).unwrap();
            assert_eq!(v.checked_add(Decimal::ZERO).unwrap(), v);
            assert_eq!(Decimal::ZERO.checked_add(v).unwrap(), v);
        }
    }

    /// Invariant: every value has an additive inverse that sums to zero.
    #[test]
    fn add_inverse() {
        for s in ["0", "42", "0.001", "9999999", "-17.5"] {
            let v = Decimal::from_str(s).unwrap();
            let neg = match v.sign() {
                Sign::Positive => Decimal::from_raw(v.as_raw(), v.scale(), Sign::Negative).unwrap(),
                Sign::Negative => Decimal::from_raw(v.as_raw(), v.scale(), Sign::Positive).unwrap(),
                Sign::Zero => Decimal::ZERO,
            };
            assert_eq!(
                v.checked_add(neg).unwrap(),
                Decimal::ZERO,
                "inverse failed for {}",
                v
            );
        }
    }

    /// Invariant: adding two negative values whose sum exceeds u128::MAX
    /// returns Overflow (mirrors the positive case).
    #[test]
    fn add_negative_sum_overflow() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Negative).unwrap();
        let b = Decimal::from_raw(1, 0, Sign::Negative).unwrap();
        assert_eq!(a.checked_add(b), Err(DecimalError::Overflow));
    }

    /// Invariant: two positive values at MAX_SCALE sum correctly without
    /// rescaling (both already at the same scale).
    #[test]
    fn add_both_max_scale_positive() {
        let a = Decimal::from_raw(3, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_positive(
            r,
            5,
            Decimal::MAX_SCALE,
            &format!("0.{}5", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: two negative values at MAX_SCALE sum correctly.
    #[test]
    fn add_both_max_scale_negative() {
        let a = Decimal::from_raw(3, Decimal::MAX_SCALE, Sign::Negative).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Negative).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_negative(
            r,
            5,
            Decimal::MAX_SCALE,
            &format!("-0.{}5", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: opposite signs at MAX_SCALE where the smaller magnitude
    /// is subtracted from the larger.
    #[test]
    fn add_opposite_signs_max_scale() {
        let a = Decimal::from_raw(5, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Negative).unwrap();
        let r = a.checked_add(b).unwrap();
        assert_positive(
            r,
            3,
            Decimal::MAX_SCALE,
            &format!("0.{}3", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    // ------------------------------------------------------------------------
    // sub tests
    // ------------------------------------------------------------------------

    /// Invariant: zero minus zero is zero.
    #[test]
    fn sub_zero_minus_zero() {
        assert_zero(Decimal::ZERO.checked_sub(Decimal::ZERO).unwrap());
    }

    /// Invariant: zero minus a positive yields the negative of that value.
    #[test]
    fn sub_zero_minus_positive() {
        let b = Decimal::from_str("5.5").unwrap();
        let r = Decimal::ZERO.checked_sub(b).unwrap();
        assert_negative(r, 55, 1, "-5.5");
    }

    /// Invariant: zero minus a negative yields the positive of that value.
    #[test]
    fn sub_zero_minus_negative() {
        let b = Decimal::from_str("-3").unwrap();
        let r = Decimal::ZERO.checked_sub(b).unwrap();
        assert_positive(r, 3, 0, "3");
    }

    /// Invariant: x minus zero returns x unchanged.
    #[test]
    fn sub_x_minus_zero() {
        assert_eq!(
            Decimal::from_str("7")
                .unwrap()
                .checked_sub(Decimal::ZERO)
                .unwrap(),
            Decimal::from_str("7").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-9")
                .unwrap()
                .checked_sub(Decimal::ZERO)
                .unwrap(),
            Decimal::from_str("-9").unwrap()
        );
    }

    /// Invariant: any value minus itself yields zero, regardless of representation.
    #[test]
    fn sub_self_minus_self() {
        assert_zero(
            Decimal::from_str("42.5")
                .unwrap()
                .checked_sub(Decimal::from_str("42.5").unwrap())
                .unwrap(),
        );
        assert_zero(
            Decimal::from_str("-42.5")
                .unwrap()
                .checked_sub(Decimal::from_str("-42.5").unwrap())
                .unwrap(),
        );
        assert_zero(Decimal::ZERO.checked_sub(Decimal::ZERO).unwrap());
    }

    /// Invariant: subtracting a smaller positive from a larger yields a positive result.
    #[test]
    fn sub_positive_larger_minus_smaller() {
        assert_eq!(
            Decimal::from_str("3")
                .unwrap()
                .checked_sub(Decimal::from_str("1").unwrap())
                .unwrap(),
            Decimal::from_str("2").unwrap()
        );
    }

    /// Invariant: subtracting a larger positive from a smaller yields a negative result.
    #[test]
    fn sub_positive_smaller_minus_larger() {
        assert_eq!(
            Decimal::from_str("1")
                .unwrap()
                .checked_sub(Decimal::from_str("3").unwrap())
                .unwrap(),
            Decimal::from_str("-2").unwrap()
        );
    }

    /// Invariant: subtracting a smaller negative from a larger negative yields
    /// a negative result (magnitude is the difference).
    #[test]
    fn sub_negative_arithmetic() {
        assert_eq!(
            Decimal::from_str("-3")
                .unwrap()
                .checked_sub(Decimal::from_str("-1").unwrap())
                .unwrap(),
            Decimal::from_str("-2").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-1")
                .unwrap()
                .checked_sub(Decimal::from_str("-3").unwrap())
                .unwrap(),
            Decimal::from_str("2").unwrap()
        );
    }

    /// Invariant: subtracting a negative from a positive is addition.
    #[test]
    fn sub_positive_minus_negative() {
        assert_eq!(
            Decimal::from_str("3")
                .unwrap()
                .checked_sub(Decimal::from_str("-1").unwrap())
                .unwrap(),
            Decimal::from_str("4").unwrap()
        );
    }

    /// Invariant: subtracting a positive from a negative yields a more negative result.
    #[test]
    fn sub_negative_minus_positive() {
        assert_eq!(
            Decimal::from_str("-3")
                .unwrap()
                .checked_sub(Decimal::from_str("1").unwrap())
                .unwrap(),
            Decimal::from_str("-4").unwrap()
        );
    }

    /// Invariant: subtraction with different scales works after rescaling.
    #[test]
    fn sub_diff_scales() {
        assert_eq!(
            Decimal::from_str("1.1")
                .unwrap()
                .checked_sub(Decimal::from_str("1.09").unwrap())
                .unwrap(),
            Decimal::from_str("0.01").unwrap()
        );
        assert_eq!(
            Decimal::from_str("1.09")
                .unwrap()
                .checked_sub(Decimal::from_str("1.1").unwrap())
                .unwrap(),
            Decimal::from_str("-0.01").unwrap()
        );
    }

    /// Invariant: sub overflows when rescaling the negated operand overflows.
    #[test]
    fn sub_rescale_overflow() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Negative).unwrap();
        assert_eq!(a.checked_sub(b), Err(DecimalError::Overflow));
    }

    /// Invariant: sub(a, b) = -(sub(b, a)) — magnitudes match, signs opposite.
    #[test]
    fn sub_antisymmetric() {
        let a = Decimal::from_str("7.3").unwrap();
        let b = Decimal::from_str("2.1").unwrap();
        let r1 = a.checked_sub(b).unwrap();
        let r2 = b.checked_sub(a).unwrap();
        assert_eq!(r1.normalized_parts(), r2.normalized_parts());
        assert_ne!(r1.sign(), r2.sign());
    }

    /// Invariant: add and sub are inverses — (a + b) - b == a.
    #[test]
    fn sub_add_roundtrip() {
        let a = Decimal::from_str("100.5").unwrap();
        let b = Decimal::from_str("23.25").unwrap();
        let sum = a.checked_add(b).unwrap();
        assert_eq!(sum.checked_sub(b).unwrap(), a);
        assert_eq!(sum.checked_sub(a).unwrap(), b);
    }

    /// Invariant: subtracting values at MAX_SCALE works without rescaling
    /// (delegates to add which sees same scale).
    #[test]
    fn sub_both_max_scale() {
        let a = Decimal::from_raw(5, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let r = a.checked_sub(b).unwrap();
        assert_positive(
            r,
            3,
            Decimal::MAX_SCALE,
            &format!("0.{}3", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: subtracting a negative MAX_SCALE value from a MAX_SCALE
    /// positive (i.e. adding them) correctly sums without rescaling.
    #[test]
    fn sub_opposite_signs_max_scale() {
        let a = Decimal::from_raw(3, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Negative).unwrap();
        let r = a.checked_sub(b).unwrap();
        assert_positive(
            r,
            5,
            Decimal::MAX_SCALE,
            &format!("0.{}5", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    // ------------------------------------------------------------------------
    // mul tests
    // ------------------------------------------------------------------------

    /// Invariant: zero times anything is zero.
    #[test]
    fn mul_zero() {
        let x = Decimal::from_str("5").unwrap();
        assert_zero(Decimal::ZERO.checked_mul(x).unwrap());
        assert_zero(x.checked_mul(Decimal::ZERO).unwrap());
        assert_zero(Decimal::ZERO.checked_mul(Decimal::ZERO).unwrap());
        let neg = Decimal::from_str("-5").unwrap();
        assert_zero(Decimal::ZERO.checked_mul(neg).unwrap());
    }

    /// Invariant: multiplying by one preserves the value.
    #[test]
    fn mul_identity() {
        let one = Decimal::from_str("1").unwrap();
        assert_eq!(
            Decimal::from_str("42.5").unwrap().checked_mul(one).unwrap(),
            Decimal::from_str("42.5").unwrap()
        );
        assert_eq!(
            one.checked_mul(Decimal::from_str("42.5").unwrap()).unwrap(),
            Decimal::from_str("42.5").unwrap()
        );
        assert_eq!(one.checked_mul(one).unwrap(), one);
    }

    /// Invariant: multiplying by -1 negates the value.
    #[test]
    fn mul_by_neg_one() {
        let neg_one = Decimal::from_str("-1").unwrap();
        assert_eq!(
            Decimal::from_str("42.5")
                .unwrap()
                .checked_mul(neg_one)
                .unwrap(),
            Decimal::from_str("-42.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-42.5")
                .unwrap()
                .checked_mul(neg_one)
                .unwrap(),
            Decimal::from_str("42.5").unwrap()
        );
        assert_eq!(
            neg_one.checked_mul(neg_one).unwrap(),
            Decimal::from_str("1").unwrap()
        );
    }

    /// Invariant: multiplying integers yields the correct integer product.
    #[test]
    fn mul_integers() {
        assert_eq!(
            Decimal::from_str("2")
                .unwrap()
                .checked_mul(Decimal::from_str("3").unwrap())
                .unwrap(),
            Decimal::from_str("6").unwrap()
        );
    }

    /// Invariant: multiplying fractions adds their scales.
    #[test]
    fn mul_fractions() {
        assert_eq!(
            Decimal::from_str("0.5")
                .unwrap()
                .checked_mul(Decimal::from_str("0.5").unwrap())
                .unwrap(),
            Decimal::from_str("0.25").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-0.5")
                .unwrap()
                .checked_mul(Decimal::from_str("0.5").unwrap())
                .unwrap(),
            Decimal::from_str("-0.25").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-0.5")
                .unwrap()
                .checked_mul(Decimal::from_str("-0.5").unwrap())
                .unwrap(),
            Decimal::from_str("0.25").unwrap()
        );
    }

    /// Invariant: scales accumulate exactly for values whose combined scale
    /// does not exceed MAX_SCALE.
    #[test]
    fn mul_scale_accumulation_fits() {
        assert_eq!(
            Decimal::from_str("0.1")
                .unwrap()
                .checked_mul(Decimal::from_str("0.1").unwrap())
                .unwrap(),
            Decimal::from_str("0.01").unwrap()
        );
        // 0.0001 * 0.0001 = 0.00000001 (inner=1, scale=8)
        let r = Decimal::from_str("0.0001")
            .unwrap()
            .checked_mul(Decimal::from_str("0.0001").unwrap())
            .unwrap();
        assert_positive(r, 1, 8, "0.00000001");
    }

    /// Invariant: when the combined scale exceeds MAX_SCALE and the product
    /// has trailing zeros, they are stripped to bring scale within bounds.
    #[test]
    fn mul_scale_overflow_saved_by_trailing_zeros() {
        // 1.00 * 1.00 → inner=10000, scale=4 → strip → inner=1, scale=0
        assert_eq!(
            Decimal::from_str("1.00")
                .unwrap()
                .checked_mul(Decimal::from_str("1.00").unwrap())
                .unwrap(),
            Decimal::from_str("1").unwrap()
        );
    }

    /// Invariant: when trailing zeros are exhausted and scale still exceeds
    /// MAX_SCALE, the least significant digits are truncated.
    #[test]
    fn mul_scale_overflow_truncation_nonzero() {
        // 31/10^10 * 11/10^30 = 341/10^40 → strip fails (ends with 1)
        // drop 2 → 3/10^38
        let a = Decimal::from_raw(31, 10, Sign::Positive).unwrap();
        let b = Decimal::from_raw(11, 30, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_positive(
            r,
            3,
            Decimal::MAX_SCALE,
            &format!("0.{}3", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: when the product is so small that truncation yields zero,
    /// the result is ZERO.
    #[test]
    fn mul_scale_overflow_truncation_to_zero() {
        // 13/10^20 * 7/10^20 = 91/10^40 → no trailing zeros → drop 2 → 0
        let a = Decimal::from_raw(13, 20, Sign::Positive).unwrap();
        let b = Decimal::from_raw(7, 20, Sign::Positive).unwrap();
        assert_zero(a.checked_mul(b).unwrap());
    }

    /// Invariant: inner product overflow returns Overflow.
    #[test]
    fn mul_inner_overflow() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, 0, Sign::Positive).unwrap();
        assert_eq!(a.checked_mul(b), Err(DecimalError::Overflow));
    }

    /// Invariant: when the product fits just under u128::MAX, it succeeds.
    #[test]
    fn mul_inner_borderline() {
        let a = Decimal::from_raw(u128::MAX / 2, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, 0, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_eq!(r.as_raw(), (u128::MAX / 2) * 2);
    }

    /// Invariant: combined scale at exactly MAX_SCALE succeeds.
    #[test]
    fn mul_scale_at_max() {
        let a = Decimal::from_raw(2, 19, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 19, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_positive(
            r,
            6,
            Decimal::MAX_SCALE,
            &format!("0.{}6", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: multiplication is commutative.
    #[test]
    fn mul_commutative() {
        let a = Decimal::from_str("123.45").unwrap();
        let b = Decimal::from_str("-67.89").unwrap();
        assert_eq!(a.checked_mul(b).unwrap(), b.checked_mul(a).unwrap());
    }

    /// Invariant: when scale overflows and truncation occurs on a negative
    /// product, the sign remains Negative.
    #[test]
    fn mul_negative_scale_overflow_truncation() {
        let a = Decimal::from_raw(31, 10, Sign::Negative).unwrap();
        let b = Decimal::from_raw(11, 30, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_negative(
            r,
            3,
            Decimal::MAX_SCALE,
            &format!("-0.{}3", "0".repeat(Decimal::MAX_SCALE as usize - 1)),
        );
    }

    /// Invariant: when a negative-times-negative product has scale overflow
    /// and is truncated to zero, the result is ZERO (not Negative zero).
    #[test]
    fn mul_negative_negative_scale_overflow_truncation_to_zero() {
        let a = Decimal::from_raw(13, 20, Sign::Negative).unwrap();
        let b = Decimal::from_raw(7, 20, Sign::Negative).unwrap();
        assert_zero(a.checked_mul(b).unwrap());
    }

    /// Invariant: multiplication accumulates scales correctly (not
    /// relying on PartialEq normalization for verification).
    #[test]
    fn mul_scale_exact() {
        // 0.002 (inner=2, scale=3) * 0.03 (inner=3, scale=2) = 0.00006 (inner=6, scale=5)
        let a = Decimal::from_raw(2, 3, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 2, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_positive(r, 6, 5, "0.00006");
    }

    /// Invariant: when the combined scale exceeds MAX_SCALE but the product
    /// inner has enough trailing zeros to bring it back within bounds without
    /// truncation, the result preserves the correct inner and reduced scale.
    #[test]
    fn mul_strip_trailing_zeros_saves_from_truncation() {
        // 10^19/10^36 * 10^19/10^36 = 10^38/10^72 → strip 38 zeros → 1/10^34
        let a = Decimal::from_raw(10u128.pow(19), 36, Sign::Positive).unwrap();
        let b = Decimal::from_raw(10u128.pow(19), 36, Sign::Positive).unwrap();
        let r = a.checked_mul(b).unwrap();
        assert_positive(r, 1, 34, "0.0000000000000000000000000000000001");
    }

    /// Invariant: when both operands are at MAX_SCALE with non-zero inner
    /// values and the inner product fits but the combined scale overflows
    /// sufficiently that truncation still leaves a non-zero value, the
    /// result has the correct truncated inner and MAX_SCALE.
    #[test]
    fn mul_both_max_scale_truncation() {
        // 3/10^38 * 4/10^38 = 12/10^76 → strip (no trailing zeros) → drop 38
        // 12 / 10^38 = 0, so result is ZERO.
        let a = Decimal::from_raw(3, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(4, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        assert_zero(a.checked_mul(b).unwrap());

        // 5/10^38 * (u128::MAX/3) = huge/10^38 → inner product overflow → Overflow
        let c = Decimal::from_raw(5, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let d = Decimal::from_raw(u128::MAX / 3, 0, Sign::Positive).unwrap();
        assert!(c.checked_mul(d).is_err());
    }

    // ------------------------------------------------------------------------
    // div tests
    // ------------------------------------------------------------------------

    /// Invariant: division by zero returns DivisionByZero regardless of sign.
    #[test]
    fn div_by_zero() {
        assert_eq!(
            Decimal::from_str("5").unwrap().ckecked_div(Decimal::ZERO),
            Err(DecimalError::DivisionByZero)
        );
        assert_eq!(
            Decimal::from_str("-5").unwrap().ckecked_div(Decimal::ZERO),
            Err(DecimalError::DivisionByZero)
        );
        assert_eq!(
            Decimal::ZERO.ckecked_div(Decimal::ZERO),
            Err(DecimalError::DivisionByZero)
        );
    }

    /// Invariant: zero divided by any non-zero value is zero.
    #[test]
    fn div_zero_dividend() {
        assert_zero(
            Decimal::ZERO
                .ckecked_div(Decimal::from_str("5").unwrap())
                .unwrap(),
        );
        assert_zero(
            Decimal::ZERO
                .ckecked_div(Decimal::from_str("-5").unwrap())
                .unwrap(),
        );
    }

    /// Invariant: exact integer division yields the correct integer quotient.
    #[test]
    fn div_exact_integers() {
        assert_eq!(
            Decimal::from_str("6")
                .unwrap()
                .ckecked_div(Decimal::from_str("3").unwrap())
                .unwrap(),
            Decimal::from_str("2").unwrap()
        );
        assert_eq!(
            Decimal::from_str("10")
                .unwrap()
                .ckecked_div(Decimal::from_str("4").unwrap())
                .unwrap(),
            Decimal::from_str("2.5").unwrap()
        );
    }

    /// Invariant: dividing by fractions that produce exact decimal results
    /// (½, ¼, ⅛) yields the correct inner and scale.
    #[test]
    fn div_exact_fractions() {
        assert_eq!(
            Decimal::from_str("1")
                .unwrap()
                .ckecked_div(Decimal::from_str("2").unwrap())
                .unwrap(),
            Decimal::from_str("0.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("1")
                .unwrap()
                .ckecked_div(Decimal::from_str("4").unwrap())
                .unwrap(),
            Decimal::from_str("0.25").unwrap()
        );
        let r = Decimal::from_str("1")
            .unwrap()
            .ckecked_div(Decimal::from_str("8").unwrap())
            .unwrap();
        assert_positive(r, 125, 3, "0.125");
    }

    /// Invariant: non-terminating division (1/3) fills all MAX_SCALE digits
    /// with the repeating digit 3.
    #[test]
    fn div_non_terminating_one_third() {
        let r = Decimal::from_str("1")
            .unwrap()
            .ckecked_div(Decimal::from_str("3").unwrap())
            .unwrap();
        assert_eq!(r.scale(), Decimal::MAX_SCALE);
        assert_eq!(r.sign(), Sign::Positive);
        let expected: u128 = (0..Decimal::MAX_SCALE).fold(0u128, |acc, _| acc * 10 + 3);
        assert_eq!(r.as_raw(), expected);
    }

    /// Invariant: another non-terminating division (1/7) fills the scale.
    #[test]
    fn div_non_terminating_one_seventh() {
        let r = Decimal::from_str("1")
            .unwrap()
            .ckecked_div(Decimal::from_str("7").unwrap())
            .unwrap();
        assert_eq!(r.scale(), Decimal::MAX_SCALE);
        assert_eq!(r.sign(), Sign::Positive);
    }

    /// Invariant: any non-zero value divided by itself equals 1.
    #[test]
    fn div_self_by_self() {
        for s in ["42.5", "-42.5", "1.00", "0.001"] {
            let v = Decimal::from_str(s).unwrap();
            assert_eq!(
                v.ckecked_div(v).unwrap(),
                Decimal::from_str("1").unwrap(),
                "failed for {s}"
            );
        }
    }

    /// Invariant: dividing by 1 preserves the value.
    #[test]
    fn div_by_one() {
        let one = Decimal::from_str("1").unwrap();
        assert_eq!(
            Decimal::from_str("42.5").unwrap().ckecked_div(one).unwrap(),
            Decimal::from_str("42.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-42.5")
                .unwrap()
                .ckecked_div(one)
                .unwrap(),
            Decimal::from_str("-42.5").unwrap()
        );
    }

    /// Invariant: dividing by -1 negates the value.
    #[test]
    fn div_by_neg_one() {
        let neg_one = Decimal::from_str("-1").unwrap();
        assert_eq!(
            Decimal::from_str("42.5")
                .unwrap()
                .ckecked_div(neg_one)
                .unwrap(),
            Decimal::from_str("-42.5").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-42.5")
                .unwrap()
                .ckecked_div(neg_one)
                .unwrap(),
            Decimal::from_str("42.5").unwrap()
        );
    }

    /// Invariant: negative division follows standard sign rules.
    #[test]
    fn div_sign_rules() {
        assert_eq!(
            Decimal::from_str("-6")
                .unwrap()
                .ckecked_div(Decimal::from_str("3").unwrap())
                .unwrap(),
            Decimal::from_str("-2").unwrap()
        );
        assert_eq!(
            Decimal::from_str("6")
                .unwrap()
                .ckecked_div(Decimal::from_str("-3").unwrap())
                .unwrap(),
            Decimal::from_str("-2").unwrap()
        );
        assert_eq!(
            Decimal::from_str("-6")
                .unwrap()
                .ckecked_div(Decimal::from_str("-3").unwrap())
                .unwrap(),
            Decimal::from_str("2").unwrap()
        );
    }

    /// Invariant: when the divisor has a larger scale than the dividend,
    /// the quotient is shifted right (multiplied by 10^diff).
    #[test]
    fn div_scale_divisor_larger() {
        assert_eq!(
            Decimal::from_str("0.2")
                .unwrap()
                .ckecked_div(Decimal::from_str("0.01").unwrap())
                .unwrap(),
            Decimal::from_str("20").unwrap()
        );
        assert_eq!(
            Decimal::from_str("2")
                .unwrap()
                .ckecked_div(Decimal::from_str("0.001").unwrap())
                .unwrap(),
            Decimal::from_str("2000").unwrap()
        );
    }

    /// Invariant: when the dividend has a larger scale than the divisor,
    /// the quotient is shifted left (scale increased).
    #[test]
    fn div_scale_dividend_larger() {
        assert_eq!(
            Decimal::from_str("0.01")
                .unwrap()
                .ckecked_div(Decimal::from_str("0.2").unwrap())
                .unwrap(),
            Decimal::from_str("0.05").unwrap()
        );
        assert_eq!(
            Decimal::from_str("0.001")
                .unwrap()
                .ckecked_div(Decimal::from_str("1").unwrap())
                .unwrap(),
            Decimal::from_str("0.001").unwrap()
        );
        assert_eq!(
            Decimal::from_str("0.01")
                .unwrap()
                .ckecked_div(Decimal::from_str("2").unwrap())
                .unwrap(),
            Decimal::from_str("0.005").unwrap()
        );
    }

    /// Invariant: dividing 1 by a very large number that exceeds the
    /// representable precision yields zero.
    #[test]
    fn div_tiny_result_is_zero() {
        let a = Decimal::from_raw(1, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        assert_zero(a.ckecked_div(b).unwrap());
    }

    /// Invariant: dividing u128::MAX by 1 returns u128::MAX unchanged.
    #[test]
    fn div_large_result() {
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let r = a.ckecked_div(Decimal::from_str("1").unwrap()).unwrap();
        assert_positive(r, u128::MAX, 0, &u128::MAX.to_string());
    }

    /// Invariant: when the quotient after scale adjustment would exceed
    /// u128 limits, rescale_up returns Overflow.
    #[test]
    fn div_overflow_rescale_up() {
        // u128::MAX / (1/10^38) = u128::MAX * 10^38 → overflow
        let a = Decimal::from_raw(u128::MAX, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        assert_eq!(a.ckecked_div(b), Err(DecimalError::Overflow));
    }

    /// Invariant: when the accumulated fractional digits overflow u128
    /// during long division, the operation halts and returns Overflow.
    #[test]
    fn div_overflow_fractional_digits() {
        // 10^35 / 3: int_quot is huge (~34 digits), fractional digits overflow fast
        let a = Decimal::from_raw(10u128.pow(35), 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 0, Sign::Positive).unwrap();
        let r = a.ckecked_div(b);
        match r {
            Ok(val) => {
                let expected_min = 10u128.pow(35) / 3;
                assert!(val.as_raw() >= expected_min);
            }
            Err(e) => assert_eq!(e, DecimalError::Overflow),
        }
    }

    /// Invariant: division that produces all-zero fractional digits
    /// (e.g. 1 / 1000000) correctly yields the integer with proper scale.
    #[test]
    fn div_all_zero_fractional_digits() {
        let r = Decimal::from_str("1")
            .unwrap()
            .ckecked_div(Decimal::from_str("1000000").unwrap())
            .unwrap();
        assert_positive(r, 1, 6, "0.000001");
    }

    /// Invariant: multiplication distributes over addition.
    #[test]
    fn property_distributive() {
        let a = Decimal::from_str("3").unwrap();
        let b = Decimal::from_str("4").unwrap();
        let c = Decimal::from_str("5").unwrap();
        let left = a.checked_mul(b.checked_add(c).unwrap()).unwrap();
        let right = a
            .checked_mul(b)
            .unwrap()
            .checked_add(a.checked_mul(c).unwrap())
            .unwrap();
        assert_eq!(left, right);
    }

    /// Invariant: division is the inverse of multiplication for exact
    /// products (a / b) * b == a.
    #[test]
    fn property_div_mul_roundtrip() {
        let a = Decimal::from_str("100").unwrap();
        let b = Decimal::from_str("25").unwrap();
        let r = a.ckecked_div(b).unwrap();
        assert_eq!(r.checked_mul(b).unwrap(), a);
    }

    /// Invariant: toggling sign via multiplication by -1 produces a value
    /// with the same normalized parts but opposite sign.
    #[test]
    fn property_sign_toggle_by_neg_one() {
        let neg_one = Decimal::from_str("-1").unwrap();
        for s in ["42.5", "0.001", "1000000"] {
            let v = Decimal::from_str(s).unwrap();
            let toggled = neg_one.checked_mul(v).unwrap();
            assert_eq!(v.normalized_parts(), toggled.normalized_parts());
            match (v.sign(), toggled.sign()) {
                (Sign::Positive, Sign::Negative)
                | (Sign::Negative, Sign::Positive)
                | (Sign::Zero, Sign::Zero) => {}
                _ => panic!("sign toggle mismatch for {s}"),
            }
        }
    }

    /// Invariant: dividing two values at the same MAX_SCALE yields a result
    /// at scale 0 with the correct quotient (scales cancel).
    #[test]
    fn div_both_max_scale() {
        // 6/10^38 ÷ 2/10^38 = 3
        let a = Decimal::from_raw(6, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(2, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let r = a.ckecked_div(b).unwrap();
        assert_positive(r, 3, 0, "3");
    }

    /// Invariant: MAX_SCALE division with a non-terminating result still
    /// fills MAX_SCALE digits (scales cancel, diff=0).
    #[test]
    fn div_both_max_scale_non_terminating() {
        let a = Decimal::from_raw(1, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, Decimal::MAX_SCALE, Sign::Positive).unwrap();
        let r = a.ckecked_div(b).unwrap();
        assert_eq!(r.scale(), Decimal::MAX_SCALE);
        assert_eq!(r.sign(), Sign::Positive);
        let expected: u128 = (0..Decimal::MAX_SCALE).fold(0u128, |acc, _| acc * 10 + 3);
        assert_eq!(r.as_raw(), expected);
    }

    /// Invariant: when result_scale + diff exceeds MAX_SCALE, the least
    /// significant digits are dropped and the most significant survive.
    #[test]
    fn div_scale_truncation_partial() {
        // 1/3 fills 38 digits (scale 38).  self.scale=10, other.scale=0.
        // diff=10, new_scale=48, drop=10 → only 28 digits survive.
        let a = Decimal::from_raw(1, 10, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 0, Sign::Positive).unwrap();
        let r = a.ckecked_div(b).unwrap();
        assert_eq!(r.scale(), Decimal::MAX_SCALE);
        assert_eq!(r.sign(), Sign::Positive);
        // 333...3 with 38 digits ÷ 10^10 = 333...3 with 28 digits
        let expected: u128 = (0..(Decimal::MAX_SCALE - 10)).fold(0u128, |acc, _| acc * 10 + 3);
        assert_eq!(r.as_raw(), expected);
    }

    /// Invariant: when the truncation drop equals or exceeds the number of
    /// significant digits, the result is ZERO.
    #[test]
    fn div_scale_truncation_to_zero() {
        // 1/7 fills 38 digits.  self.scale=38, other.scale=0.
        // diff=38, new_scale=76, drop=38 → all 38 digits dropped → 0.
        let a = Decimal::from_raw(1, 38, Sign::Positive).unwrap();
        let b = Decimal::from_raw(7, 0, Sign::Positive).unwrap();
        assert_zero(a.ckecked_div(b).unwrap());
    }

    /// Invariant: dividing by 1 preserves both inner and scale exactly.
    #[test]
    fn div_by_one_exact_representation() {
        let a = Decimal::from_raw(425, 1, Sign::Positive).unwrap(); // 42.5
        let r = a.ckecked_div(Decimal::from_str("1").unwrap()).unwrap();
        assert_positive(r, 425, 1, "42.5");
    }

    /// Invariant: dividing by -1 preserves inner and scale but flips sign.
    #[test]
    fn div_by_neg_one_exact_representation() {
        let a = Decimal::from_raw(425, 1, Sign::Positive).unwrap();
        let r = a.ckecked_div(Decimal::from_str("-1").unwrap()).unwrap();
        assert_negative(r, 425, 1, "-42.5");
    }

    /// Invariant: dividing a scale-2 value by a scale-1 divisor preserves
    /// the correct inner and scale without spurious trailing zeros.
    #[test]
    fn div_scale_dividend_larger_exact_repr() {
        // 0.01 / 0.2 = 0.05 → inner=5, scale=2
        let r = Decimal::from_str("0.01")
            .unwrap()
            .ckecked_div(Decimal::from_str("0.2").unwrap())
            .unwrap();
        assert_positive(r, 5, 2, "0.05");
    }

    /// Invariant: exact integer quotient from multi-digit dividend
    /// produces correct inner and scale.
    #[test]
    fn div_exact_integer_exact_repr() {
        let r = Decimal::from_str("15")
            .unwrap()
            .ckecked_div(Decimal::from_str("5").unwrap())
            .unwrap();
        assert_positive(r, 3, 0, "3");
    }

    /// Invariant: when the integer quotient is zero but the remainder is
    /// large, accumulating fractional digits can overflow u128 mid-loop.
    #[test]
    fn div_fractional_digit_accumulation_overflow() {
        // u128::MAX/2 ÷ 10^35: int_quot=0, remainder ~1.7e38. Each iteration
        // multiplies the growing result by 10; around scale=3 it overflows.
        let a = Decimal::from_raw(u128::MAX / 2, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(10u128.pow(35), 0, Sign::Positive).unwrap();
        let r = a.ckecked_div(b);
        match r {
            Ok(val) => {
                // Must be non-negative and ≤ u128::MAX
                assert!(!val.is_negative());
            }
            Err(e) => assert_eq!(e, DecimalError::Overflow),
        }
    }

    /// Invariant: division with a multi-digit divisor (e.g. 700) correctly
    /// accumulates digits when remainder cycles.
    #[test]
    fn div_periodic_multi_digit_divisor() {
        let r = Decimal::from_str("1")
            .unwrap()
            .ckecked_div(Decimal::from_str("700").unwrap())
            .unwrap();
        assert_positive(
            r,
            142857142857142857142857142857142857,
            38,
            "0.00142857142857142857142857142857142857",
        );
    }

    /// Invariant: dividing a value whose integer quotient is non-zero
    /// by 3 fills all MAX_SCALE fractional digits without overflowing.
    #[test]
    fn div_large_dividend_fills_all_fractional() {
        // 10 / 3: int_quot=3, remainder=1 → 38 fractional digits of .333...
        // Final result after loop: 3 * 10^38 + 333...3 (38 digits)
        // = 333...3 (39 threes) ≈ 3.33e38 < u128::MAX ≈ 3.40e38
        let a = Decimal::from_raw(10, 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 0, Sign::Positive).unwrap();
        let r = a.ckecked_div(b).unwrap();
        assert_eq!(r.scale(), Decimal::MAX_SCALE);
        assert!(r.is_positive());
        let expected: u128 = (0..Decimal::MAX_SCALE + 1).fold(0u128, |acc, _| acc * 10 + 3);
        assert_eq!(r.as_raw(), expected);
    }

    /// Invariant: a huge dividend (10^30) divided by 3 overflows during
    /// fractional digit accumulation because the accumulated result
    /// exceeds u128::MAX.
    #[test]
    fn div_overflow_large_dividend_fractional() {
        let a = Decimal::from_raw(10u128.pow(30), 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 0, Sign::Positive).unwrap();
        assert_eq!(a.ckecked_div(b), Err(DecimalError::Overflow));
    }

    /// Invariant: dividing 10^35 by 3 overflows the accumulated result
    /// during fractional digit computation (35 int digits + >3 frac overflow).
    #[test]
    fn div_overflow_fractional_digits_guaranteed() {
        // 10^35 / 3: int_quot ≈ 3.33...e34 (35 digits).
        // Only 3 more fractional digits fit before overflowing u128.
        let a = Decimal::from_raw(10u128.pow(35), 0, Sign::Positive).unwrap();
        let b = Decimal::from_raw(3, 0, Sign::Positive).unwrap();
        assert_eq!(a.ckecked_div(b), Err(DecimalError::Overflow));
    }

    // ------------------------------------------------------------------------
    // property / algebra tests
    // ------------------------------------------------------------------------

    /// Invariant: addition is commutative for small-scale values.
    #[test]
    fn property_add_commutative() {
        for (x, y) in [
            ("0", "0"),
            ("1.5", "2.5"),
            ("-3.14", "2.718"),
            ("100.001", "0.001"),
            ("9999999.9999999", "-8888888.8888888"),
        ] {
            let a = Decimal::from_str(x).unwrap();
            let b = Decimal::from_str(y).unwrap();
            assert_eq!(
                a.checked_add(b).unwrap(),
                b.checked_add(a).unwrap(),
                "commutative fail: {x} + {y}"
            );
        }
    }

    /// Invariant: multiplication is commutative for values that don't overflow.
    #[test]
    fn property_mul_commutative() {
        for (x, y) in [
            ("1", "1"),
            ("2.5", "4"),
            ("-0.5", "0.25"),
            ("123.456", "-0.001"),
        ] {
            let a = Decimal::from_str(x).unwrap();
            let b = Decimal::from_str(y).unwrap();
            assert_eq!(
                a.checked_mul(b).unwrap(),
                b.checked_mul(a).unwrap(),
                "commutative fail: {x} * {y}"
            );
        }
    }

    /// Invariant: addition is associative for values that don't overflow.
    #[test]
    fn property_add_associative() {
        for (x, y, z) in [
            ("1", "2", "3"),
            ("0.1", "0.2", "0.3"),
            ("-5.5", "3.3", "2.2"),
            ("1000.001", "2000.002", "-3000.003"),
        ] {
            let a = Decimal::from_str(x).unwrap();
            let b = Decimal::from_str(y).unwrap();
            let c = Decimal::from_str(z).unwrap();
            let left = a.checked_add(b).unwrap().checked_add(c).unwrap();
            let right = a.checked_add(b.checked_add(c).unwrap()).unwrap();
            assert_eq!(left, right, "associative fail: ({x} + {y}) + {z}");
        }
    }

    /// Invariant: div then mul roundtrips for exact integer divisions.
    #[test]
    fn property_div_mul_roundtrip_exact() {
        for (x, y) in [
            ("10", "2"),
            ("100", "25"),
            ("-60", "5"),
            ("42", "7"),
            ("1000000", "1000"),
        ] {
            let a = Decimal::from_str(x).unwrap();
            let b = Decimal::from_str(y).unwrap();
            let q = a.ckecked_div(b).unwrap();
            assert_eq!(
                q.checked_mul(b).unwrap(),
                a,
                "div-mul roundtrip fail: {x} / {y}"
            );
        }
    }
}
