use crate::decimal::{Decimal, DecimalError};
use std::{fmt, hash::Hash, str::FromStr};

/// A price value — a typed newtype over [`Decimal`].
///
/// Permits negative prices. The newtype provides type-level distinction
/// and checked arithmetic that delegates to the underlying [`Decimal`].
#[derive(Clone, Copy, Debug)]
pub struct Price(Decimal);

impl Price {
    pub const ZERO: Self = Price(Decimal::ZERO);

    pub fn new(value: Decimal) -> Self {
        Price(value)
    }

    pub fn as_decimal(&self) -> Decimal {
        self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn checked_add(self, rhs: Price) -> Result<Price, DecimalError> {
        Ok(Price(self.0.checked_add(rhs.0)?))
    }

    pub fn checked_sub(self, rhs: Price) -> Result<Price, DecimalError> {
        Ok(Price(self.0.checked_sub(rhs.0)?))
    }

    pub fn checked_mul(self, rhs: Price) -> Result<Price, DecimalError> {
        Ok(Price(self.0.checked_mul(rhs.0)?))
    }

    pub fn checked_div(self, rhs: Price) -> Result<Price, DecimalError> {
        Ok(Price(self.0.ckecked_div(rhs.0)?))
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl Eq for Price {}

impl Hash for Price {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Price {
    type Err = DecimalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Price(Decimal::from_str(s)?))
    }
}

/// These tests are much lighter b/c `Price` is just a thin wrapper around `Decimal`
/// which is already well tested
#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{DefaultHasher, Hasher};

    fn hash_of(p: &Price) -> u64 {
        let mut h = DefaultHasher::new();
        p.hash(&mut h);
        h.finish()
    }

    // ==================================================================
    // Construction — new / as_decimal roundtrip
    // ==================================================================

    /// Invariant: new(d).as_decimal() == d for positive values
    #[test]
    fn new_as_decimal_roundtrip_positive() {
        let d = Decimal::from_str("123.45").unwrap();
        let p = Price::new(d);
        assert_eq!(p.as_decimal(), d);
    }

    /// Invariant: new(ZERO) wraps correctly and is_zero() == true
    #[test]
    fn new_as_decimal_roundtrip_zero() {
        let p = Price::new(Decimal::ZERO);
        assert_eq!(p.as_decimal(), Decimal::ZERO);
        assert!(p.is_zero());
    }

    /// Invariant: new(d).as_decimal() == d for negative values
    #[test]
    fn new_as_decimal_roundtrip_negative() {
        let d = Decimal::from_str("-67.89").unwrap();
        let p = Price::new(d);
        assert_eq!(p.as_decimal(), d);
    }

    // ==================================================================
    // checked_add
    // ==================================================================

    /// Invariant: adding two positive prices yields the correct sum
    #[test]
    fn checked_add_basic() {
        let a = Price::from_str("2.5").unwrap();
        let b = Price::from_str("3.5").unwrap();
        assert_eq!(a.checked_add(b).unwrap(), Price::from_str("6.0").unwrap());
    }

    /// Invariant: ZERO is the additive identity
    #[test]
    fn checked_add_zero_identity() {
        let p = Price::from_str("42.5").unwrap();
        assert_eq!(p.checked_add(Price::ZERO).unwrap(), p);
        assert_eq!(Price::ZERO.checked_add(p).unwrap(), p);
    }

    /// Invariant: overflow returns Err
    #[test]
    fn checked_add_overflow() {
        let a =
            Price::new(Decimal::from_raw(u128::MAX, 0, crate::decimal::Sign::Positive).unwrap());
        let b = Price::new(Decimal::from_raw(1, 0, crate::decimal::Sign::Positive).unwrap());
        assert!(a.checked_add(b).is_err());
    }

    /// Invariant: adding a negative and a positive yields the correct result
    #[test]
    fn checked_add_mixed_signs() {
        let a = Price::from_str("10.0").unwrap();
        let b = Price::from_str("-4.0").unwrap();
        assert_eq!(a.checked_add(b).unwrap(), Price::from_str("6.0").unwrap());
    }

    // ==================================================================
    // checked_sub
    // ==================================================================

    /// Invariant: larger minus smaller yields the correct positive difference
    #[test]
    fn checked_sub_basic() {
        let a = Price::from_str("5.0").unwrap();
        let b = Price::from_str("3.0").unwrap();
        assert_eq!(a.checked_sub(b).unwrap(), Price::from_str("2.0").unwrap());
    }

    /// Invariant: subtracting ZERO is the identity
    #[test]
    fn checked_sub_zero_identity() {
        let p = Price::from_str("42.5").unwrap();
        assert_eq!(p.checked_sub(Price::ZERO).unwrap(), p);
    }

    /// Invariant: x - x == ZERO
    #[test]
    fn checked_sub_self_yields_zero() {
        let p = Price::from_str("7.25").unwrap();
        assert_eq!(p.checked_sub(p).unwrap(), Price::ZERO);
    }

    /// Invariant: smaller minus larger yields the correct negative difference
    #[test]
    fn checked_sub_negative_result() {
        let a = Price::from_str("3.0").unwrap();
        let b = Price::from_str("5.0").unwrap();
        assert_eq!(a.checked_sub(b).unwrap(), Price::from_str("-2.0").unwrap());
    }

    // ==================================================================
    // checked_mul
    // ==================================================================

    /// Invariant: multiplying two prices yields the correct product
    #[test]
    fn checked_mul_basic() {
        let a = Price::from_str("2.5").unwrap();
        let b = Price::from_str("4.0").unwrap();
        assert_eq!(a.checked_mul(b).unwrap(), Price::from_str("10.0").unwrap());
    }

    /// Invariant: multiplying by ZERO yields ZERO
    #[test]
    fn checked_mul_zero_annihilator() {
        let p = Price::from_str("42.5").unwrap();
        assert_eq!(p.checked_mul(Price::ZERO).unwrap(), Price::ZERO);
    }

    /// Invariant: 1 is the multiplicative identity
    #[test]
    fn checked_mul_one_identity() {
        let p = Price::from_str("42.5").unwrap();
        let one = Price::from_str("1").unwrap();
        assert_eq!(p.checked_mul(one).unwrap(), p);
    }

    /// Invariant: negative * positive yields a negative product
    #[test]
    fn checked_mul_negative() {
        let a = Price::from_str("-2.0").unwrap();
        let b = Price::from_str("3.0").unwrap();
        assert_eq!(a.checked_mul(b).unwrap(), Price::from_str("-6.0").unwrap());
    }

    /// Invariant: negative * negative yields a positive product
    #[test]
    fn checked_mul_double_negative() {
        let a = Price::from_str("-2.0").unwrap();
        let b = Price::from_str("-3.0").unwrap();
        assert_eq!(a.checked_mul(b).unwrap(), Price::from_str("6.0").unwrap());
    }

    /// Invariant: overflow returns Err
    #[test]
    fn checked_mul_overflow() {
        let a =
            Price::new(Decimal::from_raw(u128::MAX, 0, crate::decimal::Sign::Positive).unwrap());
        let b = Price::from_str("2").unwrap();
        assert!(a.checked_mul(b).is_err());
    }

    // ==================================================================
    // checked_div
    // ==================================================================

    /// Invariant: exact integer division yields the correct quotient
    #[test]
    fn checked_div_basic() {
        let a = Price::from_str("6.0").unwrap();
        let b = Price::from_str("3.0").unwrap();
        assert_eq!(a.checked_div(b).unwrap(), Price::from_str("2.0").unwrap());
    }

    /// Invariant: dividing by 1 is the identity
    #[test]
    fn checked_div_one_identity() {
        let p = Price::from_str("42.5").unwrap();
        let one = Price::from_str("1").unwrap();
        assert_eq!(p.checked_div(one).unwrap(), p);
    }

    /// Invariant: x / x == 1
    #[test]
    fn checked_div_self_yields_one() {
        let p = Price::from_str("7.0").unwrap();
        assert_eq!(p.checked_div(p).unwrap(), Price::from_str("1").unwrap());
    }

    /// Invariant: division by zero returns DivisionByZero
    #[test]
    fn checked_div_by_zero() {
        let p = Price::from_str("5.0").unwrap();
        assert_eq!(
            p.checked_div(Price::ZERO),
            Err(DecimalError::DivisionByZero)
        );
    }

    /// Invariant: negative / positive yields a negative quotient
    #[test]
    fn checked_div_negative() {
        let a = Price::from_str("-6.0").unwrap();
        let b = Price::from_str("3.0").unwrap();
        assert_eq!(a.checked_div(b).unwrap(), Price::from_str("-2.0").unwrap());
    }

    // ==================================================================
    // Eq / PartialEq
    // ==================================================================

    /// Invariant: semantically equal prices with different representation
    /// are equal (trailing zeros don't matter)
    #[test]
    fn partial_eq_trailing_zeros() {
        let a = Price::from_str("1.00").unwrap();
        let b = Price::from_str("1.0").unwrap();
        assert_eq!(a, b);
    }

    /// Invariant: different numerical values are not equal
    #[test]
    fn partial_eq_different_value() {
        let a = Price::from_str("1.0").unwrap();
        let b = Price::from_str("2.0").unwrap();
        assert_ne!(a, b);
    }

    /// Invariant: all zero forms are equal
    #[test]
    fn partial_eq_zero_forms_equal() {
        assert_eq!(Price::ZERO, Price::from_str("0").unwrap());
    }

    /// Invariant: zero is not equal to any non-zero value
    #[test]
    fn partial_eq_zero_not_equal_to_nonzero() {
        assert_ne!(Price::ZERO, Price::from_str("1").unwrap());
        assert_ne!(Price::ZERO, Price::from_str("-1").unwrap());
    }

    // ==================================================================
    // Hash
    // ==================================================================

    /// Invariant: equal prices produce the same hash
    #[test]
    fn hash_equal_values() {
        let a = Price::from_str("1.00").unwrap();
        let b = Price::from_str("1.0").unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    /// Invariant: different prices (likely) produce different hashes
    #[test]
    fn hash_different_values() {
        let a = Price::from_str("1.0").unwrap();
        let b = Price::from_str("2.0").unwrap();
        assert_ne!(hash_of(&a), hash_of(&b));
    }

    /// Invariant: all zero forms produce the same hash
    #[test]
    fn hash_zero_forms_equal() {
        assert_eq!(
            hash_of(&Price::ZERO),
            hash_of(&Price::from_str("0").unwrap())
        );
    }

    // ==================================================================
    // Ord / PartialOrd
    // ==================================================================

    /// Invariant: negative < zero < positive
    #[test]
    fn ord_negative_zero_positive() {
        let neg = Price::from_str("-1.0").unwrap();
        let pos = Price::from_str("1.0").unwrap();
        assert!(neg < Price::ZERO);
        assert!(Price::ZERO < pos);
        assert!(neg < pos);
    }

    /// Invariant: trailing zeros don't affect ordering
    #[test]
    fn ord_trailing_zeros_equal() {
        let a = Price::from_str("1.00").unwrap();
        let b = Price::from_str("1.0").unwrap();
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    /// Invariant: sort produces correct ascending order
    #[test]
    fn ord_sort_integrity() {
        let mut vals = [
            Price::from_str("-2.5").unwrap(),
            Price::from_str("0").unwrap(),
            Price::from_str("2.5").unwrap(),
            Price::from_str("-1.0").unwrap(),
            Price::from_str("0.1").unwrap(),
            Price::from_str("1.00").unwrap(),
        ];
        vals.sort();
        assert_eq!(
            vals.iter().map(|p| format!("{p}")).collect::<Vec<_>>(),
            vec!["-2.5", "-1.0", "0", "0.1", "1.00", "2.5"]
        );
    }

    // ==================================================================
    // Display / FromStr
    // ==================================================================

    /// Invariant: Display output matches the parsed input string for
    /// canonical representations
    #[test]
    fn display_positive() {
        let p = Price::from_str("123.45").unwrap();
        assert_eq!(format!("{p}"), "123.45");
    }

    /// Invariant: negative values display with a leading minus sign
    #[test]
    fn display_negative() {
        let p = Price::from_str("-67.89").unwrap();
        assert_eq!(format!("{p}"), "-67.89");
    }

    /// Invariant: ZERO displays as "0"
    #[test]
    fn display_zero() {
        assert_eq!(format!("{}", Price::ZERO), "0");
    }

    /// Invariant: Display → FromStr roundtrip is identity
    #[test]
    fn from_str_display_roundtrip() {
        for s in ["0", "1", "-1", "123.45", "-123.45", "0.5", "-0.5", "0.001"] {
            let p = Price::from_str(s).unwrap();
            assert_eq!(
                Price::from_str(&format!("{p}")).unwrap(),
                p,
                "roundtrip failed for {s}"
            );
        }
    }

    /// Invariant: invalid decimal returns Err
    #[test]
    fn from_str_invalid() {
        assert!(Price::from_str("abc").is_err());
        assert!(Price::from_str("").is_err());
    }
}
