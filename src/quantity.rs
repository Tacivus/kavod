use std::{fmt, hash::Hash, str::FromStr};

use thiserror::Error;

use crate::decimal::{Decimal, DecimalError};

#[derive(Error, Debug, PartialEq, Eq)]
pub enum QuantityError {
    #[error("quantity must be non-negative")]
    Negative,

    #[error("{0}")]
    Decimal(#[from] DecimalError),
}

/// A quantity value — a typed newtype over [`Decimal`].
///
/// Quantities are **non-negative** (zero is allowed).  The newtype provides
/// type-level distinction and checked arithmetic that preserves the
/// non-negative invariant.
#[derive(Clone, Copy, Debug)]
pub struct Quantity(Decimal);

impl Quantity {
    pub const ZERO: Self = Quantity(Decimal::ZERO);

    pub fn new(value: Decimal) -> Result<Self, QuantityError> {
        if value.is_negative() {
            return Err(QuantityError::Negative);
        }
        Ok(Quantity(value))
    }

    pub fn as_decimal(&self) -> Decimal {
        self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn checked_add(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let result = self.0.checked_add(rhs.0)?;
        if result.is_negative() {
            return Err(QuantityError::Negative);
        }
        Ok(Quantity(result))
    }

    pub fn checked_sub(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let result = self.0.checked_sub(rhs.0)?;
        if result.is_negative() {
            return Err(QuantityError::Negative);
        }
        Ok(Quantity(result))
    }

    pub fn checked_mul(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let result = self.0.checked_mul(rhs.0)?;
        if result.is_negative() {
            return Err(QuantityError::Negative);
        }
        Ok(Quantity(result))
    }

    pub fn checked_div(self, rhs: Quantity) -> Result<Quantity, QuantityError> {
        let result = self.0.ckecked_div(rhs.0)?;
        if result.is_negative() {
            return Err(QuantityError::Negative);
        }
        Ok(Quantity(result))
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl Eq for Quantity {}

impl Hash for Quantity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialOrd for Quantity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Quantity {
    type Err = QuantityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let d = Decimal::from_str(s)?;
        Quantity::new(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{DefaultHasher, Hasher};

    fn hash_of(q: &Quantity) -> u64 {
        let mut h = DefaultHasher::new();
        q.hash(&mut h);
        h.finish()
    }

    // ==================================================================
    // Construction — new / as_decimal roundtrip
    // ==================================================================

    /// Invariant: new(d).as_decimal() == d for positive values
    #[test]
    fn new_as_decimal_roundtrip_positive() {
        let d = Decimal::from_str("100.5").unwrap();
        let q = Quantity::new(d).unwrap();
        assert_eq!(q.as_decimal(), d);
    }

    /// Invariant: new(ZERO) wraps correctly and is_zero() == true
    #[test]
    fn new_as_decimal_roundtrip_zero() {
        let q = Quantity::new(Decimal::ZERO).unwrap();
        assert_eq!(q.as_decimal(), Decimal::ZERO);
        assert!(q.is_zero());
    }

    /// Invariant: negative value construction returns Err(Negative)
    #[test]
    fn new_negative_rejected() {
        let d = Decimal::from_str("-1.0").unwrap();
        assert_eq!(Quantity::new(d), Err(QuantityError::Negative));
    }

    /// Invariant: a Decimal with sign=Negative but inner=0 is already
    /// rejected by Decimal::from_raw (MismatchedSign), so new() never sees it.
    /// Still, explicit zero-even-with-negative-signal check.
    #[test]
    fn new_zero_from_str_succeeds() {
        let d = Decimal::from_str("0").unwrap();
        assert!(Quantity::new(d).is_ok());
    }

    // ==================================================================
    // checked_add
    // ==================================================================

    /// Invariant: adding two positive quantities yields the correct sum
    #[test]
    fn checked_add_basic() {
        let a = Quantity::from_str("2.5").unwrap();
        let b = Quantity::from_str("3.5").unwrap();
        assert_eq!(
            a.checked_add(b).unwrap(),
            Quantity::from_str("6.0").unwrap()
        );
    }

    /// Invariant: ZERO is the additive identity
    #[test]
    fn checked_add_zero_identity() {
        let q = Quantity::from_str("42.5").unwrap();
        assert_eq!(q.checked_add(Quantity::ZERO).unwrap(), q);
        assert_eq!(Quantity::ZERO.checked_add(q).unwrap(), q);
    }

    /// Invariant: overflow returns Err
    #[test]
    fn checked_add_overflow() {
        let a =
            Quantity::new(Decimal::from_raw(u128::MAX, 0, crate::decimal::Sign::Positive).unwrap())
                .unwrap();
        let b = Quantity::from_str("1").unwrap();
        assert!(a.checked_add(b).is_err());
    }

    /// Invariant: both operands are non-negative, so result is always
    /// non-negative (never needs the Negative guard)
    #[test]
    fn checked_add_result_is_non_negative() {
        let a = Quantity::from_str("0.1").unwrap();
        let b = Quantity::from_str("0.2").unwrap();
        let r = a.checked_add(b).unwrap();
        assert!(!r.as_decimal().is_negative());
    }

    // ==================================================================
    // checked_sub — core invariant: result is never negative
    // ==================================================================

    /// Invariant: larger minus smaller yields the correct positive difference
    #[test]
    fn checked_sub_basic() {
        let a = Quantity::from_str("5.0").unwrap();
        let b = Quantity::from_str("3.0").unwrap();
        assert_eq!(
            a.checked_sub(b).unwrap(),
            Quantity::from_str("2.0").unwrap()
        );
    }

    /// Invariant: subtracting ZERO is the identity
    #[test]
    fn checked_sub_zero_identity() {
        let q = Quantity::from_str("42.5").unwrap();
        assert_eq!(q.checked_sub(Quantity::ZERO).unwrap(), q);
    }

    /// Invariant: x - x == ZERO
    #[test]
    fn checked_sub_self_yields_zero() {
        let q = Quantity::from_str("7.25").unwrap();
        assert_eq!(q.checked_sub(q).unwrap(), Quantity::ZERO);
    }

    /// Invariant: ZERO - ZERO == ZERO
    #[test]
    fn checked_sub_zero_minus_zero() {
        assert_eq!(
            Quantity::ZERO.checked_sub(Quantity::ZERO).unwrap(),
            Quantity::ZERO
        );
    }

    /// Invariant: smaller minus larger returns Err(Negative)
    #[test]
    fn checked_sub_smaller_minus_larger_rejected() {
        let a = Quantity::from_str("3.0").unwrap();
        let b = Quantity::from_str("5.0").unwrap();
        assert_eq!(a.checked_sub(b), Err(QuantityError::Negative));
    }

    /// Invariant: ZERO minus positive returns Err(Negative)
    #[test]
    fn checked_sub_zero_minus_positive_rejected() {
        let q = Quantity::from_str("1.0").unwrap();
        assert_eq!(Quantity::ZERO.checked_sub(q), Err(QuantityError::Negative));
    }

    /// Invariant: subtracting a larger Decimal with same numerical value
    /// but different scale also fails (e.g. 1.0 vs 1.00)
    #[test]
    fn checked_sub_equal_value_different_scale_yields_zero() {
        let a = Quantity::from_str("1.00").unwrap();
        let b = Quantity::from_str("1.0").unwrap();
        assert_eq!(a.checked_sub(b).unwrap(), Quantity::ZERO);
    }

    /// Invariant: exact difference that is positive but produced by
    /// Decimal::sub with sign handling — result stays non-negative
    #[test]
    fn checked_sub_fractional_difference() {
        let a = Quantity::from_str("1.1").unwrap();
        let b = Quantity::from_str("1.09").unwrap();
        assert_eq!(
            a.checked_sub(b).unwrap(),
            Quantity::from_str("0.01").unwrap()
        );
    }

    // ==================================================================
    // checked_mul — both non-neg, result non-neg
    // ==================================================================

    /// Invariant: multiplying two quantities yields the correct product
    #[test]
    fn checked_mul_basic() {
        let a = Quantity::from_str("2.5").unwrap();
        let b = Quantity::from_str("4.0").unwrap();
        assert_eq!(
            a.checked_mul(b).unwrap(),
            Quantity::from_str("10.0").unwrap()
        );
    }

    /// Invariant: multiplying by ZERO yields ZERO
    #[test]
    fn checked_mul_zero_annihilator() {
        let q = Quantity::from_str("42.5").unwrap();
        assert_eq!(q.checked_mul(Quantity::ZERO).unwrap(), Quantity::ZERO);
    }

    /// Invariant: 1 is the multiplicative identity
    #[test]
    fn checked_mul_one_identity() {
        let q = Quantity::from_str("42.5").unwrap();
        let one = Quantity::from_str("1").unwrap();
        assert_eq!(q.checked_mul(one).unwrap(), q);
    }

    /// Invariant: both non-neg → product non-neg (never hits Negative guard)
    #[test]
    fn checked_mul_result_is_non_negative() {
        let a = Quantity::from_str("999.999").unwrap();
        let b = Quantity::from_str("0.001").unwrap();
        let r = a.checked_mul(b).unwrap();
        assert!(!r.as_decimal().is_negative());
    }

    /// Invariant: overflow returns Err
    #[test]
    fn checked_mul_overflow() {
        let a =
            Quantity::new(Decimal::from_raw(u128::MAX, 0, crate::decimal::Sign::Positive).unwrap())
                .unwrap();
        let b = Quantity::from_str("2").unwrap();
        assert!(a.checked_mul(b).is_err());
    }

    // ==================================================================
    // checked_div — both non-neg, result non-neg
    // ==================================================================

    /// Invariant: exact integer division yields the correct quotient
    #[test]
    fn checked_div_basic() {
        let a = Quantity::from_str("6.0").unwrap();
        let b = Quantity::from_str("3.0").unwrap();
        assert_eq!(
            a.checked_div(b).unwrap(),
            Quantity::from_str("2.0").unwrap()
        );
    }

    /// Invariant: dividing by 1 is the identity
    #[test]
    fn checked_div_one_identity() {
        let q = Quantity::from_str("42.5").unwrap();
        let one = Quantity::from_str("1").unwrap();
        assert_eq!(q.checked_div(one).unwrap(), q);
    }

    /// Invariant: x / x == 1
    #[test]
    fn checked_div_self_yields_one() {
        let q = Quantity::from_str("7.0").unwrap();
        assert_eq!(q.checked_div(q).unwrap(), Quantity::from_str("1").unwrap());
    }

    /// Invariant: division by zero returns DivisionByZero
    #[test]
    fn checked_div_by_zero() {
        let q = Quantity::from_str("5.0").unwrap();
        assert_eq!(
            q.checked_div(Quantity::ZERO),
            Err(QuantityError::Decimal(DecimalError::DivisionByZero))
        );
    }

    /// Invariant: both non-neg → quotient non-neg
    #[test]
    fn checked_div_result_is_non_negative() {
        let a = Quantity::from_str("1.0").unwrap();
        let b = Quantity::from_str("3.0").unwrap();
        let r = a.checked_div(b).unwrap();
        assert!(!r.as_decimal().is_negative());
    }

    // ==================================================================
    // Eq / PartialEq
    // ==================================================================

    /// Invariant: semantically equal quantities with different representations
    /// are equal (trailing zeros don't matter)
    #[test]
    fn partial_eq_trailing_zeros() {
        let a = Quantity::from_str("1.00").unwrap();
        let b = Quantity::from_str("1.0").unwrap();
        assert_eq!(a, b);
    }

    /// Invariant: different numerical values are not equal
    #[test]
    fn partial_eq_different_value() {
        let a = Quantity::from_str("1.0").unwrap();
        let b = Quantity::from_str("2.0").unwrap();
        assert_ne!(a, b);
    }

    /// Invariant: all zero forms are equal
    #[test]
    fn partial_eq_zero_forms_equal() {
        assert_eq!(Quantity::ZERO, Quantity::from_str("0").unwrap());
    }

    /// Invariant: zero is not equal to any non-zero value
    #[test]
    fn partial_eq_zero_not_equal_to_nonzero() {
        assert_ne!(Quantity::ZERO, Quantity::from_str("1").unwrap());
    }

    // ==================================================================
    // Hash
    // ==================================================================

    /// Invariant: equal quantities produce the same hash
    #[test]
    fn hash_equal_values() {
        let a = Quantity::from_str("1.00").unwrap();
        let b = Quantity::from_str("1.0").unwrap();
        assert_eq!(hash_of(&a), hash_of(&b));
    }

    /// Invariant: different quantities produce different hashes
    #[test]
    fn hash_different_values() {
        let a = Quantity::from_str("1.0").unwrap();
        let b = Quantity::from_str("2.0").unwrap();
        assert_ne!(hash_of(&a), hash_of(&b));
    }

    /// Invariant: all zero forms produce the same hash
    #[test]
    fn hash_zero_forms_equal() {
        assert_eq!(
            hash_of(&Quantity::ZERO),
            hash_of(&Quantity::from_str("0").unwrap())
        );
    }

    // ==================================================================
    // Ord / PartialOrd
    // ==================================================================

    /// Invariant: smaller < larger
    #[test]
    fn ord_less_than() {
        let a = Quantity::from_str("1.0").unwrap();
        let b = Quantity::from_str("2.0").unwrap();
        assert!(a < b);
        assert!(b > a);
    }

    /// Invariant: ZERO is less than any positive quantity
    #[test]
    fn ord_zero_less_than_positive() {
        let pos = Quantity::from_str("0.001").unwrap();
        assert!(Quantity::ZERO < pos);
    }

    /// Invariant: trailing zeros don't affect ordering
    #[test]
    fn ord_trailing_zeros_equal() {
        let a = Quantity::from_str("1.00").unwrap();
        let b = Quantity::from_str("1.0").unwrap();
        assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
    }

    /// Invariant: sort produces correct ascending order
    #[test]
    fn ord_sort_integrity() {
        let mut vals = [
            Quantity::from_str("2.5").unwrap(),
            Quantity::from_str("0").unwrap(),
            Quantity::from_str("1.0").unwrap(),
            Quantity::from_str("0.1").unwrap(),
            Quantity::from_str("1.00").unwrap(),
        ];
        vals.sort();
        assert_eq!(
            vals.iter().map(|q| format!("{q}")).collect::<Vec<_>>(),
            vec!["0", "0.1", "1.0", "1.00", "2.5"]
        );
    }

    /// Invariant: PartialOrd always returns Some (total order, no NaN)
    #[test]
    fn ord_partial_cmp_always_some() {
        let a = Quantity::from_str("3.14").unwrap();
        let b = Quantity::from_str("2.718").unwrap();
        assert!(a.partial_cmp(&b).is_some());
        assert_eq!(a.partial_cmp(&b), Some(std::cmp::Ordering::Greater));
    }

    // ==================================================================
    // Display / FromStr
    // ==================================================================

    /// Invariant: Display output for positive quantity
    #[test]
    fn display_positive() {
        let q = Quantity::from_str("123.45").unwrap();
        assert_eq!(format!("{q}"), "123.45");
    }

    /// Invariant: ZERO displays as "0"
    #[test]
    fn display_zero() {
        assert_eq!(format!("{}", Quantity::ZERO), "0");
    }

    /// Invariant: Display → FromStr roundtrip is identity
    #[test]
    fn from_str_display_roundtrip() {
        for s in ["0", "1", "123.45", "0.5", "0.001", "100.00"] {
            let q = Quantity::from_str(s).unwrap();
            assert_eq!(
                Quantity::from_str(&format!("{q}")).unwrap(),
                q,
                "roundtrip failed for {s}"
            );
        }
    }

    /// Invariant: negative string returns Err(Negative)
    #[test]
    fn from_str_negative_rejected() {
        assert_eq!(Quantity::from_str("-1.0"), Err(QuantityError::Negative));
        assert_eq!(Quantity::from_str("-0.5"), Err(QuantityError::Negative));
    }

    /// Invariant: "-0" is already rejected by Decimal::from_str
    /// (returns InvalidFormat), which propagates up.
    #[test]
    fn from_str_negative_zero_rejected() {
        assert!(Quantity::from_str("-0").is_err());
    }

    /// Invariant: invalid decimal string returns Err
    #[test]
    fn from_str_invalid() {
        assert!(Quantity::from_str("abc").is_err());
        assert!(Quantity::from_str("").is_err());
    }
}
