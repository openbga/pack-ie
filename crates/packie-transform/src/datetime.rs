//! Date and time reformatting for the `datetime` rule (configuration.md §8.2).
//!
//! Formats use the delimited-file column tokens: `yyyy`, `yy`, `MM`, `dd`, `HH`, `mm`,
//! `ss`, every token fixed-width and zero-padded, and literal separators. Any other
//! punctuation or space is a literal; a letter that is not a token must be quoted
//! (`'T'`, with `''` for a quote) so a typo such as `YYYY` is an error rather than a
//! silent literal. `yy` means 1970 to 2069.
//!
//! The input is parsed completely: a text that does not match the format, or names a
//! date that does not exist, is an error. The output needs every component its format
//! names; a missing one is an error (never a guessed zero). Seconds are the one
//! allowance in a time zone conversion: an input without seconds converts as `:00` and
//! stays without seconds.

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

use crate::error::Reason;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Year4,
    Year2,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Literal(String),
}

/// A parsed date/time format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DateFormat {
    tokens: Vec<Token>,
}

/// Why a format text is invalid, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormatError {
    pub(crate) position: usize,
    pub(crate) reason: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    year: Option<i16>,
    month: Option<i8>,
    day: Option<i8>,
    hour: Option<i8>,
    minute: Option<i8>,
    second: Option<i8>,
}

impl DateFormat {
    pub(crate) fn parse(text: &str) -> Result<Self, FormatError> {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut tokens: Vec<Token> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let (position, c) = chars[i];
            let run = chars[i..].iter().take_while(|(_, x)| *x == c).count();
            match c {
                'y' | 'M' | 'd' | 'H' | 'm' | 's' => {
                    tokens.push(token(c, run).ok_or(FormatError {
                        position,
                        reason: "unknown token; use yyyy, yy, MM, dd, HH, mm or ss",
                    })?);
                    i += run;
                }
                '\'' => i = quoted(&chars, i, &mut tokens)?,
                c if c.is_ascii_alphabetic() => {
                    return Err(FormatError {
                        position,
                        reason: "letters that are not tokens must be quoted, for example 'T'",
                    });
                }
                _ => {
                    tokens.push(Token::Literal(c.to_string()));
                    i += 1;
                }
            }
        }
        Self::checked(tokens, text.len())
    }

    fn checked(tokens: Vec<Token>, end: usize) -> Result<Self, FormatError> {
        let mut seen = Vec::new();
        for token in &tokens {
            let family = match token {
                Token::Year4 | Token::Year2 => 'y',
                Token::Month => 'M',
                Token::Day => 'd',
                Token::Hour => 'H',
                Token::Minute => 'm',
                Token::Second => 's',
                Token::Literal(_) => continue,
            };
            if seen.contains(&family) {
                return Err(FormatError {
                    position: end,
                    reason: "a component appears twice",
                });
            }
            seen.push(family);
        }
        if seen.is_empty() {
            return Err(FormatError {
                position: 0,
                reason: "the format has no date or time tokens",
            });
        }
        Ok(Self { tokens })
    }

    /// Reads `text` completely.
    pub(crate) fn read(&self, text: &str) -> Result<Parts, Reason> {
        let mut rest = text;
        let mut parts = Parts::default();
        for token in &self.tokens {
            if let Token::Literal(literal) = token {
                rest = rest
                    .strip_prefix(literal.as_str())
                    .ok_or(Reason::DatetimeInput)?;
                continue;
            }
            let width = if *token == Token::Year4 { 4 } else { 2 };
            let digits = rest.get(..width).ok_or(Reason::DatetimeInput)?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Reason::DatetimeInput);
            }
            let number: i16 = digits.parse().map_err(|_| Reason::DatetimeInput)?;
            rest = &rest[width..];
            parts.assign(token, number)?;
        }
        if !rest.is_empty() {
            return Err(Reason::DatetimeInput);
        }
        parts.validate()?;
        Ok(parts)
    }

    /// Writes `parts`, failing on the first component the format needs and `parts` lacks.
    pub(crate) fn render(&self, parts: &Parts) -> Result<String, Reason> {
        self.tokens
            .iter()
            .map(|token| {
                let (number, width) = match token {
                    Token::Literal(literal) => return Ok(literal.clone()),
                    Token::Year4 => (need(parts.year, "year")?, 4),
                    Token::Year2 => (need(parts.year, "year")?.rem_euclid(100), 2),
                    Token::Month => (need(parts.month, "month")?.into(), 2),
                    Token::Day => (need(parts.day, "day")?.into(), 2),
                    Token::Hour => (need(parts.hour, "hour")?.into(), 2),
                    Token::Minute => (need(parts.minute, "minute")?.into(), 2),
                    Token::Second => (need(parts.second, "second")?.into(), 2),
                };
                Ok(format!("{number:0width$}"))
            })
            .collect()
    }
}

fn token(c: char, run: usize) -> Option<Token> {
    match (c, run) {
        ('y', 4) => Some(Token::Year4),
        ('y', 2) => Some(Token::Year2),
        ('M', 2) => Some(Token::Month),
        ('d', 2) => Some(Token::Day),
        ('H', 2) => Some(Token::Hour),
        ('m', 2) => Some(Token::Minute),
        ('s', 2) => Some(Token::Second),
        _ => None,
    }
}

/// A quoted literal starting at `chars[start]`; returns the index after it.
fn quoted(
    chars: &[(usize, char)],
    start: usize,
    tokens: &mut Vec<Token>,
) -> Result<usize, FormatError> {
    let mut literal = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match (chars[i].1, chars.get(i + 1).map(|(_, c)| *c)) {
            ('\'', Some('\'')) => {
                literal.push('\'');
                i += 2;
            }
            ('\'', _) => {
                tokens.push(Token::Literal(literal));
                return Ok(i + 1);
            }
            (c, _) => {
                literal.push(c);
                i += 1;
            }
        }
    }
    Err(FormatError {
        position: chars[start].0,
        reason: "quote is not closed",
    })
}

fn need<T>(value: Option<T>, component: &'static str) -> Result<T, Reason> {
    value.ok_or(Reason::DatetimeMissing { component })
}

impl Parts {
    fn assign(&mut self, token: &Token, number: i16) -> Result<(), Reason> {
        let small = || i8::try_from(number).map_err(|_| Reason::DatetimeInput);
        match token {
            Token::Year4 => self.year = Some(number),
            Token::Year2 => {
                self.year = Some(if number < 70 {
                    2000 + number
                } else {
                    1900 + number
                });
            }
            Token::Month => self.month = Some(small()?),
            Token::Day => self.day = Some(small()?),
            Token::Hour => self.hour = Some(small()?),
            Token::Minute => self.minute = Some(small()?),
            Token::Second => self.second = Some(small()?),
            Token::Literal(_) => {}
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), Reason> {
        let in_range = |value: Option<i8>, range: std::ops::RangeInclusive<i8>| {
            value.is_none_or(|v| range.contains(&v))
        };
        if !(in_range(self.month, 1..=12)
            && in_range(self.day, 1..=31)
            && in_range(self.hour, 0..=23)
            && in_range(self.minute, 0..=59)
            && in_range(self.second, 0..=59))
        {
            return Err(Reason::DatetimeInput);
        }
        if let (Some(y), Some(m), Some(d)) = (self.year, self.month, self.day) {
            Date::new(y, m, d).map_err(|_| Reason::DatetimeInput)?;
        }
        Ok(())
    }

    /// The same instant on the wall clock of `to`, read as local time in `from`.
    fn convert(&self, from: &TimeZone, to: &TimeZone) -> Result<Self, Reason> {
        let year = need(self.year, "year")?;
        let month = need(self.month, "month")?;
        let day = need(self.day, "day")?;
        let hour = need(self.hour, "hour")?;
        let minute = need(self.minute, "minute")?;
        let local = DateTime::new(year, month, day, hour, minute, self.second.unwrap_or(0), 0)
            .map_err(|_| Reason::DatetimeZone)?;
        let moved = from
            .to_ambiguous_zoned(local)
            .compatible()
            .map_err(|_| Reason::DatetimeZone)?
            .with_time_zone(to.clone())
            .datetime();
        Ok(Self {
            year: Some(moved.year()),
            month: Some(moved.month()),
            day: Some(moved.day()),
            hour: Some(moved.hour()),
            minute: Some(moved.minute()),
            second: self.second.map(|_| moved.second()),
        })
    }
}

/// A compiled `datetime` rule.
#[derive(Debug, Clone)]
pub(crate) struct Reformat {
    pub(crate) input: DateFormat,
    pub(crate) output: DateFormat,
    pub(crate) from_zone: Option<TimeZone>,
    pub(crate) to_zone: Option<TimeZone>,
}

impl Reformat {
    pub(crate) fn apply(&self, text: &str) -> Result<String, Reason> {
        let parts = self.input.read(text)?;
        let parts = match (&self.from_zone, &self.to_zone) {
            (Some(from), Some(to)) => parts.convert(from, to)?,
            _ => parts,
        };
        self.output.render(&parts)
    }
}

/// The IANA zone named `name`, if the time zone database knows it.
pub(crate) fn zone(name: &str) -> Option<TimeZone> {
    TimeZone::get(name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(input: &str, output: &str, from: Option<&str>, to: Option<&str>) -> Reformat {
        Reformat {
            input: DateFormat::parse(input).expect("input format"),
            output: DateFormat::parse(output).expect("output format"),
            from_zone: from.map(|z| zone(z).expect("zone")),
            to_zone: to.map(|z| zone(z).expect("zone")),
        }
    }

    #[test]
    fn reformats_between_layouts() {
        let r = rule("yyyyMMddHHmmss", "yyyy-MM-dd'T'HH:mm:ss", None, None);
        assert_eq!(
            r.apply("20261012115959").as_deref(),
            Ok("2026-10-12T11:59:59")
        );
        let r = rule("dd/MM/yy HH:mm", "yyyyMMddHHmm", None, None);
        assert_eq!(r.apply("05/03/69 07:08").as_deref(), Ok("206903050708"));
        assert_eq!(r.apply("05/03/70 07:08").as_deref(), Ok("197003050708"));
        let r = rule("yyyyMMddHHmmss", "yyyyMMdd", None, None);
        assert_eq!(r.apply("20261012115959").as_deref(), Ok("20261012"));
        let r = rule("yyyyMMdd", "yy.MM.dd", None, None);
        assert_eq!(r.apply("20261012").as_deref(), Ok("26.10.12"));
    }

    #[test]
    fn input_must_match_exactly() {
        let r = rule("yyyyMMdd", "yyyyMMdd", None, None);
        for bad in [
            "",
            "2026101",
            "202610123",
            "2026-10-12",
            "2026132x",
            "20261301",
            "20260230",
            "20260100",
        ] {
            assert_eq!(r.apply(bad), Err(Reason::DatetimeInput), "{bad}");
        }
        assert_eq!(r.apply("20240229").as_deref(), Ok("20240229"));
        assert_eq!(r.apply("20250229"), Err(Reason::DatetimeInput));
        let t = rule("HHmm", "HH:mm", None, None);
        assert_eq!(t.apply("2460"), Err(Reason::DatetimeInput));
        assert_eq!(t.apply("2359").as_deref(), Ok("23:59"));
    }

    #[test]
    fn output_components_missing_from_the_input_are_errors() {
        let r = rule("yyyyMMdd", "yyyyMMddHHmmss", None, None);
        assert_eq!(
            r.apply("20261012"),
            Err(Reason::DatetimeMissing { component: "hour" })
        );
        let r = rule("yyyyMMddHHmm", "yyyyMMddHHmmss", None, None);
        assert_eq!(
            r.apply("202610121159"),
            Err(Reason::DatetimeMissing {
                component: "second"
            })
        );
        let r = rule("HHmm", "yyyyMMdd", None, None);
        assert_eq!(
            r.apply("1159"),
            Err(Reason::DatetimeMissing { component: "year" })
        );
    }

    #[test]
    fn converts_between_time_zones() {
        let r = rule(
            "yyyyMMddHHmmss",
            "yyyyMMddHHmmss",
            Some("America/New_York"),
            Some("UTC"),
        );
        assert_eq!(r.apply("20261012080000").as_deref(), Ok("20261012120000"));
        assert_eq!(r.apply("20261212080000").as_deref(), Ok("20261212130000"));
        let r = rule(
            "yyyyMMddHHmm",
            "yyyyMMddHHmm",
            Some("UTC"),
            Some("Europe/Berlin"),
        );
        assert_eq!(r.apply("202606302300").as_deref(), Ok("202607010100"));
        // a day boundary and a gap/overlap are resolved, not rejected
        let gap = rule(
            "yyyyMMddHHmm",
            "yyyyMMddHHmm",
            Some("America/New_York"),
            Some("UTC"),
        );
        assert_eq!(gap.apply("202603080230").as_deref(), Ok("202603080730"));
        assert_eq!(gap.apply("202611010130").as_deref(), Ok("202611010530"));
    }

    #[test]
    fn one_zone_alone_does_not_convert_and_conversion_needs_a_full_time() {
        let r = rule("yyyyMMddHHmm", "yyyyMMddHHmm", Some("UTC"), None);
        assert_eq!(r.apply("202610121159").as_deref(), Ok("202610121159"));
        let r = rule("yyyyMMdd", "yyyyMMdd", Some("UTC"), Some("Asia/Tokyo"));
        assert_eq!(
            r.apply("20261012"),
            Err(Reason::DatetimeMissing { component: "hour" })
        );
    }

    #[test]
    fn bad_formats_are_located() {
        let position = |text: &str| DateFormat::parse(text).expect_err("invalid").position;
        assert_eq!(position("yyyyMMddHHmmsss"), 12);
        assert_eq!(position("YYYY"), 0);
        assert_eq!(position("yyyy-M"), 5);
        assert_eq!(position("yyyyT"), 4);
        assert_eq!(position("yyyy'T"), 4);
        assert_eq!(position("yyyyyyyy"), 0);
        assert_eq!(position("yyyyyy"), 0);
        assert_eq!(position("-/-"), 0);
        assert_eq!(position("yyyy yyyy"), 9);
        assert_eq!(position(""), 0);
        assert!(DateFormat::parse("yyyy'T''x'HH").is_ok());
    }

    #[test]
    fn unknown_zone_names_are_not_zones() {
        assert!(zone("Mars/Olympus_Mons").is_none());
        assert!(zone("UTC").is_some());
    }
}
