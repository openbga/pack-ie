//! Exact decimal comparison for `gt`, `gte`, `lt`, `lte` (configuration.md §8.1), and the
//! decimal grammar `calculate` operands share ([`crate::exact`]).
//!
//! Values are compared as digit strings, so there is no rounding and no length limit:
//! clinical values are never converted to floating point. The accepted form is an
//! optional sign, digits with an optional fraction (`5`, `-0.25`, `.5`, `7.`). Anything
//! else, including exponents, thousands separators and comparators such as `<5`, is not
//! a number.

use std::cmp::Ordering;

/// A normalised decimal number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decimal {
    negative: bool,
    /// Digits without leading zeroes; empty for zero.
    integer: String,
    /// Digits without trailing zeroes.
    fraction: String,
}

impl Decimal {
    /// Parses `text` (surrounding whitespace is ignored); `None` when it is not a number.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (negative, digits) = match text.as_bytes().first()? {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let (integer, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if (integer.is_empty() && fraction.is_empty())
            || !all_digits(integer)
            || !all_digits(fraction)
        {
            return None;
        }
        let integer = integer.trim_start_matches('0').to_owned();
        let fraction = fraction.trim_end_matches('0').to_owned();
        let zero = integer.is_empty() && fraction.is_empty();
        Some(Self {
            negative: negative && !zero,
            integer,
            fraction,
        })
    }

    /// The sign, the integer digits without leading zeroes and the fraction digits without
    /// trailing zeroes.
    pub(crate) fn parts(&self) -> (bool, &str, &str) {
        (self.negative, &self.integer, &self.fraction)
    }

    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        self.integer
            .len()
            .cmp(&other.integer.len())
            .then_with(|| self.integer.cmp(&other.integer))
            .then_with(|| self.fraction.cmp(&other.fraction))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.magnitude_cmp(other),
            (true, true) => other.magnitude_cmp(self),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::parse(text).expect("a number")
    }

    #[test]
    fn accepts_plain_decimals_only() {
        for text in [
            "5",
            "-5",
            "+5",
            "0.25",
            ".5",
            "7.",
            " 3.14 ",
            "007",
            "-0",
            "1234567890123456789012345678901234567890.1",
        ] {
            assert!(Decimal::parse(text).is_some(), "{text}");
        }
        for text in [
            "", "-", ".", "1e3", "1,000", "<5", "5 mmol", "--1", "1.2.3", "0x10", "NaN", "٣",
        ] {
            assert!(Decimal::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn compares_exactly() {
        assert!(d("5.5") > d("5.49999999999999999999"));
        assert_eq!(d("0.1"), d("0.10"));
        assert_eq!(d("-0"), d("0"));
        assert_eq!(d("007.50"), d("7.5"));
        assert!(d("-1") < d("0"));
        assert!(d("-2") < d("-1.5"));
        assert!(d("10") > d("9.99"));
        assert!(d("12345678901234567890.0000001") > d("12345678901234567890"));
        assert!(d("5") < d("5.0000000001"));
    }

    fn scaled(n: i32, zeros: usize) -> String {
        let sign = if n < 0 { "-" } else { "" };
        let a = n.unsigned_abs();
        format!("{sign}{}.{:03}{}", a / 1000, a % 1000, "0".repeat(zeros))
    }

    proptest! {
        #[test]
        fn order_matches_integers(a in any::<i64>(), b in any::<i64>()) {
            prop_assert_eq!(d(&a.to_string()).cmp(&d(&b.to_string())), a.cmp(&b));
        }

        #[test]
        fn order_matches_scaled_decimals(a in any::<i32>(), b in any::<i32>(), za in 0usize..4, zb in 0usize..4) {
            prop_assert_eq!(d(&scaled(a, za)).cmp(&d(&scaled(b, zb))), a.cmp(&b));
        }

        #[test]
        fn parse_never_panics(text in "\\PC{0,30}") {
            let _ = Decimal::parse(&text);
        }
    }
}
