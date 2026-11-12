//! Exact arithmetic for `calculate` (configuration.md §8.2).
//!
//! A value is a reduced fraction of two `i128`, so sums, differences, products and
//! quotients of decimals are exact; the only rounding is the final one to the rule's
//! `scale`. Every operation is checked: a value whose numerator or denominator no longer
//! fits 128 bits (about 38 digits in total) is [`CalcError::OutOfRange`], never a wrapped
//! or approximated number. Writing never fails for a representable value: the digits come
//! from long division of the fraction, one digit at a time, at any scale.

use std::cmp::Ordering;

use crate::decimal::Decimal;

/// Why an arithmetic step has no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CalcError {
    /// A value does not fit the exact representation.
    OutOfRange,
    /// A divisor is zero.
    DivisionByZero,
}

/// How the final result is rounded to its scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rounding {
    /// Ties away from zero.
    HalfUp,
    /// Ties to the even digit.
    HalfEven,
    /// Toward zero.
    Truncate,
    /// Toward +∞ (the number of a carried `<` or `<=`).
    Ceiling,
    /// Toward −∞ (the number of a carried `>` or `>=`).
    Floor,
}

/// A reduced fraction: `den > 0` and `gcd(num, den) = 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exact {
    num: i128,
    den: i128,
}

type Calc<T> = Result<T, CalcError>;

fn checked<T>(value: Option<T>) -> Calc<T> {
    value.ok_or(CalcError::OutOfRange)
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    // Both inputs are at most i128::MAX in magnitude when one is a positive denominator.
    i128::try_from(a).unwrap_or(1)
}

fn pow10(exponent: usize) -> Calc<i128> {
    checked(
        u32::try_from(exponent)
            .ok()
            .and_then(|e| 10_i128.checked_pow(e)),
    )
}

impl Exact {
    pub(crate) const ZERO: Self = Self { num: 0, den: 1 };

    fn new(num: i128, den: i128) -> Calc<Self> {
        if den == 0 {
            return Err(CalcError::DivisionByZero);
        }
        let g = gcd(num, den);
        let (num, den) = (num / g, den / g);
        if den < 0 {
            Ok(Self {
                num: checked(num.checked_neg())?,
                den: checked(den.checked_neg())?,
            })
        } else {
            Ok(Self { num, den })
        }
    }

    /// The value of a parsed decimal.
    pub(crate) fn from_decimal(decimal: &Decimal) -> Calc<Self> {
        let (negative, integer, fraction) = decimal.parts();
        let mut num: i128 = 0;
        for digit in integer.bytes().chain(fraction.bytes()) {
            num = checked(
                num.checked_mul(10)
                    .and_then(|n| n.checked_add(i128::from(digit - b'0'))),
            )?;
        }
        if negative {
            num = -num;
        }
        Self::new(num, pow10(fraction.len())?)
    }

    pub(crate) const fn is_zero(self) -> bool {
        self.num == 0
    }

    pub(crate) const fn is_negative(self) -> bool {
        self.num < 0
    }

    pub(crate) fn neg(self) -> Calc<Self> {
        Ok(Self {
            num: checked(self.num.checked_neg())?,
            den: self.den,
        })
    }

    pub(crate) fn abs(self) -> Calc<Self> {
        if self.is_negative() {
            self.neg()
        } else {
            Ok(self)
        }
    }

    pub(crate) fn add(self, other: Self) -> Calc<Self> {
        let g = gcd(self.den, other.den);
        let (left, right) = (other.den / g, self.den / g);
        let num = checked(
            self.num
                .checked_mul(left)
                .zip(other.num.checked_mul(right))
                .and_then(|(a, b)| a.checked_add(b)),
        )?;
        Self::new(num, checked(self.den.checked_mul(left))?)
    }

    pub(crate) fn sub(self, other: Self) -> Calc<Self> {
        self.add(other.neg()?)
    }

    pub(crate) fn mul(self, other: Self) -> Calc<Self> {
        let g1 = gcd(self.num, other.den);
        let g2 = gcd(other.num, self.den);
        let num = checked((self.num / g1).checked_mul(other.num / g2))?;
        let den = checked((self.den / g2).checked_mul(other.den / g1))?;
        Self::new(num, den)
    }

    pub(crate) fn div(self, other: Self) -> Calc<Self> {
        if other.is_zero() {
            return Err(CalcError::DivisionByZero);
        }
        self.mul(Self::new(other.den, other.num)?)
    }

    pub(crate) fn compare(self, other: Self) -> Calc<Ordering> {
        Ok(self.sub(other)?.num.cmp(&0))
    }

    /// The value as decimal text: rounded to `scale` places and written with exactly that
    /// many, or, without a scale, exactly in its shortest form. Never `-0`.
    pub(crate) fn to_text(self, scale: Option<u32>, rounding: Rounding) -> Calc<String> {
        Ok(match scale {
            Some(places) => self.decimal(places, rounding),
            // Exact: the digits end before `places`, so truncation drops nothing.
            None => trim_fraction(self.decimal(self.terminating_places()?, Rounding::Truncate)),
        })
    }

    /// The value written with `places` fraction digits, rounded. Digits come from long
    /// division of the remainder by the denominator, one digit at a time in `u128`, so no
    /// intermediate is ever larger than twice the denominator: every representable value
    /// prints at any scale.
    fn decimal(self, places: u32, rounding: Rounding) -> String {
        let den = self.den.unsigned_abs();
        let magnitude = self.num.unsigned_abs();
        let mut digits: Vec<u8> = (magnitude / den).to_string().into_bytes();
        let mut remainder = magnitude % den;
        for _ in 0..places {
            let (digit, rest) = times_ten(remainder, den);
            digits.push(b'0' + digit);
            remainder = rest;
        }
        let last_odd = digits.last().is_some_and(|d| (d - b'0') % 2 == 1);
        let negative = self.num < 0;
        let away = remainder != 0
            && match rounding {
                // `2 × remainder < 2 × den ≤ u128::MAX`, because `den ≤ i128::MAX`.
                Rounding::HalfUp => remainder * 2 >= den,
                Rounding::HalfEven => remainder * 2 > den || (remainder * 2 == den && last_odd),
                Rounding::Truncate => false,
                Rounding::Ceiling => !negative,
                Rounding::Floor => negative,
            };
        if away {
            increment(&mut digits);
        }
        // A carry may have added an integer digit.
        let integer_len = digits.len().saturating_sub(places_len(places));
        let mut text = String::with_capacity(digits.len() + 2);
        if negative && digits.iter().any(|d| *d != b'0') {
            text.push('-');
        }
        for (i, digit) in digits.iter().enumerate() {
            if i == integer_len {
                text.push('.');
            }
            text.push(char::from(*digit));
        }
        text
    }

    /// The number of fraction digits of the exact value when the denominator is
    /// `2^a × 5^b` (always true without `div`, which validation pairs with a scale).
    fn terminating_places(self) -> Calc<u32> {
        let mut den = self.den;
        let (mut twos, mut fives) = (0_u32, 0_u32);
        while den % 2 == 0 {
            den /= 2;
            twos += 1;
        }
        while den % 5 == 0 {
            den /= 5;
            fives += 1;
        }
        if den == 1 {
            Ok(twos.max(fives))
        } else {
            Err(CalcError::OutOfRange)
        }
    }
}

/// `places` as a length.
fn places_len(places: u32) -> usize {
    usize::try_from(places).unwrap_or(usize::MAX)
}

/// One long-division step: `(10 × remainder) / den` and its remainder, by ten modular
/// additions so the product is never formed (`remainder < den ≤ i128::MAX`).
fn times_ten(remainder: u128, den: u128) -> (u8, u128) {
    let mut digit = 0;
    let mut sum = 0;
    for _ in 0..10 {
        sum += remainder;
        if sum >= den {
            sum -= den;
            digit += 1;
        }
    }
    (digit, sum)
}

/// Adds one unit in the last place of a digit string, carrying to the left.
fn increment(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return;
        }
    }
    digits.insert(0, b'1');
}

/// Drops trailing fraction zeroes and a trailing point.
fn trim_fraction(mut text: String) -> String {
    if text.contains('.') {
        let kept = text.trim_end_matches('0').trim_end_matches('.').len();
        text.truncate(kept);
    }
    text
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn x(text: &str) -> Exact {
        Exact::from_decimal(&Decimal::parse(text).expect("a decimal")).expect("in range")
    }

    fn text(value: Exact, scale: Option<u32>, rounding: Rounding) -> String {
        value.to_text(scale, rounding).expect("in range")
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(x("0.1").add(x("0.2")), Ok(x("0.3")));
        assert_eq!(x("5.50").mul(x("18.016")), Ok(x("99.088")));
        assert_eq!(x("1").div(x("3")).and_then(|v| v.mul(x("3"))), Ok(x("1")));
        assert_eq!(
            x("140").sub(x("100").add(x("24")).expect("sum")),
            Ok(x("16"))
        );
        assert_eq!(x("-2.5").abs(), Ok(x("2.5")));
        assert_eq!(x("1").div(x("0")), Err(CalcError::DivisionByZero));
        assert_eq!(x("1").div(x("-0.00")), Err(CalcError::DivisionByZero));
        assert_eq!(x("2").compare(x("1.99")), Ok(Ordering::Greater));
    }

    #[test]
    fn text_without_scale_is_shortest_exact() {
        let r = Rounding::HalfUp;
        assert_eq!(text(x("5.50").mul(x("2")).expect("product"), None, r), "11");
        assert_eq!(
            text(x("5.50").mul(x("18.016")).expect("product"), None, r),
            "99.088"
        );
        assert_eq!(text(x("-0.25"), None, r), "-0.25");
        assert_eq!(text(x("-0"), None, r), "0");
        assert_eq!(text(x(".5"), None, r), "0.5");
        assert_eq!(
            text(x("1").div(x("8")).expect("quotient"), None, r),
            "0.125"
        );
        assert_eq!(
            x("1").div(x("3")).and_then(|v| v.to_text(None, r)),
            Err(CalcError::OutOfRange)
        );
    }

    #[test]
    fn rounding_modes_at_ties_and_signs() {
        let cases = [
            ("2.345", "2.35", "2.34", "2.34"),
            ("-2.345", "-2.35", "-2.34", "-2.34"),
            ("2.335", "2.34", "2.34", "2.33"),
            ("2.3449", "2.34", "2.34", "2.34"),
            ("2.3451", "2.35", "2.35", "2.34"),
            ("-0.001", "0.00", "0.00", "0.00"),
            ("-0.005", "-0.01", "0.00", "0.00"),
            ("5.5", "5.50", "5.50", "5.50"),
        ];
        for (input, up, even, truncate) in cases {
            assert_eq!(
                text(x(input), Some(2), Rounding::HalfUp),
                up,
                "{input} half_up"
            );
            assert_eq!(
                text(x(input), Some(2), Rounding::HalfEven),
                even,
                "{input} half_even"
            );
            assert_eq!(
                text(x(input), Some(2), Rounding::Truncate),
                truncate,
                "{input} truncate"
            );
        }
        assert_eq!(text(x("2.5"), Some(0), Rounding::HalfEven), "2");
        assert_eq!(text(x("3.5"), Some(0), Rounding::HalfEven), "4");
        assert_eq!(text(x("99.5"), Some(0), Rounding::HalfUp), "100");
        assert_eq!(
            text(
                x("2").div(x("3")).expect("quotient"),
                Some(4),
                Rounding::HalfUp
            ),
            "0.6667"
        );
        assert_eq!(
            text(
                x("5").mul(x("0.0555")).expect("product"),
                Some(2),
                Rounding::HalfUp
            ),
            "0.28"
        );
    }

    #[test]
    fn overflow_is_out_of_range_not_wrapped() {
        let big = x("99999999999999999999999999999999999999");
        assert_eq!(big.mul(big), Err(CalcError::OutOfRange));
        assert_eq!(big.add(big), Err(CalcError::OutOfRange));
        let too_long = Decimal::parse("1234567890123456789012345678901234567890").expect("decimal");
        assert_eq!(Exact::from_decimal(&too_long), Err(CalcError::OutOfRange));
        let fine = Decimal::parse("0.00000000000000000000000000000000000001").expect("decimal");
        assert!(Exact::from_decimal(&fine).is_ok());
    }

    #[test]
    fn representable_high_precision_values_are_written_exactly() {
        let r = Rounding::HalfUp;
        for value in [
            "1234567890.123456789012345678",
            "12345678901234567890.123456789",
            "0.00000000000000000000000000000000000001",
            "99999999999999999999999999999999999999",
            "-1234567890.123456789012345678",
        ] {
            assert_eq!(text(x(value), None, r), value, "identity");
        }
        assert_eq!(
            text(
                x("12345678901234567890.123456789")
                    .add(x("0.000000001"))
                    .expect("sum"),
                None,
                r
            ),
            "12345678901234567890.12345679"
        );
        assert_eq!(
            text(x("1234567890.123456789012345678"), Some(12), r),
            "1234567890.123456789012"
        );
        assert_eq!(
            text(x("1234567890.123456789012345678"), Some(2), r),
            "1234567890.12"
        );
        assert_eq!(
            text(x("-1234567890.125"), Some(2), Rounding::HalfEven),
            "-1234567890.12"
        );
    }

    #[test]
    fn long_fractions_print_at_every_scale() {
        let r = Rounding::HalfUp;
        let frac = x("0.1234567890123456789012345678");
        assert_eq!(text(frac, Some(12), r), "0.123456789012");
        assert_eq!(text(frac, Some(2), r), "0.12");
        assert_eq!(text(frac, None, r), "0.1234567890123456789012345678");
        let seventh = x("1")
            .div(x("7.000000000000000000000000001"))
            .expect("quotient");
        assert_eq!(text(seventh, Some(12), r), "0.142857142857");
        assert_eq!(text(seventh, Some(2), r), "0.14");
        let negative = x("-1")
            .div(x("7.000000000000000000000000001"))
            .expect("quotient");
        assert_eq!(text(negative, Some(12), Rounding::Floor), "-0.142857142858");
        assert_eq!(
            text(negative, Some(12), Rounding::Ceiling),
            "-0.142857142857"
        );
    }

    #[test]
    fn range_edges_print_at_every_scale() {
        let r = Rounding::HalfUp;
        // The largest denominators still divide digit by digit.
        let tiny = x("0.00000000000000000000000000000000000001");
        assert_eq!(text(tiny, Some(12), r), "0.000000000000");
        assert_eq!(text(tiny, Some(12), Rounding::Ceiling), "0.000000000001");
        assert_eq!(
            text(tiny.neg().expect("negated"), Some(12), r),
            "0.000000000000"
        );
        // Integer parts at the top of the range keep every scale.
        let big = x("99999999999999999999999999999999999999");
        assert_eq!(
            text(big, Some(12), r),
            "99999999999999999999999999999999999999.000000000000"
        );
        let top = x("17014118346046923173168730371588410572.5");
        assert_eq!(
            text(top, Some(0), r),
            "17014118346046923173168730371588410573"
        );
        assert_eq!(
            text(top, Some(0), Rounding::HalfEven),
            "17014118346046923173168730371588410572"
        );
        assert_eq!(
            text(top, Some(3), r),
            "17014118346046923173168730371588410572.500"
        );
        assert_eq!(text(x("9.9995"), Some(3), r), "10.000");
        assert_eq!(text(x("-9.9995"), Some(3), r), "-10.000");
    }

    #[test]
    fn directional_rounding_never_tightens_a_bound() {
        assert_eq!(text(x("0.2725"), Some(2), Rounding::Ceiling), "0.28");
        assert_eq!(text(x("-0.2725"), Some(2), Rounding::Ceiling), "-0.27");
        assert_eq!(text(x("17.68"), Some(1), Rounding::Floor), "17.6");
        assert_eq!(text(x("-17.61"), Some(1), Rounding::Floor), "-17.7");
        assert_eq!(text(x("0.28"), Some(2), Rounding::Ceiling), "0.28");
    }

    proptest! {
        #[test]
        fn integer_arithmetic_matches_i64(a in -1_000_000_000i64..1_000_000_000, b in -1_000_000_000i64..1_000_000_000) {
            let (ea, eb) = (x(&a.to_string()), x(&b.to_string()));
            let r = Rounding::HalfUp;
            prop_assert_eq!(text(ea.add(eb).expect("sum"), None, r), (a + b).to_string());
            prop_assert_eq!(text(ea.sub(eb).expect("difference"), None, r), (a - b).to_string());
            prop_assert_eq!(text(ea.mul(eb).expect("product"), None, r), (i128::from(a) * i128::from(b)).to_string());
            prop_assert_eq!(ea.compare(eb).expect("order"), a.cmp(&b));
        }

        #[test]
        fn division_then_multiplication_round_trips(a in -1_000_000i64..1_000_000, b in 1i64..1_000_000) {
            let (ea, eb) = (x(&a.to_string()), x(&b.to_string()));
            prop_assert_eq!(ea.div(eb).and_then(|q| q.mul(eb)), Ok(ea));
        }

        #[test]
        fn scaled_text_parses_back_within_half_a_unit(n in -10_000_000i64..10_000_000, d in 1i64..10_000, places in 0u32..6) {
            let value = x(&n.to_string()).div(x(&d.to_string())).expect("quotient");
            let shown = x(&text(value, Some(places), Rounding::HalfEven));
            let half = x("0.5").div(x(&10_i64.pow(places).to_string())).expect("half unit");
            let error = shown.sub(value).and_then(Exact::abs).expect("difference");
            prop_assert!(error.compare(half).expect("order") != Ordering::Greater);
        }
    }
}
