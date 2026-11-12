use crate::{AuError, Demographics, ResultKind, SampleHeader};

fn ensure(ok: bool) -> Result<(), AuError> {
    if ok { Ok(()) } else { Err(AuError::Field) }
}

fn is_digits(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_digit)
}

fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == b' ')
}

/// Leading character of a patient sample number: a digit, `E` or `P`.
fn is_patient_lead(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'E' | b'P')
}

/// Four bytes: a leading character accepted by `lead`, then three digits.
fn is_number(bytes: &[u8], lead: impl Fn(u8) -> bool) -> bool {
    matches!(bytes, [first, rest @ ..] if rest.len() == 3 && lead(*first) && is_digits(rest))
}

/// Validates a parsed header; `kind` is `None` for instrument queries.
pub(crate) fn validate_header(
    header: &SampleHeader,
    kind: Option<ResultKind>,
) -> Result<(), AuError> {
    let sample = header.sample_no.expose().as_slice();
    ensure(
        kind == Some(ResultKind::QualityControl)
            || matches!(header.sample_type.expose().as_slice(), b" " | b"U" | b"X"),
    )?;
    ensure(
        is_number(sample, |b| {
            matches!(b, b'0'..=b'9' | b'E' | b'P' | b'R' | b'A' | b'Q')
        }) && sample != b"0000"
            && (sample[0].is_ascii_digit() || sample[1..] != *b"000"),
    )?;
    for field in [&header.rack, &header.cup] {
        ensure(is_digits(field.expose()) || is_blank(field.expose()))?;
    }
    let reserved = header.reserved.expose().as_slice();
    match kind {
        Some(ResultKind::ReagentBlank) => {
            ensure(reserved == b"    " && is_blank(header.sample_id.expose()) && sample[0] == b'R')
        }
        Some(kind @ (ResultKind::Calibration | ResultKind::QualityControl)) => {
            let [b' ', b' ', tens @ b'0'..=b'9', ones @ b'0'..=b'9'] = reserved else {
                return Err(AuError::Field);
            };
            let calibration = kind == ResultKind::Calibration;
            ensure(sample[0] == if calibration { b'A' } else { b'Q' })?;
            let number = (tens - b'0') * 10 + ones - b'0';
            ensure(!calibration || (1..=80).contains(&number))
        }
        Some(ResultKind::Normal | ResultKind::Stat) => {
            validate_patient(header)?;
            ensure(reserved == b"    ")
        }
        Some(ResultKind::Rerun | ResultKind::StatRerun) => {
            validate_patient(header)?;
            ensure(is_number(reserved, is_patient_lead))
        }
        None => validate_patient(header),
    }
}

/// Checks that apply to patient samples: sample number class and demographics.
fn validate_patient(header: &SampleHeader) -> Result<(), AuError> {
    ensure(is_patient_lead(header.sample_no.expose()[0]))?;
    header
        .demographics
        .as_ref()
        .map_or(Ok(()), validate_demographics)
}

fn validate_demographics(fields: &Demographics) -> Result<(), AuError> {
    let sex = fields.sex.expose().as_slice();
    ensure(sex.is_empty() || matches!(sex, b"M" | b"F" | b" " | b"0"))?;
    for (field, maximum) in [(&fields.age, 150), (&fields.month, 11)] {
        let bytes = field.expose();
        if is_blank(bytes) {
            continue;
        }
        ensure(is_digits(bytes))?;
        let value = bytes
            .iter()
            .fold(0u16, |value, byte| value * 10 + u16::from(byte - b'0'));
        ensure(value <= maximum)?;
    }
    Ok(())
}
